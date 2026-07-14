#!/usr/bin/env sh
set -eu

REPO="dotCipher/voicemux"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
VERSION="${VERSION:-}"

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

case "$(uname -s)" in
  Darwin)
    case "$(uname -m)" in
      arm64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) fail "unsupported macOS architecture: $(uname -m)" ;;
    esac
    ;;
  Linux)
    case "$(uname -m)" in
      aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
      x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
      *) fail "unsupported Linux architecture: $(uname -m)" ;;
    esac
    ;;
  *) fail "unsupported operating system: $(uname -s)" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required to install voicemux"
command -v tar >/dev/null 2>&1 || fail "tar is required to install voicemux"
command -v install >/dev/null 2>&1 || fail "install is required to install voicemux"

if [ -z "$VERSION" ]; then
  VERSION="$(
    curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
      sed -n 's/^[[:space:]]*"tag_name": "\([^"]*\)",$/\1/p'
  )"
fi
[ -n "$VERSION" ] || fail "could not determine the latest voicemux release"

BASE_URL="https://github.com/$REPO/releases/download/$VERSION"

ARCHIVE="voicemux-$TARGET.tar.gz"
CHECKSUM="voicemux-$TARGET.sha256"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT INT TERM

curl -fsSL "$BASE_URL/$ARCHIVE" -o "$TMPDIR/$ARCHIVE"
curl -fsSL "$BASE_URL/$CHECKSUM" -o "$TMPDIR/$CHECKSUM"

(
  cd "$TMPDIR"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$CHECKSUM"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$CHECKSUM"
  else
    fail "sha256sum or shasum is required to verify the release archive"
  fi
)

tar -xzf "$TMPDIR/$ARCHIVE" -C "$TMPDIR"
mkdir -p "$BIN_DIR"
install -m 755 "$TMPDIR/voicemux" "$BIN_DIR/voicemux"

printf 'Installed voicemux to %s\n' "$BIN_DIR/voicemux"
case ":${PATH}:" in
  *":$BIN_DIR:"*) ;;
  *) printf 'Add %s to PATH before running voicemux.\n' "$BIN_DIR" ;;
esac
