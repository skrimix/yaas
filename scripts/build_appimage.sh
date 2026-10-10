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
require_cmd python3

mkdir -p dist

echo "==> Resolving locked Flutter dependencies..."
flutter pub get --enforce-lockfile

echo "==> Generating Rinf bindings..."
rinf gen

echo "==> Building AppImage with fastforge..."
fastforge package --platform linux --targets appimage --skip-clean --flutter-build-args=no-pub

APP_VERSION="$(python3 -c 'from scripts.release import app_version; version, build = app_version(); print(f"{version}+{build}")')"
BUILT_APPIMAGE="dist/${APP_VERSION}/yaas-${APP_VERSION}-linux.AppImage"
if [[ ! -f "$BUILT_APPIMAGE" ]]; then
  echo "No AppImage found at $BUILT_APPIMAGE" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT_APPIMAGE")"
if [[ ! "$BUILT_APPIMAGE" -ef "$OUTPUT_APPIMAGE" ]]; then
  echo "==> Copying ${BUILT_APPIMAGE} to ${OUTPUT_APPIMAGE}..."
  cp -v "$BUILT_APPIMAGE" "$OUTPUT_APPIMAGE"
fi

echo "==> Repacking AppImage with bundled 7-Zip..."
app="$OUTPUT_APPIMAGE"
chmod +x "$app"
"$app" --appimage-extract

"$SCRIPT_DIR/bundle_7zip.sh" squashfs-root/usr/bin
"$SCRIPT_DIR/bundle_adb.sh" squashfs-root/usr/bin

python3 "$SCRIPT_DIR/release.py" verify-bundle linux squashfs-root

# Dart opens libmpv.so, while the video plugin links the versioned library.
# Both names must load the same bundled copy.
shopt -s nullglob
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
