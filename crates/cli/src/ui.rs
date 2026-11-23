//! ratatui rendering for the full-screen client.
//!
//! [`render`] is a pure function of [`App`]; it is tested by drawing into a
//! [`ratatui::backend::TestBackend`] and inspecting the resulting buffer, so no
//! real terminal is needed.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, ChatLine, LineKind};

/// Draw the whole UI: status bar, scrollback, input line.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_status(frame, app, chunks[0]);
    render_scrollback(frame, app, chunks[1]);
    render_input(frame, app, chunks[2]);
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let connection = if app.connected { "online" } else { "offline" };
    let text = format!(
        " encchat | {} | room {} | {} peer(s) | {} ",
        app.server,
        app.fingerprint,
        app.peers.len(),
        connection,
    );
    let style = Style::default()
        .bg(Color::Blue)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), area);
}

fn render_scrollback(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let lines: Vec<Line> = app.lines.iter().map(chat_line).collect();
    let height = area.height as usize;
    let total = lines.len();
    let max_start = total.saturating_sub(height);
    let start = max_start.saturating_sub(app.scroll as usize);
    let end = (start + height).min(total);
    let visible = if start < end {
        lines[start..end].to_vec()
    } else {
        Vec::new()
    };
    frame.render_widget(Paragraph::new(visible), area);
}

fn chat_line(line: &ChatLine) -> Line<'static> {
    let timestamp = Span::styled(
        format!("{} ", line.time),
        Style::default().fg(Color::DarkGray),
    );
    match line.kind {
        LineKind::System => Line::from(vec![
            timestamp,
            Span::styled(
                line.body.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::DIM),
            ),
        ]),
        LineKind::Chat => {
            let nick_color = if line.outgoing {
                Color::Cyan
            } else {
                Color::Green
            };
            Line::from(vec![
                timestamp,
                Span::styled(
                    format!("{} ", line.nick),
                    Style::default()
                        .fg(nick_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(line.body.clone()),
            ])
        }
    }
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let prompt = app.prompt();
    let line = Line::from(vec![
        Span::styled(
            prompt.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.input.clone()),
    ]);
    frame.render_widget(Paragraph::new(line), area);

    let x = area.x + prompt.chars().count() as u16 + app.cursor as u16;
    let x = x.min(area.x + area.width.saturating_sub(1));
    frame.set_cursor_position((x, area.y));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::PeerInfo;
    use ratatui::{backend::TestBackend, Terminal};

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
        app.set_peers(vec![PeerInfo {
            device: [7u8; 16],
            nick: Some("bob".to_string()),
            established: true,
        }]);
        let text = draw(&app, 100, 6);
        assert!(text.contains("encchat"), "buffer was:\n{text}");
        assert!(text.contains("ws://relay:8787"), "buffer was:\n{text}");
        assert!(text.contains("A1B2-C3D4-E5F6"), "buffer was:\n{text}");
        assert!(text.contains("1 peer(s)"), "buffer was:\n{text}");
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
        assert!(scrolled.contains("oldest message"), "buffer was:\n{scrolled}");
    }
}
