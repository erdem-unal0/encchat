# encchat threat model

Scope: `encchat` v1 — self-hosted, end-to-end encrypted 1:1 chat over a blind
WebSocket relay, used from a browser or a CLI on Linux, macOS, and Windows.

This document describes what the design protects, what it leaks, and what it does
not attempt to solve. It is a design document, not an audit report. The
implementation is an early scaffold and has not been reviewed by anyone
independent.

## System summary

- Two people share a 32-byte PSK out of band. BIP39 encodes it as 24 words for
  reading aloud.
- Each person runs one or more devices. A device has a random 16-byte device ID.
- Devices derive `roomID = BLAKE2b-128(PSK, "encchat/room/v1")` and connect to the
  relay, which pairs them by that opaque 16-byte value.
- A Noise `XXpsk0` handshake authenticates both ends by the PSK and exchanges
  initial Double Ratchet keys. Messages then flow as Double Ratchet ciphertexts.
- Every device maintains one ratchet session per peer device; a send fans out one
  ciphertext per recipient device.
- The relay keeps a short-lived in-memory mailbox (TTL) and writes nothing to
  disk.

## Assets

| Asset | Sensitivity | Where it lives |
| --- | --- | --- |
| PSK (32 bytes / 24 words) | Total compromise of the conversation | Human memory, paper, device secure storage |
| Room ID | Low–medium; links two devices to one conversation | Server memory, network metadata |
| Device ID | Low; identifies a device within a room | Server memory, device records |
| Message plaintext | High | Endpoints only (RAM, encrypted history) |
| Local history + ratchet state | High; past and future messages | Device disk, encrypted at rest |
| Ratchet session keys | High; compromise breaks forward secrecy for that session | Endpoint RAM, encrypted local state |
| Passphrase (local storage) | High; unlocks the local store | Human memory, OS keychain (CLI) |
| Appearance settings | Low; no message or key material | `config.json` (CLI), plain `localStorage` (web) |
| Nickname | Low; a cosmetic self-asserted label | Encrypted `Nick` payload, peer memory |
| Local peer aliases | Low; maps a device ID to a name, never transmitted | `localStorage` only |

The PSK is the root of everything: it determines the room, authenticates the
handshake, and MACs device registrations. Losing it means losing the ability to
join; leaking it means losing the conversation.

## Adversaries

### A1. Passive network attacker

Can observe, delay, reorder, and drop packets; cannot modify the PSK or read
device memory.

**Goal:** read messages; correlate users; later decrypt recorded traffic.

**Result:** sees WebSocket frames addressed by room ID, connection timing, frame
sizes, and connection endpoints. Cannot decrypt message contents. Recorded
traffic stays undecryptable as long as endpoints are not compromised, thanks to
forward secrecy.

### A2. Malicious or compromised server

Operates the relay, and in the usual deployment also serves the web app. Can read
and rewrite everything in transit, log metadata, drop or reorder frames, inject
fabricated peers and devices, and go offline.

**Goal:** read messages, impersonate a peer, inject a device, or serve backdoored
client code.

**Result:** cannot read message contents. Cannot complete a Noise `XXpsk0`
handshake without the PSK, so it cannot MITM the handshake or impersonate a peer
to a device that has the PSK. Cannot register a fake device, because peers check
`HMAC-SHA256(PSK, ...)` on device registration. **Can** serve modified JavaScript
to a browser client (see residual risk R1), which bypasses all of the above.

### A3. Malicious peer (one of the two key holders)

Possesses the PSK and is therefore an authorized participant. Can read everything,
send anything, and keep a copy of the transcript.

**Goal:** impersonate the other human, misattribute messages, or deny sending
something.

**Result:** nothing in the protocol provides non-repudiation or long-term
identity beyond "holds the PSK". Both holders of the PSK are, cryptographically,
the same principal set. A peer can also add their own devices at will, and those
devices are indistinguishable from the peer's legitimate devices to anyone else
who holds the PSK.

### A4. Stolen or seized device

Attacker has the disk and possibly the unlocked session; may be able to run code
on the device.

**Goal:** read history, read future messages, or impersonate the device.

**Result:** with only the disk, the attacker needs the local passphrase to open
the encrypted store (Argon2id-derived key). With the passphrase or an unlocked
session, they get that device's history and ratchet state, and can send as that
device for as long as the session lasts. Post-compromise security means the
peer's future ratchet steps eventually move the conversation out of reach of a
compromised *ratchet state*, but it does not undo what was already read, and it
does not help if the attacker holds the PSK itself.

### A5. Insider with server-host access

Has root on the relay host: can read memory, instrument the process, and modify
the binary and the served web assets.

**Goal:** same as A2.

**Result:** same as A2. Root on the relay is equivalent to a malicious server.
The mailbox is in memory, so a root insider can read queued ciphertexts — but
those are ciphertexts.

### A6. Malicious device in the room

Anyone who obtains the PSK can derive the room ID, join, register a device, and
read/send. There is no separate invite or revocation mechanism in v1.

**Result:** possession of the PSK *is* membership. There is no way to kick a
device or rotate a leaked key without establishing a new PSK and starting over.

## What is protected

- **Message contents** against A1, A2, A5. AEAD with per-message keys from the
  Double Ratchet; the ratchet header is authenticated as AAD, so header tampering
  fails decryption.
- **Handshake integrity** against A2/A5. Noise `XXpsk0` binds both handshake
  messages to the PSK; an active MITM without the PSK cannot finish it.
- **Device authenticity** against A2/A5. Device registrations carry a MAC under
  the PSK; peers reject unauthenticated devices.
- **Forward secrecy** against later compromise of a ratchet state. Past messages
  remain unreadable if the current ratchet keys and chain keys are lost.
- **Post-compromise security** against a transient ratchet-state compromise: after
  a DH ratchet step with fresh randomness, the attacker loses the ability to read
  new messages.
- **Replay and reordering.** Ratchet message numbers plus skipped-message keys
  reject replays and accept bounded out-of-order delivery; AEAD prevents
  tampering.
- **History at rest** against disk-only attackers (A4 without the passphrase).

## What is leaked

Be explicit: this is not a metadata-private system.

| Leaked to | What |
| --- | --- |
| Relay server (and its host) | Room ID (16 bytes), connecting device IDs, IP addresses, connection times and durations, message counts, ciphertext sizes, delivery timing, which devices are online together |
| Passive network observer | Same as above, modulo what TLS hides (TLS hides frame contents but not connection endpoints/timing) |
| Both participants | Each other's device IDs, online status, and advertised nicknames |
| Anyone with the PSK | Room ID, ability to join, full transcript |

Consequences worth stating plainly:

- **Traffic analysis works.** Message sizes and timing reveal conversation
  activity, roughly how much is said, and when both parties are online. A
  careful observer can distinguish "idle" from "active" and often guess message
  type (short reply vs. long paragraph).
- **Linkability.** The room ID is stable for a given PSK, so repeated sessions
  with the same pair are linkable by the server across time, as are device IDs.
- **No cover traffic, no padding policy in v1.** Ciphertext length is close to
  plaintext length plus header and tag overhead.
- **The server knows it is an encchat relay** if it is the one operating it, and
  sees the number of devices per room.
- **BIP39 wordlists are public**, so the mnemonic's *format* is recognizable if
  someone sees it; only the specific 24 words are secret. A photographed mnemonic
  is a total compromise.

## Assumptions

These are load-bearing. If one fails, the guarantees above degrade or vanish.

1. **The PSK is exchanged over a confidential, authenticated channel**, or in
   person. A PSK that travels over plaintext chat, SMS, email, or a support ticket
   is compromised, and everything built on it is broken.
2. **Endpoints are not compromised** while in use. Malware, a hostile browser
   extension, a keylogger, or a debugger attached to the process defeats E2EE by
   definition. The protocol protects data in transit and at rest, not a running
   process on an owned machine.
3. **The RNG is good.** Room IDs, device IDs, DH keys, and nonces come from the
   OS CSPRNG via `getrandom`. A broken or virtualized RNG (bad VM snapshot, cloned
   disk image without reseeding) can repeat keys and nonces. XChaCha20-Poly1305's
   192-bit random nonces make accidental reuse unlikely, but they do not fix a
   deterministic RNG.
4. **The cryptographic primitives hold.** X25519, ChaCha20-Poly1305, BLAKE2s,
   BLAKE2b, HKDF-SHA256, HMAC-SHA256, Argon2id, and the Noise/Double Ratchet
   constructions are assumed sound as implemented by their libraries. No claim is
   made that this project's *use* of them has been reviewed.
5. **The user picks a real passphrase** for local storage, with adequate Argon2id
   parameters. A guessable passphrase plus a stolen disk is a plaintext history.
6. **The web client is what it claims to be** — see R1. This assumption is the
   one most likely to fail silently, and it is the reason the CLI exists.
7. **Clocks are roughly sane.** TTL mailboxes and replay windows depend on
   monotonic/relative time handling; a wildly wrong clock can cause dropped
   messages or premature mailbox expiry, though not plaintext disclosure.

## Non-goals (v1)

Deliberately excluded, not merely unimplemented:

- **Metadata privacy from the server.** No sealed sender, no mixnet, no
  cover traffic, no size padding, no timing obfuscation. The server knows who
  talks to whom, when, and how much.
- **Group chat.** Exactly two principals; no sender keys, no MLS.
- **Federation.** One relay per deployment; no server-to-server protocol.
- **File transfer, images, audio, video, calls.** Text only.
- **Cross-device history sync.** Web and CLI are separate devices with separate
  histories. Switching devices does not bring the transcript along.
- **Account recovery.** Lose the 24 words and you lose the room. There is no
  reset, no server-side backup, no escrow.
- **Key revocation / device removal.** PSK holders are members; the only remedy
  is a new PSK.
- **Non-repudiation and verifiable identity.** Both PSK holders are equivalent;
  neither can prove to a third party who sent what.
- **Post-quantum resistance.** X25519 and the Noise/Double Ratchet DH steps are
  not PQ-safe. Recorded traffic could be decrypted by a future quantum adversary
  only if forward secrecy were also broken.
- **Multi-user presence, typing indicators, receipts, message editing.**
- **Server authentication.** The relay's identity is not pinned; it does not need
  to be, since it is assumed hostile and authenticated by nothing.

## Residual risks

**R1. Backdoored web client (highest practical risk).** In the usual deployment,
the same server hosts the relay and serves the web app. A malicious host can ship
JavaScript that captures the mnemonic or the decrypted plaintext before encryption
ever happens. No protocol change fixes this; it is a code-distribution problem.
Partial mitigations: pinned or reproducible builds, code signing, subresource
integrity for third-party assets, and independent builds whose hash users compare.
Full mitigation: use the CLI, which the server cannot rewrite. Treat the web
client as "convenient, not trustworthy against its own host" until a reproducible
build process is in place and documented.

**R2. PSK leakage at distribution.** If the mnemonic is ever transmitted over a
plaintext or weakly authenticated channel, an observer can derive the room, join
as a peer, and read everything going forward. This is the single most likely
real-world failure. There is no in-protocol detection of a pre-shared key that a
third party also knows.

**R3. Compromised endpoint.** Malware or a hostile browser extension reads
plaintext directly. Nothing in the protocol can detect this. On the web this
includes the browser itself, its auto-update channel, and every extension with
page access.

**R4. Local passphrase weakness.** Argon2id raises the cost of an offline attack
on a stolen disk but does not make a weak passphrase strong. The CLI's OS-keychain
option moves the secret to the OS, which inherits the OS account's security.

**R5. Denial of service and availability.** The relay can drop, delay, reorder,
or refuse traffic, and can expire the mailbox at will. There is no delivery
guarantee, no offline queue beyond the TTL, and no fallback transport.

**R6. Device-ID and registration races.** Device registration is authenticated by
a MAC under the PSK, so the server cannot forge a device, but it can withhold,
replay, or reorder registrations within the validity window. The protocol does not
provide a globally consistent device list; peers learn devices as they appear.

**R7. Ratchet-state loss and rollback.** Restoring an old encrypted state file
(a VM snapshot, a backup) can cause nonce/key reuse or message loss. The design
assumes a device's local state moves forward and is not rewound; there is no
built-in detection of stale state beyond decryption failures.

**R8. Nickname and appearance spoofing.** A nickname is a self-asserted label,
not an identity: either side can choose any name, including one that looks like
the other person. Local aliases are the mitigation on the web; they override the
peer's advertised nickname locally and are never sent anywhere. An imported
appearance profile is untrusted input, but it can only change local colors,
layout and labels: there is no custom CSS, script or remote asset loading. If
custom CSS is ever added, treat it as a new exfiltration and UI-spoofing surface
and extend this section.

**R9. Implementation risk.** This is unaudited, early-stage code. Constant-time
handling of secrets, zeroization, error paths that leak secrets, and
serialization edge cases are exactly the places where implementations break, and
none of it has been independently reviewed. Assume bugs exist.

**R10. No forward secrecy against PSK compromise.** The PSK authenticates the
handshake but does not decrypt message streams directly; still, anyone who holds
the PSK can *join* and read future messages, and a compromised endpoint with the
PSK is game over. Forward secrecy protects the ratchet state, not membership.

**R11. Traffic analysis and coercion.** An observer can tell that two parties are
talking, how much, and when. In a jurisdiction that can compel disclosure of a
device or a person, "the ciphertext is unreadable" is not a defense against being
forced to hand over the passphrase or the 24 words.
