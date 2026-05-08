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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn make_temp_dir(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let unique = format!(
            "dante-mock-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        p.push(unique);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn http_get(port: u16, path: &str) -> (u16, std::collections::HashMap<String, String>, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        let split = text.find("\r\n\r\n").expect("response has body separator");
        let head = &text[..split];
        let body = text[split + 4..].to_string();
        let mut lines = head.split("\r\n");
        let status_line = lines.next().unwrap();
        let status: u16 = status_line.split(' ').nth(1).unwrap().parse().unwrap();
        let mut headers = std::collections::HashMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
        (status, headers, body)
    }

    #[test]
    fn serves_recorded_response_from_history() {
        let dir = make_temp_dir("history");
        std::fs::write(
            dir.join("get-users.http"),
            "### List users\nGET https://api.example.com/api/users\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("get-users.http.history.jsonl"),
            r#"{"response":{"status":201,"headers":[["X-Mocked","yes"],["Content-Type","application/json"]],"body":"[{\"id\":1,\"name\":\"alice\"}]"}}
"#,
        )
        .unwrap();

        let port = free_port();
        let handle = start(&dir, port).expect("server should start");
        // Allow server thread to enter recv loop
        std::thread::sleep(Duration::from_millis(100));

        let (status, headers, body) = http_get(port, "/api/users");
        assert_eq!(status, 201);
        assert_eq!(headers.get("x-mocked").map(String::as_str), Some("yes"));
        assert_eq!(body, r#"[{"id":1,"name":"alice"}]"#);

        handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn returns_404_for_unregistered_path() {
        let dir = make_temp_dir("404");
        std::fs::write(
            dir.join("known.http"),
            "GET https://api.example.com/known\n",
        )
        .unwrap();

        let port = free_port();
        let handle = start(&dir, port).expect("server should start");
        std::thread::sleep(Duration::from_millis(100));

        let (status, _, body) = http_get(port, "/unknown-path");
        assert_eq!(status, 404);
        assert!(body.contains("no route for GET"));

        handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn placeholder_body_when_no_history() {
        let dir = make_temp_dir("placeholder");
        std::fs::write(
            dir.join("ping.http"),
            "GET https://api.example.com/ping\n",
        )
        .unwrap();

        let port = free_port();
        let handle = start(&dir, port).expect("server should start");
        std::thread::sleep(Duration::from_millis(100));

        let (status, _, body) = http_get(port, "/ping");
        assert_eq!(status, 200);
        assert!(body.contains("no recorded response"));

        handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quick_parse_extracts_method_and_path() {
        assert_eq!(
            quick_parse("GET https://api.example.com/users\nAccept: */*\n"),
            Some(("GET".to_string(), "/users".to_string()))
        );
        assert_eq!(
            quick_parse("# header comment\n\nPOST /local/path\n"),
            Some(("POST".to_string(), "/local/path".to_string()))
        );
        assert_eq!(
            quick_parse("INVALID-METHOD https://api.example.com/users\n"),
            None
        );
        assert_eq!(quick_parse("# only comments\n# more\n"), None);
    }

    #[test]
    fn normalize_path_handles_query_and_trailing_slash() {
        assert_eq!(normalize_path("/api/users?id=1"), "/api/users");
        assert_eq!(normalize_path("/api/users/"), "/api/users");
        assert_eq!(normalize_path("api/users"), "/api/users");
        assert_eq!(normalize_path("/"), "/");
    }
}
