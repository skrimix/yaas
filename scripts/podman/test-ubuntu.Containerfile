# Ubuntu 24.04 desktop runtime baseline
FROM docker.io/library/ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      alsa-topology-conf \
      alsa-ucm-conf \
      ca-certificates \
      fonts-dejavu-core \
      fuse3 \
      libasound2t64 \
      libegl1 \
      libgl1 \
      libgles2 \
      libgl1-mesa-dri \
      libgtk-3-0t64 \
    && rm -rf /var/lib/apt/lists/*
