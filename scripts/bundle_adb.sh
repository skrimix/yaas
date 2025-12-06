#!/usr/bin/env bash
set -euo pipefail

# Bundle ADB binary for the current platform
# Usage: bundle_adb.sh <destination_directory>

DEST="${1:?Usage: bundle_adb.sh <destination_directory>}"

case "$(uname -s)" in
  Linux*)   OS="linux" ;;
  Darwin*)  OS="darwin" ;;
  MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
  *)
    echo "Unsupported platform: $(uname -s)" >&2
    exit 1
    ;;
esac

URL="https://dl.google.com/android/repository/platform-tools-latest-${OS}.zip"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "Downloading Android platform-tools for ${OS}..."
curl -fsSL -o "$tmpdir/platform-tools.zip" "$URL"

echo "Extracting..."
unzip -q "$tmpdir/platform-tools.zip" -d "$tmpdir"

mkdir -p "$DEST"
if [[ "$OS" == "windows" ]]; then
  cp "$tmpdir/platform-tools/adb.exe" "$DEST/"
  cp "$tmpdir/platform-tools/AdbWinApi.dll" "$DEST/"
  cp "$tmpdir/platform-tools/AdbWinUsbApi.dll" "$DEST/"
else
  install -m755 "$tmpdir/platform-tools/adb" "$DEST/adb"
fi

echo "Installed ADB to $DEST/"
