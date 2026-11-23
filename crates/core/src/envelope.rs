use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    error::Result,
    psk::{DeviceId, Mac, RoomId},
    ratchet::RatchetMessage,
};

/// A device as published by the relay for a room.
///
/// `mac` is `HMAC-SHA256(PSK, "encchat/device/v1" || room || device)`. Peers
/// verify it locally; the relay can store and return it but cannot forge it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub id: DeviceId,
    pub mac: Mac,
}

/// Frames sent from a client to the relay server (postcard encoded).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientFrame {
    /// Join a room and register this device.
    Join {
        room: RoomId,
        device: DeviceId,
        mac: Mac,
    },
    /// Ask for the current device list of a room.
    Devices { room: RoomId },
    /// Send opaque bytes to another device in the same room.
    Relay { to: DeviceId, payload: Vec<u8> },
    Ping,
}

/// Frames sent from the relay server to a client (postcard encoded).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerFrame {
    Joined {
        room: RoomId,
        devices: Vec<DeviceEntry>,
    },
    Devices {
        devices: Vec<DeviceEntry>,
    },
    Deliver {
        from: DeviceId,
        payload: Vec<u8>,
    },
    Error {
        code: String,
        message: String,
    },
    Pong,
}

/// Frames exchanged between two peer devices inside `ClientFrame::Relay`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PeerFrame {
    /// One step of the Noise handshake (`step` is 0, 1 or 2).
    Handshake { step: u8, data: Vec<u8> },
    /// A Double Ratchet message.
    Message {
        msg_id: [u8; 16],
        ratchet: RatchetMessage,
    },
    Ack { msg_id: [u8; 16] },
    Typing,
    Bye,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayloadKind {
    Text,
    Typing,
}

/// The plaintext inside a [`RatchetMessage`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatPayload {
    pub kind: PayloadKind,
    pub msg_id: [u8; 16],
    /// Sender's wall-clock time in milliseconds since the Unix epoch.
    pub sent_at: u64,
    pub body: String,
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(postcard::to_allocvec(value)?)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    Ok(postcard::from_bytes(bytes)?)
}
