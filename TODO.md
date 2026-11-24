# TODO

A living list of what is next for encchat. Nothing here is a commitment or a
schedule; items are grouped by area and roughly ordered by value. See
[`docs/threat-model.md`](docs/threat-model.md) and
[`docs/protocol.md`](docs/protocol.md) for the reasoning behind several of them.

## Now / next

- [ ] **Surface nicknames in the web client.** The browser ignores
  `PayloadKind::Nick` and always shows the short device id. Show `<nick>` when
  known, and let the user set one (the CLI has `--nick` / `/nick`).
- [ ] **`/me` as a real action.** Today `/me waves` sends the literal text
  `* <nick> waves`, so a receiver sees `<nick> * <nick> waves`. Add a
  `PayloadKind::Action` and render it as an emote.
- [ ] **Protocol test vectors.** Freeze the wire format before a second
  implementation exists: the handshake transcript, ratchet chain derivation
  (`KDF_RK` / `KDF_CK`), and postcard encodings for every frame. The spec lists
  the exact gaps under "underspecified / to freeze".
- [ ] **Self-hosting.** A `Dockerfile` and a systemd unit (plus an example
  config) so the relay is one command to run.

## Protocol & security

- [ ] **`LocalState` versioning / migration.** `LocalState::version` exists but
  nothing consumes it; adding a field currently breaks existing state files.
  Decide on a migration story before the on-disk format is called stable.
- [ ] **Persist nicknames** across restarts (they are per-session today) once
  state migration exists.
- [ ] **Dead protocol variants.** `PeerFrame::Ack` is defined but never sent,
  and `Typing` / `Bye` have no real effect. Either implement delivery receipts
  and typing indicators, or remove the variants.
- [ ] **Fuzz and harden the frame decoders** (postcard input from the network is
  currently trusted after a length check) and review replay / reordering edge
  cases.
- [ ] **Metadata reduction.** The relay sees room id, device ids, message sizes
  and timing. Consider padding and sealed-sender style sender hiding.
- [ ] **Web client integrity.** A malicious host can serve modified JavaScript.
  Mitigate with pinned / reproducible builds or a signed release, so the served
  bundle can be verified out of band.

## Product / features (deliberately deferred)

- [ ] Cross-device history sync — each device keeps its own history today.
- [ ] Group chats — v1 is strictly 1:1.
- [ ] Federation between relays.
- [ ] File and image transfer, voice and video.
- [ ] Presence and other online-state signals.

## CLI polish

- [ ] **Tab-completion for command arguments** (currently completes command
  names only; `/theme <Tab>` could list presets and `/nick <Tab>` could list
  peers).
- [ ] **Late joiners miss history.** A peer that joins after a message was sent
  never sees it. Consider a backfill on session establishment, or at least a
  clear notice.
- [ ] **Alternate key bindings.** Each action has one binding today; allow a
  comma-separated list (for example, both `ctrl+c` and `ctrl+d` to quit).
- [ ] **Per-room themes.** The config is global; a room-specific profile would
  let one identity keep a different look from another.

## Housekeeping

- [ ] Add a `CONTRIBUTING.md`.
- [ ] Revisit the license: keep dual MIT/Apache-2.0, or switch to AGPL-3.0,
  which would require hosted forks to publish their changes (relevant to the
  "malicious host serves modified JS" caveat).
- [ ] Branch protection on `main` and release tagging.
- [ ] CI: cache Rust and npm builds, and add a web build check on
  Windows/macOS (the web job currently runs on Linux only).

## Done

- [x] Core crypto (Noise `XXpsk0` handshake + Double Ratchet), blind relay,
  CLI, and WASM-backed web client.
- [x] Multi-device fan-out with a deterministic initiator tie-break.
- [x] Encrypted local state (Argon2id + XChaCha20-Poly1305).
- [x] IRC-style CLI: scrollback, status bar, slash commands, and encrypted
  nickname announcements.
- [x] **CLI customization:** a full-screen settings editor with live preview,
  JSON config, nine theme presets, per-color overrides, configurable status bar
  and layout, timestamps, behavior, prompt template, and remappable keys.
- [x] Config-driven rendering with Unicode-aware wrapping, per-peer colors, an
  unread counter that preserves your scroll position, and `TestBackend` coverage
  for resize, tiny terminals, long lines, and wide characters.
- [x] MIT + Apache-2.0 license files.
