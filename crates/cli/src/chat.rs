//! `encchat chat`: pick the full-screen UI or the `--plain` line client.
//!
//! Shared setup (mnemonic, passphrase, settings, state file) happens here,
//! *before* the terminal is taken over, and the result is handed to one of the
//! two clients.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result};
use encchat_core::{store, ClientCore, Psk};

use crate::{
    config::{self, Config},
    plain, state, tui,
};

/// Options for the `chat` subcommand.
pub struct ChatOptions {
    pub server: String,
    pub mnemonic: String,
    pub state_path: Option<PathBuf>,
    pub password_stdin: bool,
    pub nick: Option<String>,
    pub plain: bool,
    pub theme: Option<String>,
    pub config_path: PathBuf,
}

/// Everything a client needs once setup is done.
pub struct Session {
    pub server: String,
    pub core: ClientCore,
    pub key: store::StoreKey,
    pub state_path: PathBuf,
    pub fingerprint: String,
    pub room_hex: String,
    pub config: Config,
    pub config_path: PathBuf,
    pub config_warnings: Vec<String>,
}

pub async fn run(opts: ChatOptions) -> Result<()> {
    let psk = Psk::from_mnemonic(&opts.mnemonic).context("invalid mnemonic")?;
    let fingerprint = psk.fingerprint();
    let room_hex = state::hex(&psk.room_id());

    let loaded = config::load(&opts.config_path);
    let mut config = loaded.config;
    let mut config_warnings = loaded.warnings;
    if let Some(theme) = opts.theme.as_deref() {
        config.set("theme.preset", theme).with_context(|| {
            format!(
                "invalid --theme (try {})",
                config::Theme::PRESETS.join(", ")
            )
        })?;
    }

    let (state_path, salt_path) = state::resolve_paths(opts.state_path.as_deref(), &fingerprint)?;
    // Prompts for the passphrase here, before any raw-mode terminal setup.
    let (mut core, key) = state::load_or_init(&psk, &state_path, &salt_path, opts.password_stdin)?;

    if let Some(nick) = opts.nick.as_deref() {
        if core.set_nick(nick).is_none() {
            config_warnings.push("ignoring empty nickname".to_string());
        }
    }

    // Fall back to the line client when stdout is not a terminal (scripts, CI).
    let plain_mode = opts.plain || !std::io::stdout().is_terminal();
    let session = Session {
        server: opts.server,
        core,
        key,
        state_path,
        fingerprint,
        room_hex,
        config,
        config_path: opts.config_path,
        config_warnings,
    };

    if plain_mode {
        for warning in &session.config_warnings {
            eprintln!("warning: {warning}");
        }
        plain::run(session).await
    } else {
        tui::run(session).await
    }
}
