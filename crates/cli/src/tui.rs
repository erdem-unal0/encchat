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
    event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers},
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
    net::{self, UiEvent},
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
    } = session;

    // Connect and join before taking over the terminal, so a failure prints
    // normally instead of corrupting the screen.
    let (ws, _) = tokio_tungstenite::connect_async(&server)
        .await
        .with_context(|| format!("could not connect to {server}"))?;
    let (mut sink, mut stream) = ws.split();
    sink.send(Message::Binary(encode(&core.join_frame())?.into()))
        .await?;

    let mut app = App::new(server.clone(), fingerprint, room_hex);
    app.set_nick(core.self_nick().map(str::to_string));
    app.set_connected(true);
    app.push_system(format!("Connected to {server}."));
    if core.session_count() > 0 {
        app.push_system(format!(
            "Restored {} session(s) from local state.",
            core.session_count()
        ));
    }
    app.push_system("Type /help for the command list.");

    let mut peers: Vec<DeviceId> = Vec::new();
    let mut tui = Tui::new().context("could not start the terminal UI")?;
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
                    handle_event(&mut app, event, &mut core, &peers, &key, &state_path, &mut out)?;
                }
            }
            _ = ticker.tick() => {
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
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Char('c') | KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.should_quit = true;
            }
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
            KeyCode::Up => app.history_prev(),
            KeyCode::Down => app.history_next(),
            KeyCode::PageUp => app.scroll_up(5),
            KeyCode::PageDown => app.scroll_down(5),
            KeyCode::Tab => {
                app.complete();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.clear_input();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.insert_char(c);
            }
            _ => {}
        },
        Event::Paste(text) => app.insert_str(&text),
        _ => {}
    }
    Ok(())
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
            let who = app.nick.clone().unwrap_or_else(|| "me".to_string());
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
        }
        Command::Help => {
            for line in commands::help_lines() {
                app.push_system(line);
            }
        }
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
            app.push_system(format!("sessions:   {}", core.session_count()));
        }
        Command::Fingerprint => {
            app.push_system(format!("room fingerprint: {}", app.fingerprint));
        }
        Command::Quit => app.should_quit = true,
        Command::Unknown(name) => {
            app.push_system(format!("unknown command: /{name} (try /help)"));
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
struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            cursor::Show
        );
        let _ = self.terminal.show_cursor();
    }
}
