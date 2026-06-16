use axum::extract::Multipart;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use reqwest::multipart::{Form, Part};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Modality;
use crate::providers::{NativeAdapter, ProviderAdapter, ProviderError};
use crate::routing::{plan_route, RouteError, RoutePlan, RouteRequest};
use crate::state::AppState;

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
    state: &AppState,
    request: SpeechRequest,
) -> Result<Response, OpenAiProxyError> {
    let plan = plan_route(
        &state.config,
        RouteRequest {
            modality: Modality::Tts,
            profile: request.profile.clone(),
            model: request.model.clone(),
            voice: request.voice.clone(),
            response_format: request.response_format.clone(),
        },
    )?;

    let adapter = state
        .providers
        .get(&plan.selected_provider)
        .ok_or_else(|| RouteError::UnknownProvider(plan.selected_provider.clone()))?;
    match adapter {
        ProviderAdapter::OpenAi(_) => proxy_openai_speech(state, adapter, &plan, request).await,
        ProviderAdapter::ElevenlabsTts(adapter) => {
            proxy_elevenlabs_speech(state, adapter, &plan, request).await
        }
        ProviderAdapter::DeepgramStt(adapter) => Err(ProviderError::UnsupportedPassthrough {
            provider: adapter.name().to_string(),
            endpoint: "/v1/audio/speech",
        }
        .into()),
    }
}

async fn proxy_openai_speech(
    state: &AppState,
    adapter: &ProviderAdapter,
    plan: &RoutePlan,
    request: SpeechRequest,
) -> Result<Response, OpenAiProxyError> {
    let endpoint = adapter.openai_audio_speech_endpoint()?;

    let upstream_request = SpeechRequest {
        input: request.input,
        model: plan.resolved_model.clone(),
        voice: plan.resolved_voice.clone(),
        response_format: plan.response_format.clone(),
        speed: request.speed,
        profile: None,
    };

    let mut builder = state.client.post(endpoint.url).json(&upstream_request);

    if let Some(authorization) = endpoint.authorization {
        builder = builder.header(header::AUTHORIZATION.as_str(), authorization);
    }

    let upstream_response = builder.send().await?;
    response_from_upstream(upstream_response, "application/octet-stream", plan).await
}

async fn proxy_elevenlabs_speech(
    state: &AppState,
    adapter: &NativeAdapter,
    plan: &RoutePlan,
    request: SpeechRequest,
) -> Result<Response, OpenAiProxyError> {
    let text = speech_input_text(request.input)?;
    let voice = plan
        .resolved_voice
        .as_deref()
        .ok_or_else(|| OpenAiProxyError::MissingVoice(adapter.name().to_string()))?;
    let model = plan
        .resolved_model
        .as_deref()
        .or_else(|| adapter.model())
        .unwrap_or("eleven_turbo_v2_5");
    let output_format = adapter.output_format().unwrap_or("mp3_44100_128");
    let url = elevenlabs_speech_url(voice, output_format);

    let upstream_response = state
        .client
        .post(url)
        .header("xi-api-key", adapter.api_key()?)
        .json(&serde_json::json!({
            "text": text,
            "model_id": model,
        }))
        .send()
        .await?;

    response_from_upstream(upstream_response, "audio/mpeg", plan).await
}

fn elevenlabs_speech_url(voice: &str, output_format: &str) -> String {
    let mut url = Url::parse("https://api.elevenlabs.io/v1/text-to-speech")
        .expect("static ElevenLabs URL should parse");
    url.path_segments_mut()
        .expect("ElevenLabs URL should support path segments")
        .push(voice);
    url.query_pairs_mut()
        .append_pair("output_format", output_format);
    url.to_string()
}

fn speech_input_text(input: Value) -> Result<String, OpenAiProxyError> {
    match input {
        Value::String(text) => Ok(text),
        _ => Err(OpenAiProxyError::InvalidSpeechInput),
    }
}

async fn response_from_upstream(
    upstream_response: reqwest::Response,
    fallback_content_type: &'static str,
    plan: &RoutePlan,
) -> Result<Response, OpenAiProxyError> {
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
                .unwrap_or_else(|_| HeaderValue::from_static(fallback_content_type)),
        );
    } else {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(fallback_content_type),
        );
    }
    insert_route_headers(&mut headers, plan);

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

    fn file_field(&self) -> Option<&TranscriptionField> {
        self.fields
            .iter()
            .find(|field| field.name == "file" && field.file_name.is_some())
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
    state: &AppState,
    request: TranscriptionRequest,
) -> Result<Response, OpenAiProxyError> {
    let route_request = RouteRequest {
        modality: Modality::Stt,
        profile: request.text_field("profile"),
        model: request.text_field("model"),
        voice: None,
        response_format: request.text_field("response_format"),
    };
    let plan = plan_route(&state.config, route_request)?;

    let adapter = state
        .providers
        .get(&plan.selected_provider)
        .ok_or_else(|| RouteError::UnknownProvider(plan.selected_provider.clone()))?;
    match adapter {
        ProviderAdapter::OpenAi(_) => {
            proxy_openai_transcription(state, adapter, &plan, request).await
        }
        ProviderAdapter::DeepgramStt(adapter) => {
            proxy_deepgram_transcription(state, adapter, &plan, request).await
        }
        ProviderAdapter::ElevenlabsTts(adapter) => Err(ProviderError::UnsupportedPassthrough {
            provider: adapter.name().to_string(),
            endpoint: "/v1/audio/transcriptions",
        }
        .into()),
    }
}

async fn proxy_openai_transcription(
    state: &AppState,
    adapter: &ProviderAdapter,
    plan: &RoutePlan,
    request: TranscriptionRequest,
) -> Result<Response, OpenAiProxyError> {
    let endpoint = adapter.openai_audio_transcriptions_endpoint()?;
    let form = request.into_form(plan.resolved_model.clone())?;

    let mut builder = state.client.post(endpoint.url).multipart(form);

    if let Some(authorization) = endpoint.authorization {
        builder = builder.header(header::AUTHORIZATION.as_str(), authorization);
    }

    let upstream_response = builder.send().await?;
    response_from_upstream(upstream_response, "application/json", plan).await
}

async fn proxy_deepgram_transcription(
    state: &AppState,
    adapter: &NativeAdapter,
    plan: &RoutePlan,
    request: TranscriptionRequest,
) -> Result<Response, OpenAiProxyError> {
    let response_format = request.text_field("response_format");
    let language = request.text_field("language");
    let file = request
        .file_field()
        .ok_or(OpenAiProxyError::MissingTranscriptionFile)?;
    let mut params = Vec::new();

    if let Some(model) = plan.resolved_model.as_deref().or_else(|| adapter.model()) {
        params.push(("model", model.to_string()));
    }

    if let Some(language) = language.as_deref().or_else(|| adapter.language()) {
        if language != "auto" {
            params.push(("language", language.to_string()));
        }
    }

    if let Some(smart_format) = adapter.smart_format() {
        params.push(("smart_format", smart_format.to_string()));
    }

    if let Some(punctuate) = adapter.punctuate() {
        params.push(("punctuate", punctuate.to_string()));
    }

    let url = deepgram_listen_url(&params);

    let content_type = file
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    let upstream_response = state
        .client
        .post(url)
        .header(
            header::AUTHORIZATION,
            format!("Token {}", adapter.api_key()?),
        )
        .header(header::CONTENT_TYPE, content_type)
        .body(file.bytes.clone())
        .send()
        .await?;
    let status = upstream_response.status();
    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE.as_str())
        .cloned();
    let body = upstream_response.bytes().await?;

    if !status.is_success() {
        let mut headers = HeaderMap::new();
        if let Some(content_type) = content_type {
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_bytes(content_type.as_bytes())
                    .unwrap_or_else(|_| HeaderValue::from_static("application/json")),
            );
        } else {
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
        }
        insert_route_headers(&mut headers, plan);

        return Ok((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            headers,
            body,
        )
            .into_response());
    }

    let deepgram_response: Value = serde_json::from_slice(&body)?;
    let transcript = deepgram_transcript(&deepgram_response).unwrap_or_default();
    let mut headers = HeaderMap::new();
    insert_route_headers(&mut headers, plan);

    if response_format.as_deref() == Some("text") {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        return Ok((StatusCode::OK, headers, transcript.to_string()).into_response());
    }

    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    Ok((
        StatusCode::OK,
        headers,
        serde_json::json!({ "text": transcript }).to_string(),
    )
        .into_response())
}

fn deepgram_listen_url(params: &[(&str, String)]) -> String {
    let mut url =
        Url::parse("https://api.deepgram.com/v1/listen").expect("static Deepgram URL should parse");
    url.query_pairs_mut()
        .extend_pairs(params.iter().map(|(key, value)| (*key, value.as_str())));
    url.to_string()
}

fn deepgram_transcript(response: &Value) -> Option<&str> {
    response
        .pointer("/results/channels/0/alternatives/0/transcript")
        .and_then(Value::as_str)
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
    #[error("upstream response was not valid json: {0}")]
    UpstreamJson(#[from] serde_json::Error),
    #[error("multipart request failed: {0}")]
    Multipart(#[from] axum::extract::multipart::MultipartError),
    #[error("multipart field is missing a name")]
    MissingMultipartFieldName,
    #[error("invalid multipart content type: {0}")]
    MultipartContentType(#[from] reqwest::header::InvalidHeaderValue),
    #[error("speech input must be a string")]
    InvalidSpeechInput,
    #[error("speech request must include a resolved voice for provider '{0}'")]
    MissingVoice(String),
    #[error("transcription request must include a file field")]
    MissingTranscriptionFile,
}

impl OpenAiProxyError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Route(_)
            | Self::Provider(_)
            | Self::Multipart(_)
            | Self::MissingMultipartFieldName
            | Self::MultipartContentType(_)
            | Self::InvalidSpeechInput
            | Self::MissingVoice(_)
            | Self::MissingTranscriptionFile => StatusCode::BAD_REQUEST,
            Self::Upstream(_) | Self::UpstreamJson(_) => StatusCode::BAD_GATEWAY,
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            Self::Route(_) => "invalid_route_request",
            Self::Provider(_) => "provider_configuration_error",
            Self::Upstream(_) | Self::UpstreamJson(_) => "upstream_error",
            Self::Multipart(_)
            | Self::MissingMultipartFieldName
            | Self::MultipartContentType(_) => "invalid_multipart_request",
            Self::InvalidSpeechInput | Self::MissingVoice(_) => "invalid_speech_request",
            Self::MissingTranscriptionFile => "invalid_transcription_request",
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

    #[test]
    fn extracts_deepgram_transcript() {
        let response = serde_json::json!({
            "results": {
                "channels": [{
                    "alternatives": [{ "transcript": "hello world" }]
                }]
            }
        });

        assert_eq!(deepgram_transcript(&response), Some("hello world"));
    }

    #[test]
    fn rejects_non_string_speech_input() {
        let error = speech_input_text(serde_json::json!(["hello"])).expect_err("input should fail");

        assert!(matches!(error, OpenAiProxyError::InvalidSpeechInput));
    }

    #[test]
    fn builds_encoded_elevenlabs_url() {
        let url = elevenlabs_speech_url("voice/id", "mp3_44100_128");

        assert_eq!(
            url,
            "https://api.elevenlabs.io/v1/text-to-speech/voice%2Fid?output_format=mp3_44100_128"
        );
    }

    #[test]
    fn builds_encoded_deepgram_url() {
        let url = deepgram_listen_url(&[
            ("model", "nova 3".to_string()),
            ("language", "en-US".to_string()),
        ]);

        assert_eq!(
            url,
            "https://api.deepgram.com/v1/listen?model=nova+3&language=en-US"
        );
    }
}
