# NixOS appimage-run + Mesa OpenGL driver layout
FROM docker.io/nixos/nix:latest

RUN nix-env -iA nixpkgs.appimage-run nixpkgs.mesa \
    && mkdir -p /run \
    && ln -sfn /root/.nix-profile /run/opengl-driver \
    && nix-collect-garbage -d

ENV PATH="/root/.nix-profile/bin:${PATH}"
