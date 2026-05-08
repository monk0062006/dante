use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct RequestSpec {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
}

#[derive(Debug)]
struct ResponseInfo {
    status: u16,
    elapsed_ms: u64,
    body_len: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Reporter {
    Stdout,
    Json,
    Junit,
}

fn main() -> ExitCode {
    let raw: Vec<String> = env::args().skip(1).collect();
    if raw.is_empty() || raw.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("dante-run — execute Dante .http files");
        eprintln!();
        eprintln!("usage:  dante-run [--reporter stdout|json|junit] <file-or-folder> [more...]");
        eprintln!();
        eprintln!("  --reporter stdout  human-readable output (default)");
        eprintln!("  --reporter json    JSON array of {{file, method, status, elapsed_ms, ok, error?}}");
        eprintln!("  --reporter junit   JUnit XML for CI consumption");
        eprintln!();
        eprintln!("  Exits 0 if every request returned 2xx, 1 otherwise.");
        return ExitCode::from(if raw.is_empty() { 2 } else { 0 });
    }

    let mut reporter = Reporter::Stdout;
    let mut args: Vec<String> = vec![];
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--reporter" | "-r" => {
                i += 1;
                if i >= raw.len() {
                    eprintln!("--reporter needs a value");
                    return ExitCode::from(2);
                }
                reporter = match raw[i].as_str() {
                    "stdout" | "text" => Reporter::Stdout,
                    "json" => Reporter::Json,
                    "junit" => Reporter::Junit,
                    other => {
                        eprintln!("unknown reporter: {other}");
                        return ExitCode::from(2);
                    }
                };
            }
            _ => args.push(raw[i].clone()),
        }
        i += 1;
    }
    if args.is_empty() {
        eprintln!("no path provided");
        return ExitCode::from(2);
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };

    let mut all_files: Vec<PathBuf> = vec![];
    for arg in &args {
        let p = PathBuf::from(arg);
        if !p.exists() {
            eprintln!("✗ {}: path does not exist", p.display());
            return ExitCode::from(2);
        }
        if p.is_dir() {
            collect_http_files(&p, &mut all_files);
        } else if p.extension().and_then(|s| s.to_str()) == Some("http") {
            all_files.push(p);
        } else {
            eprintln!("⚠ {}: not a .http file, skipping", p.display());
        }
    }

    if all_files.is_empty() {
        eprintln!("no .http files found");
        return ExitCode::from(2);
    }

    struct RunRow {
        file: String,
        method: String,
        url: String,
        status: Option<u16>,
        elapsed_ms: Option<u64>,
        body_len: Option<usize>,
        ok: bool,
        error: Option<String>,
    }

    let total = all_files.len();
    let mut rows: Vec<RunRow> = vec![];

    for file in &all_files {
        match runtime.block_on(run_one(file)) {
            Ok((spec, resp)) => {
                let ok = (200..300).contains(&resp.status);
                rows.push(RunRow {
                    file: short_path(file),
                    method: spec.method,
                    url: spec.url,
                    status: Some(resp.status),
                    elapsed_ms: Some(resp.elapsed_ms),
                    body_len: Some(resp.body_len),
                    ok,
                    error: None,
                });
            }
            Err(e) => {
                rows.push(RunRow {
                    file: short_path(file),
                    method: "?".to_string(),
                    url: String::new(),
                    status: None,
                    elapsed_ms: None,
                    body_len: None,
                    ok: false,
                    error: Some(e),
                });
            }
        }
    }

    let passed = rows.iter().filter(|r| r.ok).count();
    let failed = total - passed;

    match reporter {
        Reporter::Stdout => {
            for r in &rows {
                if let Some(err) = &r.error {
                    println!("✗ {}: {}", r.file, err);
                } else {
                    let sigil = if r.ok { "✓" } else { "✗" };
                    println!(
                        "{} {} {} → {} ({} ms, {} bytes)",
                        sigil,
                        r.file,
                        r.method,
                        r.status.unwrap_or(0),
                        r.elapsed_ms.unwrap_or(0),
                        r.body_len.unwrap_or(0),
                    );
                }
            }
            println!();
            println!("{} run, {} passed, {} failed", total, passed, failed);
        }
        Reporter::Json => {
            print!("[");
            for (i, r) in rows.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!(
                    "{{\"file\":{},\"method\":{},\"url\":{},\"status\":{},\"elapsed_ms\":{},\"body_len\":{},\"ok\":{}",
                    json_str(&r.file),
                    json_str(&r.method),
                    json_str(&r.url),
                    r.status.map(|s| s.to_string()).unwrap_or_else(|| "null".to_string()),
                    r.elapsed_ms.map(|e| e.to_string()).unwrap_or_else(|| "null".to_string()),
                    r.body_len.map(|b| b.to_string()).unwrap_or_else(|| "null".to_string()),
                    r.ok,
                );
                if let Some(err) = &r.error {
                    print!(",\"error\":{}", json_str(err));
                }
                print!("}}");
            }
            println!("]");
        }
        Reporter::Junit => {
            let total_time: u64 = rows.iter().map(|r| r.elapsed_ms.unwrap_or(0)).sum();
            println!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
            println!(
                "<testsuites name=\"dante-run\" tests=\"{}\" failures=\"{}\" time=\"{:.3}\">",
                total,
                failed,
                total_time as f64 / 1000.0
            );
            println!(
                "  <testsuite name=\"dante\" tests=\"{}\" failures=\"{}\" time=\"{:.3}\">",
                total,
                failed,
                total_time as f64 / 1000.0
            );
            for r in &rows {
                let elapsed = r.elapsed_ms.unwrap_or(0) as f64 / 1000.0;
                let name = xml_escape(&format!("{} {}", r.method, r.file));
                println!(
                    "    <testcase name=\"{}\" classname=\"dante\" time=\"{:.3}\">",
                    name, elapsed
                );
                if !r.ok {
                    let msg = match (&r.error, r.status) {
                        (Some(e), _) => e.clone(),
                        (None, Some(s)) => format!("HTTP {s}"),
                        _ => "failed".to_string(),
                    };
                    println!(
                        "      <failure message=\"{}\">{}</failure>",
                        xml_escape(&msg),
                        xml_escape(&msg)
                    );
                }
                println!("    </testcase>");
            }
            println!("  </testsuite>");
            println!("</testsuites>");
        }
    }

    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

async fn run_one(path: &Path) -> Result<(RequestSpec, ResponseInfo), String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let spec = parse_http(&content).ok_or_else(|| "no parseable request".to_string())?;
    let resp = execute(&spec).await?;
    Ok((spec, resp))
}

async fn execute(spec: &RequestSpec) -> Result<ResponseInfo, String> {
    let method = reqwest::Method::from_bytes(spec.method.to_uppercase().as_bytes())
        .map_err(|e| format!("invalid method: {e}"))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.request(method, &spec.url);
    for (k, v) in &spec.headers {
        req = req.header(k, v);
    }
    if let Some(body) = &spec.body {
        req = req.body(body.clone());
    }
    let started = Instant::now();
    let resp = req.send().await.map_err(|e| format!("send: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.bytes().await.map_err(|e| format!("body: {e}"))?;
    Ok(ResponseInfo {
        status,
        elapsed_ms: started.elapsed().as_millis() as u64,
        body_len: body.len(),
    })
}

fn parse_http(text: &str) -> Option<RequestSpec> {
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
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) {
        return None;
    }
    let url = split.next()?.trim().to_string();
    i += 1;

    let mut headers: Vec<(String, String)> = vec![];
    while i < lines.len() {
        let line = lines[i].trim_end_matches('\r');
        if line.trim().is_empty() {
            break;
        }
        if line.trim().starts_with("###") {
            break;
        }
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim().to_string();
            let val = line[idx + 1..].trim().to_string();
            if !key.is_empty() {
                headers.push((key, val));
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

    Some(RequestSpec {
        method,
        url,
        headers,
        body,
    })
}

fn collect_http_files(folder: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(folder) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            collect_http_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("http") {
            out.push(path);
        }
    }
}

fn short_path(p: &Path) -> String {
    if let Ok(rel) = p.strip_prefix(env::current_dir().unwrap_or_default()) {
        return rel.to_string_lossy().to_string();
    }
    p.to_string_lossy().to_string()
}
