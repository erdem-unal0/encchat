# AGENTS.md — crates/core

The single crypto and protocol implementation, shared by the CLI and (through
`crates/wasm`) the browser. Correctness here is the whole product.

## Rules

- **No I/O, no clock, no threads.** Not `std::fs`, not `SystemTime`, not
  `tokio`. `getrandom` is allowed (it has a `js` backend for wasm).
- **Never hand-roll crypto.** Build on the existing primitives and only compose
  them. If a change feels like "implementing an algorithm", stop and reconsider.
- **Never log or `Debug`-print secrets.** `Psk` zeroizes on drop; keep it that
  way. Prefer returning `Error` variants that do not embed key material.
- **Adding a field to a `Serialize` struct changes the wire/disk format.**
  `envelope.rs` is postcard-encoded and `LocalState` is persisted; both are
  documented in `docs/protocol.md`, which must be updated in the same change.
  Prefer adding a new enum variant at the end over changing a struct.

## Map

| File | Responsibility |
| --- | --- |
| `psk.rs` | Pre-shared key, mnemonic encoding, room id, device MAC, fingerprint. |
| `handshake.rs` | Noise `XXpsk0` handshake; carries each side's ratchet key. |
| `ratchet.rs` | Double Ratchet: DH ratchet, chain keys, skipped keys, AEAD. |
| `envelope.rs` | Wire types: `ClientFrame`, `ServerFrame`, `PeerFrame`, `ChatPayload`. |
| `client.rs` | `ClientCore`: the state machine every frontend drives. |
| `store.rs` | At-rest encryption (Argon2id + XChaCha20-Poly1305). |

## Testing

- Unit tests are inline in each module.
- `tests/client_flow.rs` drives two `ClientCore`s through a full handshake,
  bidirectional messaging, multi-device fan-out, and nickname exchange. Add
  cases there for anything that changes observable behaviour.
- Run: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p encchat-core`.

## Frontend API

Frontends call `ClientCore` only: `new`/`from_state`, `device_id`,
`join_frame`, `verify_device`, `has_session`, `should_initiate`,
`begin_handshake`, `handle_frame`, `send_text_to_all`, `set_nick`,
`announce_nick`, `peer_nick`, `history`. Extend this rather than adding new
entry points.
