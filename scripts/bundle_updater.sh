#!/usr/bin/env bash
set -euo pipefail

DEST="${1:?Usage: bundle_updater.sh <destination_directory>}"
mkdir -p "$DEST"
case "$(uname -s)" in
  Darwin*)
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --locked --release -p app-update --bin yaas-updater --target aarch64-apple-darwin
    cargo build --locked --release -p app-update --bin yaas-updater --target x86_64-apple-darwin
    lipo -create target/aarch64-apple-darwin/release/yaas-updater target/x86_64-apple-darwin/release/yaas-updater -output "$DEST/yaas-updater"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    cargo build --locked --release -p app-update --bin yaas-updater
    cp target/release/yaas-updater.exe "$DEST/"
    ;;
  Linux*)
    cargo build --locked --release -p app-update --bin yaas-updater
    install -m755 target/release/yaas-updater "$DEST/"
    ;;
  *) echo "Unsupported platform" >&2; exit 1 ;;
esac
