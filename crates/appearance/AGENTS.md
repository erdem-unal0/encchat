# AGENTS.md — crates/appearance

Portable appearance settings: the one place that defines themes, colors,
typography, timestamps, layout and labels for **both** frontends. The CLI and
the web client read and write the same JSON, so a theme looks the same in both.

## Rules

- **No I/O, no clock.** No `std::fs`, no sockets, no `SystemTime`. Parsing and
  serializing are the caller's job (`Appearance::from_json` / `to_json_pretty`
  work on strings).
- **No frontend dependencies.** No `ratatui`, no `crossterm`, no
  `wasm-bindgen`. Dependencies are `serde` and `serde_json` only.
- **The presets are canonical.** `Theme::preset` must keep byte-identical color
  values with `crates/cli`'s theme table until that table is deleted. Do not
  "tidy" a hex value.
- **The relay never sees this.** Appearance is local user preference, not
  protocol; nothing here goes on the wire.
- **Sanitizing defaults is silent.** `Appearance::default().sanitize()` must
  return no warnings; add a test when changing defaults.
- **Never panic on hostile JSON.** Repair repairable values with a warning;
  return `Err` from `from_json` only for the wrong JSON shape.
- Bump `VERSION` on an incompatible JSON change and keep unknown fields
  ignored so newer files still load.

## Map

| File | Responsibility |
| --- | --- |
| `lib.rs` | `Appearance`, typography/timestamps/layout/labels, sanitize rules. |
| `color.rs` | `ColorSpec`: accepted text forms, validation, `to_css`. |
| `theme.rs` | The ten presets and per-color repair. |
| `timestamp.rs` | The token language (`HH:mm:ss`) and its strftime mapping. |

## Testing

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p encchat-appearance
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy -p encchat-appearance --all-targets -- -D warnings
```
