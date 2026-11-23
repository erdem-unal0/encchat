# ADR 0001: Rust core with WASM web client and a blind relay

- **Status:** Accepted
- **Date:** 2026-11-23
- **Deciders:** project maintainers
- **Related:** [threat model](../threat-model.md)

## Context

`encchat` is a self-hosted, end-to-end encrypted 1:1 chat app that must work from
a browser and from a terminal, on Linux, macOS, and Windows. The design
constraints that drive this decision:

- The server is assumed hostile. It must not be able to read messages, forge a
  peer, inject a device, or hold history.
- The same cryptography must run in the browser and in the CLI. Two independent
  implementations of a ratchet is two places to be wrong, and subtle divergence
  (nonce sizes, HKDF info strings, state serialization) is a security bug, not a
  portability bug.
- The project is small and early. There is no team to maintain a large protocol
  surface, no appetite for reimplementing a messenger spec, and no tolerance for
  a heavyweight runtime in the CLI.
- The threat model explicitly accepts metadata leakage, so the server can be a
  dumb relay rather than a store-and-forward service with an account system.

The question is how to organize the code so that one crypto core serves a
browser, a terminal, and a relay, without pulling in a full messaging stack.

## Decision

Build a Rust workspace with four crates and a TypeScript web UI:

| Component | Role |
| --- | --- |
| `crates/core` (`encchat-core`) | All cryptography and protocol state. No I/O, no clock, no sockets, no filesystem. Pure functions plus explicit state structs. |
| `crates/server` (`encchat-server`) | Blind WebSocket relay: room routing, in-memory TTL mailbox, no disk, no accounts. Treats payloads as opaque bytes. |
| `crates/cli` (`encchat-cli`) | Terminal client. Links `encchat-core` natively; owns sockets and local encrypted storage. |
| `crates/wasm` (`encchat-wasm`) | `wasm-bindgen` wrapper exposing `encchat-core` to JavaScript. |
| `web/` | TypeScript UI. Does no cryptography; calls the WASM module for every operation that touches a key. |

Key points of the decision:

1. **One crypto implementation.** `encchat-core` is compiled twice — once to
   WASM, once native. The browser and the CLI run the same code paths and the
   same serialized state format, so protocol behavior is identical and test
   vectors can be shared.
2. **Crypto code has no ambient authority.** `encchat-core` cannot open a socket,
   read a file, or read the clock. Keys and RNG input are passed in. This makes
   the core auditable in isolation and testable deterministically.
3. **The server stays dumb and stateless on disk.** Room is an opaque 16-byte ID
   derived from the PSK (`BLAKE2b-128(PSK, "encchat/room/v1")`). The relay routes
   frames and parks them in a TTL mailbox in RAM. It never parses ciphertext.
4. **PSK-based identity.** There is no account system, no key directory, and no
   trust-on-first-use. Identity is a 32-byte PSK shared out of band, presented as
   a 24-word BIP39 mnemonic. Noise `XXpsk0` authenticates the handshake; peers
   authenticate devices by MAC under the PSK.
5. **Handshake and stream are separate layers.** Noise `XXpsk0` (X25519 /
   ChaChaPoly / BLAKE2s) establishes an authenticated, forward-secret transport
   and carries initial Double Ratchet public keys in its payload. The message
   stream is a Double Ratchet (X25519 DH ratchet, HKDF-SHA256, HMAC-SHA256 chain
   keys) over XChaCha20-Poly1305, with the ratchet header as AAD and
   skipped-message keys for out-of-order delivery.
6. **Per-device sessions.** Each device has a random 16-byte ID and its own
   ratchet session with each peer device. Sends fan out one ciphertext per
   recipient device. Devices are registered with a MAC under the PSK, so the
   server cannot add one.

## Alternatives considered

### A. Pure TypeScript (libsodium.js / WebCrypto) for web, Node for CLI

One language, one runtime family, and an easy web build.

**Rejected because:** the CLI would depend on a Node runtime and its crypto and
keychain story, which is a poor fit for a small native-feeling terminal tool.
More importantly, web and CLI crypto would still be *different* code paths
(WebCrypto vs. Node crypto vs. libsodium), so state and wire compatibility would
be maintained by hand. Rust's ecosystem also has direct, well-maintained
implementations of the exact primitives we need (`snow`, `x25519-dalek`,
`chacha20poly1305`, `hkdf`, `argon2`, `bip39`), and compiling to WASM gets us both
targets from one source. JS also makes it easy to accidentally handle key bytes
as ordinary numbers/strings and to leak them into logs or devtools.

### B. Use Matrix or XMPP as the transport and storage layer

Both are mature, federated, and already solved multi-device and message
delivery. Building a client against an existing protocol means less novel crypto.

**Rejected because:** both are large surfaces with server-side persistence,
account models, and metadata exposure far beyond what we want. Matrix homeservers
store room state and history; XMPP deployments differ. Federation is out of scope
for v1, and neither protocol gives the "blind relay with no disk" property
without substantial extension work. The goal is a small, auditable deployment, not
interoperability with an ecosystem.

### C. Use libsignal / the Signal protocol only

The Signal protocol is the well-reviewed reference for exactly the message
stream we want (Double Ratchet, prekeys, per-device sessions).

**Rejected because:** libsignal is designed around Signal's service semantics —
server-held prekey bundles, registration IDs, a specific device-linking flow — and
integrating it as a library means adopting its assumptions and its FFI/build
complexity in both native and WASM targets. Our identity model (a shared PSK, not
per-user identity keys) and our device model (PSK-authenticated registrations)
differ enough that we would fight the library at the edges. We keep the
Double Ratchet design and the Noise handshake, and accept that this means
implementing and testing the ratchet ourselves — see the risk note below.

**Risk accepted:** hand-rolled protocol composition is where projects get burned.
Mitigations are non-negotiable: known-answer tests for HKDF/HMAC/AEAD, Noise test
vectors from the spec, ratchet tests covering out-of-order and skipped messages,
and a documented state format. An external review is a prerequisite for any
production claim.

### D. Noise transport only, no Double Ratchet

Noise already gives an authenticated, forward-secret channel. Skip the ratchet
and use the Noise session for the whole conversation.

**Rejected because:** a Noise session is forward-secret per handshake but has no
post-compromise security — an attacker who learns the session state can decrypt
everything until the next handshake, and there is no per-message ratcheting to
move the key material forward. It also has no skipped-message support for
out-of-order delivery, and a long-lived chat session would need either a permanent
Noise session per peer device (no key evolution) or constant rehandshakes (cost
and metadata churn). We use Noise for what it is good at — mutual
authentication with the PSK and initial key agreement — and the Double Ratchet for
the message stream.

### E. Single binary that also serves the web app, with server-side history

Simplest deployment: one process, one database, web and CLI share history through
the server.

**Rejected because:** it directly contradicts the threat model. Server-side
history means the server holds user data; shared history means cross-device sync,
which is out of scope for v1 and requires a much larger key-management design.
Per-device local history is the honest, smaller thing.

## Consequences

### Good

- **One crypto core, two targets.** Byte-identical protocol behavior between web
  and CLI, shared test vectors, one place to fix a bug.
- **The core is auditable in isolation.** No I/O and no clock in
  `encchat-core` means its inputs and outputs are explicit, and tests are
  deterministic.
- **Small trust surface.** The server cannot read payloads, has no disk, and has
  no account system. The CLI is immune to the served-JavaScript attack; the web
  client is not (see threat model R1).
- **Good fit for the primitives.** Rust libraries for Noise, X25519, ChaCha20-
  Poly1305, Argon2id, and BIP39 are mature, and WASM keeps the browser cryptography
  in the same code as the CLI.
- **Cheap to self-host.** A relay with an in-memory mailbox and no database is a
  small binary behind a reverse proxy.

### Bad

- **WASM boundary overhead and build complexity.** `wasm-bindgen`, a Node build
  step, and two Rust targets (native + wasm32) add tooling that a pure-JS project
  would not have. WASM binary size and startup cost matter on low-end mobile
  browsers.
- **We implement the ratchet ourselves.** No library gives us exactly this
  composition, so composition bugs, state-serialization bugs, and nonce handling
  are our responsibility. This is the largest engineering risk in the project.
- **No cross-device history.** Users will expect their web session and CLI to
  agree. They will not. Reopening on another device starts empty, and losing a
  device loses its history.
- **Fan-out cost per device.** Each send produces one ciphertext per recipient
  device; adding devices adds CPU and message size. Device lists are learned
  incrementally, so a peer's newly added device may be missing from an early
  send.
- **PSK-only membership.** No revocation, no account recovery, no way to remove a
  device short of rotating the PSK and starting a new room. Losing the 24 words
  is unrecoverable.
- **Reproducibility is unsolved.** Shipping a WASM blob to a browser via the same
  server that could be malicious means the web client's integrity depends on a
  build process we have not yet built (threat model R1). Until then, the CLI is
  the only client whose code the host cannot swap.

## Open questions

1. **Ratchet review.** Do we submit the core for external review before v1, and
   does that review cover the state format and the skipped-key store, or only the
   wire protocol?
2. **Padding policy.** Do we pad ciphertexts to size buckets to blunt traffic
   analysis, knowing it costs bandwidth and still leaks timing? Deferred for v1,
   but the decision should be recorded.
3. **Mailbox TTL and capacity.** What TTL and per-room queue limits balance
   offline delivery against memory-abuse on the relay? Needs load testing, not a
   guess.
4. **PSK rotation.** Is there a migration path (new room ID, re-registered
   devices) for a leaked PSK, or is "start over with a new mnemonic" the whole
   answer?
5. **Reproducible web builds.** Which mechanism do we commit to — deterministic
   WASM builds plus a hash published out of band, signed releases, or a
   third-party build service? Without one, R1 stays open.
6. **WASM packaging.** One bundle for all browsers, or separate builds for size
   and for older engines? Where does the WASM blob live relative to the JS that
   loads it, given the trust argument above?
7. **CLI key storage.** OS keychain by default, or passphrase by default with the
   keychain as an opt-in? Keychain behavior differs across Linux desktop
   environments, which affects support burden.
8. **Device approval UX.** Registrations are PSK-authenticated, so any PSK holder
   can add a device. Do we want an explicit peer approval step (a fingerprint to
   compare out of band), knowing it is a UX cost and does not stop a malicious PSK
   holder?
9. **State format stability.** `postcard` is compact but not self-describing.
   Do we commit to a versioned state format now, before users have history worth
   preserving?
