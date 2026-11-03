use axum::body::Body;
use axum::extract::Multipart;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use reqwest::multipart::{Form, Part};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Modality;
use crate::providers::{NativeAdapter, ProviderAdapter, ProviderError};
use crate::routing::{plan_route_candidates, RouteError, RoutePlan, RouteRequest};
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
    let plans = plan_route_candidates(
        &state.config,
        RouteRequest {
            modality: Modality::Tts,
            profile: request.profile.clone(),
            model: request.model.clone(),
            voice: request.voice.clone(),
            response_format: request.response_format.clone(),
        },
    )?;

    let max_attempts = state.config.fallback.max_attempts_per_request.max(1);
    let mut last_error = None;

    for (index, plan) in plans.into_iter().take(max_attempts).enumerate() {
        let is_last_attempt = index + 1 == max_attempts || index + 1 == plan.route.len();
        match proxy_speech_attempt(state, request.clone(), &plan).await {
            Ok(response) => {
                if !is_last_attempt && should_fallback_status(state, response.status()) {
                    continue;
                }

                return Ok(response);
            }
            Err(error) => {
                if !is_last_attempt && should_fallback_error(state, &error) {
                    last_error = Some(error);
                    continue;
                }

                return Err(error);
            }
        }
    }

    Err(last_error.unwrap_or(OpenAiProxyError::Route(RouteError::NoRouteCandidates)))
}

async fn proxy_speech_attempt(
    state: &AppState,
    request: SpeechRequest,
    plan: &RoutePlan,
) -> Result<Response, OpenAiProxyError> {
    let adapter = state
        .providers
        .get(&plan.selected_provider)
        .ok_or_else(|| RouteError::UnknownProvider(plan.selected_provider.clone()))?;
    match adapter {
        ProviderAdapter::OpenAi(_) => proxy_openai_speech(state, adapter, plan, request).await,
        ProviderAdapter::ElevenlabsTts(adapter) => {
            proxy_elevenlabs_speech(state, adapter, plan, request).await
        }
        ProviderAdapter::GiggyTts(adapter) => {
            proxy_giggy_speech(state, adapter, plan, request).await
        }
        ProviderAdapter::DeepgramStt(adapter) => Err(ProviderError::UnsupportedPassthrough {
            provider: adapter.name().to_string(),
            endpoint: "/v1/audio/speech",
        }
        .into()),
    }
}

async fn proxy_giggy_speech(
    state: &AppState,
    adapter: &NativeAdapter,
    plan: &RoutePlan,
    request: SpeechRequest,
) -> Result<Response, OpenAiProxyError> {
    let text = speech_input_text(request.input)?;
    let voice_id = plan
        .resolved_voice
        .as_deref()
        .ok_or_else(|| OpenAiProxyError::MissingVoice(adapter.name().to_string()))?;
    let output_format =
        giggy_output_format(request.response_format.as_deref(), adapter.output_format())?;
    let mode = giggy_mode(adapter.mode())?;
    let model_id = plan.resolved_model.as_deref().or_else(|| adapter.model());
    let body = giggy_request_body(&text, voice_id, mode, output_format, model_id);
    let upstream_response = state
        .client
        .post("https://api.giggy.ai/v1/text-to-speech")
        .header("xi-api-key", adapter.api_key()?)
        .json(&body)
        .send()
        .await?;
    response_from_upstream_stream(upstream_response, "audio/pcm;rate=24000", plan).await
}

fn giggy_output_format<'a>(
    requested: Option<&'a str>,
    configured: Option<&'a str>,
) -> Result<&'a str, OpenAiProxyError> {
    let requested = requested.unwrap_or("pcm");
    if !matches!(requested, "pcm" | "pcm_24000") {
        return Err(OpenAiProxyError::UnsupportedOutputFormat(
            requested.to_string(),
        ));
    }
    let configured = configured.unwrap_or("pcm_24000");
    if configured != "pcm_24000" {
        return Err(OpenAiProxyError::UnsupportedOutputFormat(
            configured.to_string(),
        ));
    }
    Ok(configured)
}

fn giggy_mode(configured: Option<&str>) -> Result<&str, OpenAiProxyError> {
    let mode = configured.unwrap_or("streaming");
    if mode != "streaming" {
        return Err(OpenAiProxyError::InvalidGiggyMode(mode.to_string()));
    }
    Ok(mode)
}

fn giggy_request_body(
    text: &str,
    voice_id: &str,
    mode: &str,
    output_format: &str,
    model_id: Option<&str>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "text": text,
        "voice_id": voice_id,
        "mode": mode,
        "output_format": output_format,
    });
    if let Some(model_id) = model_id {
        body["model_id"] = serde_json::Value::String(model_id.to_string());
    }
    body
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
    // Priority: request.response_format > adapter config > hardcoded default.
    // Map OpenAI format names (e.g. "pcm") to ElevenLabs native strings (e.g. "pcm_24000").
    let config_default = adapter.output_format().unwrap_or("mp3_44100_128");
    let output_format = match request.response_format.as_deref() {
        Some(fmt) => elevenlabs_output_format(fmt),
        None => config_default,
    };
    let content_type = elevenlabs_content_type(output_format);
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

    response_from_upstream(upstream_response, content_type, plan).await
}

/// Maps an OpenAI `response_format` value (e.g. "pcm", "mp3", "opus", "wav")
/// to an ElevenLabs `output_format` query parameter.
///
/// If the value is already an ElevenLabs-native format string (contains `_`),
/// it is passed through unchanged.
fn elevenlabs_output_format(openai_format: &str) -> &str {
    // Pass through ElevenLabs-native format strings like "pcm_44100", "mp3_22050_32"
    if openai_format.contains('_') {
        return openai_format;
    }
    match openai_format {
        "pcm" => "pcm_24000",
        "mp3" => "mp3_44100_128",
        "opus" => "opus_48000_128",
        "wav" => "wav_44100",
        // aac, flac have no ElevenLabs equivalent — fall back to mp3
        "aac" | "flac" => "mp3_44100_128",
        _ => openai_format,
    }
}

/// Returns the appropriate `Content-Type` header value for a given
/// ElevenLabs `output_format` string.
fn elevenlabs_content_type(output_format: &str) -> &'static str {
    if output_format.starts_with("pcm_") {
        "audio/pcm"
    } else if output_format.starts_with("mp3_") {
        "audio/mpeg"
    } else if output_format.starts_with("opus_") {
        "audio/opus"
    } else if output_format.starts_with("wav_") {
        "audio/wav"
    } else if output_format.starts_with("alaw_") || output_format.starts_with("ulaw_") {
        "audio/basic"
    } else {
        "application/octet-stream"
    }
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

async fn response_from_upstream_stream(
    upstream_response: reqwest::Response,
    fallback_content_type: &'static str,
    plan: &RoutePlan,
) -> Result<Response, OpenAiProxyError> {
    let status = upstream_response.status();
    if !status.is_success() {
        return response_from_upstream(upstream_response, fallback_content_type, plan).await;
    }

    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE.as_str())
        .cloned();
    let content_type_text = content_type
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing");
    if !is_giggy_pcm_content_type(content_type_text) {
        return Err(OpenAiProxyError::UnexpectedUpstreamContentType(
            content_type_text.to_string(),
        ));
    }
    let mut headers = upstream_headers(content_type, fallback_content_type);
    insert_route_headers(&mut headers, plan);
    let body = Body::from_stream(upstream_response.bytes_stream());

    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        headers,
        body,
    )
        .into_response())
}

fn is_giggy_pcm_content_type(content_type: &str) -> bool {
    let mut parts = content_type.split(';');
    let media_type = parts.next().unwrap_or_default().trim();
    let rates: Vec<_> = parts
        .filter_map(|parameter| parameter.trim().split_once('='))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("rate"))
        .map(|(_, value)| value.trim())
        .collect();
    media_type.eq_ignore_ascii_case("audio/pcm") && rates.len() == 1 && rates[0] == "24000"
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

    let mut headers = upstream_headers(content_type, fallback_content_type);
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
    let plans = plan_route_candidates(&state.config, route_request)?;

    let max_attempts = state.config.fallback.max_attempts_per_request.max(1);
    let mut last_error = None;

    for (index, plan) in plans.into_iter().take(max_attempts).enumerate() {
        let is_last_attempt = index + 1 == max_attempts || index + 1 == plan.route.len();
        match proxy_transcription_attempt(state, request.clone(), &plan).await {
            Ok(response) => {
                if !is_last_attempt && should_fallback_status(state, response.status()) {
                    continue;
                }

                return Ok(response);
            }
            Err(error) => {
                if !is_last_attempt && should_fallback_error(state, &error) {
                    last_error = Some(error);
                    continue;
                }

                return Err(error);
            }
        }
    }

    Err(last_error.unwrap_or(OpenAiProxyError::Route(RouteError::NoRouteCandidates)))
}

async fn proxy_transcription_attempt(
    state: &AppState,
    request: TranscriptionRequest,
    plan: &RoutePlan,
) -> Result<Response, OpenAiProxyError> {
    let adapter = state
        .providers
        .get(&plan.selected_provider)
        .ok_or_else(|| RouteError::UnknownProvider(plan.selected_provider.clone()))?;
    match adapter {
        ProviderAdapter::OpenAi(_) => {
            proxy_openai_transcription(state, adapter, plan, request).await
        }
        ProviderAdapter::DeepgramStt(adapter) => {
            proxy_deepgram_transcription(state, adapter, plan, request).await
        }
        ProviderAdapter::GiggyTts(adapter) => {
            proxy_giggy_transcription(state, adapter, plan, request).await
        }
        ProviderAdapter::ElevenlabsTts(adapter) => Err(ProviderError::UnsupportedPassthrough {
            provider: adapter.name().to_string(),
            endpoint: "/v1/audio/transcriptions",
        }
        .into()),
    }
}

fn should_fallback_status(state: &AppState, status: StatusCode) -> bool {
    state
        .config
        .fallback
        .fallback_on_statuses
        .contains(&status.as_u16())
}

fn should_fallback_error(state: &AppState, error: &OpenAiProxyError) -> bool {
    match error {
        OpenAiProxyError::Provider(_) | OpenAiProxyError::UpstreamJson(_) => true,
        OpenAiProxyError::Upstream(error) => {
            !error.is_timeout() || state.config.fallback.retry_timeouts
        }
        OpenAiProxyError::Route(_)
        | OpenAiProxyError::Multipart(_)
        | OpenAiProxyError::MissingMultipartFieldName
        | OpenAiProxyError::MultipartContentType(_)
        | OpenAiProxyError::InvalidSpeechInput
        | OpenAiProxyError::MissingVoice(_)
        | OpenAiProxyError::UnsupportedOutputFormat(_)
        | OpenAiProxyError::InvalidGiggyMode(_)
        | OpenAiProxyError::MissingTranscriptionFile
        | OpenAiProxyError::GiggyTranscriptionRequiresStreaming
        | OpenAiProxyError::InvalidGiggyTranscriptionAudio => false,
        OpenAiProxyError::UnexpectedUpstreamContentType(_) => true,
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
        let mut headers = upstream_headers(content_type, "application/json");
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

async fn proxy_giggy_transcription(
    state: &AppState,
    adapter: &NativeAdapter,
    plan: &RoutePlan,
    request: TranscriptionRequest,
) -> Result<Response, OpenAiProxyError> {
    if !request
        .text_field("stream")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        return Err(OpenAiProxyError::GiggyTranscriptionRequiresStreaming);
    }

    let file = request
        .file_field()
        .ok_or(OpenAiProxyError::MissingTranscriptionFile)?;
    if !is_giggy_streaming_wav(&file.bytes) {
        return Err(OpenAiProxyError::InvalidGiggyTranscriptionAudio);
    }

    let upstream_response = state
        .client
        .post(giggy_transcription_url())
        .query(&[("mode", "streaming")])
        .header("xi-api-key", adapter.api_key()?)
        .header(header::CONTENT_TYPE, "audio/wav")
        .body(file.bytes.clone())
        .send()
        .await?;
    response_from_upstream_event_stream(upstream_response, plan).await
}

async fn response_from_upstream_event_stream(
    upstream_response: reqwest::Response,
    plan: &RoutePlan,
) -> Result<Response, OpenAiProxyError> {
    let status = upstream_response.status();
    if !status.is_success() {
        return response_from_upstream(upstream_response, "application/json", plan).await;
    }

    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE.as_str())
        .cloned();
    let content_type_text = content_type
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing");
    let media_type = content_type_text
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if !media_type.eq_ignore_ascii_case("text/event-stream") {
        return Err(OpenAiProxyError::UnexpectedUpstreamContentType(
            content_type_text.to_string(),
        ));
    }

    let mut headers = upstream_headers(content_type, "text/event-stream");
    insert_route_headers(&mut headers, plan);
    let body = Body::from_stream(upstream_response.bytes_stream());
    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        headers,
        body,
    )
        .into_response())
}

fn giggy_transcription_url() -> Url {
    Url::parse("https://api.giggy.ai/v1/audio/transcriptions")
        .expect("static Giggy transcription URL should parse")
}

fn is_giggy_streaming_wav(bytes: &[u8]) -> bool {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return false;
    }
    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if riff_size + 8 > bytes.len() {
        return false;
    }

    let mut cursor = 12usize;
    let mut valid_format = false;
    let mut audio_bytes = None;
    while cursor.saturating_add(8) <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let chunk_len =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let start = cursor + 8;
        let Some(end) = start.checked_add(chunk_len) else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }

        if id == b"fmt " && chunk_len >= 16 {
            let format = u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap());
            let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap());
            let sample_rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap());
            let byte_rate = u32::from_le_bytes(bytes[start + 8..start + 12].try_into().unwrap());
            let block_align = u16::from_le_bytes(bytes[start + 12..start + 14].try_into().unwrap());
            let bits_per_sample =
                u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap());
            valid_format = format == 1
                && channels == 1
                && sample_rate == 16_000
                && byte_rate == 32_000
                && block_align == 2
                && bits_per_sample == 16;
        } else if id == b"data" {
            audio_bytes = Some(chunk_len);
        }

        cursor = end + (chunk_len % 2);
    }

    valid_format && audio_bytes.is_some_and(|length| length >= 16_000 && length % 2 == 0)
}

fn upstream_headers(
    content_type: Option<HeaderValue>,
    fallback_content_type: &'static str,
) -> HeaderMap {
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

    headers
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
    #[error("upstream returned unsupported content type '{0}'")]
    UnexpectedUpstreamContentType(String),
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
    #[error("unsupported Giggy output format '{0}'; streaming Giggy TTS supports pcm_24000")]
    UnsupportedOutputFormat(String),
    #[error("invalid Giggy mode '{0}'; this provider requires 'streaming'")]
    InvalidGiggyMode(String),
    #[error("transcription request must include a file field")]
    MissingTranscriptionFile,
    #[error("Giggy transcription requires stream=true")]
    GiggyTranscriptionRequiresStreaming,
    #[error("Giggy transcription requires PCM16 mono 16 kHz WAV audio of at least 0.5 seconds")]
    InvalidGiggyTranscriptionAudio,
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
            | Self::UnsupportedOutputFormat(_)
            | Self::InvalidGiggyMode(_)
            | Self::MissingTranscriptionFile
            | Self::GiggyTranscriptionRequiresStreaming
            | Self::InvalidGiggyTranscriptionAudio => StatusCode::BAD_REQUEST,
            Self::Upstream(_) | Self::UpstreamJson(_) | Self::UnexpectedUpstreamContentType(_) => {
                StatusCode::BAD_GATEWAY
            }
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            Self::Route(_) => "invalid_route_request",
            Self::Provider(_) => "provider_configuration_error",
            Self::Upstream(_) | Self::UpstreamJson(_) | Self::UnexpectedUpstreamContentType(_) => {
                "upstream_error"
            }
            Self::Multipart(_)
            | Self::MissingMultipartFieldName
            | Self::MultipartContentType(_) => "invalid_multipart_request",
            Self::InvalidSpeechInput
            | Self::MissingVoice(_)
            | Self::UnsupportedOutputFormat(_)
            | Self::InvalidGiggyMode(_) => "invalid_speech_request",
            Self::MissingTranscriptionFile
            | Self::GiggyTranscriptionRequiresStreaming
            | Self::InvalidGiggyTranscriptionAudio => "invalid_transcription_request",
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
    use crate::config::VoicemuxConfig;
    use futures_util::StreamExt;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    fn app_state() -> AppState {
        AppState::new(
            VoicemuxConfig::from_yaml(include_str!("../examples/voicemux.yaml"))
                .expect("example config should parse"),
        )
        .expect("app state should build")
    }

    fn pcm16_wav(sample_rate: u32, channels: u16, bits_per_sample: u16, frames: usize) -> Vec<u8> {
        let data_len = frames * channels as usize * (bits_per_sample as usize / 8);
        let mut wav = Vec::with_capacity(44 + data_len);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + data_len as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        let block_align = channels * (bits_per_sample / 8);
        wav.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data_len as u32).to_le_bytes());
        wav.resize(44 + data_len, 0);
        wav
    }

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

    #[test]
    fn falls_back_on_configured_statuses() {
        let state = app_state();

        assert!(should_fallback_status(
            &state,
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(should_fallback_status(&state, StatusCode::BAD_GATEWAY));
        assert!(!should_fallback_status(&state, StatusCode::BAD_REQUEST));
    }

    #[test]
    fn falls_back_on_provider_configuration_errors() {
        let state = app_state();
        let error = OpenAiProxyError::Provider(ProviderError::MissingApiKey("elevenlabs".into()));

        assert!(should_fallback_error(&state, &error));
    }

    #[test]
    fn giggy_pcm_content_type_is_exact_and_rate_checked() {
        assert!(is_giggy_pcm_content_type("audio/pcm;rate=24000"));
        assert!(is_giggy_pcm_content_type(
            " Audio/PCM ; channels=1; rate=24000 "
        ));
        assert!(!is_giggy_pcm_content_type("audio/pcm"));
        assert!(!is_giggy_pcm_content_type("audio/pcmx;rate=24000"));
        assert!(!is_giggy_pcm_content_type("audio/pcm;rate=16000"));
        assert!(!is_giggy_pcm_content_type(
            "audio/pcm;rate=24000;rate=16000"
        ));
    }

    #[test]
    fn unexpected_giggy_content_type_uses_provider_fallback() {
        let state = app_state();
        let error = OpenAiProxyError::UnexpectedUpstreamContentType("application/json".into());
        assert!(should_fallback_error(&state, &error));
    }

    #[test]
    fn giggy_stt_accepts_only_supported_pcm_wav() {
        assert!(is_giggy_streaming_wav(&pcm16_wav(16_000, 1, 16, 8_000)));
        assert!(!is_giggy_streaming_wav(&pcm16_wav(24_000, 1, 16, 8_000)));
        assert!(!is_giggy_streaming_wav(&pcm16_wav(16_000, 2, 16, 8_000)));
        assert!(!is_giggy_streaming_wav(&pcm16_wav(16_000, 1, 8, 16_000)));
        assert!(!is_giggy_streaming_wav(&pcm16_wav(16_000, 1, 16, 7_999)));
        let mut truncated = pcm16_wav(16_000, 1, 16, 8_000);
        truncated.truncate(52);
        assert!(!is_giggy_streaming_wav(&truncated));
        let mut inconsistent_riff_size = pcm16_wav(16_000, 1, 16, 8_000);
        inconsistent_riff_size[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!is_giggy_streaming_wav(&inconsistent_riff_size));
        let mut odd_pcm = pcm16_wav(16_000, 1, 16, 8_000);
        odd_pcm.push(0);
        let riff_len = (odd_pcm.len() - 8) as u32;
        odd_pcm[4..8].copy_from_slice(&riff_len.to_le_bytes());
        odd_pcm[40..44].copy_from_slice(&16_001u32.to_le_bytes());
        assert!(!is_giggy_streaming_wav(&odd_pcm));
    }

    #[test]
    fn giggy_defaults_and_validates_mode_and_output_format() {
        assert_eq!(giggy_output_format(None, None).unwrap(), "pcm_24000");
        assert_eq!(giggy_output_format(Some("pcm"), None).unwrap(), "pcm_24000");
        assert_eq!(
            giggy_output_format(Some("pcm_24000"), None).unwrap(),
            "pcm_24000"
        );
        assert_eq!(giggy_mode(None).unwrap(), "streaming");
        assert_eq!(giggy_mode(Some("streaming")).unwrap(), "streaming");
        assert!(matches!(
            giggy_output_format(Some("wav"), None),
            Err(OpenAiProxyError::UnsupportedOutputFormat(_))
        ));
        assert!(matches!(
            giggy_output_format(None, Some("mp3_24000_160")),
            Err(OpenAiProxyError::UnsupportedOutputFormat(_))
        ));
        assert!(matches!(
            giggy_mode(Some("fast")),
            Err(OpenAiProxyError::InvalidGiggyMode(_))
        ));
    }

    #[test]
    fn builds_giggy_native_request_contract() {
        let body = giggy_request_body(
            "Hello world",
            "00000000-0000-4000-8000-000000000001",
            "streaming",
            "pcm_24000",
            None,
        );
        assert_eq!(body["text"], "Hello world");
        assert_eq!(body["voice_id"], "00000000-0000-4000-8000-000000000001");
        assert_eq!(body["mode"], "streaming");
        assert_eq!(body["output_format"], "pcm_24000");
        assert!(body.get("model_id").is_none());

        let compat_body = giggy_request_body(
            "Hi",
            "voice-id",
            "streaming",
            "pcm_24000",
            Some("giggyspeech"),
        );
        assert_eq!(compat_body["model_id"], "giggyspeech");
    }

    #[tokio::test]
    async fn forwards_giggy_audio_chunks_before_upstream_completes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock server should bind");
        let address = listener
            .local_addr()
            .expect("mock server should have an address");
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("mock client should connect");
            let mut request = [0; 1024];
            let request_bytes = socket
                .read(&mut request)
                .expect("request should be readable");
            assert!(request_bytes > 0, "mock request should not be empty");
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: audio/pcm;rate=24000\r\nConnection: close\r\n\r\n4\r\nABCD\r\n",
                )
                .expect("first audio chunk should be sent");
            socket.flush().expect("first chunk should be flushed");
            std::thread::sleep(Duration::from_millis(300));
            socket
                .write_all(b"4\r\nEFGH\r\n0\r\n\r\n")
                .expect("final audio chunk should be sent");
        });

        let upstream = reqwest::Client::new()
            .get(format!("http://{address}/audio"))
            .send()
            .await
            .expect("mock response should arrive");
        let plan = RoutePlan {
            profile: "live".to_string(),
            modality: Modality::Tts,
            route: vec!["giggy".to_string()],
            selected_provider: "giggy".to_string(),
            resolved_model: Some("giggyspeech".to_string()),
            resolved_voice: Some("voice-id".to_string()),
            response_format: Some("pcm".to_string()),
        };
        let response = response_from_upstream_stream(upstream, "audio/pcm;rate=24000", &plan)
            .await
            .expect("streaming response should be constructed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "audio/pcm;rate=24000"
        );

        let mut audio = response.into_body().into_data_stream();
        let first = audio
            .next()
            .await
            .expect("first chunk should arrive")
            .expect("first chunk should be valid");
        assert_eq!(first.as_ref(), b"ABCD");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), audio.next())
                .await
                .is_err()
        );
        let second = audio
            .next()
            .await
            .expect("second chunk should arrive")
            .expect("second chunk should be valid");
        assert_eq!(second.as_ref(), b"EFGH");
        server.join().expect("mock server should finish");
    }

    #[tokio::test]
    async fn forwards_giggy_transcription_events_before_upstream_completes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock server should bind");
        let address = listener
            .local_addr()
            .expect("mock server should have an address");
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("mock client should connect");
            let mut request = [0; 1024];
            let request_bytes = socket
                .read(&mut request)
                .expect("request should be readable");
            assert!(request_bytes > 0, "mock request should not be empty");

            let first = b"data: {\"type\":\"transcript_chunk\"}\n\n";
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{:X}\r\n",
                        first.len()
                    )
                    .as_bytes(),
                )
                .expect("response header should be sent");
            socket
                .write_all(first)
                .expect("first transcript event should be sent");
            socket
                .write_all(b"\r\n")
                .expect("first chunk should be terminated");
            socket.flush().expect("first event should be flushed");
            std::thread::sleep(Duration::from_millis(300));

            let terminal = b"data: [DONE]\n\n";
            socket
                .write_all(format!("{:X}\r\n", terminal.len()).as_bytes())
                .expect("terminal chunk header should be sent");
            socket
                .write_all(terminal)
                .expect("terminal event should be sent");
            socket
                .write_all(b"\r\n0\r\n\r\n")
                .expect("chunked response should complete");
        });

        let upstream = reqwest::Client::new()
            .get(format!("http://{address}/transcriptions"))
            .send()
            .await
            .expect("mock response should arrive");
        let plan = RoutePlan {
            profile: "live".to_string(),
            modality: Modality::Stt,
            route: vec!["giggy".to_string()],
            selected_provider: "giggy".to_string(),
            resolved_model: Some("giggyspeech".to_string()),
            resolved_voice: None,
            response_format: None,
        };
        let response = response_from_upstream_event_stream(upstream, &plan)
            .await
            .expect("streaming response should be constructed");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));

        let mut events = response.into_body().into_data_stream();
        let first = events
            .next()
            .await
            .expect("first event should arrive")
            .expect("first event should be valid");
        assert!(first
            .as_ref()
            .windows(b"transcript_chunk".len())
            .any(|window| window == b"transcript_chunk"));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), events.next())
                .await
                .is_err()
        );
        let terminal = events
            .next()
            .await
            .expect("terminal event should arrive")
            .expect("terminal event should be valid");
        assert!(terminal
            .as_ref()
            .windows(b"[DONE]".len())
            .any(|window| window == b"[DONE]"));
        server.join().expect("mock server should finish");
    }

    #[test]
    fn maps_openai_formats_to_elevenlabs() {
        assert_eq!(elevenlabs_output_format("pcm"), "pcm_24000");
        assert_eq!(elevenlabs_output_format("mp3"), "mp3_44100_128");
        assert_eq!(elevenlabs_output_format("opus"), "opus_48000_128");
        assert_eq!(elevenlabs_output_format("wav"), "wav_44100");
        assert_eq!(elevenlabs_output_format("aac"), "mp3_44100_128");
        assert_eq!(elevenlabs_output_format("flac"), "mp3_44100_128");
    }

    #[test]
    fn passes_through_elevenlabs_native_formats() {
        assert_eq!(elevenlabs_output_format("pcm_44100"), "pcm_44100");
        assert_eq!(elevenlabs_output_format("mp3_22050_32"), "mp3_22050_32");
        assert_eq!(elevenlabs_output_format("ulaw_8000"), "ulaw_8000");
    }

    #[test]
    fn maps_elevenlabs_formats_to_content_types() {
        assert_eq!(elevenlabs_content_type("pcm_24000"), "audio/pcm");
        assert_eq!(elevenlabs_content_type("pcm_44100"), "audio/pcm");
        assert_eq!(elevenlabs_content_type("mp3_44100_128"), "audio/mpeg");
        assert_eq!(elevenlabs_content_type("opus_48000_128"), "audio/opus");
        assert_eq!(elevenlabs_content_type("wav_44100"), "audio/wav");
        assert_eq!(elevenlabs_content_type("ulaw_8000"), "audio/basic");
        assert_eq!(
            elevenlabs_content_type("unknown"),
            "application/octet-stream"
        );
    }
}
