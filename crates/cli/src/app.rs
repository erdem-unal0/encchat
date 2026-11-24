//! Pure UI state for the full-screen client.
//!
//! This module knows nothing about ratatui or the network: it is the model the
//! renderer reads and the event loop mutates, which makes it easy to unit test.

use std::path::PathBuf;

use chrono::{Local, Utc};
use encchat_core::DeviceId;

use crate::{
    commands,
    config::{Config, Theme},
    settings::SettingsState,
    state,
};

/// Whether a scrollback line is a chat message or a system/notice line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Chat,
    System,
}

/// One row in the scrollback.
#[derive(Debug, Clone)]
pub struct ChatLine {
    pub time: String,
    pub kind: LineKind,
    pub nick: String,
    pub body: String,
    /// True for messages we sent, so the renderer can colour them differently.
    pub outgoing: bool,
    /// Sender device, when known. Used for stable per-peer colors.
    pub device: Option<DeviceId>,
}

impl ChatLine {
    fn new(
        time: String,
        kind: LineKind,
        nick: String,
        body: String,
        outgoing: bool,
        device: Option<DeviceId>,
    ) -> Self {
        Self {
            time,
            kind,
            nick,
            body,
            outgoing,
            device,
        }
    }
}

/// What we know about one peer device.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub device: DeviceId,
    pub nick: Option<String>,
    /// True once a secure session (handshake) has been established.
    pub established: bool,
}

/// All mutable state behind the UI.
pub struct App {
    pub server: String,
    pub fingerprint: String,
    pub room_hex: String,
    pub device_short: String,
    pub config: Config,
    pub config_path: PathBuf,
    pub settings: Option<SettingsState>,
    pub connected: bool,
    pub session_count: usize,
    pub nick: Option<String>,
    pub peers: Vec<PeerInfo>,
    pub lines: Vec<ChatLine>,
    pub input: String,
    /// Cursor position as a char index into `input`.
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    /// Scrollback offset from the bottom, in logical lines.
    pub scroll: u16,
    /// Messages that arrived while scrolled up.
    pub unread: usize,
    pub should_quit: bool,
    /// Input saved while browsing history.
    draft: String,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        server: String,
        fingerprint: String,
        room_hex: String,
        device_short: String,
        config: Config,
        config_path: PathBuf,
    ) -> Self {
        Self {
            server,
            fingerprint,
            room_hex,
            device_short,
            config,
            config_path,
            settings: None,
            connected: false,
            session_count: 0,
            nick: None,
            peers: Vec::new(),
            lines: Vec::new(),
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            scroll: 0,
            unread: 0,
            should_quit: false,
            draft: String::new(),
        }
    }

    pub fn set_nick(&mut self, nick: Option<String>) {
        self.nick = nick;
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }

    pub fn set_peers(&mut self, peers: Vec<PeerInfo>) {
        self.peers = peers;
    }

    /// The input prompt, e.g. `alice ` or `me `.
    pub fn prompt(&self) -> String {
        let nick = self
            .nick
            .as_deref()
            .unwrap_or(&self.config.behavior.own_nick_fallback);
        self.config
            .prompt
            .template
            .replace("{nick}", nick)
            .replace("{room}", &self.fingerprint)
            .replace("{device}", &self.device_short)
    }

    fn stamp(&self) -> String {
        if !self.config.timestamps.enabled {
            return String::new();
        }
        if self.config.timestamps.utc {
            Utc::now()
                .format(&self.config.timestamps.format)
                .to_string()
        } else {
            Local::now()
                .format(&self.config.timestamps.format)
                .to_string()
        }
    }

    fn trim_scrollback(&mut self) {
        let max = self.config.layout.max_scrollback.max(1);
        if self.lines.len() > max {
            let excess = self.lines.len() - max;
            self.lines.drain(..excess);
        }
    }

    /// Append a chat line with an explicit display nickname.
    #[cfg(test)]
    pub fn push_chat(&mut self, nick: &str, body: &str, outgoing: bool) {
        self.lines.push(ChatLine::new(
            self.stamp(),
            LineKind::Chat,
            nick.to_string(),
            body.to_string(),
            outgoing,
            None,
        ));
        self.trim_scrollback();
    }

    /// Append a system/notice line. Does not yank the viewport; commands that
    /// produce immediate output call [`App::scroll_to_bottom`] themselves.
    pub fn push_system(&mut self, text: impl Into<String>) {
        self.lines.push(ChatLine::new(
            self.stamp(),
            LineKind::System,
            String::new(),
            text.into(),
            false,
            None,
        ));
        self.trim_scrollback();
    }

    /// Append an incoming message, resolving the sender's display name.
    pub fn push_incoming(&mut self, from: DeviceId, body: &str) {
        let nick = self.nick_for(&from);
        self.lines.push(ChatLine::new(
            self.stamp(),
            LineKind::Chat,
            nick,
            body.to_string(),
            false,
            Some(from),
        ));
        self.trim_scrollback();
        if self.scroll > 0 {
            if self.config.behavior.auto_scroll {
                self.scroll_to_bottom();
            } else {
                self.unread = self.unread.saturating_add(1);
            }
        }
    }

    /// Append one of our own messages (nick if set, otherwise the fallback).
    pub fn push_outgoing(&mut self, body: &str) {
        let nick = self
            .nick
            .clone()
            .unwrap_or_else(|| self.config.behavior.own_nick_fallback.clone());
        self.lines.push(ChatLine::new(
            self.stamp(),
            LineKind::Chat,
            nick,
            body.to_string(),
            true,
            None,
        ));
        self.trim_scrollback();
        self.scroll_to_bottom();
    }

    /// Display name for a peer: its advertised nick, else the short device id.
    pub fn nick_for(&self, device: &DeviceId) -> String {
        self.peers
            .iter()
            .find(|peer| &peer.device == device)
            .and_then(|peer| peer.nick.clone())
            .unwrap_or_else(|| state::short(device))
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
        self.unread = 0;
    }

    // -- settings ---------------------------------------------------------

    pub fn open_settings(&mut self) {
        if self.settings.is_none() {
            self.settings = Some(SettingsState::new(
                self.config.clone(),
                self.config_path.clone(),
            ));
        }
    }

    /// Open the editor focused on one setting, e.g. `/settings theme.preset`.
    pub fn open_settings_at(&mut self, key: &str) -> bool {
        self.open_settings();
        self.settings
            .as_mut()
            .map(|settings| settings.select_key(key))
            .unwrap_or(false)
    }

    /// Toggle timestamps for this session and return the new state.
    pub fn toggle_timestamps(&mut self) -> bool {
        self.config.timestamps.enabled = !self.config.timestamps.enabled;
        self.config.timestamps.enabled
    }

    /// Switch to the next built-in theme for this session.
    pub fn cycle_theme(&mut self) -> String {
        let current = Theme::PRESETS
            .iter()
            .position(|name| *name == self.config.theme.name)
            .unwrap_or(usize::MAX);
        let next = if current == usize::MAX {
            0
        } else {
            (current + 1) % Theme::PRESETS.len()
        };
        let name = Theme::PRESETS[next].to_string();
        let _ = self.config.set("theme.preset", &name);
        self.config.theme.name.clone()
    }

    // -- input editing ----------------------------------------------------

    fn char_to_byte(&self, index: usize) -> usize {
        self.input
            .char_indices()
            .nth(index)
            .map(|(byte, _)| byte)
            .unwrap_or(self.input.len())
    }

    pub fn insert_char(&mut self, c: char) {
        let at = self.char_to_byte(self.cursor);
        self.input.insert(at, c);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, text: &str) {
        let text = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
        let at = self.char_to_byte(self.cursor);
        self.input.insert_str(at, &text);
        self.cursor += text.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.char_to_byte(self.cursor - 1);
        let end = self.char_to_byte(self.cursor);
        self.input.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.input.chars().count() {
            return;
        }
        let start = self.char_to_byte(self.cursor);
        let end = self.char_to_byte(self.cursor + 1);
        self.input.replace_range(start..end, "");
    }

    /// Delete from the cursor back to the start of the previous word.
    pub fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.input.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let start_byte = self.char_to_byte(start);
        let end_byte = self.char_to_byte(self.cursor);
        self.input.replace_range(start_byte..end_byte, "");
        self.cursor = start;
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.input.chars().count() {
            self.cursor += 1;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.input.chars().count();
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.history_pos = None;
    }

    /// Take the current input, recording it in history, and reset the line.
    pub fn take_input(&mut self) -> String {
        let text = std::mem::take(&mut self.input);
        self.cursor = 0;
        self.history_pos = None;
        self.draft.clear();
        let trimmed = text.trim();
        if !trimmed.is_empty() && self.history.last().map(String::as_str) != Some(trimmed) {
            self.history.push(trimmed.to_string());
        }
        let max = self.config.behavior.input_history_size.max(1);
        if self.history.len() > max {
            let excess = self.history.len() - max;
            self.history.drain(..excess);
        }
        trimmed.to_string()
    }

    /// Recall the previous input (Up).
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = match self.history_pos {
            None => {
                self.draft = self.input.clone();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_pos = Some(index);
        self.input = self.history[index].clone();
        self.cursor = self.input.chars().count();
    }

    /// Recall the next input (Down), or restore the draft at the end.
    pub fn history_next(&mut self) {
        match self.history_pos {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.history_pos = Some(i + 1);
                self.input = self.history[i + 1].clone();
                self.cursor = self.input.chars().count();
            }
            Some(_) => {
                self.history_pos = None;
                self.input = std::mem::take(&mut self.draft);
                self.cursor = self.input.chars().count();
            }
        }
    }

    // -- scrollback -------------------------------------------------------

    pub fn scroll_up(&mut self, lines: u16) {
        self.scroll = self.scroll.saturating_add(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.scroll = self.scroll.saturating_sub(lines);
        if self.scroll == 0 {
            self.unread = 0;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
        self.unread = 0;
    }

    // -- tab completion ---------------------------------------------------

    /// Cycle through command names when the input starts with `/`.
    pub fn complete(&mut self) -> bool {
        if !self.input.starts_with('/') {
            return false;
        }
        let matches = commands::matching(&self.input);
        if matches.is_empty() {
            return false;
        }
        let next = matches
            .iter()
            .position(|name| *name == self.input.as_str())
            .map(|i| (i + 1) % matches.len())
            .unwrap_or(0);
        self.input = matches[next].to_string();
        self.cursor = self.input.chars().count();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(
            "ws://127.0.0.1:8787".to_string(),
            "A1B2-C3D4".to_string(),
            "a1b2c3d4".to_string(),
            "abababab".to_string(),
            Config::default(),
            PathBuf::from("config.json"),
        )
    }

    #[test]
    fn pushes_messages_and_system_lines() {
        let mut app = app();
        app.push_chat("alice", "hello", false);
        app.push_system("welcome");
        assert_eq!(app.lines.len(), 2);
        assert_eq!(app.lines[0].kind, LineKind::Chat);
        assert_eq!(app.lines[0].nick, "alice");
        assert_eq!(app.lines[0].body, "hello");
        assert!(!app.lines[0].outgoing);
        assert_eq!(app.lines[1].kind, LineKind::System);
        assert_eq!(app.lines[1].body, "welcome");
    }

    #[test]
    fn outgoing_uses_nick_then_fallback() {
        let mut app = app();
        app.push_outgoing("hi");
        assert_eq!(app.lines[0].nick, "me");
        assert!(app.lines[0].outgoing);
        app.set_nick(Some("bob".to_string()));
        app.push_outgoing("again");
        assert_eq!(app.lines[1].nick, "bob");
        app.config.behavior.own_nick_fallback = "anon".to_string();
        app.set_nick(None);
        app.push_outgoing("third");
        assert_eq!(app.lines[2].nick, "anon");
    }

    #[test]
    fn incoming_falls_back_to_short_device_id() {
        let mut app = app();
        let device = [0xab; 16];
        app.push_incoming(device, "hi there");
        assert_eq!(app.lines[0].nick, "abababab");
        // Once a nick is known, it is used instead.
        app.set_peers(vec![PeerInfo {
            device,
            nick: Some("carol".to_string()),
            established: true,
        }]);
        assert_eq!(app.nick_for(&device), "carol");
    }

    #[test]
    fn prompt_uses_the_configured_template() {
        let mut app = app();
        app.set_nick(Some("alice".to_string()));
        app.config.prompt.template = "{nick}@{room}> ".to_string();
        assert_eq!(app.prompt(), "alice@A1B2-C3D4> ");
        app.set_nick(None);
        assert_eq!(app.prompt(), "me@A1B2-C3D4> ");
    }

    #[test]
    fn edits_the_input_line() {
        let mut app = app();
        for c in "helo".chars() {
            app.insert_char(c);
        }
        assert_eq!(app.input, "helo");
        app.move_left();
        app.move_left();
        app.insert_char('l');
        assert_eq!(app.input, "hello");
        app.backspace();
        assert_eq!(app.input, "helo");
        app.end();
        app.delete();
        assert_eq!(app.input, "helo");
        app.home();
        app.insert_char('X');
        assert_eq!(app.input, "Xhelo");
        assert_eq!(app.cursor, 1);
        app.clear_input();
        assert_eq!(app.input, "");
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn deletes_words_and_flattens_pasted_newlines() {
        let mut app = app();
        app.insert_str("hello big\r\nworld");
        assert_eq!(app.input, "hello big world");
        app.end();
        app.delete_word();
        assert_eq!(app.input, "hello big ");
        app.delete_word();
        assert_eq!(app.input, "hello ");
        app.delete_word();
        assert_eq!(app.input, "");
    }

    #[test]
    fn recalls_input_history() {
        let mut app = app();
        app.insert_str("first");
        assert_eq!(app.take_input(), "first");
        app.insert_str("second");
        assert_eq!(app.take_input(), "second");
        assert_eq!(app.history, vec!["first", "second"]);

        app.history_prev();
        assert_eq!(app.input, "second");
        app.history_prev();
        assert_eq!(app.input, "first");
        app.history_prev();
        assert_eq!(app.input, "first");
        app.history_next();
        assert_eq!(app.input, "second");
        app.history_next();
        assert_eq!(app.input, "");
    }

    #[test]
    fn history_is_bounded() {
        let mut app = app();
        app.config.behavior.input_history_size = 10;
        for i in 0..20 {
            app.insert_str(&format!("line {i}"));
            app.take_input();
        }
        assert_eq!(app.history.len(), 10);
        assert_eq!(app.history.first().unwrap(), "line 10");
    }

    #[test]
    fn completes_commands() {
        let mut app = app();
        app.insert_str("/ni");
        assert!(app.complete());
        assert_eq!(app.input, "/nick");
        // Cycles back around the matches for this prefix.
        assert!(app.complete());
        assert_eq!(app.input, "/nick");
        app.clear_input();
        app.insert_str("/who");
        assert!(app.complete());
        assert_eq!(app.input, "/who");
        app.clear_input();
        app.insert_str("plain");
        assert!(!app.complete());
    }

    #[test]
    fn scrolling_up_marks_incoming_as_unread() {
        let mut app = app();
        for i in 0..40 {
            app.push_chat("alice", &format!("line {i}"), false);
        }
        app.scroll_up(5);
        app.push_incoming([7; 16], "new message");
        assert_eq!(app.scroll, 5);
        assert_eq!(app.unread, 1);
        app.scroll_down(5);
        assert_eq!(app.scroll, 0);
        assert_eq!(app.unread, 0);
    }

    #[test]
    fn auto_scroll_jumps_to_the_newest_message() {
        let mut app = app();
        app.config.behavior.auto_scroll = true;
        app.push_chat("alice", "old", false);
        app.scroll_up(5);
        app.push_incoming([7; 16], "new");
        assert_eq!(app.scroll, 0);
        assert_eq!(app.unread, 0);
    }

    #[test]
    fn scrollback_is_bounded() {
        let mut app = app();
        app.config.layout.max_scrollback = 50;
        for i in 0..60 {
            app.push_chat("alice", &format!("line {i}"), false);
        }
        assert_eq!(app.lines.len(), 50);
        assert_eq!(app.lines[0].body, "line 10");
    }

    #[test]
    fn toggles_timestamps_and_cycles_themes() {
        let mut app = app();
        assert!(app.config.timestamps.enabled);
        assert!(!app.toggle_timestamps());
        assert!(app.toggle_timestamps());
        assert_eq!(app.cycle_theme(), Theme::PRESETS[1]);
        assert_eq!(app.cycle_theme(), Theme::PRESETS[2]);
        app.config.theme.name = "custom".to_string();
        assert_eq!(app.cycle_theme(), "default");
    }
}
