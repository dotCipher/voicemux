mod config;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use serde_json::json;
use tokio::net::TcpListener;
use tracing::info;

use crate::config::VoicemuxConfig;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Path to the voicemux YAML config.
    #[arg(short, long, default_value = "examples/voicemux.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let config = VoicemuxConfig::from_path(&args.config)
        .with_context(|| format!("failed to load config {}", args.config.display()))?;

    let addr = SocketAddr::new(config.server.host, config.server.port);
    let app = app(config);
    let listener = TcpListener::bind(addr).await?;

    info!(%addr, "voicemux listening");
    axum::serve(listener, app).await?;

    Ok(())
}

fn app(config: VoicemuxConfig) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(config)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "voicemux"
    }))
}
