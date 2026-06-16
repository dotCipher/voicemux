use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Modality, VoicemuxConfig};
use crate::providers::{build_provider_adapters, ProviderError};
use crate::routing::{plan_route, RouteError, RouteRequest};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SpeechRequest {
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

pub async fn proxy_speech(
    config: &VoicemuxConfig,
    request: SpeechRequest,
) -> Result<Response, OpenAiProxyError> {
    let plan = plan_route(
        config,
        RouteRequest {
            modality: Modality::Tts,
            profile: request.profile.clone(),
            model: request.model.clone(),
            voice: request.voice.clone(),
            response_format: request.response_format.clone(),
        },
    )?;

    let adapters = build_provider_adapters(config)?;
    let adapter = adapters
        .get(&plan.selected_provider)
        .ok_or_else(|| RouteError::UnknownProvider(plan.selected_provider.clone()))?;
    let endpoint = adapter.openai_audio_speech_endpoint()?;

    let upstream_request = SpeechRequest {
        input: request.input,
        model: plan.resolved_model,
        voice: plan.resolved_voice,
        response_format: plan.response_format,
        speed: request.speed,
        profile: None,
    };

    let client = Client::new();
    let mut builder = client.post(endpoint.url).json(&upstream_request);

    if let Some(authorization) = endpoint.authorization {
        builder = builder.header(header::AUTHORIZATION.as_str(), authorization);
    }

    let upstream_response = builder.send().await?;
    let status = upstream_response.status();
    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE.as_str())
        .cloned();
    let body = upstream_response.bytes().await?;

    let mut headers = HeaderMap::new();
    if let Some(content_type) = content_type {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_bytes(content_type.as_bytes())
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        );
    }

    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        headers,
        body,
    )
        .into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum OpenAiProxyError {
    #[error(transparent)]
    Route(#[from] RouteError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("upstream request failed: {0}")]
    Upstream(#[from] reqwest::Error),
}

impl OpenAiProxyError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Route(_) | Self::Provider(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            Self::Route(_) => "invalid_route_request",
            Self::Provider(_) => "provider_configuration_error",
            Self::Upstream(_) => "upstream_error",
        }
    }
}

pub fn error_response(error: OpenAiProxyError) -> (StatusCode, axum::Json<serde_json::Value>) {
    let status = error.status_code();
    let error_type = error.error_type();

    (
        status,
        axum::Json(serde_json::json!({
            "error": {
                "type": error_type,
                "message": error.to_string()
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_speech_request_without_profile() {
        let request = SpeechRequest {
            input: Value::String("hello".to_string()),
            model: Some("tts-1".to_string()),
            voice: Some("alloy".to_string()),
            response_format: Some("mp3".to_string()),
            speed: Some(1.0),
            profile: None,
        };

        let json = serde_json::to_value(request).expect("request should serialize");

        assert_eq!(json["model"], "tts-1");
        assert_eq!(json["voice"], "alloy");
        assert!(json.get("profile").is_none());
    }
}
