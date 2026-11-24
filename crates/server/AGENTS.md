# AGENTS.md — crates/server

A **blind** WebSocket relay. It forwards opaque, already-encrypted payloads
between devices in a room and keeps nothing on disk.

## Rules

- **Never parse `PeerFrame` or look at payload contents.** Payloads are opaque
  bytes. The relay may see room ids, device ids, sizes, and timing — nothing
  else.
- **Never validate device MACs.** The relay has no key and cannot; peers verify
  MACs themselves. Just store and echo them.
- **No logging of payloads or room/device contents** at info level.
- **No persistence.** Only an in-memory TTL mailbox for offline devices.

## Map

| File | Responsibility |
| --- | --- |
| `relay.rs` | Pure routing state (no I/O): join, device list, deliver-or-queue. Unit-tested directly. |
| `mailbox.rs` | Per-device FIFO with a size cap and TTL. |
| `http.rs` | axum HTTP/WebSocket front end; `build_router` is exposed for tests. |
| `main.rs` | Argument parsing and serving. Thin. |

## Behaviour to preserve

- On join, reply `Joined` (all devices incl. self) and flush the mailbox, then
  broadcast the updated device list to other live members so they learn about
  the newcomer.
- Deliver to a live connection, else queue in the mailbox. Keep a device's
  registration after it disconnects.
- The WS upgrade is served on both `/` and `/ws`.

## Testing

- `tests/relay_routing.rs` drives `Relay` directly (no network).
- `tests/e2e_ws.rs` runs two real clients through a real relay over WebSockets.
- Run: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p encchat-server`.
