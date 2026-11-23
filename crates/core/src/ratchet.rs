use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::error::{Error, Result};

/// Maximum number of message keys we are willing to derive ahead of time to
/// tolerate reordering. This bounds memory and work done for a malicious peer.
pub const MAX_SKIP: u32 = 2000;

/// Message keys are single-use, so a fixed all-zero AEAD nonce is safe and is
/// the convention used by the Double Ratchet specification.
const ZERO_NONCE: [u8; 24] = [0u8; 24];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatchetHeader {
    /// Sender's current ratchet public key.
    pub dh: [u8; 32],
    /// Number of messages in the previous sending chain.
    pub pn: u32,
    /// Message number in the current sending chain.
    pub n: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RatchetMessage {
    pub header: RatchetHeader,
    pub ciphertext: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SkippedKey {
    dh: [u8; 32],
    n: u32,
    mk: [u8; 32],
}

/// Double Ratchet state for a single pair of devices.
///
/// The header travels in the clear but is authenticated as AEAD associated
/// data, so it cannot be modified without failing decryption.
#[derive(Clone, Serialize, Deserialize)]
pub struct DoubleRatchet {
    root_key: [u8; 32],
    self_secret: [u8; 32],
    self_public: [u8; 32],
    remote_public: [u8; 32],
    send_chain: Option<[u8; 32]>,
    recv_chain: Option<[u8; 32]>,
    send_n: u32,
    recv_n: u32,
    prev_send_n: u32,
    skipped: Vec<SkippedKey>,
}

impl std::fmt::Debug for DoubleRatchet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleRatchet")
            .field("send_n", &self.send_n)
            .field("recv_n", &self.recv_n)
            .field("skipped", &self.skipped.len())
            .finish_non_exhaustive()
    }
}

impl DoubleRatchet {
    fn bootstrap(
        shared: &[u8; 32],
        self_secret: [u8; 32],
        self_public: [u8; 32],
        remote_public: [u8; 32],
        sending: bool,
    ) -> Result<Self> {
        let dh = StaticSecret::from(self_secret).diffie_hellman(&PublicKey::from(remote_public));
        let (root_key, chain) = kdf_rk(shared, dh.as_bytes())?;
        let (send_chain, recv_chain) = if sending {
            (Some(chain), None)
        } else {
            (None, Some(chain))
        };
        Ok(Self {
            root_key,
            self_secret,
            self_public,
            remote_public,
            send_chain,
            recv_chain,
            send_n: 0,
            recv_n: 0,
            prev_send_n: 0,
            skipped: Vec::new(),
        })
    }

    /// Build the state for the peer that spoke first in the Noise handshake.
    pub fn init_initiator(
        shared: &[u8; 32],
        self_secret: [u8; 32],
        self_public: [u8; 32],
        remote_public: [u8; 32],
    ) -> Result<Self> {
        Self::bootstrap(shared, self_secret, self_public, remote_public, true)
    }

    /// Build the state for the peer that answered the Noise handshake.
    pub fn init_responder(
        shared: &[u8; 32],
        self_secret: [u8; 32],
        self_public: [u8; 32],
        remote_public: [u8; 32],
    ) -> Result<Self> {
        Self::bootstrap(shared, self_secret, self_public, remote_public, false)
    }

    /// Encrypt one message, advancing the sending chain.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<RatchetMessage> {
        if self.send_chain.is_none() {
            self.ratchet_send()?;
        }

        let mk = {
            let chain = self
                .send_chain
                .as_mut()
                .ok_or_else(|| Error::Ratchet("missing sending chain".into()))?;
            let (next, mk) = kdf_ck(chain);
            *chain = next;
            mk
        };

        let header = RatchetHeader {
            dh: self.self_public,
            pn: self.prev_send_n,
            n: self.send_n,
        };
        self.send_n = self
            .send_n
            .checked_add(1)
            .ok_or_else(|| Error::Ratchet("send counter overflow".into()))?;

        let aad = postcard::to_allocvec(&header)?;
        let ciphertext = seal(&mk, &aad, plaintext)?;
        Ok(RatchetMessage { header, ciphertext })
    }

    /// Decrypt one message, handling out-of-order delivery via skipped keys.
    pub fn decrypt(&mut self, message: &RatchetMessage) -> Result<Vec<u8>> {
        let aad = postcard::to_allocvec(&message.header)?;

        // A message from an older chain that we already skipped over.
        if let Some(index) = self
            .skipped
            .iter()
            .position(|k| k.dh == message.header.dh && k.n == message.header.n)
        {
            let skipped = self.skipped.remove(index);
            return open(&skipped.mk, &aad, &message.ciphertext);
        }

        if message.header.dh != self.remote_public {
            // Finish the current receiving chain, then perform a DH ratchet:
            // derive the new receiving chain, generate a fresh sending key,
            // and derive the new sending chain.
            self.skip_message_keys(message.header.pn)?;
            self.ratchet_recv(message.header.dh)?;
            self.ratchet_send()?;
        }

        if message.header.n < self.recv_n {
            // Older than the current chain and not in the skipped set: replay
            // or a message we can no longer recover.
            return Err(Error::MissingMessageKey);
        }

        self.skip_message_keys(message.header.n)?;

        let mk = {
            let chain = self
                .recv_chain
                .as_mut()
                .ok_or_else(|| Error::Ratchet("missing receiving chain".into()))?;
            let (next, mk) = kdf_ck(chain);
            *chain = next;
            mk
        };
        self.recv_n = self
            .recv_n
            .checked_add(1)
            .ok_or_else(|| Error::Ratchet("receive counter overflow".into()))?;

        open(&mk, &aad, &message.ciphertext)
    }

    fn ratchet_recv(&mut self, remote_public: [u8; 32]) -> Result<()> {
        let dh = StaticSecret::from(self.self_secret)
            .diffie_hellman(&PublicKey::from(remote_public));
        let (root_key, chain) = kdf_rk(&self.root_key, dh.as_bytes())?;
        self.root_key = root_key;
        self.recv_chain = Some(chain);
        self.remote_public = remote_public;
        self.recv_n = 0;
        Ok(())
    }

    fn ratchet_send(&mut self) -> Result<()> {
        let (secret, public) = new_dh()?;
        let dh =
            StaticSecret::from(secret).diffie_hellman(&PublicKey::from(self.remote_public));
        let (root_key, chain) = kdf_rk(&self.root_key, dh.as_bytes())?;
        self.root_key = root_key;
        self.self_secret = secret;
        self.self_public = public;
        self.send_chain = Some(chain);
        self.prev_send_n = self.send_n;
        self.send_n = 0;
        Ok(())
    }

    fn skip_message_keys(&mut self, until: u32) -> Result<()> {
        if self.recv_chain.is_none() || until <= self.recv_n {
            return Ok(());
        }
        if until - self.recv_n > MAX_SKIP {
            return Err(Error::Ratchet("too many skipped messages".into()));
        }

        while self.recv_n < until {
            let mk = {
                let chain = self
                    .recv_chain
                    .as_mut()
                    .ok_or_else(|| Error::Ratchet("missing receiving chain".into()))?;
                let (next, mk) = kdf_ck(chain);
                *chain = next;
                mk
            };
            self.skipped.push(SkippedKey {
                dh: self.remote_public,
                n: self.recv_n,
                mk,
            });
            self.recv_n += 1;
        }

        if self.skipped.len() > MAX_SKIP as usize {
            let excess = self.skipped.len() - MAX_SKIP as usize;
            self.skipped.drain(0..excess);
        }
        Ok(())
    }
}

fn kdf_rk(root_key: &[u8; 32], dh_out: &[u8; 32]) -> Result<([u8; 32], [u8; 32])> {
    let hk = Hkdf::<Sha256>::new(Some(root_key), dh_out);
    let mut okm = [0u8; 64];
    hk.expand(b"encchat/ratchet/rk/v1", &mut okm)
        .map_err(|_| Error::Crypto("hkdf expand failed".into()))?;
    let mut next_root = [0u8; 32];
    next_root.copy_from_slice(&okm[..32]);
    let mut chain = [0u8; 32];
    chain.copy_from_slice(&okm[32..]);
    Ok((next_root, chain))
}

fn kdf_ck(chain_key: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(chain_key).expect("HMAC accepts any key length");
    mac.update(&[0x01]);
    let next = mac.finalize().into_bytes();

    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(chain_key).expect("HMAC accepts any key length");
    mac.update(&[0x02]);
    let message_key = mac.finalize().into_bytes();

    let mut next_chain = [0u8; 32];
    next_chain.copy_from_slice(&next);
    let mut mk = [0u8; 32];
    mk.copy_from_slice(&message_key);
    (next_chain, mk)
}

fn new_dh() -> Result<([u8; 32], [u8; 32])> {
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
    let sk = StaticSecret::from(secret);
    let public = PublicKey::from(&sk).to_bytes();
    Ok((secret, public))
}

fn seal(message_key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(message_key)
        .map_err(|_| Error::Crypto("invalid AEAD key length".into()))?;
    cipher
        .encrypt(
            XNonce::from_slice(&ZERO_NONCE),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Crypto("AEAD encryption failed".into()))
}

fn open(message_key: &[u8; 32], aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(message_key)
        .map_err(|_| Error::Crypto("invalid AEAD key length".into()))?;
    cipher
        .decrypt(
            XNonce::from_slice(&ZERO_NONCE),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| Error::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (DoubleRatchet, DoubleRatchet) {
        let shared = [9u8; 32];
        let (a_secret, a_public) = new_dh().unwrap();
        let (b_secret, b_public) = new_dh().unwrap();
        let a = DoubleRatchet::init_initiator(&shared, a_secret, a_public, b_public).unwrap();
        let b = DoubleRatchet::init_responder(&shared, b_secret, b_public, a_public).unwrap();
        (a, b)
    }

    #[test]
    fn basic_roundtrip_and_ratchet() {
        let (mut a, mut b) = pair();
        for i in 0..10u8 {
            let msg = a.encrypt(&[i]).unwrap();
            assert_eq!(b.decrypt(&msg).unwrap(), vec![i]);
        }
        // Replying forces a DH ratchet in both directions.
        let reply = b.encrypt(b"reply").unwrap();
        assert_eq!(a.decrypt(&reply).unwrap(), b"reply");
        let msg = a.encrypt(b"after ratchet").unwrap();
        assert_eq!(b.decrypt(&msg).unwrap(), b"after ratchet");
    }

    #[test]
    fn out_of_order_messages_recover() {
        let (mut a, mut b) = pair();
        let m0 = a.encrypt(b"zero").unwrap();
        let m1 = a.encrypt(b"one").unwrap();
        let m2 = a.encrypt(b"two").unwrap();

        assert_eq!(b.decrypt(&m2).unwrap(), b"two");
        assert_eq!(b.decrypt(&m0).unwrap(), b"zero");
        assert_eq!(b.decrypt(&m1).unwrap(), b"one");
    }

    #[test]
    fn tampering_fails() {
        let (mut a, mut b) = pair();
        let mut msg = a.encrypt(b"secret").unwrap();
        msg.ciphertext[0] ^= 0x01;
        assert!(b.decrypt(&msg).is_err());
    }

    #[test]
    fn replay_is_rejected() {
        let (mut a, mut b) = pair();
        let msg = a.encrypt(b"once").unwrap();
        assert_eq!(b.decrypt(&msg).unwrap(), b"once");
        assert!(b.decrypt(&msg).is_err());
    }
}
