#!/usr/bin/env sh
set -eu

ACTION="${1:-install}"
if [ "$#" -gt 0 ]; then
  shift
fi

BIN="${VOICEMUX_BIN:-}"
CONFIG_DIR="${VOICEMUX_CONFIG_DIR:-$HOME/.config/voicemux}"
CONFIG="$CONFIG_DIR/voicemux.yaml"
ENV_FILE="$CONFIG_DIR/voicemux.env"
RUNNER="$CONFIG_DIR/run-voicemux.sh"
LABEL="com.dotcipher.voicemux"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bin)
      BIN="$2"
      shift 2
      ;;
    --config)
      CONFIG="$2"
      shift 2
      ;;
    --env)
      ENV_FILE="$2"
      shift 2
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

detect_bin() {
  if [ -n "$BIN" ]; then
    printf '%s\n' "$BIN"
    return
  fi

  command -v voicemux 2>/dev/null || {
    printf 'voicemux binary not found. Pass --bin /path/to/voicemux or put it on PATH.\n' >&2
    exit 1
  }
}

write_default_config() {
  mkdir -p "$(dirname "$CONFIG")"
  if [ ! -f "$CONFIG" ]; then
    cat >"$CONFIG" <<'YAML'
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
    language: auto
    tags: [cloud, low_latency]
  elevenlabs:
    type: elevenlabs_tts
    api_key_env: ELEVENLABS_API_KEY
    model: eleven_turbo_v2_5
    output_format: mp3_44100_128
    tags: [cloud, premium]
  local_whisper:
    type: openai_stt
    base_url: http://127.0.0.1:2022/v1
    api_key: not-needed
    tags: [local, private]
  local_kokoro:
    type: openai_tts
    base_url: http://127.0.0.1:8880/v1
    api_key: not-needed
    tags: [local, private]

aliases:
  models:
    whisper-1:
      deepgram: nova-3
      local_whisper: whisper-1
    tts-1:
      elevenlabs: eleven_turbo_v2_5
      local_kokoro: tts-1
  voices:
    assistant:
      elevenlabs: ELEVENLABS_VOICE_ID_HERE
      local_kokoro: af_sky

server:
  host: 127.0.0.1
  port: 8787
  max_body_bytes: 26214400
  request_timeout_seconds: 120
YAML
  fi

  if [ ! -f "$ENV_FILE" ]; then
    cat >"$ENV_FILE" <<'ENV'
# Fill these in for cloud routing. Leave blank for local-only profile use.
DEEPGRAM_API_KEY=
ELEVENLABS_API_KEY=
ENV
    chmod 600 "$ENV_FILE"
  fi
}

write_runner() {
  mkdir -p "$CONFIG_DIR"
  cat >"$RUNNER" <<EOF
#!/usr/bin/env sh
set -eu
if [ -f "$ENV_FILE" ]; then
  set -a
  . "$ENV_FILE"
  set +a
fi
exec "$(detect_bin)" --config "$CONFIG"
EOF
  chmod 700 "$RUNNER"
}

install_macos() {
  plist="$HOME/Library/LaunchAgents/$LABEL.plist"
  mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs/voicemux"
  cat >"$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/sh</string>
    <string>$RUNNER</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/voicemux/voicemux.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/voicemux/voicemux.err.log</string>
</dict>
</plist>
EOF
  launchctl bootout "gui/$(id -u)" "$plist" >/dev/null 2>&1 || true
  launchctl bootstrap "gui/$(id -u)" "$plist"
  launchctl kickstart -k "gui/$(id -u)/$LABEL"
}

install_linux() {
  service_dir="$HOME/.config/systemd/user"
  service_file="$service_dir/voicemux.service"
  mkdir -p "$service_dir"
  bin_path="$(detect_bin)"
  cat >"$service_file" <<EOF
[Unit]
Description=voicemux OpenAI-compatible STT/TTS router
After=network-online.target

[Service]
Type=simple
EnvironmentFile=-$ENV_FILE
ExecStart="$bin_path" --config "$CONFIG"
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
  systemctl --user daemon-reload
  systemctl --user enable --now voicemux.service
}

case "$ACTION" in
  install)
    write_default_config
    write_runner
    case "$(uname -s)" in
      Darwin) install_macos ;;
      Linux) install_linux ;;
      *) printf 'unsupported OS for install-service.sh\n' >&2; exit 1 ;;
    esac
    printf 'voicemux service installed. Config: %s Env: %s\n' "$CONFIG" "$ENV_FILE"
    ;;
  start)
    case "$(uname -s)" in
      Darwin) launchctl kickstart -k "gui/$(id -u)/$LABEL" ;;
      Linux) systemctl --user start voicemux.service ;;
    esac
    ;;
  stop)
    case "$(uname -s)" in
      Darwin) launchctl kill TERM "gui/$(id -u)/$LABEL" ;;
      Linux) systemctl --user stop voicemux.service ;;
    esac
    ;;
  restart)
    "$0" stop || true
    "$0" start
    ;;
  status)
    case "$(uname -s)" in
      Darwin) launchctl print "gui/$(id -u)/$LABEL" ;;
      Linux) systemctl --user status voicemux.service ;;
    esac
    ;;
  uninstall)
    case "$(uname -s)" in
      Darwin)
        plist="$HOME/Library/LaunchAgents/$LABEL.plist"
        launchctl bootout "gui/$(id -u)" "$plist" >/dev/null 2>&1 || true
        rm -f "$plist"
        ;;
      Linux)
        systemctl --user disable --now voicemux.service >/dev/null 2>&1 || true
        rm -f "$HOME/.config/systemd/user/voicemux.service"
        systemctl --user daemon-reload
        ;;
    esac
    ;;
  *)
    printf 'usage: %s [install|start|stop|restart|status|uninstall] [--bin PATH] [--config PATH] [--env PATH]\n' "$0" >&2
    exit 2
    ;;
esac
