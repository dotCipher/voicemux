mod config;
mod openai;
mod providers;
mod routing;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use serde_json::json;
use tokio::net::TcpListener;
use tower_http::timeout::TimeoutLayer;
use tracing::info;

use crate::config::VoicemuxConfig;
use crate::openai::{
    error_response, proxy_speech, proxy_transcription, SpeechRequest, TranscriptionRequest,
};
use crate::providers::provider_descriptors_from_adapters;
use crate::routing::{plan_route, RouteRequest};
use crate::state::AppState;

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
    let app = app(AppState::new(config)?);
    let listener = TcpListener::bind(addr).await?;

    info!(%addr, "voicemux listening");
    axum::serve(listener, app).await?;

    Ok(())
}

fn app(state: AppState) -> Router {
    let max_body_bytes = state.config.server.max_body_bytes;
    let request_timeout = Duration::from_secs(state.config.server.request_timeout_seconds);

    Router::new()
        .route("/health", get(health))
        .route("/v1/audio/transcriptions", post(transcription))
        .route("/v1/audio/speech", post(speech))
        .route("/v1/providers", get(list_providers))
        .route("/v1/route/dry-run", post(dry_run_route))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "voicemux"
    }))
}

async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let providers = provider_descriptors_from_adapters(&state.providers);

    Ok(Json(json!({ "data": providers })))
}

async fn speech(
    State(state): State<AppState>,
    Json(request): Json<SpeechRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    proxy_speech(&state, request).await.map_err(error_response)
}

async fn transcription(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let request = TranscriptionRequest::from_multipart(multipart)
        .await
        .map_err(error_response)?;

    proxy_transcription(&state, request)
        .await
        .map_err(error_response)
}

async fn dry_run_route(
    State(state): State<AppState>,
    Json(request): Json<RouteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    plan_route(&state.config, request)
        .map(|plan| Json(json!(plan)))
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "type": "invalid_route_request",
                        "message": error.to_string()
                    }
                })),
            )
        })
}
