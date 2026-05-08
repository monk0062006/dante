use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Serialize)]
#[serde(tag = "kind")]
pub enum WsEvent {
    Connected { id: String },
    Message { id: String, direction: String, text: String, ts_ms: u64 },
    Closed { id: String, reason: Option<String> },
    Error { id: String, message: String },
}

pub struct WsConnection {
    pub send_tx: mpsc::UnboundedSender<Message>,
}

pub type WsConnections = Arc<Mutex<HashMap<String, WsConnection>>>;

pub fn new_registry() -> WsConnections {
    Arc::new(Mutex::new(HashMap::new()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn connect(
    url: String,
    id: String,
    app: AppHandle,
    registry: WsConnections,
) -> Result<(), String> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("connect: {e}"))?;

    let (mut writer, mut reader) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    {
        let mut map = registry.lock().map_err(|e| e.to_string())?;
        map.insert(id.clone(), WsConnection { send_tx: tx });
    }

    let _ = app.emit("ws", WsEvent::Connected { id: id.clone() });

    let id_for_writer = id.clone();
    let app_for_writer = app.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let is_close = matches!(msg, Message::Close(_));
            let text_repr = match &msg {
                Message::Text(t) => Some(t.to_string()),
                Message::Binary(b) => Some(format!("<binary {} bytes>", b.len())),
                Message::Ping(_) => Some("<ping>".to_string()),
                Message::Pong(_) => Some("<pong>".to_string()),
                Message::Close(_) => Some("<close>".to_string()),
                _ => None,
            };
            if let Err(e) = writer.send(msg).await {
                let _ = app_for_writer.emit(
                    "ws",
                    WsEvent::Error {
                        id: id_for_writer.clone(),
                        message: format!("send: {e}"),
                    },
                );
                break;
            }
            if let Some(text) = text_repr {
                let _ = app_for_writer.emit(
                    "ws",
                    WsEvent::Message {
                        id: id_for_writer.clone(),
                        direction: "out".to_string(),
                        text,
                        ts_ms: now_ms(),
                    },
                );
            }
            if is_close {
                break;
            }
        }
    });

    let id_for_reader = id.clone();
    let registry_for_reader = registry.clone();
    let app_for_reader = app.clone();
    tokio::spawn(async move {
        while let Some(msg) = reader.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let _ = app_for_reader.emit(
                        "ws",
                        WsEvent::Message {
                            id: id_for_reader.clone(),
                            direction: "in".to_string(),
                            text: text.to_string(),
                            ts_ms: now_ms(),
                        },
                    );
                }
                Ok(Message::Binary(b)) => {
                    let _ = app_for_reader.emit(
                        "ws",
                        WsEvent::Message {
                            id: id_for_reader.clone(),
                            direction: "in".to_string(),
                            text: format!("<binary {} bytes>", b.len()),
                            ts_ms: now_ms(),
                        },
                    );
                }
                Ok(Message::Close(frame)) => {
                    let reason = frame.map(|f| f.reason.to_string());
                    let _ = app_for_reader.emit(
                        "ws",
                        WsEvent::Closed {
                            id: id_for_reader.clone(),
                            reason,
                        },
                    );
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    let _ = app_for_reader.emit(
                        "ws",
                        WsEvent::Error {
                            id: id_for_reader.clone(),
                            message: format!("recv: {e}"),
                        },
                    );
                    break;
                }
            }
        }
        if let Ok(mut map) = registry_for_reader.lock() {
            map.remove(&id_for_reader);
        }
    });

    Ok(())
}

pub fn send(
    registry: &WsConnections,
    id: &str,
    text: String,
) -> Result<(), String> {
    let map = registry.lock().map_err(|e| e.to_string())?;
    let conn = map.get(id).ok_or_else(|| "connection not found".to_string())?;
    conn.send_tx
        .send(Message::Text(text.into()))
        .map_err(|e| format!("send: {e}"))
}

pub fn close(registry: &WsConnections, id: &str) -> Result<(), String> {
    let mut map = registry.lock().map_err(|e| e.to_string())?;
    if let Some(conn) = map.remove(id) {
        let _ = conn.send_tx.send(Message::Close(None));
    }
    Ok(())
}
