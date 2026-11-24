//! ratatui rendering for the full-screen client and the settings customizer.
//!
//! [`render`] is a pure function of [`App`]; it is tested by drawing into a
//! [`ratatui::backend::TestBackend`] and inspecting the resulting buffer, so no
//! real terminal is needed.

use chrono::Local;
use encchat_core::DeviceId;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{App, ChatLine, LineKind},
    config::{ColorSpec, Config, FieldKind, KeyAction, StatusAlign, StatusPosition, StatusSegment},
    settings::SettingsState,
};

/// Draw the whole chat UI, or the settings editor when it is open.
pub fn render(frame: &mut Frame, app: &App) {
    paint_background(frame, &app.config);
    if let Some(settings) = &app.settings {
        render_settings(frame, settings);
        return;
    }

    let config = &app.config;
    let area = frame.area();
    let input_height = if config.layout.input_border { 3 } else { 1 };
    let hints_visible = config.layout.show_hints
        || (config.layout.show_scroll_indicator && app.scroll > 0)
        || (config.layout.show_unread && app.unread > 0);

    let mut constraints = Vec::new();
    let mut slots = Vec::new();
    if config.layout.status_position == StatusPosition::Top {
        constraints.push(Constraint::Length(1));
        slots.push(Slot::Status);
    }
    constraints.push(Constraint::Min(1));
    slots.push(Slot::Scrollback);
    if hints_visible {
        constraints.push(Constraint::Length(1));
        slots.push(Slot::Hints);
    }
    constraints.push(Constraint::Length(input_height));
    slots.push(Slot::Input);
    if config.layout.status_position == StatusPosition::Bottom {
        constraints.push(Constraint::Length(1));
        slots.push(Slot::Status);
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    for (slot, chunk) in slots.iter().zip(chunks.iter()) {
        match slot {
            Slot::Status => render_status(frame, app, *chunk),
            Slot::Scrollback => render_scrollback(frame, app, *chunk),
            Slot::Hints => render_hints(frame, app, *chunk),
            Slot::Input => render_input(frame, app, *chunk),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Slot {
    Status,
    Scrollback,
    Hints,
    Input,
}

fn paint_background(frame: &mut Frame, config: &Config) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let style = Style::default().bg(color(&config.theme.background));
    frame.render_widget(Block::default().style(style), area);
}

fn color(spec: &ColorSpec) -> Color {
    spec.color().unwrap_or(Color::Reset)
}

fn dim_style(config: &Config) -> Style {
    Style::default().fg(color(&config.theme.timestamp))
}

// -- chat ---------------------------------------------------------------

pub(crate) struct StatusInfo<'a> {
    pub server: &'a str,
    pub fingerprint: &'a str,
    pub peers: usize,
    pub connected: bool,
    pub nick: Option<&'a str>,
    pub device: &'a str,
    pub sessions: usize,
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let info = StatusInfo {
        server: &app.server,
        fingerprint: &app.fingerprint,
        peers: app.peers.len(),
        connected: app.connected,
        nick: app.nick.as_deref(),
        device: &app.device_short,
        sessions: app.session_count,
    };
    let line = status_line(&app.config, &info);
    let style = Style::default()
        .bg(color(&app.config.theme.status_bg))
        .fg(color(&app.config.theme.status_fg));
    frame.render_widget(Paragraph::new(line).style(style), area);
}

pub(crate) fn status_line(config: &Config, info: &StatusInfo<'_>) -> Line<'static> {
    let bg = color(&config.theme.status_bg);
    let base = Style::default().fg(color(&config.theme.status_fg)).bg(bg);
    let accent = Style::default()
        .fg(color(&config.theme.status_accent))
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let error = Style::default()
        .fg(color(&config.theme.error))
        .bg(bg)
        .add_modifier(Modifier::BOLD);

    let mut spans = Vec::new();
    for segment in &config.layout.status_segments {
        let (text, style) = match segment {
            StatusSegment::App => ("encchat".to_string(), accent),
            StatusSegment::Connection => {
                if info.connected {
                    ("● online".to_string(), accent)
                } else {
                    ("○ offline".to_string(), error)
                }
            }
            StatusSegment::Server => (info.server.to_string(), base),
            StatusSegment::Room => (format!("room {}", info.fingerprint), base),
            StatusSegment::Peers => (
                format!(
                    "{} peer{}",
                    info.peers,
                    if info.peers == 1 { "" } else { "s" }
                ),
                base,
            ),
            StatusSegment::Nick => match info.nick {
                Some(nick) => (format!("@{nick}"), base),
                None => continue,
            },
            StatusSegment::Device => (format!("dev {}", info.device), base),
            StatusSegment::Sessions => (
                format!(
                    "{} session{}",
                    info.sessions,
                    if info.sessions == 1 { "" } else { "s" }
                ),
                base,
            ),
            StatusSegment::Clock => (
                Local::now().format(&config.layout.clock_format).to_string(),
                base,
            ),
        };
        if text.is_empty() {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::styled(config.layout.status_separator.clone(), base));
        }
        spans.push(Span::styled(text, style));
    }

    if spans.is_empty() {
        spans.push(Span::styled(" ", base));
    }
    let mut line = vec![Span::styled(" ", base)];
    line.extend(spans);
    let alignment = match config.layout.status_align {
        StatusAlign::Left => Alignment::Left,
        StatusAlign::Center => Alignment::Center,
        StatusAlign::Right => Alignment::Right,
    };
    Line::from(line).alignment(alignment)
}

fn render_scrollback(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let mut rows = Vec::new();
    for line in &app.lines {
        rows.extend(chat_rows(line, &app.config, area.width));
    }
    render_rows(frame, &rows, area, app.scroll as usize);
}

/// Turn one logical chat line into one or more wrapped screen rows.
pub(crate) fn chat_rows(line: &ChatLine, config: &Config, width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut prefix: Vec<Span<'static>> = Vec::new();
    let show_time =
        config.timestamps.enabled && !(line.kind == LineKind::System && !config.timestamps.system);
    if show_time && !line.time.is_empty() {
        prefix.push(Span::styled(format!("{} ", line.time), dim_style(config)));
    }

    let (body_style, body) = match line.kind {
        LineKind::System => {
            if !config.behavior.system_prefix.is_empty() {
                prefix.push(Span::styled(
                    config.behavior.system_prefix.clone(),
                    Style::default().fg(color(&config.theme.system)),
                ));
            }
            (
                Style::default()
                    .fg(color(&config.theme.system))
                    .add_modifier(Modifier::DIM),
                line.body.clone(),
            )
        }
        LineKind::Chat => {
            let nick_color = if line.outgoing {
                color(&config.theme.outgoing_nick)
            } else {
                peer_color(config, line.device)
            };
            prefix.push(Span::styled(
                format!("{} ", line.nick),
                Style::default().fg(nick_color).add_modifier(Modifier::BOLD),
            ));
            (
                Style::default().fg(color(&config.theme.text)),
                line.body.clone(),
            )
        }
    };

    let prefix_width = prefix.iter().map(Span::width).sum::<usize>();
    let avail = width.saturating_sub(prefix_width).max(1);
    let chunks = wrap_text(&body, avail);
    let indent = " ".repeat(prefix_width.min(width.saturating_sub(1)));
    let mut rows = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.into_iter().enumerate() {
        let mut spans = Vec::new();
        if index == 0 {
            spans.extend(prefix.clone());
        } else if !indent.is_empty() {
            spans.push(Span::raw(indent.clone()));
        }
        spans.push(Span::styled(chunk, body_style));
        rows.push(Line::from(spans));
    }
    rows
}

fn peer_color(config: &Config, device: Option<DeviceId>) -> Color {
    if !config.behavior.peer_colors || config.theme.peer_palette.is_empty() {
        return color(&config.theme.incoming_nick);
    }
    let Some(device) = device else {
        return color(&config.theme.incoming_nick);
    };
    let mut hash = 0usize;
    for byte in device {
        hash = hash.wrapping_mul(31).wrapping_add(byte as usize);
    }
    let index = hash % config.theme.peer_palette.len();
    color(&config.theme.peer_palette[index])
}

fn render_rows(frame: &mut Frame, rows: &[Line<'static>], area: Rect, scroll: usize) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let height = area.height as usize;
    let total = rows.len();
    let scroll = scroll.min(total.saturating_sub(height));
    let start = total.saturating_sub(height + scroll);
    let end = (start + height).min(total);
    let visible = rows[start..end].to_vec();
    frame.render_widget(Paragraph::new(visible), area);
}

fn render_hints(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let line = hints_line(&app.config, app.scroll as usize, app.unread, area.width);
    let style = Style::default().bg(color(&app.config.theme.background));
    frame.render_widget(Paragraph::new(line).style(style), area);
}

fn hints_line(config: &Config, scroll: usize, unread: usize, width: u16) -> Line<'static> {
    let left = if config.layout.show_hints {
        format!(
            " {} help · {} settings · {} / {} scroll · {} complete · {} quit ",
            config.keys.pretty(KeyAction::Help),
            config.keys.pretty(KeyAction::Settings),
            config.keys.pretty(KeyAction::ScrollUp),
            config.keys.pretty(KeyAction::ScrollDown),
            config.keys.pretty(KeyAction::Complete),
            config.keys.pretty(KeyAction::Quit),
        )
    } else {
        String::new()
    };
    let mut right = Vec::new();
    if config.layout.show_scroll_indicator && scroll > 0 {
        right.push(format!(
            "↑ {} line{}",
            scroll,
            if scroll == 1 { "" } else { "s" }
        ));
    }
    if config.layout.show_unread && unread > 0 {
        right.push(format!("{unread} new"));
    }
    let right = if right.is_empty() {
        String::new()
    } else {
        format!("{} ", right.join(" · "))
    };
    let right_style = if unread > 0 {
        Style::default()
            .fg(color(&config.theme.status_accent))
            .add_modifier(Modifier::BOLD)
    } else {
        dim_style(config).add_modifier(Modifier::DIM)
    };
    two_sided(left, right, width, dim_style(config), right_style)
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let config = &app.config;
    let prompt = app.prompt();
    let inner_width = if config.layout.input_border {
        area.width.saturating_sub(2)
    } else {
        area.width
    };
    let (visible, cursor_x) = fit_input(&prompt, &app.input, app.cursor, inner_width as usize);
    let line = input_line(config, &prompt, &visible);
    let bg = Style::default().bg(color(&config.theme.input_bg));

    if config.layout.input_border {
        let block = Block::bordered()
            .border_style(Style::default().fg(color(&config.theme.border)))
            .style(bg);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width > 0 && inner.height > 0 {
            frame.render_widget(Paragraph::new(line).style(bg), inner);
            let x = inner.x + cursor_x.min(inner.width.saturating_sub(1));
            frame.set_cursor_position((x, inner.y));
        }
    } else {
        frame.render_widget(Paragraph::new(line).style(bg), area);
        let x = area.x + cursor_x.min(area.width.saturating_sub(1));
        frame.set_cursor_position((x, area.y));
    }
}

fn input_line(config: &Config, prompt: &str, input: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            prompt.to_string(),
            Style::default()
                .fg(color(&config.theme.input_prompt))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            input.to_string(),
            Style::default().fg(color(&config.theme.input_text)),
        ),
    ])
}

/// Keep the cursor visible by horizontally scrolling the input line.
fn fit_input(prompt: &str, input: &str, cursor: usize, width: usize) -> (String, u16) {
    if width == 0 {
        return (String::new(), 0);
    }
    let prompt_width = UnicodeWidthStr::width(prompt);
    let avail = width.saturating_sub(prompt_width).max(1);
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());

    let mut start = 0;
    while start < cursor {
        let used: usize = chars[start..cursor]
            .iter()
            .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        if used <= avail {
            break;
        }
        start += 1;
    }

    let mut visible = String::new();
    let mut used = 0;
    for &c in &chars[start..] {
        let width = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + width > avail {
            break;
        }
        visible.push(c);
        used += width;
    }
    let cursor_used: usize = chars[start..cursor]
        .iter()
        .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
        .sum();
    let cursor_x = prompt_width
        .saturating_add(cursor_used)
        .min(width.saturating_sub(1));
    (visible, cursor_x as u16)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            rows.push(String::new());
            continue;
        }
        let mut row = String::new();
        let mut row_width = 0usize;
        for word in paragraph.split_whitespace() {
            let word_width = UnicodeWidthStr::width(word);
            let needed = if row.is_empty() {
                word_width
            } else {
                row_width + 1 + word_width
            };
            if !row.is_empty() && needed > width {
                rows.push(std::mem::take(&mut row));
            }
            if row.is_empty() && word_width > width {
                let mut chunk = String::new();
                let mut chunk_width = 0usize;
                for c in word.chars() {
                    let char_width = UnicodeWidthChar::width(c).unwrap_or(0);
                    if chunk_width + char_width > width && !chunk.is_empty() {
                        rows.push(std::mem::take(&mut chunk));
                        chunk_width = 0;
                    }
                    chunk.push(c);
                    chunk_width += char_width;
                }
                row = chunk;
                row_width = chunk_width;
            } else if row.is_empty() {
                row.push_str(word);
                row_width = word_width;
            } else {
                row.push(' ');
                row.push_str(word);
                row_width = needed;
            }
        }
        rows.push(row);
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

// -- settings -----------------------------------------------------------

/// Draw the full-screen customizer with a live chat preview.
pub fn render_settings(frame: &mut Frame, state: &SettingsState) {
    paint_background(frame, &state.config);
    let area = frame.area();
    if area.width < 20 || area.height < 5 {
        return;
    }
    let config = &state.config;
    let border_style = Style::default().fg(color(&config.theme.border));
    let title_style = Style::default()
        .fg(color(&config.theme.status_accent))
        .add_modifier(Modifier::BOLD);
    let outer = Block::bordered()
        .border_style(border_style)
        .title(Line::from(vec![
            Span::styled(" encchat settings ", title_style),
            Span::styled(format!("{} ", state.path.display()), dim_style(config)),
        ]));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width < 30 || inner.height < 8 {
        let warning = Paragraph::new("Terminal too small for settings; resize the window.")
            .style(Style::default().fg(color(&config.theme.error)));
        frame.render_widget(warning, inner);
        return;
    }

    let footer_height = if state.editing.is_some() { 3 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(footer_height),
        ])
        .split(inner);

    render_settings_header(frame, state, chunks[0]);
    render_settings_tabs(frame, state, chunks[1]);
    render_settings_body(frame, state, chunks[2]);
    render_settings_footer(frame, state, chunks[3]);
}

fn render_settings_header(frame: &mut Frame, state: &SettingsState, area: Rect) {
    let config = &state.config;
    let (right, style) = if state.is_dirty() {
        (
            "● unsaved ".to_string(),
            Style::default()
                .fg(color(&config.theme.status_accent))
                .add_modifier(Modifier::BOLD),
        )
    } else if state.saved {
        (
            "● saved ".to_string(),
            Style::default().fg(color(&config.theme.status_accent)),
        )
    } else {
        ("○ not saved ".to_string(), dim_style(config))
    };
    let line = two_sided(
        " Customize the client · changes preview live ".to_string(),
        right,
        area.width,
        dim_style(config),
        style,
    );
    let bg = Style::default().bg(color(&config.theme.background));
    frame.render_widget(Paragraph::new(line).style(bg), area);
}

fn render_settings_tabs(frame: &mut Frame, state: &SettingsState, area: Rect) {
    let config = &state.config;
    let mut spans = vec![Span::raw(" ")];
    for section in crate::config::Section::ALL {
        let active = *section == state.section;
        let style = if active {
            Style::default()
                .bg(color(&config.theme.status_bg))
                .fg(color(&config.theme.status_accent))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color(&config.theme.status_fg))
        };
        spans.push(Span::styled(format!(" {} ", section.title()), style));
        spans.push(Span::raw(" "));
    }
    let bg = Style::default().bg(color(&config.theme.background));
    frame.render_widget(Paragraph::new(Line::from(spans)).style(bg), area);
}

fn render_settings_body(frame: &mut Frame, state: &SettingsState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(area);
    render_settings_fields(frame, state, chunks[0]);
    render_settings_preview(frame, state, chunks[1]);
}

fn render_settings_fields(frame: &mut Frame, state: &SettingsState, area: Rect) {
    let config = &state.config;
    let block = Block::bordered()
        .border_style(Style::default().fg(color(&config.theme.border)))
        .title(Line::from(Span::styled(
            " Options ",
            Style::default().fg(color(&config.theme.status_accent)),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let fields = state.section_fields();
    let height = inner.height as usize;
    let offset = state.selected.saturating_sub(height.saturating_sub(1));
    for (row, field) in fields.iter().enumerate().skip(offset).take(height) {
        let selected = row == state.selected;
        let value = if selected {
            state
                .editing
                .as_ref()
                .map(|edit| edit.value.clone())
                .unwrap_or_else(|| state.selected_value())
        } else {
            state.config.get(field.key).unwrap_or_default()
        };
        let value = match field.kind {
            FieldKind::Bool if value == "true" => "on".to_string(),
            FieldKind::Bool if value == "false" => "off".to_string(),
            _ => value,
        };
        let line = field_line(config, field.label, &value, selected, inner.width);
        let style = if selected {
            Style::default().bg(color(&config.theme.status_bg))
        } else {
            Style::default().bg(color(&config.theme.background))
        };
        let row_area = Rect {
            x: inner.x,
            y: inner.y + (row - offset) as u16,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(line).style(style), row_area);
    }
}

fn field_line(
    config: &Config,
    label: &str,
    value: &str,
    selected: bool,
    width: u16,
) -> Line<'static> {
    let width = width as usize;
    let marker = if selected { "▸ " } else { "  " };
    let label = format!("{marker}{label}");
    let label_width = UnicodeWidthStr::width(label.as_str());
    let value_width = width.saturating_sub(label_width + 1);
    let label = truncate_width(&label, width);
    let value = truncate_width(value, value_width);
    let label_style = if selected {
        Style::default()
            .fg(color(&config.theme.status_accent))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color(&config.theme.text))
    };
    let value_style = if selected {
        Style::default().fg(color(&config.theme.status_fg))
    } else {
        Style::default().fg(color(&config.theme.timestamp))
    };
    let label_width = UnicodeWidthStr::width(label.as_str());
    let value_width = UnicodeWidthStr::width(value.as_str());
    let pad = width.saturating_sub(label_width + value_width);
    let mut spans = vec![Span::styled(label, label_style)];
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    spans.push(Span::styled(value, value_style));
    Line::from(spans)
}

fn render_settings_preview(frame: &mut Frame, state: &SettingsState, area: Rect) {
    let config = &state.config;
    let block = Block::bordered()
        .border_style(Style::default().fg(color(&config.theme.border)))
        .title(Line::from(Span::styled(
            " Preview ",
            Style::default().fg(color(&config.theme.status_accent)),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    render_preview_content(frame, config, inner);
}

fn render_preview_content(frame: &mut Frame, config: &Config, area: Rect) {
    let input_height = if config.layout.input_border { 3 } else { 1 };
    let hints_visible = config.layout.show_hints;
    let mut constraints = Vec::new();
    let mut slots = Vec::new();
    if config.layout.status_position == StatusPosition::Top {
        constraints.push(Constraint::Length(1));
        slots.push(Slot::Status);
    }
    constraints.push(Constraint::Min(1));
    slots.push(Slot::Scrollback);
    if hints_visible {
        constraints.push(Constraint::Length(1));
        slots.push(Slot::Hints);
    }
    constraints.push(Constraint::Length(input_height));
    slots.push(Slot::Input);
    if config.layout.status_position == StatusPosition::Bottom {
        constraints.push(Constraint::Length(1));
        slots.push(Slot::Status);
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let info = StatusInfo {
        server: "ws://relay:8787",
        fingerprint: "A1B2-C3D4-E5F6",
        peers: 1,
        connected: true,
        nick: Some("alice"),
        device: "a1b2c3d4",
        sessions: 1,
    };

    for (slot, chunk) in slots.iter().zip(chunks.iter()) {
        match slot {
            Slot::Status => {
                let line = status_line(config, &info);
                let style = Style::default()
                    .bg(color(&config.theme.status_bg))
                    .fg(color(&config.theme.status_fg));
                frame.render_widget(Paragraph::new(line).style(style), *chunk);
            }
            Slot::Scrollback => {
                let mut rows = Vec::new();
                for line in sample_lines() {
                    rows.extend(chat_rows(&line, config, chunk.width));
                }
                render_rows(frame, &rows, *chunk, 0);
            }
            Slot::Hints => {
                let line = hints_line(config, 0, 0, chunk.width);
                let style = Style::default().bg(color(&config.theme.background));
                frame.render_widget(Paragraph::new(line).style(style), *chunk);
            }
            Slot::Input => {
                let prompt = config
                    .prompt
                    .template
                    .replace("{nick}", "alice")
                    .replace("{room}", "A1B2-C3D4-E5F6")
                    .replace("{device}", "a1b2c3d4");
                render_static_input(frame, config, *chunk, &prompt, "type a message…");
            }
        }
    }
}

fn sample_lines() -> Vec<ChatLine> {
    vec![
        ChatLine {
            time: "12:04:01".to_string(),
            kind: LineKind::Chat,
            nick: "bob".to_string(),
            body: "hey! the new theme looks great.".to_string(),
            outgoing: false,
            device: Some([0x42; 16]),
        },
        ChatLine {
            time: "12:04:06".to_string(),
            kind: LineKind::Chat,
            nick: "alice".to_string(),
            body: "thanks — press s to keep it.".to_string(),
            outgoing: true,
            device: None,
        },
        ChatLine {
            time: "12:04:09".to_string(),
            kind: LineKind::System,
            nick: String::new(),
            body: "secure session with bob".to_string(),
            outgoing: false,
            device: None,
        },
    ]
}

fn render_static_input(frame: &mut Frame, config: &Config, area: Rect, prompt: &str, input: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let line = input_line(config, prompt, input);
    let bg = Style::default().bg(color(&config.theme.input_bg));
    if config.layout.input_border {
        let block = Block::bordered()
            .border_style(Style::default().fg(color(&config.theme.border)))
            .style(bg);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width > 0 && inner.height > 0 {
            frame.render_widget(Paragraph::new(line).style(bg), inner);
        }
    } else {
        frame.render_widget(Paragraph::new(line).style(bg), area);
    }
}

fn render_settings_footer(frame: &mut Frame, state: &SettingsState, area: Rect) {
    let config = &state.config;
    let editing = state.editing.is_some();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(if editing { 1 } else { 0 }),
            Constraint::Length(1),
        ])
        .split(area);
    let bg = Style::default().bg(color(&config.theme.background));

    let selected = state.selected_field();
    let (help_text, help_style) = if let Some(edit) = &state.editing {
        match &edit.error {
            Some(error) => (
                format!(" {error}"),
                Style::default()
                    .fg(color(&config.theme.error))
                    .add_modifier(Modifier::BOLD),
            ),
            None => (
                format!(
                    " {} — {}",
                    selected.map(|field| field.label).unwrap_or("Value"),
                    selected.map(|field| field.help).unwrap_or("")
                ),
                dim_style(config),
            ),
        }
    } else if let Some(notice) = &state.notice {
        (
            format!(" {}", notice.text),
            if notice.error {
                Style::default()
                    .fg(color(&config.theme.error))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color(&config.theme.system))
            },
        )
    } else {
        (
            format!(
                " {} — {}",
                selected.map(|field| field.label).unwrap_or("Settings"),
                selected.map(|field| field.help).unwrap_or("")
            ),
            dim_style(config),
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(help_text, help_style))).style(bg),
        chunks[0],
    );

    if let Some(edit) = &state.editing {
        let label = selected.map(|field| field.label).unwrap_or("Value");
        let prefix = format!(" {label}: ");
        let width = chunks[1].width as usize;
        let prefix_width = UnicodeWidthStr::width(prefix.as_str());
        let avail = width.saturating_sub(prefix_width).max(1);
        let (visible, cursor) = fit_input("", &edit.value, edit.cursor, avail);
        let line = Line::from(vec![
            Span::styled(
                prefix,
                Style::default()
                    .fg(color(&config.theme.input_prompt))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                visible,
                Style::default().fg(color(&config.theme.input_text)),
            ),
        ]);
        let input_bg = Style::default().bg(color(&config.theme.input_bg));
        frame.render_widget(Paragraph::new(line).style(input_bg), chunks[1]);
        if chunks[1].width > 0 {
            let x =
                chunks[1].x + (prefix_width + cursor as usize).min(width.saturating_sub(1)) as u16;
            frame.set_cursor_position((x, chunks[1].y));
        }
    }

    let hints = " ↑↓ move · ←→ change · Enter edit · Tab section · s save · r reset · R reset all · Esc close ";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hints,
            dim_style(config).add_modifier(Modifier::DIM),
        )))
        .style(bg),
        chunks[2],
    );
}

// -- helpers ------------------------------------------------------------

fn two_sided(
    left: String,
    right: String,
    width: u16,
    left_style: Style,
    right_style: Style,
) -> Line<'static> {
    let width = width as usize;
    if width == 0 {
        return Line::default();
    }
    let mut left = left;
    let right_width = UnicodeWidthStr::width(right.as_str());
    let mut left_width = UnicodeWidthStr::width(left.as_str());
    if left_width + right_width > width {
        left = truncate_width(&left, width.saturating_sub(right_width));
        left_width = UnicodeWidthStr::width(left.as_str());
    }
    let pad = width.saturating_sub(left_width + right_width);
    let mut spans = vec![Span::styled(left, left_style)];
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    if !right.is_empty() {
        spans.push(Span::styled(right, right_style));
    }
    Line::from(spans)
}

fn truncate_width(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let width = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + width > max {
            break;
        }
        out.push(c);
        used += width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::PeerInfo;
    use crate::config::{Config, StatusSegment};
    use crate::settings::SettingsState;
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    fn draw(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        buffer_text(&terminal)
    }

    fn app() -> App {
        App::new(
            "ws://relay:8787".to_string(),
            "A1B2-C3D4-E5F6".to_string(),
            "a1b2c3d4e5f6".to_string(),
            "a1b2c3d4".to_string(),
            Config::default(),
            PathBuf::from("config.json"),
        )
    }

    #[test]
    fn renders_message_body_and_nick() {
        let mut app = app();
        app.push_chat("alice", "hello world", false);
        let text = draw(&app, 80, 10);
        assert!(text.contains("hello world"), "buffer was:\n{text}");
        assert!(text.contains("alice"), "buffer was:\n{text}");
    }

    #[test]
    fn renders_status_bar() {
        let mut app = app();
        app.set_connected(true);
        app.session_count = 2;
        app.set_peers(vec![PeerInfo {
            device: [7u8; 16],
            nick: Some("bob".to_string()),
            established: true,
        }]);
        let text = draw(&app, 120, 6);
        assert!(text.contains("encchat"), "buffer was:\n{text}");
        assert!(text.contains("ws://relay:8787"), "buffer was:\n{text}");
        assert!(text.contains("A1B2-C3D4-E5F6"), "buffer was:\n{text}");
        assert!(text.contains("1 peer"), "buffer was:\n{text}");
        assert!(text.contains("online"), "buffer was:\n{text}");
    }

    #[test]
    fn renders_system_line_and_prompt() {
        let mut app = app();
        app.set_nick(Some("carol".to_string()));
        app.push_system("connected to relay");
        app.push_incoming([0xab; 16], "hi there");
        let text = draw(&app, 80, 12);
        assert!(text.contains("connected to relay"), "buffer was:\n{text}");
        assert!(text.contains("hi there"), "buffer was:\n{text}");
        // Peer nickname falls back to the short device id.
        assert!(text.contains("abababab"), "buffer was:\n{text}");
        // The input prompt shows our nickname.
        assert!(text.contains("carol "), "buffer was:\n{text}");
    }

    #[test]
    fn scrolls_back_to_older_lines() {
        let mut app = app();
        app.push_chat("alice", "oldest message", false);
        for i in 0..20 {
            app.push_chat("alice", &format!("line {i}"), false);
        }
        // Without scrolling, the oldest line is off-screen.
        let bottom = draw(&app, 80, 6);
        assert!(!bottom.contains("oldest message"), "buffer was:\n{bottom}");
        // Scrolling up brings it back.
        app.scroll_up(40);
        let scrolled = draw(&app, 80, 6);
        assert!(
            scrolled.contains("oldest message"),
            "buffer was:\n{scrolled}"
        );
    }

    #[test]
    fn wraps_long_lines_without_losing_words() {
        let mut app = app();
        app.push_chat("alice", "alpha beta gamma delta epsilon zeta", false);
        let text = draw(&app, 30, 14);
        for word in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"] {
            assert!(text.contains(word), "missing {word} in:\n{text}");
        }
    }

    #[test]
    fn timestamps_can_be_hidden() {
        let mut app = app();
        app.lines.push(ChatLine {
            time: "12:34:56".to_string(),
            kind: LineKind::Chat,
            nick: "alice".to_string(),
            body: "hello".to_string(),
            outgoing: false,
            device: None,
        });
        let visible = draw(&app, 80, 8);
        assert!(visible.contains("12:34:56"), "buffer was:\n{visible}");
        app.config.timestamps.enabled = false;
        let hidden = draw(&app, 80, 8);
        assert!(!hidden.contains("12:34:56"), "buffer was:\n{hidden}");
    }

    #[test]
    fn status_segments_can_be_reordered_or_hidden() {
        let mut app = app();
        app.config.layout.status_segments = vec![StatusSegment::Room];
        let text = draw(&app, 100, 8);
        assert!(text.contains("A1B2-C3D4-E5F6"), "buffer was:\n{text}");
        assert!(!text.contains("encchat"), "buffer was:\n{text}");
        assert!(!text.contains("ws://relay:8787"), "buffer was:\n{text}");
    }

    #[test]
    fn status_bar_can_move_to_the_bottom_or_hide() {
        let mut app = app();
        app.config.layout.status_position = StatusPosition::Bottom;
        let text = draw(&app, 100, 8);
        assert!(text.contains("encchat"), "buffer was:\n{text}");
        app.config.layout.status_position = StatusPosition::Hidden;
        let hidden = draw(&app, 100, 8);
        assert!(!hidden.contains("encchat"), "buffer was:\n{hidden}");
    }

    #[test]
    fn renders_settings_with_a_live_preview() {
        let mut app = app();
        app.open_settings();
        let text = draw(&app, 110, 32);
        assert!(text.contains("encchat settings"), "buffer was:\n{text}");
        assert!(text.contains("Preview"), "buffer was:\n{text}");
        assert!(text.contains("Theme preset"), "buffer was:\n{text}");
        assert!(text.contains("Options"), "buffer was:\n{text}");
        assert!(
            text.contains("secure session with bob"),
            "buffer was:\n{text}"
        );
    }

    #[test]
    fn settings_preview_follows_the_draft_theme_and_layout() {
        let mut state = SettingsState::new(Config::default(), PathBuf::from("config.json"));
        state.config.set("theme.preset", "matrix").unwrap();
        state.config.layout.status_position = StatusPosition::Hidden;
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_settings(frame, &state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("Matrix") || text.contains("matrix"), "{text}");
        assert!(text.contains("Preview"), "{text}");
    }

    #[test]
    fn long_input_scrolls_to_keep_the_cursor_visible() {
        let (visible, cursor) = fit_input("alice ", "abcdefghij", 10, 13);
        assert_eq!(visible, "defghij");
        assert_eq!(cursor, 12);
    }

    #[test]
    fn wraps_wide_characters_by_display_width() {
        let rows = wrap_text("日本語 テスト", 4);
        assert!(rows.len() > 1, "rows: {rows:?}");
        for row in &rows {
            assert!(
                UnicodeWidthStr::width(row.as_str()) <= 4,
                "row {row:?} is too wide"
            );
        }
        assert_eq!(rows.concat().replace(' ', ""), "日本語テスト");
    }

    #[test]
    fn draws_at_tiny_and_large_sizes_without_panicking() {
        let mut app = app();
        app.push_system("a notice that is considerably longer than the terminal");
        app.push_chat("alice", "a message that also needs to wrap", false);
        for (width, height) in [(1, 1), (5, 2), (20, 4), (120, 40)] {
            let _ = draw(&app, width, height);
        }
        app.open_settings();
        for (width, height) in [(1, 1), (20, 5), (40, 12), (140, 45)] {
            let _ = draw(&app, width, height);
        }
    }

    #[test]
    fn resizing_the_terminal_redraws_cleanly() {
        let mut app = app();
        app.push_chat("alice", "hello after a resize", false);
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        terminal.backend_mut().resize(60, 10);
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("hello after a resize"), "buffer was:\n{text}");
        assert_eq!(terminal.backend().buffer().area.width, 60);
    }
}
