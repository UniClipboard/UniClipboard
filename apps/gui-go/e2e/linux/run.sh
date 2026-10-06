#!/usr/bin/env bash
# Host-side driver for the Linux container checks (macOS or Linux host with Docker).
#   run.sh build                 build daemon, CLI and the gtk3,e2e GUI inside the container (needs frontend/dist built with VITE_GUI_GO_E2E=1)
#   run.sh xvfb <outdir>         run the Xvfb + private D-Bus scenarios (linux_xvfb_run.py) and write the artifacts to <outdir>
#   run.sh wayland <outdir>      (17c2, image :17c2 via UC_LINUX_IMAGE) headless sway + Layer Shell scenarios (linux_wayland_run.py)
#   run.sh wayland-nolib <outdir>  same container image with libgtk-layer-shell removed INSIDE the throwaway container: fallback scenario
#   run.sh package <outdir>      run package_linux.py with the daemon from `build` (needs frontend/dist built with VITE_GUI_GO_E2E=0)
# 17c4 (self-contained AppImage; docs/architecture/gui-go-linux-appimage.md), image :17c2 for the build steps:
#   run.sh daemon-release        build the shipped release daemon (build_daemon_release.sh) into /cache/out-release with build-evidence.txt
#   run.sh release-e2e-build     build the GUI with tags gtk3,production,release,e2e (needs frontend/dist built with VITE_GUI_GO_E2E=1)
#   run.sh package-release <outdir>      full release package (release-tag GUI, deb, rpm, AppImage) from the release daemon (dist built with E2E=0)
#   run.sh package-appimage <outdir> [package_linux.py args]   AppImage-only package of the release+e2e GUI (E2E-prefixed; v2 adds --update-marker)
#   run.sh appimage-e2e <outdir> <appimage dir>   (image uc-gui-go-linux-runtime:17c4, NO GTK/WebKitGTK) linux_appimage_run.py
# The image is uc-gui-go-linux-build:17c (e2e/linux/Dockerfile); build artifacts live in the docker volume uc-gui-go-linux-cache.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c}"
VOLUME=uc-gui-go-linux-cache
mode="${1:?usage: run.sh build|xvfb|package [outdir]}"
docker image inspect "$IMAGE" >/dev/null
# A git worktree's .git file points at the main repository's metadata by absolute path: mount that read-only at the same
# path so git works inside the container (provenance in the manifests), without letting the container write to it.
GITCOMMON="$(cd "$ROOT" && cd "$(git rev-parse --git-common-dir)" && pwd -P)"
common=(-e UC_WAYLAND_RUN_ARGS="${UC_WAYLAND_RUN_ARGS:-}" --rm --platform linux/arm64 -v "$ROOT:/work" --mount "type=bind,src=$GITCOMMON,dst=$GITCOMMON,readonly" -e GIT_OPTIONAL_LOCKS=0 -v "$VOLUME:/cache" -w /work)
case "$mode" in
  build)
    docker run "${common[@]}" -e SKIP_DAEMON="${SKIP_DAEMON:-0}" "$IMAGE" bash apps/gui-go/e2e/linux/build_in_container.sh ;;
  xvfb)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    docker run "${common[@]}" -v "$out:/out" "$IMAGE" bash -c '
      set -e
      Xvfb :99 -screen 0 1280x800x24 -nolisten tcp & xvfb=$!
      export DISPLAY=:99
      for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
      uname -a > /out/container-uname.txt; xdpyinfo | head -12 > /out/xvfb-info.txt
      set +e
      dbus-run-session -- python3 apps/gui-go/e2e/linux_xvfb_run.py --out /out --binaries /cache/out
      code=$?
      kill $xvfb 2>/dev/null
      exit $code' ;;
  wayland|wayland-nolib)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    extra=""; [ "$mode" = wayland-nolib ] && extra="--no-layer-library"
    docker run "${common[@]}" -v "$out:/out" "$IMAGE" bash -c "
      set -e
      if [ -n '$extra' ]; then rm -f /lib/aarch64-linux-gnu/libgtk-layer-shell.so.0* /usr/lib/aarch64-linux-gnu/libgtk-layer-shell.so.0* && ldconfig && ! ldconfig -p | grep -q gtk-layer-shell; fi
      uname -a > /out/container-uname.txt; sway --version > /out/sway-version.txt
      set +e
      dbus-run-session -- python3 apps/gui-go/e2e/linux_wayland_run.py --out /out --binaries /cache/out $extra ${UC_WAYLAND_RUN_ARGS:-}
      exit \$?" ;;
  package)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    docker run "${common[@]}" -v "$out:/out" "$IMAGE" bash -c '
      git config --global --add safe.directory /work
      python3 apps/gui-go/e2e/package_linux.py --arch arm64 --daemon /cache/out/uniclipd --out /out/packages' ;;
  daemon-release)
    docker run "${common[@]}" "$IMAGE" bash apps/gui-go/e2e/linux/build_daemon_release.sh ;;
  release-e2e-build)
    docker run "${common[@]}" "$IMAGE" bash apps/gui-go/e2e/linux/build_release_e2e_in_container.sh ;;
  package-release)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    docker run "${common[@]}" -v "$out:/out" "$IMAGE" bash -c '
      git config --global --add safe.directory /work; export GOPATH=/cache/gopath GOFLAGS=-mod=mod
      python3 apps/gui-go/e2e/package_linux.py --arch arm64 --daemon /cache/out-release/uniclipd --daemon-evidence /cache/out-release/build-evidence.txt --tools-dir /cache/tools --out /out/packages' ;;
  package-appimage)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"; shift 2
    docker run "${common[@]}" -v "$out:/out" "$IMAGE" bash -c '
      git config --global --add safe.directory /work; export GOPATH=/cache/gopath GOFLAGS=-mod=mod
      python3 apps/gui-go/e2e/package_linux.py --arch arm64 --daemon /cache/out-release/uniclipd --daemon-evidence /cache/out-release/build-evidence.txt --gui-binary /cache/out-release/gui-go-release-e2e --appimage-only --tools-dir /cache/tools --out /out/pkg "$@"' _ "$@" ;;
  *) echo "usage: run.sh build|xvfb|package|daemon-release|release-e2e-build|package-release|package-appimage [outdir]" >&2; exit 2 ;;
esac
