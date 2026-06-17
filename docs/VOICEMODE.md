# VoiceMode Setup

`voicemux` can replace separate OpenAI-compatible proxy repos for VoiceMode by exposing one local OpenAI-compatible audio endpoint:

- VoiceMode sends STT requests to `voicemux` `/v1/audio/transcriptions`.
- VoiceMode sends TTS requests to `voicemux` `/v1/audio/speech`.
- `voicemux` chooses Deepgram, ElevenLabs, or local OpenAI-compatible providers from one centralized profile.

## Recommended Shape

Use `voicemux` as the only VoiceMode audio endpoint:

```env
VOICEMODE_STT_BASE_URLS=http://127.0.0.1:8787/v1
VOICEMODE_TTS_BASE_URLS=http://127.0.0.1:8787/v1
VOICEMODE_VOICES=assistant
```

Then control provider choice in `examples/voicemux.yaml` or your own config:

```yaml
active_profile: hybrid

profiles:
  hybrid:
    stt: [deepgram, local_whisper]
    tts: [elevenlabs, local_kokoro]

  local:
    stt: [local_whisper]
    tts: [local_kokoro]
```

With this setup, VoiceMode does not need to know about Deepgram, ElevenLabs, Kokoro, or Whisper directly. The `hybrid` profile is cloud-first with local fallback: Deepgram falls back to local Whisper for STT, and ElevenLabs falls back to local Kokoro for TTS.

## Voice Mapping

VoiceMode sends one voice name. `voicemux` maps that name per provider:

```yaml
aliases:
  voices:
    assistant:
      elevenlabs: ELEVENLABS_VOICE_ID_HERE
      local_kokoro: af_sky
```

Set `VOICEMODE_VOICES=assistant`, then switch `active_profile` between `hybrid`, `cloud`, and `local` without changing VoiceMode.

Deepgram is currently STT-only in `voicemux`, so voice aliases apply to TTS providers such as ElevenLabs, Kokoro, Speaches, or Vox Box. If Deepgram TTS is added later, it can receive its own value under the same alias.

## Required Secrets

Native cloud providers read keys from environment variables configured in `voicemux.yaml`:

```yaml
providers:
  deepgram:
    type: deepgram_stt
    api_key_env: DEEPGRAM_API_KEY

  elevenlabs:
    type: elevenlabs_tts
    api_key_env: ELEVENLABS_API_KEY
```

Run `voicemux` with those variables set:

```bash
DEEPGRAM_API_KEY=... ELEVENLABS_API_KEY=... cargo run -- --config examples/voicemux.yaml
```

## Native Providers

Current native translations:

- `deepgram_stt`: accepts OpenAI multipart transcription requests and calls Deepgram `/v1/listen`.
- `elevenlabs_tts`: accepts OpenAI speech JSON requests and calls ElevenLabs text-to-speech.

Local OpenAI-compatible providers still work through passthrough:

- `openai_stt`
- `openai_tts`
- `openai_audio`

## Profile Behavior

VoiceMode does not need to send a profile. `voicemux` uses `active_profile` by default.

For testing, `voicemux` also accepts an internal `profile` field on JSON speech requests and multipart transcription requests. This is mainly for direct clients and debugging; the normal VoiceMode path should use `active_profile`.
