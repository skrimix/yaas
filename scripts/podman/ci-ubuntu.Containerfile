# Ubuntu 24.04 AppImage builder driven by scripts/ci.sh
FROM docker.io/library/ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG FLUTTER_VERSION
ARG RINF_CLI_VERSION

RUN test -n "${FLUTTER_VERSION}" && test -n "${RINF_CLI_VERSION}"

COPY ci.sh /usr/local/libexec/yaas-ci.sh
RUN chmod +x /usr/local/libexec/yaas-ci.sh \
    && /usr/local/libexec/yaas-ci.sh install-container-deps

# Suppress Flutter's root check inside rootless Podman (Flutter checks /.dockerenv or CI=true)
ENV CI=true

# Flutter SDK
RUN curl -fsSL --retry 3 "https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_${FLUTTER_VERSION}-stable.tar.xz" \
      | tar -xJ -C /opt \
    && git config --system --add safe.directory /opt/flutter \
    && /opt/flutter/bin/flutter config --no-analytics --enable-linux-desktop

ENV PATH="/opt/flutter/bin:/root/.cargo/bin:/usr/local/bin:${PATH}"

# Rust stable toolchain + rinf_cli
RUN curl -fsSL --retry 3 https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && cargo install rinf_cli --version "${RINF_CLI_VERSION}" --locked \
    && rm -rf /root/.cargo/registry /root/.cargo/git

# appimagetool + fastforge (isolated from /root/.pub-cache so mounting a cache volume won't shadow it)
RUN /usr/local/libexec/yaas-ci.sh install-linux-tools /usr/local/bin /opt/fastforge-pub-cache

# Used only inside the builder so appimagetool and --appimage-extract run without FUSE
ENV APPIMAGE_EXTRACT_AND_RUN=1

WORKDIR /workspace
