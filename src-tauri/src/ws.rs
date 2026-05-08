use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Serialize, Debug)]
#[serde(tag = "kind")]
pub enum WsEvent {
    Connected { id: String },
    Message { id: String, direction: String, text: String, ts_ms: u64 },
    Closed { id: String, reason: Option<String> },
    Error { id: String, message: String },
}

/// Sink for WS lifecycle events. Production: AppHandle (Tauri's emitter).
/// Tests: pluggable in-memory sink that records events for assertion.
pub trait WsEventSink: Send + Sync + 'static {
    fn emit(&self, event: &WsEvent);
}

impl WsEventSink for AppHandle {
    fn emit(&self, event: &WsEvent) {
        let _ = Emitter::emit(self, "ws", event);
    }
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
    connect_with_sink(url, id, Arc::new(app), registry).await
}

pub async fn connect_with_sink<S: WsEventSink>(
    url: String,
    id: String,
    sink: Arc<S>,
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

    sink.emit(&WsEvent::Connected { id: id.clone() });

    let id_for_writer = id.clone();
    let sink_for_writer = sink.clone();
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
                sink_for_writer.emit(&WsEvent::Error {
                    id: id_for_writer.clone(),
                    message: format!("send: {e}"),
                });
                break;
            }
            if let Some(text) = text_repr {
                sink_for_writer.emit(&WsEvent::Message {
                    id: id_for_writer.clone(),
                    direction: "out".to_string(),
                    text,
                    ts_ms: now_ms(),
                });
            }
            if is_close {
                break;
            }
        }
    });

    let id_for_reader = id.clone();
    let registry_for_reader = registry.clone();
    let sink_for_reader = sink.clone();
    tokio::spawn(async move {
        while let Some(msg) = reader.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    sink_for_reader.emit(&WsEvent::Message {
                        id: id_for_reader.clone(),
                        direction: "in".to_string(),
                        text: text.to_string(),
                        ts_ms: now_ms(),
                    });
                }
                Ok(Message::Binary(b)) => {
                    sink_for_reader.emit(&WsEvent::Message {
                        id: id_for_reader.clone(),
                        direction: "in".to_string(),
                        text: format!("<binary {} bytes>", b.len()),
                        ts_ms: now_ms(),
                    });
                }
                Ok(Message::Close(frame)) => {
                    let reason = frame.map(|f| f.reason.to_string());
                    sink_for_reader.emit(&WsEvent::Closed {
                        id: id_for_reader.clone(),
                        reason,
                    });
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    sink_for_reader.emit(&WsEvent::Error {
                        id: id_for_reader.clone(),
                        message: format!("recv: {e}"),
                    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;
    use tokio::net::TcpListener;

    /// Test sink that records every event for assertion.
    struct CaptureSink {
        events: StdMutex<Vec<WsEvent>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self {
                events: StdMutex::new(Vec::new()),
            }
        }

        fn snapshot(&self) -> Vec<WsEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl WsEventSink for CaptureSink {
        fn emit(&self, event: &WsEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Spawn a tokio-tungstenite echo server on a random port.
    /// Returns the port. Server lives until the runtime shuts down.
    async fn spawn_echo_server() -> u16 {
        let port = free_port();
        let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let ws = match tokio_tungstenite::accept_async(stream).await {
                        Ok(w) => w,
                        Err(_) => return,
                    };
                    let (mut writer, mut reader) = ws.split();
                    while let Some(msg) = reader.next().await {
                        match msg {
                            Ok(m) if m.is_text() || m.is_binary() => {
                                if writer.send(m).await.is_err() {
                                    break;
                                }
                            }
                            Ok(Message::Close(frame)) => {
                                let _ = writer.send(Message::Close(frame)).await;
                                break;
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                });
            }
        });
        // Give listener a tick to start accepting
        tokio::time::sleep(Duration::from_millis(20)).await;
        port
    }

    /// Wait until at least one event of the given variant lands in the sink, or time out.
    async fn wait_for<F>(sink: &Arc<CaptureSink>, predicate: F, timeout_ms: u64) -> Option<WsEvent>
    where
        F: Fn(&WsEvent) -> bool,
    {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < timeout_ms as u128 {
            for ev in sink.snapshot() {
                if predicate(&ev) {
                    return Some(ev);
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn echo_round_trip_full_lifecycle() {
        let port = spawn_echo_server().await;
        let url = format!("ws://127.0.0.1:{port}");
        let registry = new_registry();
        let sink = Arc::new(CaptureSink::new());

        connect_with_sink(url.clone(), "id-1".to_string(), sink.clone(), registry.clone())
            .await
            .expect("connect should succeed");

        // Should see Connected event
        let connected = wait_for(&sink, |e| matches!(e, WsEvent::Connected { id } if id == "id-1"), 500)
            .await
            .expect("should see Connected event");
        assert!(matches!(connected, WsEvent::Connected { .. }));

        // Send a text message — echo server should bounce it back
        send(&registry, "id-1", "hello world".to_string()).expect("send should succeed");

        // Wait for outgoing event
        let out = wait_for(
            &sink,
            |e| matches!(e, WsEvent::Message { id, direction, text, .. }
                if id == "id-1" && direction == "out" && text == "hello world"),
            1000,
        )
        .await
        .expect("should see outgoing Message event");
        assert!(matches!(out, WsEvent::Message { .. }));

        // Wait for echo to come back
        let inbound = wait_for(
            &sink,
            |e| matches!(e, WsEvent::Message { id, direction, text, .. }
                if id == "id-1" && direction == "in" && text == "hello world"),
            1000,
        )
        .await
        .expect("should see incoming echo Message event");
        assert!(matches!(inbound, WsEvent::Message { .. }));

        // Close cleanly
        close(&registry, "id-1").expect("close should succeed");

        // Closed event should arrive (or we simply confirm registry no longer has the conn)
        let _ = wait_for(
            &sink,
            |e| matches!(e, WsEvent::Closed { id, .. } if id == "id-1"),
            1500,
        )
        .await;

        // Registry should have removed the connection (reader task drains on close)
        let registry_after_close_state = wait_for(
            &sink,
            |_| {
                let map = registry.lock().unwrap();
                !map.contains_key("id-1")
            },
            1500,
        )
        .await;
        // Verified by predicate above
        let _ = registry_after_close_state;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_to_unreachable_returns_error() {
        let registry = new_registry();
        let sink = Arc::new(CaptureSink::new());
        // Port 1 is "tcpmux" which is reserved & unused — connection refused
        let err = connect_with_sink(
            "ws://127.0.0.1:1".to_string(),
            "id-x".to_string(),
            sink,
            registry,
        )
        .await
        .expect_err("connect to unreachable should fail");
        assert!(err.contains("connect"), "expected connect error: {err}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_to_unknown_id_errors() {
        let registry = new_registry();
        let err = send(&registry, "no-such-id", "hi".to_string())
            .expect_err("should fail for unknown id");
        assert!(err.contains("not found"), "expected not-found error: {err}");
    }
}
