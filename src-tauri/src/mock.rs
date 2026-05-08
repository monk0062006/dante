use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tiny_http::{Header, Response, Server};

#[derive(Clone)]
struct MockRoute {
    method: String,
    path: String,
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub struct MockServerHandle {
    pub port: u16,
    stop_flag: Arc<AtomicBool>,
}

impl MockServerHandle {
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

#[derive(Deserialize)]
struct HistoryEntry {
    response: HistoryResponse,
}

#[derive(Deserialize)]
struct HistoryResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn quick_parse(content: &str) -> Option<(String, String)> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let method = parts.next()?.to_uppercase();
        let url = parts.next()?.trim().to_string();
        if !matches!(
            method.as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
        ) {
            return None;
        }
        let path = if let Ok(parsed) = url::Url::parse(&url) {
            let mut p = parsed.path().to_string();
            if let Some(q) = parsed.query() {
                p.push('?');
                p.push_str(q);
            }
            p
        } else if url.starts_with('/') {
            url
        } else {
            return None;
        };
        return Some((method, path));
    }
    None
}

fn collect_routes(folder: &Path, out: &mut Vec<MockRoute>) {
    let dir = match fs::read_dir(folder) {
        Ok(d) => d,
        Err(_) => return,
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_routes(&path, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("http") {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let (method, route_path) = match quick_parse(&content) {
            Some(p) => p,
            None => continue,
        };

        let mut history_path = path.clone().into_os_string();
        history_path.push(".history.jsonl");
        let history_path = std::path::PathBuf::from(history_path);
        let mut latest_status: u16 = 200;
        let mut latest_headers: Vec<(String, String)> = vec![];
        let mut latest_body: Vec<u8> = vec![];
        let mut found_history = false;
        if history_path.exists() {
            if let Ok(file) = fs::File::open(&history_path) {
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
                        latest_status = entry.response.status;
                        latest_headers = entry.response.headers;
                        latest_body = entry.response.body.into_bytes();
                        found_history = true;
                    }
                }
            }
        }

        if !found_history {
            latest_status = 200;
            latest_headers = vec![("Content-Type".to_string(), "text/plain".to_string())];
            latest_body = format!("(no recorded response for {} {})", method, route_path)
                .into_bytes();
        }

        out.push(MockRoute {
            method,
            path: route_path,
            status: latest_status,
            headers: latest_headers,
            body: latest_body,
        });
    }
}

pub fn start(folder: &Path, port: u16) -> Result<MockServerHandle, String> {
    let mut routes: Vec<MockRoute> = vec![];
    collect_routes(folder, &mut routes);

    let by_key: Arc<HashMap<String, MockRoute>> = Arc::new(
        routes
            .into_iter()
            .map(|r| {
                let key = format!("{} {}", r.method, normalize_path(&r.path));
                (key, r)
            })
            .collect(),
    );

    let addr = format!("127.0.0.1:{port}");
    let server = Server::http(&addr).map_err(|e| format!("listen: {e}"))?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    thread::spawn(move || {
        loop {
            if stop_clone.load(Ordering::SeqCst) {
                break;
            }
            match server.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(Some(req)) => {
                    let method = req.method().as_str().to_uppercase();
                    let url = req.url().to_string();
                    let key = format!("{method} {}", normalize_path(&url));
                    if let Some(route) = by_key.get(&key) {
                        let mut resp = Response::from_data(route.body.clone())
                            .with_status_code(route.status as i32);
                        for (k, v) in &route.headers {
                            if let Ok(h) = Header::from_bytes(k.as_bytes(), v.as_bytes()) {
                                resp = resp.with_header(h);
                            }
                        }
                        let _ = req.respond(resp);
                    } else {
                        let body =
                            format!("dante mock: no route for {method} {url}").into_bytes();
                        let _ = req.respond(Response::from_data(body).with_status_code(404));
                    }
                }
                Ok(None) => continue,
                Err(_) => break,
            }
        }
    });

    Ok(MockServerHandle { port, stop_flag })
}

fn normalize_path(path: &str) -> String {
    let mut p = path.split('?').next().unwrap_or(path).trim().to_string();
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    if p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}
