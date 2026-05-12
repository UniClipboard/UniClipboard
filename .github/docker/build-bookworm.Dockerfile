# Build container for UniClipboard Linux Tauri + CLI builds.
#
# 替代 .github/workflows/{build,build-cli}.yml 里每次重复 1m+ 的
# `apt-get install` step。镜像同时覆盖:
#   - Tauri 桌面端 (webkit2gtk-4.1 / appindicator / gtk3 / rpm 等)
#   - CLI musl 静态编译 (musl-tools)
#
# glibc 基线锁在 bookworm 的 2.36 — 跟工作流里 container=debian:bookworm
# 的预期一致。multi-arch (linux/amd64 + linux/arm64) 由 build workflow
# 通过 buildx 推送,aarch64 host 自动选到 arm64 manifest。

FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      git curl wget ca-certificates xz-utils unzip sudo \
      build-essential pkg-config cmake clang \
      libssl-dev \
      libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf \
      libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
      file desktop-file-utils \
      xdg-utils \
      rpm \
      musl-tools \
 && rm -rf /var/lib/apt/lists/*
