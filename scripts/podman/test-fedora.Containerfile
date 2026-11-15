# Fedora Workstation desktop runtime baseline
FROM registry.fedoraproject.org/fedora:43

RUN dnf install -y --setopt=install_weak_deps=False \
      alsa-lib \
      dejavu-sans-fonts \
      fuse3 \
      gtk3 \
      libglvnd-gles \
      mesa-dri-drivers \
      mesa-libEGL \
      mesa-libGL \
    && dnf clean all
