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

# libglib2.0-bin / libgdk-pixbuf2.0-bin / libgtk-3-bin 是 AppImage 打包必需的
# 运行时工具包:新版 tauri-bundler 的 AppImage 流程会跑 linuxdeploy-plugin-gtk.sh,
# 该脚本 `set -e` 下调用 glib-compile-schemas / gdk-pixbuf-query-loaders /
# gtk-query-immodules-3.0,缺失会让脚本中途 sed 一个没生成的 cache 文件而失败,
# 最终 tauri 只吐一句笼统的 `failed to run linuxdeploy`。这些工具仅出现在上面
# -dev 包的 Recommends 里,被 --no-install-recommends 滤掉,所以必须显式装。
# 2026-05-10 的旧 bundler 不跑这个插件,所以同一镜像当时能成功(详见 #938 的
# FUSE 误诊:APPIMAGE_EXTRACT_AND_RUN 只让 linuxdeploy 自身能启动,真正的失败
# 在它调用的 gtk 插件里)。
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
      libglib2.0-bin libgdk-pixbuf2.0-bin libgtk-3-bin \
 && rm -rf /var/lib/apt/lists/*
