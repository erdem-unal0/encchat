use snow::{Builder, HandshakeState};

use crate::{
    error::{Error, Result},
    psk::Psk,
    ratchet::DoubleRatchet,
};

/// `XX` gives mutual authentication; `psk0` mixes the shared key into the very
/// first message, so an attacker (including the relay) that does not know the
/// PSK cannot complete or MITM the handshake.
const NOISE_PARAMS: &str = "Noise_XXpsk0_25519_ChaChaPoly_BLAKE2s";
const MAX_HANDSHAKE_MSG: usize = 1024;
const RATCHET_KEY_LEN: usize = 32;

/// `(handshake hash, own ratchet secret, own ratchet public, peer ratchet public)`.
type RatchetSeed = ([u8; 32], [u8; 32], [u8; 32], [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Initiator,
    Responder,
}

/// A three-message Noise handshake that also carries each side's initial
/// Double Ratchet DH public key as the handshake payload.
///
/// Flow:
/// 1. initiator `write()`  -> msg1 (payload: initiator ratchet pub)
/// 2. responder `read(msg1)`, `write()` -> msg2 (payload: responder ratchet pub)
/// 3. initiator `read(msg2)`, `write()` -> msg3 (empty payload)
/// 4. responder `read(msg3)`
///
/// Then `finish_initiator()` / `finish_responder()` produce a [`DoubleRatchet`].
pub struct XxHandshake {
    inner: HandshakeState,
    self_secret: [u8; RATCHET_KEY_LEN],
    self_public: [u8; RATCHET_KEY_LEN],
    remote_public: Option<[u8; RATCHET_KEY_LEN]>,
    sent_own_public: bool,
    role: Role,
}

impl XxHandshake {
    pub fn initiator(psk: &Psk) -> Result<Self> {
        let params = NOISE_PARAMS
            .parse()
            .map_err(|e: snow::Error| Error::Handshake(e.to_string()))?;
        let mut static_secret = [0u8; 32];
        getrandom::getrandom(&mut static_secret).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
        let inner = Builder::new(params)
            .psk(0, psk.as_bytes())
            .local_private_key(&static_secret)
            .build_initiator()
            .map_err(|e| Error::Handshake(e.to_string()))?;
        Self::with_inner(inner, Role::Initiator)
    }

    pub fn responder(psk: &Psk) -> Result<Self> {
        let params = NOISE_PARAMS
            .parse()
            .map_err(|e: snow::Error| Error::Handshake(e.to_string()))?;
        let mut static_secret = [0u8; 32];
        getrandom::getrandom(&mut static_secret).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
        let inner = Builder::new(params)
            .psk(0, psk.as_bytes())
            .local_private_key(&static_secret)
            .build_responder()
            .map_err(|e| Error::Handshake(e.to_string()))?;
        Self::with_inner(inner, Role::Responder)
    }

    fn with_inner(inner: HandshakeState, role: Role) -> Result<Self> {
        let mut secret = [0u8; RATCHET_KEY_LEN];
        getrandom::getrandom(&mut secret).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
        let sk = x25519_dalek::StaticSecret::from(secret);
        let public = x25519_dalek::PublicKey::from(&sk).to_bytes();
        Ok(Self {
            inner,
            self_secret: secret,
            self_public: public,
            remote_public: None,
            sent_own_public: false,
            role,
        })
    }

    pub fn is_finished(&self) -> bool {
        self.inner.is_handshake_finished()
    }

    /// Produce the next handshake message. The first message written by each
    /// side carries its ratchet public key; later messages carry no payload.
    pub fn write(&mut self) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        if !self.sent_own_public {
            payload.extend_from_slice(&self.self_public);
        }
        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG];
        let written = self
            .inner
            .write_message(&payload, &mut buf)
            .map_err(|e| Error::Handshake(e.to_string()))?;
        self.sent_own_public = true;
        buf.truncate(written);
        Ok(buf)
    }

    /// Consume a handshake message and return its payload.
    pub fn read(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG];
        let read = self
            .inner
            .read_message(message, &mut buf)
            .map_err(|e| Error::Handshake(e.to_string()))?;
        buf.truncate(read);
        if self.remote_public.is_none() && buf.len() == RATCHET_KEY_LEN {
            let mut key = [0u8; RATCHET_KEY_LEN];
            key.copy_from_slice(&buf);
            self.remote_public = Some(key);
        }
        Ok(buf)
    }

    pub fn finish_initiator(self) -> Result<DoubleRatchet> {
        if self.role != Role::Initiator {
            return Err(Error::Handshake("not the initiator".into()));
        }
        let (shared, secret, public, remote) = self.finish_parts()?;
        DoubleRatchet::init_initiator(&shared, secret, public, remote)
    }

    pub fn finish_responder(self) -> Result<DoubleRatchet> {
        if self.role != Role::Responder {
            return Err(Error::Handshake("not the responder".into()));
        }
        let (shared, secret, public, remote) = self.finish_parts()?;
        DoubleRatchet::init_responder(&shared, secret, public, remote)
    }

    fn finish_parts(self) -> Result<RatchetSeed> {
        if !self.inner.is_handshake_finished() {
            return Err(Error::Handshake("handshake is not finished".into()));
        }
        let remote = self
            .remote_public
            .ok_or_else(|| Error::Handshake("peer did not send a ratchet key".into()))?;
        let hash = self.inner.get_handshake_hash();
        if hash.len() != 32 {
            return Err(Error::Handshake("unexpected handshake hash length".into()));
        }
        let mut shared = [0u8; 32];
        shared.copy_from_slice(hash);
        Ok((shared, self.self_secret, self.self_public, remote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_handshake(psk: &Psk) -> (DoubleRatchet, DoubleRatchet) {
        let mut alice = XxHandshake::initiator(psk).unwrap();
        let mut bob = XxHandshake::responder(psk).unwrap();

        let m1 = alice.write().unwrap();
        bob.read(&m1).unwrap();
        let m2 = bob.write().unwrap();
        alice.read(&m2).unwrap();
        let m3 = alice.write().unwrap();
        bob.read(&m3).unwrap();

        assert!(alice.is_finished());
        assert!(bob.is_finished());

        let alice = alice.finish_initiator().unwrap();
        let bob = bob.finish_responder().unwrap();
        (alice, bob)
    }

    #[test]
    fn handshake_derives_matching_sessions() {
        let psk = Psk::generate().unwrap();
        let (mut alice, mut bob) = complete_handshake(&psk);

        let message = alice.encrypt(b"hello bob").unwrap();
        assert_eq!(bob.decrypt(&message).unwrap(), b"hello bob");

        let reply = bob.encrypt(b"hello alice").unwrap();
        assert_eq!(alice.decrypt(&reply).unwrap(), b"hello alice");
    }

    #[test]
    fn wrong_psk_cannot_handshake() {
        let alice_psk = Psk::from_bytes([1u8; 32]);
        let bob_psk = Psk::from_bytes([2u8; 32]);

        let mut alice = XxHandshake::initiator(&alice_psk).unwrap();
        let mut bob = XxHandshake::responder(&bob_psk).unwrap();

        let m1 = alice.write().unwrap();
        // psk0 is mixed before the first message, but `e` carries no keyed
        // operation, so the failure surfaces when the initiator processes the
        // responder's message (or when the handshake hash is compared).
        if bob.read(&m1).is_ok() {
            let m2 = bob.write().unwrap();
            assert!(alice.read(&m2).is_err());
        }
    }
}
