#!/usr/bin/env bash

set -euo pipefail

# Shared CI configuration and setup/build helpers used by both GitHub Actions
# (.github/workflows/*.yaml) and scripts/build_appimage_ci.sh.

FLUTTER_VERSION="${FLUTTER_VERSION:-3.47.0}"
RINF_CLI_VERSION="${RINF_CLI_VERSION:-8.10.1}"
LINUX_APT_CACHE_VERSION="2"
LINUX_APT_PACKAGES=(
  ninja-build
  patchelf
  libgtk-3-dev
  libasound2-dev
  libmpv-dev
  locate
  libmimalloc-dev
  libmimalloc2.0
  libblas3
  liblapack3
)
# Baseline toolchain packages already present on GitHub's ubuntu-24.04 runner
# that bare docker.io/library/ubuntu:24.04 containers also need.
CONTAINER_BASE_APT_PACKAGES=(
  build-essential
  ca-certificates
  clang
  cmake
  curl
  file
  git
  liblzma-dev
  pkg-config
  python3
  unzip
  wget
  xz-utils
  zip
)
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

emit_github_env() {
  printf 'FLUTTER_VERSION=%s\n' "$FLUTTER_VERSION"
  printf 'RINF_CLI_VERSION=%s\n' "$RINF_CLI_VERSION"
  printf 'LINUX_APT_CACHE_VERSION=%s\n' "$LINUX_APT_CACHE_VERSION"
  printf 'LINUX_APT_PACKAGES=%s\n' "${LINUX_APT_PACKAGES[*]}"
}

emit_shell_env() {
  printf 'FLUTTER_VERSION=%q\n' "$FLUTTER_VERSION"
  printf 'RINF_CLI_VERSION=%q\n' "$RINF_CLI_VERSION"
  printf 'LINUX_APT_CACHE_VERSION=%q\n' "$LINUX_APT_CACHE_VERSION"
  printf 'LINUX_APT_PACKAGES=%q\n' "${LINUX_APT_PACKAGES[*]}"
}

fix_linux_libs() {
  local library_dir directory library target
  library_dir="/usr/lib/$(dpkg-architecture -qDEB_HOST_MULTIARCH)"
  for directory in blas lapack; do
    for library in "$library_dir/$directory/"*.so.*; do
      [ -e "$library" ] || continue
      target="$library_dir/$(basename "$library")"
      [ -e "$target" ] || ln -sf "$library" "$target"
    done
  done
  ldconfig
}

install_container_deps() {
  apt-get update
  apt-get install -y --no-install-recommends \
    "${CONTAINER_BASE_APT_PACKAGES[@]}" \
    "${LINUX_APT_PACKAGES[@]}"
  fix_linux_libs
  updatedb
  rm -rf /var/lib/apt/lists/*
}

install_linux_tools() {
  local bin_dir="${1:-$HOME/bin}"
  local fastforge_cache="${2:-$HOME/.fastforge-pub-cache}"
  local dart_bin
  dart_bin="$(command -v dart)"

  mkdir -p "$bin_dir" "$fastforge_cache"

  curl -fsSL --retry 3 -o "$bin_dir/appimagetool" "$APPIMAGETOOL_URL"
  chmod +x "$bin_dir/appimagetool"

  # Keep fastforge in its own pub cache and invoke its compiled snapshot directly
  # so child `flutter build` invocations continue using the caller's PUB_CACHE.
  PUB_CACHE="$fastforge_cache" "$dart_bin" pub global activate fastforge
  cat > "$bin_dir/fastforge" <<EOF
#!/usr/bin/env bash
set -euo pipefail
shopt -s nullglob
snapshots=("${fastforge_cache}"/global_packages/fastforge/bin/main.dart-*.snapshot)
if (( \${#snapshots[@]} == 0 )); then
  echo "fastforge snapshot not found under ${fastforge_cache}" >&2
  exit 1
fi
exec "${dart_bin}" "\${snapshots[0]}" "\$@"
EOF
  chmod +x "$bin_dir/fastforge"

  if [[ -n "${GITHUB_PATH:-}" ]]; then
    echo "$bin_dir" >> "$GITHUB_PATH"
  fi
}

prepare_target() {
  local target="${1:?Usage: ci.sh prepare <linux|windows|macos>}"
  cd "$REPO_ROOT"
  echo "==> Resolving locked Flutter dependencies..."
  flutter pub get --enforce-lockfile
  echo "==> Generating Rinf bindings..."
  rinf gen
  echo "==> Generating localizations..."
  flutter gen-l10n
  echo "==> Preparing ${target} configuration..."
  flutter build "$target" --config-only --release --no-pub
}

check_linux() {
  cd "$REPO_ROOT"
  echo "==> Checking release tooling..."
  python3 -m unittest discover -s scripts/tests -v
  echo "==> Analyzing Flutter..."
  flutter analyze
  echo "==> Testing Rust..."
  cargo test --locked
}

usage() {
  cat <<'EOF'
Usage:
  scripts/ci.sh <command> [args...]

Commands:
  shell-env                     Print shell-escaped CI variables for `eval`
  github-env                    Append CI variables to $GITHUB_ENV
  fix-linux-libs                Register BLAS/LAPACK shared library symlinks + ldconfig
  install-container-deps        Install Ubuntu 24.04 apt dependencies inside a container
  install-linux-tools [bin] [cache]
                                Install appimagetool and isolated fastforge into [bin]
  prepare <target>              Resolve pub deps, generate bindings/l10n, and run --config-only
  check-linux                   Run release script unit tests, flutter analyze, and cargo test
EOF
}

cmd="${1:-}"
shift || true

case "$cmd" in
  shell-env)
    emit_shell_env
    ;;
  github-env)
    : "${GITHUB_ENV:?GITHUB_ENV must be set}"
    emit_github_env >> "$GITHUB_ENV"
    ;;
  fix-linux-libs)
    fix_linux_libs
    ;;
  install-container-deps)
    install_container_deps
    ;;
  install-linux-tools)
    install_linux_tools "$@"
    ;;
  prepare)
    prepare_target "$@"
    ;;
  check-linux)
    check_linux
    ;;
  -h|--help|"")
    usage
    [[ -n "$cmd" ]]
    ;;
  *)
    echo "Unknown command: $cmd" >&2
    usage >&2
    exit 1
    ;;
esac
