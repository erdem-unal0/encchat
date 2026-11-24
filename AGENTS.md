# AGENTS.md

Guidance for agents working in this repository. Keep it short — this file is
loaded at the start of every session. Per-crate detail lives in nested
`AGENTS.md` files that load only when you touch that crate.

## What this is

`encchat`: self-hosted, end-to-end encrypted **1:1** chat, usable from a CLI or a
web app. A blind relay routes ciphertext between devices; it never sees
plaintext, nicknames, or history. Identity is a 32-byte pre-shared key that the
two humans exchange out of band, shown as a 24-word mnemonic.

Rust workspace (crypto core, relay, CLI, WASM bindings) + a TypeScript web
client. One crypto implementation is shared by CLI and browser.

## Commands

On **Windows PowerShell**, `cargo` is not on `PATH`. Use the full path:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy --all-targets
```

On Linux/macOS use `cargo` / `npm` directly.

One-shot verification (format, lint, test, web build):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check.ps1
```

Web client (from `web/`), on Windows use `npm.cmd`:

```powershell
npm.cmd install
npm.cmd run build:wasm   # required before dev/build after a fresh clone
npm.cmd run build
npm.cmd run test:wasm
```

End-to-end smoke test (needs the relay running on `127.0.0.1:8787`):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/smoke.ps1
```

## Layout

| Path | Contents |
| --- | --- |
| `crates/core` | Crypto + protocol, **no I/O**, wasm-compatible. Shared by all frontends. |
| `crates/server` | Blind WebSocket relay (axum) + in-memory TTL mailbox. |
| `crates/cli` | Terminal client (`keygen`, `room`, `chat`). |
| `crates/wasm` | `wasm-bindgen` surface over `core` for the browser. |
| `crates/appearance` | Portable themes, layout, labels and timestamp settings for both frontends. |
| `web/` | Vite + TypeScript client. `web/pkg/` is generated, never edited. |
| `docs/` | Protocol spec, threat model, ADRs. |

## Hard invariants — do not break

- **`encchat-core` has no I/O and no clock.** No `std::fs`, sockets, or
  `SystemTime`. `getrandom` is fine (it has a browser backend).
- **Never hand-roll crypto.** Compose the existing primitives: Noise (`snow`,
  `handshake.rs`), Double Ratchet (`ratchet.rs`), AEAD (`chacha20poly1305`),
  at-rest store (`store.rs`), identity (`psk.rs`).
- **The relay stays blind.** It must not parse `PeerFrame`, see plaintext, or
  persist anything. Anything user-visible (messages, nicknames) is encrypted
  inside the ratchet as a `ChatPayload`, never sent as a bare `PeerFrame`.
- **Wire format changes update `docs/protocol.md`** in the same change, and vice
  versa. Prefer additive changes (a new enum variant at the end).
- **Frontends never touch keys.** CLI and web both drive `ClientCore`; the web
  goes through `crates/wasm`.
- **Appearance stays local.** `encchat-appearance` is UI preference, not
  protocol: nothing in it is sent to the relay or a peer. Nicknames are the
  exception and already travel as encrypted `PayloadKind::Nick`; local peer
  aliases never leave the device.

## Gotchas

- `web/pkg/` is generated and git-ignored: run `npm run build:wasm` before
  `dev`/`build` after a fresh clone.
- Windows PowerShell blocks `npm.ps1` (execution policy) — use `npm.cmd`. Run
  `.ps1` files with `powershell -ExecutionPolicy Bypass -File ...`.
- The relay accepts the WebSocket upgrade on both `/` and `/ws`.
- `LocalState` is postcard-encoded, so adding a field breaks existing state
  files. See `TODO.md`.
- Nicknames are labels, not identities: they are encrypted and authenticated,
  but a peer chooses its own and two peers may collide.

## Docs map — read only what you need

- `README.md` — overview and quickstart.
- `TODO.md` — planned work and known limitations.
- `docs/web-customization.md` — web settings page, portable profiles, nicknames.
- `docs/protocol.md` — full wire spec (~1,000 lines). **Only** needed when
  changing the protocol or frame types.
- `docs/threat-model.md` — what is and is not protected. Read before any
  security-sensitive change.
- `docs/adr/` — architecture decisions and the alternatives considered.

## How to task an agent in this repo

Scope tightly and give everything up front:

1. Name the **exact directory** the agent may edit, and say explicitly that
   other crates are owned by other workstreams.
2. List the **exact files to read** and the **exact API** to call (signatures).
3. Give the **exact command** that must pass before reporting.
4. Require **tests**, and prefer pure, unit-testable logic over untested
   rendering or I/O. For TUIs, ask for `ratatui` `TestBackend` tests.

Nested `AGENTS.md` files already carry the per-crate detail; point agents at
them rather than restating it.
