# Customizing the web client

The conversation fills the browser window; connection details live in a drawer
that slides out from the header **Connect** button (it opens automatically on
first load). The browser client also has a dedicated settings page with a live
chat preview. Open it with the **Settings** button in the header or by visiting
`#/settings` directly. Settings are local to the browser, work before you
connect, and opening the page does not close an active session.

Changes apply to the preview and the running UI immediately. **Save** writes them
to `localStorage`; **Revert** restores the last saved profile. The same page can
export the profile as JSON and import one from the CLI or another browser.

## What can be changed

### Theme and colors

Ten presets ship in `encchat-appearance` and are shared with the CLI:

`default`, `light`, `midnight`, `dracula`, `gruvbox`, `solarized`, `nord`,
`mono`, `matrix`, `sunset`.

Selecting a preset replaces every color and resets `theme.name`. Any individual
color can then be overridden with a color picker or a text value; that switches
the theme to `custom`. In a hand-written JSON file, a known preset name always
wins over the colors beside it: use `theme.name: "custom"` with the full color
set to preserve overrides (the UI exports this shape automatically). `reset`
(and ANSI names such as `cyan` or `17`) means "use the client's default for this
slot", so the same profile can look reasonable in a terminal and a browser.

The peer palette gives each device a stable nickname color when **Peer colors**
is enabled. Colors are only presentation: they are never sent to the relay or a
peer.

### Typography

- Family: system, sans, serif, or monospace.
- Base size: 12–22 px.
- Line height: 1.0–2.0.

The terminal client ignores these fields; they are part of the shared document
so a profile can travel between clients without losing information.

### Layout and density

- **Message style:** bubbles or IRC-style lines.
- **Density:** compact, comfortable, or spacious.
- **Maximum width:** 480–1600 px.
- **Status bar:** the strip above the conversation with connection, room,
  peers, sessions and clock.
- **Hints:** the keyboard/send hint row.
- **Input border:** the border around the composer.
- **Auto-scroll:** jump to new messages even while you are reading older ones.
- **Peer colors:** use the theme palette for nicknames.
- **Unread:** show the unread counter when you are scrolled away from the bottom.

### Timestamps

Timestamps can be shown on chat messages and, separately, on system notices.
The format uses small tokens rather than `strftime` so both clients can render
it:

| Token | Meaning |
| --- | --- |
| `YYYY` | Four-digit year |
| `MM` | Two-digit month |
| `DD` | Two-digit day |
| `HH` | Two-digit hour (24-hour clock) |
| `mm` | Two-digit minute |
| `ss` | Two-digit second |

Examples: `HH:mm`, `HH:mm:ss`, `YYYY-MM-DD HH:mm`, `DD/MM HH:mm`. Common formats
are offered as presets; a custom format is validated inline.

### Labels, nicknames and local aliases

- **Display name** is your advertised nickname. It travels inside the Double
  Ratchet as the existing `PayloadKind::Nick` payload, so the relay cannot read
  it and cannot forge one on your behalf. Leave it blank to use the fallback.
- **Own fallback** is shown for your own messages before a display name is set.
- **Show peer nicknames** controls whether a peer's advertised name is used.
- **Show device IDs** controls the short hex fallback when no name is known.
- **Local aliases** map a peer device ID to a name that only you see. Aliases
  override advertised nicknames locally and are never transmitted or exported.

A nickname is a cosmetic label, not an identity. Two peers can choose the same
name. Use a local alias when you need a stable name for a specific device.

## Import and export

The settings page can download `encchat-appearance.json`, copy it to the
clipboard, import a file, or paste JSON. The CLI uses the same document:

```sh
encchat config export-appearance ./appearance.json
encchat config import-appearance ./appearance.json
```

The file contains only appearance data. It never includes the mnemonic, a
password, the relay URL, the encrypted state, message history, key bindings, the
CLI prompt template, or nicknames. Imported JSON is validated and repaired, and
any warnings are shown before the profile is applied.

CLI-only fields (key bindings, prompt template, status-bar segments and order,
scroll steps, mouse capture) stay in the CLI's `config.json`; web-only fields
(typography, message style, density, maximum width) are ignored by the CLI with
a notice.

## Where settings live

| Key | Contents |
| --- | --- |
| `encchat.settings.v1` | Portable appearance profile |
| `encchat.labels.v1` | Local peer aliases (device ID to name) |
| `encchat.nick.v1` | Your display name, so it survives a reload |
| `encchat.state.v1` | Passphrase-encrypted device state |
| `encchat.serverUrl` | Last relay URL |

Appearance, aliases and the display name are not secret and are stored in plain
text. The mnemonic is never stored; only the passphrase-encrypted state blob and
its salt are.

## Deliberate limits

- **No custom CSS.** Arbitrary CSS can load remote URLs with `url(...)`,
  exfiltrate data, and visually spoof the UI — especially dangerous when a
  profile is imported from someone else. A sanitized, opt-in subset is tracked in
  `TODO.md`; it is not part of this release.
- **No per-room profiles yet.** One appearance profile applies to the browser.
- **No server sync.** Settings do not travel through the relay. Use
  export/import to move them.
- **The host can still serve modified JavaScript.** Customization does not
  change the caveat shown once when a room starts: a malicious host bypasses
  every client-side control. Use the CLI when that matters.
