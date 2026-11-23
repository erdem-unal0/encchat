//! `encchat-server`: the blind relay behind encchat.
//!
//! The relay routes already-encrypted payloads between devices in a room. It
//! never sees plaintext, the pre-shared key, or user identities, and it keeps
//! nothing on disk.
//!
//! - [`relay`] — the routing state machine (pure, testable without a network)
//! - [`mailbox`] — the offline queue
//! - [`http`] — the axum HTTP/WebSocket front end

pub mod http;
pub mod mailbox;
pub mod relay;

pub use http::{build_router, default_config};
pub use mailbox::{Mailbox, MailboxConfig, QueuedMessage};
pub use relay::Relay;
