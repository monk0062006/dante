use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::time::sleep;

#[derive(Clone, Serialize)]
pub struct MonitorEvent {
    pub path: String,
    pub ts_ms: u64,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u64>,
    pub ok: bool,
    pub error: Option<String>,
}

pub struct MonitorHandle {
    pub interval_secs: u64,
    pub stop: Arc<std::sync::atomic::AtomicBool>,
}

pub type MonitorRegistry = Arc<Mutex<HashMap<String, MonitorHandle>>>;

pub fn new_registry() -> MonitorRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Parse the `### schedule` block. Supports `every: 30s|5m|1h` (interval) only for v0.
pub fn parse_schedule(content: &str) -> Option<u64> {
    let mut in_block = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if regex_lite::matches_schedule_header(trimmed) {
            in_block = true;
            continue;
        }
        if trimmed.starts_with("###") {
            in_block = false;
            continue;
        }
        if !in_block {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("every:") {
            return parse_duration(rest.trim());
        }
        if let Some(rest) = trimmed.strip_prefix("every ") {
            return parse_duration(rest.trim());
        }
    }
    None
}

fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_part, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = num_part.parse().ok()?;
    let mult: u64 = match unit.trim() {
        "s" | "sec" | "secs" | "" => 1,
        "m" | "min" | "mins" => 60,
        "h" | "hr" | "hrs" => 3600,
        "d" | "day" | "days" => 86400,
        _ => return None,
    };
    let total = n.saturating_mul(mult);
    if total < 5 {
        // Sanity floor: 5s minimum
        return Some(5);
    }
    Some(total)
}

mod regex_lite {
    pub fn matches_schedule_header(line: &str) -> bool {
        let lower = line.to_lowercase();
        let after_hashes = lower.trim_start_matches('#').trim();
        after_hashes == "schedule" || after_hashes.starts_with("schedule ")
    }
}

fn monitor_log_path(request_path: &str) -> PathBuf {
    let p = PathBuf::from(request_path);
    let mut name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    name.push_str(".monitor.jsonl");
    p.with_file_name(name)
}

pub fn append_monitor_log(request_path: &str, event: &MonitorEvent) -> std::io::Result<()> {
    let path = monitor_log_path(request_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    writeln!(file, "{line}")
}

async fn run_one(request_path: &Path) -> MonitorEvent {
    let path_str = request_path.to_string_lossy().to_string();
    let content = match fs::read_to_string(request_path) {
        Ok(c) => c,
        Err(e) => {
            return MonitorEvent {
                path: path_str,
                ts_ms: now_ms(),
                status: None,
                elapsed_ms: None,
                ok: false,
                error: Some(format!("read: {e}")),
            }
        }
    };
    let (method, url, headers, body) = match parse_http(&content) {
        Some(t) => t,
        None => {
            return MonitorEvent {
                path: path_str,
                ts_ms: now_ms(),
                status: None,
                elapsed_ms: None,
                ok: false,
                error: Some("could not parse".to_string()),
            }
        }
    };
    let started = std::time::Instant::now();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return MonitorEvent {
                path: path_str,
                ts_ms: now_ms(),
                status: None,
                elapsed_ms: None,
                ok: false,
                error: Some(format!("client: {e}")),
            }
        }
    };
    let m = match reqwest::Method::from_bytes(method.to_uppercase().as_bytes()) {
        Ok(m) => m,
        Err(_) => {
            return MonitorEvent {
                path: path_str,
                ts_ms: now_ms(),
                status: None,
                elapsed_ms: None,
                ok: false,
                error: Some(format!("bad method: {method}")),
            }
        }
    };
    let mut req = client.request(m, &url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    if let Some(b) = body {
        req = req.body(b);
    }
    let resp = req.send().await;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            let ok = (200..300).contains(&status);
            MonitorEvent {
                path: path_str,
                ts_ms: now_ms(),
                status: Some(status),
                elapsed_ms: Some(elapsed_ms),
                ok,
                error: None,
            }
        }
        Err(e) => MonitorEvent {
            path: path_str,
            ts_ms: now_ms(),
            status: None,
            elapsed_ms: Some(elapsed_ms),
            ok: false,
            error: Some(e.to_string()),
        },
    }
}

fn parse_http(text: &str) -> Option<(String, String, Vec<(String, String)>, Option<String>)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.starts_with("###")) {
            i += 1;
            continue;
        }
        if trimmed.starts_with("###") {
            return None;
        }
        break;
    }
    if i >= lines.len() {
        return None;
    }
    let req_line = lines[i].trim();
    let mut split = req_line.splitn(2, char::is_whitespace);
    let method = split.next()?.to_uppercase();
    let url = split.next()?.trim().to_string();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) {
        return None;
    }
    i += 1;
    let mut headers: Vec<(String, String)> = vec![];
    while i < lines.len() {
        let l = lines[i].trim_end_matches('\r');
        if l.trim().is_empty() || l.trim().starts_with("###") {
            break;
        }
        if let Some(idx) = l.find(':') {
            let k = l[..idx].trim().to_string();
            let v = l[idx + 1..].trim().to_string();
            if !k.is_empty() {
                headers.push((k, v));
            }
        } else {
            break;
        }
        i += 1;
    }
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    let mut body_lines: Vec<&str> = vec![];
    while i < lines.len() {
        if lines[i].trim().starts_with("###") {
            break;
        }
        body_lines.push(lines[i]);
        i += 1;
    }
    let body = body_lines.join("\n").trim().to_string();
    let body = if body.is_empty() { None } else { Some(body) };
    Some((method, url, headers, body))
}

pub fn start(
    registry: &MonitorRegistry,
    request_path: String,
    interval_secs: u64,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut map = registry.lock().map_err(|e| e.to_string())?;
        if let Some(handle) = map.remove(&request_path) {
            handle.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = stop.clone();
    let path_clone = request_path.clone();

    {
        let mut map = registry.lock().map_err(|e| e.to_string())?;
        map.insert(
            request_path.clone(),
            MonitorHandle {
                interval_secs,
                stop,
            },
        );
    }

    tauri::async_runtime::spawn(async move {
        let path = PathBuf::from(&path_clone);
        loop {
            if stop_clone.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            let event = run_one(&path).await;
            let _ = append_monitor_log(&path_clone, &event);
            let _ = app.emit("monitor", &event);
            // Sleep
            for _ in 0..(interval_secs.max(1)) {
                if stop_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                sleep(Duration::from_secs(1)).await;
            }
        }
    });
    Ok(())
}

pub fn stop(registry: &MonitorRegistry, request_path: &str) -> Result<(), String> {
    let mut map = registry.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = map.remove(request_path) {
        handle.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(())
}

pub fn list_active(registry: &MonitorRegistry) -> Vec<(String, u64)> {
    let map = match registry.lock() {
        Ok(m) => m,
        Err(_) => return vec![],
    };
    map.iter()
        .map(|(k, v)| (k.clone(), v.interval_secs))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30s"), Some(30));
        assert_eq!(parse_duration("5m"), Some(300));
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("1d"), Some(86400));
        assert_eq!(parse_duration("90"), Some(90));
        // bare numbers default to seconds
        assert_eq!(parse_duration("10mins"), Some(600));
    }

    #[test]
    fn parse_duration_floors_to_5_seconds() {
        assert_eq!(parse_duration("1s"), Some(5));
        assert_eq!(parse_duration("0s"), Some(5));
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("forever"), None);
        assert_eq!(parse_duration("5x"), None);
    }

    #[test]
    fn parse_schedule_finds_every_directive() {
        let content = r#"
GET https://api.example.com/health

### schedule
every: 60s
"#;
        assert_eq!(parse_schedule(content), Some(60));
    }

    #[test]
    fn parse_schedule_supports_space_form() {
        let content = "### schedule\nevery 5m\n";
        assert_eq!(parse_schedule(content), Some(300));
    }

    #[test]
    fn parse_schedule_returns_none_when_block_missing() {
        let content = "GET https://api.example.com/health\n";
        assert_eq!(parse_schedule(content), None);
    }

    #[test]
    fn parse_schedule_resets_block_on_next_section() {
        let content = "### schedule\n### other\nevery: 30s\n";
        assert_eq!(parse_schedule(content), None);
    }

    #[test]
    fn parse_http_extracts_method_url_headers_body() {
        let raw = "POST https://api.example.com/users\nContent-Type: application/json\nAuthorization: Bearer abc\n\n{\"name\":\"alice\"}\n";
        let (method, url, headers, body) = parse_http(raw).expect("should parse");
        assert_eq!(method, "POST");
        assert_eq!(url, "https://api.example.com/users");
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0], ("Content-Type".to_string(), "application/json".to_string()));
        assert_eq!(headers[1], ("Authorization".to_string(), "Bearer abc".to_string()));
        assert_eq!(body, Some("{\"name\":\"alice\"}".to_string()));
    }

    #[test]
    fn parse_http_skips_comments_and_blank_lines() {
        let raw = "# this is a comment\n\nGET https://api.example.com/x\n";
        let (method, url, _, body) = parse_http(raw).expect("should parse");
        assert_eq!(method, "GET");
        assert_eq!(url, "https://api.example.com/x");
        assert_eq!(body, None);
    }

    #[test]
    fn parse_http_rejects_invalid_method() {
        let raw = "FOOBAR https://api.example.com/x\n";
        assert!(parse_http(raw).is_none());
    }

    #[test]
    fn append_monitor_log_writes_jsonl() {
        let dir = std::env::temp_dir().join(format!(
            "dante-mon-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let req_path = dir.join("ping.http");
        fs::write(&req_path, "GET https://example.com/\n").unwrap();

        let event = MonitorEvent {
            path: req_path.to_string_lossy().to_string(),
            ts_ms: 1_700_000_000_000,
            status: Some(200),
            elapsed_ms: Some(123),
            ok: true,
            error: None,
        };
        append_monitor_log(&req_path.to_string_lossy(), &event).unwrap();
        append_monitor_log(&req_path.to_string_lossy(), &event).unwrap();

        let log_path = dir.join("ping.http.monitor.jsonl");
        assert!(log_path.exists(), "log should exist");
        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2);
        // Each line should parse as a MonitorEvent shape
        for line in lines {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v["status"], 200);
            assert_eq!(v["elapsed_ms"], 123);
            assert_eq!(v["ok"], true);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_one_against_unreachable_host_returns_error_event() {
        let dir = std::env::temp_dir().join(format!(
            "dante-mon-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let req_path = dir.join("bad.http");
        // 127.0.0.1:1 is reserved/unreachable in practice
        fs::write(&req_path, "GET http://127.0.0.1:1/\n").unwrap();

        let event = run_one(&req_path).await;
        assert!(!event.ok, "should not be ok");
        assert!(event.error.is_some(), "should have error message");
        // ts_ms should be populated
        assert!(event.ts_ms > 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
