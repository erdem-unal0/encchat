use thiserror::Error;

/// Errors produced by the crypto/protocol core.
///
/// Variants intentionally avoid embedding secret material in their messages.
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid mnemonic: {0}")]
    Mnemonic(String),

    #[error("cryptographic operation failed: {0}")]
    Crypto(String),

    #[error("handshake failed: {0}")]
    Handshake(String),

    #[error("ratchet failed: {0}")]
    Ratchet(String),

    #[error("encoding failed: {0}")]
    Codec(#[from] postcard::Error),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("decryption failed")]
    Decrypt,

    #[error("message key not found (replayed, too old, or skipped too far)")]
    MissingMessageKey,
}

pub type Result<T> = std::result::Result<T, Error>;
