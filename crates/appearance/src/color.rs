//! Color values shared by every encchat frontend.
//!
//! A color is stored as text so it can live in JSON and be edited by hand. The
//! terminal clients understand color names and ANSI indexes; the web client
//! renders the very same value with [`ColorSpec::to_css`], so one stored color
//! looks as close as possible in both frontends. Nothing here depends on
//! `ratatui` or on a browser.

use std::fmt;

use serde::{Deserialize, Serialize};

/// ANSI colors 0-15 as rendered by the usual xterm palette.
const ANSI_16: [&str; 16] = [
    "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
    "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
];

/// The six levels of the xterm 6x6x6 color cube.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// A color stored as text so it can live in JSON and be edited by hand.
///
/// Accepted forms: a terminal color name (`cyan`, `darkgray`, `lightblue`), an
/// ANSI index (`0`..=`255`), `#rgb`, `#rgba`, `#rrggbb`, or `#rrggbbaa`.
/// `reset` and `default` mean "use the frontend's own default".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColorSpec(pub String);

impl ColorSpec {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate `value` and store a normalized (trimmed, lowercase) copy.
    pub fn checked(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("a color cannot be empty".to_string());
        }
        if parse_color(value).is_none() {
            return Err(format!(
                "unknown color {value:?}; use a name (cyan), an ANSI index (0-255), or hex (#89b4fa)"
            ));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    /// Whether the stored text names a color this crate understands.
    pub fn is_valid(&self) -> bool {
        parse_color(&self.0).is_some()
    }

    /// CSS value for the web: hex stays hex, an ANSI index or a name maps to a
    /// reasonable CSS color, and `reset`/`default` (or junk) return `None` so
    /// the caller can use its own default.
    pub fn to_css(&self) -> Option<String> {
        match parse_color(&self.0)? {
            Parsed::Reset => None,
            Parsed::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
            Parsed::Indexed(index) => Some(indexed_css(index)),
        }
    }
}

impl Default for ColorSpec {
    fn default() -> Self {
        Self("reset".to_string())
    }
}

impl fmt::Display for ColorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ColorSpec {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for ColorSpec {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parsed {
    Reset,
    Rgb(u8, u8, u8),
    Indexed(u8),
}

fn parse_color(value: &str) -> Option<Parsed> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower == "reset" || lower == "default" {
        return Some(Parsed::Reset);
    }
    if let Some(index) = named_index(&lower) {
        return Some(Parsed::Indexed(index));
    }
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex(hex);
    }
    // Only bare decimal digits count as an ANSI index; `+5`, `1.5` and `256`
    // are not colors.
    if lower.chars().all(|c| c.is_ascii_digit()) {
        return lower.parse::<u8>().ok().map(Parsed::Indexed);
    }
    None
}

/// Resolve a color name to its ANSI slot. Separators are ignored so
/// `lightgray`, `light_gray` and `light-gray` all work.
fn named_index(name: &str) -> Option<u8> {
    let compact: String = name.chars().filter(|c| *c != '-' && *c != '_').collect();
    let index = match compact.as_str() {
        "black" => 0,
        "red" => 1,
        "green" => 2,
        "yellow" => 3,
        "blue" => 4,
        "magenta" => 5,
        "cyan" => 6,
        "gray" | "grey" | "lightgray" | "lightgrey" => 7,
        "darkgray" | "darkgrey" => 8,
        "lightred" => 9,
        "lightgreen" => 10,
        "lightyellow" => 11,
        "lightblue" => 12,
        "lightmagenta" => 13,
        "lightcyan" => 14,
        "white" => 15,
        _ => return None,
    };
    Some(index)
}

fn parse_hex(hex: &str) -> Option<Parsed> {
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let expand = |c: char| c.to_digit(16).map(|digit| (digit as u8) * 17);
    let mut chars = hex.chars();
    match hex.len() {
        // `#rgb` and `#rgba`; the alpha channel is dropped for the terminal.
        3 | 4 => {
            let r = expand(chars.next()?)?;
            let g = expand(chars.next()?)?;
            let b = expand(chars.next()?)?;
            Some(Parsed::Rgb(r, g, b))
        }
        6 | 8 => {
            let mut byte = || -> Option<u8> {
                let high = chars.next()?.to_digit(16)? as u8;
                let low = chars.next()?.to_digit(16)? as u8;
                Some(high * 16 + low)
            };
            let r = byte()?;
            let g = byte()?;
            let b = byte()?;
            Some(Parsed::Rgb(r, g, b))
        }
        _ => None,
    }
}

fn indexed_css(index: u8) -> String {
    if let Some(hex) = ANSI_16.get(usize::from(index)) {
        return (*hex).to_string();
    }
    if index < 232 {
        let cube = index - 16;
        let r = CUBE_LEVELS[usize::from(cube / 36)];
        let g = CUBE_LEVELS[usize::from((cube % 36) / 6)];
        let b = CUBE_LEVELS[usize::from(cube % 6)];
        return format!("#{r:02x}{g:02x}{b:02x}");
    }
    let level = 8 + (index - 232) * 10;
    format!("#{level:02x}{level:02x}{level:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCEPTED: &[&str] = &[
        "#abc",
        "#abcd",
        "#aabbcc",
        "#aabbccdd",
        "#89B4FA",
        "0",
        "17",
        "255",
        "cyan",
        "darkgray",
        "dark_gray",
        "dark-grey",
        "lightblue",
        "light-blue",
        "white",
        "grey",
        "reset",
        "default",
    ];

    const REJECTED: &[&str] = &[
        "",
        "   ",
        "256",
        "-1",
        "1.5",
        "#12",
        "#",
        "#12345",
        "#gggggg",
        "not-a-color",
        "rgb(1,2,3)",
        "cyan!",
        "256.0",
    ];

    #[test]
    fn accepted_colors_validate_and_normalize() {
        for value in ACCEPTED {
            let color = ColorSpec::checked(value)
                .unwrap_or_else(|error| panic!("{value:?} should be accepted: {error}"));
            assert!(color.is_valid(), "{value:?} should validate");
            assert_eq!(
                color.as_str(),
                value.trim().to_ascii_lowercase(),
                "{value:?} should be normalized"
            );
            assert!(
                color.to_css().is_some() || *value == "reset" || *value == "default",
                "{value:?} should have a CSS value"
            );
        }
    }

    #[test]
    fn rejected_colors_are_refused() {
        for value in REJECTED {
            assert!(
                ColorSpec::checked(value).is_err(),
                "{value:?} should be rejected"
            );
            assert!(!ColorSpec::new(*value).is_valid(), "{value:?} is not valid");
        }
    }

    #[test]
    fn default_is_reset() {
        assert_eq!(ColorSpec::default().as_str(), "reset");
        assert_eq!(ColorSpec::default().to_css(), None);
        assert_eq!(ColorSpec::new("default").to_css(), None);
    }

    #[test]
    fn to_css_maps_hex_ansi_and_names() {
        let cases: &[(&str, Option<&str>)] = &[
            ("#89b4fa", Some("#89b4fa")),
            ("#89B4FA", Some("#89b4fa")),
            ("#abc", Some("#aabbcc")),
            ("#abcd", Some("#aabbcc")),
            ("#aabbccdd", Some("#aabbcc")),
            ("0", Some("#000000")),
            ("17", Some("#00005f")),
            ("232", Some("#080808")),
            ("255", Some("#eeeeee")),
            ("cyan", Some("#00cdcd")),
            ("white", Some("#ffffff")),
            ("darkgray", Some("#7f7f7f")),
            ("reset", None),
            ("nonsense", None),
        ];
        for (value, expected) in cases {
            assert_eq!(
                ColorSpec::new(*value).to_css().as_deref(),
                *expected,
                "CSS for {value:?}"
            );
        }
    }

    #[test]
    fn display_round_trips() {
        let color = ColorSpec::new("#aabbcc");
        assert_eq!(color.to_string(), "#aabbcc");
    }
}
