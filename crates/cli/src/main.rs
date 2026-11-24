use std::path::PathBuf;

use clap::{Parser, Subcommand};
use encchat_core::Psk;

use crate::{config::Config, settings::SettingsState};

mod app;
mod chat;
mod commands;
mod config;
mod net;
mod plain;
mod settings;
mod state;
mod tui;
mod ui;

/// encchat — end-to-end encrypted, self-hosted 1:1 chat.
#[derive(Debug, Parser)]
#[command(
    name = "encchat",
    version,
    about,
    after_help = "Customize the client:        encchat settings\n\
                  Change one setting:          encchat config set theme.preset dracula\n\
                  See every setting and value: encchat config keys"
)]
struct Cli {
    /// Settings file to use (defaults to the OS config directory).
    #[arg(
        long,
        global = true,
        env = "ENCCHAT_CONFIG",
        value_name = "PATH",
        help = "Settings file to use (default: OS config dir/encchat/config.json)"
    )]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate a new pre-shared key and print its 24-word mnemonic.
    Keygen,
    /// Show the room fingerprint and room ID for an existing mnemonic.
    Room {
        /// The 24-word mnemonic (quote it).
        mnemonic: String,
    },
    /// Connect to a relay and exchange messages in a full-screen UI.
    Chat {
        /// Relay WebSocket URL.
        #[arg(long, default_value = "ws://127.0.0.1:8787", env = "ENCCHAT_SERVER")]
        server: String,
        /// The shared 24-word mnemonic (quote it).
        #[arg(long, env = "ENCCHAT_MNEMONIC")]
        mnemonic: String,
        /// Where to store encrypted local state. Defaults to the OS data dir.
        #[arg(long, env = "ENCCHAT_STATE")]
        state: Option<PathBuf>,
        /// Read the passphrase from stdin instead of prompting (for scripts).
        #[arg(long)]
        password_stdin: bool,
        /// Nickname to advertise to peers.
        #[arg(long, env = "ENCCHAT_NICK")]
        nick: Option<String>,
        /// Use the simple line-based client instead of the full-screen UI.
        #[arg(long)]
        plain: bool,
        /// Use a theme preset for this session without saving it.
        #[arg(long, value_name = "NAME")]
        theme: Option<String>,
    },
    /// Open the interactive settings customizer with a live preview.
    #[command(alias = "customize")]
    Settings,
    /// Inspect or change settings without opening the full-screen UI.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Print the settings file path.
    Path,
    /// Print the effective settings as JSON.
    Show,
    /// List every setting and its current value.
    Keys,
    /// Print one setting.
    Get {
        /// Setting key, e.g. theme.preset (see `config keys`).
        key: String,
    },
    /// Validate, set, and save one setting.
    Set {
        /// Setting key, e.g. theme.preset.
        key: String,
        /// New value, e.g. dracula or #89b4fa.
        value: String,
    },
    /// Reset every setting to its built-in default.
    Reset,
    /// List the built-in themes.
    Themes,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_path = config::resolve_path(cli.config.as_deref());

    match cli.command {
        Command::Keygen => {
            let psk = Psk::generate()?;
            println!("Pre-shared key (24 words, keep this secret):");
            println!();
            println!("  {}", psk.to_mnemonic()?);
            println!();
            println!("Room fingerprint: {}", psk.fingerprint());
        }
        Command::Room { mnemonic } => {
            let psk = Psk::from_mnemonic(&mnemonic)?;
            let room_hex: String = psk.room_id().iter().map(|b| format!("{b:02x}")).collect();
            println!("Room fingerprint: {}", psk.fingerprint());
            println!("Room ID (hex):     {room_hex}");
        }
        Command::Chat {
            server,
            mnemonic,
            state,
            password_stdin,
            nick,
            plain,
            theme,
        } => {
            chat::run(chat::ChatOptions {
                server,
                mnemonic,
                state_path: state,
                password_stdin,
                nick,
                plain,
                theme,
                config_path,
            })
            .await?;
        }
        Command::Settings => {
            let loaded = config::load(&config_path);
            for warning in &loaded.warnings {
                eprintln!("warning: {warning}");
            }
            let state = SettingsState::new(loaded.config, config_path);
            tui::run_settings(state).await?;
        }
        Command::Config { action } => {
            run_config_action(action, &config_path)?;
        }
    }
    Ok(())
}

fn run_config_action(action: ConfigAction, config_path: &std::path::Path) -> anyhow::Result<()> {
    let loaded = config::load(config_path);
    for warning in &loaded.warnings {
        eprintln!("warning: {warning}");
    }
    let mut config: Config = loaded.config;

    match action {
        ConfigAction::Path => {
            println!("{}", config_path.display());
        }
        ConfigAction::Show => {
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        ConfigAction::Keys => {
            for field in config::FIELDS {
                let value = config.get(field.key)?;
                println!("{:<30} = {:<30} # {}", field.key, value, field.help);
            }
        }
        ConfigAction::Get { key } => {
            println!("{}", config.get(&key)?);
        }
        ConfigAction::Set { key, value } => {
            config.set(&key, &value)?;
            config::save(config_path, &config)?;
            println!("{} = {}", key, config.get(&key)?);
            println!("Saved to {}", config_path.display());
        }
        ConfigAction::Reset => {
            config = Config::default();
            config::save(config_path, &config)?;
            println!("Reset settings to defaults in {}", config_path.display());
        }
        ConfigAction::Themes => {
            for name in config::Theme::PRESETS {
                let marker = if *name == config.theme.name { "*" } else { " " };
                println!("{marker} {name}");
            }
        }
    }
    Ok(())
}
