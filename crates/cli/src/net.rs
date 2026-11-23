//! Transport-agnostic handling of relay frames.
//!
//! [`handle_server_frame`] turns one decoded [`ServerFrame`] into outgoing
//! relay bytes and UI events. Both the TUI and the `--plain` client use it, so
//! the protocol handling lives in exactly one place.

use anyhow::{Context, Result};
use encchat_core::{decode, encode, Action, ClientCore, ClientFrame, DeviceId, PeerFrame, ServerFrame};

use crate::state::{hex, short};

/// Something the UI should show after handling a server frame.
pub enum UiEvent {
    /// A decrypted message arrived from `from`.
    Incoming { from: DeviceId, body: String },
    /// A human-readable system/notice line.
    System(String),
}

/// Outgoing relay frames plus UI events produced by one server frame.
#[derive(Default)]
pub struct Outcome {
    pub sends: Vec<Vec<u8>>,
    pub events: Vec<UiEvent>,
}

/// Encode a peer frame wrapped in a `Relay` client frame.
pub fn relay_bytes(to: DeviceId, frame: &PeerFrame) -> Result<Vec<u8>> {
    let payload = encode(frame)?;
    Ok(encode(&ClientFrame::Relay { to, payload })?)
}

/// Translate one inbound server frame into outgoing frames and UI events.
pub fn handle_server_frame(
    frame: ServerFrame,
    core: &mut ClientCore,
    peers: &mut Vec<DeviceId>,
) -> Result<Outcome> {
    let mut out = Outcome::default();

    match frame {
        ServerFrame::Joined { devices, .. } => {
            let me = core.device_id();
            peers.clear();
            for entry in &devices {
                if entry.id == me {
                    continue;
                }
                if core.verify_device(entry) {
                    peers.push(entry.id);
                } else {
                    out.events.push(UiEvent::System(format!(
                        "! ignored device with invalid MAC: {}",
                        hex(&entry.id)
                    )));
                }
            }
            out.events.push(UiEvent::System(format!(
                "{} peer device(s) present.",
                peers.len()
            )));

            for &peer in peers.iter() {
                if core.should_initiate(peer) {
                    let frame = core.begin_handshake(peer)?;
                    out.sends.push(relay_bytes(peer, &frame)?);
                }
            }
        }
        ServerFrame::Devices { devices } => {
            let me = core.device_id();
            for entry in &devices {
                if entry.id == me || peers.contains(&entry.id) {
                    continue;
                }
                if core.verify_device(entry) {
                    peers.push(entry.id);
                    if core.should_initiate(entry.id) {
                        let frame = core.begin_handshake(entry.id)?;
                        out.sends.push(relay_bytes(entry.id, &frame)?);
                    }
                }
            }
        }
        ServerFrame::Deliver { from, payload } => {
            let peer_frame: PeerFrame = decode(&payload)
                .with_context(|| format!("bad peer frame from {}", hex(&from)))?;
            let actions = core.handle_frame(from, peer_frame)?;
            for action in actions {
                match action {
                    Action::Send { to, frame } => out.sends.push(relay_bytes(to, &frame)?),
                    Action::Received { from, body, .. } => {
                        out.events.push(UiEvent::Incoming { from, body });
                    }
                    Action::HandshakeComplete { with } => {
                        if !peers.contains(&with) {
                            peers.push(with);
                        }
                        out.events.push(UiEvent::System(format!(
                            "• secure session with {}",
                            short(&with)
                        )));
                        // A peer only learns our nickname from a session it is
                        // part of, so (re-)announce it now that one exists. The
                        // announcement is encrypted inside the ratchet.
                        if let Some(frame) = core.announce_nick(with) {
                            out.sends.push(relay_bytes(with, &frame)?);
                        }
                    }
                    Action::Notice(message) => out.events.push(UiEvent::System(message)),
                }
            }
        }
        ServerFrame::Error { code, message } => {
            out.events
                .push(UiEvent::System(format!("server error ({code}): {message}")));
        }
        ServerFrame::Pong => {}
    }

    Ok(out)
}
