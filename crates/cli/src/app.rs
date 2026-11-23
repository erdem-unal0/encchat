//! Pure UI state for the full-screen client.
//!
//! This module knows nothing about ratatui or the network: it is the model the
//! renderer reads and the event loop mutates, which makes it easy to unit test.

use encchat_core::DeviceId;

use crate::{commands, state};

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
}

impl ChatLine {
    fn chat(time: String, nick: String, body: String, outgoing: bool) -> Self {
        Self {
            time,
            kind: LineKind::Chat,
            nick,
            body,
            outgoing,
        }
    }

    fn system(time: String, body: String) -> Self {
        Self {
            time,
            kind: LineKind::System,
            nick: String::new(),
            body,
            outgoing: false,
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
    pub connected: bool,
    pub nick: Option<String>,
    pub peers: Vec<PeerInfo>,
    pub lines: Vec<ChatLine>,
    pub input: String,
    /// Cursor position as a char index into `input`.
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    /// Scrollback offset from the bottom, in lines.
    pub scroll: u16,
    pub should_quit: bool,
    /// Input saved while browsing history.
    draft: String,
}

impl App {
    pub fn new(server: String, fingerprint: String, room_hex: String) -> Self {
        Self {
            server,
            fingerprint,
            room_hex,
            connected: false,
            nick: None,
            peers: Vec::new(),
            lines: Vec::new(),
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            scroll: 0,
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
        format!("{} ", self.nick.as_deref().unwrap_or("me"))
    }

    fn stamp() -> String {
        chrono::Local::now().format("%H:%M:%S").to_string()
    }

    /// Append a chat line with an explicit display nickname.
    pub fn push_chat(&mut self, nick: &str, body: &str, outgoing: bool) {
        self.lines.push(ChatLine::chat(
            Self::stamp(),
            nick.to_string(),
            body.to_string(),
            outgoing,
        ));
        self.scroll = 0;
    }

    /// Append a system/notice line.
    pub fn push_system(&mut self, text: impl Into<String>) {
        self.lines.push(ChatLine::system(Self::stamp(), text.into()));
        self.scroll = 0;
    }

    /// Append an incoming message, resolving the sender's display name.
    pub fn push_incoming(&mut self, from: DeviceId, body: &str) {
        let nick = self.nick_for(&from);
        self.push_chat(&nick, body, false);
    }

    /// Append one of our own messages (nick if set, otherwise `me`).
    pub fn push_outgoing(&mut self, body: &str) {
        let nick = self.nick.clone().unwrap_or_else(|| "me".to_string());
        self.push_chat(&nick, body, true);
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
        let at = self.char_to_byte(self.cursor);
        self.input.insert_str(at, text);
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
    fn outgoing_uses_nick_then_me() {
        let mut app = app();
        app.push_outgoing("hi");
        assert_eq!(app.lines[0].nick, "me");
        assert!(app.lines[0].outgoing);
        app.set_nick(Some("bob".to_string()));
        app.push_outgoing("again");
        assert_eq!(app.lines[1].nick, "bob");
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
}
