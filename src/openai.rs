use axum::extract::Multipart;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Modality, VoicemuxConfig};
use crate::providers::{build_provider_adapters, ProviderError};
use crate::routing::{plan_route, RouteError, RoutePlan, RouteRequest};

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
        model: plan.resolved_model.clone(),
        voice: plan.resolved_voice.clone(),
        response_format: plan.response_format.clone(),
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
    insert_route_headers(&mut headers, &plan);

    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        headers,
        body,
    )
        .into_response())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionField {
    pub name: String,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRequest {
    pub fields: Vec<TranscriptionField>,
}

impl TranscriptionRequest {
    pub async fn from_multipart(mut multipart: Multipart) -> Result<Self, OpenAiProxyError> {
        let mut fields = Vec::new();

        while let Some(field) = multipart.next_field().await? {
            let name = field
                .name()
                .ok_or(OpenAiProxyError::MissingMultipartFieldName)?
                .to_string();
            let file_name = field.file_name().map(ToOwned::to_owned);
            let content_type = field.content_type().map(ToOwned::to_owned);
            let bytes = field.bytes().await?.to_vec();

            fields.push(TranscriptionField {
                name,
                file_name,
                content_type,
                bytes,
            });
        }

        Ok(Self { fields })
    }

    fn text_field(&self, name: &str) -> Option<String> {
        self.fields
            .iter()
            .find(|field| field.name == name && field.file_name.is_none())
            .and_then(|field| std::str::from_utf8(&field.bytes).ok())
            .map(ToOwned::to_owned)
    }

    fn into_form(self, model: Option<String>) -> Result<Form, OpenAiProxyError> {
        let mut form = Form::new();
        let mut inserted_model = false;

        for field in self.fields {
            if field.name == "profile" {
                continue;
            }

            if field.name == "model" {
                if let Some(model) = &model {
                    form = form.text("model", model.clone());
                    inserted_model = true;
                }
                continue;
            }

            form = form.part(field.name.clone(), multipart_part(field)?);
        }

        if !inserted_model {
            if let Some(model) = model {
                form = form.text("model", model);
            }
        }

        Ok(form)
    }
}

pub async fn proxy_transcription(
    config: &VoicemuxConfig,
    request: TranscriptionRequest,
) -> Result<Response, OpenAiProxyError> {
    let route_request = RouteRequest {
        modality: Modality::Stt,
        profile: request.text_field("profile"),
        model: request.text_field("model"),
        voice: None,
        response_format: request.text_field("response_format"),
    };
    let plan = plan_route(config, route_request)?;

    let adapters = build_provider_adapters(config)?;
    let adapter = adapters
        .get(&plan.selected_provider)
        .ok_or_else(|| RouteError::UnknownProvider(plan.selected_provider.clone()))?;
    let endpoint = adapter.openai_audio_transcriptions_endpoint()?;
    let form = request.into_form(plan.resolved_model.clone())?;

    let client = Client::new();
    let mut builder = client.post(endpoint.url).multipart(form);

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
                .unwrap_or_else(|_| HeaderValue::from_static("application/json")),
        );
    }
    insert_route_headers(&mut headers, &plan);

    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        headers,
        body,
    )
        .into_response())
}

fn insert_route_headers(headers: &mut HeaderMap, plan: &RoutePlan) {
    insert_header(headers, "x-voicemux-profile", &plan.profile);
    insert_header(headers, "x-voicemux-provider", &plan.selected_provider);
    insert_header(headers, "x-voicemux-route", &plan.route.join(","));

    if let Some(model) = &plan.resolved_model {
        insert_header(headers, "x-voicemux-model", model);
    }

    if let Some(voice) = &plan.resolved_voice {
        insert_header(headers, "x-voicemux-voice", voice);
    }
}

fn insert_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

fn multipart_part(field: TranscriptionField) -> Result<Part, OpenAiProxyError> {
    let mut part = if field.file_name.is_some() {
        Part::bytes(field.bytes)
    } else if let Ok(value) = String::from_utf8(field.bytes.clone()) {
        Part::text(value)
    } else {
        Part::bytes(field.bytes)
    };

    if let Some(file_name) = field.file_name {
        part = part.file_name(file_name);
    }

    if let Some(content_type) = field.content_type {
        part = part.mime_str(&content_type)?;
    }

    Ok(part)
}

#[derive(Debug, thiserror::Error)]
pub enum OpenAiProxyError {
    #[error(transparent)]
    Route(#[from] RouteError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("upstream request failed: {0}")]
    Upstream(#[from] reqwest::Error),
    #[error("multipart request failed: {0}")]
    Multipart(#[from] axum::extract::multipart::MultipartError),
    #[error("multipart field is missing a name")]
    MissingMultipartFieldName,
    #[error("invalid multipart content type: {0}")]
    MultipartContentType(#[from] reqwest::header::InvalidHeaderValue),
}

impl OpenAiProxyError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Route(_)
            | Self::Provider(_)
            | Self::Multipart(_)
            | Self::MissingMultipartFieldName
            | Self::MultipartContentType(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            Self::Route(_) => "invalid_route_request",
            Self::Provider(_) => "provider_configuration_error",
            Self::Upstream(_) => "upstream_error",
            Self::Multipart(_)
            | Self::MissingMultipartFieldName
            | Self::MultipartContentType(_) => "invalid_multipart_request",
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

    #[test]
    fn transcription_request_extracts_text_fields() {
        let request = TranscriptionRequest {
            fields: vec![
                TranscriptionField {
                    name: "model".to_string(),
                    file_name: None,
                    content_type: None,
                    bytes: b"whisper-1".to_vec(),
                },
                TranscriptionField {
                    name: "profile".to_string(),
                    file_name: None,
                    content_type: None,
                    bytes: b"local".to_vec(),
                },
            ],
        };

        assert_eq!(request.text_field("model").as_deref(), Some("whisper-1"));
        assert_eq!(request.text_field("profile").as_deref(), Some("local"));
    }

    #[test]
    fn inserts_route_headers() {
        let plan = RoutePlan {
            profile: "local".to_string(),
            modality: Modality::Tts,
            route: vec!["local_kokoro".to_string()],
            selected_provider: "local_kokoro".to_string(),
            resolved_model: Some("tts-1".to_string()),
            resolved_voice: Some("af_sky".to_string()),
            response_format: Some("mp3".to_string()),
        };
        let mut headers = HeaderMap::new();

        insert_route_headers(&mut headers, &plan);

        assert_eq!(headers["x-voicemux-profile"], "local");
        assert_eq!(headers["x-voicemux-provider"], "local_kokoro");
        assert_eq!(headers["x-voicemux-route"], "local_kokoro");
        assert_eq!(headers["x-voicemux-model"], "tts-1");
        assert_eq!(headers["x-voicemux-voice"], "af_sky");
    }
}
