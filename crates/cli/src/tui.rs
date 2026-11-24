//! Full-screen, IRC-style client built on ratatui + crossterm.
//!
//! The event loop is a `tokio::select!` over the WebSocket, the crossterm
//! `EventStream`, and a periodic tick. A [`Tui`] guard restores the terminal on
//! every exit path, including panics and early returns.

use std::io::Stdout;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use encchat_core::{encode, store, ClientCore, DeviceId, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    app::{App, PeerInfo},
    chat::Session,
    commands::{self, Command},
    config::{KeyAction, Theme},
    net::{self, UiEvent},
    settings::{SettingsState, SettingsUpdate},
    state, ui,
};

pub async fn run(session: Session) -> Result<()> {
    let Session {
        server,
        mut core,
        key,
        state_path,
        fingerprint,
        room_hex,
        config,
        config_path,
        config_warnings,
    } = session;

    // Connect and join before taking over the terminal, so a failure prints
    // normally instead of corrupting the screen.
    let (ws, _) = tokio_tungstenite::connect_async(&server)
        .await
        .with_context(|| format!("could not connect to {server}"))?;
    let (mut sink, mut stream) = ws.split();
    sink.send(Message::Binary(encode(&core.join_frame())?.into()))
        .await?;

    let mut app = App::new(
        server.clone(),
        fingerprint,
        room_hex,
        state::short(&core.device_id()),
        config,
        config_path,
    );
    app.set_nick(core.self_nick().map(str::to_string));
    app.set_connected(true);
    app.session_count = core.session_count();
    for warning in config_warnings {
        app.push_system(format!("config: {warning}"));
    }
    app.push_system(format!("Connected to {server}."));
    if core.session_count() > 0 {
        app.push_system(format!(
            "Restored {} session(s) from local state.",
            core.session_count()
        ));
    }
    if app.config.behavior.show_help_on_start {
        app.push_system("Type /help for commands, /settings to customize.");
    }

    let mut peers: Vec<DeviceId> = Vec::new();
    let mut mouse = app.config.behavior.mouse;
    let mut tui = Tui::new(mouse).context("could not start the terminal UI")?;
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ws_open = true;
    let mut out: Vec<Vec<u8>> = Vec::new();

    while !app.should_quit {
        tui.terminal.draw(|frame| ui::render(frame, &app))?;

        tokio::select! {
            incoming = stream.next(), if ws_open => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        match encchat_core::decode::<ServerFrame>(&bytes) {
                            Ok(frame) => match net::handle_server_frame(frame, &mut core, &mut peers) {
                                Ok(outcome) => {
                                    apply_events(&mut app, outcome.events);
                                    out.extend(outcome.sends);
                                    let _ = state::persist(&key, &core, &state_path);
                                }
                                Err(e) => app.push_system(format!("protocol error: {e}")),
                            },
                            Err(e) => app.push_system(format!("bad server frame: {e}")),
                        }
                        sync_peers(&mut app, &core, &peers);
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        app.push_system("Disconnected from the relay.");
                        app.set_connected(false);
                        ws_open = false;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        app.push_system(format!("connection error: {e}"));
                        app.set_connected(false);
                        ws_open = false;
                    }
                }
            }
            maybe_event = events.next() => {
                if let Some(Ok(event)) = maybe_event {
                    // Quitting always wins, even while the settings editor is open.
                    if let Event::Key(key) = &event {
                        if key.kind == KeyEventKind::Press
                            && app.config.keys.matches(KeyAction::Quit, key)
                        {
                            app.should_quit = true;
                            continue;
                        }
                    }
                    if app.settings.is_some() {
                        let update = handle_settings_event(&mut app, event);
                        apply_settings_update(&mut app, update, &mut tui, &mut mouse)?;
                        continue;
                    }
                    handle_event(&mut app, event, &mut core, &peers, &key, &state_path, &mut out)?;
                }
            }
            _ = ticker.tick() => {
                app.session_count = core.session_count();
                sync_peers(&mut app, &core, &peers);
            }
        }

        for bytes in out.drain(..) {
            if ws_open {
                if let Err(e) = sink.send(Message::Binary(bytes.into())).await {
                    app.push_system(format!("send failed: {e}"));
                    app.set_connected(false);
                    ws_open = false;
                }
            }
        }
    }

    let _ = state::persist(&key, &core, &state_path);
    drop(tui);
    Ok(())
}

/// Run the customizer without a chat session, e.g. `encchat settings`.
pub async fn run_settings(mut state: SettingsState) -> Result<()> {
    let mut tui = Tui::new(false).context("could not start the settings UI")?;
    let mut events = EventStream::new();
    loop {
        tui.terminal
            .draw(|frame| ui::render_settings(frame, &state))?;
        match events.next().await {
            Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                // Ctrl+C asks before discarding unsaved changes, like Esc.
                let key = if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c'))
                {
                    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
                } else {
                    key
                };
                let update = state.handle_key(key);
                if update.closed {
                    break;
                }
            }
            Some(Ok(Event::Paste(text))) => state.handle_paste(&text),
            Some(Ok(_)) => {}
            _ => break,
        }
    }
    drop(tui);
    if state.saved {
        println!("Settings saved to {}.", state.path.display());
    } else {
        println!("Settings were not saved.");
    }
    Ok(())
}

fn handle_settings_event(app: &mut App, event: Event) -> SettingsUpdate {
    let Some(settings) = app.settings.as_mut() else {
        return SettingsUpdate::default();
    };
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => settings.handle_key(key),
        Event::Paste(text) => {
            settings.handle_paste(&text);
            SettingsUpdate::default()
        }
        _ => SettingsUpdate::default(),
    }
}

fn apply_settings_update(
    app: &mut App,
    update: SettingsUpdate,
    tui: &mut Tui,
    mouse: &mut bool,
) -> Result<()> {
    if update.saved {
        if let Some(settings) = app.settings.as_ref() {
            app.config = settings.config.clone();
        }
        if app.config.behavior.mouse != *mouse {
            *mouse = app.config.behavior.mouse;
            tui.set_mouse(*mouse)?;
        }
    }
    if update.closed {
        let (saved_ever, dirty) = app
            .settings
            .as_ref()
            .map(|state| (state.saved, state.is_dirty()))
            .unwrap_or((false, false));
        app.settings = None;
        if dirty {
            app.push_system("Settings closed; unsaved changes discarded.");
        } else if saved_ever {
            app.push_system("Settings saved.");
        } else {
            app.push_system("Settings unchanged.");
        }
        app.scroll_to_bottom();
    }
    Ok(())
}

/// Apply UI events produced by one server frame.
fn apply_events(app: &mut App, events: Vec<UiEvent>) {
    for event in events {
        match event {
            UiEvent::Incoming { from, body } => app.push_incoming(from, &body),
            UiEvent::System(text) => app.push_system(text),
        }
    }
}

/// Rebuild the peer list shown in the status bar and `/who`.
fn sync_peers(app: &mut App, core: &ClientCore, peers: &[DeviceId]) {
    app.session_count = core.session_count();
    let list = peers
        .iter()
        .map(|id| PeerInfo {
            device: *id,
            nick: core.peer_nick(*id).map(str::to_string),
            established: core.has_session(*id),
        })
        .collect();
    app.set_peers(list);
}

/// Handle one crossterm event. Any relay frames to send are appended to `out`.
fn handle_event(
    app: &mut App,
    event: Event,
    core: &mut ClientCore,
    peers: &[DeviceId],
    store_key: &store::StoreKey,
    state_path: &Path,
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            handle_key_press(app, key, core, peers, store_key, state_path, out)?;
        }
        Event::Mouse(mouse) if app.config.behavior.mouse => match mouse.kind {
            MouseEventKind::ScrollUp => app.scroll_up(app.config.layout.scroll_step),
            MouseEventKind::ScrollDown => app.scroll_down(app.config.layout.scroll_step),
            _ => {}
        },
        Event::Paste(text) => app.insert_str(&text),
        _ => {}
    }
    Ok(())
}

fn handle_key_press(
    app: &mut App,
    key: KeyEvent,
    core: &mut ClientCore,
    peers: &[DeviceId],
    store_key: &store::StoreKey,
    state_path: &Path,
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    if app.config.keys.matches(KeyAction::Settings, &key) {
        app.open_settings();
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::Help, &key) {
        show_help(app);
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::ToggleTimestamps, &key) {
        let enabled = app.toggle_timestamps();
        app.push_system(format!(
            "timestamps {} (session only; save with /settings)",
            if enabled { "on" } else { "off" }
        ));
        app.scroll_to_bottom();
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::CycleTheme, &key) {
        let theme = app.cycle_theme();
        app.push_system(format!(
            "theme: {theme} (session only; save with /settings)"
        ));
        app.scroll_to_bottom();
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::ScrollUp, &key) {
        app.scroll_up(app.config.layout.page_step);
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::ScrollDown, &key) {
        app.scroll_down(app.config.layout.page_step);
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::Complete, &key) {
        app.complete();
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::ClearInput, &key) {
        app.clear_input();
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::DeleteWord, &key) {
        app.delete_word();
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::HistoryPrev, &key) {
        app.history_prev();
        return Ok(());
    }
    if app.config.keys.matches(KeyAction::HistoryNext, &key) {
        app.history_next();
        return Ok(());
    }

    match key.code {
        KeyCode::Enter => {
            let text = app.take_input();
            if !text.is_empty() {
                run_command(app, core, peers, store_key, state_path, &text, out)?;
            }
        }
        KeyCode::Backspace => app.backspace(),
        KeyCode::Delete => app.delete(),
        KeyCode::Left => app.move_left(),
        KeyCode::Right => app.move_right(),
        KeyCode::Home => app.home(),
        KeyCode::End => app.end(),
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            app.insert_char(c);
        }
        _ => {}
    }
    Ok(())
}

fn show_help(app: &mut App) {
    for line in commands::help_lines() {
        app.push_system(line);
    }
    app.scroll_to_bottom();
}

/// Execute a parsed input line, mutating the app and queueing relay frames.
fn run_command(
    app: &mut App,
    core: &mut ClientCore,
    peers: &[DeviceId],
    key: &store::StoreKey,
    state_path: &Path,
    text: &str,
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    match commands::parse(text) {
        Command::Empty => {}
        Command::Message(body) => send_message(app, core, peers, key, state_path, &body, out)?,
        Command::Me(action) => {
            let who = app
                .nick
                .clone()
                .unwrap_or_else(|| app.config.behavior.own_nick_fallback.clone());
            let body = format!("* {who} {action}");
            send_message(app, core, peers, key, state_path, &body, out)?;
        }
        Command::Nick(name) => {
            if name.is_empty() {
                app.push_system("usage: /nick <name>");
            } else if core.set_nick(&name).is_some() {
                app.set_nick(core.self_nick().map(str::to_string));
                for (peer, frame) in core.announce_nick_to_all(peers) {
                    out.push(net::relay_bytes(peer, &frame)?);
                }
                let shown = app.nick.clone().unwrap_or_default();
                app.push_system(format!("You are now known as {shown}."));
            } else {
                app.push_system("that nickname is not usable");
            }
            app.scroll_to_bottom();
        }
        Command::Help => show_help(app),
        Command::Clear => app.clear(),
        Command::Who => {
            if app.peers.is_empty() {
                app.push_system("no peers connected yet.");
            }
            let mut lines = Vec::new();
            for peer in &app.peers {
                let nick = peer
                    .nick
                    .clone()
                    .unwrap_or_else(|| state::short(&peer.device));
                let status = if peer.established {
                    "session"
                } else {
                    "no session"
                };
                lines.push(format!("{nick}  {}  {status}", state::hex(&peer.device)));
            }
            for line in lines {
                app.push_system(line);
            }
            app.scroll_to_bottom();
        }
        Command::Status => {
            let connection = if app.connected {
                "connected"
            } else {
                "disconnected"
            };
            app.push_system(format!("connection: {connection}"));
            app.push_system(format!("server:     {}", app.server));
            app.push_system(format!("room:       {}", app.fingerprint));
            app.push_system(format!("room id:    {}", app.room_hex));
            app.push_system(format!("device:     {}", app.device_short));
            app.push_system(format!("sessions:   {}", core.session_count()));
            app.push_system(format!("theme:      {}", app.config.theme.name));
            app.push_system(format!("config:     {}", app.config_path.display()));
            app.scroll_to_bottom();
        }
        Command::Fingerprint => {
            app.push_system(format!("room fingerprint: {}", app.fingerprint));
            app.scroll_to_bottom();
        }
        Command::Quit => app.should_quit = true,
        Command::Settings(target) => {
            if target.is_empty() {
                app.open_settings();
            } else if crate::config::field(&target).is_some() {
                app.open_settings_at(&target);
            } else {
                app.push_system(format!("unknown setting {target:?}; try /settings"));
                app.scroll_to_bottom();
            }
        }
        Command::Theme(name) => {
            if name.is_empty() {
                app.push_system(format!("themes: {}", Theme::PRESETS.join(", ")));
                app.push_system(format!("current: {}", app.config.theme.name));
            } else {
                match app.config.set("theme.preset", &name) {
                    Ok(()) => app.push_system(format!(
                        "theme: {} (session only; save with /settings)",
                        app.config.theme.name
                    )),
                    Err(error) => app.push_system(format!("theme: {error}")),
                }
            }
            app.scroll_to_bottom();
        }
        Command::Timestamps => {
            let enabled = app.toggle_timestamps();
            app.push_system(format!(
                "timestamps {} (session only; save with /settings)",
                if enabled { "on" } else { "off" }
            ));
            app.scroll_to_bottom();
        }
        Command::Keys => {
            for action in KeyAction::ALL {
                app.push_system(format!(
                    "{:<24} {}",
                    action.label(),
                    app.config.keys.pretty(*action)
                ));
            }
            app.push_system("change them in /settings → Keys".to_string());
            app.scroll_to_bottom();
        }
        Command::Config => {
            app.push_system(format!("config: {}", app.config_path.display()));
            app.push_system(format!("theme:  {}", app.config.theme.name));
            app.push_system(
                "open /settings (F2) to edit graphically, or use `encchat config`.".to_string(),
            );
            app.scroll_to_bottom();
        }
        Command::Unknown(name) => {
            app.push_system(format!("unknown command: /{name} (try /help)"));
            app.scroll_to_bottom();
        }
    }
    Ok(())
}

/// Encrypt and queue a text message for every peer with a session.
fn send_message(
    app: &mut App,
    core: &mut ClientCore,
    peers: &[DeviceId],
    key: &store::StoreKey,
    state_path: &Path,
    body: &str,
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    match core.send_text_to_all(peers, body, state::now_ms()) {
        Ok(frames) => {
            for (to, frame) in frames {
                out.push(net::relay_bytes(to, &frame)?);
            }
            app.push_outgoing(body);
            let _ = state::persist(key, core, state_path);
        }
        Err(e) => app.push_system(format!("cannot send yet: {e}")),
    }
    Ok(())
}

/// Owns the terminal and restores it when dropped.
pub(crate) struct Tui {
    pub(crate) terminal: Terminal<CrosstermBackend<Stdout>>,
    mouse: bool,
}

impl Tui {
    pub(crate) fn new(mouse: bool) -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            cursor::Hide
        )?;
        if mouse {
            execute!(stdout, EnableMouseCapture)?;
        }
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal, mouse })
    }

    pub(crate) fn set_mouse(&mut self, enabled: bool) -> Result<()> {
        if self.mouse == enabled {
            return Ok(());
        }
        let mut stdout = std::io::stdout();
        if enabled {
            execute!(stdout, EnableMouseCapture)?;
        } else {
            execute!(stdout, DisableMouseCapture)?;
        }
        self.mouse = enabled;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            cursor::Show
        );
        let _ = self.terminal.show_cursor();
    }
}
