FROM mcr.microsoft.com/devcontainers/base:ubuntu

ARG USERNAME=root
ARG FLUTTER_CHANNEL=stable

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    file \
    git \
    gnupg \
    jq \
    locate \
    lsb-release \
    openssh-client \
    pkg-config \
    unzip \
    wget \
    xz-utils \
    zip \
    tar \
    clang \
    cmake \
    ninja-build \
    build-essential \
    libgtk-3-dev \
    libasound2-dev \
    libmpv-dev \
    libmimalloc-dev \
    libmimalloc2.0 \
    libblkid-dev \
    liblzma-dev \
    libsecret-1-dev \
    libjsoncpp-dev \
    libssl-dev \
    fuse \
    libfuse2t64 \
    desktop-file-utils \
    zsync \
    sccache \
    p7zip-full \
    just \
    lld \
    llvm \
  && apt-get clean \
  && rm -rf /var/lib/apt/lists/*

# Flutter stable.
RUN git clone --depth 1 --branch "${FLUTTER_CHANNEL}" https://github.com/flutter/flutter.git /home/${USERNAME}/flutter \
  && chown -R ${USERNAME}:${USERNAME} /home/${USERNAME}/flutter \
  && git config --system --add safe.directory /home/${USERNAME}/flutter

ENV PATH="/home/${USERNAME}/flutter/bin:/home/${USERNAME}/.pub-cache/bin:/home/${USERNAME}/.cargo/bin:${PATH}"
ENV PUB_CACHE="/home/${USERNAME}/.pub-cache"

# Fix AppImage in container
ENV APPIMAGE_EXTRACT_AND_RUN=1

# Install appimagetool.
RUN mkdir -p /usr/local/bin \
  && wget -O /usr/local/bin/appimagetool \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" \
  && chmod +x /usr/local/bin/appimagetool


# Install appimage-builder
RUN wget -O appimage-builder-x86_64.AppImage https://github.com/AppImageCrafters/appimage-builder/releases/download/v1.1.0/appimage-builder-1.1.0-x86_64.AppImage \
  && chmod +x appimage-builder-x86_64.AppImage \
  && ./appimage-builder-x86_64.AppImage --appimage-extract \
  # Built-in strace is broken
  && rm squashfs-root/usr/bin/strace \
  && appimagetool squashfs-root/ \
  && rm -rf squashfs-root \
  && mv appimage-builder-x86_64.AppImage /usr/local/bin/appimage-builder

USER ${USERNAME}

RUN flutter --version \
  && flutter config --enable-linux-desktop \
  && dart --version
