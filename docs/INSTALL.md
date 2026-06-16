# Install And Release

`voicemux` is designed to be installed as a small native binary and optionally managed as a user-level background service.

## Recommended Release Strategy

Research summary:

- `dist` / `cargo-dist` is the strongest Rust-native release tool when we want generated GitHub Releases, archives, installer scripts, Homebrew, npm wrappers, MSI, and eventually updater support.
- `cargo-binstall` is useful once GitHub Releases exist because it can install Rust binaries without building from source.
- `service-manager` is a good future in-binary service-management library for systemd, launchd, Windows service managers, OpenRC, and FreeBSD rc.d, but starting with explicit OS-native scripts keeps the binary smaller and easier to debug.

Current implementation:

- CI runs `cargo fmt`, `cargo clippy`, and `cargo test` on Linux, macOS, and Windows.
- Tag pushes like `v0.1.0` build release archives for Linux, macOS, and Windows using GitHub Actions.
- macOS/Linux service installation is managed by `scripts/install-service.sh`.
- Windows autostart is managed by `scripts/install-service.ps1` using a Scheduled Task.

## From Source

```bash
cargo install --path . --locked
```

Or run directly while developing:

```bash
cargo run -- --config examples/voicemux.yaml
```

## From GitHub Releases

After a tagged release is published, download the archive for your platform from GitHub Releases and place `voicemux` on your `PATH`.

Release targets currently built:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

## Service Install: macOS And Linux

Install as a user-level background service:

```bash
scripts/install-service.sh install --bin "$(command -v voicemux)"
```

Manage it:

```bash
scripts/install-service.sh status
scripts/install-service.sh restart
scripts/install-service.sh stop
scripts/install-service.sh start
scripts/install-service.sh uninstall
```

Files created:

- Config: `~/.config/voicemux/voicemux.yaml`
- Env file: `~/.config/voicemux/voicemux.env`
- macOS launchd plist: `~/Library/LaunchAgents/com.dotcipher.voicemux.plist`
- Linux systemd user unit: `~/.config/systemd/user/voicemux.service`

Fill in secrets in `~/.config/voicemux/voicemux.env`:

```env
DEEPGRAM_API_KEY=...
ELEVENLABS_API_KEY=...
```

Then restart:

```bash
scripts/install-service.sh restart
```

## Service Install: Windows

Install as a user-level Scheduled Task:

```powershell
.\scripts\install-service.ps1 install -Bin "C:\path\to\voicemux.exe"
```

Manage it:

```powershell
.\scripts\install-service.ps1 status
.\scripts\install-service.ps1 restart
.\scripts\install-service.ps1 stop
.\scripts\install-service.ps1 start
.\scripts\install-service.ps1 uninstall
```

Files created under `%APPDATA%\voicemux`:

- `voicemux.yaml`
- `voicemux.env`
- `Start-Voicemux.ps1`

## Release Process

1. Update `Cargo.toml` version.
2. Commit the version/docs changes.
3. Tag the release:

```bash
git tag v0.1.0
git push origin main --tags
```

GitHub Actions will build archives, checksums, and attach them to the GitHub Release.

## Future Improvements

- Add `dist` once release needs grow beyond basic GitHub archive publishing.
- Add Homebrew formula publishing.
- Add `cargo-binstall` metadata after crates.io publishing.
- Add in-binary `voicemux service install/start/stop/status` using `service-manager` if scripts become too hard to maintain.
- Add signed artifacts and GitHub provenance attestations.
