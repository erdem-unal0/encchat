# encchat

Self-hosted, end-to-end encrypted 1:1 chat with an IRC feel: you join a room with
a key, talk, and leave. Nothing is stored by the server. Usable from a web app or
a CLI.

Two people share a 32-byte pre-shared key (PSK) out of band. The PSK is printed as
a 24-word BIP39 mnemonic so it can be read aloud. Everything else — room
derivation, handshake, message encryption, multi-device registration — follows
from that key.

**Status: working core, CLI and relay.** The Rust core, the CLI client and the
relay server build and pass their tests, including an end-to-end test that runs
two clients through a real relay. The web UI is in progress. Nothing here is
audited or production-ready, and the on-disk/on-wire formats are not frozen.

## Design at a glance

- **One crypto implementation, two frontends.** The Rust core (`encchat-core`,
  no I/O) is compiled to WASM for the browser and linked natively into the CLI.
- **The server is blind.** It relays WebSocket frames between devices in an
  opaque 16-byte room and holds a short-lived in-memory mailbox. It has no
  accounts, no disk storage, no message logs, and never sees the PSK.
- **Room ID.** `roomID = BLAKE2b-512("encchat/room/v1" || PSK)` truncated to 16
  bytes. The server learns only this 16-byte value, which is not invertible to the
  PSK.
- **Handshake.** Noise `XXpsk0` (X25519 / ChaChaPoly / BLAKE2s). The PSK
  authenticates both ends, so a malicious server cannot MITM the handshake. The
  handshake payload carries each side's initial Double Ratchet DH public key.
- **Messages.** Double Ratchet (X25519 DH ratchet, HKDF-SHA256, HMAC-SHA256 chain
  keys) with XChaCha20-Poly1305 AEAD, ratchet header as AAD. Forward secrecy and
  post-compromise security, plus skipped-message keys for out-of-order delivery.
- **Multi-device.** Each device has a random 16-byte device ID and its own ratchet
  session per peer device. Devices register with a MAC under the PSK
  (`HMAC-SHA256(PSK, ...)`), so the server cannot inject a device. Sends fan out
  one ciphertext per recipient device.
- **Local storage.** History and ratchet state are encrypted at rest with
  XChaCha20-Poly1305; the key comes from a passphrase via Argon2id (the CLI may use
  the OS keychain instead).

## Architecture

```
        out of band: 32-byte PSK  ==  24-word BIP39 mnemonic
        (in person, or over an existing E2EE channel)
             |                                        |
             v                                        v
  +----------------------+                 +----------------------+
  | CLI device           |                 | Web device           |
  | encchat-cli          |                 | browser              |
  |   encchat-core       |                 |   encchat-wasm       |
  |   ratchet sessions   |                 |   (same core)        |
  |   encrypted history  |                 |   encrypted local    |
  |   passphrase/keychain|                 |   state + passphrase |
  +----------+-----------+                 +----------+-----------+
             |                                        |
             |   WebSocket: Noise transport frames,   |
             |   then ratchet ciphertexts only        |
             +------------------+---------------------+
                                v
                 +-------------------------------+
                 | encchat-server                |
                 | blind WebSocket relay         |
                 | room = BLAKE2b-128(PSK)       |
                 | in-memory TTL mailbox         |
                 | no disk / accounts / history  |
                 +-------------------------------+
```

## Repository layout

| Path             | Contents                                                        |
| ---------------- | --------------------------------------------------------------- |
| `crates/core`    | `encchat-core` — crypto and protocol primitives, no I/O        |
| `crates/server`  | `encchat-server` — blind WebSocket relay and TTL mailbox        |
| `crates/cli`     | `encchat-cli` — terminal client (native core)                   |
| `crates/wasm`    | `encchat-wasm` — wasm-bindgen wrapper around the core           |
| `web/`           | TypeScript web UI (WIP)                                         |
| `docs/`          | Threat model, ADRs, protocol notes                              |

## Quickstart

Requires a stable Rust toolchain (see `rust-toolchain.toml`) and, for the web UI,
Node.js with npm.

Generate an identity. Write the 24 words down; they *are* the key:

```sh
cargo run -p encchat-cli -- keygen
```

Derive the room ID from a mnemonic:

```sh
cargo run -p encchat-cli -- room "<24 words>"
```

Start the relay (in one terminal):

```sh
cargo run -p encchat-server
# listening on 127.0.0.1:8787; see --help for bind, mailbox TTL/size, payload cap
```

Chat (in two terminals, using the same mnemonic). On first run each client asks
for a passphrase that encrypts its local state:

```sh
cargo run -p encchat-cli -- chat --server ws://127.0.0.1:8787 --mnemonic "<24 words>"
```

You should see `secure session with <id>` on both sides, after which anything you
type is sent to the peer. `/quit` exits.

Run the test suite (core, relay, CLI, plus the end-to-end WebSocket test):

```sh
cargo test
```

Web UI (in progress):

```sh
cd web
npm.cmd install
npm.cmd run dev
```

> **Windows note:** use `npm.cmd` rather than `npm`. PowerShell resolves `npm` to
> `npm.ps1`, which is commonly blocked by the default execution policy
> (`running scripts is disabled on this system`).

## Security

Read [docs/threat-model.md](docs/threat-model.md) before trusting this with
anything that matters. Two points belong in the summary:

1. **The web app can be backdoored by the host that serves it.** A malicious or
   compromised server can serve modified JavaScript that exfiltrates the PSK on
   entry, no matter how strong the cryptography below it is. Mitigations are
   pinned or reproducible builds, code signing, or using the CLI — the CLI has no
   such weakness. There is no way to verify in-browser that the JavaScript you
   loaded is the JavaScript that was audited.
2. **PSK distribution is the weak link.** If the 24 words ever travel over a
   plaintext channel (chat, SMS, email, a screenshot in a ticket), the scheme is
   broken. Exchange them in person or over an existing end-to-end encrypted
   channel.

The server is assumed hostile. It sees room IDs, device IDs, message sizes, and
timing; it cannot read message contents, forge a peer's identity, or complete a
MITM handshake.

## What is deliberately not here yet

- Hiding metadata from the server: room ID, device IDs, message sizes, and
  timing are all visible to it.
- Group chats (v1 is strictly 1:1).
- Federation between servers.
- File transfer, images, voice, or video calls.
- Cross-device history sync — each device keeps its own encrypted history.
- Audits, fuzzing at scale, and a stable on-disk or on-wire format.
- Multi-user presence, typing indicators, or delivery receipts beyond what the
  protocol needs.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in this project, as defined in the Apache
2.0 license, shall be dual licensed as above, without any additional terms or
conditions.

> Note: this is a permissive license. Anyone may fork this project, modify the
> web client, host it, and share nothing back. Given that the server serves the
> JavaScript, that is a real limitation — see
> [docs/threat-model.md](docs/threat-model.md). If you later want hosted
> modifications to remain open, AGPL-3.0 is the license to consider.
