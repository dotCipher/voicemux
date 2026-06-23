# voicemux

OpenAI-compatible STT/TTS router for local and cloud speech providers.

`voicemux` is a small, self-hostable infrastructure layer for speech systems. It exposes stable OpenAI-compatible audio endpoints, then routes each request to providers such as Deepgram, ElevenLabs, OpenAI, Speaches, Vox Box, Kokoro, Whisper-compatible servers, LiteLLM, and other local or cloud backends.

It is the routing layer between your clients and your speech providers: a single OpenAI-compatible endpoint in front, many interchangeable backends behind it, with profiles, aliases, and cloud-to-local fallback in between.

```text
OpenAI-compatible client
  -> voicemux
    -> Deepgram (STT)
    -> ElevenLabs (TTS)
    -> Speaches / Vox Box / LiteLLM
    -> local Kokoro / Whisper servers
    -> other OpenAI-compatible audio backends
```

## Why

Speech stacks are fragmented. One app supports OpenAI TTS, another supports Deepgram STT, another talks to a local Whisper server, and each has its own provider plugins. Switching providers usually means changing app-specific settings, rewriting integration code, or spreading API keys across multiple tools.

`voicemux` makes speech routing boring:

- One OpenAI-compatible STT/TTS API for every client.
- Many local and cloud backends behind it.
- Cloud-first routing with automatic local fallback.
- Consistent voice and model aliases across providers.
- Profiles for privacy, latency, cost, and quality.
- Centralized routing, credentials, and observability.

## Quick Start

Run from source against the example config:

```bash
cargo run -- --config examples/voicemux.yaml
```

Check it is up:

```bash
curl http://127.0.0.1:8787/health
```

Synthesize speech through the active profile:

```bash
curl -X POST http://127.0.0.1:8787/v1/audio/speech \
  -H 'content-type: application/json' \
  -d '{"model":"tts-1","voice":"assistant","input":"voicemux is online"}' \
  --output speech.mp3
```

Transcribe audio through the active profile:

```bash
curl -X POST http://127.0.0.1:8787/v1/audio/transcriptions \
  -F "model=whisper-1" \
  -F "file=@speech.mp3;type=audio/mpeg"
```

See [`docs/INSTALL.md`](docs/INSTALL.md) for prebuilt binaries and running `voicemux` as a background service on macOS, Linux, and Windows.

## How It Works

`voicemux` keeps the OpenAI-compatible API at the edge and resolves each request against a configured profile:

1. A client calls `/v1/audio/transcriptions` or `/v1/audio/speech`.
2. `voicemux` selects the active profile (or one named in the request) and builds an ordered route chain of providers.
3. Model and voice names are resolved to provider-specific values through aliases.
4. The first provider is attempted; on a configured failure it falls back to the next provider in the chain, bounded by `fallback.max_attempts_per_request`.
5. The response is returned in OpenAI-compatible shape, with `X-Voicemux-*` headers describing the profile, provider, route, model, and voice that served it.

Native adapters translate OpenAI-compatible requests into provider-specific APIs (for example Deepgram `/v1/listen` and ElevenLabs text-to-speech), while OpenAI-compatible backends are proxied directly.

## Scope

`voicemux` is intentionally small, focused, and composable. It is an audio router, not a model server, voice-agent framework, dictation app, or general-purpose LLM gateway. It does not run or download speech models, orchestrate agents, handle wake words or hotkeys, proxy chat completions, or expose a realtime WebSocket API. Those concerns belong to the tools `voicemux` routes to.

In scope:

- `POST /v1/audio/transcriptions`
- `POST /v1/audio/speech`
- `GET /v1/providers`
- `POST /v1/route/dry-run`
- `GET /health`
- STT and TTS route chains with cloud-to-local fallback
- Voice aliases and model aliases
- Local/cloud profiles
- Generic OpenAI-compatible provider adapters
- Native provider adapters where they materially improve compatibility

## Configuration

A profile is an ordered route chain per modality. Earlier providers are preferred; later providers are fallbacks.

```yaml
active_profile: hybrid

profiles:
  hybrid:
    stt: [deepgram, local_whisper]
    tts: [elevenlabs, local_kokoro]
  local:
    stt: [local_whisper]
    tts: [local_kokoro]

providers:
  deepgram:
    type: deepgram_stt
    api_key_env: DEEPGRAM_API_KEY
    model: nova-3
  elevenlabs:
    type: elevenlabs_tts
    api_key_env: ELEVENLABS_API_KEY
    model: eleven_turbo_v2_5
  local_whisper:
    type: openai_stt
    base_url: http://127.0.0.1:2022/v1
  local_kokoro:
    type: openai_tts
    base_url: http://127.0.0.1:8880/v1

aliases:
  voices:
    assistant:
      elevenlabs: ELEVENLABS_VOICE_ID_HERE
      local_kokoro: af_sky
  models:
    whisper-1:
      deepgram: nova-3
      local_whisper: whisper-1
    tts-1:
      elevenlabs: eleven_turbo_v2_5
      local_kokoro: tts-1
```

Clients keep sending stable names like `assistant`, `tts-1`, and `whisper-1`; `voicemux` maps them per provider. With the `hybrid` profile above, TTS prefers ElevenLabs and falls back to local Kokoro, while STT prefers Deepgram and falls back to local Whisper.

The full annotated example lives in [`examples/voicemux.yaml`](examples/voicemux.yaml) and is exercised by the test suite.

## Stack

`voicemux` is built to be fast, predictable, and easy to operate as always-on local infrastructure.

- Rust for the routing service.
- Tokio for the async runtime.
- Axum on Hyper for the HTTP server.
- Reqwest for outbound provider calls.
- Serde for config and API models.
- Tower / tower-http for timeouts, body limits, and middleware.
- Tracing for structured logs and request spans.
- Rustls for TLS.

Why Rust: low overhead for an always-on proxy, strong async performance for concurrent STT/TTS requests, good streaming primitives for binary audio bodies, predictable memory usage, and easy single-binary distribution. Details are in [`docs/STACK.md`](docs/STACK.md).

Performance goals:

- Stream TTS responses when providers support streaming.
- Avoid buffering audio bodies unless a provider requires it.
- Bound request sizes and timeouts.
- Keep fallback decisions cheap and deterministic.
- Avoid global locks on the request path.

## Positioning

`voicemux` is designed to complement existing tools, not replace them.

- Speaches and Vox Box are local OpenAI-compatible speech model servers; `voicemux` routes to them.
- LiteLLM is a broad provider gateway; `voicemux` can route to it or stay smaller for audio-only deployments.
- Pipecat and LiveKit Agents are voice-agent frameworks; `voicemux` can centralize audio policy for projects built with them.
- VoiceMode is a voice client/workflow layer; `voicemux` can provide its STT/TTS endpoints. See [`docs/VOICEMODE.md`](docs/VOICEMODE.md).

The guiding rule:

> `voicemux` routes speech providers. It does not try to become the speech provider, voice app, or agent framework.

## Compatibility

Clients: any tool that speaks OpenAI-compatible audio APIs, including VoiceMode, the OpenAI Python and JavaScript SDKs, Open WebUI, LibreChat, AnythingLLM, and custom bots or scripts that want a single audio endpoint.

Backends: generic OpenAI-compatible STT, TTS, and combined audio servers, plus native Deepgram STT and ElevenLabs TTS. Verified-friendly backends include Speaches, Vox Box, LiteLLM, Kokoro servers, Whisper-compatible servers, and OpenAI.

## Robustness Goals

`voicemux` should be thin, but not fragile.

- Never log API keys or raw audio.
- Keep provider failures isolated.
- Fall back only when the next provider can satisfy the request.
- Make routing decisions observable via response headers and logs.
- Prefer explicit config over magic.
- Support local-only deployments and cloud-first with local fallback.
- Keep the core dependency footprint small.
- Avoid provider-specific behavior leaking into clients.

## For AI Agents

This repository is structured to be easy for AI coding agents and search tools to understand and extend.

- [`AGENTS.md`](AGENTS.md) is the canonical agent guide: scope, key files, commands, and design principles.
- [`.github/copilot-instructions.md`](.github/copilot-instructions.md) mirrors the core constraints for Copilot-style tools.
- [`docs/`](docs) covers architecture, providers, stack, install, running, and VoiceMode setup.
- [`examples/voicemux.yaml`](examples/voicemux.yaml) is the canonical config shape and is covered by tests.

Common tasks:

- Add a provider: implement an adapter in `src/providers.rs` and route to it in `src/openai.rs`.
- Change routing or fallback: see `src/routing.rs` and the fallback loop in `src/openai.rs`.
- Adjust config schema: see `src/config.rs`.

Before considering a change complete, run:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

## Project Status

Early Rust prototype under active development.

Implemented:

- YAML config parsing and validation.
- Profile-based route planning with model and voice alias resolution.
- `GET /health`, `GET /v1/providers`, and `POST /v1/route/dry-run`.
- OpenAI-compatible `/v1/audio/speech` and `/v1/audio/transcriptions` passthrough.
- Native Deepgram STT and ElevenLabs TTS translation.
- Bounded cloud-to-local route-chain fallback.
- `X-Voicemux-*` response headers for profile, provider, route, model, and voice.

Docs:

- [`ROADMAP.md`](ROADMAP.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/INSTALL.md`](docs/INSTALL.md)
- [`docs/PROVIDERS.md`](docs/PROVIDERS.md)
- [`docs/RUNNING.md`](docs/RUNNING.md)
- [`docs/STACK.md`](docs/STACK.md)
- [`docs/VOICEMODE.md`](docs/VOICEMODE.md)
- [`examples/voicemux.yaml`](examples/voicemux.yaml)

## License

Licensed under the [MIT License](LICENSE).
