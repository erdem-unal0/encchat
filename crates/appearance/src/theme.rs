//! Named color themes shared by the CLI and the web client.
//!
//! The ten presets below are the canonical values; both frontends must use
//! them so a theme looks the same everywhere. Every color can be overridden
//! individually, which turns the theme into a `custom` one.

use serde::{Deserialize, Serialize};

use crate::ColorSpec;

/// A named color theme. Individual colors can be overridden independently.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// Preset name, or `custom` once any color has been changed.
    pub name: String,
    pub background: ColorSpec,
    /// Normal message text.
    pub text: ColorSpec,
    pub status_bg: ColorSpec,
    pub status_fg: ColorSpec,
    pub status_accent: ColorSpec,
    pub incoming_nick: ColorSpec,
    pub outgoing_nick: ColorSpec,
    pub timestamp: ColorSpec,
    pub system: ColorSpec,
    pub error: ColorSpec,
    pub input_prompt: ColorSpec,
    pub input_text: ColorSpec,
    pub input_bg: ColorSpec,
    pub border: ColorSpec,
    /// Colors used to distinguish peers when `layout.peer_colors` is on.
    pub peer_palette: Vec<ColorSpec>,
}

impl Theme {
    pub const PRESETS: &'static [&'static str] = &[
        "default",
        "light",
        "midnight",
        "dracula",
        "gruvbox",
        "solarized",
        "nord",
        "mono",
        "matrix",
        "sunset",
    ];

    /// Look up a preset by name (case-insensitive).
    pub fn preset(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        let (colors, palette): ([&str; 14], &[&str]) = match name.as_str() {
            "default" => (
                [
                    "reset",
                    "reset",
                    "blue",
                    "white",
                    "lightcyan",
                    "green",
                    "cyan",
                    "darkgray",
                    "yellow",
                    "lightred",
                    "cyan",
                    "reset",
                    "reset",
                    "darkgray",
                ],
                &[
                    "green",
                    "cyan",
                    "magenta",
                    "yellow",
                    "lightblue",
                    "lightgreen",
                ],
            ),
            "light" => (
                [
                    "#f6f7fb", "#1f2430", "#e4e7ef", "#1f2430", "#2563eb", "#047857", "#6d28d9",
                    "#64748b", "#b45309", "#b91c1c", "#2563eb", "#1f2430", "#ffffff", "#cbd2e0",
                ],
                &[
                    "#2563eb", "#047857", "#6d28d9", "#b45309", "#be123c", "#0e7490",
                ],
            ),
            "midnight" => (
                [
                    "#0b1020", "#e2e8f0", "#1e1b4b", "#e2e8f0", "#38bdf8", "#34d399", "#a78bfa",
                    "#64748b", "#fbbf24", "#f87171", "#38bdf8", "#e2e8f0", "#111827", "#334155",
                ],
                &[
                    "#34d399", "#38bdf8", "#a78bfa", "#fbbf24", "#f472b6", "#94a3b8",
                ],
            ),
            "dracula" => (
                [
                    "#282a36", "#f8f8f2", "#44475a", "#f8f8f2", "#8be9fd", "#50fa7b", "#ff79c6",
                    "#6272a4", "#f1fa8c", "#ff5555", "#bd93f9", "#f8f8f2", "#21222c", "#6272a4",
                ],
                &[
                    "#50fa7b", "#8be9fd", "#ff79c6", "#f1fa8c", "#bd93f9", "#ffb86c",
                ],
            ),
            "gruvbox" => (
                [
                    "#282828", "#ebdbb2", "#3c3836", "#ebdbb2", "#fabd2f", "#b8bb26", "#83a598",
                    "#928374", "#d79921", "#fb4934", "#8ec07c", "#ebdbb2", "#1d2021", "#504945",
                ],
                &[
                    "#b8bb26", "#83a598", "#d3869b", "#fabd2f", "#8ec07c", "#fe8019",
                ],
            ),
            "solarized" => (
                [
                    "#002b36", "#93a1a1", "#073642", "#93a1a1", "#2aa198", "#859900", "#268bd2",
                    "#586e75", "#b58900", "#dc322f", "#6c71c4", "#93a1a1", "#00252e", "#586e75",
                ],
                &[
                    "#859900", "#268bd2", "#d33682", "#b58900", "#2aa198", "#6c71c4",
                ],
            ),
            "nord" => (
                [
                    "#2e3440", "#eceff4", "#3b4252", "#eceff4", "#88c0d0", "#a3be8c", "#81a1c1",
                    "#4c566a", "#ebcb8b", "#bf616a", "#b48ead", "#eceff4", "#272c36", "#4c566a",
                ],
                &[
                    "#a3be8c", "#88c0d0", "#b48ead", "#ebcb8b", "#81a1c1", "#8fbcbb",
                ],
            ),
            "mono" => (
                [
                    "reset", "reset", "darkgray", "white", "white", "white", "gray", "darkgray",
                    "gray", "lightred", "white", "reset", "reset", "darkgray",
                ],
                &["white", "gray", "lightgray", "darkgray"],
            ),
            "matrix" => (
                [
                    "#000000", "#00ff66", "#001a00", "#00ff66", "#7cff9b", "#00ff66", "#00cc55",
                    "#007733", "#b6ff00", "#ff3355", "#00ff99", "#00ff66", "#001100", "#005522",
                ],
                &[
                    "#00ff66", "#7cff9b", "#00cc55", "#b6ff00", "#00ff99", "#009944",
                ],
            ),
            "sunset" => (
                [
                    "#1a1020", "#ffe8d6", "#3b1f2b", "#ffe8d6", "#ffb86b", "#ff9e64", "#bb9af7",
                    "#8a6f7d", "#e0af68", "#f7768e", "#ff9e64", "#ffe8d6", "#241426", "#5a3a4a",
                ],
                &[
                    "#ff9e64", "#bb9af7", "#ffb86b", "#f7768e", "#e0af68", "#7dcfff",
                ],
            ),
            _ => return None,
        };
        Some(Self::from_parts(&name, colors, palette))
    }

    fn from_parts(name: &str, colors: [&str; 14], palette: &[&str]) -> Self {
        let [background, text, status_bg, status_fg, status_accent, incoming_nick, outgoing_nick, timestamp, system, error, input_prompt, input_text, input_bg, border] =
            colors;
        Self {
            name: name.to_string(),
            background: ColorSpec::new(background),
            text: ColorSpec::new(text),
            status_bg: ColorSpec::new(status_bg),
            status_fg: ColorSpec::new(status_fg),
            status_accent: ColorSpec::new(status_accent),
            incoming_nick: ColorSpec::new(incoming_nick),
            outgoing_nick: ColorSpec::new(outgoing_nick),
            timestamp: ColorSpec::new(timestamp),
            system: ColorSpec::new(system),
            error: ColorSpec::new(error),
            input_prompt: ColorSpec::new(input_prompt),
            input_text: ColorSpec::new(input_text),
            input_bg: ColorSpec::new(input_bg),
            border: ColorSpec::new(border),
            peer_palette: palette.iter().map(|color| ColorSpec::new(*color)).collect(),
        }
    }

    /// Repair a hand-edited theme in place.
    ///
    /// A known preset name replaces every color from that preset; `custom`
    /// validates each color; an unknown name becomes `custom` and keeps the
    /// colors. An empty peer palette falls back to the default preset's.
    pub fn sanitize(&mut self, warnings: &mut Vec<String>) {
        let requested = self.name.trim().to_ascii_lowercase();
        if requested == "custom" {
            self.name = "custom".to_string();
        } else if let Some(preset) = Self::preset(&requested) {
            *self = preset;
            return;
        } else {
            if requested.is_empty() {
                warnings.push("theme.name was empty; keeping the colors as a custom theme".into());
            } else {
                warnings.push(format!(
                    "theme.name {:?} is unknown; keeping the colors as a custom theme",
                    self.name
                ));
            }
            self.name = "custom".to_string();
        }

        for (key, color) in [
            ("background", &mut self.background),
            ("text", &mut self.text),
            ("status_bg", &mut self.status_bg),
            ("status_fg", &mut self.status_fg),
            ("status_accent", &mut self.status_accent),
            ("incoming_nick", &mut self.incoming_nick),
            ("outgoing_nick", &mut self.outgoing_nick),
            ("timestamp", &mut self.timestamp),
            ("system", &mut self.system),
            ("error", &mut self.error),
            ("input_prompt", &mut self.input_prompt),
            ("input_text", &mut self.input_text),
            ("input_bg", &mut self.input_bg),
            ("border", &mut self.border),
        ] {
            sanitize_color(color, key, warnings);
        }
        sanitize_peer_palette(&mut self.peer_palette, warnings);
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::preset("default").expect("the default theme is built in")
    }
}

fn sanitize_color(color: &mut ColorSpec, key: &str, warnings: &mut Vec<String>) {
    if !color.is_valid() {
        warnings.push(format!("theme.{key} was not a valid color; using reset"));
        *color = ColorSpec::default();
        return;
    }
    let normalized = color.as_str().trim().to_ascii_lowercase();
    if normalized != color.as_str() {
        *color = ColorSpec::new(normalized);
    }
}

fn sanitize_peer_palette(palette: &mut Vec<ColorSpec>, warnings: &mut Vec<String>) {
    let before = palette.len();
    for color in palette.iter_mut() {
        if color.is_valid() {
            let normalized = color.as_str().trim().to_ascii_lowercase();
            if normalized != color.as_str() {
                *color = ColorSpec::new(normalized);
            }
        }
    }
    palette.retain(ColorSpec::is_valid);
    if palette.len() != before {
        warnings.push("theme.peer_palette contained invalid colors; removed them".into());
    }
    if palette.is_empty() {
        warnings.push("theme.peer_palette was empty; using the default palette".into());
        *palette = Theme::default().peer_palette;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Theme {
        fn colors(&self) -> Vec<(&'static str, &ColorSpec)> {
            vec![
                ("background", &self.background),
                ("text", &self.text),
                ("status_bg", &self.status_bg),
                ("status_fg", &self.status_fg),
                ("status_accent", &self.status_accent),
                ("incoming_nick", &self.incoming_nick),
                ("outgoing_nick", &self.outgoing_nick),
                ("timestamp", &self.timestamp),
                ("system", &self.system),
                ("error", &self.error),
                ("input_prompt", &self.input_prompt),
                ("input_text", &self.input_text),
                ("input_bg", &self.input_bg),
                ("border", &self.border),
            ]
        }
    }

    #[test]
    fn every_preset_resolves_with_valid_colors() {
        assert_eq!(Theme::PRESETS.len(), 10);
        for name in Theme::PRESETS {
            let theme = Theme::preset(name).unwrap_or_else(|| panic!("{name} should resolve"));
            assert_eq!(&theme.name, name);
            for (key, color) in theme.colors() {
                assert!(color.is_valid(), "{name}.{key} = {color} should be valid");
            }
            assert!(!theme.peer_palette.is_empty(), "{name} needs a palette");
            for color in &theme.peer_palette {
                assert!(color.is_valid(), "{name} palette {color} should be valid");
            }
        }
    }

    #[test]
    fn preset_names_are_case_insensitive() {
        for name in Theme::PRESETS {
            let upper = name.to_ascii_uppercase();
            assert_eq!(
                Theme::preset(&upper).map(|theme| theme.name),
                Some(name.to_string())
            );
            assert_eq!(
                Theme::preset(&format!("  {name}  ")).map(|theme| theme.name),
                Some(name.to_string())
            );
        }
        assert_eq!(Theme::preset("neon"), None);
        assert_eq!(Theme::preset(""), None);
    }

    #[test]
    fn presets_round_trip_through_json() {
        for name in Theme::PRESETS {
            let theme = Theme::preset(name).unwrap();
            let json = serde_json::to_string(&theme).unwrap();
            let back: Theme = serde_json::from_str(&json).unwrap();
            assert_eq!(back, theme, "{name} should survive JSON");
            // Exact text, not just "parses to the same color".
            assert_eq!(
                back.colors()
                    .iter()
                    .map(|(_, color)| color.as_str())
                    .collect::<Vec<_>>(),
                theme
                    .colors()
                    .iter()
                    .map(|(_, color)| color.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn sanitizing_a_preset_is_a_no_op() {
        for name in Theme::PRESETS {
            let mut theme = Theme::preset(name).unwrap();
            let mut warnings = Vec::new();
            theme.sanitize(&mut warnings);
            assert!(warnings.is_empty(), "{name} warned: {warnings:?}");
            assert_eq!(theme, Theme::preset(name).unwrap());
        }
    }

    #[test]
    fn preset_name_replaces_edited_colors() {
        let mut theme = Theme::preset("dracula").unwrap();
        theme.background = ColorSpec::new("#000000");
        let mut warnings = Vec::new();
        theme.sanitize(&mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(theme.background.as_str(), "#282a36");
        assert_eq!(theme.name, "dracula");
    }

    #[test]
    fn invalid_custom_color_becomes_reset() {
        let mut theme = Theme {
            name: "custom".into(),
            text: ColorSpec::new("not-a-color"),
            ..Theme::default()
        };
        let mut warnings = Vec::new();
        theme.sanitize(&mut warnings);
        assert_eq!(theme.text.as_str(), "reset");
        assert_eq!(theme.name, "custom");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("theme.text"), "{warnings:?}");
    }

    #[test]
    fn unknown_preset_keeps_colors_and_marks_custom() {
        let mut theme = Theme {
            name: "neon".into(),
            background: ColorSpec::new("#123456"),
            ..Theme::default()
        };
        let mut warnings = Vec::new();
        theme.sanitize(&mut warnings);
        assert_eq!(theme.name, "custom");
        assert_eq!(theme.background.as_str(), "#123456");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("neon"), "{warnings:?}");
    }

    #[test]
    fn colors_are_normalized_when_custom() {
        let mut theme = Theme {
            name: "custom".into(),
            background: ColorSpec::new("  #AABBCC "),
            ..Theme::default()
        };
        let mut warnings = Vec::new();
        theme.sanitize(&mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(theme.background.as_str(), "#aabbcc");
    }

    #[test]
    fn peer_palette_is_short_kept_and_empty_refilled() {
        let mut theme = Theme {
            name: "custom".into(),
            peer_palette: vec![ColorSpec::new("#ff0000")],
            ..Theme::default()
        };

        let mut warnings = Vec::new();
        theme.sanitize(&mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(theme.peer_palette, vec![ColorSpec::new("#ff0000")]);

        theme.peer_palette = Vec::new();
        let mut warnings = Vec::new();
        theme.sanitize(&mut warnings);
        assert_eq!(theme.peer_palette, Theme::default().peer_palette);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("peer_palette"), "{warnings:?}");
    }

    #[test]
    fn peer_palette_drops_invalid_entries() {
        let mut theme = Theme {
            name: "custom".into(),
            peer_palette: vec![
                ColorSpec::new("#ff0000"),
                ColorSpec::new("bogus"),
                ColorSpec::new("17"),
            ],
            ..Theme::default()
        };
        let mut warnings = Vec::new();
        theme.sanitize(&mut warnings);
        assert_eq!(
            theme.peer_palette,
            vec![ColorSpec::new("#ff0000"), ColorSpec::new("17")]
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("invalid"), "{warnings:?}");
    }
}
