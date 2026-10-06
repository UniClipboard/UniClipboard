#!/usr/bin/env bash
# Host-side driver for the Linux container checks (macOS or Linux host with Docker).
#   run.sh build                 build daemon, CLI and the gtk3,e2e GUI inside the container (needs frontend/dist built with VITE_GUI_GO_E2E=1)
#   run.sh xvfb <outdir>         run the Xvfb + private D-Bus scenarios (linux_xvfb_run.py) and write the artifacts to <outdir>
#   run.sh wayland <outdir>      (17c2, image :17c2 via UC_LINUX_IMAGE) headless sway + Layer Shell scenarios (linux_wayland_run.py)
#   run.sh wayland-nolib <outdir>  same container image with libgtk-layer-shell removed INSIDE the throwaway container: fallback scenario
#   run.sh package <outdir>      run package_linux.py with the daemon from `build` (needs frontend/dist built with VITE_GUI_GO_E2E=0)
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
common=(-e UC_WAYLAND_RUN_ARGS="${UC_WAYLAND_RUN_ARGS:-}" --rm --platform linux/arm64 -v "$ROOT:/work" -v "$GITCOMMON:$GITCOMMON:ro" -e GIT_OPTIONAL_LOCKS=0 -v "$VOLUME:/cache" -w /work)
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
  *) echo "usage: run.sh build|xvfb|package [outdir]" >&2; exit 2 ;;
esac
