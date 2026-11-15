#!/usr/bin/env bash

set -euo pipefail

# Launch an AppImage with live desktop GUI passthrough in clean Podman containers
# (Fedora, Nix appimage-run, Ubuntu).
#
# Usage:
#   scripts/test_appimage.sh [options] [fedora|nix|ubuntu|all] [appimage-path] [-- app-args...]

usage() {
  cat <<'EOF'
Usage:
  scripts/test_appimage.sh [options] [distro] [appimage-path] [-- app-args...]

Launch an AppImage with live GUI (Wayland/X11 + /dev/dri GPU + native FUSE / stock appimage-run)
inside a clean Podman container.

Distros:
  fedora           Fedora 43 Workstation baseline (default)
  nix              Nix with stock appimage-run + Mesa OpenGL driver
  ubuntu           Ubuntu 24.04 Desktop baseline
  all              Run ubuntu, fedora, and nix sequentially and report results

Options:
  --rebuild-image  Force rebuilding the test container image(s)
  --shell          Open an interactive shell inside the test container instead of launching
  --timeout SEC    Stop each launch after SEC seconds (exit 124 treated as clean startup)
  -h, --help       Show this help

Arguments:
  appimage-path    Path to AppImage to test (default: dist/yaas.AppImage)
  app-args         Arguments forwarded to the AppImage after '--'
EOF
}

CALLER_PWD="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

REBUILD_IMAGE=0
OPEN_SHELL=0
TIMEOUT_SEC=""
POSITIONAL=()
APP_ARGS=()

while (($# > 0)); do
  case "$1" in
    --rebuild-image)
      REBUILD_IMAGE=1
      shift
      ;;
    --shell)
      OPEN_SHELL=1
      shift
      ;;
    --timeout)
      TIMEOUT_SEC="${2:?--timeout requires seconds}"
      shift 2
      ;;
    --timeout=*)
      TIMEOUT_SEC="${1#*=}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      APP_ARGS=("$@")
      break
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      POSITIONAL+=("$1")
      shift
      ;;
  esac
done

DISTRO="${POSITIONAL[0]:-fedora}"
APPIMAGE_ARG="${POSITIONAL[1]:-}"

if ((${#POSITIONAL[@]} > 2)); then
  echo "Unexpected extra argument: ${POSITIONAL[2]}" >&2
  usage >&2
  exit 1
fi

case "$DISTRO" in
  fedora|nix|ubuntu|all) ;;
  *)
    echo "Unsupported distro '${DISTRO}'. Expected one of: fedora, nix, ubuntu, all" >&2
    exit 1
    ;;
esac

if ((OPEN_SHELL)) && [[ "$DISTRO" == "all" ]]; then
  echo "--shell requires a single distro (fedora, nix, or ubuntu)" >&2
  exit 1
fi

if ! command -v podman >/dev/null 2>&1; then
  echo "Missing required command: podman" >&2
  exit 1
fi

if [[ "$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null || true)" != "true" ]]; then
  echo "Refusing to run with --privileged because Podman is not running in rootless mode." >&2
  exit 1
fi

if [[ -z "$APPIMAGE_ARG" ]]; then
  APPIMAGE_PATH="${REPO_ROOT}/dist/yaas.AppImage"
elif [[ "$APPIMAGE_ARG" = /* ]]; then
  APPIMAGE_PATH="$APPIMAGE_ARG"
else
  APPIMAGE_PATH="${CALLER_PWD}/${APPIMAGE_ARG}"
fi

if [[ ! -f "$APPIMAGE_PATH" ]]; then
  echo "AppImage not found at: ${APPIMAGE_PATH}" >&2
  echo "Hint: build one first with 'just build-appimage-ci' (or pass a path to an existing .AppImage)." >&2
  exit 1
fi

APPIMAGE_REAL="$(realpath "$APPIMAGE_PATH")"
APPIMAGE_NAME="$(basename "$APPIMAGE_REAL")"

if [[ ! -e /dev/fuse ]]; then
  echo "/dev/fuse is missing on the host; required for native AppImage mounting." >&2
  exit 1
fi

ensure_image() {
  local target="$1"
  local containerfile="${SCRIPT_DIR}/podman/test-${target}.Containerfile"
  local image_tag="localhost/yaas-test-${target}:latest"
  local spec_hash current_hash

  spec_hash="$(sha256sum "$containerfile" | awk '{print $1}')"
  current_hash="$(podman image inspect --format '{{ index .Config.Labels "io.yaas.spec-sha256" }}' "$image_tag" 2>/dev/null || true)"

  if ((REBUILD_IMAGE)) || [[ "$current_hash" != "$spec_hash" ]]; then
    echo "==> Building ${image_tag}..." >&2
    podman build \
      --pull=missing \
      --label "io.yaas.spec-sha256=${spec_hash}" \
      -t "$image_tag" \
      -f "$containerfile" \
      "${SCRIPT_DIR}/podman" >&2
  fi
}

build_gui_flags() {
  GUI_FLAGS=(
    --privileged
    --device /dev/fuse
    --hostname "$(hostname)"
    -e "XDG_RUNTIME_DIR=/tmp"
    -e "XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-}"
    -e "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-}"
  )

  if [[ -d /dev/dri ]]; then
    GUI_FLAGS+=(--device /dev/dri)
  fi

  local has_display=0
  if [[ -n "${WAYLAND_DISPLAY:-}" && -n "${XDG_RUNTIME_DIR:-}" && -S "${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}" ]]; then
    GUI_FLAGS+=(
      -v "${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}:/tmp/${WAYLAND_DISPLAY}:ro"
      -e "WAYLAND_DISPLAY=${WAYLAND_DISPLAY}"
    )
    has_display=1
  fi

  if [[ -n "${DISPLAY:-}" && -d /tmp/.X11-unix ]]; then
    GUI_FLAGS+=(
      -v /tmp/.X11-unix:/tmp/.X11-unix:ro
      -e "DISPLAY=${DISPLAY}"
    )
    if [[ -n "${XAUTHORITY:-}" && -r "${XAUTHORITY}" ]]; then
      GUI_FLAGS+=(
        -v "${XAUTHORITY}:/tmp/.Xauthority:ro"
        -e "XAUTHORITY=/tmp/.Xauthority"
      )
    fi
    has_display=1
  fi

  if ((has_display == 0)); then
    echo "Warning: neither WAYLAND_DISPLAY nor DISPLAY socket detected on host." >&2
  fi

  if [[ -n "${XDG_RUNTIME_DIR:-}" && -S "${XDG_RUNTIME_DIR}/pulse/native" ]]; then
    GUI_FLAGS+=(
      -v "${XDG_RUNTIME_DIR}/pulse/native:/tmp/pulse-native:ro"
      -e "PULSE_SERVER=unix:/tmp/pulse-native"
    )
  fi
}

run_one() {
  local target="$1"
  local image_tag="localhost/yaas-test-${target}:latest"

  ensure_image "$target" || return $?

  local tty_flags=()
  if [[ -t 0 && -t 1 ]]; then
    tty_flags=(-it)
  fi

  local -a cmd
  if ((OPEN_SHELL)); then
    echo "==> Entering ${target} container (/app/${APPIMAGE_NAME})..."
    if [[ "$target" == "nix" ]]; then
      echo "    Launch manually with: appimage-run '/app/${APPIMAGE_NAME}'"
    else
      echo "    Launch manually with: '/app/${APPIMAGE_NAME}'"
    fi
    cmd=(bash)
  elif [[ "$target" == "nix" ]]; then
    cmd=(appimage-run "/app/${APPIMAGE_NAME}" "${APP_ARGS[@]}")
  else
    cmd=("/app/${APPIMAGE_NAME}" "${APP_ARGS[@]}")
  fi

  if [[ -n "$TIMEOUT_SEC" ]] && ((!OPEN_SHELL)); then
    cmd=(timeout --preserve-status "$TIMEOUT_SEC" "${cmd[@]}")
  fi

  echo "==> Launching ${APPIMAGE_NAME} on ${target} (${image_tag})..."
  local rc=0
  podman run --rm \
    "${tty_flags[@]}" \
    "${GUI_FLAGS[@]}" \
    -v "${APPIMAGE_REAL}:/app/${APPIMAGE_NAME}:ro" \
    "$image_tag" \
    "${cmd[@]}" || rc=$?

  # timeout SIGTERM (143) or 124 means the app stayed alive until the deadline
  if [[ -n "$TIMEOUT_SEC" ]] && ((rc == 124 || rc == 143)); then
    echo "==> ${target}: stayed running for ${TIMEOUT_SEC}s (OK)"
    return 0
  fi

  return "$rc"
}

build_gui_flags

if [[ "$DISTRO" == "all" ]]; then
  TARGETS=(ubuntu fedora nix)
  declare -A RESULTS=()
  OVERALL_RC=0

  for target in "${TARGETS[@]}"; do
    echo
    if run_one "$target"; then
      RESULTS["$target"]="PASS"
    else
      rc=$?
      RESULTS["$target"]="FAIL (exit ${rc})"
      OVERALL_RC=1
    fi
  done

  echo
  echo "==> Summary for ${APPIMAGE_NAME}:"
  for target in "${TARGETS[@]}"; do
    printf "  %-8s %s\n" "${target}:" "${RESULTS[$target]}"
  done
  exit "$OVERALL_RC"
else
  run_one "$DISTRO"
fi
