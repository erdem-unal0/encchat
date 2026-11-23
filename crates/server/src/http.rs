//! HTTP/WebSocket front end for the relay.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use encchat_core::{decode, encode, ClientFrame, DeviceId, RoomId, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

use crate::{MailboxConfig, Relay};

#[derive(Clone)]
struct AppState {
    relay: Arc<Mutex<Relay>>,
    max_payload: usize,
}

/// Build the relay's router. Exposed so tests can serve it on an ephemeral port.
pub fn build_router(config: MailboxConfig, max_payload: usize) -> Router {
    let state = AppState {
        relay: Arc::new(Mutex::new(Relay::new(config))),
        max_payload,
    };
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/", get(ws_handler))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// Convenience wrapper for the binary: `MailboxConfig` from plain seconds/count.
pub fn default_config(ttl_secs: u64, max_entries: usize) -> MailboxConfig {
    MailboxConfig {
        max_entries,
        ttl: Duration::from_secs(ttl_secs),
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

fn send_error(tx: &UnboundedSender<Vec<u8>>, code: &str, message: &str) {
    let frame = ServerFrame::Error {
        code: code.to_string(),
        message: message.to_string(),
    };
    let _ = tx.send(encode(&frame).expect("Error frame is always encodable"));
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    // All writes go through `tx`, so the read loop never borrows the sink.
    let writer = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if sink.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    let mut current: Option<(RoomId, DeviceId)> = None;

    while let Some(Ok(message)) = stream.next().await {
        match message {
            Message::Binary(bytes) => {
                let frame: ClientFrame = match decode(&bytes) {
                    Ok(frame) => frame,
                    Err(_) => {
                        send_error(&tx, "bad_frame", "could not decode frame");
                        continue;
                    }
                };

                match frame {
                    ClientFrame::Join { room, device, mac } => {
                        let outcome = {
                            let mut relay = state.relay.lock().expect("relay mutex poisoned");
                            relay.join(room, device, mac, tx.clone())
                        };
                        for frame in outcome.to_self {
                            let _ = tx.send(frame);
                        }
                        for (peer_tx, frame) in outcome.to_others {
                            let _ = peer_tx.send(frame);
                        }
                        current = Some((room, device));
                        info!(device = %hex16(&device), "device joined");
                    }
                    ClientFrame::Devices { room } => {
                        let devices = {
                            let relay = state.relay.lock().expect("relay mutex poisoned");
                            relay.devices(room)
                        };
                        let frame = encode(&ServerFrame::Devices { devices })
                            .expect("Devices frame is always encodable");
                        let _ = tx.send(frame);
                    }
                    ClientFrame::Relay { to, payload } => {
                        let Some((room, from)) = current else {
                            send_error(&tx, "not_joined", "join a room first");
                            continue;
                        };
                        if payload.len() > state.max_payload {
                            send_error(&tx, "too_large", "payload exceeds the size limit");
                            continue;
                        }
                        let mut relay = state.relay.lock().expect("relay mutex poisoned");
                        relay.deliver(room, from, to, payload);
                    }
                    ClientFrame::Ping => {
                        let frame =
                            encode(&ServerFrame::Pong).expect("Pong frame is always encodable");
                        let _ = tx.send(frame);
                    }
                }
            }
            Message::Text(_) => send_error(&tx, "bad_frame", "binary frames only"),
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => break,
        }
    }

    if let Some((room, device)) = current {
        let mut relay = state.relay.lock().expect("relay mutex poisoned");
        relay.disconnect(room, device);
        info!(device = %hex16(&device), "device disconnected");
    } else {
        warn!("websocket closed before joining");
    }

    writer.abort();
}

fn hex16(bytes: &[u8; 16]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
