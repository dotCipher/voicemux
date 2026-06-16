# Provider Roadmap

`voicemux` should support many speech providers without turning into a pile of provider-specific glue. The broader feature plan lives in [`../ROADMAP.md`](../ROADMAP.md); this document focuses on provider adapter shape and provider rollout order.

The roadmap is adapter-family first:

1. Generic OpenAI-compatible audio adapters.
2. Native adapters for high-value providers that need translation.
3. Provider capability discovery and richer policy.
4. Optional realtime routing, only after HTTP routing is proven.

## Provider Shape

Every provider should be represented as a typed config entry.

```yaml
providers:
  speaches:
    type: openai_audio
    base_url: http://127.0.0.1:8000/v1
    api_key_env: SPEACHES_API_KEY
    timeout_seconds: 60
    capabilities: [stt, tts]
```

Provider fields should be explicit and boring:

- `type`: adapter family.
- `base_url`: for OpenAI-compatible backends.
- `api_key_env`: preferred secret reference.
- `api_key`: direct key only for local placeholders or testing.
- `timeout_seconds`: provider-specific timeout.
- `capabilities`: optional explicit modality list: `stt`, `tts`, or both.
- `models`: optional model alias/default overrides.
- `voices`: optional voice alias/default overrides.
- `headers`: optional static headers for local/internal backends.

Secrets should be referenced through environment variables by default. Inline secrets should be discouraged in docs.

## Adapter Contract

Internally, adapters should expose a small capability-oriented contract.

```text
ProviderAdapter
  name() -> ProviderName
  capabilities() -> ProviderCapabilities
  health() -> ProviderHealth
  transcribe(request) -> TranscriptionResult, if STT-capable
  synthesize(request) -> SpeechStream, if TTS-capable
```

The router owns:

- profile selection
- route-chain ordering
- alias resolution
- fallback decisions
- max attempts
- logging and timing

Adapters own:

- provider-specific request translation
- provider-specific auth headers
- provider-specific response normalization
- provider-specific health checks
- provider-specific supported formats

## Capability Model

Capabilities should drive routing, not provider names.

Core capabilities:

- `stt`: can serve `/v1/audio/transcriptions`.
- `tts`: can serve `/v1/audio/speech`.

Optional capability flags:

- `streaming_tts`: can stream TTS responses.
- `streaming_stt`: can stream transcription responses.
- `translations`: can serve `/v1/audio/translations`.
- `voices`: can list voices.
- `models`: can list models.
- `realtime`: supports realtime sessions.
- `local`: runs locally or on user infrastructure.
- `cloud`: sends requests to a hosted third-party API.

The MVP should only require `stt` and `tts`. Everything else can be advisory metadata until implemented.

## Roadmap Tiers

### Tier 0: OpenAI-Compatible Foundation

Goal: make `voicemux` useful with the broadest set of existing tools with minimal code.

Adapters:

- `openai_stt`: backend implements `/v1/audio/transcriptions`.
- `openai_tts`: backend implements `/v1/audio/speech`.
- `openai_audio`: backend implements both.

Backends supported through this tier:

- Speaches
- Vox Box
- LiteLLM
- OpenAI
- local Whisper-compatible servers
- local Kokoro-compatible servers
- any hosted OpenAI-compatible audio endpoint

Why first:

- This makes Speaches and Vox Box immediate backend targets.
- This gives the project broad compatibility without native integrations.
- This validates routing, aliases, fallback, and streaming before provider sprawl.

### Tier 1: High-Value Native Adapters

Goal: support providers that users strongly want and that do not naturally expose OpenAI-compatible audio endpoints.

Initial native adapters:

- `deepgram_stt`
- `elevenlabs_tts`

Deepgram STT shape:

```yaml
providers:
  deepgram:
    type: deepgram_stt
    api_key_env: DEEPGRAM_API_KEY
    model: nova-3
    language: auto
    smart_format: true
    punctuate: true
    timeout_seconds: 20
```

ElevenLabs TTS shape:

```yaml
providers:
  elevenlabs:
    type: elevenlabs_tts
    api_key_env: ELEVENLABS_API_KEY
    model: eleven_turbo_v2_5
    output_format: mp3_44100_128
    timeout_seconds: 30
```

Why these first:

- Deepgram is a strong STT default for latency and quality.
- ElevenLabs is a strong TTS default for voice quality.
- They are common choices in voice stacks.
- Existing local Python proxy experiments already clarify the translation shape.

Rule:

Native adapters should be added only when they materially improve compatibility, latency, streaming, auth, or UX over routing through a generic OpenAI-compatible backend or LiteLLM.

### Tier 2: Common Cloud Speech Providers

Goal: add native adapters where there is clear demand and no better generic path.

STT candidates:

- Groq Whisper
- AssemblyAI
- Speechmatics
- Soniox
- Azure Speech
- Google Speech-to-Text
- AWS Transcribe
- Cloudflare Workers AI Whisper
- NVIDIA Riva

TTS candidates:

- Cartesia
- Deepgram Aura
- Azure Speech
- Google Cloud TTS
- AWS Polly
- PlayHT
- Resemble AI
- Fish Audio
- Rime
- LMNT
- NVIDIA Riva

Selection criteria:

- user demand
- provider quality/latency
- pricing/free tier usefulness
- streaming support
- stable API
- whether LiteLLM already solves it well enough
- whether a generic OpenAI-compatible adapter can already cover it

### Tier 3: Discovery And Advanced Policy

Goal: make providers easier to inspect and route intelligently.

Features:

- `GET /v1/voices`
- `GET /v1/providers`
- `GET /v1/profiles`
- provider model discovery
- provider voice discovery
- provider health cache
- per-provider circuit breaker
- per-provider cost metadata
- policy tags like `local`, `cloud`, `cheap`, `premium`, `low_latency`

Example:

```yaml
providers:
  cartesia:
    type: cartesia_tts
    api_key_env: CARTESIA_API_KEY
    tags: [cloud, low_latency]

  speaches:
    type: openai_audio
    base_url: http://127.0.0.1:8000/v1
    tags: [local, private]
```

### Tier 4: Realtime

Goal: evaluate OpenAI Realtime-compatible routing without compromising the HTTP proxy.

Realtime is intentionally later because it changes the product shape:

- WebSocket sessions
- bidirectional audio streaming
- partial transcripts
- turn detection
- barge-in/interruptions
- provider-specific session state

Initial realtime candidates:

- OpenAI Realtime
- Speaches Realtime
- LiteLLM realtime-compatible paths, if mature

Realtime should likely be a separate adapter path or package rather than bolted onto the HTTP route-chain MVP.

## Provider Types

### `openai_audio`

Combined OpenAI-compatible STT/TTS backend.

Expected endpoints:

- `POST {base_url}/audio/transcriptions`
- `POST {base_url}/audio/speech`

Optional endpoints:

- `GET {base_url}/models`
- `GET {base_url}/voices`
- provider-specific health endpoint

Use for:

- Speaches
- Vox Box
- LiteLLM
- OpenAI-compatible local stacks

### `openai_stt`

OpenAI-compatible STT-only backend.

Expected endpoint:

- `POST {base_url}/audio/transcriptions`

Use for:

- Whisper-compatible servers
- hosted OpenAI-compatible transcription endpoints

### `openai_tts`

OpenAI-compatible TTS-only backend.

Expected endpoint:

- `POST {base_url}/audio/speech`

Use for:

- Kokoro servers
- Chatterbox servers
- OpenAI-compatible TTS servers

### `deepgram_stt`

Native STT adapter that translates OpenAI transcription requests to Deepgram `/v1/listen`.

OpenAI request fields to map:

- `file`
- `model`
- `language`
- `prompt`, likely as keywords/context where appropriate
- `response_format`

Deepgram options to expose:

- `model`
- `language`
- `smart_format`
- `punctuate`
- `diarize`, later
- `keywords`, later

### `elevenlabs_tts`

Native TTS adapter that translates OpenAI speech requests to ElevenLabs TTS.

OpenAI request fields to map:

- `input`
- `model`
- `voice`
- `response_format`
- `speed`

ElevenLabs options to expose:

- `model`
- `output_format`
- `voice_settings`
- voice aliases

MVP should prefer provider-native MP3 output. PCM/WAV transcoding should not be in the core request path unless later justified.

## Alias Shape

Aliases should let clients use portable names.

```yaml
aliases:
  models:
    whisper-1:
      deepgram: nova-3
      speaches: Systran/faster-distil-whisper-small.en
      voxbox: whisper-large-v3

    tts-1:
      elevenlabs: eleven_turbo_v2_5
      speaches: speaches-ai/Kokoro-82M-v1.0-ONNX
      voxbox: cosyvoice

  voices:
    assistant:
      elevenlabs: ELEVENLABS_VOICE_ID_HERE
      speaches: af_heart
      voxbox: English Female
```

Resolution order:

1. Use provider-specific alias if present.
2. Use request value directly if provider accepts direct IDs.
3. Use provider default if configured.
4. Fail clearly if no usable model/voice exists.

## Fallback Shape

Fallback should be route-chain based and bounded.

```yaml
profiles:
  hybrid:
    stt: [deepgram, speaches]
    tts: [elevenlabs, speaches]

fallback:
  fallback_on_statuses: [408, 429, 500, 502, 503, 504]
  max_attempts_per_request: 2
  retry_timeouts: false
```

Provider failures should produce logs like:

```text
modality=tts profile=hybrid attempted=elevenlabs,speaches winner=speaches fallback_reason=provider_503 latency_ms=840
```

## First Implementation Order

1. Config types for providers, profiles, aliases, server limits, and fallback.
2. Generic `openai_audio`, `openai_stt`, and `openai_tts` adapters.
3. Route-chain selection and bounded fallback.
4. Mock-provider tests for STT/TTS success, timeout, and fallback.
5. Speaches or Vox Box local validation through `openai_audio`.
6. VoiceMode/OpenAI SDK docs against the generic adapter path.
7. Deepgram native adapter.
8. ElevenLabs native adapter.
9. Provider discovery endpoints.
10. Additional cloud-native adapters based on demand.

## Non-Goals

Provider support should not pull `voicemux` into these areas early:

- provider SDK dependency sprawl
- local model downloading
- audio transcoding
- chat-completions proxying
- realtime session orchestration
- billing dashboard
- hosted SaaS control plane
