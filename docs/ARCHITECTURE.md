# Architecture

`voicemux` is an OpenAI-compatible audio routing layer.

It accepts client requests in the OpenAI audio API shape, chooses a backend provider from the active profile, forwards the request, and returns an OpenAI-compatible response.

## Core Principles

- Thin routing layer, not model runtime.
- Audio-only, not a broad LLM gateway.
- OpenAI-compatible at the edge.
- Local and cloud providers are both first-class.
- Generic OpenAI-compatible adapters come before bespoke provider integrations.
- Fallback behavior should be explicit and observable.
- Provider secrets stay server-side.
- The request path should be async, streaming-friendly, and low overhead.

## Implementation Stack

The core service should be implemented in Rust.

Recommended stack:

- Tokio for async execution.
- Axum for routing and HTTP ergonomics, or Hyper directly if lower-level streaming control becomes necessary.
- Reqwest for provider calls initially, with a possible move to Hyper client if we need finer control over streaming and connection reuse.
- Serde and serde_yaml for config parsing.
- Tracing and tracing-subscriber for structured logs.
- Tower and tower-http for middleware, request IDs, limits, timeouts, and tracing.
- Thiserror and anyhow for adapter/config errors.
- Rustls for TLS defaults.

This is an infrastructure proxy, so performance matters even though the first version should stay simple. The implementation should avoid a Python/FastAPI core unless iteration speed becomes more important than long-running proxy performance and binary distribution.

## Performance Model

Most `voicemux` work is I/O-bound:

- accept multipart audio uploads
- forward audio to STT providers
- accept JSON TTS requests
- stream audio bytes back from TTS providers
- enforce timeouts
- try fallback providers when needed

Design implications:

- Do not perform CPU-heavy audio processing in the core router.
- Do not decode/transcode audio in the core MVP.
- Do not buffer large audio payloads unless a provider adapter requires it.
- Stream request and response bodies when possible.
- Keep provider clients reusable to preserve connection pooling.
- Make fallback policy deterministic and cheap.
- Use explicit limits for request body size, provider timeout, and max fallback attempts.

## Request Flow

```text
Client
  -> /v1/audio/transcriptions or /v1/audio/speech
  -> voicemux route resolver
  -> provider chain for active profile
  -> first healthy provider that can satisfy the request
  -> normalized OpenAI-compatible response
```

The router should preserve streaming semantics when possible. For example, if a TTS backend streams audio chunks, `voicemux` should forward those chunks to the client rather than waiting for the entire audio file.

## Main Concepts

### Provider

A provider is a backend capable of serving one or both audio modalities.

Examples:

- `deepgram`: native STT provider.
- `elevenlabs`: native TTS provider.
- `speaches`: OpenAI-compatible local STT/TTS provider.
- `voxbox`: OpenAI-compatible local STT/TTS provider.
- `litellm`: OpenAI-compatible gateway backend.
- `kokoro`: OpenAI-compatible TTS provider.
- `whisper`: OpenAI-compatible STT provider.

### Profile

A profile defines ordered route chains for STT and TTS.

Examples:

- `local`: local providers only.
- `hybrid`: cloud first, local fallback.
- `premium`: best quality providers first.
- `cheap`: low-cost providers first.
- `private`: no cloud providers.

### Alias

Aliases let clients use stable names while providers use provider-specific IDs.

Examples:

- `voice=assistant` maps to an ElevenLabs voice ID for ElevenLabs.
- `voice=assistant` maps to `af_heart` for Speaches.
- `model=whisper-1` maps to `nova-3` for Deepgram.
- `model=tts-1` maps to a local Kokoro model for Speaches.

## Adapter Types

Provider support is roadmapped in [`PROVIDERS.md`](PROVIDERS.md). The short version: generic OpenAI-compatible adapters come first, native adapters come only where they materially improve compatibility or performance.

### Generic OpenAI-Compatible STT

For backends that implement `/v1/audio/transcriptions`.

This should support:

- Speaches
- Vox Box
- Whisper-compatible servers
- LiteLLM
- OpenAI-compatible hosted providers

### Generic OpenAI-Compatible TTS

For backends that implement `/v1/audio/speech`.

This should support:

- Speaches
- Vox Box
- Kokoro servers
- LiteLLM
- OpenAI-compatible hosted providers

### Native STT Providers

Native adapters are useful when a provider does not expose OpenAI-compatible endpoints or requires request translation.

Initial candidate:

- Deepgram STT

### Native TTS Providers

Native adapters are useful when a provider does not expose OpenAI-compatible endpoints or requires request translation.

Initial candidate:

- ElevenLabs TTS

## Fallback Semantics

Fallback happens only when it is safe and useful. Each request walks the configured route chain up to `fallback.max_attempts_per_request`.

Current fallback candidates:

- Provider configuration errors, such as a missing cloud API key.
- Upstream request errors, except timeouts unless `retry_timeouts` is enabled.
- Upstream JSON parse errors.
- Upstream HTTP statuses listed in `fallback.fallback_on_statuses`.

Fallback does not silently hide invalid client requests when all providers would fail.

## Observability

Every routed request should produce structured logs with:

- request modality: `stt` or `tts`
- selected profile
- attempted providers
- winning provider
- latency
- fallback reason, if any
- response status

Do not log:

- API keys
- raw audio bytes
- full request payloads by default

## Performance Guardrails

- No synchronous provider calls on async request handlers.
- No unbounded body buffering.
- No API-key lookup or config parsing on every request unless hot reload is explicitly enabled.
- No fallback loops without a max-attempt limit.
- No provider health checks inline when cached health is recent enough.
- No audio transcoding in the MVP request path.
- No global mutable state that serializes unrelated requests.

## Non-Goals

`voicemux` should not initially implement:

- model serving
- model downloads
- realtime sessions
- chat completions
- voice-agent pipelines
- browser microphone capture
- desktop dictation UI
- provider billing dashboard

These are real needs, but they belong in adjacent layers or later milestones.
