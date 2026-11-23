# encchat wire-protocol specification (v1)

Status: **draft, pre-interop, unaudited.** This document is derived from the
`encchat-core` implementation and is intended to be precise enough for an
independent implementer. Where the implementation and this document disagree,
the implementation is authoritative; every place where that happened is called
out in [§12 Deviations and mismatches](#12-deviations-and-mismatches).

The relay server is currently a scaffold (`crates/server/src/main.rs` prints
"relay not implemented yet"); only its mailbox helper
(`crates/server/src/mailbox.rs`) exists. Section [§7](#7-relay-behavior) states
the intended relay contract and marks what is not yet implemented.

---

## Table of contents

1. [Overview and roles](#1-overview-and-roles)
2. [Notation and primitives](#2-notation-and-primitives)
3. [Identity and rendezvous](#3-identity-and-rendezvous)
4. [Noise handshake](#4-noise-handshake)
5. [Double Ratchet](#5-double-ratchet)
6. [Frames](#6-frames)
7. [Relay behavior](#7-relay-behavior)
8. [Multi-device fanout](#8-multi-device-fanout)
9. [Local encrypted state](#9-local-encrypted-state)
10. [Versioning and test vectors](#10-versioning-and-test-vectors)
11. [Security notes](#11-security-notes)
12. [Deviations and mismatches](#12-deviations-and-mismatches)
13. [Underspecified items (to freeze)](#13-underspecified-items-to-freeze)

---

## 1. Overview and roles

`encchat` is a self-hosted, end-to-end encrypted 1:1 text chat. Two humans
share one secret out of band and talk through a relay that is assumed hostile
and is designed to be blind.

### 1.1 Parties

| Role | Description |
| --- | --- |
| **Human** | One of the two participants. Holds the PSK. |
| **Device** | One running instance of the client (browser tab / CLI). A human has `N ≥ 1` devices. Each device has a random 16-byte device ID and its own ratchet session with every peer device. |
| **Peer** | The *other* human. The set of peer devices is the fan-out target set. |
| **Relay (server)** | Routes opaque frames between devices in the same room, and queues frames for offline devices for a short TTL. Never holds the PSK, never sees plaintext, writes nothing to disk. |

There are exactly **two principals** (the two humans). Multi-device support is
per-principal, not a group: every device belongs to one of the two humans, and
all devices of a human share the same PSK.

### 1.2 Trust model in one paragraph

The PSK is the single trust root: it determines the room, authenticates the
Noise handshake, and MACs device registrations. The relay learns the 16-byte
room ID, device IDs, message sizes, and timing, but cannot read messages, cannot
complete a handshake, cannot forge a device, and cannot hold history. A full
treatment is in `docs/threat-model.md`; this document only restates the
spec-relevant consequences in [§11](#11-security-notes).

### 1.3 Layering

```
+-----------------------------------------------------------+
| Application: text chat, typing notices                    |
+-----------------------------------------------------------+
| ChatPayload (postcard)   -- plaintext, per message        |
+-----------------------------------------------------------+
| Double Ratchet (X25519 + XChaCha20-Poly1305)              |
|   RatchetMessage = header || AEAD ciphertext              |
+-----------------------------------------------------------+
| PeerFrame (postcard)     -- handshake / message / control |
+-----------------------------------------------------------+
| ClientFrame / ServerFrame (postcard)                      |
|   PeerFrame bytes travel inside ClientFrame::Relay.payload |
+-----------------------------------------------------------+
| WebSocket binary messages (relay)                         |
+-----------------------------------------------------------+
| Noise XXpsk0 -- one per device pair, establishes the      |
|   Double Ratchet seed; carries initial ratchet public keys |
+-----------------------------------------------------------+
```

A device pair performs exactly **one** Noise handshake, then exchanges
Double Ratchet messages for the lifetime of the session. The Noise session
itself is not used for transport after the handshake; only its handshake hash
is used, as the Double Ratchet seed.

### 1.4 End-to-end flow

1. Both humans obtain the same 32-byte PSK and its 24-word mnemonic out of band.
2. Each device derives `roomID` from the PSK and connects to the relay with
   `ClientFrame::Join`.
3. The relay returns the current device list (`ServerFrame::Joined`). Devices
   verify each entry's device MAC against the PSK.
4. For each peer device, a device starts a Noise handshake
   (`PeerFrame::Handshake { step: 0, .. }`) and both sides reach
   `HandshakeComplete`.
5. Text is sent as one `PeerFrame::Message` per recipient device, each encrypted
   under that device pair's Double Ratchet session.
6. Frames for offline devices are queued by the relay and delivered on reconnect
   within the mailbox TTL.

---

## 2. Notation and primitives

### 2.1 Notation

| Notation | Meaning |
| --- | --- |
| `a || b` | Byte-string concatenation. |
| `x[0..n]` | First `n` bytes of `x`. |
| `b"..."` | A literal ASCII/UTF-8 byte string. |
| `HKDF-SHA256(salt, ikm, info, L)` | HKDF (RFC 5869) with HMAC-SHA256, output length `L`. |
| `HMAC-SHA256(k, m)` | HMAC with SHA-256. |
| `BLAKE2b-512(m)` | Unkeyed BLAKE2b with a 64-byte digest. |
| `X25519(sk, pk)` | X25519 scalar multiplication (Diffie–Hellman). |
| `AEAD_seal(k, n, aad, pt)` / `AEAD_open(k, n, aad, ct)` | XChaCha20-Poly1305 encrypt / decrypt (16-byte tag appended). |
| `postcard(x)` | The `postcard` crate's compact binary encoding of `x`. |

### 2.2 Primitive inventory

These are the exact algorithms and library crates used by `encchat-core`
(`crates/core/Cargo.toml`).

| Purpose | Primitive | Crate / notes |
| --- | --- | --- |
| Key agreement (Noise + ratchet DH) | X25519 | `x25519-dalek` 2 (`static_secrets`) |
| Noise handshake | `Noise_XXpsk0_25519_ChaChaPoly_BLAKE2s` | `snow` 0.9 |
| Room ID hash | BLAKE2b-512, truncated to 16 bytes | `blake2` 0.10 (`Blake2b512`) |
| Device MAC | HMAC-SHA256 | `hmac` 0.12 + `sha2` 0.10 |
| Ratchet root KDF | HKDF-SHA256, 64-byte output | `hkdf` 0.12 |
| Ratchet chain KDF | HMAC-SHA256 | `hmac` + `sha2` |
| Message / store AEAD | XChaCha20-Poly1305 (24-byte nonce) | `chacha20poly1305` 0.10 |
| Local-store passphrase KDF | Argon2id | `argon2` 0.5 |
| Mnemonic encoding | BIP39, 256-bit entropy → 24 words | `bip39` 2 |
| Wire/state serialization | `postcard` 1 (`alloc`) | `postcard` |
| Constant-time compare | `subtle` | `subtle` 2 |
| Secret zeroization | `zeroize` | `zeroize` 1 |

> **Note (Noise):** the protocol name `Noise_XXpsk0_25519_ChaChaPoly_BLAKE2s`
> fully determines the handshake, per the Noise spec (revision 34). `XXpsk0`
> expands to `-> psk, e` / `<- e, ee, s, es` / `-> s, se`: the `psk` token is
> mixed into the first handshake message via `MixKeyAndHash(PSK)`, before the
> initiator's ephemeral key.

### 2.3 postcard encoding

`postcard` is compact and **not self-describing**: there are no field names or
lengths for structs and tuples. Field order in the encoding is exactly the
declaration order in the Rust source. Implementers must match the struct/enum
definitions byte for byte. The relevant rules (verified against `postcard`
1.2.0):

| Rust type | postcard encoding |
| --- | --- |
| `u8` | 1 raw byte (not varint) |
| `u16`, `u32`, `u64`, `u128` | LEB128 varint (little-endian, continuation bit) |
| `bool` | `u8`: `0x00` / `0x01` |
| `String` / `&str` | varint byte-length, then UTF-8 bytes |
| `Vec<T>` / slice | varint element count, then each element |
| `[T; N]` (e.g. `[u8; 16]`) | exactly `N` elements, **no length prefix** (serde `serialize_tuple`) |
| `Option<T>` | `0x00` = `None`; `0x01` followed by `T` = `Some` |
| struct | fields concatenated, no tag/length |
| enum | varint `u32` variant index, then the variant's fields |

Consequences used throughout this document:

- `[u8; 16]` encodes as exactly 16 bytes; `[u8; 32]` as exactly 32 bytes.
- Enum variant indices are `0, 1, 2, …` in declaration order (see [§6](#6-frames)).

### 2.4 BIP39 mnemonic

The 32-byte PSK is encoded as a **24-word BIP39 mnemonic** (256 bits of entropy,
8-bit checksum). It is an *encoding* of the raw key only — it is **not** run
through BIP39 seed derivation, and it is not used as a wallet seed. Parsing uses
`bip39::Mnemonic::parse_normalized` after trimming surrounding whitespace
(i.e. the default English wordlist; input is Unicode-normalized).

---

## 3. Identity and rendezvous

### 3.1 Pre-shared key (PSK)

- Length: **32 bytes** (`PSK_LEN`). Any other length is rejected.
- Source: OS CSPRNG (`getrandom`) for `Psk::generate`.
- The `Psk` value is zeroized on drop (`zeroize` + `ZeroizeOnDrop`) and is
  `Clone` but deliberately has no `Debug`.
- The PSK is exchanged out of band. It is never sent to the relay.

### 3.2 Mnemonic

`to_mnemonic` / `from_mnemonic` round-trip the 32 bytes through the 24-word
BIP39 representation ([§2.4](#24-bip39-mnemonic)).

### 3.3 Room ID

```
roomID = BLAKE2b-512(b"encchat/room/v1" || PSK)[0..16]
```

- 16 bytes (`ROOM_ID_LEN`).
- Domain separation label: `b"encchat/room/v1"` (prepended to the 32 PSK bytes).
- The digest is **BLAKE2b with a 64-byte output** (`Blake2b512`), truncated to
  its first 16 bytes. (The task brief said "BLAKE2b-256"; the code uses
  BLAKE2b-512. See [§12](#12-deviations-and-mismatches).)
- The room ID is the only identifier the relay uses to group devices. It is
  stable for a given PSK and is therefore linkable across sessions.

### 3.4 Device ID

- Length: **16 bytes** (`DEVICE_ID_LEN`), drawn from the OS CSPRNG at device
  creation (`LocalState::new`).
- A device ID is local to a room (it is not globally unique by construction).
- Device IDs are visible to the relay.

### 3.5 Device MAC

```
deviceMAC = HMAC-SHA256(key = PSK,
                        msg = b"encchat/device/v1" || roomID || deviceID)
```

- 32 bytes (`MAC_LEN`).
- HMAC key is the PSK; the message is the literal label, then the 16-byte room
  ID, then the 16-byte device ID.
- Verification is constant-time (`subtle::ConstantTimeEq`) via
  `Psk::verify_device_mac`.
- Purpose: bind a device ID to the shared key so a relay cannot inject a device
  into a room. Peers verify; the relay cannot (it has no key).

### 3.6 Human-readable fingerprint

`Psk::fingerprint()` returns the room ID as 8 hyphen-separated groups of four
**uppercase** hex digits, e.g. `A1B2-C3D4-E5F6-0718-293A-4B5C-6D7E-8F90`. It is
derived from the same bytes the relay already sees, so it is safe to display and
compare verbally.

> **Mismatch:** the WASM binding reimplements `fingerprint()` and produces the
> same grouping in **lowercase** hex. The core (`encchat-core`) version is
> canonical. See [§12](#12-deviations-and-mismatches).

### 3.7 Constants

| Constant | Value | Source |
| --- | --- | --- |
| `PSK_LEN` | 32 | `psk.rs` |
| `ROOM_ID_LEN` | 16 | `psk.rs` |
| `DEVICE_ID_LEN` | 16 | `psk.rs` |
| `MAC_LEN` | 32 | `psk.rs` |
| `RATCHET_KEY_LEN` | 32 | `handshake.rs` |
| `MAX_HANDSHAKE_MSG` | 1024 | `handshake.rs` |
| `MAX_SKIP` | 2000 | `ratchet.rs` |
| `SALT_LEN` | 16 | `store.rs` |
| `NONCE_LEN` | 24 | `store.rs` |
| `KEY_LEN` | 32 | `store.rs` |
| `LocalState.version` | 1 | `client.rs` |

---

## 4. Noise handshake

The handshake establishes a mutually authenticated, forward-secret seed for the
Double Ratchet. Authentication comes **entirely from the PSK**: the static keys
are freshly generated per handshake and are not identities.

### 4.1 Protocol name

```
Noise_XXpsk0_25519_ChaChaPoly_BLAKE2s
```

Expanded pattern (Noise spec rev 34):

```
XXpsk0:
  -> psk, e
  <- e, ee, s, es
  -> s, se
```

`snow` is configured with `Builder::new(params).psk(0, psk).local_private_key(&static_secret)`.
The PSK index is `0`.

### 4.2 Keys

- Each side generates a **fresh random 32-byte X25519 static secret** for the
  handshake (`getrandom`), and a fresh ephemeral key pair inside Noise.
- The static keys are *not* identities and are discarded after the handshake;
  there is no long-term key. An attacker without the PSK cannot complete or MITM
  the handshake because the `psk0` token mixes the PSK into the very first
  message.

### 4.3 Message flow and payloads

Each side also generates a **separate random X25519 key pair** for the initial
Double Ratchet DH. The first handshake message each side *writes* carries that
side's ratchet public key (32 bytes) as the Noise handshake payload. Later
payloads are empty.

| Noise msg | Sender | Pattern | Payload |
| --- | --- | --- | --- |
| msg1 | initiator | `psk, e` | initiator's ratchet public key (32 B) |
| msg2 | responder | `e, ee, s, es` | responder's ratchet public key (32 B) |
| msg3 | initiator | `s, se` | empty |

Approximate sizes (X25519 keys 32 B, AEAD tag 16 B, encrypted payload = 32+16):
msg1 = 80 B, msg2 = 128 B, msg3 = 64 B. All are well under
`MAX_HANDSHAKE_MSG = 1024`; a message that does not fit the 1024-byte buffer is
an error.

On `read`, a decrypted payload of exactly 32 bytes is captured as the remote
ratchet public key (only the first time; `remote_public` is set once).

### 4.4 `PeerFrame::Handshake` step numbers

The `step` field is a `u8` assigned by the **writer** of the frame and consumed
by the reader:

| `step` | Meaning | Sent by | Reader action |
| --- | --- | --- | --- |
| `0` | Noise **msg1** (initiator's first write). | Initiator (the device that called `begin_handshake`) | Treat sender as responder: create a responder `XxHandshake`, `read(data)`, `write()` msg2, store the pending handshake, reply with `step: 1`. |
| `1` | Noise **msg2** (responder's write). | Responder | Look up pending initiator handshake, `read(data)`, `write()` msg3, `finish_initiator()`, install the session, emit `HandshakeComplete`, reply with `step: 2`. |
| `2` | Noise **msg3** (initiator's final write, empty payload). | Initiator | Look up pending responder handshake, `read(data)`, `finish_responder()`, install the session, emit `HandshakeComplete`. No reply. |

Any other `step` value is an error.

### 4.5 Completion and handshake hash

After the third message, both sides call
`snow::HandshakeState::get_handshake_hash()` (the Noise `h` value, 32 bytes with
BLAKE2s). This hash is the **seed** passed to the Double Ratchet bootstrap as
`shared` ([§5.2](#52-initialization)). The Noise `Split()` transport keys are
**not** used; only `h` is.

- Initiator finishes as soon as it has read msg2 (before sending msg3), so it can
  begin sending ratchet messages immediately after `step 1` is processed.
- Responder finishes after reading msg3.

### 4.6 Failure behavior

- A wrong PSK is not detected on msg1 (it contains no keyed ciphertext), but the
  handshake fails when the other side processes a later message, or when the
  handshake hash would differ. Implementations must abort and discard state on
  any handshake error.
- `finish_*` errors if the handshake is not finished or if the peer never sent a
  32-byte ratchet key.

---

## 5. Double Ratchet

One Double Ratchet instance exists per device pair (one per peer device). It
provides per-message keys, forward secrecy, post-compromise security, and
bounded out-of-order delivery.

### 5.1 State

| Field | Type | Meaning |
| --- | --- | --- |
| `root_key` | `[u8; 32]` | Current root key `RK`. |
| `self_secret` | `[u8; 32]` | Our current ratchet X25519 secret. |
| `self_public` | `[u8; 32]` | Our current ratchet X25519 public (`header.dh` when sending). |
| `remote_public` | `[u8; 32]` | Peer's current ratchet X25519 public. |
| `send_chain` | `Option<[u8; 32]>` | Current sending chain key `CK_s`. `None` until first send. |
| `recv_chain` | `Option<[u8; 32]>` | Current receiving chain key `CK_r`. `None` until first receive. |
| `send_n` | `u32` | Message number in the current sending chain. |
| `recv_n` | `u32` | Message number in the current receiving chain. |
| `prev_send_n` | `u32` | Length of the previous sending chain (`header.pn`). |
| `skipped` | `Vec<SkippedKey>` | Stored message keys for out-of-order delivery. |

`SkippedKey` is `{ dh: [u8; 32], n: u32, mk: [u8; 32] }`.

### 5.2 Initialization

Both sides bootstrap from the Noise handshake hash `shared` and the ratchet keys
carried in the handshake payloads:

```
bootstrap(shared, self_secret, self_public, remote_public, sending):
    dh            = X25519(self_secret, remote_public)
    (RK, CK)      = KDF_RK(shared, dh)
    root_key      = RK
    if sending:                       # initiator
        send_chain = Some(CK); recv_chain = None
    else:                             # responder
        send_chain = None;     recv_chain = Some(CK)
    send_n = recv_n = prev_send_n = 0
    skipped = []
```

- Initiator: `init_initiator` (`sending = true`).
- Responder: `init_responder` (`sending = false`).

Both sides compute the same `dh` (X25519 is symmetric), so both derive the same
`(RK, CK)`; the initiator uses `CK` as its sending chain and the responder uses
it as its receiving chain.

### 5.3 Root KDF (`KDF_RK`)

```
KDF_RK(RK, DH):
    okm = HKDF-SHA256(salt = RK, ikm = DH, info = b"encchat/ratchet/rk/v1", L = 64)
    return (okm[0..32], okm[32..64])      # (new RK, chain key CK)
```

The HKDF salt is the current root key; the IKM is the X25519 DH output; the info
label is exactly `encchat/ratchet/rk/v1`.

### 5.4 Chain KDF (`KDF_CK`)

```
KDF_CK(CK):
    next_CK = HMAC-SHA256(key = CK, msg = 0x01)
    MK      = HMAC-SHA256(key = CK, msg = 0x02)
    return (next_CK, MK)
```

`MK` is a 32-byte message key; it is single-use.

### 5.5 Header

`RatchetHeader` travels in the clear but is authenticated as AEAD associated data.

| Field | Type | Meaning |
| --- | --- | --- |
| `dh` | `[u8; 32]` | Sender's current ratchet public key. |
| `pn` | `u32` | Number of messages in the sender's previous sending chain. |
| `n` | `u32` | Message number in the sender's current sending chain. |

postcard layout: `dh(32) || varint(pn) || varint(n)`.

### 5.6 AEAD

- Algorithm: **XChaCha20-Poly1305**.
- Nonce: the **all-zero 24-byte nonce** `[0u8; 24]`. This is safe because every
  `MK` is used exactly once (message keys are single-use by construction).
- AAD: `postcard(header)` — the exact bytes of the header that is transmitted.
- Ciphertext: Poly1305 tag is appended (16 bytes).

```
seal(MK, header, plaintext):
    return XChaCha20Poly1305_encrypt(key = MK, nonce = 0^24,
                                     aad = postcard(header), msg = plaintext)
open(MK, header, ciphertext):
    return XChaCha20Poly1305_decrypt(key = MK, nonce = 0^24,
                                     aad = postcard(header), msg = ciphertext)
```

### 5.7 Encrypt

```
encrypt(plaintext):
    if send_chain is None:
        ratchet_send()                       # first send triggers a DH ratchet
    (next_CK, MK) = KDF_CK(send_chain)
    send_chain = next_CK
    header = { dh: self_public, pn: prev_send_n, n: send_n }
    send_n = send_n + 1                      # error on u32 overflow
    ct = seal(MK, header, plaintext)
    return { header, ciphertext: ct }
```

### 5.8 Decrypt

```
decrypt(message):
    header = message.header
    # 1. Out-of-order message from a chain we already skipped over.
    if a SkippedKey exists with (dh == header.dh and n == header.n):
        remove it and return open(its MK, header, message.ciphertext)

    # 2. Sender has ratcheted: finish the old chain, then DH-ratchet.
    if header.dh != remote_public:
        skip_message_keys(header.pn)         # drain remainder of old recv chain
        ratchet_recv(header.dh)              # new recv chain from DH
        ratchet_send()                       # new send key + chain from DH

    # 3. Too old / replayed and not in the skipped set.
    if header.n < recv_n:
        return error MissingMessageKey

    # 4. Skip forward within the current chain, then derive the message key.
    skip_message_keys(header.n)
    (next_CK, MK) = KDF_CK(recv_chain)
    recv_chain = next_CK
    recv_n = recv_n + 1
    return open(MK, header, message.ciphertext)
```

### 5.9 DH ratchet steps

```
ratchet_recv(remote_pub):
    dh        = X25519(self_secret, remote_pub)
    (RK, CK)  = KDF_RK(root_key, dh)
    root_key  = RK
    recv_chain = Some(CK)
    remote_public = remote_pub
    recv_n = 0

ratchet_send():
    (secret, public) = fresh X25519 key pair
    dh        = X25519(secret, remote_public)
    (RK, CK)  = KDF_RK(root_key, dh)
    root_key  = RK
    self_secret = secret
    self_public = public
    send_chain = Some(CK)
    prev_send_n = send_n
    send_n = 0
```

The first send by the responder triggers `ratchet_send` (its `send_chain` is
`None` after bootstrap), which introduces a fresh DH public key; the initiator
observes a new `header.dh` and performs `ratchet_recv` + `ratchet_send`.

### 5.10 Skipped keys and `MAX_SKIP`

```
skip_message_keys(until):
    if recv_chain is None or until <= recv_n:
        return
    if until - recv_n > MAX_SKIP:            # MAX_SKIP = 2000
        return error "too many skipped messages"
    while recv_n < until:
        (next_CK, MK) = KDF_CK(recv_chain)
        recv_chain = next_CK
        skipped.push({ dh: remote_public, n: recv_n, mk: MK })
        recv_n = recv_n + 1
    if skipped.len() > MAX_SKIP:
        drop the oldest (skipped.len() - MAX_SKIP) entries
```

- `MAX_SKIP = 2000` bounds both the work done for a single jump and the total
  retained skipped keys (oldest are evicted first).
- A skipped key is removed from the store the first time it is used, so it
  cannot be reused.

### 5.11 Replay rejection

- A message whose `n < recv_n` (current chain) and whose `(dh, n)` is not in the
  skipped set is rejected with `MissingMessageKey`.
- A successfully decrypted message cannot be replayed: its skipped key (if any)
  was consumed, and otherwise its `n` is now `< recv_n`.
- Header tampering fails AEAD authentication because the header is the AAD.
- Ciphertext tampering fails the Poly1305 tag check.

---

## 6. Frames

All frames are **postcard-encoded binary WebSocket messages**. `ClientFrame`
travels client → relay; `ServerFrame` travels relay → client; `PeerFrame`
travels device → device, nested inside `ClientFrame::Relay.payload`.

Helper functions `encode` / `decode` in `envelope.rs` are thin wrappers over
`postcard::to_allocvec` / `postcard::from_bytes`.

> **Underspecified:** the wire does not distinguish these three frame kinds with
> a type tag; the receiver must know, from context, whether a WebSocket message
> is a `ClientFrame`/`ServerFrame` (relay link) or a `PeerFrame` (inside
> `Deliver.payload`). There is no framing/version byte on the WebSocket message.

### 6.1 `ClientFrame` (client → relay)

| Variant (index) | Fields | Field types | Semantics |
| --- | --- | --- | --- |
| `Join` (0) | `room`, `device`, `mac` | `[u8;16]`, `[u8;16]`, `[u8;32]` | Join the room and register this device. |
| `Devices` (1) | `room` | `[u8;16]` | Request the current device list. |
| `Relay` (2) | `to`, `payload` | `[u8;16]`, `Vec<u8>` | Send opaque bytes to another device in the same room. |
| `Ping` (3) | — | — | Keepalive. |

Encoding examples (postcard):

```
Join{room,device,mac} = 00 || room(16) || device(16) || mac(32)      # 65 bytes
Devices{room}         = 01 || room(16)                               # 17 bytes
Relay{to,payload}     = 02 || to(16) || varint(len) || payload
Ping                  = 03                                           # 1 byte
```

`Relay.payload` is a `PeerFrame` ([§6.3](#63-peerframe-device--device)) encoded
as bytes; the relay never decodes it.

### 6.2 `ServerFrame` (relay → client)

| Variant (index) | Fields | Field types | Semantics |
| --- | --- | --- | --- |
| `Joined` (0) | `room`, `devices` | `[u8;16]`, `Vec<DeviceEntry>` | Acknowledgement of `Join`, with the current device list. |
| `Devices` (1) | `devices` | `Vec<DeviceEntry>` | Response to `Devices`. |
| `Deliver` (2) | `from`, `payload` | `[u8;16]`, `Vec<u8>` | A relayed payload from `from`; `payload` is a `PeerFrame`. |
| `Error` (3) | `code`, `message` | `String`, `String` | Error report. |
| `Pong` (4) | — | — | Keepalive response. |

```
Joined{room,devices}   = 00 || room(16) || varint(count) || DeviceEntry*
Devices{devices}       = 01 || varint(count) || DeviceEntry*
Deliver{from,payload}  = 02 || from(16) || varint(len) || payload
Error{code,message}    = 03 || str(code) || str(message)
Pong                   = 04
```

`DeviceEntry` = `id(16) || mac(32)` (48 bytes). The relay treats `mac` as opaque
bytes.

> **Underspecified:** the `Error.code` vocabulary is not defined anywhere in the
> implementation; the server is a scaffold. The client currently surfaces
> `ServerFrame::Error` without a defined handling path.

### 6.3 `PeerFrame` (device → device)

| Variant (index) | Fields | Field types | Semantics |
| --- | --- | --- | --- |
| `Handshake` (0) | `step`, `data` | `u8`, `Vec<u8>` | One Noise handshake step; `step ∈ {0,1,2}` ([§4.4](#44-peerframehandshake-step-numbers)). |
| `Message` (1) | `msg_id`, `ratchet` | `[u8;16]`, `RatchetMessage` | An encrypted chat message. |
| `Ack` (2) | `msg_id` | `[u8;16]` | Acknowledgement. |
| `Typing` (3) | — | — | Typing indicator. |
| `Bye` (4) | — | — | Peer is leaving. |

```
Handshake{step,data}  = 00 || step(u8) || varint(len) || data
Message{msg_id,r}     = 01 || msg_id(16) || RatchetMessage
Ack{msg_id}           = 02 || msg_id(16)
Typing                = 03
Bye                   = 04
```

> **Underspecified / partially unused.** `Ack` is defined but **never emitted**
> by `ClientCore`, and inbound `Ack` is ignored. `Typing` (peer frame) is
> ignored. `Bye` only produces a human-readable notice. There is no
> retransmission, acknowledgement, or delivery-receipt protocol; reliability is
> not specified.

### 6.4 `RatchetMessage`

| Field | Type | Semantics |
| --- | --- | --- |
| `header` | `RatchetHeader` | See [§5.5](#55-header). |
| `ciphertext` | `Vec<u8>` | AEAD output: encrypted `postcard(ChatPayload)` + 16-byte tag. |

```
RatchetMessage = RatchetHeader || varint(len(ciphertext)) || ciphertext
RatchetHeader  = dh(32) || varint(pn) || varint(n)
```

### 6.5 `ChatPayload` (plaintext inside a `RatchetMessage`)

| Field | Type | Semantics |
| --- | --- | --- |
| `kind` | `PayloadKind` | `Text` (0) or `Typing` (1). |
| `msg_id` | `[u8;16]` | Random per-message ID (CSPRNG), also echoed in `PeerFrame::Message.msg_id`. |
| `sent_at` | `u64` | Sender wall-clock time, milliseconds since the Unix epoch. Not validated against any clock skew. |
| `body` | `String` | Message text (UTF-8). |

```
PayloadKind::Text   = 00
PayloadKind::Typing = 01
ChatPayload = kind(u8) || msg_id(16) || varint(sent_at) || str(body)
```

`ChatPayload` is encrypted, so all of its fields are authenticated by the
ratchet. `kind: Typing` currently ignores `body` (and `msg_id`/`sent_at`).

### 6.6 `DeviceEntry`

| Field | Type | Semantics |
| --- | --- | --- |
| `id` | `[u8;16]` | Device ID. |
| `mac` | `[u8;32]` | `deviceMAC` ([§3.5](#35-device-mac)); peers MUST verify. |

### 6.7 Nesting

```
WebSocket binary message (relay link)
  └── ClientFrame::Relay { to, payload }        (client → relay)
        └── payload = postcard(PeerFrame)       (opaque to the relay)
              └── PeerFrame::Message { msg_id, ratchet }
                    └── RatchetMessage.ciphertext = AEAD(postcard(ChatPayload))
```

On the receive path the relay sends `ServerFrame::Deliver { from, payload }`; the
client decodes `payload` as a `PeerFrame` and calls
`ClientCore::handle_frame(from, frame)`.

### 6.8 Internal state machine output (`Action`, not wire)

`ClientCore::handle_frame` returns `Vec<Action>`, a frontend instruction (not
serialized on the wire). Implementers should reproduce the same decisions:

| `Action` | Meaning |
| --- | --- |
| `Send { to, frame }` | Encode `frame` as a `PeerFrame` and send it to `to` inside `ClientFrame::Relay`. |
| `Received { from, msg_id, sent_at, body }` | A text message was decrypted and appended to local history. |
| `HandshakeComplete { with }` | A session with `with` is installed and ready. |
| `Notice(String)` | Human-readable status. |

`Action` variant order (for reference only): `Send` (0), `Received` (1),
`HandshakeComplete` (2), `Notice` (3).

---

## 7. Relay behavior

This is the **intended** contract. Only the mailbox
(`crates/server/src/mailbox.rs`) is implemented today; the WebSocket relay in
`crates/server/src/main.rs` is a scaffold that prints "not implemented yet".

### 7.1 Responsibilities

The relay:

1. Accepts WebSocket connections and decodes `ClientFrame`s.
2. Tracks which connection belongs to which `(room, device)`.
3. Returns device lists on `Join` and `Devices`.
4. Forwards `ClientFrame::Relay` payloads to the target device if online, or
   queues them for later delivery if offline.
5. Answers `Ping` with `Pong`.
6. Treats every payload as opaque bytes and writes nothing to disk.

### 7.2 Join / register

- On `ClientFrame::Join { room, device, mac }`, the relay associates the
  connection with `(room, device)` and replies with
  `ServerFrame::Joined { room, devices }`, where `devices` is the current
  device list of the room (including the joining device).
- The relay **does not validate `mac`** (it has no PSK and cannot). It stores and
  returns `mac` verbatim. Peers validate it ([§3.5](#35-device-mac)).
- The relay has no accounts and no persistent registry; registration lives only
  for the process lifetime.

> **Underspecified:** duplicate `Join` for the same `(room, device)`, ID
> conflicts, and whether a device may belong to more than one room are not
> defined by the code.

### 7.3 Device list

- `ClientFrame::Devices { room }` → `ServerFrame::Devices { devices }`.
- **Underspecified:** ordering of `devices` (insertion order vs. sorted) is not
  defined, nor whether offline devices are included. The client must not rely on
  order.

### 7.4 Relay-or-queue with TTL

The mailbox (`mailbox.rs`) is per-device, in-memory, FIFO:

| Parameter | Default | Meaning |
| --- | --- | --- |
| `max_entries` | 512 | Max queued messages per device. When full, the **oldest** entry is dropped to make room. |
| `ttl` | 300 s | A queued message is discarded once `now - queued_at >= ttl`. |

Behavior details from the implementation:

- `push` first prunes expired entries, then drops oldest while at capacity, then
  appends the new message.
- An entry expires exactly at the TTL boundary (`>= ttl`).
- `ttl == 0` expires everything immediately; `max_entries == 0` queues nothing.
- `restore_front` re-inserts a batch at the front preserving original
  timestamps (used when a flush fails partway through); it trims from the back
  to respect `max_entries`.
- Delivery to an online device is expected to bypass the queue; the queued
  messages are drained oldest-first when the device reconnects.

> **Underspecified:** how queued messages are drained into `ServerFrame::Deliver`
> on reconnect, and the online/offline transition logic, are not implemented.

### 7.5 Blindness requirements

The relay **MUST NOT**:

- attempt to decrypt or parse `Relay.payload` / `Deliver.payload` (they are
  opaque `PeerFrame` bytes);
- validate or reject device MACs (it cannot);
- store plaintext, history, or the PSK;
- persist anything to disk (mailbox is RAM-only).

The relay **MAY** observe room IDs, device IDs, IPs, connection timing, message
counts, ciphertext sizes, and delivery timing.

### 7.6 `Ping` / `Pong`

`ClientFrame::Ping` (no fields) is answered with `ServerFrame::Pong`. No interval
or timeout is specified by the implementation.

---

## 8. Multi-device fanout

- A device maintains **one Double Ratchet session per peer device**
  (`LocalState.sessions`, keyed by `peer_device`). Sessions are independent:
  chain keys, counters, and skipped keys never mix across peers.
- `send_text_to_all(peers, body, sent_at)` encrypts the **same plaintext**
  separately for every peer device that already has a session, producing **one
  `PeerFrame::Message` (one ciphertext) per recipient device**. Devices without
  a session are silently skipped; if none has a session, the call errors.
- The **same `body` and `sent_at`** are used for every recipient, but each
  recipient gets a **distinct `msg_id`** (each `send_text` call generates a fresh
  random `msg_id`). Consequently, the same logical message has a different
  `msg_id` per recipient device, and `sent_at` is duplicated.
- Each device keeps its **own local history** (`LocalState.history`), appended
  with `outgoing = true` on send and `outgoing = false` on receive. History is
  not synchronized across devices; there is no cross-device history protocol.
- The frontend decides which peer devices to fan out to, based on the relay's
  device list (filtered by device-MAC verification).

> **Underspecified:** how a device learns about a peer's *newly added* device
> and backfills a session is not defined beyond re-requesting the device list
> and starting a new handshake.

---

## 9. Local encrypted state

All persistent state lives in `LocalState`, serialized with postcard and sealed
with a passphrase-derived key.

### 9.1 `LocalState`

| Field | Type | Semantics |
| --- | --- | --- |
| `version` | `u8` | Format version; currently `1`. |
| `device_id` | `[u8;16]` | This device's ID. |
| `sessions` | `Vec<SessionRecord>` | One per peer device. |
| `history` | `Vec<HistoryEntry>` | Decrypted local transcript, oldest first. |

```
LocalState = version(u8) || device_id(16) || varint(n_sessions) || SessionRecord*
             || varint(n_history) || HistoryEntry*
```

### 9.2 `SessionRecord`

| Field | Type | Semantics |
| --- | --- | --- |
| `peer_device` | `[u8;16]` | Peer device ID. |
| `ratchet` | `DoubleRatchet` | Serialized ratchet state ([§9.4](#94-doubleratchet-serialization)). |

### 9.3 `HistoryEntry`

| Field | Type | Semantics |
| --- | --- | --- |
| `peer_device` | `[u8;16]` | Peer device ID. |
| `msg_id` | `[u8;16]` | Message ID. |
| `sent_at` | `u64` | Sender wall-clock ms since epoch. |
| `outgoing` | `bool` | `true` if we sent it. |
| `body` | `String` | Plaintext. |

### 9.4 `DoubleRatchet` serialization

postcard field order (this is a persistence format; changing it breaks existing
stores):

```
DoubleRatchet = root_key(32) || self_secret(32) || self_public(32)
              || remote_public(32)
              || send_chain(Option<[u8;32]>)     # 00, or 01 || 32 bytes
              || recv_chain(Option<[u8;32]>)
              || varint(send_n) || varint(recv_n) || varint(prev_send_n)
              || varint(len(skipped)) || SkippedKey*
SkippedKey   = dh(32) || varint(n) || mk(32)
```

> **Security note:** this blob contains live ratchet secrets. It must only ever
> be written sealed ([§9.5](#95-store-blob-layout)).

### 9.5 Store blob layout

```
blob = nonce(24) || XChaCha20-Poly1305(key = StoreKey, nonce = nonce,
                                        aad = b"encchat/store/v1",
                                        msg = postcard(LocalState))
```

- `nonce`: 24 random bytes from the CSPRNG, fresh on every `seal_state`.
- `key`: `StoreKey` = 32 bytes, derived by Argon2id ([§9.6](#96-passphrase-kdf-argon2id)).
- AAD: the literal `b"encchat/store/v1"`.
- `open_state` rejects a blob shorter than `24 + 16` bytes; otherwise it splits
  at 24 bytes and decrypts.
- The **salt is not part of the blob**; the caller must store the 16-byte salt
  separately (e.g. alongside the blob) and supply it to `derive_key`.

### 9.6 Passphrase KDF (Argon2id)

```
StoreKey = Argon2id(password = passphrase_utf8, salt = salt(16), out_len = 32)
```

`derive_key` uses `Argon2::default()`. As of `argon2` 0.5.3 that means:

| Parameter | Value |
| --- | --- |
| Algorithm | Argon2id |
| Version | `0x13` (19) |
| Memory (`m_cost`) | 19456 KiB (19 MiB) |
| Iterations (`t_cost`) | 2 |
| Parallelism (`p_cost`) | 1 |
| Output length | 32 bytes |
| Salt length | 16 bytes |

> **Underspecified / to pin:** these are **library defaults**, not pinned by
> `encchat-core`. A dependency bump that changed `Params::default()` would
> silently make existing stores unreadable and could weaken the KDF. They should
> be frozen explicitly (see [§13](#13-underspecified-items-to-freeze)).

---

## 10. Versioning and test vectors

### 10.1 Current version

| Item | Value |
| --- | --- |
| Workspace/crate version | `0.1.0` |
| Domain-separation labels | `.../v1` (room, device, ratchet/rk, store) |
| `LocalState.version` | `1` |
| Wire format version | **none** (no version byte on any frame) |

Versioning is currently **implicit**: it lives in the `/v1` suffix of the domain
strings and in `LocalState.version`, plus the assumption that both peers run
identical struct definitions.

### 10.2 What must be frozen before interop

An independent implementation can interoperate only if all of the following are
frozen and identical:

1. Noise protocol name string and PSK index (`Noise_XXpsk0_25519_ChaChaPoly_BLAKE2s`, `psk(0)`).
2. All four domain-separation labels, byte for byte.
3. Room ID hash: **BLAKE2b-512** with 16-byte truncation (not BLAKE2b-256).
4. Device MAC construction (HMAC-SHA256 key/message order).
5. Ratchet: `KDF_RK` (HKDF-SHA256, salt=RK, ikm=DH, info string, 64-byte split),
   `KDF_CK` (HMAC inputs `0x01`/`0x02`), zero nonce, header as AAD,
   `RatchetHeader` field order and integer widths, `MAX_SKIP`.
6. Frame enum variant **order** and struct field **order** (postcard is
   positional and not self-describing), including `Action` if ever serialized.
7. Store blob layout (nonce/ciphertext/AAD), `LocalState` field order,
   `DoubleRatchet` field order, `SkippedKey` field order.
8. Argon2id parameters and salt length.
9. BIP39 wordlist and normalization.
10. Semantics of `PeerFrame::Handshake.step` and the first-message payload
    convention.

### 10.3 Test vectors to add

None exist today. The following known-answer tests should be added, and are
required before any interop or production claim (cf. ADR 0001 "Risk accepted"):

| Vector | Inputs | Expected output |
| --- | --- | --- |
| **Room ID** | PSK `00 01 … 1f` | `roomID` hex |
| **Device MAC** | fixed PSK, room, device | 32-byte MAC hex |
| **Mnemonic round-trip** | fixed 32-byte entropy | the 24 BIP39 words |
| **Argon2id KDF** | passphrase + 16-byte salt | 32-byte `StoreKey` |
| **KDF_RK** | fixed RK, DH | 64 bytes → (RK', CK) |
| **KDF_CK** | fixed CK | (next_CK, MK) |
| **Ratchet message** | fixed ratchet state + plaintext | `RatchetHeader` + ciphertext bytes |
| **Skipped-key / out-of-order** | scripted message order | recovered plaintexts |
| **Noise transcript** | fixed static + ephemeral keys, fixed PSK | msg1/msg2/msg3 bytes + final handshake hash |
| **Frame encodings** | one value per variant | exact postcard bytes |
| **Store blob** | fixed key + state (+ fixed nonce) | exact blob bytes |

> **Blocker for reproducible vectors:** the core calls `getrandom` directly for
> every nonce and key pair, and `snow` generates ephemeral keys internally.
> There is currently **no RNG seam** to inject deterministic randomness, so the
> handshake, ratchet-message, and store-blob vectors cannot be reproduced
> deterministically without adding one (or recording vectors from a seeded
> build). This is the single biggest gap for interop testing.

---

## 11. Security notes (spec-relevant)

1. **The PSK is the trust root.** It determines the room ID, authenticates the
   Noise handshake (`psk0`), and MACs device registrations. Anyone who holds it
   is a full member; there is no revocation, no account recovery, and no
   distinction between the two humans cryptographically.
2. **The server cannot MITM without the PSK.** The `psk0` token is mixed into
   the first handshake message, so an active attacker (including the relay)
   without the PSK cannot complete the handshake, cannot impersonate a peer, and
   cannot forge a device MAC. A successful handshake *is* proof of PSK
   knowledge.
3. **Metadata is visible.** The relay learns the room ID, device IDs, IP
   addresses, connection times/durations, message counts, ciphertext sizes, and
   delivery timing. There is no padding, no cover traffic, and no sealed sender
   in v1. Ciphertext length ≈ plaintext length + header + 16-byte tag.
4. **Device identity is not bound to the Noise transcript.** The handshake
   authenticates "someone who holds the PSK", not a specific device ID. The
   mapping from a session to a peer device ID comes from the relay-supplied
   `from` field, and the device MAC only ties an ID to the PSK (not to the
   session). A malicious relay can therefore misattribute a session to a
   different device ID in the same room, though it still cannot read or forge
   message content. Per-device attribution is relay-dependent; per-human
   authenticity is not.
5. **`from` is unauthenticated by the relay but bounded by the ratchet.** If the
   relay delivers a payload with the wrong `from`, decryption fails (the client
   looks up the wrong session), so misrouting causes a failure rather than an
   impersonation of message content.
6. **Forward secrecy vs. post-compromise security.** The DH ratchet provides
   forward secrecy for past messages and post-compromise security for future
   messages after a DH step. It does **not** protect against compromise of the
   PSK itself, and it does not undo messages already read.
7. **Replay and reordering.** Message numbers plus the skipped-key store accept
   bounded out-of-order delivery and reject replays; the header is authenticated
   as AAD, so header tampering fails decryption. `MAX_SKIP` bounds the memory and
   CPU a malicious peer can force.
8. **Web-client-serving caveat.** In the usual deployment the same host serves
   the relay and the web app, so a malicious host can ship backdoored JavaScript
   that captures the mnemonic or plaintext before encryption. No protocol change
   fixes this; it is a code-distribution problem. The CLI is not subject to it.
   (See threat model R1.)
9. **No nonce randomness in the message layer.** Message AEAD uses an all-zero
   nonce and relies on message keys being single-use. This is the Double Ratchet
   convention and is safe **only if** the ratchet is never rewound (e.g. by
   restoring an old state file). State rollback is not detected and can cause
   key reuse. (See threat model R7.)
10. **Local store.** History and ratchet secrets are protected at rest only by
    the Argon2id-derived key; a weak passphrase is a weak store. The store salt
    is separate from the blob and must be persisted.

---

## 12. Deviations and mismatches

Resolved in favor of the implementation:

1. **Room ID hash.** The brief specified `BLAKE2b-256(...)[0..16]`. The code uses
   `Blake2b512` (BLAKE2b with a 64-byte digest) and truncates to the first 16
   bytes. Documented as BLAKE2b-512 in [§3.3](#33-room-id).
2. **Fingerprint casing.** `encchat-core`'s `Psk::fingerprint()` is uppercase
   hex; the WASM binding (`crates/wasm/src/lib.rs`) reimplements it as lowercase
   hex. Grouping is identical (8 groups of 4 hex chars). The core version is
   treated as canonical in [§3.6](#36-human-readable-fingerprint).
3. **`Noise_XXpsk0` token placement.** The brief said only that the PSK
   authenticates the handshake; the exact `psk0` expansion (`-> psk, e` …) is
   taken from the Noise spec rev 34 and documented in [§2.2](#22-primitive-inventory)
   and [§4.1](#41-protocol-name).

Everything else in the brief matched the code: PSK length, 24-word mnemonic,
device ID/MAC construction, handshake step numbers 0/1/2, first-write payload =
ratchet public key, `KDF_RK`/`KDF_CK` definitions, zero nonce + header-as-AAD,
`MAX_SKIP = 2000`, replay rejection, frame shapes, store layout and AAD, and the
`/v1` domain labels.

---

## 13. Underspecified items (to freeze)

These are gaps an independent implementer would hit. None are resolved by the
current code.

| # | Item | Impact |
| --- | --- | --- |
| U1 | **Relay not implemented** (`crates/server/src/main.rs` is a scaffold). | No defined online/offline transitions, queue flush format, or `Error.code` vocabulary. |
| U2 | **No RNG seam** for deterministic test vectors. | Handshake/ratchet/store known-answer tests are not reproducible ([§10.3](#103-test-vectors-to-add)). |
| U3 | **No wire version byte.** | No way to negotiate or detect format skew; postcard is positional. |
| U4 | **Argon2id parameters are library defaults**, not pinned. | A dependency bump could break or weaken stores ([§9.6](#96-passphrase-kdf-argon2id)). |
| U5 | **No handshake concurrency tie-break.** | Simultaneous `begin_handshake` from both devices overwrites pending state and fails; there is no device-ID-based tie-break. |
| U6 | **`Ack`/`Typing`/`Bye` semantics.** | `Ack` is never sent; `Typing` and `Bye` have no protocol effect beyond notices; no receipts/retransmission. |
| U7 | **Device list ordering / offline inclusion.** | Clients must not assume order; behavior unspecified ([§7.3](#73-device-list)). |
| U8 | **Message-size limits.** | Only `MAX_HANDSHAKE_MSG = 1024` (handshake) and Noise's 65535-byte cap are implied; no application message cap or fragmentation. |
| U9 | **`sent_at` clock trust.** | Sender-supplied, unvalidated; no skew bound or monotonicity requirement. |
| U10 | **Device-ID ↔ session binding.** | Not cryptographically bound; relies on relay `from` ([§11.4](#11-security-notes-spec-relevant)). |
| U11 | **Store salt storage.** | The salt is not part of the blob; its location/format is left to the frontend. |
| U12 | **`msg_id` semantics.** | No dedup, ordering, or acknowledgement protocol; `msg_id` is not bound to `sent_at`/`body` beyond the ratchet AAD. |
| U13 | **Duplicate/conflicting `Join`.** | Server-side behavior for ID conflicts or re-registration is undefined. |
| U14 | **Multi-room devices.** | Whether one device may join multiple rooms is undefined; `LocalState` holds one `device_id` and one room's sessions/history. |
