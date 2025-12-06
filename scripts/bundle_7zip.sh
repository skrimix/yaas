#!/usr/bin/env bash
set -euo pipefail

# Bundle 7-Zip binary for the current platform
# Usage: bundle_7zip.sh <destination_directory>

SEVENZIP_VERSION="25.01"
DEST="${1:?Usage: bundle_7zip.sh <destination_directory>}"

case "$(uname -s)" in
  Linux*)
    URL="https://github.com/ip7z/7zip/releases/download/${SEVENZIP_VERSION}/7z${SEVENZIP_VERSION//./}-linux-x64.tar.xz"
    BINARY="7zzs"
    ;;
  Darwin*)
    URL="https://github.com/ip7z/7zip/releases/download/${SEVENZIP_VERSION}/7z${SEVENZIP_VERSION//./}-mac.tar.xz"
    BINARY="7zz"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    URL="https://github.com/ip7z/7zip/releases/download/${SEVENZIP_VERSION}/7z${SEVENZIP_VERSION//./}-extra.7z"
    BINARY="7za.exe"
    ;;
  *)
    echo "Unsupported platform: $(uname -s)" >&2
    exit 1
    ;;
esac

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "Downloading 7-Zip ${SEVENZIP_VERSION}..."
curl -fsSL -o "$tmpdir/archive" "$URL"

echo "Extracting..."
cd "$tmpdir"
case "$URL" in
  *.tar.xz) tar -xJf archive ;;
  *.7z)     7z x -y archive >/dev/null ;;
esac

found=$(find . -type f -name "$BINARY" | head -n1)
if [[ -z "$found" ]]; then
  echo "Binary $BINARY not found in archive" >&2
  exit 1
fi

mkdir -p "$DEST"
install -m755 "$found" "$DEST/$BINARY"
echo "Installed $BINARY to $DEST/"
