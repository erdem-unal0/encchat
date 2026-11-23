use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    envelope::{decode, encode, ChatPayload, ClientFrame, PayloadKind, PeerFrame},
    error::{Error, Result},
    handshake::XxHandshake,
    psk::{DeviceId, Mac, Psk, RoomId},
    ratchet::DoubleRatchet,
};

/// Persisted ratchet state for one peer device.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub peer_device: DeviceId,
    pub ratchet: DoubleRatchet,
}

/// A decrypted message kept in the local, encrypted history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub peer_device: DeviceId,
    pub msg_id: [u8; 16],
    pub sent_at: u64,
    pub outgoing: bool,
    pub body: String,
}

/// Everything that must be persisted on a device, encrypted with the local
/// store key (see [`crate::store`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalState {
    pub version: u8,
    pub device_id: DeviceId,
    pub sessions: Vec<SessionRecord>,
    pub history: Vec<HistoryEntry>,
}

impl LocalState {
    pub fn new() -> Result<Self> {
        let mut device_id = [0u8; 16];
        getrandom::getrandom(&mut device_id).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
        Ok(Self {
            version: 1,
            device_id,
            sessions: Vec::new(),
            history: Vec::new(),
        })
    }
}

/// What the frontend should do after feeding an inbound frame to the core.
#[derive(Clone, Debug)]
pub enum Action {
    /// Send this encrypted peer frame to `to` through the relay.
    Send { to: DeviceId, frame: PeerFrame },
    /// A message was decrypted successfully.
    Received {
        from: DeviceId,
        msg_id: [u8; 16],
        sent_at: u64,
        body: String,
    },
    /// A handshake finished; a session is now available.
    HandshakeComplete { with: DeviceId },
    /// Human-readable status for the UI.
    Notice(String),
}

/// Protocol state machine shared by the CLI and the web app.
///
/// The frontend is responsible for transport (WebSocket), persistence (via
/// [`crate::store`]), and deciding which peer devices to fan out to.
pub struct ClientCore {
    psk: Psk,
    pub room: RoomId,
    pub state: LocalState,
    pending: HashMap<DeviceId, XxHandshake>,
}

impl ClientCore {
    pub fn new(psk: Psk) -> Result<Self> {
        let room = psk.room_id();
        Ok(Self {
            psk,
            room,
            state: LocalState::new()?,
            pending: HashMap::new(),
        })
    }

    pub fn from_state(psk: Psk, state: LocalState) -> Self {
        let room = psk.room_id();
        Self {
            psk,
            room,
            state,
            pending: HashMap::new(),
        }
    }

    pub fn device_id(&self) -> DeviceId {
        self.state.device_id
    }

    pub fn device_mac(&self) -> Mac {
        self.psk.device_mac(&self.room, &self.state.device_id)
    }

    pub fn join_frame(&self) -> ClientFrame {
        ClientFrame::Join {
            room: self.room,
            device: self.state.device_id,
            mac: self.device_mac(),
        }
    }

    /// True if the PSK has been verified against a device entry from the relay.
    pub fn verify_device(&self, entry: &crate::envelope::DeviceEntry) -> bool {
        self.psk.verify_device_mac(&self.room, &entry.id, &entry.mac)
    }

    pub fn has_session(&self, peer: DeviceId) -> bool {
        self.state.sessions.iter().any(|s| s.peer_device == peer)
    }

    /// Deterministic initiator election.
    ///
    /// When two devices discover each other at the same time, exactly one must
    /// start the handshake or they would collide. We use the device IDs: the
    /// lexicographically smaller ID initiates. Both sides compute the same
    /// answer, so no extra coordination is needed.
    pub fn should_initiate(&self, peer: DeviceId) -> bool {
        !self.has_session(peer) && self.state.device_id < peer
    }

    pub fn session_count(&self) -> usize {
        self.state.sessions.len()
    }

    /// Start a Noise handshake with a peer device (we are the initiator).
    pub fn begin_handshake(&mut self, peer: DeviceId) -> Result<PeerFrame> {
        if self.has_session(peer) {
            return Err(Error::Invalid(
                "a session with this device already exists".into(),
            ));
        }
        let mut handshake = XxHandshake::initiator(&self.psk)?;
        let data = handshake.write()?;
        self.pending.insert(peer, handshake);
        Ok(PeerFrame::Handshake { step: 0, data })
    }

    /// Feed an inbound peer frame. Returns everything the caller should send
    /// or display. Handshake messages may produce a reply to relay back.
    pub fn handle_frame(&mut self, from: DeviceId, frame: PeerFrame) -> Result<Vec<Action>> {
        match frame {
            PeerFrame::Handshake { step, data } => self.handle_handshake(from, step, &data),
            PeerFrame::Message {
                msg_id: _,
                ratchet,
            } => self.handle_message(from, ratchet),
            PeerFrame::Typing => Ok(vec![]),
            PeerFrame::Ack { .. } => Ok(vec![]),
            PeerFrame::Bye => Ok(vec![Action::Notice(format!(
                "device {from:02x?} left"
            ))]),
        }
    }

    fn handle_handshake(
        &mut self,
        from: DeviceId,
        step: u8,
        data: &[u8],
    ) -> Result<Vec<Action>> {
        match step {
            // msg1: peer started a handshake with us; we are the responder.
            0 => {
                // Ignore a second initiation for a device we already have a
                // session with, so a stray/replayed hello cannot clobber it.
                if self.has_session(from) {
                    return Ok(vec![]);
                }
                let mut handshake = XxHandshake::responder(&self.psk)?;
                handshake.read(data)?;
                let reply = handshake.write()?;
                self.pending.insert(from, handshake);
                Ok(vec![Action::Send {
                    to: from,
                    frame: PeerFrame::Handshake {
                        step: 1,
                        data: reply,
                    },
                }])
            }
            // msg2: reply to our handshake; finish and send msg3.
            1 => {
                let mut handshake = self
                    .pending
                    .remove(&from)
                    .ok_or_else(|| Error::Handshake("unexpected handshake reply".into()))?;
                handshake.read(data)?;
                let reply = handshake.write()?;
                let ratchet = handshake.finish_initiator()?;
                self.install_session(from, ratchet);
                Ok(vec![
                    Action::Send {
                        to: from,
                        frame: PeerFrame::Handshake {
                            step: 2,
                            data: reply,
                        },
                    },
                    Action::HandshakeComplete { with: from },
                ])
            }
            // msg3: peer finished the handshake with us.
            2 => {
                let mut handshake = self
                    .pending
                    .remove(&from)
                    .ok_or_else(|| Error::Handshake("unexpected handshake finish".into()))?;
                handshake.read(data)?;
                let ratchet = handshake.finish_responder()?;
                self.install_session(from, ratchet);
                Ok(vec![Action::HandshakeComplete { with: from }])
            }
            other => Err(Error::Handshake(format!("unknown handshake step {other}"))),
        }
    }

    fn handle_message(
        &mut self,
        from: DeviceId,
        ratchet: crate::ratchet::RatchetMessage,
    ) -> Result<Vec<Action>> {
        let plaintext = {
            let session = self.session_mut(from).ok_or_else(|| {
                Error::Invalid("received a message without a session".to_string())
            })?;
            session.ratchet.decrypt(&ratchet)?
        };

        let payload: ChatPayload = decode(&plaintext)?;
        match payload.kind {
            PayloadKind::Text => {
                self.state.history.push(HistoryEntry {
                    peer_device: from,
                    msg_id: payload.msg_id,
                    sent_at: payload.sent_at,
                    outgoing: false,
                    body: payload.body.clone(),
                });
                Ok(vec![Action::Received {
                    from,
                    msg_id: payload.msg_id,
                    sent_at: payload.sent_at,
                    body: payload.body,
                }])
            }
            PayloadKind::Typing => Ok(vec![Action::Notice(format!(
                "device {from:02x?} is typing"
            ))]),
        }
    }

    /// Encrypt a text message for one peer device.
    pub fn send_text(&mut self, peer: DeviceId, body: &str, sent_at: u64) -> Result<PeerFrame> {
        let mut msg_id = [0u8; 16];
        getrandom::getrandom(&mut msg_id).map_err(|e| Error::Crypto(format!("rng: {e}")))?;

        let payload = ChatPayload {
            kind: PayloadKind::Text,
            msg_id,
            sent_at,
            body: body.to_string(),
        };
        let plaintext = encode(&payload)?;

        let ratchet = {
            let session = self.session_mut(peer).ok_or_else(|| {
                Error::Invalid("no session with this device; complete the handshake first".into())
            })?;
            session.ratchet.encrypt(&plaintext)?
        };

        self.state.history.push(HistoryEntry {
            peer_device: peer,
            msg_id,
            sent_at,
            outgoing: true,
            body: body.to_string(),
        });

        Ok(PeerFrame::Message { msg_id, ratchet })
    }

    /// Encrypt the same text for several devices of the same person.
    ///
    /// Devices without an established session are skipped; the caller is
    /// expected to have run [`begin_handshake`](Self::begin_handshake) first.
    pub fn send_text_to_all(
        &mut self,
        peers: &[DeviceId],
        body: &str,
        sent_at: u64,
    ) -> Result<Vec<(DeviceId, PeerFrame)>> {
        let mut out = Vec::new();
        for &peer in peers {
            if self.has_session(peer) {
                out.push((peer, self.send_text(peer, body, sent_at)?));
            }
        }
        if out.is_empty() {
            return Err(Error::Invalid(
                "no established sessions; handshakes are still in progress".into(),
            ));
        }
        Ok(out)
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.state.history
    }

    fn install_session(&mut self, peer: DeviceId, ratchet: DoubleRatchet) {
        self.state.sessions.retain(|s| s.peer_device != peer);
        self.state.sessions.push(SessionRecord {
            peer_device: peer,
            ratchet,
        });
    }

    fn session_mut(&mut self, peer: DeviceId) -> Option<&mut SessionRecord> {
        self.state
            .sessions
            .iter_mut()
            .find(|s| s.peer_device == peer)
    }
}

/// Convenience for frontends that need to persist and resume state.
pub fn encode_state(state: &LocalState) -> Result<Vec<u8>> {
    encode(state)
}

pub fn decode_state(bytes: &[u8]) -> Result<LocalState> {
    decode(bytes)
}
