# Install And Release

`voicemux` is a single native binary. It can run in a terminal or as a per-user background service.

## macOS And Linux: Recommended Install

The release installer queries the current GitHub Release, verifies its SHA-256 checksum, and installs the binary to `~/.local/bin` by default.

```bash
git clone https://github.com/dotCipher/voicemux.git
cd voicemux
sh scripts/install.sh
```

Use another install prefix if needed:

```bash
PREFIX=/usr/local sh scripts/install.sh
```

Install a specific release instead of the latest:

```bash
VERSION=v0.1.2 sh scripts/install.sh
```

The installer supports these release targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

Linux binaries require a glibc-based distribution. Build from source for other libc environments.

If `~/.local/bin` is not already on `PATH`, add it before invoking `voicemux`.

## Configure And Run As A Service

Install the user-level service after the binary is available:

```bash
sh scripts/install-service.sh install --bin "$HOME/.local/bin/voicemux"
```

The installer creates these files without overwriting an existing config:

- Config: `~/.config/voicemux/voicemux.yaml`
- Environment: `~/.config/voicemux/voicemux.env`
- macOS launchd agent: `~/Library/LaunchAgents/com.dotcipher.voicemux.plist`
- Linux systemd user unit: `~/.config/systemd/user/voicemux.service`

`voicemux` intentionally binds only to loopback addresses because its OpenAI-compatible endpoints have no request authentication. To use it from another machine, keep `voicemux` on loopback and put an authenticated reverse proxy in front of it.

For the recommended cloud-first profile, add your provider keys to `~/.config/voicemux/voicemux.env`, set an ElevenLabs voice ID for the `assistant` alias in `~/.config/voicemux/voicemux.yaml`, then restart:

```bash
sh scripts/install-service.sh restart
curl http://127.0.0.1:8787/health
```

Manage the service with:

```bash
sh scripts/install-service.sh status
sh scripts/install-service.sh restart
sh scripts/install-service.sh stop
sh scripts/install-service.sh start
sh scripts/install-service.sh uninstall
```

## Windows

Download `voicemux-x86_64-pc-windows-msvc.zip` and its matching `.sha256` file from the [latest release](https://github.com/dotCipher/voicemux/releases/latest). Verify the checksum, extract `voicemux.exe`, and put it on `PATH`.

Clone the repository to use the service script, then create a per-user Scheduled Task:

```powershell
.\scripts\install-service.ps1 install -Bin "C:\path\to\voicemux.exe"
```

The script creates `%APPDATA%\voicemux\voicemux.yaml`, `%APPDATA%\voicemux\voicemux.env`, and `%APPDATA%\voicemux\Start-Voicemux.ps1`.

Manage the service with:

```powershell
.\scripts\install-service.ps1 status
.\scripts\install-service.ps1 restart
.\scripts\install-service.ps1 stop
.\scripts\install-service.ps1 start
.\scripts\install-service.ps1 uninstall
```

## From Source

Install the binary from a checkout:

```bash
cargo install --path . --locked
```

Or run it during development:

```bash
cargo run -- --config examples/voicemux.yaml
```

## Release Process

1. Update `Cargo.toml` and `Cargo.lock` to the release version.
2. Commit the version and release notes.
3. Create and push an annotated matching tag:

```bash
git tag -a v0.1.3 -m "v0.1.3"
git push origin main v0.1.3
```

GitHub Actions validates that the tag matches the Cargo package version, builds archives and SHA-256 checksums for every supported target, and attaches them to the GitHub Release.
