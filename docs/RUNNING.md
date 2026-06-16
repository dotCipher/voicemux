# Running voicemux

This is the local operations checklist for testing `voicemux` as a VoiceMode STT/TTS router.

## Start

```bash
DEEPGRAM_API_KEY=... ELEVENLABS_API_KEY=... cargo run -- --config examples/voicemux.yaml
```

The example config listens on `127.0.0.1:8787`.

## Health

```bash
curl http://127.0.0.1:8787/health
curl http://127.0.0.1:8787/v1/providers
```

## Dry Run

Check routing before sending audio:

```bash
curl -s http://127.0.0.1:8787/v1/route/dry-run \
  -H 'content-type: application/json' \
  -d '{"modality":"tts","model":"tts-1","voice":"assistant"}'
```

## VoiceMode

Point VoiceMode at `voicemux`:

```env
VOICEMODE_STT_BASE_URLS=http://127.0.0.1:8787/v1
VOICEMODE_TTS_BASE_URLS=http://127.0.0.1:8787/v1
VOICEMODE_VOICES=assistant
```

See [`docs/VOICEMODE.md`](VOICEMODE.md) for profile and voice alias details.

## Request Limits

`server.max_body_bytes` and `server.request_timeout_seconds` in `voicemux.yaml` are enforced by the HTTP server.

The default example values are intended for local VoiceMode usage:

- `max_body_bytes: 26214400`
- `request_timeout_seconds: 120`

## Response Metadata

Proxied responses include routing metadata headers:

- `x-voicemux-profile`
- `x-voicemux-provider`
- `x-voicemux-route`
- `x-voicemux-model`
- `x-voicemux-voice`

These are useful when confirming whether requests are hitting Deepgram, ElevenLabs, or local providers.
