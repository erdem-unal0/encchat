# encchat web client

Browser client for **encchat**: end-to-end encrypted 1:1 chat over a blind relay.
The UI does no cryptography. Every operation that touches a key is performed by
the WebAssembly module in `pkg/`, which is a thin `wasm-bindgen` wrapper around
`encchat-core` — the same code the CLI runs.

## Prerequisites

- Rust with the `wasm32-unknown-unknown` target:
  ```sh
  rustup target add wasm32-unknown-unknown
  ```
- Node.js 18+ and npm.

## Run it

From the repository root:

```sh
# 1. Build the WASM bindings into web/pkg/
cd web
npm install          # on Windows use: npm.cmd install
npm run build:wasm   # on Windows use: npm.cmd run build:wasm

# 2. Start the dev server
npm run dev          # on Windows use: npm.cmd run dev
```

Then open the printed URL (default <http://localhost:5173>).

To produce a static production bundle in `web/dist/`:

```sh
npm run build        # tsc --noEmit && vite build
npm run preview      # serve the built bundle locally
```

### Windows note

If PowerShell blocks the `npm.ps1` shim with an execution-policy error, call
`npm.cmd` instead of `npm`.

## Using the app

1. **Relay URL** — the WebSocket endpoint of `encchat-server`
   (default `ws://127.0.0.1:8787`).
2. **Shared mnemonic** — the 24-word phrase both people agreed on out of band.
   Click **Generate mnemonic** to create a new room; share the words over a
   channel you trust. The **room fingerprint** under the title is derived from
   the same key and can be compared verbally.
3. **Passphrase** (optional) — encrypts this device's session state locally.
   Tick **Save encrypted device state** to keep sessions across reloads.
4. **Connect**. The client joins the room, verifies each advertised device
   against the room key, excludes itself, and starts a Noise handshake with each
   peer device. Once a handshake completes you can send messages.
5. **Settings** (gear in the header, or `#/settings`) opens the live-preview
   customizer: themes and colors, typography, bubbles or IRC layout, density,
   timestamps, nicknames and local aliases, and JSON import/export. It works
   before connecting and does not disconnect an active session.

The raw mnemonic is **never** written to `localStorage`. If you enable saved
state, only the passphrase-encrypted blob and its salt are stored, and you must
re-enter the mnemonic and passphrase to restore the device.

## Layout

| Path | Purpose |
| --- | --- |
| `index.html` | Static markup: setup panel, chat panel, settings page, footer. |
| `src/main.ts` | Transport, UI wiring, nicknames, and the action loop. No crypto. |
| `src/appearance.ts` | Portable appearance types, storage, and CSS mapping. |
| `src/settings-page.ts` | The `#/settings` page and live preview. |
| `src/style.css` | Styles. |
| `scripts/build-wasm.mjs` | Cross-platform `wasm-pack` wrapper. |
| `scripts/smoke-test.mjs` | Runs a real handshake + message + state round-trip in Node. |
| `pkg/` | Generated WASM bindings (`npm run build:wasm`). |

Run the bindings smoke test (needs `pkg/` built first):

```sh
npm run test:wasm
```

## Limitations

- The relay is assumed hostile, but this web client is loaded from a server that
  could serve modified JavaScript. The WASM bundle is not verified out of band;
  use the CLI when that matters (see the footer in the UI and threat-model R1).
- No cross-device history: a device only sees messages it decrypted locally.
- A newly added peer device will not receive messages sent before its handshake
  completed.
- No custom CSS. Arbitrary CSS can exfiltrate with `url()` and spoof the UI, so
  only the structured appearance profile is supported (see `TODO.md`).
