//! Portable appearance settings shared by the encchat CLI and web client.
//!
//! This crate is the single source of truth for themes, typography, timestamps,
//! layout and labels. It has no I/O, no clock, and no frontend dependencies (no
//! `ratatui`, no `wasm-bindgen`), so the terminal client and the browser read
//! and write exactly the same JSON. Colors stay as text; the CLI parses them
//! and the web client asks for CSS with [`ColorSpec::to_css`].
//!
//! ```
//! use encchat_appearance::Appearance;
//!
//! let (appearance, warnings) = Appearance::from_json(r#"{"theme":{"name":"nord"}}"#).unwrap();
//! assert!(warnings.is_empty());
//! assert_eq!(appearance.theme.background.as_str(), "#2e3440");
//! ```

mod color;
mod theme;
mod timestamp;

pub use color::ColorSpec;
pub use theme::Theme;
pub use timestamp::{
    timestamp_to_strftime, validate_timestamp_format, DEFAULT_TIMESTAMP_FORMAT,
    MAX_TIMESTAMP_FORMAT_LEN,
};

use serde::{Deserialize, Serialize};

/// Bumped whenever the JSON shape changes incompatibly.
pub const VERSION: u32 = 1;

/// The `kind` marker written into every appearance file.
pub const KIND: &str = "encchat-appearance";

/// How messages are laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStyle {
    #[default]
    Bubbles,
    Irc,
}

impl MessageStyle {
    pub const ALL: &'static [&'static str] = &["bubbles", "irc"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bubbles => "bubbles",
            Self::Irc => "irc",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "bubbles" => Some(Self::Bubbles),
            "irc" => Some(Self::Irc),
            _ => None,
        }
    }
}

/// Vertical spacing of the message list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

impl Density {
    pub const ALL: &'static [&'static str] = &["compact", "comfortable", "spacious"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Comfortable => "comfortable",
            Self::Spacious => "spacious",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" => Some(Self::Compact),
            "comfortable" => Some(Self::Comfortable),
            "spacious" => Some(Self::Spacious),
            _ => None,
        }
    }
}

/// Font stack used by the web client. The CLI ignores it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontFamily {
    #[default]
    System,
    Sans,
    Serif,
    Mono,
}

impl FontFamily {
    pub const ALL: &'static [&'static str] = &["system", "sans", "serif", "mono"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Sans => "sans",
            Self::Serif => "serif",
            Self::Mono => "mono",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "sans" => Some(Self::Sans),
            "serif" => Some(Self::Serif),
            "mono" => Some(Self::Mono),
            _ => None,
        }
    }
}

/// Text sizing for the web client. The CLI renders with the terminal's font.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Typography {
    pub family: FontFamily,
    /// Base font size in pixels, clamped to `12..=22`.
    pub size: u8,
    /// Line height multiple, clamped to `1.0..=2.0`.
    pub line_height: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            family: FontFamily::System,
            size: 15,
            line_height: 1.45,
        }
    }
}

/// Timestamp display shared by both frontends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timestamps {
    pub enabled: bool,
    /// A [`validate_timestamp_format`]-style format, e.g. `"HH:mm:ss"`.
    pub format: String,
    /// Also show timestamps on system/notice lines.
    pub system: bool,
}

impl Default for Timestamps {
    fn default() -> Self {
        Self {
            enabled: true,
            format: DEFAULT_TIMESTAMP_FORMAT.to_string(),
            system: true,
        }
    }
}

/// Layout switches that both frontends can honour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub message_style: MessageStyle,
    pub density: Density,
    /// Maximum content width in pixels, clamped to `480..=1600`.
    pub max_width: u16,
    pub show_status_bar: bool,
    pub show_hints: bool,
    pub input_border: bool,
    pub auto_scroll: bool,
    pub peer_colors: bool,
    pub show_unread: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            message_style: MessageStyle::default(),
            density: Density::default(),
            max_width: 1100,
            show_status_bar: true,
            show_hints: true,
            input_border: true,
            auto_scroll: false,
            peer_colors: true,
            show_unread: true,
        }
    }
}

/// Names shown next to messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Labels {
    /// Shown before a nickname is set: trimmed, whitespace-collapsed, max 32.
    pub own_fallback: String,
    pub show_peer_nicks: bool,
    pub show_device_ids: bool,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            own_fallback: "me".to_string(),
            show_peer_nicks: true,
            show_device_ids: true,
        }
    }
}

/// The complete portable appearance document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub version: u32,
    pub kind: String,
    pub theme: Theme,
    pub typography: Typography,
    pub timestamps: Timestamps,
    pub layout: Layout,
    pub labels: Labels,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            version: VERSION,
            kind: KIND.to_string(),
            theme: Theme::default(),
            typography: Typography::default(),
            timestamps: Timestamps::default(),
            layout: Layout::default(),
            labels: Labels::default(),
        }
    }
}

impl Appearance {
    /// Repair anything a hand-edited or newer file could contain.
    ///
    /// Always mutates in place and returns human-readable warnings; it never
    /// panics, and sanitizing already-clean settings returns no warnings.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.version > VERSION {
            warnings.push(format!(
                "appearance version {} is newer than this build ({}); unknown fields were ignored",
                self.version, VERSION
            ));
        }
        self.version = VERSION;

        if self.kind.trim() != KIND {
            warnings.push(format!(
                "kind {:?} is not {KIND:?}; resetting it",
                self.kind
            ));
            self.kind = KIND.to_string();
        }

        self.theme.sanitize(&mut warnings);

        if !(12..=22).contains(&self.typography.size) {
            warnings.push(format!(
                "typography.size {} was out of range; clamped to 12-22",
                self.typography.size
            ));
            self.typography.size = self.typography.size.clamp(12, 22);
        }

        let line_height = self.typography.line_height;
        if !line_height.is_finite() {
            warnings.push("typography.line_height was not a number; using 1.45".into());
            self.typography.line_height = 1.45;
        } else {
            let clamped = line_height.clamp(1.0, 2.0);
            if clamped != line_height {
                warnings.push(format!(
                    "typography.line_height {line_height} was out of range; clamped to 1.0-2.0"
                ));
            }
            self.typography.line_height = (clamped * 100.0).round() / 100.0;
        }

        if !(480..=1600).contains(&self.layout.max_width) {
            warnings.push(format!(
                "layout.max_width {} was out of range; clamped to 480-1600",
                self.layout.max_width
            ));
            self.layout.max_width = self.layout.max_width.clamp(480, 1600);
        }

        let own_fallback = normalize_label(&self.labels.own_fallback);
        if own_fallback.is_empty() {
            warnings.push("labels.own_fallback was empty; using \"me\"".into());
            self.labels.own_fallback = Labels::default().own_fallback;
        } else if own_fallback.chars().count() > 32 {
            warnings.push("labels.own_fallback was too long; truncated to 32 characters".into());
            self.labels.own_fallback = own_fallback.chars().take(32).collect();
        } else if own_fallback != self.labels.own_fallback {
            self.labels.own_fallback = own_fallback;
        }

        if !validate_timestamp_format(&self.timestamps.format) {
            warnings.push(format!(
                "timestamps.format {:?} was invalid; using {DEFAULT_TIMESTAMP_FORMAT:?}",
                self.timestamps.format
            ));
            self.timestamps.format = DEFAULT_TIMESTAMP_FORMAT.to_string();
        }

        warnings
    }

    /// Parse a document, then [`sanitize`](Self::sanitize) it.
    ///
    /// Returns `Err` only when the text is not the right JSON shape (malformed
    /// JSON, wrong types, or an unknown enum value); repairable values come
    /// back as warnings instead. A UTF-8 BOM is ignored.
    pub fn from_json(text: &str) -> Result<(Self, Vec<String>), String> {
        let document = text.trim_start_matches('\u{feff}').trim_start();
        if !document.starts_with('{') {
            return Err("appearance JSON must be an object".to_string());
        }
        let mut appearance: Self = serde_json::from_str(document)
            .map_err(|error| format!("could not parse appearance JSON: {error}"))?;
        let warnings = appearance.sanitize();
        Ok((appearance, warnings))
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Every built-in theme, in [`Theme::PRESETS`] order.
    pub fn presets() -> Vec<Theme> {
        Theme::PRESETS
            .iter()
            .filter_map(|name| Theme::preset(name))
            .collect()
    }
}

/// Trim, drop control characters, and collapse internal whitespace runs.
fn normalize_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for ch in value.chars() {
        if ch.is_control() || ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_clean_and_round_trips() {
        let appearance = Appearance::default();
        assert_eq!(appearance.version, VERSION);
        assert_eq!(appearance.kind, KIND);

        let mut clean = appearance.clone();
        let warnings = clean.sanitize();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(clean, appearance);

        let json = appearance.to_json_pretty();
        let (back, warnings) = Appearance::from_json(&json).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back, appearance);
    }

    #[test]
    fn sanitizing_twice_is_stable() {
        let text = r#"{
            "version": 7,
            "kind": "other",
            "theme": {"name": "neon", "background": "junk"},
            "typography": {"size": 2, "line_height": 9.0},
            "layout": {"max_width": 10},
            "labels": {"own_fallback": "  "},
            "timestamps": {"format": "nope"}
        }"#;
        let (mut appearance, warnings) = Appearance::from_json(text).unwrap();
        assert!(!warnings.is_empty());
        let second = appearance.sanitize();
        assert!(second.is_empty(), "{second:?}");
        assert_eq!(appearance.version, VERSION);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        for text in ["{}", r#"{"theme":{}}"#, r#"{"layout":{},"labels":{}}"#] {
            let (appearance, warnings) = Appearance::from_json(text).unwrap();
            assert_eq!(appearance, Appearance::default(), "input {text}");
            assert!(warnings.is_empty(), "input {text}: {warnings:?}");
        }
        let (partial, _) = Appearance::from_json(r#"{"layout":{"max_width":900}}"#).unwrap();
        assert_eq!(partial.layout.max_width, 900);
        assert_eq!(partial.typography, Typography::default());
        assert_eq!(partial.theme, Theme::default());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let text = r#"{
            "version": 1,
            "kind": "encchat-appearance",
            "future": {"nested": true},
            "theme": {"name": "default", "glow": 3},
            "layout": {"max_width": 900, "sparkles": true}
        }"#;
        let (appearance, warnings) = Appearance::from_json(text).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(appearance.layout.max_width, 900);
        assert_eq!(appearance, {
            let mut expected = Appearance::default();
            expected.layout.max_width = 900;
            expected
        });
    }

    #[test]
    fn version_and_kind_are_repaired() {
        let (appearance, warnings) =
            Appearance::from_json(r#"{"version":99,"kind":"someone-else"}"#).unwrap();
        assert_eq!(appearance.version, VERSION);
        assert_eq!(appearance.kind, KIND);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn numeric_ranges_clamp_low_and_high() {
        let low = r#"{"typography":{"size":1,"line_height":0.1},"layout":{"max_width":0}}"#;
        let (appearance, warnings) = Appearance::from_json(low).unwrap();
        assert_eq!(appearance.typography.size, 12);
        assert_eq!(appearance.typography.line_height, 1.0);
        assert_eq!(appearance.layout.max_width, 480);
        assert_eq!(warnings.len(), 3, "{warnings:?}");

        let high = r#"{"typography":{"size":200,"line_height":12.0},"layout":{"max_width":40000}}"#;
        let (appearance, warnings) = Appearance::from_json(high).unwrap();
        assert_eq!(appearance.typography.size, 22);
        assert_eq!(appearance.typography.line_height, 2.0);
        assert_eq!(appearance.layout.max_width, 1600);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }

    #[test]
    fn line_height_handles_nan_and_rounding() {
        let mut appearance = Appearance::default();
        appearance.typography.line_height = f32::NAN;
        let warnings = appearance.sanitize();
        assert_eq!(appearance.typography.line_height, 1.45);
        assert_eq!(warnings.len(), 1, "{warnings:?}");

        let mut appearance = Appearance::default();
        appearance.typography.line_height = 1.234;
        let warnings = appearance.sanitize();
        assert!((appearance.typography.line_height - 1.23).abs() < f32::EPSILON);
        assert!(warnings.is_empty(), "{warnings:?}");

        let mut appearance = Appearance::default();
        appearance.typography.line_height = 1.0;
        assert!(appearance.sanitize().is_empty());
        assert_eq!(appearance.typography.line_height, 1.0);
    }

    #[test]
    fn bad_timestamp_format_is_repaired() {
        for format in ["%H:%M", "", "no tokens", "HH\u{7}mm"] {
            let text = serde_json::json!({"timestamps": {"format": format}}).to_string();
            let (appearance, warnings) = Appearance::from_json(&text).unwrap();
            assert_eq!(appearance.timestamps.format, "HH:mm:ss", "input {format:?}");
            assert_eq!(warnings.len(), 1, "input {format:?}: {warnings:?}");
        }

        let (valid, warnings) =
            Appearance::from_json(r#"{"timestamps":{"format":"DD MMM YYYY HH:mm"}}"#).unwrap();
        assert_eq!(valid.timestamps.format, "DD MMM YYYY HH:mm");
        assert!(warnings.is_empty());
    }

    #[test]
    fn empty_own_fallback_is_repaired() {
        for fallback in ["", "   ", "\u{7}", " \n\t "] {
            let text = serde_json::json!({"labels": {"own_fallback": fallback}}).to_string();
            let (appearance, warnings) = Appearance::from_json(&text).unwrap();
            assert_eq!(appearance.labels.own_fallback, "me", "input {fallback:?}");
            assert_eq!(warnings.len(), 1, "input {fallback:?}: {warnings:?}");
        }
    }

    #[test]
    fn own_fallback_is_trimmed_collapsed_and_capped() {
        let text = serde_json::json!({"labels": {"own_fallback": "  a \t  b  "}}).to_string();
        let (appearance, warnings) = Appearance::from_json(&text).unwrap();
        assert_eq!(appearance.labels.own_fallback, "a b");
        assert!(warnings.is_empty());

        let long = "x".repeat(100);
        let text = serde_json::json!({"labels": {"own_fallback": long}}).to_string();
        let (appearance, warnings) = Appearance::from_json(&text).unwrap();
        assert_eq!(appearance.labels.own_fallback.chars().count(), 32);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    #[test]
    fn custom_theme_invalid_color_is_repaired() {
        let text = r#"{"theme":{"name":"custom","background":"not-a-color"}}"#;
        let (appearance, warnings) = Appearance::from_json(text).unwrap();
        assert_eq!(appearance.theme.name, "custom");
        assert_eq!(appearance.theme.background.as_str(), "reset");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("background"), "{warnings:?}");
    }

    #[test]
    fn unknown_theme_name_keeps_colors() {
        let text = r##"{"theme":{"name":"neon","background":"#123456","text":"#abcdef"}}"##;
        let (appearance, warnings) = Appearance::from_json(text).unwrap();
        assert_eq!(appearance.theme.name, "custom");
        assert_eq!(appearance.theme.background.as_str(), "#123456");
        assert_eq!(appearance.theme.text.as_str(), "#abcdef");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("neon"), "{warnings:?}");
    }

    #[test]
    fn named_theme_replaces_colors() {
        let text = r##"{"theme":{"name":"matrix","background":"#ffffff"}}"##;
        let (appearance, warnings) = Appearance::from_json(text).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(appearance.theme, Theme::preset("matrix").unwrap());
    }

    #[test]
    fn empty_peer_palette_falls_back() {
        let text = r#"{"theme":{"name":"custom","peer_palette":[]}}"#;
        let (appearance, warnings) = Appearance::from_json(text).unwrap();
        assert_eq!(appearance.theme.peer_palette, Theme::default().peer_palette);
        assert_eq!(warnings.len(), 1, "{warnings:?}");

        let text = r##"{"theme":{"name":"custom","peer_palette":["#ff0000"]}}"##;
        let (appearance, warnings) = Appearance::from_json(text).unwrap();
        assert_eq!(
            appearance.theme.peer_palette,
            vec![ColorSpec::new("#ff0000")]
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn wrong_types_are_errors_not_panics() {
        let bad = [
            "",
            "not json",
            "\"appearance\"",
            "42",
            "null",
            "[]",
            "  [1, 2]",
            r#"{"layout":{"max_width":"wide"}}"#,
            r#"{"layout":{"message_style":"fancy"}}"#,
            r#"{"theme":{"background":5}}"#,
            r#"{"theme":"dracula"}"#,
            r#"{"timestamps":{"enabled":"yes"}}"#,
        ];
        for text in bad {
            assert!(
                Appearance::from_json(text).is_err(),
                "{text:?} should be rejected"
            );
        }
    }

    #[test]
    fn leading_bom_is_ignored() {
        let (appearance, warnings) = Appearance::from_json(&format!(
            "\u{feff}{}",
            Appearance::default().to_json_pretty()
        ))
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(appearance, Appearance::default());
    }

    #[test]
    fn presets_lists_all_in_order() {
        let presets = Appearance::presets();
        assert_eq!(presets.len(), Theme::PRESETS.len());
        let names: Vec<&str> = presets.iter().map(|theme| theme.name.as_str()).collect();
        assert_eq!(names.as_slice(), Theme::PRESETS);
    }

    #[test]
    fn json_uses_snake_case_names() {
        let json = Appearance::default().to_json_pretty();
        for key in [
            "\"message_style\"",
            "\"max_width\"",
            "\"line_height\"",
            "\"own_fallback\"",
            "\"show_status_bar\"",
            "\"peer_palette\"",
            "\"kind\"",
        ] {
            assert!(json.contains(key), "missing {key} in\n{json}");
        }
    }

    #[test]
    fn enum_helpers_agree() {
        assert_eq!(MessageStyle::ALL.len(), 2);
        for name in MessageStyle::ALL {
            assert_eq!(
                MessageStyle::parse(name).map(MessageStyle::as_str),
                Some(*name)
            );
        }
        assert_eq!(Density::ALL.len(), 3);
        for name in Density::ALL {
            assert_eq!(Density::parse(name).map(Density::as_str), Some(*name));
        }
        assert_eq!(FontFamily::ALL.len(), 4);
        for name in FontFamily::ALL {
            assert_eq!(FontFamily::parse(name).map(FontFamily::as_str), Some(*name));
        }
        assert_eq!(MessageStyle::parse("  IRC "), Some(MessageStyle::Irc));
        assert_eq!(Density::parse("nope"), None);
        assert_eq!(FontFamily::parse(""), None);
        assert_eq!(MessageStyle::default(), MessageStyle::Bubbles);
        assert_eq!(Density::default(), Density::Comfortable);
        assert_eq!(FontFamily::default(), FontFamily::System);
    }
}
