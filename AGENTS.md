# voicemux Agent Guide

This file helps AI coding agents quickly orient themselves in this repository.

## Project Summary

`voicemux` is an OpenAI-compatible STT/TTS router for local and cloud speech providers.

It is intended to be a small, self-hostable infrastructure layer that routes audio requests across OpenAI-compatible model servers, local services, and native cloud adapters.

## Scope

Build:

- OpenAI-compatible `/v1/audio/transcriptions` and `/v1/audio/speech` endpoints.
- Profile-based routing for STT and TTS.
- Model and voice aliases per provider.
- Generic OpenAI-compatible backend passthrough.
- Native provider adapters where they add clear value.
- Reliability, fallback, observability, and configuration validation.

Avoid:

- LLM chat proxying.
- Voice-agent orchestration.
- Dictation UI features.
- Realtime audio until the core STT/TTS router is solid.
- Vendor-specific behavior in the request edge unless it is hidden behind config or adapters.

## Key Files

- `README.md`: public positioning and quick overview.
- `ROADMAP.md`: implementation phases and priorities.
- `docs/ARCHITECTURE.md`: system design and boundaries.
- `docs/PROVIDERS.md`: provider strategy and adapter tiers.
- `docs/STACK.md`: Rust stack and performance choices.
- `docs/VOICEMODE.md`: VoiceMode setup and profile/voice mapping guidance.
- `examples/voicemux.yaml`: canonical example config used by tests.
- `examples/voicemode.env`: VoiceMode environment example pointing at voicemux.
- `src/config.rs`: typed config parsing and validation.
- `src/routing.rs`: route planning, model aliases, and voice aliases.
- `src/providers.rs`: provider adapter descriptors and factories.
- `src/openai.rs`: OpenAI-compatible endpoint handling.
- `src/main.rs`: Axum app wiring and HTTP routes.

## Commands

Run these before considering a change complete:

```bash
cargo fmt
cargo test
```

Use this during development:

```bash
cargo run -- --config examples/voicemux.yaml
```

## Design Principles

- Keep the OpenAI-compatible API at the edge.
- Keep config explicit and validated at startup.
- Prefer small, composable modules over broad abstractions.
- Make unsupported provider behavior fail clearly.
- Do not add backward compatibility until there is shipped behavior to preserve.
- Avoid adding dependencies unless they directly support the router MVP.
- Keep personal planning notes out of git; public docs should be useful to contributors and agents.

## Current MVP Direction

The near-term implementation path is:

1. Configuration and route planning.
2. Generic OpenAI-compatible TTS passthrough.
3. Generic OpenAI-compatible STT passthrough.
4. Native Deepgram STT and ElevenLabs TTS adapters.
5. Fallback and health-aware routing.

## Agent Notes

- The repository may be worked on by multiple agents or humans at once. Do not revert unrelated changes.
- Keep changes small and testable.
- Prefer adding tests near the module being changed.
- If adding a new public behavior, update `README.md`, `ROADMAP.md`, or `docs/` as appropriate.
