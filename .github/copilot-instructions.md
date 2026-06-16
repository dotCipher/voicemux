# Copilot Instructions

`voicemux` is a Rust/Axum OpenAI-compatible STT/TTS router for local and cloud speech providers.

Follow `AGENTS.md` first. It is the canonical agent guide for this repository.

Important constraints:

- Keep the project audio-only for now.
- Do not add LLM chat, realtime, or voice-agent orchestration features unless the roadmap changes.
- Preserve OpenAI-compatible request and response shapes at the API edge.
- Prefer minimal, well-tested changes.
- Run `cargo fmt` and `cargo test` before finishing implementation work.
