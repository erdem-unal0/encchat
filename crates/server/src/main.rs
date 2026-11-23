//! encchat relay server: parse arguments and serve [`encchat_server::build_router`].

use clap::Parser;
use encchat_server::{build_router, default_config};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "encchat-server", version, about = "Blind relay for encchat")]
struct Args {
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,
    /// How long an offline device's queued messages are kept.
    #[arg(long, default_value_t = 300)]
    mailbox_ttl_secs: u64,
    /// Maximum queued messages per offline device.
    #[arg(long, default_value_t = 512)]
    mailbox_max: usize,
    /// Maximum size of a single relayed payload, in bytes.
    #[arg(long, default_value_t = 262_144)]
    max_payload: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let app = build_router(
        default_config(args.mailbox_ttl_secs, args.mailbox_max),
        args.max_payload,
    );

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    info!(bind = %args.bind, "encchat relay listening");
    axum::serve(listener, app).await?;
    Ok(())
}
