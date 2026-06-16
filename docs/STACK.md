# Stack Decisions

This document records the intended implementation stack and performance posture for `voicemux`.

## Primary Language

Use Rust for the core service.

`voicemux` is an always-on infrastructure proxy. It handles binary audio payloads, provider routing, fallback, timeouts, and streaming responses. Rust is the best fit for a small, reliable, high-performance service that should be easy to ship as a single binary.

## Runtime And HTTP

Recommended stack:

- Tokio: async runtime.
- Axum: HTTP routing and request extraction.
- Hyper: underlying HTTP primitives and fallback option for lower-level streaming control.
- Reqwest: outbound provider HTTP client for the first implementation.
- Tower: middleware, limits, timeouts, and service composition.
- Rustls: TLS default.

## Config And Data

Recommended crates:

- Serde: serialization/deserialization.
- serde_yaml: YAML config.
- serde_json: OpenAI-compatible request/response bodies.
- config or figment: optional layered config later, if simple serde parsing becomes insufficient.

## Errors And Logging

Recommended crates:

- thiserror: typed library errors.
- anyhow: application bootstrap errors.
- tracing: structured spans and events.
- tracing-subscriber: formatting and filtering.

## Testing

Recommended test approach:

- Unit tests for config parsing, alias resolution, route selection, and fallback policy.
- Mock HTTP providers for adapter behavior.
- Integration tests that run the router against local mock STT/TTS endpoints.
- Golden tests for OpenAI-compatible JSON response shapes.

## Performance Requirements

The MVP should be designed around these requirements:

- Async request handling end to end.
- Streaming TTS responses where supported.
- Bounded request body sizes.
- Configurable provider timeouts.
- Connection reuse for provider clients.
- No per-request config reparsing by default.
- No unnecessary audio decoding, transcoding, or inspection.
- No logging of audio bytes or secrets.

## Non-Goals For The Core Runtime

Avoid these in the initial core service:

- Python web server core.
- Node.js web server core.
- Built-in audio transcoding.
- Built-in model inference.
- Embedded provider SDKs unless plain HTTP is insufficient.
- Persistent database dependency.
- Web UI dependency.

## Why Not Python First

The existing Deepgram and ElevenLabs proxy experiments are Python/FastAPI, which is useful for fast exploration. The production-oriented `voicemux` core has different priorities:

- long-running local service behavior
- low memory overhead
- fast concurrent proxying
- straightforward static distribution
- strict config and adapter types
- fewer runtime dependency issues

Python can still be useful for examples, tests, or comparison scripts, but it should not be the default service runtime.

## Future Options

If the core router is stable, later additions can include:

- optional WASM/plugin adapter boundary
- optional Prometheus metrics endpoint
- optional OpenTelemetry traces
- optional realtime proxy package
- optional CLI helper for config validation
