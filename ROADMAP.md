# Roadmap

`voicemux` should grow from a fast local OpenAI-compatible audio router into a robust speech policy layer for local and cloud providers.

The open-source core should remain useful on its own. Optional commercial features, if they ever exist, should build around hosted management, team policy, secrets, and observability rather than weakening the local router.

## Product North Star

`voicemux` is a thin but powerful infrastructure layer for STT/TTS routing.

It should provide:

- one OpenAI-compatible audio endpoint for clients
- many local and cloud speech backends
- profiles for routing policy
- aliases for portable model and voice names
- fallback for reliability
- observability for latency, provider choice, and failures
- local-first operation with no required cloud dependency

## Phase 0: Project Foundation

Goal: make the project clear, scoped, and credible before implementation.

Status: mostly complete; implementation is now underway.

Deliverables:

- README with explicit scope and positioning.
- Architecture doc.
- Stack decision doc.
- Provider roadmap.
- Example config.
- Comparison with Speaches, Vox Box, LiteLLM, Pipecat, LiveKit Agents, and VoiceMode.

Non-goals:

- no public claims of realtime support yet
- no model serving
- no provider marketplace positioning
- no hosted-service positioning as the primary identity

## Phase 1: Rust HTTP Router MVP

Goal: prove fast OpenAI-compatible STT/TTS routing with generic backends.

Status: in progress. Config loading, route planning, provider descriptors, dry-run routing, generic OpenAI-compatible STT/TTS passthrough scaffolding, and selected-route response headers are implemented.

Core endpoints:

- `POST /v1/audio/transcriptions`
- `POST /v1/audio/speech`
- `GET /v1/models`
- `GET /health`

Core features:

- Rust/Tokio/Axum service.
- YAML config loading.
- In-memory parsed config.
- Generic `openai_audio` adapter.
- Generic `openai_stt` adapter.
- Generic `openai_tts` adapter.
- Static profiles.
- Ordered route chains.
- Model aliases.
- Voice aliases.
- Provider timeouts.
- Max fallback attempts.
- Structured request logs.
- Response headers for selected provider.

Performance requirements:

- async request path
- provider connection reuse
- streaming TTS response forwarding where possible
- bounded request body size
- no per-request config parsing
- no provider health probing on every request
- no audio transcoding in the MVP path

Validation targets:

- Speaches through `openai_audio`.
- Vox Box through `openai_audio`.
- Local Whisper through `openai_stt`.
- Local Kokoro through `openai_tts`.
- VoiceMode pointing both STT and TTS at `voicemux`.
- OpenAI Python SDK.
- OpenAI JavaScript SDK.

## Phase 2: Native High-Value Providers

Goal: add native provider translation only where it creates clear value.

Adapters:

- `deepgram_stt`
- `elevenlabs_tts`

Features:

- Deepgram `/v1/listen` translation from OpenAI transcription shape.
- ElevenLabs speech generation translation from OpenAI speech shape.
- Provider-specific model/voice mapping.
- Provider-specific health checks.
- Provider-specific timeout defaults.
- Fallback from cloud native providers to local OpenAI-compatible backends.

Guardrails:

- Do not add provider SDKs if plain HTTP is sufficient.
- Do not add broad provider sprawl before route/fallback behavior is excellent.
- Do not add audio transcoding unless the lack of it blocks a major use case.

## Phase 3: Profiles And Policy

Goal: make routing behavior understandable and controllable.

Profiles are named routing policies.

Examples:

```yaml
profiles:
  local:
    stt: [speaches, local_whisper]
    tts: [speaches, local_kokoro]

  hybrid:
    stt: [deepgram, speaches]
    tts: [elevenlabs, speaches]

  private:
    stt: [local_whisper]
    tts: [local_kokoro]
    allow_cloud: false

  premium:
    stt: [deepgram]
    tts: [elevenlabs]
```

Features:

- active default profile
- request header profile override: `X-Voicemux-Profile`
- local-only enforcement
- cloud-provider blocking by profile
- policy tags: `local`, `cloud`, `private`, `cheap`, `premium`, `low_latency`
- request hints: prefer local, cheap, premium, low-latency, or private
- profile-aware logs

High-value use cases:

- one profile for coding
- one profile for meetings
- one profile for private/local work
- one profile for premium cloud quality
- one profile for low-cost operation

## Phase 4: Reliability And Observability

Goal: make `voicemux` operationally trustworthy.

Features:

- cached provider health checks
- provider circuit breakers
- fallback reason tracking
- selected-provider response headers
- dry-run routing endpoint
- config validation CLI
- alias validation
- provider latency histograms
- request counters by provider/profile/modality
- error counters by provider/profile/modality
- optional Prometheus metrics
- optional OpenTelemetry traces

Useful endpoints:

- `GET /v1/providers`
- `GET /v1/profiles`
- `POST /v1/route/dry-run`

Example dry-run request:

```json
{
  "modality": "tts",
  "profile": "hybrid",
  "model": "tts-1",
  "voice": "assistant",
  "response_format": "mp3"
}
```

Example dry-run response:

```json
{
  "profile": "hybrid",
  "route": ["elevenlabs", "speaches"],
  "selected_provider": "elevenlabs",
  "resolved_model": "eleven_turbo_v2_5",
  "resolved_voice": "ELEVENLABS_VOICE_ID_HERE"
}
```

## Phase 5: Discovery And Provider Expansion

Goal: improve ergonomics and add providers based on demand.

Features:

- `GET /v1/voices`
- provider voice discovery
- provider model discovery
- provider capability reporting
- cost metadata
- format support metadata
- streaming support metadata

Provider candidates:

- Cartesia TTS
- Deepgram Aura TTS
- Groq Whisper
- AssemblyAI STT
- Speechmatics STT
- Soniox STT
- Azure Speech STT/TTS
- Google Speech-to-Text and TTS
- AWS Transcribe and Polly
- Cloudflare Workers AI Whisper
- NVIDIA Riva
- PlayHT
- Resemble AI
- Fish Audio
- Rime
- LMNT

Selection criteria:

- user demand
- quality
- latency
- streaming support
- pricing/free tier
- API stability
- whether LiteLLM already handles it well
- whether an OpenAI-compatible adapter already handles it

## Phase 6: Advanced Routing

Goal: route by policy, not just fixed provider order.

Features:

- language-aware STT routing
- length-aware STT routing
- format-aware TTS routing
- streaming-aware TTS routing
- cost-aware routing
- latency-aware routing
- provider A/B tests
- weighted routing
- per-profile budget guards
- per-provider usage caps

Examples:

- English short clips go to local Whisper; multilingual or long audio goes to Deepgram.
- `response_format=wav` routes to a provider with native WAV support.
- Interactive TTS prefers streaming providers.
- Premium profile uses ElevenLabs; cheap profile uses Kokoro or Deepgram Aura.

## Phase 7: Realtime Evaluation

Goal: decide whether realtime belongs in `voicemux` core, a plugin, or a sibling project.

Potential targets:

- OpenAI Realtime
- Speaches Realtime
- LiteLLM realtime-compatible paths, if mature

Research questions:

- Can realtime be routed cleanly across providers with different session semantics?
- Is transparent fallback possible for long-lived WebSocket sessions?
- Should realtime be limited to profile selection and endpoint forwarding?
- Would users prefer direct provider connections for realtime latency?

Likely decision:

- keep realtime out of the HTTP MVP
- revisit after `voicemux` proves value as a normal STT/TTS router

## Optional Commercial / Hosted Roadmap

This is hypothetical and should not weaken the open-source core.

Potential paid layers:

- hosted dashboard/control plane
- team-managed profiles
- secrets vault integration
- config sync across many `voicemux` instances
- SSO/SAML/RBAC
- audit logs
- usage and cost analytics
- provider uptime analytics
- policy approval workflows
- managed cloud fallback
- support and deployment help

OSS should keep:

- local router
- profiles
- aliases
- fallback
- config validation
- logs
- generic OpenAI-compatible adapters
- enough observability for single-node operation

The commercial angle is closer to a control plane for speech routing than a closed hosted-only OpenRouter clone.

## Success Criteria

Early success:

- users can point VoiceMode at `voicemux` and switch between local and cloud profiles
- Speaches/Vox Box work through generic adapters
- fallback is reliable and observable
- aliases make provider switching painless
- local routing overhead is negligible compared with inference time

Broader success:

- self-hosted AI users use `voicemux` as their shared speech endpoint
- agent builders use `voicemux` for provider policy and fallback
- teams use profiles to enforce local/private/cloud routing policy
- provider additions happen through clear adapter families, not ad-hoc glue
