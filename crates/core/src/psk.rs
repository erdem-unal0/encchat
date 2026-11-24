use blake2::{Blake2b512, Digest};
use hmac::{Hmac, Mac as HmacTrait};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Error, Result};

pub const PSK_LEN: usize = 32;
pub const ROOM_ID_LEN: usize = 16;
pub const DEVICE_ID_LEN: usize = 16;
pub const MAC_LEN: usize = 32;

pub type RoomId = [u8; ROOM_ID_LEN];
pub type DeviceId = [u8; DEVICE_ID_LEN];
pub type Mac = [u8; MAC_LEN];
pub type PskBytes = [u8; PSK_LEN];

/// The shared identity of a conversation.
///
/// Both humans agree on these 32 bytes out of band. Everything in the app is
/// derived from them; the relay server never learns the key.
///
/// The bytes are zeroed when the value is dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Psk(PskBytes);

impl Psk {
    /// Generate a fresh random key using the operating system CSPRNG.
    pub fn generate() -> Result<Self> {
        let mut bytes = [0u8; PSK_LEN];
        getrandom::getrandom(&mut bytes).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
        Ok(Self(bytes))
    }

    /// Wrap raw key bytes. Prefer [`Psk::from_mnemonic`] for human input.
    pub fn from_bytes(bytes: PskBytes) -> Self {
        Self(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != PSK_LEN {
            return Err(Error::Invalid(format!(
                "pre-shared key must be exactly {PSK_LEN} bytes"
            )));
        }
        let mut out = [0u8; PSK_LEN];
        out.copy_from_slice(bytes);
        Ok(Self(out))
    }

    pub fn as_bytes(&self) -> &PskBytes {
        &self.0
    }

    /// Encode the key as a 24-word BIP39 mnemonic so it can be read aloud.
    ///
    /// This is only an encoding of 256 bits of entropy; it is not used as a
    /// BIP39 wallet seed.
    pub fn to_mnemonic(&self) -> Result<String> {
        let mnemonic =
            bip39::Mnemonic::from_entropy(&self.0).map_err(|e| Error::Mnemonic(e.to_string()))?;
        Ok(mnemonic.to_string())
    }

    pub fn from_mnemonic(text: &str) -> Result<Self> {
        let mnemonic = bip39::Mnemonic::parse_normalized(text.trim())
            .map_err(|e| Error::Mnemonic(e.to_string()))?;
        let entropy = mnemonic.to_entropy();
        Self::from_slice(&entropy)
    }

    /// Opaque 16-byte rendezvous identifier. The server sees only this.
    pub fn room_id(&self) -> RoomId {
        let mut hasher = Blake2b512::new();
        Digest::update(&mut hasher, b"encchat/room/v1");
        Digest::update(&mut hasher, &self.0[..]);
        let digest = hasher.finalize();
        let mut out = [0u8; ROOM_ID_LEN];
        out.copy_from_slice(&digest[..ROOM_ID_LEN]);
        out
    }

    /// MAC that binds a device ID to the shared key. Peers verify this so the
    /// relay cannot inject extra devices into a room.
    pub fn device_mac(&self, room: &RoomId, device: &DeviceId) -> Mac {
        let mut mac = <Hmac<Sha256> as HmacTrait>::new_from_slice(&self.0)
            .expect("HMAC accepts any key length");
        mac.update(b"encchat/device/v1");
        mac.update(room);
        mac.update(device);
        let tag = mac.finalize().into_bytes();
        let mut out = [0u8; MAC_LEN];
        out.copy_from_slice(&tag);
        out
    }

    pub fn verify_device_mac(&self, room: &RoomId, device: &DeviceId, mac: &Mac) -> bool {
        let expected = self.device_mac(room, device);
        expected.as_slice().ct_eq(mac.as_slice()).into()
    }

    /// Human-readable room fingerprint, e.g. `A1B2-C3D4-...`.
    ///
    /// Safe to show in the UI and to compare verbally; it is derived from the
    /// same bytes the server already sees as the room ID.
    pub fn fingerprint(&self) -> String {
        let room = self.room_id();
        let mut groups = Vec::with_capacity(ROOM_ID_LEN / 2);
        for pair in room.chunks_exact(2) {
            groups.push(format!("{:02X}{:02X}", pair[0], pair[1]));
        }
        groups.join("-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonic_roundtrip() {
        let psk = Psk::generate().unwrap();
        let words = psk.to_mnemonic().unwrap();
        assert_eq!(words.split_whitespace().count(), 24);
        let back = Psk::from_mnemonic(&words).unwrap();
        assert_eq!(psk.as_bytes(), back.as_bytes());
        assert_eq!(psk.room_id(), back.room_id());
    }

    #[test]
    fn room_id_is_stable_and_psk_specific() {
        let a = Psk::from_bytes([7u8; 32]);
        let b = Psk::from_bytes([8u8; 32]);
        assert_eq!(a.room_id(), a.room_id());
        assert_ne!(a.room_id(), b.room_id());
    }

    #[test]
    fn device_mac_roundtrip() {
        let psk = Psk::generate().unwrap();
        let room = psk.room_id();
        let device = [3u8; DEVICE_ID_LEN];
        let mac = psk.device_mac(&room, &device);
        assert!(psk.verify_device_mac(&room, &device, &mac));
        let other = [4u8; DEVICE_ID_LEN];
        assert!(!psk.verify_device_mac(&room, &other, &mac));
    }
}
