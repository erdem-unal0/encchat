//! State and key handling for the interactive settings customizer.
//!
//! The editor owns a *draft* copy of the config. It renders live from that
//! draft, writes it with `s`, and discards it with `Esc`. Keeping this module
//! free of ratatui makes the navigation logic unit-testable with plain key
//! events.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{self, Config, Field, FieldKind, Section, Theme};

/// A short status message shown in the editor footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

impl Notice {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

/// What happened after one key event, so the chat loop can react.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsUpdate {
    /// The draft was written to disk during this event.
    pub saved: bool,
    /// The editor asked to close.
    pub closed: bool,
}

/// Inline text editing for one field.
#[derive(Debug, Clone)]
pub struct EditState {
    pub value: String,
    pub cursor: usize,
    pub error: Option<String>,
}

impl EditState {
    fn new(value: String) -> Self {
        let cursor = value.chars().count();
        Self {
            value,
            cursor,
            error: None,
        }
    }

    fn char_to_byte(&self, index: usize) -> usize {
        self.value
            .char_indices()
            .nth(index)
            .map(|(byte, _)| byte)
            .unwrap_or(self.value.len())
    }

    fn insert_char(&mut self, c: char) {
        let at = self.char_to_byte(self.cursor);
        self.value.insert(at, c);
        self.cursor += 1;
        self.error = None;
    }

    fn insert_str(&mut self, text: &str) {
        let text = normalize_paste(text);
        let at = self.char_to_byte(self.cursor);
        self.value.insert_str(at, &text);
        self.cursor += text.chars().count();
        self.error = None;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.char_to_byte(self.cursor - 1);
        let end = self.char_to_byte(self.cursor);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
        self.error = None;
    }

    fn delete(&mut self) {
        if self.cursor >= self.value.chars().count() {
            return;
        }
        let start = self.char_to_byte(self.cursor);
        let end = self.char_to_byte(self.cursor + 1);
        self.value.replace_range(start..end, "");
        self.error = None;
    }

    fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn right(&mut self) {
        if self.cursor < self.value.chars().count() {
            self.cursor += 1;
        }
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.value.chars().count();
    }
}

fn normalize_paste(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// The live settings editor.
pub struct SettingsState {
    /// The working copy; the UI renders from this.
    pub config: Config,
    original: Config,
    pub section: Section,
    /// Index within the current section.
    pub selected: usize,
    pub editing: Option<EditState>,
    pub notice: Option<Notice>,
    pub path: PathBuf,
    /// True once the draft has been written successfully at least once.
    pub saved: bool,
    quit_armed: bool,
}

impl SettingsState {
    pub fn new(config: Config, path: PathBuf) -> Self {
        Self {
            original: config.clone(),
            config,
            section: Section::Appearance,
            selected: 0,
            editing: None,
            notice: None,
            path,
            saved: false,
            quit_armed: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.config != self.original
    }

    pub fn section_fields(&self) -> Vec<&'static Field> {
        config::fields_in(self.section).collect()
    }

    pub fn selected_field(&self) -> Option<&'static Field> {
        self.section_fields().get(self.selected).copied()
    }

    /// The current value of the selected field, ready to display.
    pub fn selected_value(&self) -> String {
        self.selected_field()
            .and_then(|field| self.config.get(field.key).ok())
            .unwrap_or_default()
    }

    /// Jump to a field by key. Returns false if the key is unknown.
    pub fn select_key(&mut self, key: &str) -> bool {
        let Some(field) = config::field(key) else {
            return false;
        };
        self.section = field.section;
        self.selected = config::fields_in(self.section)
            .position(|candidate| candidate.key == key)
            .unwrap_or(0);
        true
    }

    pub fn next_section(&mut self, delta: i64) {
        let current = Section::ALL
            .iter()
            .position(|section| *section == self.section)
            .unwrap_or(0);
        let next = cycle_index(current, Section::ALL.len(), delta);
        self.section = Section::ALL[next];
        self.selected = 0;
    }

    pub fn move_selection(&mut self, delta: i64) {
        let len = self.section_fields().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let next = (self.selected as i64 + delta).clamp(0, len as i64 - 1);
        self.selected = next as usize;
    }

    /// Toggle/cycle the selected field. `big` uses a larger step for numbers.
    pub fn adjust(&mut self, delta: i64, big: bool) {
        let Some(field) = self.selected_field() else {
            return;
        };
        let current = self.config.get(field.key).unwrap_or_default();
        let result = match &field.kind {
            FieldKind::Bool => {
                let next = if current == "true" { "false" } else { "true" };
                self.config.set(field.key, next)
            }
            FieldKind::Int { min, max } => {
                let step = if big { 10 } else { 1 };
                let value = current.parse::<i64>().unwrap_or(*min);
                let next = (value + delta.signum() * step).clamp(*min, *max);
                self.config.set(field.key, &next.to_string())
            }
            FieldKind::Choice(values) => {
                if values.is_empty() {
                    return;
                }
                let current = values
                    .iter()
                    .position(|value| *value == current)
                    .unwrap_or(0);
                let next = cycle_index(current, values.len(), delta);
                self.config.set(field.key, values[next])
            }
            FieldKind::ThemePreset => {
                let presets = Theme::PRESETS;
                let current = presets
                    .iter()
                    .position(|name| *name == current)
                    .unwrap_or(0);
                let next = cycle_index(current, presets.len(), delta);
                self.config.set("theme.preset", presets[next])
            }
            FieldKind::Color => {
                let colors = config::COLOR_CHOICES;
                let current = colors
                    .iter()
                    .position(|color| *color == current)
                    .unwrap_or(0);
                let next = cycle_index(current, colors.len(), delta);
                self.config.set(field.key, colors[next])
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.notice = Some(Notice::error(error.to_string()));
        }
    }

    pub fn activate(&mut self) {
        let Some(field) = self.selected_field() else {
            return;
        };
        match &field.kind {
            FieldKind::Bool | FieldKind::Choice(_) | FieldKind::ThemePreset => {
                self.adjust(1, false);
            }
            _ => self.begin_edit(),
        }
    }

    pub fn begin_edit(&mut self) {
        let Some(field) = self.selected_field() else {
            return;
        };
        match self.config.get(field.key) {
            Ok(value) => self.editing = Some(EditState::new(value)),
            Err(error) => self.notice = Some(Notice::error(error.to_string())),
        }
    }

    pub fn reset_selected(&mut self) {
        let Some(field) = self.selected_field() else {
            return;
        };
        match self.config.reset_key(field.key) {
            Ok(()) => {
                self.notice = Some(Notice::ok(format!("Reset {}", field.label.to_lowercase())));
            }
            Err(error) => self.notice = Some(Notice::error(error.to_string())),
        }
    }

    pub fn reset_all(&mut self) {
        self.config = Config::default();
        self.section = Section::Appearance;
        self.selected = 0;
        self.editing = None;
        self.notice = Some(Notice::ok("All settings reset; press s to save"));
    }

    pub fn save(&mut self) -> SettingsUpdate {
        match config::save(&self.path, &self.config) {
            Ok(()) => {
                self.original = self.config.clone();
                self.saved = true;
                self.quit_armed = false;
                self.notice = Some(Notice::ok(format!("Saved to {}", self.path.display())));
                SettingsUpdate {
                    saved: true,
                    closed: false,
                }
            }
            Err(error) => {
                self.notice = Some(Notice::error(format!("Could not save: {error}")));
                SettingsUpdate::default()
            }
        }
    }

    /// Handle one key press. Callers must only pass `KeyEventKind::Press`.
    pub fn handle_key(&mut self, key: KeyEvent) -> SettingsUpdate {
        if self.editing.is_some() {
            self.handle_edit_key(key);
            return SettingsUpdate::default();
        }

        self.notice = None;
        if key.code != KeyCode::Esc {
            self.quit_armed = false;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if ctrl && matches!(key.code, KeyCode::Char('s')) {
            return self.save();
        }
        if !ctrl && !alt && matches!(key.code, KeyCode::Char('s')) {
            return self.save();
        }

        match key.code {
            KeyCode::Esc => {
                if self.is_dirty() && !self.quit_armed {
                    self.quit_armed = true;
                    self.notice = Some(Notice::error(
                        "Unsaved changes — press Esc again to discard, or s to save",
                    ));
                    return SettingsUpdate::default();
                }
                return SettingsUpdate {
                    saved: false,
                    closed: true,
                };
            }
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => {
                self.selected = self.section_fields().len().saturating_sub(1);
            }
            KeyCode::Tab => self.next_section(1),
            KeyCode::BackTab => self.next_section(-1),
            KeyCode::Left => self.adjust(-1, shift),
            KeyCode::Right => self.adjust(1, shift),
            KeyCode::Enter => self.activate(),
            KeyCode::Char(' ') => {
                if matches!(
                    self.selected_field().map(|field| &field.kind),
                    Some(FieldKind::Bool)
                ) {
                    self.adjust(1, false);
                } else {
                    self.activate();
                }
            }
            KeyCode::Char('r') if !ctrl && !alt && !shift => self.reset_selected(),
            KeyCode::Char('R') if !ctrl && !alt => self.reset_all(),
            KeyCode::Char('?') if !ctrl && !alt => {
                self.notice = Some(Notice::ok(
                    "↑↓ move · ←→ change · Enter edit · Tab section · s save · r reset · R reset all · Esc close",
                ));
            }
            _ => {}
        }
        SettingsUpdate::default()
    }

    /// Paste into the inline editor, if one is open.
    pub fn handle_paste(&mut self, text: &str) {
        if let Some(edit) = self.editing.as_mut() {
            edit.insert_str(text);
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.editing = None;
            return;
        }
        if key.code == KeyCode::Enter {
            self.commit_edit();
            return;
        }

        let Some(edit) = self.editing.as_mut() else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Backspace => edit.backspace(),
            KeyCode::Delete => edit.delete(),
            KeyCode::Left => edit.left(),
            KeyCode::Right => edit.right(),
            KeyCode::Home => edit.home(),
            KeyCode::End => edit.end(),
            KeyCode::Char('u') if ctrl => {
                edit.value.clear();
                edit.cursor = 0;
                edit.error = None;
            }
            KeyCode::Char(c) if !ctrl && !alt => edit.insert_char(c),
            _ => {}
        }
    }

    fn commit_edit(&mut self) {
        let Some(field) = self.selected_field() else {
            self.editing = None;
            return;
        };
        let value = self
            .editing
            .as_ref()
            .map(|edit| edit.value.clone())
            .unwrap_or_default();
        if let FieldKind::Text { max } = &field.kind {
            if value.chars().count() > *max {
                if let Some(edit) = self.editing.as_mut() {
                    edit.error = Some(format!("{} is limited to {max} characters", field.label));
                }
                return;
            }
        }
        match self.config.set(field.key, &value) {
            Ok(()) => {
                self.editing = None;
                self.notice = Some(Notice::ok(format!(
                    "Updated {}",
                    field.label.to_lowercase()
                )));
            }
            Err(error) => {
                if let Some(edit) = self.editing.as_mut() {
                    edit.error = Some(error.to_string());
                }
            }
        }
    }
}

fn cycle_index(current: usize, len: usize, delta: i64) -> usize {
    if len == 0 {
        return 0;
    }
    if delta >= 0 {
        (current + delta as usize) % len
    } else {
        let back = ((-delta) as usize) % len;
        (current + len - back) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn state() -> SettingsState {
        SettingsState::new(Config::default(), PathBuf::from("config.json"))
    }

    fn temp_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "encchat-settings-{}-{stamp}.json",
            std::process::id()
        ))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn starts_on_the_first_field() {
        let state = state();
        assert_eq!(state.section, Section::Appearance);
        assert_eq!(
            state.selected_field().map(|field| field.key),
            Some("theme.preset")
        );
        assert!(!state.is_dirty());
    }

    #[test]
    fn moves_between_sections_and_fields() {
        let mut state = state();
        state.handle_key(key(KeyCode::Down));
        assert_eq!(
            state.selected_field().map(|field| field.key),
            Some("timestamps.enabled")
        );
        state.handle_key(key(KeyCode::Tab));
        assert_eq!(state.section, Section::Colors);
        assert_eq!(state.selected, 0);
        state.handle_key(key(KeyCode::BackTab));
        assert_eq!(state.section, Section::Appearance);
        state.handle_key(key(KeyCode::End));
        assert_eq!(
            state.selected_field().map(|field| field.key),
            Some("behavior.system_prefix")
        );
    }

    #[test]
    fn toggles_boolean_fields() {
        let mut state = state();
        assert!(state.select_key("timestamps.enabled"));
        assert!(state.config.timestamps.enabled);
        state.handle_key(key(KeyCode::Enter));
        assert!(!state.config.timestamps.enabled);
        assert!(state.is_dirty());
        state.handle_key(key(KeyCode::Char(' ')));
        assert!(state.config.timestamps.enabled);
        assert!(!state.is_dirty());
        state.handle_key(key(KeyCode::Char(' ')));
        assert!(!state.config.timestamps.enabled);
        assert!(state.is_dirty());
    }

    #[test]
    fn cycles_theme_presets() {
        let mut state = state();
        assert!(state.select_key("theme.preset"));
        assert_eq!(state.config.theme.name, "default");
        state.handle_key(key(KeyCode::Right));
        assert_eq!(state.config.theme.name, "midnight");
        state.handle_key(key(KeyCode::Left));
        assert_eq!(state.config.theme.name, "default");
    }

    #[test]
    fn edits_text_fields() {
        let mut state = state();
        assert!(state.select_key("timestamps.format"));
        state.handle_key(key(KeyCode::Enter));
        assert!(state.editing.is_some());
        for _ in 0..state.editing.as_ref().unwrap().value.chars().count() {
            state.handle_key(key(KeyCode::Backspace));
        }
        for c in "%d %b %H:%M".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }
        state.handle_key(key(KeyCode::Enter));
        assert!(state.editing.is_none());
        assert_eq!(state.config.timestamps.format, "%d %b %H:%M");
    }

    #[test]
    fn invalid_values_stay_in_the_editor_with_an_error() {
        let mut state = state();
        assert!(state.select_key("theme.background"));
        state.handle_key(key(KeyCode::Enter));
        for _ in 0..state.editing.as_ref().unwrap().value.chars().count() {
            state.handle_key(key(KeyCode::Backspace));
        }
        for c in "definitely-not-a-color".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }
        state.handle_key(key(KeyCode::Enter));
        assert!(state.editing.is_some());
        assert!(state.editing.as_ref().unwrap().error.is_some());
        state.handle_key(key(KeyCode::Esc));
        assert!(state.editing.is_none());
        assert_eq!(state.config.theme.background.as_str(), "reset");
    }

    #[test]
    fn save_writes_the_draft_and_clears_dirty() {
        let path = temp_path();
        let mut state = SettingsState::new(Config::default(), path.clone());
        assert!(state.select_key("timestamps.enabled"));
        state.handle_key(key(KeyCode::Enter));
        assert!(state.is_dirty());
        let update = state.handle_key(key(KeyCode::Char('s')));
        assert!(update.saved);
        assert!(!update.closed);
        assert!(!state.is_dirty());
        assert!(state.saved);
        let loaded = config::load(&path);
        assert!(loaded.warnings.is_empty());
        assert!(!loaded.config.timestamps.enabled);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn escape_asks_before_discarding_changes() {
        let mut state = state();
        state.handle_key(key(KeyCode::Down));
        state.handle_key(key(KeyCode::Enter));
        assert!(state.is_dirty());
        let first = state.handle_key(key(KeyCode::Esc));
        assert!(!first.closed);
        assert!(state.notice.as_ref().unwrap().text.contains("Unsaved"));
        let second = state.handle_key(key(KeyCode::Esc));
        assert!(second.closed);
    }

    #[test]
    fn clean_escape_closes_immediately() {
        let mut state = state();
        let update = state.handle_key(key(KeyCode::Esc));
        assert!(update.closed);
    }

    #[test]
    fn reset_all_and_selected() {
        let mut state = state();
        state.handle_key(key(KeyCode::Right)); // change theme
        state.handle_key(key(KeyCode::Char('R')));
        assert_eq!(state.config, Config::default());
        assert!(state.select_key("timestamps.enabled"));
        state.handle_key(key(KeyCode::Enter));
        assert!(!state.config.timestamps.enabled);
        state.handle_key(key(KeyCode::Char('r')));
        assert!(state.config.timestamps.enabled);
    }

    #[test]
    fn pasted_text_is_flattened_into_the_editor() {
        let mut state = state();
        assert!(state.select_key("prompt.template"));
        state.handle_key(key(KeyCode::Enter));
        state.handle_key(ctrl('u'));
        state.handle_paste("a\r\nb\nc");
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.config.prompt.template, "a b c");
    }

    #[test]
    fn ctrl_u_clears_the_editor() {
        let mut state = state();
        assert!(state.select_key("behavior.own_nick_fallback"));
        state.handle_key(key(KeyCode::Enter));
        state.handle_key(ctrl('u'));
        assert_eq!(state.editing.as_ref().unwrap().value, "");
        state.handle_key(key(KeyCode::Esc));
    }
}
