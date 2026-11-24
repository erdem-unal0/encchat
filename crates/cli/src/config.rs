//! Persistent, user-editable settings for the terminal client.
//!
//! Settings live in a small JSON file (see [`default_path`]) so they can be
//! edited by hand, by `encchat config set`, or by the full-screen customizer
//! (`encchat settings`, or `/settings` inside a chat). Every field is optional:
//! missing or invalid values fall back to the defaults in [`Config::default`].

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::format::{Item, StrftimeItems};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped when the on-disk shape changes. Older files are still accepted.
pub const CONFIG_VERSION: u32 = 1;

/// A color stored as text so it can live in JSON and be edited by hand.
///
/// Accepted forms: a ratatui color name (`cyan`, `darkgray`, `lightblue`), an
/// ANSI index (`0`..`255`), `#rrggbb`, or `#rgb`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColorSpec(String);

impl ColorSpec {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The ratatui color, or `None` if the stored text is not a valid color.
    pub fn color(&self) -> Option<Color> {
        parse_color(&self.0)
    }

    /// Validate `value` and store a normalized (lowercase) copy.
    pub fn checked(value: &str) -> Result<Self> {
        let value = value.trim();
        if value.is_empty() {
            bail!("a color cannot be empty");
        }
        if parse_color(value).is_none() {
            bail!(
                "unknown color {value:?}; use a name (cyan), an ANSI index (0-255), or hex (#89b4fa)"
            );
        }
        Ok(Self(value.to_ascii_lowercase()))
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

/// Colors offered by the settings editor when cycling a color with Left/Right.
pub const COLOR_CHOICES: &[&str] = &[
    "reset",
    "black",
    "darkgray",
    "gray",
    "white",
    "red",
    "lightred",
    "green",
    "lightgreen",
    "yellow",
    "lightyellow",
    "blue",
    "lightblue",
    "magenta",
    "lightmagenta",
    "cyan",
    "lightcyan",
    "#0b1020",
    "#1e1b4b",
    "#89b4fa",
    "#a6e3a1",
    "#f38ba8",
    "#f9e2af",
    "#cba6f7",
    "#94e2d5",
    "#fab387",
];

fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim().to_ascii_lowercase();
    let named = match value.as_str() {
        "reset" | "default" => Some(Color::Reset),
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" | "lightgray" | "light_gray" | "light-grey" => Some(Color::Gray),
        "darkgray" | "dark_gray" | "darkgrey" | "dark_grey" | "dark-gray" | "dark-grey" => {
            Some(Color::DarkGray)
        }
        "lightred" | "light_red" | "light-red" => Some(Color::LightRed),
        "lightgreen" | "light_green" | "light-green" => Some(Color::LightGreen),
        "lightyellow" | "light_yellow" | "light-yellow" => Some(Color::LightYellow),
        "lightblue" | "light_blue" | "light-blue" => Some(Color::LightBlue),
        "lightmagenta" | "light_magenta" | "light-magenta" => Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" | "light-cyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex(hex);
    }
    value.parse::<u8>().ok().map(Color::Indexed)
}

fn parse_hex(hex: &str) -> Option<Color> {
    let expand = |c: char| c.to_digit(16).map(|d| (d as u8) * 17);
    let pair = |a: char, b: char| -> Option<u8> { u8::from_str_radix(&format!("{a}{b}"), 16).ok() };
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            let r = expand(chars.next()?)?;
            let g = expand(chars.next()?)?;
            let b = expand(chars.next()?)?;
            Some(Color::Rgb(r, g, b))
        }
        6 => {
            let mut chars = hex.chars();
            let r = pair(chars.next()?, chars.next()?)?;
            let g = pair(chars.next()?, chars.next()?)?;
            let b = pair(chars.next()?, chars.next()?)?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// A named color theme. Individual colors can be overridden independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// Preset name, or `custom` once a color has been changed.
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
    /// Colors used to distinguish peers when `behavior.peer_colors` is on.
    pub peer_palette: Vec<ColorSpec>,
}

impl Theme {
    pub const PRESETS: &'static [&'static str] = &[
        "default",
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
            peer_palette: palette.iter().map(|c| ColorSpec::new(*c)).collect(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::preset("default").expect("the default theme is built in")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPosition {
    Top,
    Bottom,
    Hidden,
}

impl StatusPosition {
    pub const ALL: &'static [&'static str] = &["top", "bottom", "hidden"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Hidden => "hidden",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "top" => Some(Self::Top),
            "bottom" | "bottom_bar" => Some(Self::Bottom),
            "hidden" | "off" | "none" => Some(Self::Hidden),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusAlign {
    Left,
    Center,
    Right,
}

impl StatusAlign {
    pub const ALL: &'static [&'static str] = &["left", "center", "right"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => Some(Self::Left),
            "center" | "centre" | "middle" => Some(Self::Center),
            "right" | "end" => Some(Self::Right),
            _ => None,
        }
    }
}

/// One piece of information shown in the status bar, in user-chosen order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusSegment {
    App,
    Connection,
    Server,
    Room,
    Peers,
    Nick,
    Device,
    Sessions,
    Clock,
}

impl StatusSegment {
    pub const ALL: &'static [&'static str] = &[
        "app",
        "connection",
        "server",
        "room",
        "peers",
        "nick",
        "device",
        "sessions",
        "clock",
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Connection => "connection",
            Self::Server => "server",
            Self::Room => "room",
            Self::Peers => "peers",
            Self::Nick => "nick",
            Self::Device => "device",
            Self::Sessions => "sessions",
            Self::Clock => "clock",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "app" | "appname" | "name" => Some(Self::App),
            "connection" | "status" | "online" => Some(Self::Connection),
            "server" | "relay" => Some(Self::Server),
            "room" | "fingerprint" | "room_id" => Some(Self::Room),
            "peers" | "peer_count" => Some(Self::Peers),
            "nick" | "nickname" | "me" => Some(Self::Nick),
            "device" | "device_id" => Some(Self::Device),
            "sessions" | "session_count" => Some(Self::Sessions),
            "clock" | "time" => Some(Self::Clock),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub status_position: StatusPosition,
    pub status_separator: String,
    pub status_align: StatusAlign,
    pub status_segments: Vec<StatusSegment>,
    pub clock_format: String,
    pub show_hints: bool,
    pub show_scroll_indicator: bool,
    pub show_unread: bool,
    pub input_border: bool,
    pub scroll_step: u16,
    pub page_step: u16,
    pub max_scrollback: usize,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            status_position: StatusPosition::Top,
            status_separator: " | ".to_string(),
            status_align: StatusAlign::Left,
            status_segments: vec![
                StatusSegment::App,
                StatusSegment::Server,
                StatusSegment::Room,
                StatusSegment::Peers,
                StatusSegment::Connection,
                StatusSegment::Clock,
            ],
            clock_format: "%H:%M".to_string(),
            show_hints: true,
            show_scroll_indicator: true,
            show_unread: true,
            input_border: true,
            scroll_step: 3,
            page_step: 10,
            max_scrollback: 5_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timestamps {
    pub enabled: bool,
    pub format: String,
    pub utc: bool,
    /// Also show timestamps on system/notice lines.
    pub system: bool,
}

impl Default for Timestamps {
    fn default() -> Self {
        Self {
            enabled: true,
            format: "%H:%M:%S".to_string(),
            utc: false,
            system: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Behavior {
    pub mouse: bool,
    pub auto_scroll: bool,
    pub show_help_on_start: bool,
    pub peer_colors: bool,
    pub input_history_size: usize,
    pub own_nick_fallback: String,
    pub system_prefix: String,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            mouse: true,
            auto_scroll: false,
            show_help_on_start: true,
            peer_colors: true,
            input_history_size: 500,
            own_nick_fallback: "me".to_string(),
            system_prefix: "• ".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prompt {
    /// Template for the input prompt. Supports `{nick}`, `{room}` and `{device}`.
    pub template: String,
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            template: "{nick} ".to_string(),
        }
    }
}

/// Actions whose keys can be remapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Quit,
    Settings,
    Help,
    ScrollUp,
    ScrollDown,
    Complete,
    ClearInput,
    DeleteWord,
    HistoryPrev,
    HistoryNext,
    ToggleTimestamps,
    CycleTheme,
}

impl KeyAction {
    pub const ALL: &'static [KeyAction] = &[
        KeyAction::Quit,
        KeyAction::Settings,
        KeyAction::Help,
        KeyAction::ScrollUp,
        KeyAction::ScrollDown,
        KeyAction::Complete,
        KeyAction::ClearInput,
        KeyAction::DeleteWord,
        KeyAction::HistoryPrev,
        KeyAction::HistoryNext,
        KeyAction::ToggleTimestamps,
        KeyAction::CycleTheme,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quit => "Quit",
            Self::Settings => "Open settings",
            Self::Help => "Show help",
            Self::ScrollUp => "Scroll up a page",
            Self::ScrollDown => "Scroll down a page",
            Self::Complete => "Complete command",
            Self::ClearInput => "Clear input",
            Self::DeleteWord => "Delete previous word",
            Self::HistoryPrev => "Previous input",
            Self::HistoryNext => "Next input",
            Self::ToggleTimestamps => "Toggle timestamps",
            Self::CycleTheme => "Cycle theme",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Settings => "settings",
            Self::Help => "help",
            Self::ScrollUp => "scroll_up",
            Self::ScrollDown => "scroll_down",
            Self::Complete => "complete",
            Self::ClearInput => "clear_input",
            Self::DeleteWord => "delete_word",
            Self::HistoryPrev => "history_prev",
            Self::HistoryNext => "history_next",
            Self::ToggleTimestamps => "toggle_timestamps",
            Self::CycleTheme => "cycle_theme",
        }
    }
}

/// A parsed key chord such as `ctrl+c`, `f2`, or `shift+tab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyChord {
    pub fn parse(input: &str) -> Option<Self> {
        let text = input.trim();
        if text.is_empty() {
            return None;
        }
        let mut parts: Vec<&str> = text
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        let key = parts.pop()?;
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" => ctrl = true,
                "alt" | "option" | "opt" => alt = true,
                "shift" => shift = true,
                _ => return None,
            }
        }

        let lowered = key.to_ascii_lowercase();
        let mut code = match lowered.as_str() {
            "backspace" | "bs" => KeyCode::Backspace,
            "enter" | "return" => KeyCode::Enter,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" | "page_up" | "pgup" => KeyCode::PageUp,
            "pagedown" | "page_down" | "pgdn" => KeyCode::PageDown,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "delete" | "del" => KeyCode::Delete,
            "insert" | "ins" => KeyCode::Insert,
            "esc" | "escape" => KeyCode::Esc,
            "space" => KeyCode::Char(' '),
            "plus" => KeyCode::Char('+'),
            "minus" => KeyCode::Char('-'),
            "comma" => KeyCode::Char(','),
            "period" | "dot" => KeyCode::Char('.'),
            "slash" => KeyCode::Char('/'),
            "backslash" => KeyCode::Char('\\'),
            "semicolon" => KeyCode::Char(';'),
            "quote" => KeyCode::Char('\''),
            "backtick" => KeyCode::Char('`'),
            "leftbracket" | "lbracket" => KeyCode::Char('['),
            "rightbracket" | "rbracket" => KeyCode::Char(']'),
            _ => {
                if let Some(number) = lowered
                    .strip_prefix('f')
                    .and_then(|number| number.parse::<u8>().ok())
                {
                    if (1..=24).contains(&number) {
                        KeyCode::F(number)
                    } else {
                        return None;
                    }
                } else if key.chars().count() == 1 {
                    KeyCode::Char(key.chars().next()?)
                } else {
                    return None;
                }
            }
        };
        // Terminals report Shift+Tab as BackTab; accept both spellings and
        // treat the chord as BackTab.
        if code == KeyCode::Tab && shift {
            code = KeyCode::BackTab;
        }
        if code == KeyCode::BackTab {
            shift = false;
        }
        Some(Self {
            code,
            ctrl,
            alt,
            shift,
        })
    }

    /// A plain printable character would steal ordinary typing, so bindings
    /// must use Ctrl/Alt or a navigation/function key.
    pub fn is_plain_char(self) -> bool {
        !self.ctrl && !self.alt && matches!(self.code, KeyCode::Char(c) if !c.is_control())
    }

    pub fn matches(self, event: &KeyEvent) -> bool {
        if self.code != event.code {
            return false;
        }
        if event.modifiers.contains(KeyModifiers::CONTROL) != self.ctrl
            || event.modifiers.contains(KeyModifiers::ALT) != self.alt
        {
            return false;
        }
        // The character itself carries the case, so ignore SHIFT for chars.
        // BackTab already implies Shift on every terminal that reports it.
        if matches!(self.code, KeyCode::Char(_) | KeyCode::BackTab) {
            true
        } else {
            event.modifiers.contains(KeyModifiers::SHIFT) == self.shift
        }
    }

    pub fn pretty(self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift && self.code != KeyCode::BackTab {
            out.push_str("Shift+");
        }
        out.push_str(&match self.code {
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Left => "←".to_string(),
            KeyCode::Right => "→".to_string(),
            KeyCode::Up => "↑".to_string(),
            KeyCode::Down => "↓".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PgUp".to_string(),
            KeyCode::PageDown => "PgDn".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "Shift+Tab".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Insert => "Insert".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::F(number) => format!("F{number}"),
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
            other => format!("{other:?}"),
        });
        out
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.pretty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keybinds {
    pub quit: String,
    pub settings: String,
    pub help: String,
    pub scroll_up: String,
    pub scroll_down: String,
    pub complete: String,
    pub clear_input: String,
    pub delete_word: String,
    pub history_prev: String,
    pub history_next: String,
    pub toggle_timestamps: String,
    pub cycle_theme: String,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            quit: "ctrl+c".to_string(),
            settings: "f2".to_string(),
            help: "f1".to_string(),
            scroll_up: "pageup".to_string(),
            scroll_down: "pagedown".to_string(),
            complete: "tab".to_string(),
            clear_input: "ctrl+u".to_string(),
            delete_word: "ctrl+w".to_string(),
            history_prev: "up".to_string(),
            history_next: "down".to_string(),
            toggle_timestamps: "f3".to_string(),
            cycle_theme: "f4".to_string(),
        }
    }
}

impl Keybinds {
    pub fn spec(&self, action: KeyAction) -> &str {
        match action {
            KeyAction::Quit => &self.quit,
            KeyAction::Settings => &self.settings,
            KeyAction::Help => &self.help,
            KeyAction::ScrollUp => &self.scroll_up,
            KeyAction::ScrollDown => &self.scroll_down,
            KeyAction::Complete => &self.complete,
            KeyAction::ClearInput => &self.clear_input,
            KeyAction::DeleteWord => &self.delete_word,
            KeyAction::HistoryPrev => &self.history_prev,
            KeyAction::HistoryNext => &self.history_next,
            KeyAction::ToggleTimestamps => &self.toggle_timestamps,
            KeyAction::CycleTheme => &self.cycle_theme,
        }
    }

    fn set(&mut self, action: KeyAction, value: String) {
        match action {
            KeyAction::Quit => self.quit = value,
            KeyAction::Settings => self.settings = value,
            KeyAction::Help => self.help = value,
            KeyAction::ScrollUp => self.scroll_up = value,
            KeyAction::ScrollDown => self.scroll_down = value,
            KeyAction::Complete => self.complete = value,
            KeyAction::ClearInput => self.clear_input = value,
            KeyAction::DeleteWord => self.delete_word = value,
            KeyAction::HistoryPrev => self.history_prev = value,
            KeyAction::HistoryNext => self.history_next = value,
            KeyAction::ToggleTimestamps => self.toggle_timestamps = value,
            KeyAction::CycleTheme => self.cycle_theme = value,
        }
    }

    pub fn matches(&self, action: KeyAction, event: &KeyEvent) -> bool {
        KeyChord::parse(self.spec(action))
            .map(|chord| chord.matches(event))
            .unwrap_or(false)
    }

    pub fn pretty(&self, action: KeyAction) -> String {
        KeyChord::parse(self.spec(action))
            .map(KeyChord::pretty)
            .unwrap_or_else(|| self.spec(action).to_string())
    }
}

/// The complete client configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub theme: Theme,
    pub layout: LayoutConfig,
    pub timestamps: Timestamps,
    pub behavior: Behavior,
    pub prompt: Prompt,
    pub keys: Keybinds,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            theme: Theme::default(),
            layout: LayoutConfig::default(),
            timestamps: Timestamps::default(),
            behavior: Behavior::default(),
            prompt: Prompt::default(),
            keys: Keybinds::default(),
        }
    }
}

/// Where the config lives unless `--config` or `ENCCHAT_CONFIG` says otherwise.
pub fn default_path() -> PathBuf {
    if let Some(path) = env::var_os("ENCCHAT_CONFIG") {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("encchat")
        .join("config.json")
}

pub fn resolve_path(explicit: Option<&Path>) -> PathBuf {
    explicit.map(Path::to_path_buf).unwrap_or_else(default_path)
}

/// The result of loading a config file: usable settings plus human-readable
/// warnings (invalid file, sanitized values, ...).
pub struct LoadedConfig {
    pub config: Config,
    pub warnings: Vec<String>,
}

/// Deserialize a config, expanding a named theme first.
fn parse_config(text: &str) -> serde_json::Result<Config> {
    let raw: Value = serde_json::from_str(text)?;
    serde_json::from_value(apply_theme_preset(raw))
}

/// Start from a named preset, then let explicitly written fields win.
///
/// Every struct has `#[serde(default)]`, so `{"theme": {"name": "dracula"}}`
/// would otherwise keep the *default* palette while claiming to be Dracula.
/// A hand-edited theme name is meant to select that theme, so merge the preset
/// under the user's fields. If the merged colors differ from the preset,
/// [`Config::sanitize`] labels the result `custom`.
fn apply_theme_preset(raw: Value) -> Value {
    let Some(theme) = raw.get("theme").and_then(Value::as_object) else {
        return raw;
    };
    let Some(name) = theme.get("name").and_then(Value::as_str) else {
        return raw;
    };
    let Some(preset) = Theme::preset(name) else {
        return raw;
    };
    let Ok(Value::Object(mut merged)) = serde_json::to_value(preset) else {
        return raw;
    };
    merged.extend(theme.clone());
    let mut raw = raw;
    if let Some(root) = raw.as_object_mut() {
        root.insert("theme".to_string(), Value::Object(merged));
    }
    raw
}

/// Load `path`, falling back to defaults without overwriting a bad file.
pub fn load(path: &Path) -> LoadedConfig {
    match fs::read_to_string(path) {
        Ok(text) => {
            // Notepad and some editors save UTF-8 with a BOM, which serde_json
            // rejects even though the rest of the file is valid.
            let text = text.trim_start_matches('\u{feff}');
            match parse_config(text) {
                Ok(mut config) => {
                    let mut warnings = Vec::new();
                    config.sanitize(&mut warnings);
                    LoadedConfig { config, warnings }
                }
                Err(error) => LoadedConfig {
                    config: Config::default(),
                    warnings: vec![format!(
                        "could not parse {} ({error}); using defaults",
                        path.display()
                    )],
                },
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => LoadedConfig {
            config: Config::default(),
            warnings: Vec::new(),
        },
        Err(error) => LoadedConfig {
            config: Config::default(),
            warnings: vec![format!(
                "could not read {} ({error}); using defaults",
                path.display()
            )],
        },
    }
}

/// Pretty-print and atomically write the config.
pub fn save(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let mut config = config.clone();
    config.version = CONFIG_VERSION;
    let text = serde_json::to_string_pretty(&config)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).with_context(|| format!("could not write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

fn valid_strftime(format: &str) -> bool {
    !format.trim().is_empty() && !StrftimeItems::new(format).any(|item| matches!(item, Item::Error))
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => bail!("expected true/false, on/off, yes/no, or 1/0"),
    }
}

fn parse_int<T>(value: &str, min: i64, max: i64, label: &str) -> Result<T>
where
    T: TryFrom<i64>,
{
    let parsed: i64 = value
        .trim()
        .parse()
        .with_context(|| format!("{label} must be a whole number"))?;
    if parsed < min || parsed > max {
        bail!("{label} must be between {min} and {max}");
    }
    T::try_from(parsed).map_err(|_| anyhow::anyhow!("{label} is out of range"))
}

fn parse_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

impl Config {
    /// Read one setting as text. Used by `config get`, `/keys` and the editor.
    pub fn get(&self, key: &str) -> Result<String> {
        let value = match key {
            "theme.preset" => self.theme.name.clone(),
            "theme.background" => self.theme.background.to_string(),
            "theme.text" => self.theme.text.to_string(),
            "theme.status_bg" => self.theme.status_bg.to_string(),
            "theme.status_fg" => self.theme.status_fg.to_string(),
            "theme.status_accent" => self.theme.status_accent.to_string(),
            "theme.incoming_nick" => self.theme.incoming_nick.to_string(),
            "theme.outgoing_nick" => self.theme.outgoing_nick.to_string(),
            "theme.timestamp" => self.theme.timestamp.to_string(),
            "theme.system" => self.theme.system.to_string(),
            "theme.error" => self.theme.error.to_string(),
            "theme.input_prompt" => self.theme.input_prompt.to_string(),
            "theme.input_text" => self.theme.input_text.to_string(),
            "theme.input_bg" => self.theme.input_bg.to_string(),
            "theme.border" => self.theme.border.to_string(),
            "theme.peer_palette" => self
                .theme
                .peer_palette
                .iter()
                .map(ColorSpec::as_str)
                .collect::<Vec<_>>()
                .join(","),
            "layout.status_position" => self.layout.status_position.as_str().to_string(),
            "layout.status_separator" => self.layout.status_separator.clone(),
            "layout.status_align" => self.layout.status_align.as_str().to_string(),
            "layout.status_segments" => self
                .layout
                .status_segments
                .iter()
                .map(|segment| segment.as_str())
                .collect::<Vec<_>>()
                .join(","),
            "layout.clock_format" => self.layout.clock_format.clone(),
            "layout.show_hints" => self.layout.show_hints.to_string(),
            "layout.show_scroll_indicator" => self.layout.show_scroll_indicator.to_string(),
            "layout.show_unread" => self.layout.show_unread.to_string(),
            "layout.input_border" => self.layout.input_border.to_string(),
            "layout.scroll_step" => self.layout.scroll_step.to_string(),
            "layout.page_step" => self.layout.page_step.to_string(),
            "layout.max_scrollback" => self.layout.max_scrollback.to_string(),
            "timestamps.enabled" => self.timestamps.enabled.to_string(),
            "timestamps.format" => self.timestamps.format.clone(),
            "timestamps.utc" => self.timestamps.utc.to_string(),
            "timestamps.system" => self.timestamps.system.to_string(),
            "behavior.mouse" => self.behavior.mouse.to_string(),
            "behavior.auto_scroll" => self.behavior.auto_scroll.to_string(),
            "behavior.show_help_on_start" => self.behavior.show_help_on_start.to_string(),
            "behavior.peer_colors" => self.behavior.peer_colors.to_string(),
            "behavior.input_history_size" => self.behavior.input_history_size.to_string(),
            "behavior.own_nick_fallback" => self.behavior.own_nick_fallback.clone(),
            "behavior.system_prefix" => self.behavior.system_prefix.clone(),
            "prompt.template" => self.prompt.template.clone(),
            key if key.starts_with("keys.") => {
                let action = KeyAction::ALL
                    .iter()
                    .copied()
                    .find(|action| action.key() == &key[5..])
                    .ok_or_else(|| unknown_key(key))?;
                self.keys.spec(action).to_string()
            }
            _ => return Err(unknown_key(key)),
        };
        Ok(value)
    }

    /// Set one setting from text. The value is validated before anything is
    /// changed, so a failed `set` leaves the config untouched.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let trimmed = value.trim();
        match key {
            "theme.preset" => {
                self.theme = Theme::preset(trimmed).ok_or_else(|| {
                    anyhow::anyhow!(
                        "unknown theme {trimmed:?}; choose one of: {}",
                        Theme::PRESETS.join(", ")
                    )
                })?;
            }
            "theme.background" => self.set_color(ThemeField::Background, trimmed)?,
            "theme.text" => self.set_color(ThemeField::Text, trimmed)?,
            "theme.status_bg" => self.set_color(ThemeField::StatusBg, trimmed)?,
            "theme.status_fg" => self.set_color(ThemeField::StatusFg, trimmed)?,
            "theme.status_accent" => self.set_color(ThemeField::StatusAccent, trimmed)?,
            "theme.incoming_nick" => self.set_color(ThemeField::IncomingNick, trimmed)?,
            "theme.outgoing_nick" => self.set_color(ThemeField::OutgoingNick, trimmed)?,
            "theme.timestamp" => self.set_color(ThemeField::Timestamp, trimmed)?,
            "theme.system" => self.set_color(ThemeField::System, trimmed)?,
            "theme.error" => self.set_color(ThemeField::Error, trimmed)?,
            "theme.input_prompt" => self.set_color(ThemeField::InputPrompt, trimmed)?,
            "theme.input_text" => self.set_color(ThemeField::InputText, trimmed)?,
            "theme.input_bg" => self.set_color(ThemeField::InputBg, trimmed)?,
            "theme.border" => self.set_color(ThemeField::Border, trimmed)?,
            "theme.peer_palette" => {
                let mut palette = Vec::new();
                for part in parse_list(trimmed) {
                    palette.push(ColorSpec::checked(part)?);
                }
                if palette.is_empty() {
                    bail!("theme.peer_palette needs at least one color");
                }
                self.theme.peer_palette = palette;
                self.theme.name = "custom".to_string();
            }
            "layout.status_position" => {
                self.layout.status_position = StatusPosition::parse(trimmed).ok_or_else(|| {
                    anyhow::anyhow!(
                        "unknown status position {trimmed:?}; use {}",
                        StatusPosition::ALL.join(", ")
                    )
                })?;
            }
            "layout.status_separator" => {
                if value.chars().count() > 32 {
                    bail!("layout.status_separator is limited to 32 characters");
                }
                self.layout.status_separator = value.to_string();
            }
            "layout.status_align" => {
                self.layout.status_align = StatusAlign::parse(trimmed).ok_or_else(|| {
                    anyhow::anyhow!(
                        "unknown status alignment {trimmed:?}; use {}",
                        StatusAlign::ALL.join(", ")
                    )
                })?;
            }
            "layout.status_segments" => {
                let mut segments = Vec::new();
                for part in parse_list(trimmed) {
                    let segment = StatusSegment::parse(part).ok_or_else(|| {
                        anyhow::anyhow!(
                            "unknown status segment {part:?}; use {}",
                            StatusSegment::ALL.join(", ")
                        )
                    })?;
                    if !segments.contains(&segment) {
                        segments.push(segment);
                    }
                }
                if segments.is_empty() {
                    bail!("layout.status_segments needs at least one segment");
                }
                self.layout.status_segments = segments;
            }
            "layout.clock_format" => {
                if !valid_strftime(trimmed) {
                    bail!("layout.clock_format is not a valid strftime format");
                }
                self.layout.clock_format = trimmed.to_string();
            }
            "layout.show_hints" => self.layout.show_hints = parse_bool(trimmed)?,
            "layout.show_scroll_indicator" => {
                self.layout.show_scroll_indicator = parse_bool(trimmed)?;
            }
            "layout.show_unread" => self.layout.show_unread = parse_bool(trimmed)?,
            "layout.input_border" => self.layout.input_border = parse_bool(trimmed)?,
            "layout.scroll_step" => {
                self.layout.scroll_step = parse_int(trimmed, 1, 100, "layout.scroll_step")?;
            }
            "layout.page_step" => {
                self.layout.page_step = parse_int(trimmed, 1, 100, "layout.page_step")?;
            }
            "layout.max_scrollback" => {
                self.layout.max_scrollback =
                    parse_int(trimmed, 50, 100_000, "layout.max_scrollback")?;
            }
            "timestamps.enabled" => self.timestamps.enabled = parse_bool(trimmed)?,
            "timestamps.format" => {
                if !valid_strftime(trimmed) {
                    bail!("timestamps.format is not a valid strftime format");
                }
                self.timestamps.format = trimmed.to_string();
            }
            "timestamps.utc" => self.timestamps.utc = parse_bool(trimmed)?,
            "timestamps.system" => self.timestamps.system = parse_bool(trimmed)?,
            "behavior.mouse" => self.behavior.mouse = parse_bool(trimmed)?,
            "behavior.auto_scroll" => self.behavior.auto_scroll = parse_bool(trimmed)?,
            "behavior.show_help_on_start" => {
                self.behavior.show_help_on_start = parse_bool(trimmed)?;
            }
            "behavior.peer_colors" => self.behavior.peer_colors = parse_bool(trimmed)?,
            "behavior.input_history_size" => {
                self.behavior.input_history_size =
                    parse_int(trimmed, 10, 10_000, "behavior.input_history_size")?;
            }
            "behavior.own_nick_fallback" => {
                if trimmed.is_empty() {
                    bail!("behavior.own_nick_fallback cannot be empty");
                }
                if trimmed.chars().count() > 24 {
                    bail!("behavior.own_nick_fallback is limited to 24 characters");
                }
                self.behavior.own_nick_fallback = trimmed.to_string();
            }
            "behavior.system_prefix" => {
                if value.chars().count() > 16 {
                    bail!("behavior.system_prefix is limited to 16 characters");
                }
                self.behavior.system_prefix = value.to_string();
            }
            "prompt.template" => {
                if trimmed.is_empty() {
                    bail!("prompt.template cannot be empty");
                }
                if value.chars().count() > 64 {
                    bail!("prompt.template is limited to 64 characters");
                }
                self.prompt.template = value.to_string();
            }
            key if key.starts_with("keys.") => {
                let action = KeyAction::ALL
                    .iter()
                    .copied()
                    .find(|action| action.key() == &key[5..])
                    .ok_or_else(|| unknown_key(key))?;
                self.set_keybind(action, trimmed)?;
            }
            _ => return Err(unknown_key(key)),
        }
        Ok(())
    }

    /// Reset one setting to its built-in default.
    pub fn reset_key(&mut self, key: &str) -> Result<()> {
        let defaults = Self::default();
        let value = defaults.get(key)?;
        self.set(key, &value)
    }

    fn set_color(&mut self, field: ThemeField, value: &str) -> Result<()> {
        let color = ColorSpec::checked(value)?;
        match field {
            ThemeField::Background => self.theme.background = color,
            ThemeField::Text => self.theme.text = color,
            ThemeField::StatusBg => self.theme.status_bg = color,
            ThemeField::StatusFg => self.theme.status_fg = color,
            ThemeField::StatusAccent => self.theme.status_accent = color,
            ThemeField::IncomingNick => self.theme.incoming_nick = color,
            ThemeField::OutgoingNick => self.theme.outgoing_nick = color,
            ThemeField::Timestamp => self.theme.timestamp = color,
            ThemeField::System => self.theme.system = color,
            ThemeField::Error => self.theme.error = color,
            ThemeField::InputPrompt => self.theme.input_prompt = color,
            ThemeField::InputText => self.theme.input_text = color,
            ThemeField::InputBg => self.theme.input_bg = color,
            ThemeField::Border => self.theme.border = color,
        }
        self.theme.name = "custom".to_string();
        Ok(())
    }

    fn set_keybind(&mut self, action: KeyAction, value: &str) -> Result<()> {
        let chord = KeyChord::parse(value).ok_or_else(|| {
            anyhow::anyhow!("invalid key {value:?}; examples: ctrl+c, f2, alt+enter, shift+tab")
        })?;
        if chord.is_plain_char() {
            bail!(
                "a binding must use Ctrl/Alt or a navigation/function key, otherwise it would block typing"
            );
        }
        for other in KeyAction::ALL
            .iter()
            .copied()
            .filter(|other| *other != action)
        {
            if KeyChord::parse(self.keys.spec(other)) == Some(chord) {
                bail!(
                    "{} is already bound to {}",
                    chord.pretty(),
                    other.label().to_ascii_lowercase()
                );
            }
        }
        self.keys.set(action, value.to_string());
        Ok(())
    }

    /// Repair values that could have come from a hand-edited or old config.
    pub fn sanitize(&mut self, warnings: &mut Vec<String>) {
        if self.version > CONFIG_VERSION {
            warnings.push(format!(
                "config version {} is newer than this build ({}); unknown fields were ignored",
                self.version, CONFIG_VERSION
            ));
        }
        self.version = CONFIG_VERSION;

        let named_preset = if self.theme.name == "custom" {
            None
        } else {
            match Theme::preset(&self.theme.name) {
                Some(preset) => {
                    self.theme.name = preset.name.clone();
                    Some(preset)
                }
                None => {
                    warnings.push(format!(
                        "theme.preset {:?} is unknown; keeping the colors as a custom theme",
                        self.theme.name
                    ));
                    self.theme.name = "custom".to_string();
                    None
                }
            }
        };
        sanitize_color(&mut self.theme.background, "theme.background", warnings);
        sanitize_color(&mut self.theme.text, "theme.text", warnings);
        sanitize_color(&mut self.theme.status_bg, "theme.status_bg", warnings);
        sanitize_color(&mut self.theme.status_fg, "theme.status_fg", warnings);
        sanitize_color(
            &mut self.theme.status_accent,
            "theme.status_accent",
            warnings,
        );
        sanitize_color(
            &mut self.theme.incoming_nick,
            "theme.incoming_nick",
            warnings,
        );
        sanitize_color(
            &mut self.theme.outgoing_nick,
            "theme.outgoing_nick",
            warnings,
        );
        sanitize_color(&mut self.theme.timestamp, "theme.timestamp", warnings);
        sanitize_color(&mut self.theme.system, "theme.system", warnings);
        sanitize_color(&mut self.theme.error, "theme.error", warnings);
        sanitize_color(&mut self.theme.input_prompt, "theme.input_prompt", warnings);
        sanitize_color(&mut self.theme.input_text, "theme.input_text", warnings);
        sanitize_color(&mut self.theme.input_bg, "theme.input_bg", warnings);
        sanitize_color(&mut self.theme.border, "theme.border", warnings);
        self.theme
            .peer_palette
            .retain(|color| color.color().is_some());
        if self.theme.peer_palette.is_empty() {
            warnings.push("theme.peer_palette was empty or invalid; using the default".into());
            self.theme.peer_palette = Theme::default().peer_palette;
        }
        // A preset name with edited colors is a custom theme, matching what
        // `config set theme.<color>` would have done.
        if let Some(preset) = named_preset {
            if !same_theme_colors(&self.theme, &preset) {
                self.theme.name = "custom".to_string();
            }
        }

        if self.layout.status_segments.is_empty() {
            warnings.push("layout.status_segments was empty; using the default".into());
            self.layout.status_segments = LayoutConfig::default().status_segments;
        } else {
            let before = self.layout.status_segments.len();
            let mut seen = Vec::new();
            self.layout.status_segments.retain(|segment| {
                if seen.contains(segment) {
                    false
                } else {
                    seen.push(*segment);
                    true
                }
            });
            if self.layout.status_segments.len() != before {
                warnings.push("layout.status_segments contained duplicates; removed them".into());
            }
        }
        if self.layout.scroll_step == 0 || self.layout.scroll_step > 100 {
            warnings.push("layout.scroll_step was out of range; clamped to 1-100".into());
            self.layout.scroll_step = self.layout.scroll_step.clamp(1, 100);
        }
        if self.layout.page_step == 0 || self.layout.page_step > 100 {
            warnings.push("layout.page_step was out of range; clamped to 1-100".into());
            self.layout.page_step = self.layout.page_step.clamp(1, 100);
        }
        if self.layout.max_scrollback < 50 || self.layout.max_scrollback > 100_000 {
            warnings.push("layout.max_scrollback was out of range; clamped to 50-100000".into());
            self.layout.max_scrollback = self.layout.max_scrollback.clamp(50, 100_000);
        }
        if self.layout.status_separator.chars().count() > 32 {
            warnings.push("layout.status_separator was too long; truncated".into());
            self.layout.status_separator = self.layout.status_separator.chars().take(32).collect();
        }
        if !valid_strftime(&self.layout.clock_format) {
            warnings.push("layout.clock_format was invalid; using %H:%M".into());
            self.layout.clock_format = "%H:%M".to_string();
        }
        if !valid_strftime(&self.timestamps.format) {
            warnings.push("timestamps.format was invalid; using %H:%M:%S".into());
            self.timestamps.format = "%H:%M:%S".to_string();
        }
        if self.behavior.input_history_size < 10 || self.behavior.input_history_size > 10_000 {
            warnings.push("behavior.input_history_size was out of range; clamped".into());
            self.behavior.input_history_size = self.behavior.input_history_size.clamp(10, 10_000);
        }
        let fallback = self.behavior.own_nick_fallback.trim().to_string();
        if fallback.is_empty() {
            self.behavior.own_nick_fallback = "me".to_string();
        } else {
            let too_long = fallback.chars().count() > 24;
            self.behavior.own_nick_fallback = fallback.chars().take(24).collect();
            if too_long {
                warnings.push("behavior.own_nick_fallback was too long; truncated".into());
            }
        }
        if self.behavior.system_prefix.chars().count() > 16 {
            warnings.push("behavior.system_prefix was too long; truncated".into());
            self.behavior.system_prefix = self.behavior.system_prefix.chars().take(16).collect();
        }
        if self.prompt.template.trim().is_empty() {
            warnings.push("prompt.template was empty; using the default".into());
            self.prompt.template = Prompt::default().template;
        } else if self.prompt.template.chars().count() > 64 {
            warnings.push("prompt.template was too long; truncated".into());
            self.prompt.template = self.prompt.template.chars().take(64).collect();
        }

        let mut seen: Vec<KeyChord> = Vec::new();
        let mut keys_need_reset = false;
        for action in KeyAction::ALL.iter().copied() {
            let spec = self.keys.spec(action);
            match KeyChord::parse(spec) {
                Some(chord) if !chord.is_plain_char() => {
                    if seen.contains(&chord) {
                        warnings.push(format!(
                            "keys.{} duplicated another binding; all key bindings were reset",
                            action.key()
                        ));
                        keys_need_reset = true;
                        break;
                    }
                    seen.push(chord);
                }
                _ => {
                    warnings.push(format!(
                        "keys.{} was invalid; all key bindings were reset",
                        action.key()
                    ));
                    keys_need_reset = true;
                    break;
                }
            }
        }
        if keys_need_reset {
            self.keys = Keybinds::default();
        }
    }
}

fn unknown_key(key: &str) -> anyhow::Error {
    anyhow::anyhow!("unknown setting {key:?}; run `encchat config keys` for the full list")
}

#[derive(Debug, Clone, Copy)]
enum ThemeField {
    Background,
    Text,
    StatusBg,
    StatusFg,
    StatusAccent,
    IncomingNick,
    OutgoingNick,
    Timestamp,
    System,
    Error,
    InputPrompt,
    InputText,
    InputBg,
    Border,
}

fn sanitize_color(color: &mut ColorSpec, key: &str, warnings: &mut Vec<String>) {
    if color.color().is_none() {
        warnings.push(format!("{key} was not a valid color; using reset"));
        *color = ColorSpec::new("reset");
    }
}

/// Compare every rendered color, ignoring the preset name.
fn same_theme_colors(a: &Theme, b: &Theme) -> bool {
    let same = |left: &ColorSpec, right: &ColorSpec| left.color() == right.color();
    same(&a.background, &b.background)
        && same(&a.text, &b.text)
        && same(&a.status_bg, &b.status_bg)
        && same(&a.status_fg, &b.status_fg)
        && same(&a.status_accent, &b.status_accent)
        && same(&a.incoming_nick, &b.incoming_nick)
        && same(&a.outgoing_nick, &b.outgoing_nick)
        && same(&a.timestamp, &b.timestamp)
        && same(&a.system, &b.system)
        && same(&a.error, &b.error)
        && same(&a.input_prompt, &b.input_prompt)
        && same(&a.input_text, &b.input_text)
        && same(&a.input_bg, &b.input_bg)
        && same(&a.border, &b.border)
        && a.peer_palette.len() == b.peer_palette.len()
        && a.peer_palette
            .iter()
            .zip(&b.peer_palette)
            .all(|(left, right)| same(left, right))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Colors,
    Layout,
    Behavior,
    Keys,
}

impl Section {
    pub const ALL: &'static [Section] = &[
        Section::Appearance,
        Section::Colors,
        Section::Layout,
        Section::Behavior,
        Section::Keys,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Colors => "Colors",
            Self::Layout => "Layout",
            Self::Behavior => "Behavior",
            Self::Keys => "Keys",
        }
    }
}

/// How the settings editor should present a field.
#[derive(Debug, Clone)]
pub enum FieldKind {
    Bool,
    Int { min: i64, max: i64 },
    Text { max: usize },
    Color,
    ColorList,
    Choice(&'static [&'static str]),
    ThemePreset,
    Keybind,
    List,
}

/// Metadata for one editable setting.
#[derive(Debug, Clone)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub section: Section,
    pub kind: FieldKind,
    pub help: &'static str,
}

const STATUS_POSITIONS: &[&str] = StatusPosition::ALL;
const STATUS_ALIGNS: &[&str] = StatusAlign::ALL;

/// Every setting, in the order shown by the customizer and `config keys`.
pub const FIELDS: &[Field] = &[
    Field {
        key: "theme.preset",
        label: "Theme preset",
        section: Section::Appearance,
        kind: FieldKind::ThemePreset,
        help: "Cycle the built-in themes, then fine-tune individual colors.",
    },
    Field {
        key: "timestamps.enabled",
        label: "Timestamps",
        section: Section::Appearance,
        kind: FieldKind::Bool,
        help: "Show a timestamp before each scrollback line.",
    },
    Field {
        key: "timestamps.format",
        label: "Timestamp format",
        section: Section::Appearance,
        kind: FieldKind::Text { max: 64 },
        help: "A strftime format such as %H:%M:%S or %d %b %H:%M.",
    },
    Field {
        key: "timestamps.utc",
        label: "Use UTC",
        section: Section::Appearance,
        kind: FieldKind::Bool,
        help: "Format timestamps in UTC instead of local time.",
    },
    Field {
        key: "timestamps.system",
        label: "System timestamps",
        section: Section::Appearance,
        kind: FieldKind::Bool,
        help: "Also timestamp notices such as session and connection messages.",
    },
    Field {
        key: "prompt.template",
        label: "Prompt template",
        section: Section::Appearance,
        kind: FieldKind::Text { max: 64 },
        help: "Supports {nick}, {room} and {device}. Spaces are preserved.",
    },
    Field {
        key: "behavior.own_nick_fallback",
        label: "Fallback nickname",
        section: Section::Appearance,
        kind: FieldKind::Text { max: 24 },
        help: "Shown before /nick has been set, and as the author of your messages.",
    },
    Field {
        key: "behavior.system_prefix",
        label: "Notice prefix",
        section: Section::Appearance,
        kind: FieldKind::Text { max: 16 },
        help: "Prefix for system lines; set it to empty for no prefix.",
    },
    Field {
        key: "theme.background",
        label: "Background",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Terminal background used by the whole client.",
    },
    Field {
        key: "theme.text",
        label: "Message text",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Normal chat message and system text.",
    },
    Field {
        key: "theme.status_bg",
        label: "Status background",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Background of the status bar and selected settings rows.",
    },
    Field {
        key: "theme.status_fg",
        label: "Status text",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Normal text in the status bar.",
    },
    Field {
        key: "theme.status_accent",
        label: "Status accent",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "App name, connection state, and other highlights.",
    },
    Field {
        key: "theme.incoming_nick",
        label: "Incoming nick",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Nickname color for incoming messages when peer colors are off.",
    },
    Field {
        key: "theme.outgoing_nick",
        label: "Outgoing nick",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Nickname color for messages you send.",
    },
    Field {
        key: "theme.timestamp",
        label: "Timestamp",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Timestamp and hint-bar color.",
    },
    Field {
        key: "theme.system",
        label: "System text",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Color of notices and status output.",
    },
    Field {
        key: "theme.error",
        label: "Error text",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Errors and a disconnected status indicator.",
    },
    Field {
        key: "theme.input_prompt",
        label: "Input prompt",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Color of the prompt before your input.",
    },
    Field {
        key: "theme.input_text",
        label: "Input text",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Color of the text you are typing.",
    },
    Field {
        key: "theme.input_bg",
        label: "Input background",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Background of the input line.",
    },
    Field {
        key: "theme.border",
        label: "Borders",
        section: Section::Colors,
        kind: FieldKind::Color,
        help: "Border color for the input box and settings panes.",
    },
    Field {
        key: "theme.peer_palette",
        label: "Peer palette",
        section: Section::Colors,
        kind: FieldKind::ColorList,
        help: "Comma-separated colors; peers get a stable color from this list.",
    },
    Field {
        key: "layout.status_position",
        label: "Status position",
        section: Section::Layout,
        kind: FieldKind::Choice(STATUS_POSITIONS),
        help: "Place the status bar at the top, bottom, or hide it.",
    },
    Field {
        key: "layout.status_separator",
        label: "Status separator",
        section: Section::Layout,
        kind: FieldKind::Text { max: 32 },
        help: "Text between status segments; spaces are preserved.",
    },
    Field {
        key: "layout.status_align",
        label: "Status alignment",
        section: Section::Layout,
        kind: FieldKind::Choice(STATUS_ALIGNS),
        help: "Align the status bar contents left, center, or right.",
    },
    Field {
        key: "layout.status_segments",
        label: "Status segments",
        section: Section::Layout,
        kind: FieldKind::List,
        help: "Comma-separated order: app, connection, server, room, peers, nick, device, sessions, clock.",
    },
    Field {
        key: "layout.clock_format",
        label: "Clock format",
        section: Section::Layout,
        kind: FieldKind::Text { max: 32 },
        help: "strftime format for the clock segment, e.g. %H:%M.",
    },
    Field {
        key: "layout.show_hints",
        label: "Key hints",
        section: Section::Layout,
        kind: FieldKind::Bool,
        help: "Show the shortcut hint line above the input.",
    },
    Field {
        key: "layout.show_scroll_indicator",
        label: "Scroll indicator",
        section: Section::Layout,
        kind: FieldKind::Bool,
        help: "Show how far up you have scrolled.",
    },
    Field {
        key: "layout.show_unread",
        label: "Unread count",
        section: Section::Layout,
        kind: FieldKind::Bool,
        help: "Count messages that arrived while you were scrolled up.",
    },
    Field {
        key: "layout.input_border",
        label: "Input border",
        section: Section::Layout,
        kind: FieldKind::Bool,
        help: "Draw a box around the input line.",
    },
    Field {
        key: "layout.scroll_step",
        label: "Mouse scroll step",
        section: Section::Layout,
        kind: FieldKind::Int { min: 1, max: 100 },
        help: "Lines moved per mouse-wheel tick.",
    },
    Field {
        key: "layout.page_step",
        label: "Page scroll step",
        section: Section::Layout,
        kind: FieldKind::Int { min: 1, max: 100 },
        help: "Lines moved by the scroll-up/down keys.",
    },
    Field {
        key: "layout.max_scrollback",
        label: "Scrollback limit",
        section: Section::Layout,
        kind: FieldKind::Int {
            min: 50,
            max: 100_000,
        },
        help: "Maximum number of lines kept in memory.",
    },
    Field {
        key: "behavior.mouse",
        label: "Mouse support",
        section: Section::Behavior,
        kind: FieldKind::Bool,
        help: "Capture the mouse wheel for scrolling (turn off to select text normally).",
    },
    Field {
        key: "behavior.auto_scroll",
        label: "Auto-scroll",
        section: Section::Behavior,
        kind: FieldKind::Bool,
        help: "Jump to new messages even while you are reading older ones.",
    },
    Field {
        key: "behavior.show_help_on_start",
        label: "Startup hint",
        section: Section::Behavior,
        kind: FieldKind::Bool,
        help: "Show the /help and /settings hint when a chat starts.",
    },
    Field {
        key: "behavior.peer_colors",
        label: "Per-peer colors",
        section: Section::Behavior,
        kind: FieldKind::Bool,
        help: "Give each peer a stable color from the peer palette.",
    },
    Field {
        key: "behavior.input_history_size",
        label: "Input history",
        section: Section::Behavior,
        kind: FieldKind::Int { min: 10, max: 10_000 },
        help: "Number of submitted lines kept for Up/Down recall.",
    },
    Field {
        key: "keys.quit",
        label: "Quit",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Exit the client and save state.",
    },
    Field {
        key: "keys.settings",
        label: "Open settings",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Open this customizer from a chat.",
    },
    Field {
        key: "keys.help",
        label: "Help",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Print the slash-command list.",
    },
    Field {
        key: "keys.scroll_up",
        label: "Scroll up",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Move the scrollback up by the page step.",
    },
    Field {
        key: "keys.scroll_down",
        label: "Scroll down",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Move the scrollback down by the page step.",
    },
    Field {
        key: "keys.complete",
        label: "Complete",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Complete slash-command names.",
    },
    Field {
        key: "keys.clear_input",
        label: "Clear input",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Erase the current input line.",
    },
    Field {
        key: "keys.delete_word",
        label: "Delete word",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Delete the word before the cursor.",
    },
    Field {
        key: "keys.history_prev",
        label: "Previous input",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Recall an older submitted line.",
    },
    Field {
        key: "keys.history_next",
        label: "Next input",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Move forward through submitted lines.",
    },
    Field {
        key: "keys.toggle_timestamps",
        label: "Toggle timestamps",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Turn timestamps on or off for this session.",
    },
    Field {
        key: "keys.cycle_theme",
        label: "Cycle theme",
        section: Section::Keys,
        kind: FieldKind::Keybind,
        help: "Switch to the next built-in theme for this session.",
    },
];

pub fn field(key: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|field| field.key == key)
}

pub fn fields_in(section: Section) -> impl Iterator<Item = &'static Field> {
    FIELDS.iter().filter(move |field| field.section == section)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "encchat-config-{name}-{}-{stamp}.json",
            std::process::id()
        ))
    }

    #[test]
    fn defaults_round_trip_through_json() {
        let config = Config::default();
        let text = serde_json::to_string_pretty(&config).unwrap();
        let parsed: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn missing_file_uses_defaults_without_warnings() {
        let loaded = load(&temp_path("missing"));
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn partial_config_uses_defaults_for_missing_fields() {
        let parsed: Config = serde_json::from_str(r#"{"timestamps":{"enabled":false}}"#).unwrap();
        assert!(!parsed.timestamps.enabled);
        assert_eq!(parsed.timestamps.format, "%H:%M:%S");
        assert_eq!(parsed.theme.name, "default");
    }

    #[test]
    fn invalid_file_falls_back_and_keeps_the_file() {
        let path = temp_path("invalid");
        fs::write(&path, "{not json").unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.config, Config::default());
        assert!(!loaded.warnings.is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn utf8_bom_is_accepted() {
        let path = temp_path("bom");
        let json = serde_json::to_string(&Config::default()).unwrap();
        fs::write(&path, format!("\u{feff}{json}")).unwrap();
        let loaded = load(&path);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.config, Config::default());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = temp_path("save");
        let mut config = Config::default();
        config.set("theme.preset", "dracula").unwrap();
        config.set("timestamps.enabled", "false").unwrap();
        config.set("prompt.template", "{nick} › ").unwrap();
        save(&path, &config).unwrap();
        let loaded = load(&path);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.config, config);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn every_field_can_be_read_and_written() {
        let mut config = Config::default();
        for field in FIELDS {
            let value = config.get(field.key).unwrap_or_else(|e| {
                panic!("get {} failed: {e}", field.key);
            });
            config
                .set(field.key, &value)
                .unwrap_or_else(|e| panic!("set {} = {value:?} failed: {e}", field.key));
        }
    }

    #[test]
    fn color_parsing_accepts_names_indices_and_hex() {
        assert_eq!(parse_color("cyan"), Some(Color::Cyan));
        assert_eq!(parse_color("lightgray"), Some(Color::Gray));
        assert_eq!(parse_color("dark-gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("17"), Some(Color::Indexed(17)));
        assert_eq!(parse_color("#89b4fa"), Some(Color::Rgb(0x89, 0xb4, 0xfa)));
        assert_eq!(parse_color("#abc"), Some(Color::Rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(parse_color("not-a-color"), None);
        assert!(ColorSpec::checked("nope").is_err());
    }

    #[test]
    fn theme_presets_are_complete_and_valid() {
        for name in Theme::PRESETS {
            let theme = Theme::preset(name).unwrap_or_else(|| panic!("missing preset {name}"));
            assert_eq!(theme.name, *name);
            for (label, color) in [
                ("background", &theme.background),
                ("text", &theme.text),
                ("status_bg", &theme.status_bg),
                ("status_fg", &theme.status_fg),
                ("status_accent", &theme.status_accent),
                ("incoming", &theme.incoming_nick),
                ("outgoing", &theme.outgoing_nick),
                ("timestamp", &theme.timestamp),
                ("system", &theme.system),
                ("error", &theme.error),
                ("prompt", &theme.input_prompt),
                ("input_text", &theme.input_text),
                ("input_bg", &theme.input_bg),
                ("border", &theme.border),
            ] {
                assert!(color.color().is_some(), "{name}: bad {label}");
            }
            assert!(!theme.peer_palette.is_empty());
            assert!(theme.peer_palette.iter().all(|c| c.color().is_some()));
        }
    }

    #[test]
    fn setting_a_preset_replaces_the_theme() {
        let mut config = Config::default();
        config.set("theme.preset", "matrix").unwrap();
        assert_eq!(config.theme.name, "matrix");
        assert_eq!(config.theme.background.as_str(), "#000000");
        assert!(config.set("theme.preset", "nope").is_err());
        assert_eq!(config.theme.name, "matrix");
    }

    #[test]
    fn changing_one_color_marks_the_theme_custom() {
        let mut config = Config::default();
        config.set("theme.incoming_nick", "#ff0000").unwrap();
        assert_eq!(config.theme.name, "custom");
        assert_eq!(config.theme.incoming_nick.as_str(), "#ff0000");
    }

    #[test]
    fn status_segments_are_ordered_and_validated() {
        let mut config = Config::default();
        config
            .set("layout.status_segments", "clock,room,peers")
            .unwrap();
        assert_eq!(
            config.layout.status_segments,
            vec![
                StatusSegment::Clock,
                StatusSegment::Room,
                StatusSegment::Peers
            ]
        );
        assert!(config.set("layout.status_segments", "room,nope").is_err());
        assert!(config.set("layout.status_segments", "").is_err());
    }

    #[test]
    fn booleans_and_numbers_are_validated() {
        let mut config = Config::default();
        assert!(config.set("layout.show_hints", "maybe").is_err());
        config.set("layout.show_hints", "off").unwrap();
        assert!(!config.layout.show_hints);
        config.set("layout.scroll_step", "7").unwrap();
        assert_eq!(config.layout.scroll_step, 7);
        assert!(config.set("layout.scroll_step", "0").is_err());
        assert!(config.set("layout.scroll_step", "101").is_err());
        assert!(config.set("layout.scroll_step", "many").is_err());
    }

    #[test]
    fn timestamp_formats_are_validated() {
        let mut config = Config::default();
        config.set("timestamps.format", "%d %b %H:%M").unwrap();
        assert_eq!(config.timestamps.format, "%d %b %H:%M");
        assert!(config.set("timestamps.format", "%Q").is_err());
        assert!(config.set("timestamps.format", "").is_err());
    }

    #[test]
    fn keybindings_are_parsed_and_checked_for_duplicates() {
        let mut config = Config::default();
        config.set("keys.quit", "ctrl+q").unwrap();
        assert!(config.keys.matches(
            KeyAction::Quit,
            &KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)
        ));
        // Plain characters would swallow typing, with or without Shift.
        assert!(config.set("keys.quit", "q").is_err());
        assert!(config.set("keys.quit", "shift+q").is_err());
        // Duplicates are rejected.
        assert!(config.set("keys.quit", "f2").is_err());
        assert!(config.set("keys.quit", "nonsense+key").is_err());
    }

    #[test]
    fn key_chords_pretty_print() {
        assert_eq!(
            KeyChord::parse("ctrl+shift+pageup").unwrap().pretty(),
            "Ctrl+Shift+PgUp"
        );
        assert_eq!(KeyChord::parse("f12").unwrap().pretty(), "F12");
        assert_eq!(KeyChord::parse("alt+enter").unwrap().pretty(), "Alt+Enter");
        let tab = KeyChord::parse("shift+tab").unwrap();
        assert_eq!(tab, KeyChord::parse("backtab").unwrap());
        assert_eq!(tab.pretty(), "Shift+Tab");
        assert!(tab.matches(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)));
    }

    #[test]
    fn sanitize_repairs_hand_edited_values() {
        let mut config = Config::default();
        config.theme.background = ColorSpec::new("not-a-color");
        config.timestamps.format = "%Q".to_string();
        config.layout.scroll_step = 0;
        config.layout.status_segments = vec![StatusSegment::App, StatusSegment::App];
        config.behavior.own_nick_fallback = "  alice  ".to_string();
        config.behavior.system_prefix = "a prefix that is far too long".to_string();
        config.prompt.template = "  ".to_string();
        config.keys.quit = "q".to_string();
        let mut warnings = Vec::new();
        config.sanitize(&mut warnings);
        assert_eq!(config.theme.background.as_str(), "reset");
        assert_eq!(config.timestamps.format, "%H:%M:%S");
        assert_eq!(config.layout.scroll_step, 1);
        assert_eq!(config.layout.status_segments, vec![StatusSegment::App]);
        assert_eq!(config.behavior.own_nick_fallback, "alice");
        assert_eq!(config.behavior.system_prefix, "a prefix that is");
        assert_eq!(config.prompt.template, "{nick} ");
        assert_eq!(config.keys.quit, "ctrl+c");
        assert!(!warnings.is_empty());
    }

    #[test]
    fn reset_key_restores_defaults() {
        let mut config = Config::default();
        config.set("theme.preset", "nord").unwrap();
        config.set("timestamps.enabled", "false").unwrap();
        config.reset_key("timestamps.enabled").unwrap();
        assert!(config.timestamps.enabled);
        config.reset_key("theme.preset").unwrap();
        assert_eq!(config.theme.name, "default");
    }
}
