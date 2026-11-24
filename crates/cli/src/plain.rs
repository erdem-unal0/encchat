//! The original line-based client, kept as `encchat chat --plain`.
//!
//! It is also used automatically when stdout is not a terminal, so scripts and
//! the smoke test keep working. Output matches the pre-TUI client exactly.

use anyhow::{Context, Result};
use encchat_core::{decode, encode, ClientFrame, DeviceId, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    chat::Session,
    net::{self, UiEvent},
    state,
};

pub async fn run(session: Session) -> Result<()> {
    let Session {
        server,
        mut core,
        key,
        state_path,
        fingerprint,
        room_hex: _,
        config: _,
        config_path: _,
        config_warnings: _,
    } = session;

    println!("Room fingerprint: {fingerprint}");
    println!("This device:      {}", state::hex(&core.device_id()));
    if core.session_count() > 0 {
        println!(
            "Restored {} session(s) from local state.",
            core.session_count()
        );
    }

    let (ws, _) = tokio_tungstenite::connect_async(&server)
        .await
        .with_context(|| format!("could not connect to {server}"))?;
    let (mut sink, mut stream) = ws.split();

    sink.send(Message::Binary(encode(&core.join_frame())?.into()))
        .await?;
    println!("Connected to {server}. Type a message, or /quit to exit.");

    // A blocking stdin reader feeds lines to the async loop.
    let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut buf = String::new();
        loop {
            buf.clear();
            match stdin.read_line(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if line_tx.send(buf.trim_end().to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut peers: Vec<DeviceId> = Vec::new();

    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        let frame: ServerFrame = decode(&bytes)?;
                        let outcome = net::handle_server_frame(frame, &mut core, &mut peers)?;
                        for event in &outcome.events {
                            match event {
                                UiEvent::Incoming { from, body } => {
                                    println!("[{}] {body}", state::short(from));
                                }
                                UiEvent::System(text) => println!("{text}"),
                            }
                        }
                        for bytes in outcome.sends {
                            sink.send(Message::Binary(bytes.into())).await?;
                        }
                        state::persist(&key, &core, &state_path)?;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        println!("Disconnected from the relay.");
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        eprintln!("connection error: {e}");
                        break;
                    }
                }
            }
            line = line_rx.recv() => {
                match line {
                    Some(text) => {
                        let text = text.trim();
                        if text.is_empty() {
                            continue;
                        }
                        if text == "/quit" || text == "/exit" {
                            break;
                        }
                        match core.send_text_to_all(&peers, text, state::now_ms()) {
                            Ok(frames) => {
                                for (to, frame) in frames {
                                    let payload = encode(&frame)?;
                                    let relay = encode(&ClientFrame::Relay { to, payload })?;
                                    sink.send(Message::Binary(relay.into())).await?;
                                }
                                println!("[me] {text}");
                                state::persist(&key, &core, &state_path)?;
                            }
                            Err(e) => println!("(cannot send yet: {e})"),
                        }
                    }
                    None => break,
                }
            }
        }
    }

    state::persist(&key, &core, &state_path)?;
    Ok(())
}
