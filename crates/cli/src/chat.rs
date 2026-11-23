//! `encchat chat`: pick the full-screen UI or the `--plain` line client.
//!
//! Shared setup (mnemonic, passphrase, state file) happens here, *before* the
//! terminal is taken over, and the result is handed to one of the two clients.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result};
use encchat_core::{store, ClientCore, Psk};

use crate::{plain, state, tui};

/// Options for the `chat` subcommand.
pub struct ChatOptions {
    pub server: String,
    pub mnemonic: String,
    pub state_path: Option<PathBuf>,
    pub password_stdin: bool,
    pub nick: Option<String>,
    pub plain: bool,
}

/// Everything a client needs once setup is done.
pub struct Session {
    pub server: String,
    pub core: ClientCore,
    pub key: store::StoreKey,
    pub state_path: PathBuf,
    pub fingerprint: String,
    pub room_hex: String,
}

pub async fn run(opts: ChatOptions) -> Result<()> {
    let psk = Psk::from_mnemonic(&opts.mnemonic).context("invalid mnemonic")?;
    let fingerprint = psk.fingerprint();
    let room_hex = state::hex(&psk.room_id());

    let (state_path, salt_path) = state::resolve_paths(opts.state_path.as_deref(), &fingerprint)?;
    // Prompts for the passphrase here, before any raw-mode terminal setup.
    let (mut core, key) = state::load_or_init(&psk, &state_path, &salt_path, opts.password_stdin)?;

    if let Some(nick) = opts.nick.as_deref() {
        if core.set_nick(nick).is_none() {
            eprintln!("ignoring empty nickname");
        }
    }

    let session = Session {
        server: opts.server,
        core,
        key,
        state_path,
        fingerprint,
        room_hex,
    };

    // Fall back to the line client when stdout is not a terminal (scripts, CI).
    if opts.plain || !std::io::stdout().is_terminal() {
        plain::run(session).await
    } else {
        tui::run(session).await
    }
}
