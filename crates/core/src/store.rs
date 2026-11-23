use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};

use crate::{
    client::LocalState,
    error::{Error, Result},
};

pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
pub const KEY_LEN: usize = 32;

pub type StoreKey = [u8; KEY_LEN];
pub type Salt = [u8; SALT_LEN];

const AAD: &[u8] = b"encchat/store/v1";

/// Fresh random salt for a new store file.
pub fn new_salt() -> Result<Salt> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::getrandom(&mut salt).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
    Ok(salt)
}

/// Derive the local-store key from a passphrase with Argon2id (defaults).
pub fn derive_key(passphrase: &str, salt: &Salt) -> Result<StoreKey> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| Error::Crypto(format!("argon2: {e}")))?;
    Ok(key)
}

/// Encrypt the whole local state. Output layout: `nonce || ciphertext`.
pub fn seal_state(key: &StoreKey, state: &LocalState) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|e| Error::Crypto(format!("rng: {e}")))?;

    let plaintext = postcard::to_allocvec(state)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| Error::Crypto("invalid store key length".into()))?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: AAD,
            },
        )
        .map_err(|_| Error::Crypto("store encryption failed".into()))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn open_state(key: &StoreKey, blob: &[u8]) -> Result<LocalState> {
    if blob.len() < NONCE_LEN + 16 {
        return Err(Error::Invalid("store file is too short".into()));
    }
    let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| Error::Crypto("invalid store key length".into()))?;
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: AAD,
            },
        )
        .map_err(|_| Error::Decrypt)?;
    Ok(postcard::from_bytes(&plaintext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::LocalState;

    #[test]
    fn store_roundtrip() {
        let salt = new_salt().unwrap();
        let key = derive_key("correct horse battery staple", &salt).unwrap();
        let state = LocalState::new().unwrap();
        let blob = seal_state(&key, &state).unwrap();
        let back = open_state(&key, &blob).unwrap();
        assert_eq!(state.device_id, back.device_id);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let salt = new_salt().unwrap();
        let key = derive_key("right", &salt).unwrap();
        let wrong = derive_key("wrong", &salt).unwrap();
        let state = LocalState::new().unwrap();
        let blob = seal_state(&key, &state).unwrap();
        assert!(open_state(&wrong, &blob).is_err());
    }
}
