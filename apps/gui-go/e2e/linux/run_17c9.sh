#!/usr/bin/env bash
# 17c9 driver: Linux X11 quick panel under a real window manager (Openbox) (../linux_x11_wm_run.py).
#   run_17c9.sh image                  build uc-gui-go-linux-build:17c9-wm (Dockerfile.17c9-wm); prints the image ID and Openbox version
#   run_17c9.sh build <tag> <artdir>   same build as 17c8 (real VITE_GUI_GO_E2E=1 frontend, gtk3,e2e GUI + Go CLI in the container) into a NEW
#                                      /cache/out-17c9-<tag>; the 17c5 release daemon is copied in after a SHA-256 check. Provenance -> <artdir>/inputs/.
#   run_17c9.sh wayland <tag> <outdir>  the existing headless-sway Layer Shell E2E (linux_wayland_run.py) against the same binaries: the 17c9 change touches the
#                                      panel's creation options, which that path shares.
#   run_17c9.sh run <tag> <outdir>     run the scenario against /cache/out-17c9-<tag> in Xvfb 1920x1200 + Openbox; the binaries it ran are copied out.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../../.." && pwd)"
IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c9-wm}"
EXPECTED_DAEMON_SHA=ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6
mode="${1:?usage: run_17c9.sh image|build|run}"
if [ "$mode" = image ]; then
  docker build --platform linux/arm64 -f "$HERE/Dockerfile.17c9-wm" -t "$IMAGE" "$HERE"
  docker image inspect "$IMAGE" --format 'image id: {{.Id}}'
  docker run --rm --platform linux/arm64 "$IMAGE" openbox --version | head -1
  exit 0
fi
tag="${2:?tag}"; dir="$(mkdir -p "${3:?dir}" && cd "$3" && pwd)"
CACHE_DIR="/cache/out-17c9-$tag"
GITCOMMON="$(cd "$ROOT" && cd "$(git rev-parse --git-common-dir)" && pwd -P)"
docker_run=(docker run --rm --platform linux/arm64 -v "$ROOT:/work" --mount "type=bind,src=$GITCOMMON,dst=$GITCOMMON,readonly" -e GIT_OPTIONAL_LOCKS=0 -v uc-gui-go-linux-cache:/cache -v "$dir:/out" -w /work)
case "$mode" in
  build)
    mkdir -p "$dir/inputs"
    (cd "$ROOT" && git rev-parse HEAD > "$dir/inputs/head.txt" && git status --porcelain > "$dir/inputs/status.txt" && git diff HEAD > "$dir/inputs/dirty.diff")
    docker image inspect "$IMAGE" --format '{{.Id}}' > "$dir/inputs/image-id.txt"
    (cd "$ROOT" && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go/frontend build > "$dir/inputs/frontend-build.log" 2>&1)
    grep -rl "linux-shortcut-ui" "$ROOT/apps/gui-go/frontend/dist/assets" > "$dir/inputs/dist-contains-driver.txt" || { echo "dist lacks the E2E driver: wrong or stale frontend build" >&2; exit 1; }
    (cd "$ROOT/apps/gui-go/frontend/dist" && find . -type f | LC_ALL=C sort | xargs shasum -a 256 > "$dir/inputs/dist.sha256" && shasum -a 256 "$dir/inputs/dist.sha256" > "$dir/inputs/dist-tree.sha256")
    "${docker_run[@]}" -e UC_OUT_DIR="$CACHE_DIR" -e SKIP_DAEMON=1 "$IMAGE" bash -c '
      set -e
      [ ! -e "$UC_OUT_DIR" ] || { echo "$UC_OUT_DIR exists: use a new tag" >&2; exit 1; }
      bash apps/gui-go/e2e/linux/build_in_container.sh
      echo "'"$EXPECTED_DAEMON_SHA"'  /cache/out-release/uniclipd" | sha256sum -c -
      cp /cache/out-release/uniclipd "$UC_OUT_DIR/uniclipd"
      (cd "$UC_OUT_DIR" && sha256sum gui-go uniclip uniclipd > /out/inputs/binaries.sha256)' | tee "$dir/inputs/container-build.log" ;;
  run)
    "${docker_run[@]}" -e CACHE_DIR="$CACHE_DIR" "$IMAGE" bash -c '
      set -e
      [ -d "$CACHE_DIR" ]
      mkdir -p /out/binaries && cp "$CACHE_DIR"/gui-go "$CACHE_DIR"/uniclip "$CACHE_DIR"/uniclipd /out/binaries/
      (cd /out/binaries && sha256sum * > /out/binaries.sha256)
      Xvfb :99 -screen 0 1920x1200x24 -nolisten tcp & xvfb=$!
      export DISPLAY=:99
      for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
      uname -a > /out/container-uname.txt; xdpyinfo | head -12 > /out/xvfb-info.txt; git -C /work rev-parse HEAD > /out/head.txt
      openbox --version | head -1 > /out/openbox-version.txt; wmctrl -V | head -1 > /out/wmctrl-version.txt
      set +e
      dbus-run-session -- python3 apps/gui-go/e2e/linux_x11_wm_run.py --out /out --binaries /out/binaries
      code=$?
      kill $xvfb 2>/dev/null
      exit $code' ;;
  wayland)
    "${docker_run[@]}" -e CACHE_DIR="$CACHE_DIR" "$IMAGE" bash -c '
      set -e
      [ -d "$CACHE_DIR" ]
      mkdir -p /out/binaries && cp "$CACHE_DIR"/gui-go "$CACHE_DIR"/uniclip "$CACHE_DIR"/uniclipd /out/binaries/
      (cd /out/binaries && sha256sum * > /out/binaries.sha256)
      uname -a > /out/container-uname.txt; sway --version > /out/sway-version.txt; git -C /work rev-parse HEAD > /out/head.txt
      set +e
      dbus-run-session -- python3 apps/gui-go/e2e/linux_wayland_run.py --out /out --binaries /out/binaries
      exit $?' ;;
  *) echo "unknown mode" >&2; exit 2 ;;
esac
