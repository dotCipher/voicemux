param(
    [ValidateSet("install", "start", "stop", "restart", "status", "uninstall")]
    [string]$Action = "install",
    [string]$Bin = "",
    [string]$Config = "",
    [string]$EnvFile = ""
)

$ErrorActionPreference = "Stop"
$TaskName = "voicemux"
$ConfigDir = Join-Path $env:APPDATA "voicemux"
if (-not $Config) { $Config = Join-Path $ConfigDir "voicemux.yaml" }
if (-not $EnvFile) { $EnvFile = Join-Path $ConfigDir "voicemux.env" }
$Runner = Join-Path $ConfigDir "Start-Voicemux.ps1"

function Resolve-VoicemuxBinary {
    if ($Bin) { return $Bin }
    $cmd = Get-Command voicemux.exe -ErrorAction SilentlyContinue
    if (-not $cmd) { $cmd = Get-Command voicemux -ErrorAction SilentlyContinue }
    if (-not $cmd) { throw "voicemux binary not found. Pass -Bin C:\path\to\voicemux.exe or put it on PATH." }
    return $cmd.Source
}

function Write-DefaultConfig {
    New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
    if (-not (Test-Path $Config)) {
@'
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
  elevenlabs:
    type: elevenlabs_tts
    api_key_env: ELEVENLABS_API_KEY
    model: eleven_turbo_v2_5
    output_format: mp3_44100_128
  local_whisper:
    type: openai_stt
    base_url: http://127.0.0.1:2022/v1
    api_key: not-needed
  local_kokoro:
    type: openai_tts
    base_url: http://127.0.0.1:8880/v1
    api_key: not-needed

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
'@ | Set-Content -Encoding UTF8 $Config
    }
    if (-not (Test-Path $EnvFile)) {
@'
# Fill these in for cloud routing. Leave blank for local-only profile use.
DEEPGRAM_API_KEY=
ELEVENLABS_API_KEY=
'@ | Set-Content -Encoding UTF8 $EnvFile
    }
}

function Write-Runner {
    $ResolvedBin = Resolve-VoicemuxBinary
@"
`$ErrorActionPreference = "Stop"
if (Test-Path "$EnvFile") {
    Get-Content "$EnvFile" | ForEach-Object {
        if (`$_ -match '^\s*#' -or `$_ -notmatch '=') { return }
        `$name, `$value = `$_ -split '=', 2
        [Environment]::SetEnvironmentVariable(`$name.Trim(), `$value.Trim(), 'Process')
    }
}
& "$ResolvedBin" --config "$Config"
"@ | Set-Content -Encoding UTF8 $Runner
}

switch ($Action) {
    "install" {
        Write-DefaultConfig
        Write-Runner
        $ActionObj = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$Runner`""
        $Trigger = New-ScheduledTaskTrigger -AtLogOn
        $Principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
        Register-ScheduledTask -TaskName $TaskName -Action $ActionObj -Trigger $Trigger -Principal $Principal -Force | Out-Null
        Start-ScheduledTask -TaskName $TaskName
        Write-Host "voicemux task installed. Config: $Config Env: $EnvFile"
    }
    "start" { Start-ScheduledTask -TaskName $TaskName }
    "stop" { Stop-ScheduledTask -TaskName $TaskName }
    "restart" { Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue; Start-ScheduledTask -TaskName $TaskName }
    "status" { Get-ScheduledTask -TaskName $TaskName | Format-List * }
    "uninstall" { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false }
}
