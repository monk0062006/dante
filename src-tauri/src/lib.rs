mod ai;
mod mock;
mod monitors;
mod script;
mod sigv4;
mod ws;

use reqwest::Method;
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Manager;

use mock::MockServerHandle;
use sigv4::AwsParams;

pub struct AppState {
    pub cookie_jar: Arc<CookieStoreMutex>,
    pub mock_server: Mutex<Option<MockServerHandle>>,
    pub ws_connections: ws::WsConnections,
    pub monitors: monitors::MonitorRegistry,
}

#[derive(Deserialize)]
pub struct RequestSpec {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Serialize)]
pub struct ResponseData {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed_ms: u64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Settings {
    pub project_folder: Option<String>,
    #[serde(default)]
    pub active_env: Option<String>,
}

#[derive(Serialize)]
pub struct CookieView {
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
}

#[tauri::command]
fn list_cookies(state: tauri::State<'_, AppState>) -> Result<Vec<CookieView>, String> {
    let store = state.cookie_jar.lock().map_err(|e| e.to_string())?;
    let mut out: Vec<CookieView> = vec![];
    for cookie in store.iter_any() {
        out.push(CookieView {
            domain: cookie.domain().unwrap_or("").to_string(),
            name: cookie.name().to_string(),
            value: cookie.value().to_string(),
            path: cookie.path().unwrap_or("/").to_string(),
        });
    }
    Ok(out)
}

#[tauri::command]
fn clear_cookies(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut store = state.cookie_jar.lock().map_err(|e| e.to_string())?;
    store.clear();
    Ok(())
}

#[tauri::command]
fn delete_cookie(
    domain: String,
    path: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut store = state.cookie_jar.lock().map_err(|e| e.to_string())?;
    store.remove(&domain, &path, &name);
    Ok(())
}

fn cookies_path(folder: &str) -> PathBuf {
    PathBuf::from(folder).join(".cookies.json")
}

#[tauri::command]
fn load_cookies(folder: String, state: tauri::State<'_, AppState>) -> Result<usize, String> {
    let path = cookies_path(&folder);
    if !path.exists() {
        return Ok(0);
    }
    let file = fs::File::open(&path).map_err(|e| format!("open cookies: {e}"))?;
    let store = CookieStore::load_json(BufReader::new(file)).map_err(|e| e.to_string())?;
    let count = store.iter_any().count();
    let mut current = state.cookie_jar.lock().map_err(|e| e.to_string())?;
    *current = store;
    Ok(count)
}

#[derive(Serialize, Debug)]
pub struct ImportResult {
    pub created: Vec<String>,
    pub skipped: Vec<String>,
}

#[tauri::command]
fn export_markdown(folder: String) -> Result<String, String> {
    let root = PathBuf::from(&folder);
    if !root.exists() {
        return Err("folder does not exist".to_string());
    }
    let mut entries: Vec<RequestEntry> = vec![];
    collect_http_files(&root, &root, "", &mut entries, 0);
    entries.sort_by(|a, b| a.folder.cmp(&b.folder).then_with(|| a.name.cmp(&b.name)));

    let title = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("API")
        .to_string();

    let mut out = String::new();
    out.push_str(&format!("# {title}\n\n"));
    out.push_str(&format!("{} endpoints across {} folders.\n\n", entries.len(), {
        let mut s = std::collections::HashSet::new();
        for e in &entries {
            s.insert(e.folder.clone());
        }
        s.len()
    }));

    out.push_str("## Index\n\n");
    out.push_str("| Method | Endpoint | Folder |\n");
    out.push_str("| --- | --- | --- |\n");
    for e in &entries {
        out.push_str(&format!(
            "| `{}` | [{}](#{}) | {} |\n",
            e.method,
            e.name,
            slugify(&e.name),
            if e.folder.is_empty() { "—".to_string() } else { e.folder.clone() }
        ));
    }
    out.push_str("\n");

    let mut current_folder = String::from("\0");
    for e in &entries {
        if e.folder != current_folder {
            current_folder = e.folder.clone();
            out.push_str(&format!(
                "## {}\n\n",
                if current_folder.is_empty() {
                    "(root)"
                } else {
                    &current_folder
                }
            ));
        }

        let content = fs::read_to_string(&e.path).unwrap_or_default();
        out.push_str(&format!("### {}\n\n", e.name));
        if !e.description.is_empty() {
            out.push_str(&format!("{}\n\n", e.description));
        }
        out.push_str(&format!("**`{} {}`**\n\n", e.method, e.url));
        out.push_str("```http\n");
        out.push_str(content.trim());
        out.push_str("\n```\n\n");
    }

    let docs_path = root.join("README.md");
    fs::write(&docs_path, &out).map_err(|e| format!("write docs: {e}"))?;
    Ok(docs_path.to_string_lossy().to_string())
}

#[derive(Serialize)]
pub struct MockStatus {
    pub running: bool,
    pub port: Option<u16>,
}

#[tauri::command]
fn start_mock_server(
    folder: String,
    port: u16,
    state: tauri::State<'_, AppState>,
) -> Result<MockStatus, String> {
    let mut current = state
        .mock_server
        .lock()
        .map_err(|e| format!("lock: {e}"))?;
    if let Some(handle) = current.take() {
        handle.stop();
    }
    let handle = mock::start(&PathBuf::from(folder), port)?;
    let port = handle.port;
    *current = Some(handle);
    Ok(MockStatus {
        running: true,
        port: Some(port),
    })
}

#[tauri::command]
fn stop_mock_server(state: tauri::State<'_, AppState>) -> Result<MockStatus, String> {
    let mut current = state
        .mock_server
        .lock()
        .map_err(|e| format!("lock: {e}"))?;
    if let Some(handle) = current.take() {
        handle.stop();
    }
    Ok(MockStatus {
        running: false,
        port: None,
    })
}

#[tauri::command]
fn mock_server_status(state: tauri::State<'_, AppState>) -> Result<MockStatus, String> {
    let current = state
        .mock_server
        .lock()
        .map_err(|e| format!("lock: {e}"))?;
    Ok(MockStatus {
        running: current.is_some(),
        port: current.as_ref().map(|h| h.port),
    })
}

#[tauri::command]
fn export_postman(folder: String) -> Result<String, String> {
    let root = PathBuf::from(&folder);
    if !root.exists() {
        return Err("folder does not exist".to_string());
    }
    let mut entries: Vec<RequestEntry> = vec![];
    collect_http_files(&root, &root, "", &mut entries, 0);

    let title = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Dante")
        .to_string();

    let mut groups: std::collections::BTreeMap<String, Vec<&RequestEntry>> =
        std::collections::BTreeMap::new();
    for e in &entries {
        groups.entry(e.folder.clone()).or_default().push(e);
    }

    let mut items: Vec<serde_json::Value> = vec![];
    for (folder_name, group) in groups {
        let mut sub_items: Vec<serde_json::Value> = vec![];
        for e in group {
            let content = fs::read_to_string(&e.path).unwrap_or_default();
            let body_text = extract_body(&content);
            let mut header_objs: Vec<serde_json::Value> = vec![];
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with("#") || trimmed.starts_with("###") {
                    continue;
                }
                if let Some((method, _)) = trimmed.split_once(char::is_whitespace) {
                    if matches!(
                        method.to_uppercase().as_str(),
                        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
                    ) {
                        // skip the request line
                        continue;
                    }
                }
                if let Some(idx) = trimmed.find(':') {
                    let key = trimmed[..idx].trim();
                    let val = trimmed[idx + 1..].trim();
                    header_objs.push(serde_json::json!({"key": key, "value": val}));
                }
            }
            let mut request_obj = serde_json::json!({
                "method": e.method,
                "header": header_objs,
                "url": {
                    "raw": e.url,
                }
            });
            if let Some(body) = body_text {
                if let Some(map) = request_obj.as_object_mut() {
                    map.insert(
                        "body".to_string(),
                        serde_json::json!({
                            "mode": "raw",
                            "raw": body,
                        }),
                    );
                }
            }
            sub_items.push(serde_json::json!({
                "name": e.name,
                "request": request_obj,
            }));
        }
        if folder_name.is_empty() {
            items.extend(sub_items);
        } else {
            items.push(serde_json::json!({
                "name": folder_name,
                "item": sub_items,
            }));
        }
    }

    let collection = serde_json::json!({
        "info": {
            "name": title,
            "_postman_id": format!("dante-{}", title),
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
        },
        "item": items,
    });

    let json = serde_json::to_string_pretty(&collection).map_err(|e| e.to_string())?;
    let target = root.join("postman_collection.json");
    fs::write(&target, json).map_err(|e| format!("write: {e}"))?;
    Ok(target.to_string_lossy().to_string())
}

#[tauri::command]
fn export_openapi(folder: String) -> Result<String, String> {
    let root = PathBuf::from(&folder);
    if !root.exists() {
        return Err("folder does not exist".to_string());
    }
    let mut entries: Vec<RequestEntry> = vec![];
    collect_http_files(&root, &root, "", &mut entries, 0);
    entries.sort_by(|a, b| a.url.cmp(&b.url));

    let title = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("API")
        .to_string();

    let mut paths_map: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    let mut servers_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for e in &entries {
        let (server, path) = split_url(&e.url);
        if !server.is_empty() {
            servers_set.insert(server);
        }
        let path_obj = paths_map
            .entry(path.clone())
            .or_insert_with(|| serde_json::json!({}));
        let op = build_op_for_export(e);
        if let Some(map) = path_obj.as_object_mut() {
            map.insert(e.method.to_lowercase(), op);
        }
    }

    let openapi = serde_json::json!({
        "openapi": "3.0.3",
        "info": {
            "title": title,
            "version": "0.1.0",
        },
        "servers": servers_set
            .into_iter()
            .map(|s| serde_json::json!({"url": s}))
            .collect::<Vec<_>>(),
        "paths": paths_map,
    });

    let yaml = serde_yaml::to_string(&openapi).map_err(|e| format!("yaml: {e}"))?;
    let target = root.join("openapi.yaml");
    fs::write(&target, yaml).map_err(|e| format!("write: {e}"))?;
    Ok(target.to_string_lossy().to_string())
}

fn split_url(url: &str) -> (String, String) {
    if let Some(idx) = url.find("://") {
        let after = &url[idx + 3..];
        if let Some(slash) = after.find('/') {
            return (url[..idx + 3 + slash].to_string(), after[slash..].to_string());
        }
        return (url.to_string(), "/".to_string());
    }
    if url.starts_with('/') {
        return (String::new(), url.to_string());
    }
    (String::new(), format!("/{}", url))
}

fn build_op_for_export(entry: &RequestEntry) -> serde_json::Value {
    let content = fs::read_to_string(&entry.path).unwrap_or_default();
    let summary = entry.name.replace('-', " ");
    let mut op = serde_json::json!({
        "summary": summary,
        "operationId": entry.name,
        "tags": if entry.folder.is_empty() { Vec::<String>::new() } else { vec![entry.folder.clone()] },
        "responses": {
            "200": {"description": "ok"}
        }
    });

    let body_block = extract_body(&content);
    if let Some(body) = body_block {
        let example: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body.clone()));
        if let Some(map) = op.as_object_mut() {
            map.insert(
                "requestBody".to_string(),
                serde_json::json!({
                    "content": {
                        "application/json": {
                            "example": example,
                        }
                    }
                }),
            );
        }
    }
    op
}

fn extract_body(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() && (lines[i].trim().is_empty() || lines[i].starts_with('#')) {
        i += 1;
    }
    if i >= lines.len() {
        return None;
    }
    i += 1; // skip request line
    while i < lines.len() && !lines[i].trim().is_empty() && !lines[i].trim().starts_with("###") {
        i += 1;
    }
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    let mut body_lines: Vec<&str> = vec![];
    while i < lines.len() && !lines[i].trim().starts_with("###") {
        body_lines.push(lines[i]);
        i += 1;
    }
    let body = body_lines.join("\n").trim().to_string();
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

#[tauri::command]
fn import_har(folder: String, spec_path: String) -> Result<ImportResult, String> {
    let raw = fs::read_to_string(&spec_path).map_err(|e| format!("read har: {e}"))?;
    let har: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("har json: {e}"))?;
    let entries = har
        .pointer("/log/entries")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "HAR has no /log/entries".to_string())?;

    let root = PathBuf::from(folder).join("har-import");
    fs::create_dir_all(&root).map_err(|e| format!("mkdir: {e}"))?;

    let mut result = ImportResult {
        created: vec![],
        skipped: vec![],
    };

    for entry in entries {
        let req = match entry.get("request") {
            Some(r) => r,
            None => continue,
        };
        let method = req
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();
        let url = req
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if url.is_empty() {
            continue;
        }

        let mut content = String::new();
        content.push_str(&format!("{method} {url}\n"));
        if let Some(headers) = req.get("headers").and_then(|v| v.as_array()) {
            for h in headers {
                let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() || name.starts_with(':') {
                    continue;
                }
                content.push_str(&format!("{name}: {value}\n"));
            }
        }
        if let Some(body_text) = req
            .pointer("/postData/text")
            .and_then(|v| v.as_str())
        {
            if !body_text.is_empty() {
                content.push('\n');
                content.push_str(body_text);
                content.push('\n');
            }
        }

        let url_for_name = url::Url::parse(&url)
            .map(|u| {
                let path = u.path().trim_start_matches('/');
                if path.is_empty() {
                    u.host_str().unwrap_or("request").to_string()
                } else {
                    path.to_string()
                }
            })
            .unwrap_or_else(|_| "request".to_string());
        let name = format!("{}-{}", method.to_lowercase(), slugify(&url_for_name));
        let target = unique_path(&root, &name, "http");
        match fs::write(&target, &content) {
            Ok(_) => result.created.push(target.to_string_lossy().to_string()),
            Err(e) => result.skipped.push(format!("{}: {e}", target.display())),
        }
    }

    Ok(result)
}

#[tauri::command]
fn import_insomnia(folder: String, spec_path: String) -> Result<ImportResult, String> {
    let raw = fs::read_to_string(&spec_path).map_err(|e| format!("read insomnia: {e}"))?;
    let json: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("json: {e}"))?;
    let resources = json
        .get("resources")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Insomnia export has no /resources".to_string())?;

    let root = PathBuf::from(folder).join("insomnia-import");
    fs::create_dir_all(&root).map_err(|e| format!("mkdir: {e}"))?;

    let folder_names: std::collections::HashMap<String, String> = resources
        .iter()
        .filter(|r| r.get("_type").and_then(|t| t.as_str()) == Some("request_group"))
        .filter_map(|r| {
            let id = r.get("_id").and_then(|v| v.as_str())?.to_string();
            let name = r
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("group")
                .to_string();
            Some((id, name))
        })
        .collect();

    let mut result = ImportResult {
        created: vec![],
        skipped: vec![],
    };

    for resource in resources {
        if resource.get("_type").and_then(|t| t.as_str()) != Some("request") {
            continue;
        }
        let method = resource
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();
        let url = resource
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if url.is_empty() {
            continue;
        }
        let parent_id = resource.get("parentId").and_then(|v| v.as_str()).unwrap_or("");
        let folder_name = folder_names
            .get(parent_id)
            .cloned()
            .unwrap_or_else(|| String::new());
        let dir = if folder_name.is_empty() {
            root.clone()
        } else {
            root.join(slugify(&folder_name))
        };
        fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

        let mut content = String::new();
        let req_name = resource
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("request");
        if !req_name.is_empty() {
            content.push_str(&format!("# {req_name}\n\n"));
        }
        content.push_str(&format!("{method} {url}\n"));
        if let Some(headers) = resource.get("headers").and_then(|v| v.as_array()) {
            for h in headers {
                let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                content.push_str(&format!("{name}: {value}\n"));
            }
        }
        let body_text = resource
            .pointer("/body/text")
            .and_then(|v| v.as_str())
            .map(String::from);
        if let Some(b) = body_text {
            if !b.is_empty() {
                content.push('\n');
                content.push_str(&b);
                content.push('\n');
            }
        }
        let target = unique_path(&dir, &slugify(req_name), "http");
        match fs::write(&target, &content) {
            Ok(_) => result.created.push(target.to_string_lossy().to_string()),
            Err(e) => result.skipped.push(format!("{}: {e}", target.display())),
        }
    }

    Ok(result)
}

#[tauri::command]
fn import_postman(folder: String, spec_path: String) -> Result<ImportResult, String> {
    let raw = fs::read_to_string(&spec_path).map_err(|e| format!("read collection: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("collection json: {e}"))?;
    let collection_name = json
        .pointer("/info/name")
        .and_then(|v| v.as_str())
        .map(slugify)
        .unwrap_or_else(|| "postman".to_string());
    let root = PathBuf::from(folder).join(&collection_name);
    fs::create_dir_all(&root).map_err(|e| format!("mkdir: {e}"))?;

    let items = match json.pointer("/item").and_then(|v| v.as_array()) {
        Some(i) => i,
        None => return Err("collection has no /item".to_string()),
    };

    let mut result = ImportResult {
        created: vec![],
        skipped: vec![],
    };
    walk_postman_items(items, &root, &mut result);
    Ok(result)
}

fn walk_postman_items(items: &[serde_json::Value], dir: &Path, result: &mut ImportResult) {
    for item in items {
        let item = match item.as_object() {
            Some(o) => o,
            None => continue,
        };
        if let Some(sub) = item.get("item").and_then(|v| v.as_array()) {
            let folder_name = item
                .get("name")
                .and_then(|v| v.as_str())
                .map(slugify)
                .unwrap_or_else(|| "folder".to_string());
            let sub_dir = dir.join(folder_name);
            if let Err(e) = fs::create_dir_all(&sub_dir) {
                result.skipped.push(format!("{}: {e}", sub_dir.display()));
                continue;
            }
            walk_postman_items(sub, &sub_dir, result);
            continue;
        }
        let req = match item.get("request") {
            Some(r) => r,
            None => continue,
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(slugify)
            .unwrap_or_else(|| "request".to_string());

        let method = req
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();

        let url_raw = match req.get("url") {
            Some(u) => u,
            None => continue,
        };
        let url = if url_raw.is_string() {
            url_raw.as_str().unwrap_or("").to_string()
        } else if let Some(obj) = url_raw.as_object() {
            obj.get("raw")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            continue;
        };
        let url = postman_substitute_vars(&url);
        if url.is_empty() {
            continue;
        }

        let mut headers: Vec<(String, String)> = vec![];
        if let Some(arr) = req.get("header").and_then(|v| v.as_array()) {
            for h in arr {
                let h = match h.as_object() {
                    Some(o) => o,
                    None => continue,
                };
                if h.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false) {
                    continue;
                }
                let k = h.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let v = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if k.is_empty() {
                    continue;
                }
                headers.push((k.to_string(), postman_substitute_vars(v)));
            }
        }

        let body = req
            .get("body")
            .and_then(|b| b.get("raw"))
            .and_then(|v| v.as_str())
            .map(postman_substitute_vars);

        let mut content = String::new();
        content.push_str(&format!("{} {}\n", method, url));
        for (k, v) in &headers {
            content.push_str(&format!("{k}: {v}\n"));
        }
        if let Some(b) = body {
            if !b.trim().is_empty() {
                content.push_str("\n");
                content.push_str(&b);
                content.push('\n');
            }
        }

        let target = unique_path(dir, &name, "http");
        match fs::write(&target, &content) {
            Ok(_) => result.created.push(target.to_string_lossy().to_string()),
            Err(e) => result.skipped.push(format!("{}: {e}", target.display())),
        }
    }
}

fn postman_substitute_vars(s: &str) -> String {
    s.replace(":", ":")
}

#[tauri::command]
fn import_openapi(folder: String, spec_path: String) -> Result<ImportResult, String> {
    let raw = fs::read_to_string(&spec_path).map_err(|e| format!("read spec: {e}"))?;
    let mut spec: serde_json::Value = if spec_path.ends_with(".yaml") || spec_path.ends_with(".yml") {
        let v: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|e| format!("yaml: {e}"))?;
        serde_json::to_value(v).map_err(|e| format!("yaml→json: {e}"))?
    } else {
        serde_json::from_str(&raw).map_err(|e| format!("json: {e}"))?
    };
    let root = spec.clone();
    resolve_refs(&mut spec, &root, 0);
    generate_openapi_files(folder, spec)
}

fn resolve_refs(value: &mut serde_json::Value, root: &serde_json::Value, depth: u32) {
    if depth > 20 {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            if map.len() == 1 {
                if let Some(serde_json::Value::String(ref_str)) = map.get("$ref") {
                    if let Some(suffix) = ref_str.strip_prefix("#/") {
                        let pointer = format!("/{}", suffix);
                        if let Some(target) = root.pointer(&pointer) {
                            let mut resolved = target.clone();
                            resolve_refs(&mut resolved, root, depth + 1);
                            *value = resolved;
                            return;
                        }
                    }
                }
            }
            for (_, v) in map.iter_mut() {
                resolve_refs(v, root, depth + 1);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                resolve_refs(v, root, depth + 1);
            }
        }
        _ => {}
    }
}

fn generate_openapi_files(folder: String, spec: serde_json::Value) -> Result<ImportResult, String> {
    let root = PathBuf::from(folder);
    fs::create_dir_all(&root).map_err(|e| format!("mkdir: {e}"))?;

    let api_name = spec
        .pointer("/info/title")
        .and_then(|v| v.as_str())
        .map(|s| slugify(s))
        .unwrap_or_else(|| "api".to_string());

    let base_url = spec
        .pointer("/servers/0/url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();

    let security_schemes = spec
        .pointer("/components/securitySchemes")
        .and_then(|v| v.as_object())
        .cloned();

    let mut result = ImportResult {
        created: vec![],
        skipped: vec![],
    };

    let paths = match spec.pointer("/paths").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return Err("spec has no /paths".to_string()),
    };

    for (path, methods) in paths {
        let methods = match methods.as_object() {
            Some(m) => m,
            None => continue,
        };
        for (method, op) in methods {
            let m_upper = method.to_uppercase();
            if !matches!(
                m_upper.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
            ) {
                continue;
            }
            let op = match op.as_object() {
                Some(o) => o,
                None => continue,
            };

            let summary = op
                .get("summary")
                .and_then(|v| v.as_str())
                .or_else(|| op.get("description").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();

            let op_id = op
                .get("operationId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    format!(
                        "{}-{}",
                        method.to_lowercase(),
                        slugify(path.trim_start_matches('/'))
                    )
                });

            let url = transform_path(&base_url, path);

            let mut headers: Vec<(String, String)> = vec![];
            apply_security(&security_schemes, op, &mut headers);
            apply_param_headers(op, &mut headers);

            let body = build_body(op, &mut headers);

            let mut content = String::new();
            if !summary.is_empty() {
                content.push_str(&format!("### {summary}\n\n"));
            }
            content.push_str(&format!("{m_upper} {url}\n"));
            for (k, v) in &headers {
                content.push_str(&format!("{k}: {v}\n"));
            }
            if let Some(b) = &body {
                content.push_str("\n");
                content.push_str(b);
                content.push_str("\n");
            }

            let dir = root.join(&api_name);
            fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
            let target = unique_path(&dir, &slugify(&op_id), "http");
            match fs::write(&target, &content) {
                Ok(_) => result.created.push(target.to_string_lossy().to_string()),
                Err(e) => result.skipped.push(format!("{}: {e}", target.display())),
            }
        }
    }

    Ok(result)
}

fn transform_path(base: &str, path: &str) -> String {
    let mut out = String::from(base);
    let mut chars = path.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut name = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                name.push(c);
            }
            out.push_str("{{");
            out.push_str(&name);
            out.push_str("}}");
        } else {
            out.push(ch);
        }
    }
    out
}

fn apply_security(
    schemes: &Option<serde_json::Map<String, serde_json::Value>>,
    op: &serde_json::Map<String, serde_json::Value>,
    headers: &mut Vec<(String, String)>,
) {
    let security = op.get("security").and_then(|v| v.as_array());
    let Some(security) = security else { return };
    let Some(schemes) = schemes else { return };

    for entry in security {
        let entry = match entry.as_object() {
            Some(o) => o,
            None => continue,
        };
        for name in entry.keys() {
            let scheme = match schemes.get(name).and_then(|v| v.as_object()) {
                Some(s) => s,
                None => continue,
            };
            let kind = scheme.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "http" {
                let bearer = scheme.get("scheme").and_then(|v| v.as_str()) == Some("bearer");
                if bearer {
                    headers.push(("Authorization".to_string(), "Bearer {{accessToken}}".to_string()));
                } else {
                    headers.push(("Authorization".to_string(), "{{auth}}".to_string()));
                }
            } else if kind == "apiKey" {
                let in_loc = scheme.get("in").and_then(|v| v.as_str()).unwrap_or("");
                let header_name = scheme.get("name").and_then(|v| v.as_str()).unwrap_or("X-API-Key");
                if in_loc == "header" {
                    headers.push((header_name.to_string(), "{{apiKey}}".to_string()));
                }
            }
            return;
        }
    }
}

fn apply_param_headers(
    op: &serde_json::Map<String, serde_json::Value>,
    headers: &mut Vec<(String, String)>,
) {
    let params = match op.get("parameters").and_then(|v| v.as_array()) {
        Some(p) => p,
        None => return,
    };
    for p in params {
        let p = match p.as_object() {
            Some(o) => o,
            None => continue,
        };
        if p.get("in").and_then(|v| v.as_str()) != Some("header") {
            continue;
        }
        let name = match p.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let placeholder = format!("{{{{{}}}}}", name);
        headers.push((name.to_string(), placeholder));
    }
}

fn build_body(
    op: &serde_json::Map<String, serde_json::Value>,
    headers: &mut Vec<(String, String)>,
) -> Option<String> {
    let body = op.get("requestBody").and_then(|v| v.as_object())?;
    let content = body.get("content").and_then(|v| v.as_object())?;
    let json = content.get("application/json").and_then(|v| v.as_object());
    if let Some(json) = json {
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
        let example = json
            .get("example")
            .or_else(|| json.get("schema").and_then(|s| s.get("example")))
            .or_else(|| json.get("schema").and_then(|s| s.get("default")));
        if let Some(ex) = example {
            return serde_json::to_string_pretty(ex).ok();
        }
        if let Some(schema) = json.get("schema").and_then(|v| v.as_object()) {
            return Some(sample_from_schema(schema));
        }
        return Some("{}".to_string());
    }
    if let Some(form) = content.get("application/x-www-form-urlencoded") {
        headers.push((
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ));
        if let Some(props) = form
            .get("schema")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
        {
            return Some(
                props
                    .keys()
                    .map(|k| format!("{k}={{{{{}}}}}", k))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }
        return Some(String::new());
    }
    None
}

fn sample_from_schema(schema: &serde_json::Map<String, serde_json::Value>) -> String {
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        let mut obj = serde_json::Map::new();
        for (k, v) in props {
            let prop_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("string");
            let val = match prop_type {
                "integer" | "number" => serde_json::json!(0),
                "boolean" => serde_json::json!(false),
                "array" => serde_json::json!([]),
                "object" => serde_json::json!({}),
                _ => serde_json::json!(""),
            };
            obj.insert(k.clone(), val);
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or("{}".to_string())
    } else {
        "{}".to_string()
    }
}

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::Digest as _;

#[derive(Deserialize, Clone)]
pub struct DigestCreds {
    pub username: String,
    pub password: String,
}

fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn parse_challenge(s: &str) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let mut out = HashMap::new();
    let lower = s.trim_start();
    let after = if lower.to_lowercase().starts_with("digest ") {
        &lower[7..]
    } else {
        lower
    };
    let mut chars = after.chars().peekable();
    while chars.peek().is_some() {
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else {
                break;
            }
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(chars.next().unwrap());
        }
        while let Some(&c) = chars.peek() {
            if c == '=' {
                chars.next();
                break;
            }
            if c.is_whitespace() {
                chars.next();
                continue;
            }
            break;
        }
        let mut value = String::new();
        let in_quotes = matches!(chars.peek(), Some(&'"'));
        if in_quotes {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                value.push(chars.next().unwrap());
            }
        }
        if !key.is_empty() {
            out.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    out
}

fn compute_digest_header(
    user: &str,
    pass: &str,
    method: &str,
    uri: &str,
    body: &[u8],
    challenge: &str,
) -> Option<String> {
    let mut cnonce_bytes = [0u8; 8];
    getrandom::getrandom(&mut cnonce_bytes).ok()?;
    let cnonce = hex::encode(cnonce_bytes);
    compute_digest_header_with_cnonce(user, pass, method, uri, body, challenge, &cnonce)
}

fn compute_digest_header_with_cnonce(
    user: &str,
    pass: &str,
    method: &str,
    uri: &str,
    body: &[u8],
    challenge: &str,
    cnonce: &str,
) -> Option<String> {
    let params = parse_challenge(challenge);
    let realm = params.get("realm")?.clone();
    let nonce = params.get("nonce")?.clone();
    let qop = params.get("qop").cloned();
    let algorithm = params
        .get("algorithm")
        .cloned()
        .unwrap_or_else(|| "MD5".to_string());
    let opaque = params.get("opaque").cloned();

    let nc = "00000001";

    let ha1 = if algorithm.eq_ignore_ascii_case("MD5-SESS") {
        let base = md5_hex(format!("{user}:{realm}:{pass}").as_bytes());
        md5_hex(format!("{base}:{nonce}:{cnonce}").as_bytes())
    } else {
        md5_hex(format!("{user}:{realm}:{pass}").as_bytes())
    };

    let auth_int = qop.as_deref().map(|q| q.contains("auth-int")).unwrap_or(false);
    let ha2 = if auth_int {
        md5_hex(format!("{method}:{uri}:{}", md5_hex(body)).as_bytes())
    } else {
        md5_hex(format!("{method}:{uri}").as_bytes())
    };

    let response = if let Some(_q) = &qop {
        let q_value = if auth_int { "auth-int" } else { "auth" };
        md5_hex(format!("{ha1}:{nonce}:{nc}:{cnonce}:{q_value}:{ha2}").as_bytes())
    } else {
        md5_hex(format!("{ha1}:{nonce}:{ha2}").as_bytes())
    };

    let mut header = format!(
        r#"Digest username="{user}", realm="{realm}", nonce="{nonce}", uri="{uri}", response="{response}", algorithm={algorithm}"#,
    );
    if qop.is_some() {
        let q_value = if auth_int { "auth-int" } else { "auth" };
        header.push_str(&format!(
            r#", qop={q_value}, nc={nc}, cnonce="{cnonce}""#
        ));
    }
    if let Some(o) = opaque {
        header.push_str(&format!(r#", opaque="{o}""#));
    }
    Some(header)
}

#[derive(Deserialize)]
pub struct ScriptRunArgs {
    pub script: String,
    pub env: Vec<(String, String)>,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub response: Option<script::ResponseDataForScript>,
    pub timeout_ms: Option<u64>,
}

const GRAPHQL_INTROSPECTION: &str = r#"query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      kind name description
      fields(includeDeprecated: true) {
        name description
        args { name description type { kind name ofType { kind name ofType { kind name ofType { kind name } } } } defaultValue }
        type { kind name ofType { kind name ofType { kind name ofType { kind name } } } }
      }
      inputFields { name description type { kind name ofType { kind name } } defaultValue }
      enumValues(includeDeprecated: true) { name description }
      interfaces { name }
      possibleTypes { name }
    }
  }
}"#;

#[tauri::command]
async fn graphql_introspect(
    url: String,
    headers: Vec<(String, String)>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_provider(state.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "query": GRAPHQL_INTROSPECTION });
    let body_str = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    let mut req = client.post(&url).header("Content-Type", "application/json");
    for (k, v) in &headers {
        if k.eq_ignore_ascii_case("content-type") {
            continue;
        }
        req = req.header(k, v);
    }
    let resp = req
        .body(body_str)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("introspection {status}: {}", text.chars().take(200).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse: {e}"))
}

#[tauri::command]
fn monitor_start(
    request_path: String,
    interval_secs: u64,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    monitors::start(&state.monitors, request_path, interval_secs, app)
}

#[tauri::command]
fn monitor_stop(request_path: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    monitors::stop(&state.monitors, &request_path)
}

#[tauri::command]
fn monitor_list(state: tauri::State<'_, AppState>) -> Result<Vec<(String, u64)>, String> {
    Ok(monitors::list_active(&state.monitors))
}

#[tauri::command]
fn monitor_parse_schedule(content: String) -> Option<u64> {
    monitors::parse_schedule(&content)
}

#[tauri::command]
async fn ws_connect(
    url: String,
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let registry = state.ws_connections.clone();
    ws::connect(url, id, app, registry).await
}

#[tauri::command]
fn ws_send(id: String, text: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    ws::send(&state.ws_connections, &id, text)
}

#[tauri::command]
fn ws_close(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    ws::close(&state.ws_connections, &id)
}

#[tauri::command]
fn run_script(args: ScriptRunArgs) -> script::ScriptOutcome {
    script::run_script(script::ScriptInput {
        script: args.script,
        env: args.env,
        method: args.method,
        url: args.url,
        headers: args.headers,
        body: args.body,
        response: args.response,
        timeout_ms: args.timeout_ms.unwrap_or(5000),
    })
}

#[tauri::command]
async fn ai_review_request(
    api_key: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<ai::ReviewResult, String> {
    ai::review_request(api_key, method, url, headers, body).await
}

#[tauri::command]
async fn ai_review_request_openai_compat(
    base_url: String,
    api_key: String,
    model: String,
    supports_json_mode: bool,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<ai::ReviewResult, String> {
    ai::review_request_openai_compat(
        base_url,
        api_key,
        model,
        supports_json_mode,
        method,
        url,
        headers,
        body,
    )
    .await
}

#[tauri::command]
async fn fetch_oauth_authcode(
    auth_url: String,
    token_url: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
    redirect_port: u16,
    state_app: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    // PKCE
    let mut verifier_bytes = [0u8; 32];
    getrandom::getrandom(&mut verifier_bytes).map_err(|e| e.to_string())?;
    let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge_digest = sha2::Sha256::digest(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(challenge_digest);

    // CSRF state
    let mut state_bytes = [0u8; 16];
    getrandom::getrandom(&mut state_bytes).map_err(|e| e.to_string())?;
    let csrf_state = hex::encode(state_bytes);

    let redirect_uri = format!("http://localhost:{redirect_port}/callback");

    let mut auth = url::Url::parse(&auth_url).map_err(|e| format!("auth_url: {e}"))?;
    {
        let mut q = auth.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", &client_id);
        q.append_pair("redirect_uri", &redirect_uri);
        q.append_pair("state", &csrf_state);
        q.append_pair("code_challenge", &code_challenge);
        q.append_pair("code_challenge_method", "S256");
        if let Some(s) = &scope {
            if !s.is_empty() {
                q.append_pair("scope", s);
            }
        }
    }

    let server = tiny_http::Server::http(format!("127.0.0.1:{redirect_port}"))
        .map_err(|e| format!("listen on :{redirect_port}: {e}"))?;

    if let Err(e) = webbrowser::open(auth.as_str()) {
        return Err(format!("open browser: {e}"));
    }

    let started = Instant::now();
    let mut received_code: Option<String> = None;
    let mut received_state: Option<String> = None;
    let mut received_error: Option<String> = None;

    while started.elapsed() < Duration::from_secs(300) {
        match server.recv_timeout(Duration::from_millis(500)) {
            Ok(Some(req)) => {
                let path_with_query = req.url().to_string();
                let parsed = url::Url::parse(&format!("http://localhost{path_with_query}"))
                    .ok();
                if let Some(parsed) = parsed {
                    for (k, v) in parsed.query_pairs() {
                        match k.as_ref() {
                            "code" => received_code = Some(v.into_owned()),
                            "state" => received_state = Some(v.into_owned()),
                            "error" => received_error = Some(v.into_owned()),
                            _ => {}
                        }
                    }
                }
                let response_html = if received_error.is_some() {
                    format!(
                        "<!DOCTYPE html><html><body style='font-family:sans-serif;text-align:center;padding:60px'><h1>Authorization failed</h1><p>{}</p><p>You can close this tab.</p></body></html>",
                        received_error.as_deref().unwrap_or("")
                    )
                } else if received_code.is_some() {
                    "<!DOCTYPE html><html><body style='font-family:sans-serif;text-align:center;padding:60px;background:#0e0f12;color:#e6e8ec'><h1 style='color:#8b5cf6'>Authorized — return to Dante</h1><p>You can close this tab.</p></body></html>".to_string()
                } else {
                    "<!DOCTYPE html><html><body><h1>Missing authorization code</h1></body></html>".to_string()
                };
                let header = tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap();
                let _ = req.respond(
                    tiny_http::Response::from_string(response_html).with_header(header),
                );
                break;
            }
            Ok(None) => continue,
            Err(_) => break,
        }
    }

    drop(server);

    if let Some(err) = received_error {
        return Err(format!("authorization error: {err}"));
    }
    let code = received_code.ok_or_else(|| {
        "no callback received within 5 minutes — check redirect URL is http://localhost:{port}/callback"
            .to_string()
    })?;
    if received_state.as_deref() != Some(&csrf_state) {
        return Err("state parameter mismatch (possible CSRF)".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_provider(state_app.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;

    let form: Vec<(&str, String)> = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code_verifier", code_verifier),
    ];
    let resp = client
        .post(&token_url)
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("token endpoint {status}: {body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("token response: {e}"))
}

#[derive(Serialize)]
pub struct DeviceAuthInit {
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub device_code: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[tauri::command]
async fn oauth_device_init(
    device_authorization_url: String,
    client_id: String,
    scope: Option<String>,
    state_app: tauri::State<'_, AppState>,
) -> Result<DeviceAuthInit, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_provider(state_app.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;
    let mut form: Vec<(&str, String)> = vec![("client_id", client_id)];
    if let Some(s) = scope {
        if !s.is_empty() {
            form.push(("scope", s));
        }
    }
    let resp = client
        .post(&device_authorization_url)
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("device endpoint {status}: {body}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("device response: {e}"))?;
    let user_code = v
        .get("user_code")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "no user_code".to_string())?
        .to_string();
    let verification_uri = v
        .get("verification_uri")
        .or_else(|| v.get("verification_url"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| "no verification_uri".to_string())?
        .to_string();
    let verification_uri_complete = v
        .get("verification_uri_complete")
        .and_then(|x| x.as_str())
        .map(String::from);
    let device_code = v
        .get("device_code")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "no device_code".to_string())?
        .to_string();
    let expires_in = v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(900);
    let interval = v.get("interval").and_then(|x| x.as_u64()).unwrap_or(5);

    let open_target = verification_uri_complete.clone().unwrap_or_else(|| verification_uri.clone());
    let _ = webbrowser::open(&open_target);

    Ok(DeviceAuthInit {
        user_code,
        verification_uri,
        verification_uri_complete,
        device_code,
        expires_in,
        interval,
    })
}

#[tauri::command]
async fn oauth_device_poll(
    token_url: String,
    client_id: String,
    client_secret: Option<String>,
    device_code: String,
    interval_sec: u64,
    expires_in_sec: u64,
    state_app: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_provider(state_app.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;

    let started = Instant::now();
    let mut current_interval = std::cmp::max(interval_sec, 1);
    while started.elapsed() < Duration::from_secs(expires_in_sec) {
        tokio::time::sleep(Duration::from_secs(current_interval)).await;
        let mut form: Vec<(&str, String)> = vec![
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            ),
            ("client_id", client_id.clone()),
            ("device_code", device_code.clone()),
        ];
        if let Some(s) = &client_secret {
            if !s.is_empty() {
                form.push(("client_secret", s.clone()));
            }
        }
        let resp = client
            .post(&token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body = resp.text().await.map_err(|e| e.to_string())?;
        let v: serde_json::Value =
            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
            match err {
                "authorization_pending" => continue,
                "slow_down" => {
                    current_interval += 5;
                    continue;
                }
                "access_denied" => return Err("user denied authorization".to_string()),
                "expired_token" => return Err("device code expired".to_string()),
                other => return Err(format!("device flow error: {other}")),
            }
        }
        if v.get("access_token").is_some() {
            return Ok(v);
        }
        return Err(format!("unexpected device response: {body}"));
    }
    Err("device flow timed out".to_string())
}

async fn post_token_form(
    client: &reqwest::Client,
    token_url: &str,
    form: &[(&str, String)],
    error_label: &str,
) -> Result<serde_json::Value, String> {
    let resp = client
        .post(token_url)
        .form(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{error_label} {status}: {body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))
}

#[tauri::command]
async fn fetch_oauth_refresh(
    token_url: String,
    client_id: String,
    client_secret: Option<String>,
    refresh_token: String,
    scope: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_provider(state.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
    ];
    if let Some(s) = client_secret {
        if !s.is_empty() {
            form.push(("client_secret", s));
        }
    }
    if let Some(s) = scope {
        if !s.is_empty() {
            form.push(("scope", s));
        }
    }
    post_token_form(&client, &token_url, &form, "refresh").await
}

#[tauri::command]
async fn fetch_oauth_password(
    token_url: String,
    client_id: String,
    client_secret: Option<String>,
    username: String,
    password: String,
    scope: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_provider(state.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "password".to_string()),
        ("client_id", client_id),
        ("username", username),
        ("password", password),
    ];
    if let Some(s) = client_secret {
        if !s.is_empty() {
            form.push(("client_secret", s));
        }
    }
    if let Some(s) = scope {
        if !s.is_empty() {
            form.push(("scope", s));
        }
    }
    let resp = client
        .post(&token_url)
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("token endpoint {status}: {body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("token response: {e}"))
}

#[tauri::command]
async fn fetch_oauth_token(
    token_url: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_provider(state.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "client_credentials".to_string()),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    if let Some(s) = scope {
        if !s.is_empty() {
            form.push(("scope", s));
        }
    }
    let resp = client
        .post(&token_url)
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("token endpoint returned {status}: {body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("token response not JSON: {e}"))
}

#[tauri::command]
fn save_cookies(folder: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let path = cookies_path(&folder);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let mut file = fs::File::create(&path).map_err(|e| format!("create cookies: {e}"))?;
    let store = state.cookie_jar.lock().map_err(|e| e.to_string())?;
    store.save_json(&mut file).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct RequestEntry {
    pub name: String,
    pub path: String,
    pub folder: String,
    pub method: String,
    pub url: String,
    pub description: String,
    pub modified_ms: u64,
}

fn url_path_with_query(u: &str) -> String {
    if let Ok(parsed) = url::Url::parse(u) {
        let mut p = parsed.path().to_string();
        if let Some(q) = parsed.query() {
            p.push('?');
            p.push_str(q);
        }
        if p.is_empty() {
            p.push('/');
        }
        p
    } else {
        "/".to_string()
    }
}

#[tauri::command]
async fn run_request(
    spec: RequestSpec,
    aws: Option<AwsParams>,
    digest: Option<DigestCreds>,
    state: tauri::State<'_, AppState>,
) -> Result<ResponseData, String> {
    let method = Method::from_str(&spec.method.to_uppercase())
        .map_err(|e| format!("invalid method: {e}"))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .cookie_provider(state.cookie_jar.clone())
        .build()
        .map_err(|e| e.to_string())?;

    let mut headers = spec.headers.clone();
    let body_bytes: Vec<u8> = match &spec.body {
        Some(s) if s.starts_with('@') => {
            let path = s[1..].trim();
            fs::read(path).map_err(|e| format!("read @{path}: {e}"))?
        }
        Some(s) => s.as_bytes().to_vec(),
        None => Vec::new(),
    };

    if let Some(aws) = aws {
        let signed = sigv4::sign_request(
            &aws,
            &spec.method,
            &spec.url,
            &headers,
            &body_bytes,
        )?;
        headers.retain(|(k, _)| {
            let lk = k.to_lowercase();
            lk != "authorization" && lk != "x-amz-date" && lk != "x-amz-security-token" && lk != "x-amz-content-sha256"
        });
        headers.push(("X-Amz-Date".to_string(), signed.amz_date));
        headers.push(("X-Amz-Content-Sha256".to_string(), signed.content_sha256));
        if let Some(tok) = signed.session_token {
            headers.push(("X-Amz-Security-Token".to_string(), tok));
        }
        headers.push(("Authorization".to_string(), signed.authorization));
    }

    let mut req = client.request(method.clone(), &spec.url);
    for (k, v) in &headers {
        req = req.header(k, v);
    }
    if !body_bytes.is_empty() {
        req = req.body(body_bytes.clone());
    }

    let started = Instant::now();
    let initial_resp = req.send().await.map_err(|e| e.to_string())?;

    let initial_status = initial_resp.status();
    let www_auth = if initial_status == 401 {
        initial_resp
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    } else {
        None
    };

    let resp = if let (Some(creds), Some(challenge)) = (&digest, &www_auth) {
        if challenge.to_lowercase().starts_with("digest ") {
            let uri = url_path_with_query(&spec.url);
            if let Some(auth_header) = compute_digest_header(
                &creds.username,
                &creds.password,
                method.as_str(),
                &uri,
                &body_bytes,
                challenge,
            ) {
                let mut req2 = client.request(method.clone(), &spec.url);
                let mut new_headers: Vec<(String, String)> = headers
                    .iter()
                    .filter(|(k, _)| !k.eq_ignore_ascii_case("authorization"))
                    .cloned()
                    .collect();
                for (k, v) in &new_headers {
                    req2 = req2.header(k, v);
                }
                req2 = req2.header("Authorization", auth_header);
                if !body_bytes.is_empty() {
                    req2 = req2.body(body_bytes.clone());
                }
                let _ = new_headers;
                req2.send().await.map_err(|e| e.to_string())?
            } else {
                initial_resp
            }
        } else {
            initial_resp
        }
    } else {
        initial_resp
    };
    let status = resp.status().as_u16();
    let status_text = resp
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    Ok(ResponseData {
        status,
        status_text,
        headers,
        body,
        elapsed_ms,
    })
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    Ok(dir.join("settings.json"))
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let path = settings_path(&app)?;
    if !path.exists() {
        return Ok(Settings::default());
    }
    let s = fs::read_to_string(&path).map_err(|e| format!("read settings: {e}"))?;
    serde_json::from_str(&s).map_err(|e| format!("parse settings: {e}"))
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), String> {
    let path = settings_path(&app)?;
    let s = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, s).map_err(|e| format!("write settings: {e}"))
}

#[derive(Serialize, Deserialize, Default)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub default_headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[tauri::command]
fn read_workspace_config(folder: String) -> Result<WorkspaceConfig, String> {
    let path = PathBuf::from(&folder).join(".dante.config.json");
    if !path.exists() {
        return Ok(WorkspaceConfig::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse: {e}"))
}

#[tauri::command]
fn write_workspace_config(folder: String, config: WorkspaceConfig) -> Result<(), String> {
    let path = PathBuf::from(&folder).join(".dante.config.json");
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| format!("write: {e}"))
}

#[tauri::command]
fn default_project_folder(app: tauri::AppHandle) -> Result<String, String> {
    let docs = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| format!("docs dir: {e}"))?;
    let folder = docs.join("Dante");
    Ok(folder.to_string_lossy().to_string())
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for ch in s.chars() {
        let c = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if c == '-' {
            if !last_dash && !out.is_empty() {
                out.push('-');
            }
            last_dash = true;
        } else {
            out.push(c);
            last_dash = false;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("request");
    }
    out
}

fn unique_path(folder: &Path, base: &str, ext: &str) -> PathBuf {
    let mut candidate = folder.join(format!("{base}.{ext}"));
    let mut i = 2;
    while candidate.exists() {
        candidate = folder.join(format!("{base}-{i}.{ext}"));
        i += 1;
    }
    candidate
}

#[tauri::command]
fn save_request(
    folder: String,
    name: String,
    content: String,
    overwrite_path: Option<String>,
    subfolder: Option<String>,
) -> Result<String, String> {
    let root = PathBuf::from(&folder);

    let target = if let Some(p) = overwrite_path {
        let path = PathBuf::from(p);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        path
    } else {
        let dir = match subfolder.as_deref() {
            Some(sf) if !sf.is_empty() => root.join(sf),
            _ => root.clone(),
        };
        fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
        unique_path(&dir, &slugify(&name), "http")
    };

    fs::write(&target, &content).map_err(|e| format!("write: {e}"))?;
    target
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "non-utf8 path".to_string())
}

fn quick_parse_http(content: &str) -> (String, String, String) {
    let mut description_lines: Vec<String> = vec![];
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("###") {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            description_lines.push(rest.trim().to_string());
            continue;
        }
        if let Some((method, url)) = trimmed.split_once(char::is_whitespace) {
            return (
                method.to_uppercase(),
                url.trim().to_string(),
                description_lines.join(" "),
            );
        }
        break;
    }
    (
        "GET".to_string(),
        "".to_string(),
        description_lines.join(" "),
    )
}

fn collect_http_files(
    root: &Path,
    current: &Path,
    folder: &str,
    out: &mut Vec<RequestEntry>,
    depth: usize,
) {
    let dir = match fs::read_dir(current) {
        Ok(d) => d,
        Err(_) => return,
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth >= 1 {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name.starts_with('.') {
                continue;
            }
            collect_http_files(root, &path, &name, out, depth + 1);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("http") {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let (method, url, description) = quick_parse_http(&content);
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let modified_ms = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.push(RequestEntry {
            name,
            path: path.to_string_lossy().to_string(),
            folder: folder.to_string(),
            method,
            url,
            description,
            modified_ms,
        });
    }
}

#[tauri::command]
fn list_requests(folder: String) -> Result<Vec<RequestEntry>, String> {
    let root = PathBuf::from(folder);
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut entries: Vec<RequestEntry> = vec![];
    collect_http_files(&root, &root, "", &mut entries, 0);
    entries.sort_by(|a, b| {
        a.folder
            .cmp(&b.folder)
            .then_with(|| b.modified_ms.cmp(&a.modified_ms))
    });
    Ok(entries)
}

#[tauri::command]
fn load_request(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| format!("read: {e}"))
}

#[tauri::command]
fn delete_request(path: String) -> Result<(), String> {
    fs::remove_file(&path).map_err(|e| format!("delete: {e}"))?;
    let history = history_path_for(&path);
    if history.exists() {
        let _ = fs::remove_file(history);
    }
    Ok(())
}

#[tauri::command]
fn rename_request(old_path: String, new_name: String) -> Result<String, String> {
    let old = PathBuf::from(&old_path);
    let parent = old
        .parent()
        .ok_or_else(|| "no parent dir".to_string())?
        .to_path_buf();
    let slug = slugify(&new_name);
    if slug.is_empty() {
        return Err("invalid name".to_string());
    }
    let target = unique_path(&parent, &slug, "http");
    fs::rename(&old, &target).map_err(|e| format!("rename: {e}"))?;
    // Also rename the history sidecar if present
    let old_hist = history_path_for(&old_path);
    if old_hist.exists() {
        let new_hist = history_path_for(&target.to_string_lossy());
        let _ = fs::rename(&old_hist, &new_hist);
    }
    Ok(target.to_string_lossy().to_string())
}

#[tauri::command]
fn rename_folder(root: String, old_name: String, new_name: String) -> Result<String, String> {
    let root_path = PathBuf::from(&root);
    let old = root_path.join(&old_name);
    let new_slug = slugify(&new_name);
    if new_slug.is_empty() {
        return Err("invalid name".to_string());
    }
    let new = root_path.join(&new_slug);
    if !old.exists() {
        return Err("folder not found".to_string());
    }
    if new.exists() {
        return Err(format!("a folder named {new_slug} already exists"));
    }
    fs::rename(&old, &new).map_err(|e| format!("rename: {e}"))?;
    Ok(new.to_string_lossy().to_string())
}

#[tauri::command]
fn delete_folder(root: String, folder_name: String) -> Result<(), String> {
    if folder_name.is_empty() {
        return Err("cannot delete root".to_string());
    }
    let path = PathBuf::from(&root).join(&folder_name);
    if !path.exists() {
        return Err("folder not found".to_string());
    }
    fs::remove_dir_all(&path).map_err(|e| format!("delete: {e}"))
}

#[tauri::command]
fn move_request(path: String, target_folder: String, root: String) -> Result<String, String> {
    let src = PathBuf::from(&path);
    let root_path = PathBuf::from(&root);
    let target_dir = if target_folder.is_empty() {
        root_path
    } else {
        root_path.join(&target_folder)
    };
    fs::create_dir_all(&target_dir).map_err(|e| format!("mkdir: {e}"))?;
    let file_name = src
        .file_name()
        .ok_or_else(|| "no file name".to_string())?
        .to_string_lossy()
        .to_string();
    let target = target_dir.join(&file_name);
    if src == target {
        return Ok(target.to_string_lossy().to_string());
    }
    let final_target = if target.exists() {
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("request");
        unique_path(&target_dir, stem, "http")
    } else {
        target
    };
    fs::rename(&src, &final_target).map_err(|e| format!("move: {e}"))?;
    let old_hist = history_path_for(&path);
    if old_hist.exists() {
        let new_hist = history_path_for(&final_target.to_string_lossy());
        let _ = fs::rename(&old_hist, &new_hist);
    }
    Ok(final_target.to_string_lossy().to_string())
}

#[tauri::command]
fn duplicate_request(path: String) -> Result<String, String> {
    let src = PathBuf::from(&path);
    let parent = src
        .parent()
        .ok_or_else(|| "no parent dir".to_string())?
        .to_path_buf();
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("request");
    let target = unique_path(&parent, &format!("{stem}-copy"), "http");
    fs::copy(&src, &target).map_err(|e| format!("copy: {e}"))?;
    Ok(target.to_string_lossy().to_string())
}

fn history_path_for(request_path: &str) -> PathBuf {
    let p = PathBuf::from(request_path);
    let mut name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    name.push_str(".history.jsonl");
    p.with_file_name(name)
}

#[derive(Serialize, Deserialize)]
pub struct HistoryEntry {
    pub ts: u64,
    pub request: serde_json::Value,
    pub response: serde_json::Value,
}

#[tauri::command]
fn append_history(
    request_path: String,
    request: serde_json::Value,
    response: serde_json::Value,
) -> Result<u64, String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let entry = HistoryEntry { ts, request, response };
    let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    let path = history_path_for(&request_path);
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open history: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("write history: {e}"))?;
    Ok(ts)
}

#[derive(Serialize)]
pub struct EnvFile {
    pub name: String,
    pub path: String,
}

#[tauri::command]
fn list_envs(folder: String) -> Result<Vec<EnvFile>, String> {
    let folder = PathBuf::from(folder);
    if !folder.exists() {
        return Ok(vec![]);
    }
    let mut envs: Vec<EnvFile> = vec![];
    for entry in fs::read_dir(&folder).map_err(|e| format!("read_dir: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if name == ".env" || name.starts_with(".env.") {
            envs.push(EnvFile {
                name,
                path: path.to_string_lossy().to_string(),
            });
        }
    }
    envs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(envs)
}

#[tauri::command]
fn read_env(path: String) -> Result<Vec<(String, String)>, String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&p).map_err(|e| format!("read env: {e}"))?;
    let mut pairs: Vec<(String, String)> = vec![];
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_string();
            let value = strip_quotes(v.trim());
            pairs.push((key, value));
        }
    }
    Ok(pairs)
}

fn strip_quotes(s: &str) -> String {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0];
        let last = bytes[s.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

#[tauri::command]
fn write_env(path: String, pairs: Vec<(String, String)>) -> Result<(), String> {
    let mut content = String::new();
    for (k, v) in pairs {
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        let needs_quotes = v.contains(char::is_whitespace) || v.contains('#');
        if needs_quotes {
            content.push_str(&format!("{key}=\"{v}\"\n"));
        } else {
            content.push_str(&format!("{key}={v}\n"));
        }
    }
    fs::write(&path, content).map_err(|e| format!("write env: {e}"))
}

#[tauri::command]
fn create_env(folder: String, name: String) -> Result<String, String> {
    let folder = PathBuf::from(folder);
    fs::create_dir_all(&folder).map_err(|e| format!("mkdir: {e}"))?;
    let filename = if name == ".env" || name.starts_with(".env.") {
        name
    } else {
        format!(".env.{name}")
    };
    let path = folder.join(&filename);
    if !path.exists() {
        fs::write(&path, "").map_err(|e| format!("create env: {e}"))?;
    }
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn read_history(request_path: String, limit: Option<usize>) -> Result<Vec<HistoryEntry>, String> {
    let path = history_path_for(&request_path);
    if !path.exists() {
        return Ok(vec![]);
    }
    let file = fs::File::open(&path).map_err(|e| format!("open history: {e}"))?;
    let reader = BufReader::new(file);
    let mut entries: Vec<HistoryEntry> = Vec::new();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
            entries.push(entry);
        }
    }
    entries.reverse();
    if let Some(n) = limit {
        entries.truncate(n);
    }
    Ok(entries)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState {
        cookie_jar: Arc::new(CookieStoreMutex::default()),
        mock_server: Mutex::new(None),
        ws_connections: ws::new_registry(),
        monitors: monitors::new_registry(),
    };
    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            run_request,
            get_settings,
            save_settings,
            default_project_folder,
            read_workspace_config,
            write_workspace_config,
            save_request,
            list_requests,
            load_request,
            delete_request,
            rename_request,
            duplicate_request,
            move_request,
            rename_folder,
            delete_folder,
            append_history,
            read_history,
            list_envs,
            read_env,
            write_env,
            create_env,
            list_cookies,
            clear_cookies,
            delete_cookie,
            load_cookies,
            save_cookies,
            fetch_oauth_token,
            fetch_oauth_authcode,
            fetch_oauth_password,
            fetch_oauth_refresh,
            oauth_device_init,
            oauth_device_poll,
            ai_review_request,
            ai_review_request_openai_compat,
            run_script,
            ws_connect,
            ws_send,
            ws_close,
            monitor_start,
            monitor_stop,
            monitor_list,
            monitor_parse_schedule,
            export_postman,
            graphql_introspect,
            import_openapi,
            import_postman,
            import_har,
            import_insomnia,
            export_markdown,
            export_openapi,
            start_mock_server,
            stop_mock_server,
            mock_server_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Dante");
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- slugify ----

    #[test]
    fn slugify_kebab_cases_with_lowercase() {
        assert_eq!(slugify("List Users"), "list-users");
        assert_eq!(slugify("Auth Login"), "auth-login");
        assert_eq!(slugify("Stripe Charges - List"), "stripe-charges-list");
    }

    #[test]
    fn slugify_collapses_runs_of_dashes() {
        assert_eq!(slugify("a   b   c"), "a-b-c");
        assert_eq!(slugify("foo!!!bar"), "foo-bar");
    }

    #[test]
    fn slugify_strips_leading_and_trailing_separators() {
        assert_eq!(slugify("  hello  "), "hello");
        assert_eq!(slugify("---a---"), "a");
    }

    #[test]
    fn slugify_falls_back_to_request_for_empty() {
        assert_eq!(slugify(""), "request");
        assert_eq!(slugify("!!!"), "request");
    }

    // ---- md5_hex (Digest auth depends on this; one bug breaks all digest auth) ----

    #[test]
    fn md5_hex_matches_known_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        // RFC 2617 HA1 vector
        assert_eq!(
            md5_hex(b"Mufasa:testrealm@host.com:Circle Of Life"),
            "939e7578ed9e3c518a452acee763bce9"
        );
    }

    // ---- parse_challenge (WWW-Authenticate digest header) ----

    #[test]
    fn parse_challenge_extracts_digest_params() {
        let raw = r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
        let params = parse_challenge(raw);
        assert_eq!(params.get("realm").map(String::as_str), Some("testrealm@host.com"));
        assert_eq!(params.get("nonce").map(String::as_str), Some("dcd98b7102dd2f0e8b11d0f600bfb0c093"));
        assert_eq!(params.get("opaque").map(String::as_str), Some("5ccc069c403ebaf9f0171e9517f40e41"));
        assert_eq!(params.get("qop").map(String::as_str), Some("auth,auth-int"));
    }

    #[test]
    fn parse_challenge_handles_unquoted_values() {
        let raw = "Digest algorithm=MD5, realm=\"x\"";
        let params = parse_challenge(raw);
        assert_eq!(params.get("algorithm").map(String::as_str), Some("MD5"));
        assert_eq!(params.get("realm").map(String::as_str), Some("x"));
    }

    #[test]
    fn parse_challenge_lowercases_digest_prefix() {
        let raw = "DIGEST realm=\"x\"";
        let params = parse_challenge(raw);
        assert_eq!(params.get("realm").map(String::as_str), Some("x"));
    }

    // ---- compute_digest_header (RFC 2617 vector) ----

    #[test]
    fn compute_digest_header_matches_rfc_2617_vector() {
        // RFC 2617 §3.5: Mufasa accessing /dir/index.html on testrealm@host.com
        let challenge = r#"Digest realm="testrealm@host.com", qop="auth", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
        let header = compute_digest_header_with_cnonce(
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            b"",
            challenge,
            "0a4f113b", // RFC's example cnonce
        )
        .expect("digest should produce header");

        // Expected response from RFC: 6629fae49393a05397450978507c4ef1
        assert!(
            header.contains(r#"response="6629fae49393a05397450978507c4ef1""#),
            "digest header doesn't match RFC 2617 vector: {header}"
        );
        assert!(header.contains(r#"username="Mufasa""#));
        assert!(header.contains(r#"realm="testrealm@host.com""#));
        assert!(header.contains(r#"uri="/dir/index.html""#));
        assert!(header.contains(r#"qop=auth"#));
        assert!(header.contains(r#"nc=00000001"#));
        assert!(header.contains(r#"cnonce="0a4f113b""#));
        assert!(header.contains(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#));
    }

    #[test]
    fn compute_digest_header_supports_md5_sess() {
        let challenge = r#"Digest realm="r", qop="auth", nonce="n", algorithm=MD5-sess"#;
        let header =
            compute_digest_header_with_cnonce("u", "p", "GET", "/", b"", challenge, "fixed")
                .expect("md5-sess should produce header");
        assert!(header.contains("algorithm=MD5-sess"));
        // Compute expected ha1 manually
        let base = md5_hex(b"u:r:p");
        let ha1 = md5_hex(format!("{base}:n:fixed").as_bytes());
        let ha2 = md5_hex(b"GET:/");
        let expected_response =
            md5_hex(format!("{ha1}:n:00000001:fixed:auth:{ha2}").as_bytes());
        assert!(header.contains(&format!(r#"response="{expected_response}""#)));
    }

    // ---- quick_parse_http ----

    #[test]
    fn quick_parse_http_extracts_method_url_description() {
        let content = "# Auth login\n# Returns a token\nPOST https://api.example.com/auth/login\nContent-Type: application/json\n";
        let (method, url, description) = quick_parse_http(content);
        assert_eq!(method, "POST");
        assert_eq!(url, "https://api.example.com/auth/login");
        assert_eq!(description, "Auth login Returns a token");
    }

    #[test]
    fn quick_parse_http_handles_no_description() {
        let (method, url, desc) = quick_parse_http("GET https://x.com/y\n");
        assert_eq!(method, "GET");
        assert_eq!(url, "https://x.com/y");
        assert_eq!(desc, "");
    }

    #[test]
    fn quick_parse_http_returns_empty_url_for_invalid_content() {
        let (method, url, _) = quick_parse_http("###\n");
        assert_eq!(method, "GET");
        assert_eq!(url, "");
    }

    // ---- extract_body ----

    #[test]
    fn extract_body_returns_body_after_blank_line() {
        let content = "POST https://x.com/y\nContent-Type: application/json\n\n{\"a\":1}\n";
        assert_eq!(extract_body(content), Some(r#"{"a":1}"#.to_string()));
    }

    #[test]
    fn extract_body_returns_none_when_no_body() {
        assert_eq!(extract_body("GET https://x.com/y\n"), None);
    }

    #[test]
    fn extract_body_stops_at_separator() {
        let content = "POST https://x.com/y\n\n{\"a\":1}\n###\nGET https://x.com/z\n";
        assert_eq!(extract_body(content), Some(r#"{"a":1}"#.to_string()));
    }

    // ---- split_url ----

    #[test]
    fn split_url_separates_origin_from_path() {
        assert_eq!(
            split_url("https://api.example.com/v1/users?id=1"),
            ("https://api.example.com".to_string(), "/v1/users?id=1".to_string())
        );
    }

    #[test]
    fn split_url_handles_no_path() {
        assert_eq!(
            split_url("https://api.example.com"),
            ("https://api.example.com".to_string(), "/".to_string())
        );
    }

    #[test]
    fn split_url_handles_relative_paths() {
        assert_eq!(split_url("/api/users"), ("".to_string(), "/api/users".to_string()));
        assert_eq!(split_url("api/users"), ("".to_string(), "/api/users".to_string()));
    }

    // ---- transform_path (OpenAPI {param} → Dante {{param}}) ----

    #[test]
    fn transform_path_converts_openapi_params_to_dante_vars() {
        assert_eq!(
            transform_path("https://api.example.com", "/users/{id}/posts/{postId}"),
            "https://api.example.com/users/{{id}}/posts/{{postId}}"
        );
    }

    #[test]
    fn transform_path_passes_through_static_paths() {
        assert_eq!(
            transform_path("https://api.example.com", "/health"),
            "https://api.example.com/health"
        );
    }

    // ---- url_path_with_query ----

    #[test]
    fn url_path_with_query_preserves_query_string() {
        assert_eq!(
            url_path_with_query("https://api.example.com/users?limit=10&offset=20"),
            "/users?limit=10&offset=20"
        );
    }

    #[test]
    fn url_path_with_query_returns_root_for_invalid_url() {
        assert_eq!(url_path_with_query("not-a-url"), "/");
    }

    #[test]
    fn url_path_with_query_returns_just_path_when_no_query() {
        assert_eq!(
            url_path_with_query("https://api.example.com/users"),
            "/users"
        );
    }

    // ---- strip_quotes ----

    #[test]
    fn strip_quotes_removes_surrounding_quotes() {
        assert_eq!(strip_quotes(r#""hello""#), "hello");
        assert_eq!(strip_quotes("'world'"), "world");
        assert_eq!(strip_quotes("no quotes"), "no quotes");
    }

    #[test]
    fn strip_quotes_does_not_remove_mismatched() {
        assert_eq!(strip_quotes(r#""mismatched'"#), r#""mismatched'"#);
    }

    #[test]
    fn strip_quotes_handles_short_strings() {
        assert_eq!(strip_quotes(""), "");
        assert_eq!(strip_quotes("\""), "\"");
        assert_eq!(strip_quotes("\"\""), "");
    }

    // ---- unique_path ----

    #[test]
    fn unique_path_appends_counter_when_collision() {
        let dir = std::env::temp_dir().join(format!(
            "dante-unique-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();

        let p1 = unique_path(&dir, "users", "http");
        assert_eq!(p1.file_name().unwrap(), "users.http");
        fs::write(&p1, "x").unwrap();

        let p2 = unique_path(&dir, "users", "http");
        assert_eq!(p2.file_name().unwrap(), "users-2.http");
        fs::write(&p2, "x").unwrap();

        let p3 = unique_path(&dir, "users", "http");
        assert_eq!(p3.file_name().unwrap(), "users-3.http");

        let _ = fs::remove_dir_all(&dir);
    }

    // ---- OAuth flow form bodies (build via reqwest's form encoding) ----
    // We can't easily call the async fns directly (they need tauri::State + cookie jar),
    // but we can verify the form-encoded output that reqwest produces from the same
    // input vectors used inside fetch_oauth_token / fetch_oauth_password / etc.

    fn encode_form(form: &[(&str, String)]) -> String {
        // Minimal application/x-www-form-urlencoded encoder matching what reqwest's
        // .form() ultimately produces — spaces as +, RFC 3986 reserved as %xx.
        use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
        const FORM: &AsciiSet = &CONTROLS
            .add(b' ')
            .add(b'!')
            .add(b'"')
            .add(b'#')
            .add(b'$')
            .add(b'%')
            .add(b'&')
            .add(b'\'')
            .add(b'(')
            .add(b')')
            .add(b'*')
            .add(b'+')
            .add(b',')
            .add(b'/')
            .add(b':')
            .add(b';')
            .add(b'=')
            .add(b'?')
            .add(b'@');
        form.iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    utf8_percent_encode(k, FORM),
                    utf8_percent_encode(v, FORM)
                )
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    #[test]
    fn client_credentials_form_shape() {
        // Mirrors fetch_oauth_token's form construction (lib.rs:1749-1758)
        let form: Vec<(&str, String)> = vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", "my-client".to_string()),
            ("client_secret", "my-secret".to_string()),
            ("scope", "read users".to_string()), // space => + or %20
        ];
        let encoded = encode_form(&form);
        assert!(encoded.contains("grant_type=client_credentials"), "encoded: {encoded}");
        assert!(encoded.contains("client_id=my-client"));
        assert!(encoded.contains("client_secret=my-secret"));
        // space must be encoded
        assert!(!encoded.contains("read users"), "space should be encoded");
        assert!(encoded.contains("scope=read%20users"));
    }

    #[test]
    fn refresh_token_form_shape() {
        // Mirrors fetch_oauth_refresh's form construction
        let form: Vec<(&str, String)> = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", "old-refresh-value".to_string()),
            ("client_id", "my-client".to_string()),
        ];
        let encoded = encode_form(&form);
        assert!(encoded.contains("grant_type=refresh_token"));
        assert!(encoded.contains("refresh_token=old-refresh-value"));
        assert!(encoded.contains("client_id=my-client"));
    }

    #[test]
    fn password_grant_form_shape() {
        // Mirrors fetch_oauth_password's form construction
        let form: Vec<(&str, String)> = vec![
            ("grant_type", "password".to_string()),
            ("username", "alice@example.com".to_string()),
            ("password", "p@$$w0rd!".to_string()),
            ("client_id", "my-client".to_string()),
        ];
        let encoded = encode_form(&form);
        assert!(encoded.contains("grant_type=password"));
        // @ in username must be percent-encoded for safety in form bodies
        assert!(encoded.contains("alice%40example.com"));
        // Special chars in password must be encoded
        assert!(!encoded.contains("p@$$"), "password special chars must be encoded");
        assert!(encoded.contains("p%40%24%24w0rd%21"));
    }

    #[test]
    fn form_encoding_round_trips_for_safe_values() {
        let form: Vec<(&str, String)> = vec![
            ("a", "simple".to_string()),
            ("b", "value-with-dashes".to_string()),
        ];
        let encoded = encode_form(&form);
        // Safe alphanumerics + - . _ ~ must NOT be percent-encoded
        assert_eq!(encoded, "a=simple&b=value-with-dashes");
    }

    // ---- Importer integration tests ----

    fn temp_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dante-import-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn import_har_creates_http_files_with_headers_and_body() {
        let dir = temp_test_dir("har");
        let har_path = dir.join("session.har");
        fs::write(
            &har_path,
            r#"{
                "log": {
                    "entries": [
                        {
                            "request": {
                                "method": "GET",
                                "url": "https://api.example.com/users",
                                "headers": [
                                    {"name": "Accept", "value": "application/json"},
                                    {"name": ":authority", "value": "should-be-skipped"}
                                ]
                            }
                        },
                        {
                            "request": {
                                "method": "POST",
                                "url": "https://api.example.com/users",
                                "headers": [{"name": "Content-Type", "value": "application/json"}],
                                "postData": {"text": "{\"name\":\"alice\"}"}
                            }
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = import_har(
            dir.to_string_lossy().to_string(),
            har_path.to_string_lossy().to_string(),
        )
        .expect("import should succeed");

        assert_eq!(result.created.len(), 2, "expected 2 files");
        let import_dir = dir.join("har-import");
        let entries: Vec<_> = fs::read_dir(&import_dir).unwrap().flatten().collect();
        assert_eq!(entries.len(), 2);

        // Verify content of one of the files
        let mut found_post = false;
        for entry in entries {
            let content = fs::read_to_string(entry.path()).unwrap();
            if content.starts_with("POST") {
                found_post = true;
                assert!(content.contains("https://api.example.com/users"));
                assert!(content.contains("Content-Type: application/json"));
                assert!(content.contains(r#"{"name":"alice"}"#));
                // The pseudo-header `:authority` must be skipped (HTTP/2 internal)
                assert!(!content.contains(":authority"));
            }
        }
        assert!(found_post, "POST file should exist");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_har_returns_error_on_missing_log_entries() {
        let dir = temp_test_dir("har-bad");
        let har_path = dir.join("bad.har");
        fs::write(&har_path, r#"{"not_har": true}"#).unwrap();
        let err = import_har(
            dir.to_string_lossy().to_string(),
            har_path.to_string_lossy().to_string(),
        )
        .expect_err("should fail on missing /log/entries");
        assert!(err.contains("entries"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_insomnia_groups_requests_into_subfolders() {
        let dir = temp_test_dir("insomnia");
        let spec_path = dir.join("insomnia.json");
        fs::write(
            &spec_path,
            r#"{
                "resources": [
                    {"_type": "request_group", "_id": "g1", "name": "Authentication"},
                    {
                        "_type": "request",
                        "_id": "r1",
                        "name": "Login",
                        "method": "POST",
                        "url": "https://api.example.com/login",
                        "parentId": "g1",
                        "body": {"text": "{\"username\":\"alice\"}"}
                    },
                    {
                        "_type": "request",
                        "_id": "r2",
                        "name": "Health Check",
                        "method": "GET",
                        "url": "https://api.example.com/health",
                        "parentId": ""
                    }
                ]
            }"#,
        )
        .unwrap();

        let result = import_insomnia(
            dir.to_string_lossy().to_string(),
            spec_path.to_string_lossy().to_string(),
        )
        .expect("import should succeed");
        assert_eq!(result.created.len(), 2);

        let import_dir = dir.join("insomnia-import");
        let auth_dir = import_dir.join("authentication");
        assert!(auth_dir.exists(), "auth subfolder should exist");
        let login_file = auth_dir.join("login.http");
        assert!(login_file.exists(), "login.http should exist in auth/");
        let login_content = fs::read_to_string(&login_file).unwrap();
        assert!(login_content.contains("# Login"));
        assert!(login_content.contains("POST https://api.example.com/login"));
        assert!(login_content.contains(r#"{"username":"alice"}"#));

        let health_file = import_dir.join("health-check.http");
        assert!(health_file.exists(), "ungrouped request goes to root");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_postman_creates_collection_folder() {
        let dir = temp_test_dir("postman");
        let spec_path = dir.join("collection.json");
        fs::write(
            &spec_path,
            r#"{
                "info": {"name": "My API"},
                "item": [
                    {
                        "name": "List Users",
                        "request": {
                            "method": "GET",
                            "url": {"raw": "https://api.example.com/users?limit=10"},
                            "header": [{"key": "Accept", "value": "application/json"}]
                        }
                    },
                    {
                        "name": "Auth",
                        "item": [
                            {
                                "name": "Login",
                                "request": {
                                    "method": "POST",
                                    "url": {"raw": "https://api.example.com/login"},
                                    "body": {"raw": "{\"u\":\"a\"}"}
                                }
                            }
                        ]
                    }
                ]
            }"#,
        )
        .unwrap();

        let result = import_postman(
            dir.to_string_lossy().to_string(),
            spec_path.to_string_lossy().to_string(),
        )
        .expect("import should succeed");
        assert!(result.created.len() >= 2);

        let collection_dir = dir.join("my-api");
        assert!(collection_dir.exists());
        // List Users at top level
        let mut found_list = false;
        let mut found_login = false;
        for created in &result.created {
            let content = fs::read_to_string(created).unwrap();
            if content.contains("https://api.example.com/users") {
                found_list = true;
            }
            if content.contains("https://api.example.com/login") {
                found_login = true;
                assert!(content.contains(r#"{"u":"a"}"#));
            }
        }
        assert!(found_list && found_login);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_openapi_generates_request_per_operation() {
        let dir = temp_test_dir("openapi");
        let spec_path = dir.join("openapi.json");
        fs::write(
            &spec_path,
            r#"{
                "openapi": "3.0.0",
                "info": {"title": "Test API", "version": "1.0"},
                "servers": [{"url": "https://api.example.com/v1"}],
                "paths": {
                    "/users": {
                        "get": {
                            "summary": "List users",
                            "operationId": "listUsers"
                        },
                        "post": {
                            "summary": "Create user",
                            "operationId": "createUser",
                            "requestBody": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "type": "object",
                                            "properties": {"name": {"type": "string"}},
                                            "required": ["name"]
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "/users/{id}": {
                        "get": {
                            "summary": "Get user",
                            "parameters": [
                                {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                            ]
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let result = import_openapi(
            dir.to_string_lossy().to_string(),
            spec_path.to_string_lossy().to_string(),
        )
        .expect("import should succeed");
        assert!(result.created.len() >= 3, "expected 3+ operations: got {}", result.created.len());

        let mut found_get_users = false;
        let mut found_post_users = false;
        let mut found_get_user_by_id = false;
        for created in &result.created {
            let content = fs::read_to_string(created).unwrap();
            // Path templating: OpenAPI {id} should become Dante {{id}}
            if content.contains("GET https://api.example.com/v1/users/{{id}}") {
                found_get_user_by_id = true;
            } else if content.contains("GET https://api.example.com/v1/users\n")
                || content.contains("GET https://api.example.com/v1/users\r\n")
            {
                found_get_users = true;
            } else if content.contains("POST https://api.example.com/v1/users\n")
                || content.contains("POST https://api.example.com/v1/users\r\n")
            {
                found_post_users = true;
            }
        }
        assert!(found_get_users, "GET /users missing in: {:?}", result.created);
        assert!(found_post_users, "POST /users missing");
        assert!(found_get_user_by_id, "GET /users/{{id}} missing or path not transformed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_openapi_supports_yaml() {
        let dir = temp_test_dir("openapi-yaml");
        let spec_path = dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            r#"openapi: 3.0.0
info:
  title: YAML API
  version: '1.0'
servers:
  - url: https://api.example.com
paths:
  /ping:
    get:
      summary: Ping
"#,
        )
        .unwrap();

        let result = import_openapi(
            dir.to_string_lossy().to_string(),
            spec_path.to_string_lossy().to_string(),
        )
        .expect("yaml import should succeed");
        assert_eq!(result.created.len(), 1);
        let content = fs::read_to_string(&result.created[0]).unwrap();
        assert!(content.contains("GET https://api.example.com/ping"));
        let _ = fs::remove_dir_all(&dir);
    }

    // ---- Postman var substitution helper ----

    #[test]
    fn postman_substitute_vars_passes_template_format() {
        // Postman uses {{var}} same as Dante; the helper today is a no-op
        assert_eq!(postman_substitute_vars("{{token}}"), "{{token}}");
        assert_eq!(postman_substitute_vars("https://{{host}}/api"), "https://{{host}}/api");
    }

    // ---- OAuth end-to-end against a real local token endpoint ----

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Run an HTTP server on a random port that responds with `response_body` (status 200)
    /// and captures the request body bytes + content-type into the returned channel.
    /// Returns (port, recv_for_captured_body, recv_for_captured_content_type, stop_flag).
    fn spawn_token_endpoint(
        response_body: String,
        response_status: u16,
    ) -> (
        u16,
        std::sync::mpsc::Receiver<String>,
        std::sync::mpsc::Receiver<String>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        use std::sync::Arc;
        use tiny_http::{Header, Response, Server};

        let port = free_port();
        let (body_tx, body_rx) = mpsc::channel::<String>();
        let (ct_tx, ct_rx) = mpsc::channel::<String>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        std::thread::spawn(move || {
            let server = Server::http(format!("127.0.0.1:{port}")).unwrap();
            loop {
                if stop_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                match server.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(Some(mut req)) => {
                        let ct = req
                            .headers()
                            .iter()
                            .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("content-type"))
                            .map(|h| h.value.as_str().to_string())
                            .unwrap_or_default();
                        let mut body = String::new();
                        let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
                        let _ = body_tx.send(body);
                        let _ = ct_tx.send(ct);

                        let mut resp = Response::from_string(&response_body)
                            .with_status_code(response_status as i32);
                        if let Ok(h) = Header::from_bytes(
                            b"Content-Type" as &[u8],
                            b"application/json" as &[u8],
                        ) {
                            resp = resp.with_header(h);
                        }
                        let _ = req.respond(resp);
                    }
                    Ok(None) => continue,
                    Err(_) => break,
                }
            }
        });

        // Give the server a moment to enter the recv loop
        std::thread::sleep(std::time::Duration::from_millis(50));
        (port, body_rx, ct_rx, stop)
    }

    #[tokio::test]
    async fn post_token_form_success_full_roundtrip() {
        let canned = r#"{"access_token":"abc123","token_type":"Bearer","expires_in":3600}"#;
        let (port, body_rx, ct_rx, stop) =
            spawn_token_endpoint(canned.to_string(), 200);

        let client = reqwest::Client::builder().build().unwrap();
        let form: Vec<(&str, String)> = vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", "my-client-id".to_string()),
            ("client_secret", "shhh secret!".to_string()),
        ];
        let result = post_token_form(
            &client,
            &format!("http://127.0.0.1:{port}/token"),
            &form,
            "test",
        )
        .await
        .expect("token request should succeed");

        // Verify response was parsed
        assert_eq!(result["access_token"], "abc123");
        assert_eq!(result["token_type"], "Bearer");
        assert_eq!(result["expires_in"], 3600);

        // Verify the request body our code sent
        let received_body = body_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let received_ct = ct_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert!(
            received_ct.contains("application/x-www-form-urlencoded"),
            "wrong content-type: {received_ct}"
        );
        assert!(received_body.contains("grant_type=client_credentials"));
        assert!(received_body.contains("client_id=my-client-id"));
        // Special chars must be percent-encoded
        assert!(
            received_body.contains("client_secret=shhh+secret%21")
                || received_body.contains("client_secret=shhh%20secret%21"),
            "secret was not properly encoded: {received_body}"
        );
        assert!(!received_body.contains("shhh secret!"), "raw value leaked");

        stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[tokio::test]
    async fn post_token_form_propagates_4xx_with_body() {
        let canned = r#"{"error":"invalid_client","error_description":"Bad client_id"}"#;
        let (port, _body_rx, _ct_rx, stop) =
            spawn_token_endpoint(canned.to_string(), 401);

        let client = reqwest::Client::builder().build().unwrap();
        let form: Vec<(&str, String)> = vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", "bad".to_string()),
        ];
        let err = post_token_form(
            &client,
            &format!("http://127.0.0.1:{port}/token"),
            &form,
            "client_credentials",
        )
        .await
        .expect_err("should fail on 401");

        assert!(err.contains("client_credentials"), "missing label: {err}");
        assert!(err.contains("401"), "missing status: {err}");
        assert!(err.contains("invalid_client"), "missing body: {err}");

        stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[tokio::test]
    async fn post_token_form_rejects_non_json_response() {
        let (port, _, _, stop) = spawn_token_endpoint("not json".to_string(), 200);

        let client = reqwest::Client::builder().build().unwrap();
        let form: Vec<(&str, String)> = vec![("grant_type", "x".to_string())];
        let err = post_token_form(
            &client,
            &format!("http://127.0.0.1:{port}/token"),
            &form,
            "test",
        )
        .await
        .expect_err("should fail to parse");
        assert!(err.contains("parse"), "expected parse error: {err}");

        stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[tokio::test]
    async fn post_token_form_handles_password_grant_with_special_chars() {
        // Mirrors fetch_oauth_password's form. Password contains chars that BREAK auth
        // if not properly url-encoded. This test catches that.
        let canned = r#"{"access_token":"pwd-grant-token","token_type":"Bearer"}"#;
        let (port, body_rx, _, stop) =
            spawn_token_endpoint(canned.to_string(), 200);

        let client = reqwest::Client::builder().build().unwrap();
        let form: Vec<(&str, String)> = vec![
            ("grant_type", "password".to_string()),
            ("client_id", "my-app".to_string()),
            ("username", "alice@example.com".to_string()),
            ("password", "p@ss w0rd!&".to_string()), // every char is dangerous
        ];
        let result = post_token_form(
            &client,
            &format!("http://127.0.0.1:{port}/token"),
            &form,
            "password",
        )
        .await
        .unwrap();
        assert_eq!(result["access_token"], "pwd-grant-token");

        let received_body = body_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        // `@`, ` `, `!`, `&` must all be encoded; raw password must NOT appear
        assert!(!received_body.contains("p@ss w0rd!&"), "raw password leaked: {received_body}");
        assert!(received_body.contains("alice%40example.com"));
        // Verify password param is round-trip safe by parsing the form back
        let mut params: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for pair in received_body.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                let decoded = percent_encoding::percent_decode_str(&v.replace('+', " "))
                    .decode_utf8_lossy()
                    .to_string();
                params.insert(k.to_string(), decoded);
            }
        }
        assert_eq!(params.get("password").map(String::as_str), Some("p@ss w0rd!&"));
        assert_eq!(params.get("username").map(String::as_str), Some("alice@example.com"));

        stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
