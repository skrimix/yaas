#!/usr/bin/env bash

set -euo pipefail

# Build and repack the AppImage into a stable path.
# Can be run locally or from CI.
#
# Usage:
#   scripts/build_appimage.sh [output-path]
#
# Default output path: dist/yaas.AppImage

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${SCRIPT_DIR}/.."
cd "$REPO_ROOT"

OUTPUT_APPIMAGE="${1:-dist/yaas.AppImage}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

require_cmd fastforge
require_cmd appimagetool
require_cmd curl
require_cmd tar
require_cmd rinf
require_cmd unzip

mkdir -p dist

echo "==> Resolving locked Flutter dependencies..."
flutter pub get --enforce-lockfile

echo "==> Generating Rinf bindings..."
rinf gen

echo "==> Building AppImage with fastforge..."
fastforge package --platform linux --targets appimage --skip-clean --flutter-build-args=no-pub

echo "==> Locating built AppImage under dist/..."
shopt -s globstar nullglob
files=(dist/**/*.AppImage)
if (( ${#files[@]} == 0 )); then
  echo "No AppImage found in dist/" >&2
  find dist -maxdepth 3 -type f -name '*.AppImage' -print || true
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT_APPIMAGE")"
echo "==> Copying ${files[0]} to ${OUTPUT_APPIMAGE}..."
cp -v "${files[0]}" "$OUTPUT_APPIMAGE"

echo "==> Repacking AppImage with bundled 7-Zip..."
app="$OUTPUT_APPIMAGE"
chmod +x "$app"
"$app" --appimage-extract

"$SCRIPT_DIR/bundle_7zip.sh" squashfs-root/usr/bin
"$SCRIPT_DIR/bundle_adb.sh" squashfs-root/usr/bin

# Dart opens libmpv.so, while the video plugin links the versioned library.
# Both names must load the same bundled copy.
mpv_libraries=(squashfs-root/usr/lib/libmpv.so.*)
if (( ${#mpv_libraries[@]} != 1 )); then
  echo "Expected one bundled libmpv library, found ${#mpv_libraries[@]}" >&2
  exit 1
fi
ln -sfn "$(basename "${mpv_libraries[0]}")" squashfs-root/usr/lib/libmpv.so

# ALSA needs an absolute library path when it reopens itself for config hooks.
# AppRun expands these variables at launch.
# shellcheck disable=SC2016
sed -i \
  -e 's|^export LD_LIBRARY_PATH=.*|export LD_LIBRARY_PATH="$PWD/usr/lib"|' \
  -e '/^exec/i export PATH="$PWD/usr/bin:$PATH"' \
  -e '/^exec /{/\"\$@\"/!s/$/ "$@"/}' squashfs-root/AppRun
cat squashfs-root/AppRun

appimagetool --no-appstream squashfs-root "$app"
rm -rf squashfs-root

echo "==> AppImage ready at $OUTPUT_APPIMAGE"
