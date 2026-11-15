#!/usr/bin/env bash

set -euo pipefail

# Build the Linux AppImage inside an Ubuntu 24.04 Podman container matching CI
# (.github/workflows/packages.yaml).
#
# Usage:
#   scripts/build_appimage_ci.sh [options] [output-path]

usage() {
  cat <<'EOF'
Usage:
  scripts/build_appimage_ci.sh [options] [output-path]

Build dist/yaas.AppImage inside an Ubuntu 24.04 Podman container matching CI.
Uses a Podman overlay mount (:O) so host build/, target/, and .dart_tool/ stay untouched,
while caching Ubuntu Cargo/Flutter/Pub artifacts in persistent Podman volumes.

Options:
  --check          Also run Linux CI checks (release unit tests, flutter analyze, cargo test)
  --rebuild-image  Force rebuilding the Ubuntu 24.04 CI builder container image
  --clean-cache    Remove cached Podman build volumes before building
  -h, --help       Show this help

Arguments:
  output-path      Destination AppImage path (default: dist/yaas.AppImage)
EOF
}

CALLER_PWD="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

RUN_CHECKS=0
REBUILD_IMAGE=0
CLEAN_CACHE=0
OUTPUT_ARG=""

while (($# > 0)); do
  case "$1" in
    --check)
      RUN_CHECKS=1
      shift
      ;;
    --rebuild-image)
      REBUILD_IMAGE=1
      shift
      ;;
    --clean-cache)
      CLEAN_CACHE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      if [[ -n "$OUTPUT_ARG" ]]; then
        echo "Unexpected extra argument: $1" >&2
        usage >&2
        exit 1
      fi
      OUTPUT_ARG="$1"
      shift
      ;;
  esac
done

if ! command -v podman >/dev/null 2>&1; then
  echo "Missing required command: podman" >&2
  exit 1
fi

if [[ -z "$OUTPUT_ARG" ]]; then
  OUTPUT_PATH="${REPO_ROOT}/dist/yaas.AppImage"
elif [[ "$OUTPUT_ARG" = /* ]]; then
  OUTPUT_PATH="$OUTPUT_ARG"
else
  OUTPUT_PATH="${CALLER_PWD}/${OUTPUT_ARG}"
fi

OUTPUT_DIR="$(mkdir -p "$(dirname "$OUTPUT_PATH")" && cd "$(dirname "$OUTPUT_PATH")" && pwd)"
OUTPUT_NAME="$(basename "$OUTPUT_PATH")"

CI_SCRIPT="${SCRIPT_DIR}/ci.sh"
CONTAINERFILE="${SCRIPT_DIR}/podman/ci-ubuntu.Containerfile"
IMAGE_TAG="localhost/yaas-ci-ubuntu:24.04"

eval "$("$CI_SCRIPT" shell-env)"

VOLUMES=(
  yaas-ci-cargo-registry
  yaas-ci-cargo-git
  yaas-ci-pub-cache
  yaas-ci-target
  yaas-ci-build
)

if ((CLEAN_CACHE)); then
  echo "==> Removing cached CI Podman volumes..."
  podman volume rm -f "${VOLUMES[@]}" >/dev/null 2>&1 || true
fi

SPEC_HASH="$(
  {
    cat "$CONTAINERFILE" "$CI_SCRIPT"
    printf 'FLUTTER_VERSION=%s\nRINF_CLI_VERSION=%s\n' "$FLUTTER_VERSION" "$RINF_CLI_VERSION"
  } | sha256sum | awk '{print $1}'
)"

CURRENT_HASH="$(podman image inspect --format '{{ index .Config.Labels "io.yaas.spec-sha256" }}' "$IMAGE_TAG" 2>/dev/null || true)"

if ((REBUILD_IMAGE)) || [[ "$CURRENT_HASH" != "$SPEC_HASH" ]]; then
  echo "==> Building ${IMAGE_TAG} (Flutter ${FLUTTER_VERSION}, rinf_cli ${RINF_CLI_VERSION})..."
  podman build \
    --pull=missing \
    --build-arg "FLUTTER_VERSION=${FLUTTER_VERSION}" \
    --build-arg "RINF_CLI_VERSION=${RINF_CLI_VERSION}" \
    --label "io.yaas.spec-sha256=${SPEC_HASH}" \
    -t "$IMAGE_TAG" \
    -f "$CONTAINERFILE" \
    "$SCRIPT_DIR"
fi

EXTRA_MOUNTS=()
if [[ -f "${REPO_ROOT}/.git" ]]; then
  git_common_dir="$(git -C "$REPO_ROOT" rev-parse --path-format=absolute --git-common-dir)"
  EXTRA_MOUNTS+=(-v "${git_common_dir}:${git_common_dir}:ro")
fi

echo "==> Building AppImage inside ${IMAGE_TAG}..."
podman run --rm \
  -v "${REPO_ROOT}:/workspace:O" \
  "${EXTRA_MOUNTS[@]}" \
  -v "yaas-ci-cargo-registry:/root/.cargo/registry" \
  -v "yaas-ci-cargo-git:/root/.cargo/git" \
  -v "yaas-ci-pub-cache:/root/.pub-cache" \
  -v "yaas-ci-target:/workspace/target" \
  -v "yaas-ci-build:/workspace/build" \
  -v "${OUTPUT_DIR}:/out" \
  -e "OUTPUT_NAME=${OUTPUT_NAME}" \
  -e "RUN_CHECKS=${RUN_CHECKS}" \
  -e "YAAS_RELEASE_CHANNEL=${YAAS_RELEASE_CHANNEL:-development}" \
  -e "YAAS_RUN_NUMBER=${YAAS_RUN_NUMBER:-}" \
  -e "YAAS_RUN_ATTEMPT=${YAAS_RUN_ATTEMPT:-}" \
  "$IMAGE_TAG" \
  bash -euo pipefail -c '
    git config --global --add safe.directory /workspace
    mkdir -p /tmp/appimage-out
    ./scripts/ci.sh prepare linux
    if ((RUN_CHECKS)); then
      ./scripts/ci.sh check-linux
    fi
    ./scripts/build_appimage.sh "/tmp/appimage-out/${OUTPUT_NAME}"
    cp -f "/tmp/appimage-out/${OUTPUT_NAME}" "/out/${OUTPUT_NAME}"
    chmod +x "/out/${OUTPUT_NAME}"
  '

echo "==> CI-built AppImage ready at ${OUTPUT_DIR}/${OUTPUT_NAME}"
