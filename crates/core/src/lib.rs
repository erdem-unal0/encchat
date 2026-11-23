#![forbid(unsafe_code)]
//! `encchat-core` holds the cryptography and wire protocol for encchat.
//!
//! There is deliberately **no I/O** in this crate. The CLI, the relay server
//! and the WASM bindings are thin shells around these types, so the browser
//! and the terminal run exactly the same crypto.
//!
//! Layout:
//! - [`psk`]      — pre-shared identity, mnemonic encoding, room ID, device MAC.
//! - [`handshake`]— Noise `XXpsk0` handshake that authenticates both peers.
//! - [`ratchet`]  — Double Ratchet for forward-secret message streams.
//! - [`envelope`] — client/server/peer frame types (postcard encoded).
//! - [`client`]   — state machine shared by every frontend.
//! - [`store`]    — passphrase-encrypted local state.

pub mod client;
pub mod envelope;
pub mod error;
pub mod handshake;
pub mod psk;
pub mod ratchet;
pub mod store;

pub use client::{Action, ClientCore, HistoryEntry, LocalState, SessionRecord};
pub use envelope::{
    decode, encode, ChatPayload, ClientFrame, DeviceEntry, PayloadKind, PeerFrame, ServerFrame,
};
pub use error::{Error, Result};
pub use handshake::XxHandshake;
pub use psk::{DeviceId, Mac, Psk, RoomId, DEVICE_ID_LEN, MAC_LEN, PSK_LEN, ROOM_ID_LEN};
pub use ratchet::{DoubleRatchet, RatchetHeader, RatchetMessage};
