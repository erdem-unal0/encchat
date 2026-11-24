# Customizing the clients

The terminal client is configurable at runtime, from a JSON file, from the
command line, and from an interactive full-screen customizer with a live chat
preview. Nothing here is required to use `encchat`; the defaults match the
original IRC-style look.

Themes, colors, timestamps and the portable appearance profile are shared with
the web client, which has its own settings page with a live preview. See
[web-customization.md](web-customization.md) for the browser side.

## Three ways to change settings

| How | Best for |
| --- | --- |
| `encchat settings` (or F2 / `/settings` inside a chat) | Exploring options with a live preview. |
| `encchat config set <key> <value>` | Scripts, dotfiles, and quick one-off changes. |
| Editing `config.json` by hand | Versioning your configuration or copying it between machines. |

The customizer is also reachable from the command palette: run `encchat
settings` to open it without a chat session, press F2 inside a chat, or type
`/settings` (optionally with a key, e.g. `/settings theme.preset`).

In the customizer:

| Key | Action |
| --- | --- |
| `Up` / `Down`, `PgUp` / `PgDn`, `Home` / `End` | Move between settings. |
| `Tab` / `Shift+Tab` | Next / previous section. |
| `Left` / `Right` | Change a toggle, choice, number, preset, or color. |
| `Shift+Left` / `Shift+Right` | Change a number by 10. |
| `Enter` | Edit a text value, or toggle/advance the selected setting. |
| `Space` | Toggle a boolean; otherwise same as `Enter`. |
| `s` | Save the draft to disk. |
| `r` / `R` | Reset the selected setting / every setting. |
| `?` | Show the editor key help. |
| `Esc` | Close. If there are unsaved changes, press it twice to discard. |
| `Ctrl+U` | Clear the inline editor while editing. |

While editing text, `Enter` validates and commits, `Esc` cancels, and pasted
newlines are flattened to spaces. Invalid values stay in the editor with an
error instead of being saved. The preview renders from the draft immediately;
only `s` (or `config set`) writes to disk.

## Where settings live

The default path is the OS config directory:

| OS | Path |
| --- | --- |
| Linux | `~/.config/encchat/config.json` |
| macOS | `~/Library/Application Support/encchat/config.json` |
| Windows | `%APPDATA%\encchat\config.json` |

Override it with the global `--config PATH` flag or the `ENCCHAT_CONFIG`
environment variable:

```sh
encchat --config ./my-encchat.json settings
ENCCHAT_CONFIG=./my-encchat.json encchat chat --mnemonic "…"
```

`encchat config path` prints the effective path.

A missing file is not an error: the built-in defaults are used and nothing is
written until you save. Files are written atomically (temporary file + rename).
Invalid JSON falls back to defaults with a warning. Individually repairable
values (colors, numbers, timestamp formats, key bindings, duplicate status
segments) are repaired in memory at load time and reported; the repaired values
are only written when you save. A file with an unknown status segment, status
position, alignment, or wrong JSON type cannot be parsed at all, so the whole
file falls back to defaults with a warning rather than starting with a partly
applied configuration.

## Command-line config commands

```sh
encchat config path                  # print the settings file path
encchat config keys                  # list every key, value, and description
encchat config show                  # print the effective config as JSON
encchat config get theme.preset      # print one value
encchat config set theme.preset nord # validate, set, save, and echo the result
encchat config themes                # list built-in themes
encchat config reset                 # write all defaults
```

`config set` validates before writing, so a bad value leaves the file untouched:

```text
$ encchat config set layout.scroll_step 0
Error: layout.scroll_step must be between 1 and 100
```

### Portable appearance profile

`config export-appearance` writes the subset both frontends understand (theme,
timestamps, status-bar visibility, hints, input border, auto-scroll, peer
colors, and the fallback label). With no path it prints to stdout:

```sh
encchat config export-appearance ./appearance.json
encchat config import-appearance ./appearance.json
```

The profile never contains key bindings, the prompt template, mnemonics,
passwords, the server URL, the state path or your nickname. Those stay in
`config.json` on the CLI, and in `localStorage` in the browser. Exporting from
the CLI sets the message style to `irc` and leaves web-only typography at its
defaults; importing a web profile applies the fields the terminal can render
and prints one notice about the rest.

The schema lives in `crates/appearance` and is versioned (`version: 1`,
`kind: "encchat-appearance"`). Unknown fields are ignored, repairable values are
clamped with warnings, and a newer version loads with a warning instead of
failing.

## Themes

Ten presets ship in the binary:

`default`, `light`, `midnight`, `dracula`, `gruvbox`, `solarized`, `nord`,
`mono`, `matrix`, `sunset`.

Switch persistently:

```sh
encchat config set theme.preset dracula
```

Preview a preset for one session without saving it:

```sh
encchat chat … --theme matrix
```

Or cycle it inside a chat with F4 / `/theme`. `/theme <name>` switches for the
current session only; save it with `/settings`.

Every color is independently overridable. A color accepts a ratatui color name
(`cyan`, `darkgray`, `lightblue`, …), an ANSI index (`0`–`255`), or hex
(`#89b4fa` or `#8bf`):

```sh
encchat config set theme.incoming_nick '#8be9fd'
encchat config set theme.status_bg 17
encchat config set theme.input_bg '#111827'
```

Setting any individual color changes `theme.name` to `custom`; choosing a preset
later replaces all of them. The `theme.peer_palette` is a comma-separated list
used to give each peer device a stable nickname color when
`behavior.peer_colors` is on:

```sh
encchat config set theme.peer_palette '#50fa7b,#8be9fd,#ff79c6,#f1fa8c'
```

### Color keys

| Key | Used for |
| --- | --- |
| `theme.background` | Whole-client background. |
| `theme.text` | Normal message text. |
| `theme.status_bg`, `theme.status_fg` | Status bar background and text. |
| `theme.status_accent` | App name, connection state, highlights. |
| `theme.incoming_nick`, `theme.outgoing_nick` | Nicknames. |
| `theme.timestamp` | Timestamps and dim hint text. |
| `theme.system` | Notices and system output. |
| `theme.error` | Errors and the offline indicator. |
| `theme.input_prompt`, `theme.input_text`, `theme.input_bg` | Input line. |
| `theme.border` | Input box and settings pane borders. |
| `theme.peer_palette` | Per-peer nickname colors (comma-separated). |

## Layout

```sh
encchat config set layout.status_position bottom      # top | bottom | hidden
encchat config set layout.status_align center         # left | center | right
encchat config set layout.status_separator ' · '      # up to 32 characters
encchat config set layout.status_segments 'app,server,room,peers,connection,clock'
encchat config set layout.clock_format '%H:%M'
encchat config set layout.input_border false
encchat config set layout.show_hints false
encchat config set layout.show_scroll_indicator true
encchat config set layout.show_unread true
encchat config set layout.scroll_step 3               # mouse wheel, 1–100
encchat config set layout.page_step 10                # PgUp/PgDn, 1–100
encchat config set layout.max_scrollback 5000         # 50–100000 lines
```

Status segments can appear in any order, at most once each:

| Segment | Shows |
| --- | --- |
| `app` | `encchat`. |
| `connection` | `● online` or `○ offline`. |
| `server` | Relay URL. |
| `room` | Room fingerprint. |
| `peers` | Number of peer devices. |
| `nick` | Your nickname, when set. |
| `device` | Your short device id. |
| `sessions` | Number of established ratchet sessions. |
| `clock` | The current time in `layout.clock_format`. |

When the viewport is scrolled up, incoming messages increment an unread counter
instead of yanking you to the bottom. `behavior.auto_scroll` changes that.
`layout.show_unread` and `layout.show_scroll_indicator` control whether the hint
line reports them.

## Timestamps

```sh
encchat config set timestamps.enabled true
encchat config set timestamps.format '%d %b %H:%M'
encchat config set timestamps.utc false
encchat config set timestamps.system true
```

`timestamps.format` is a `strftime` format string (`%H:%M:%S`, `%d %b %H:%M`,
`%Y-%m-%d %H:%M`, …) and is validated when set. `timestamps.system` controls
whether notices get timestamps in addition to chat messages. F3 or
`/timestamps` toggles timestamps for the current session; save the change with
`/settings`.

## Behavior

```sh
encchat config set behavior.mouse true
encchat config set behavior.auto_scroll false
encchat config set behavior.show_help_on_start true
encchat config set behavior.peer_colors true
encchat config set behavior.input_history_size 500
encchat config set behavior.own_nick_fallback me
encchat config set behavior.system_prefix '• '
```

- `mouse`: capture the wheel for scrolling. Turn it off if you would rather use
  the terminal's native text selection and scrolling.
- `auto_scroll`: jump to new messages even while you are reading older ones. Off
  by default, so reading back does not get interrupted.
- `peer_colors`: pick nickname colors from `theme.peer_palette` per device.
- `input_history_size`: submitted lines kept for Up/Down recall (10–10000).
- `own_nick_fallback`: name shown before `/nick` is set and as the author of
  your own messages.
- `system_prefix`: prefix for notices (up to 16 characters); set it to an empty
  string for no prefix.

## Prompt

The input prompt is a template with three placeholders:

```sh
encchat config set prompt.template '{nick}@{room} › '
```

- `{nick}` — your nickname, or `behavior.own_nick_fallback`.
- `{room}` — the room fingerprint.
- `{device}` — your short device id.

The template is limited to 64 characters and cannot be empty.

## Key bindings

Bindings use `ctrl+`, `alt+`, and `shift+` modifiers plus a key name. Plain
printable keys are rejected (they would block typing), and two actions cannot
share a binding.

```sh
encchat config set keys.quit ctrl+q
encchat config set keys.settings f2
encchat config set keys.complete tab
```

Accepted key names include `a`–`z`, `f1`–`f24`, `enter`, `esc`, `tab`,
`backtab`, `space`, `backspace`, `delete`, `insert`, `home`, `end`, `left`,
`right`, `up`, `down`, `pageup`/`pgdn`, and punctuation names such as `comma`,
`period`, `slash`, `backslash`, `semicolon`, `quote`, `backtick`, `minus`, and
`plus`.

| Action | Default | Does |
| --- | --- | --- |
| `quit` | `ctrl+c` | Save state and exit. |
| `settings` | `f2` | Open the customizer. |
| `help` | `f1` | Print the slash-command list. |
| `scroll_up` | `pageup` | Scroll up `layout.page_step` lines. |
| `scroll_down` | `pagedown` | Scroll down `layout.page_step` lines. |
| `complete` | `tab` | Complete slash-command names. |
| `clear_input` | `ctrl+u` | Clear the input line. |
| `delete_word` | `ctrl+w` | Delete the previous word. |
| `history_prev` | `up` | Recall an older submitted line. |
| `history_next` | `down` | Move forward through history. |
| `toggle_timestamps` | `f3` | Toggle timestamps for this session. |
| `cycle_theme` | `f4` | Cycle themes for this session. |

Notes:

- Arrow keys, Home/End, Backspace/Delete, and Enter still edit the input line
  even if you bind other actions to those keys; the bindings are checked first.
- `/keys` prints the active bindings. The customizer shows them under the
  **Keys** tab.
- In a standalone `encchat settings` run, `Ctrl+C` behaves like `Esc`, including
  the unsaved-changes confirmation. Inside a chat, `Ctrl+C` always exits.

## The settings file

`encchat config show` prints the complete effective config. A representative
file looks like this (comments are not valid JSON; they are only here to explain
the sections):

```json
{
  "version": 1,
  "theme": {
    "name": "dracula",
    "background": "#282a36",
    "text": "#f8f8f2",
    "status_bg": "#44475a",
    "status_fg": "#f8f8f2",
    "status_accent": "#8be9fd",
    "incoming_nick": "#50fa7b",
    "outgoing_nick": "#ff79c6",
    "timestamp": "#6272a4",
    "system": "#f1fa8c",
    "error": "#ff5555",
    "input_prompt": "#bd93f9",
    "input_text": "#f8f8f2",
    "input_bg": "#21222c",
    "border": "#6272a4",
    "peer_palette": ["#50fa7b", "#8be9fd", "#ff79c6", "#f1fa8c", "#bd93f9", "#ffb86c"]
  },
  "layout": {
    "status_position": "top",
    "status_separator": " | ",
    "status_align": "left",
    "status_segments": ["app", "server", "room", "peers", "connection", "clock"],
    "clock_format": "%H:%M",
    "show_hints": true,
    "show_scroll_indicator": true,
    "show_unread": true,
    "input_border": true,
    "scroll_step": 3,
    "page_step": 10,
    "max_scrollback": 5000
  },
  "timestamps": {
    "enabled": true,
    "format": "%H:%M:%S",
    "utc": false,
    "system": true
  },
  "behavior": {
    "mouse": true,
    "auto_scroll": false,
    "show_help_on_start": true,
    "peer_colors": true,
    "input_history_size": 500,
    "own_nick_fallback": "me",
    "system_prefix": "• "
  },
  "prompt": {
    "template": "{nick} "
  },
  "keys": {
    "quit": "ctrl+c",
    "settings": "f2",
    "help": "f1",
    "scroll_up": "pageup",
    "scroll_down": "pagedown",
    "complete": "tab",
    "clear_input": "ctrl+u",
    "delete_word": "ctrl+w",
    "history_prev": "up",
    "history_next": "down",
    "toggle_timestamps": "f3",
    "cycle_theme": "f4"
  }
}
```

Every field is optional. Missing fields use the built-in defaults, unknown
fields are ignored, and an older file without newer fields still loads. A
future `version` only produces a warning; it does not prevent startup.

## Environment variables

| Variable | Equivalent |
| --- | --- |
| `ENCCHAT_CONFIG` | `--config PATH` |
| `ENCCHAT_SERVER` | `chat --server URL` |
| `ENCCHAT_MNEMONIC` | `chat --mnemonic "…"` |
| `ENCCHAT_STATE` | `chat --state PATH` |
| `ENCCHAT_NICK` | `chat --nick NAME` |

Command-line flags win over environment variables.

## Recipes

A compact dark setup with timestamps that include the date:

```sh
encchat config set theme.preset nord
encchat config set timestamps.format '%d %b %H:%M'
encchat config set layout.input_border false
encchat config set layout.status_position bottom
encchat config set layout.status_segments 'connection,room,peers,clock'
```

A minimal, terminal-native look:

```sh
encchat config set theme.preset mono
encchat config set theme.background reset
encchat config set layout.status_position hidden
encchat config set layout.input_border false
encchat config set layout.show_hints false
```

A Matrix-style one-session preview:

```sh
encchat chat --server ws://127.0.0.1:8787 --mnemonic "<24 words>" --theme matrix
```
