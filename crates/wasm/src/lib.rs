#![forbid(unsafe_code)]
//! WASM bindings for the encchat web client.
//!
//! Everything here is a thin wrapper around [`encchat_core`]: the browser gets
//! the exact same handshake, ratchet and store code as the CLI. The only jobs
//! of this crate are (a) marshalling bytes/JS objects and (b) keeping the
//! frontend free of protocol knowledge.

use encchat_appearance::Appearance;
use encchat_core::{
    decode,
    envelope::{ClientFrame, PeerFrame, ServerFrame},
    store, Action, ClientCore, DeviceEntry, DeviceId, Psk,
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Build a real JS `Error` so the browser console shows a stack trace.
fn err(message: impl Into<String>) -> JsValue {
    js_sys::Error::new(&message.into()).into()
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex_decode(text: &str) -> Result<Vec<u8>, JsValue> {
    let trimmed = text.trim();
    let digits = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if digits.len() % 2 != 0 {
        return Err(err("hex string must have an even number of digits"));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_bytes().chunks_exact(2) {
        let hi = hex_value(pair[0]).ok_or_else(|| err("invalid hex digit"))?;
        let lo = hex_value(pair[1]).ok_or_else(|| err("invalid hex digit"))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn parse_device_hex(text: &str) -> Result<[u8; 16], JsValue> {
    let bytes = hex_decode(text)?;
    if bytes.len() != 16 {
        return Err(err(format!(
            "device id must be 32 hex characters (got {})",
            bytes.len() * 2
        )));
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_mac_hex(text: &str) -> Result<[u8; 32], JsValue> {
    let bytes = hex_decode(text)?;
    if bytes.len() != 32 {
        return Err(err(format!(
            "device MAC must be 64 hex characters (got {})",
            bytes.len() * 2
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_salt(bytes: &[u8]) -> Result<store::Salt, JsValue> {
    if bytes.len() != store::SALT_LEN {
        return Err(err(format!(
            "salt must be {} bytes (got {})",
            store::SALT_LEN,
            bytes.len()
        )));
    }
    let mut out = [0u8; store::SALT_LEN];
    out.copy_from_slice(bytes);
    Ok(out)
}

fn parse_mnemonic(text: &str) -> Result<Psk, JsValue> {
    Psk::from_mnemonic(text).map_err(|e| err(e.to_string()))
}

/// Split a comma-separated device list (as the UI passes it) into ids.
///
/// Empty segments are ignored, so `""` parses to an empty list.
fn parse_peer_list(peers_csv: &str) -> Result<Vec<DeviceId>, JsValue> {
    peers_csv
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(parse_device_hex)
        .collect()
}

// ---------------------------------------------------------------------------
// JS-facing shapes
// ---------------------------------------------------------------------------

/// One thing the frontend should do after feeding a frame to the core.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JsAction {
    /// Relay `data` to device `to` inside a `ClientFrame::Relay`.
    Send {
        to: String,
        data: serde_bytes::ByteBuf,
    },
    /// A message was decrypted.
    Received {
        from: String,
        #[serde(rename = "msgId")]
        msg_id: String,
        #[serde(rename = "sentAt")]
        sent_at: u64,
        body: String,
    },
    /// A session with `with` is ready; messages can be sent.
    Handshake { with: String },
    /// Human-readable status for the UI.
    Notice { message: String },
    /// Device `from` is now advertising the nickname `nick`.
    Nick { from: String, nick: String },
}

impl JsAction {
    fn from_action(action: Action) -> Result<Self, JsValue> {
        Ok(match action {
            Action::Send { to, frame } => {
                let data = encchat_core::encode(&frame).map_err(|e| err(e.to_string()))?;
                JsAction::Send {
                    to: hex_encode(&to),
                    data: serde_bytes::ByteBuf::from(data),
                }
            }
            Action::Received {
                from,
                msg_id,
                sent_at,
                body,
            } => JsAction::Received {
                from: hex_encode(&from),
                msg_id: hex_encode(&msg_id),
                sent_at,
                body,
            },
            Action::HandshakeComplete { with } => JsAction::Handshake {
                with: hex_encode(&with),
            },
            Action::Notice(message) => JsAction::Notice { message },
        })
    }
}

#[derive(Serialize)]
struct DeviceView {
    id: String,
    mac: String,
}

impl From<&DeviceEntry> for DeviceView {
    fn from(entry: &DeviceEntry) -> Self {
        Self {
            id: hex_encode(&entry.id),
            mac: hex_encode(&entry.mac),
        }
    }
}

/// Plain-object view of [`ServerFrame`] with hex ids and a `Uint8Array` payload.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ServerFrameView {
    Joined {
        room: String,
        devices: Vec<DeviceView>,
    },
    Devices {
        devices: Vec<DeviceView>,
    },
    Deliver {
        from: String,
        payload: serde_bytes::ByteBuf,
    },
    Error {
        code: String,
        message: String,
    },
    Pong {},
}

#[derive(Serialize)]
struct OutgoingView {
    to: String,
    data: serde_bytes::ByteBuf,
}

#[derive(Serialize)]
struct PeerNickView {
    device: String,
    nick: String,
}

#[derive(Serialize)]
struct HistoryView {
    #[serde(rename = "peerDevice")]
    peer_device: String,
    #[serde(rename = "msgId")]
    msg_id: String,
    #[serde(rename = "sentAt")]
    sent_at: u64,
    outgoing: bool,
    body: String,
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value).map_err(|e| err(e.to_string()))
}

fn convert_actions(actions: Vec<Action>) -> Result<Vec<JsAction>, JsValue> {
    actions
        .into_iter()
        .map(JsAction::from_action)
        .collect::<Result<Vec<_>, _>>()
}

/// Nicknames that appeared or changed between two `ClientCore::peer_nicks`
/// snapshots. Both inputs are sorted by device id, so the output is too.
fn changed_nicks(
    before: &[(DeviceId, String)],
    after: &[(DeviceId, String)],
) -> Vec<(DeviceId, String)> {
    after
        .iter()
        .filter(|(id, nick)| {
            !before
                .iter()
                .any(|(old_id, old_nick)| old_id == id && old_nick == nick)
        })
        .map(|(id, nick)| (*id, nick.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Generate a fresh 24-word mnemonic (256 bits of entropy).
#[wasm_bindgen]
pub fn generate_mnemonic() -> Result<String, JsValue> {
    let psk = Psk::generate().map_err(|e| err(e.to_string()))?;
    psk.to_mnemonic().map_err(|e| err(e.to_string()))
}

/// Human-readable room fingerprint for a mnemonic, e.g. `A1B2-...`.
#[wasm_bindgen]
pub fn room_fingerprint(mnemonic: &str) -> Result<String, JsValue> {
    Ok(parse_mnemonic(mnemonic)?.fingerprint())
}

/// Fresh random salt for the encrypted local state. Store this next to the blob.
#[wasm_bindgen]
pub fn new_salt() -> Result<Vec<u8>, JsValue> {
    store::new_salt()
        .map(|salt| salt.to_vec())
        .map_err(|e| err(e.to_string()))
}

/// Decode a relay frame into a plain JS object (see `ServerFrameView`).
#[wasm_bindgen]
pub fn decode_server_frame(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let frame: ServerFrame = decode(bytes).map_err(|e| err(e.to_string()))?;
    let view = match frame {
        ServerFrame::Joined { room, devices } => ServerFrameView::Joined {
            room: hex_encode(&room),
            devices: devices.iter().map(DeviceView::from).collect(),
        },
        ServerFrame::Devices { devices } => ServerFrameView::Devices {
            devices: devices.iter().map(DeviceView::from).collect(),
        },
        ServerFrame::Deliver { from, payload } => ServerFrameView::Deliver {
            from: hex_encode(&from),
            payload: serde_bytes::ByteBuf::from(payload),
        },
        ServerFrame::Error { code, message } => ServerFrameView::Error { code, message },
        ServerFrame::Pong => ServerFrameView::Pong {},
    };
    to_js(&view)
}

/// Encode `ClientFrame::Relay { to, payload }` for the WebSocket.
#[wasm_bindgen]
pub fn encode_relay(to: &str, payload: &[u8]) -> Result<Vec<u8>, JsValue> {
    let frame = ClientFrame::Relay {
        to: parse_device_hex(to)?,
        payload: payload.to_vec(),
    };
    encchat_core::encode(&frame).map_err(|e| err(e.to_string()))
}

/// Encode a bare keepalive `ClientFrame::Ping`.
#[wasm_bindgen]
pub fn encode_ping() -> Result<Vec<u8>, JsValue> {
    encchat_core::encode(&ClientFrame::Ping).map_err(|e| err(e.to_string()))
}

/// Reopen a persisted device from its encrypted blob.
#[wasm_bindgen]
pub fn open_state(
    mnemonic: &str,
    password: &str,
    salt: &[u8],
    blob: &[u8],
) -> Result<WasmClient, JsValue> {
    let psk = parse_mnemonic(mnemonic)?;
    let salt = parse_salt(salt)?;
    let key = store::derive_key(password, &salt).map_err(|e| err(e.to_string()))?;
    let state = store::open_state(&key, blob).map_err(|e| err(e.to_string()))?;
    Ok(WasmClient {
        inner: ClientCore::from_state(psk, state),
    })
}

/// Version of the bindings crate, handy for a "build" footer.
#[wasm_bindgen]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

/// Result of [`validate_appearance`]: the repaired document plus any warnings.
#[derive(Serialize)]
struct ValidatedAppearance {
    appearance: Appearance,
    warnings: Vec<String>,
}

/// All built-in themes, as an array of Theme objects.
#[wasm_bindgen]
pub fn appearance_presets() -> Result<JsValue, JsValue> {
    to_js(&Appearance::presets())
}

/// The default portable appearance document.
#[wasm_bindgen]
pub fn appearance_defaults() -> Result<JsValue, JsValue> {
    to_js(&Appearance::default())
}

/// Parse, sanitize and return `{ appearance: <object>, warnings: [<string>] }`.
///
/// Throws a JS `Error` for malformed JSON or the wrong JSON shape; repairable
/// values (an invalid color, an out-of-range size, ...) are fixed and reported
/// in `warnings` instead.
#[wasm_bindgen]
pub fn validate_appearance(text: &str) -> Result<JsValue, JsValue> {
    let (appearance, warnings) = Appearance::from_json(text).map_err(err)?;
    to_js(&ValidatedAppearance {
        appearance,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct WasmClient {
    inner: ClientCore,
}

#[wasm_bindgen]
impl WasmClient {
    /// Create a brand-new device identity for this room.
    #[wasm_bindgen(constructor)]
    pub fn new(mnemonic: &str) -> Result<WasmClient, JsValue> {
        let psk = parse_mnemonic(mnemonic)?;
        let inner = ClientCore::new(psk).map_err(|e| err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Our random 16-byte device id as hex.
    pub fn device_id_hex(&self) -> String {
        hex_encode(&self.inner.device_id())
    }

    /// Device MAC published to the relay (peers verify it locally).
    pub fn device_mac_hex(&self) -> String {
        hex_encode(&self.inner.device_mac())
    }

    pub fn room_id_hex(&self) -> String {
        hex_encode(&self.inner.room)
    }

    pub fn fingerprint(&self) -> String {
        self.inner
            .room
            .chunks_exact(2)
            .map(|pair| format!("{:02X}{:02X}", pair[0], pair[1]))
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn session_count(&self) -> usize {
        self.inner.session_count()
    }

    pub fn has_session(&self, peer_hex: &str) -> Result<bool, JsValue> {
        Ok(self.inner.has_session(parse_device_hex(peer_hex)?))
    }

    /// Deterministic initiator election: the smaller device id starts the
    /// handshake, so two devices that discover each other simultaneously do not
    /// both initiate. Mirrors `ClientCore::should_initiate`.
    pub fn should_initiate(&self, peer_hex: &str) -> Result<bool, JsValue> {
        Ok(self.inner.should_initiate(parse_device_hex(peer_hex)?))
    }

    /// Check a device entry from the relay against the room PSK.
    pub fn verify_device(&self, device_hex: &str, mac_hex: &str) -> Result<bool, JsValue> {
        let entry = DeviceEntry {
            id: parse_device_hex(device_hex)?,
            mac: parse_mac_hex(mac_hex)?,
        };
        Ok(self.inner.verify_device(&entry))
    }

    /// Postcard-encoded `ClientFrame::Join`, to send on WebSocket open.
    pub fn join_frame(&self) -> Result<Vec<u8>, JsValue> {
        encchat_core::encode(&self.inner.join_frame()).map_err(|e| err(e.to_string()))
    }

    /// Ask the relay for the current device list.
    pub fn devices_frame(&self) -> Result<Vec<u8>, JsValue> {
        let frame = ClientFrame::Devices {
            room: self.inner.room,
        };
        encchat_core::encode(&frame).map_err(|e| err(e.to_string()))
    }

    pub fn ping_frame(&self) -> Result<Vec<u8>, JsValue> {
        encode_ping()
    }

    /// Start a handshake with `peer_hex`; returns a postcard `PeerFrame` to relay.
    pub fn begin_handshake(&mut self, peer_hex: &str) -> Result<Vec<u8>, JsValue> {
        let peer = parse_device_hex(peer_hex)?;
        let frame = self
            .inner
            .begin_handshake(peer)
            .map_err(|e| err(e.to_string()))?;
        encchat_core::encode(&frame).map_err(|e| err(e.to_string()))
    }

    /// Feed an inbound `PeerFrame`; returns an array of action objects.
    ///
    /// Any nickname the frame updated is reported as a trailing `nick` action
    /// (`{ kind: "nick", from, nick }`), since the core returns no `Action` for
    /// it.
    pub fn handle_frame(&mut self, from_hex: &str, bytes: &[u8]) -> Result<JsValue, JsValue> {
        let from = parse_device_hex(from_hex)?;
        let frame: PeerFrame = decode(bytes).map_err(|e| err(e.to_string()))?;
        let before = self.inner.peer_nicks();
        let actions = self
            .inner
            .handle_frame(from, frame)
            .map_err(|e| err(e.to_string()))?;
        let mut converted = convert_actions(actions)?;
        for (device, nick) in changed_nicks(&before, &self.inner.peer_nicks()) {
            converted.push(JsAction::Nick {
                from: hex_encode(&device),
                nick,
            });
        }
        to_js(&converted)
    }

    /// Encrypt `body` for every peer in `peers_csv` that has a session.
    ///
    /// Returns `[{ to, data }]`; `data` is a postcard `PeerFrame`.
    pub fn send_text(
        &mut self,
        peers_csv: &str,
        body: &str,
        sent_at_ms: f64,
    ) -> Result<JsValue, JsValue> {
        if !sent_at_ms.is_finite() || sent_at_ms < 0.0 {
            return Err(err(
                "sentAt must be a non-negative timestamp in milliseconds",
            ));
        }
        let peers = parse_peer_list(peers_csv)?;
        if peers.is_empty() {
            return Err(err("no peer devices selected"));
        }
        let frames = self
            .inner
            .send_text_to_all(&peers, body, sent_at_ms as u64)
            .map_err(|e| err(e.to_string()))?;
        let outgoing = frames
            .into_iter()
            .map(|(peer, frame)| {
                let data = encchat_core::encode(&frame).map_err(|e| err(e.to_string()))?;
                Ok(OutgoingView {
                    to: hex_encode(&peer),
                    data: serde_bytes::ByteBuf::from(data),
                })
            })
            .collect::<Result<Vec<_>, JsValue>>()?;
        to_js(&outgoing)
    }

    /// Set our advertised nickname.
    ///
    /// The core trims and sanitizes the label, so this returns `false` when
    /// nothing usable is left (which also clears any previous nickname).
    pub fn set_nick(&mut self, name: &str) -> bool {
        self.inner.set_nick(name).is_some()
    }

    /// Our advertised nickname, or `undefined` if we have not set one.
    pub fn self_nick(&self) -> Option<String> {
        self.inner.self_nick().map(str::to_string)
    }

    /// Encrypt our nickname for every peer in `peers_csv` that has a session.
    ///
    /// Returns `[{ to, data }]` like [`Self::send_text`]. An empty array is
    /// normal: either no peer has a session yet or we have no nickname.
    pub fn announce_nick(&mut self, peers_csv: &str) -> Result<JsValue, JsValue> {
        let mut peers = parse_peer_list(peers_csv)?;
        peers.sort_unstable();
        peers.dedup();
        let frames = self.inner.announce_nick_to_all(&peers);
        let outgoing = frames
            .into_iter()
            .map(|(peer, frame)| {
                let data = encchat_core::encode(&frame).map_err(|e| err(e.to_string()))?;
                Ok(OutgoingView {
                    to: hex_encode(&peer),
                    data: serde_bytes::ByteBuf::from(data),
                })
            })
            .collect::<Result<Vec<_>, JsValue>>()?;
        to_js(&outgoing)
    }

    /// Every peer nickname currently known, as `[{ device, nick }]` sorted by
    /// device id.
    pub fn peer_nicks(&self) -> Result<JsValue, JsValue> {
        let nicks = self
            .inner
            .peer_nicks()
            .into_iter()
            .map(|(device, nick)| PeerNickView {
                device: hex_encode(&device),
                nick,
            })
            .collect::<Vec<_>>();
        to_js(&nicks)
    }

    /// Encrypt the whole local state with `password` and `salt`.
    pub fn encode_state(&self, password: &str, salt: &[u8]) -> Result<Vec<u8>, JsValue> {
        let salt = parse_salt(salt)?;
        let key = store::derive_key(password, &salt).map_err(|e| err(e.to_string()))?;
        store::seal_state(&key, &self.inner.state).map_err(|e| err(e.to_string()))
    }

    /// Local history, oldest first, for restoring the scrollback.
    pub fn history(&self) -> Result<JsValue, JsValue> {
        let entries = self
            .inner
            .history()
            .iter()
            .map(|entry| HistoryView {
                peer_device: hex_encode(&entry.peer_device),
                msg_id: hex_encode(&entry.msg_id),
                sent_at: entry.sent_at,
                outgoing: entry.outgoing,
                body: entry.body.clone(),
            })
            .collect::<Vec<_>>();
        to_js(&entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let text = hex_encode(&bytes);
        assert_eq!(text.len(), 512);
        assert_eq!(hex_decode(&text).unwrap(), bytes);
        assert_eq!(hex_decode(&format!("0x{text}")).unwrap(), bytes);
    }

    #[test]
    fn parse_peer_list_skips_blanks() {
        assert!(parse_peer_list("").unwrap().is_empty());
        assert!(parse_peer_list(" , ,").unwrap().is_empty());
        let hex = hex_encode(&[3u8; 16]);
        let parsed = parse_peer_list(&format!(" {hex} ,{hex}")).unwrap();
        assert_eq!(parsed, vec![[3u8; 16], [3u8; 16]]);
    }

    #[test]
    fn changed_nicks_reports_new_and_updated_only() {
        let alice = [7u8; 16];
        let bob = [9u8; 16];
        let before = [(alice, "alice".to_string())];
        let after = [(bob, "bob".to_string()), (alice, "alice2".to_string())];

        let changed = changed_nicks(&before, &after);
        assert_eq!(
            changed,
            vec![(bob, "bob".to_string()), (alice, "alice2".to_string())]
        );

        assert!(changed_nicks(&after, &after).is_empty());
        assert!(changed_nicks(&[], &[]).is_empty());
        assert_eq!(changed_nicks(&after, &[]).len(), 0);
        assert_eq!(changed_nicks(&[], &after).len(), 2);
    }
}
