//! Interactive CLI client.
//!
//! The transport is a WebSocket to a blind relay; the protocol state machine
//! lives in `encchat-core` and is shared verbatim with the web app. Local
//! history and ratchet state are stored encrypted under a passphrase.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use encchat_core::{
    decode, encode, store, Action, ClientCore, ClientFrame, DeviceId, PeerFrame, Psk, ServerFrame,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

pub struct ChatOptions {
    pub server: String,
    pub mnemonic: String,
    pub state_path: Option<PathBuf>,
    pub password_stdin: bool,
}

/// Frames to send and lines to print, produced by handling one server frame.
#[derive(Default)]
struct Outcome {
    sends: Vec<Vec<u8>>,
    lines: Vec<String>,
}

pub async fn run(opts: ChatOptions) -> Result<()> {
    let psk = Psk::from_mnemonic(&opts.mnemonic).context("invalid mnemonic")?;
    let fingerprint = psk.fingerprint();

    let (state_path, salt_path) = resolve_paths(opts.state_path.as_deref(), &fingerprint)?;
    let (mut core, key) = load_or_init(&psk, &state_path, &salt_path, opts.password_stdin)?;

    println!("Room fingerprint: {fingerprint}");
    println!("This device:      {}", hex(&core.device_id()));
    if core.session_count() > 0 {
        println!("Restored {} session(s) from local state.", core.session_count());
    }

    let (ws, _) = tokio_tungstenite::connect_async(&opts.server)
        .await
        .with_context(|| format!("could not connect to {}", opts.server))?;
    let (mut sink, mut stream) = ws.split();

    sink.send(Message::Binary(encode(&core.join_frame())?.into()))
        .await?;
    println!("Connected to {}. Type a message, or /quit to exit.", opts.server);

    // A blocking stdin reader feeds lines to the async loop.
    let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut buf = String::new();
        loop {
            buf.clear();
            match stdin.read_line(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if line_tx.send(buf.trim_end().to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut peers: Vec<DeviceId> = Vec::new();

    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        let frame: ServerFrame = decode(&bytes)?;
                        let outcome = handle_server_frame(frame, &mut core, &mut peers)?;
                        for line in &outcome.lines {
                            println!("{line}");
                        }
                        for bytes in outcome.sends {
                            sink.send(Message::Binary(bytes.into())).await?;
                        }
                        persist(&key, &core, &state_path)?;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        println!("Disconnected from the relay.");
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        eprintln!("connection error: {e}");
                        break;
                    }
                }
            }
            line = line_rx.recv() => {
                match line {
                    Some(text) => {
                        let text = text.trim();
                        if text.is_empty() {
                            continue;
                        }
                        if text == "/quit" || text == "/exit" {
                            break;
                        }
                        match core.send_text_to_all(&peers, text, now_ms()) {
                            Ok(frames) => {
                                for (to, frame) in frames {
                                    let payload = encode(&frame)?;
                                    let relay = encode(&ClientFrame::Relay { to, payload })?;
                                    sink.send(Message::Binary(relay.into())).await?;
                                }
                                println!("[me] {text}");
                                persist(&key, &core, &state_path)?;
                            }
                            Err(e) => println!("(cannot send yet: {e})"),
                        }
                    }
                    None => break,
                }
            }
        }
    }

    persist(&key, &core, &state_path)?;
    Ok(())
}

/// Translate one inbound server frame into outgoing frames and printed lines.
fn handle_server_frame(
    frame: ServerFrame,
    core: &mut ClientCore,
    peers: &mut Vec<DeviceId>,
) -> Result<Outcome> {
    let mut out = Outcome::default();

    match frame {
        ServerFrame::Joined { devices, .. } => {
            let me = core.device_id();
            peers.clear();
            for entry in &devices {
                if entry.id == me {
                    continue;
                }
                if core.verify_device(entry) {
                    peers.push(entry.id);
                } else {
                    out.lines
                        .push(format!("! ignored device with invalid MAC: {}", hex(&entry.id)));
                }
            }
            out.lines
                .push(format!("{} peer device(s) present.", peers.len()));

            for &peer in peers.iter() {
                if core.should_initiate(peer) {
                    let frame = core.begin_handshake(peer)?;
                    out.sends.push(relay_bytes(peer, &frame)?);
                }
            }
        }
        ServerFrame::Devices { devices } => {
            let me = core.device_id();
            for entry in &devices {
                if entry.id == me || peers.contains(&entry.id) {
                    continue;
                }
                if core.verify_device(entry) {
                    peers.push(entry.id);
                    if core.should_initiate(entry.id) {
                        let frame = core.begin_handshake(entry.id)?;
                        out.sends.push(relay_bytes(entry.id, &frame)?);
                    }
                }
            }
        }
        ServerFrame::Deliver { from, payload } => {
            let peer_frame: PeerFrame = decode(&payload)
                .with_context(|| format!("bad peer frame from {}", hex(&from)))?;
            let actions = core.handle_frame(from, peer_frame)?;
            for action in actions {
                match action {
                    Action::Send { to, frame } => out.sends.push(relay_bytes(to, &frame)?),
                    Action::Received { from, body, .. } => {
                        out.lines.push(format!("[{}] {}", short(&from), body));
                    }
                    Action::HandshakeComplete { with } => {
                        if !peers.contains(&with) {
                            peers.push(with);
                        }
                        out.lines.push(format!("• secure session with {}", short(&with)));
                    }
                    Action::Notice(message) => out.lines.push(message),
                }
            }
        }
        ServerFrame::Error { code, message } => {
            out.lines.push(format!("server error ({code}): {message}"));
        }
        ServerFrame::Pong => {}
    }

    Ok(out)
}

fn relay_bytes(to: DeviceId, frame: &PeerFrame) -> Result<Vec<u8>> {
    let payload = encode(frame)?;
    Ok(encode(&ClientFrame::Relay { to, payload })?)
}

fn resolve_paths(explicit: Option<&Path>, fingerprint: &str) -> Result<(PathBuf, PathBuf)> {
    let base = match explicit {
        Some(path) => path.to_path_buf(),
        None => {
            let dir = dirs::data_dir()
                .context("could not determine a data directory")?
                .join("encchat");
            std::fs::create_dir_all(&dir)?;
            dir.join(fingerprint.replace('-', ""))
        }
    };
    if let Some(parent) = base.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok((base.with_extension("state"), base.with_extension("salt")))
}

fn load_or_init(
    psk: &Psk,
    state_path: &Path,
    salt_path: &Path,
    password_stdin: bool,
) -> Result<(ClientCore, store::StoreKey)> {
    if state_path.exists() && salt_path.exists() {
        let salt_bytes = std::fs::read(salt_path)?;
        let salt: store::Salt = salt_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("local salt file is corrupt"))?;
        let password = read_password("Passphrase: ", password_stdin)?;
        let key = store::derive_key(&password, &salt)?;
        let blob = std::fs::read(state_path)?;
        let state = store::open_state(&key, &blob)
            .context("could not decrypt local state (wrong passphrase?)")?;
        Ok((ClientCore::from_state(psk.clone(), state), key))
    } else {
        let password = read_password("Choose a passphrase: ", password_stdin)?;
        let confirm = read_password("Confirm passphrase:  ", password_stdin)?;
        if password != confirm {
            bail!("passphrases do not match");
        }
        let salt = store::new_salt()?;
        let key = store::derive_key(&password, &salt)?;
        std::fs::write(salt_path, salt)?;
        let core = ClientCore::new(psk.clone())?;
        persist(&key, &core, state_path)?;
        Ok((core, key))
    }
}

fn read_password(prompt: &str, from_stdin: bool) -> Result<String> {
    if from_stdin {
        print!("{prompt}");
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok(line.trim_end().to_string())
    } else {
        Ok(rpassword::prompt_password(prompt)?)
    }
}

fn persist(key: &store::StoreKey, core: &ClientCore, state_path: &Path) -> Result<()> {
    let blob = store::seal_state(key, &core.state)?;
    let tmp = state_path.with_extension("tmp");
    std::fs::write(&tmp, &blob)?;
    std::fs::rename(&tmp, state_path)?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// First four bytes of a device ID, for compact display.
fn short(id: &DeviceId) -> String {
    hex(&id[..4])
}
