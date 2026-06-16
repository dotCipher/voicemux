# voicemux

OpenAI-compatible STT/TTS routing for local and cloud voice providers.

`voicemux` is a thin infrastructure layer for speech systems. It exposes stable OpenAI-compatible audio endpoints, then routes requests to providers such as Deepgram, ElevenLabs, OpenAI, Speaches, Vox Box, Kokoro, Whisper-compatible servers, LiteLLM, and other local or cloud backends.

It is not a model server, voice-agent framework, dictation app, or broad LLM gateway. It is the small routing layer between your clients and your speech providers.

```text
OpenAI-compatible client
  -> voicemux
    -> Deepgram
    -> ElevenLabs
    -> Speaches
    -> Vox Box
    -> LiteLLM
    -> local Kokoro / Whisper servers
    -> other OpenAI-compatible audio backends
```

## Why

Speech stacks are fragmented. One app supports OpenAI TTS, another supports Deepgram STT, another supports a local Whisper server, and another has its own provider plugins. Switching providers usually means changing app-specific settings, rewriting integration code, or spreading API keys across multiple tools.

`voicemux` aims to make speech routing boring:

- One OpenAI-compatible STT/TTS API for clients.
- Multiple local and cloud backends behind it.
- Fallback from premium cloud providers to local servers.
- Consistent voice aliases across providers.
- Profiles for privacy, latency, cost, and quality.
- Centralized health, routing, and observability.

## Scope

`voicemux` should be small, focused, and composable.

In scope:

- `POST /v1/audio/transcriptions`
- `POST /v1/audio/speech`
- `GET /v1/models`
- `GET /health`
- STT and TTS route chains
- Provider fallback
- Provider health checks
- Voice aliases
- Model aliases
- Local/cloud profiles
- Generic OpenAI-compatible provider adapters
- Native provider adapters where they materially improve compatibility

Out of scope for the initial project:

- Running speech models directly
- Downloading or managing Hugging Face models
- Full voice-agent orchestration
- Wake words or hotkeys
- Dictation desktop app UX
- Chat-completions proxying
- Broad LLM/image/embedding gateway behavior
- Realtime WebSocket API, until the HTTP routing layer is proven

## Stack

`voicemux` should be fast, predictable, and easy to operate as local infrastructure.

Planned core stack:

- Rust for the routing service.
- Tokio for async runtime.
- Axum or Hyper for HTTP server behavior.
- Reqwest or Hyper client for outbound provider calls.
- Serde for config and API models.
- Tracing for structured logs and request spans.
- Tower middleware for timeouts, limits, and request instrumentation.
- Rustls by default for TLS.

Why Rust:

- Low overhead for an always-on local proxy.
- Strong async performance for concurrent STT/TTS requests.
- Good streaming primitives for binary audio bodies.
- Predictable memory usage compared with a Python web service.
- Easy static binary distribution later.
- Strong type safety for provider adapters, config, and fallback rules.

Python provider shims remain useful as references, but the main `voicemux` service should not be a FastAPI app unless we discover a concrete reason to trade performance and distribution simplicity for iteration speed.

Performance goals:

- Stream TTS responses when providers support streaming.
- Avoid buffering audio bodies unless a provider requires it.
- Bound request sizes and timeouts.
- Keep fallback decisions cheap and deterministic.
- Keep provider health checks asynchronous and cached briefly.
- Avoid global locks on the request path.
- Prefer zero-copy or low-copy request forwarding where practical.

## Positioning

`voicemux` is designed to complement existing tools, not replace them.

- Speaches is an excellent local speech model server. `voicemux` can route to it.
- Vox Box is a local OpenAI-compatible STT/TTS model server. `voicemux` can route to it.
- LiteLLM is a broad provider gateway. `voicemux` can route to it or stay smaller for audio-only deployments.
- Pipecat and LiveKit Agents are voice-agent frameworks. `voicemux` can centralize audio policy for projects built with them.
- VoiceMode is a voice client/workflow layer. `voicemux` can provide its STT/TTS endpoints.

The guiding rule:

> `voicemux` routes speech providers. It does not try to become the speech provider, voice app, or agent framework.

## Example

```yaml
profiles:
  hybrid:
    stt: [deepgram, speaches]
    tts: [elevenlabs, speaches]

  local:
    stt: [speaches]
    tts: [speaches]

providers:
  deepgram:
    type: deepgram_stt
    api_key_env: DEEPGRAM_API_KEY
    model: nova-3

  elevenlabs:
    type: elevenlabs_tts
    api_key_env: ELEVENLABS_API_KEY
    model: eleven_turbo_v2_5

  speaches:
    type: openai_audio
    base_url: http://127.0.0.1:8000/v1

voices:
  assistant:
    elevenlabs: ELEVENLABS_VOICE_ID_HERE
    speaches: af_heart
```

With this shape, clients can point at one local OpenAI-compatible endpoint while `voicemux` handles the provider decision.

## Target Clients

Any client that can speak OpenAI-compatible audio APIs should be a candidate.

Initial targets:

- VoiceMode
- OpenAI Python SDK
- OpenAI JavaScript SDK
- Open WebUI
- LibreChat
- AnythingLLM
- Custom bots and scripts
- Agent prototypes that want a single audio endpoint

## Target Backends

Initial backend categories:

- Generic OpenAI-compatible STT
- Generic OpenAI-compatible TTS
- Generic OpenAI-compatible combined audio servers
- Deepgram STT
- ElevenLabs TTS

Important compatible backends:

- Speaches
- Vox Box
- LiteLLM
- Kokoro servers
- Whisper-compatible servers
- OpenAI

## Robustness Goals

`voicemux` should be thin, but not fragile.

- Never log API keys.
- Keep provider failures isolated.
- Fall back only when the next provider can satisfy the request.
- Make routing decisions observable.
- Prefer explicit config over magic.
- Support local-only deployments.
- Support cloud-first with local fallback.
- Keep the core dependency footprint small.
- Avoid provider-specific behavior leaking into clients.

## AI Coding Agents

This repository is intentionally easy for AI coding agents to inspect and extend.

- [`AGENTS.md`](AGENTS.md) is the canonical agent guide.
- [`.github/copilot-instructions.md`](.github/copilot-instructions.md) mirrors the core constraints for GitHub Copilot-style tools.
- Public docs in `docs/` describe architecture, provider strategy, and stack decisions.
- `examples/voicemux.yaml` is the canonical config shape and is covered by tests.

Agents should run `cargo fmt` and `cargo test` before considering implementation work complete.

## Project Status

Early Rust prototype.

Currently implemented:

- YAML config parsing and validation.
- Profile-based route planning.
- Model and voice alias resolution.
- `GET /health`.
- `GET /v1/providers`.
- `POST /v1/route/dry-run`.
- Generic OpenAI-compatible `/v1/audio/speech` passthrough.
- Generic OpenAI-compatible `/v1/audio/transcriptions` passthrough.
- Native Deepgram STT translation.
- Native ElevenLabs TTS translation.
- `X-Voicemux-*` response headers for selected profile/provider/route metadata.

Current docs:

- [`ROADMAP.md`](ROADMAP.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/PROVIDERS.md`](docs/PROVIDERS.md)
- [`docs/RUNNING.md`](docs/RUNNING.md)
- [`docs/STACK.md`](docs/STACK.md)
- [`docs/VOICEMODE.md`](docs/VOICEMODE.md)
- [`examples/voicemux.yaml`](examples/voicemux.yaml)

## Repository Description

Suggested GitHub repo description:

> OpenAI-compatible STT/TTS router for local and cloud speech providers.
