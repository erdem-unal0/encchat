use std::path::PathBuf;

use clap::{Parser, Subcommand};
use encchat_core::Psk;

mod app;
mod chat;
mod commands;
mod net;
mod plain;
mod state;
mod tui;
mod ui;

/// encchat — end-to-end encrypted, self-hosted 1:1 chat.
#[derive(Debug, Parser)]
#[command(name = "encchat", version, about)]
struct Cli {
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
        #[arg(long, default_value = "ws://127.0.0.1:8787")]
        server: String,
        /// The shared 24-word mnemonic (quote it).
        #[arg(long)]
        mnemonic: String,
        /// Where to store encrypted local state. Defaults to the OS data dir.
        #[arg(long)]
        state: Option<PathBuf>,
        /// Read the passphrase from stdin instead of prompting (for scripts).
        #[arg(long)]
        password_stdin: bool,
        /// Nickname to advertise to peers.
        #[arg(long)]
        nick: Option<String>,
        /// Use the simple line-based client instead of the full-screen UI.
        #[arg(long)]
        plain: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
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
        } => {
            chat::run(chat::ChatOptions {
                server,
                mnemonic,
                state_path: state,
                password_stdin,
                nick,
                plain,
            })
            .await?;
        }
    }
    Ok(())
}
