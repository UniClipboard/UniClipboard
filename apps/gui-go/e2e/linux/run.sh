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
#   run.sh appimage-feed <outdir> <v2 AppImage.tar.gz>   sign the v2 updater archive with isolated fixture keys (e2e/updatetool, needs Go: build image)
#   run.sh appimage-e2e <outdir> <full|negative|smoke> <AppImage> [feed dir] [package-manifest.json]
#                                (image uc-gui-go-linux-runtime:17c4, NO GTK/WebKitGTK; the Secret Service runs in its own
#                                container, uc-gui-go-linux-keyring:17c4, sharing a session bus volume) linux_appimage_run.py
#   (UC_PORTABLE_E2E_ARGS=--supplement runs only the stale-APPIMAGE and XDG_CONFIG_HOME scenarios)
#   run.sh appimage-portable-e2e <outdir> <AppImage> <feed dir> <package-manifest.json>
#                                (17c5, image uc-gui-go-linux-runtime:17c4 as an UNPRIVILEGED user, NO Secret Service and no session bus:
#                                portable mode uses the file keystore) linux_appimage_portable_run.py
#   run.sh appimage-helpers-e2e <outdir> <AppImage> <package-manifest.json>   (17c10; UC_HELPERS_IMAGE, UC_HELPERS_DESKTOP=generic|gnome)
#   run.sh appimage-tls-e2e <outdir> <AppImage> <package-manifest.json>   (17c7; UC_TLS_E2E_ARGS=--expect-tls absent is the failing control)
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
common=(-e UC_OUT_DIR="${UC_OUT_DIR:-}" -e UC_WAYLAND_RUN_ARGS="${UC_WAYLAND_RUN_ARGS:-}" --rm --platform linux/arm64 -v "$ROOT:/work" --mount "type=bind,src=$GITCOMMON,dst=$GITCOMMON,readonly" -e GIT_OPTIONAL_LOCKS=0 -v "$VOLUME:/cache" -w /work)
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
    # UC_PACKAGE_DOCKER_ARGS adds docker options for this container only (17c6: `--network none` proves the pack needs no download);
    # UC_PACKAGE_TOOLS is a host directory used as the tools directory instead of the cache volume's /cache/tools (negative cases).
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"; shift 2
    extra=(); [ -z "${UC_PACKAGE_TOOLS:-}" ] || extra+=(-v "$UC_PACKAGE_TOOLS:/tools")
    docker run "${common[@]}" ${UC_PACKAGE_DOCKER_ARGS:-} ${extra[@]+"${extra[@]}"} -v "$out:/out" "$IMAGE" bash -c '
      git config --global --add safe.directory /work; export GOPATH=/cache/gopath GOFLAGS=-mod=mod
      tools=/cache/tools; [ ! -d /tools ] || tools=/tools
      python3 apps/gui-go/e2e/package_linux.py --arch arm64 --daemon /cache/out-release/uniclipd --daemon-evidence /cache/out-release/build-evidence.txt --gui-binary /cache/out-release/gui-go-release-e2e --appimage-only --tools-dir "$tools" --out /out/pkg "$@"' _ "$@" ;;
  appimage-feed)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"; archive="$(cd "$(dirname "${3:?v2 AppImage.tar.gz}")" && pwd)/$(basename "$3")"
    docker run "${common[@]}" -v "$out:/out" -v "$archive:/in/update.AppImage.tar.gz:ro" "$IMAGE" bash -c '
      export GOPATH=/cache/gopath GOFLAGS=-mod=mod
      cp /in/update.AppImage.tar.gz /out/update.AppImage.tar.gz
      tar -xzOf /in/update.AppImage.tar.gz | sha256sum > /out/v2.sha256
      cd apps/gui-go && go run ./e2e/updatetool /out/update.AppImage.tar.gz /out' ;;
  appimage-e2e)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"; mode="${3:?full|negative|smoke}"
    image="$(cd "$(dirname "${4:?AppImage}")" && pwd)/$(basename "$4")"
    tag="$(basename "$out")-$$"; bus="uc-17c4-bus-$tag"; keyring="uc-17c4-keyring-$tag"
    mounts=(-v "$image:/in/appimage.AppImage:ro")
    runargs=(--mode "$mode" --out /out --appimage /in/appimage.AppImage)
    if [ -n "${5:-}" ]; then mounts+=(-v "$(cd "$5" && pwd):/in/feed"); runargs+=(--feed /in/feed); fi
    if [ -n "${6:-}" ]; then mounts+=(-v "$(cd "$(dirname "$6")" && pwd)/$(basename "$6"):/in/package-manifest.json:ro"); runargs+=(--manifest /in/package-manifest.json); fi
    docker volume create "$bus" >/dev/null
    docker run -d --name "$keyring" --platform linux/arm64 -v "$bus:/bus" uc-gui-go-linux-keyring:17c4 /usr/local/bin/keyring_service.sh >/dev/null
    for _ in $(seq 60); do docker run --rm -v "$bus:/bus" uc-gui-go-linux-runtime:17c4 test -f /bus/ready && break; sleep 1; done
    set +e
    docker run --rm --init --platform linux/arm64 --device /dev/fuse --cap-add SYS_ADMIN --security-opt apparmor:unconfined \
      -e UC_E2E_BUS=unix:path=/bus/bus -v "$bus:/bus" -v "$ROOT:/work:ro" -v "uc-gui-go-linux-cache:/cache:ro" -v "$out:/out" "${mounts[@]}" \
      uc-gui-go-linux-runtime:17c4 python3 /work/apps/gui-go/e2e/linux_appimage_run.py "${runargs[@]}" --uniclip /cache/out/uniclip > "$out/run.log" 2>&1
    code=$?
    docker logs "$keyring" > "$out/keyring-container.log" 2>&1
    # A bus-activated (locked) keyring or a prompt request means the Secret Service did not behave like an unlocked desktop keyring:
    # whatever the runner reported, the run is not valid evidence (see keyring_service.sh).
    if grep -qE "SystemPrompter|couldn't create system prompt|Activating service name='org.freedesktop.secrets'" "$out/keyring-container.log"; then
      echo "INVALID RUN: the Secret Service container asked for a prompt or re-activated the keyring (see keyring-container.log)" >&2
      [ "$code" = 0 ] && code=3
    fi
    docker stop "$keyring" >/dev/null; docker rm "$keyring" >/dev/null; docker volume rm "$bus" >/dev/null
    tail -n 30 "$out/run.log"; exit $code ;;
  appimage-portable-e2e)  # SYS_PTRACE: the runner (root) reads /proc/<pid>/{exe,environ,maps} of the unprivileged user's processes
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    image="$(cd "$(dirname "${3:?AppImage}")" && pwd)/$(basename "$3")"
    feed="$(cd "${4:?feed dir}" && pwd)"
    manifest="$(cd "$(dirname "${5:?package-manifest.json}")" && pwd)/$(basename "$5")"
    set +e
    docker run --rm --init --platform linux/arm64 --device /dev/fuse --cap-add SYS_ADMIN --cap-add SYS_PTRACE --security-opt apparmor:unconfined \
      -v "$ROOT:/work:ro" -v "uc-gui-go-linux-cache:/cache:ro" -v "$out:/out" -v "$image:/in/appimage.AppImage:ro" -v "$feed:/in/feed" \
      -v "$manifest:/in/package-manifest.json:ro" \
      uc-gui-go-linux-runtime:17c4 python3 /work/apps/gui-go/e2e/linux_appimage_portable_run.py --out /out --appimage /in/appimage.AppImage \
      --uniclip /cache/out/uniclip --feed /in/feed --manifest /in/package-manifest.json ${UC_PORTABLE_E2E_ARGS:-} > "$out/run.log" 2>&1
    code=$?
    exit "$code" ;;
  appimage-tls-e2e)  # 17c7: WebView HTTPS + runtime library origin. UC_TLS_IMAGE selects the host distribution image (default: Ubuntu runtime :17c7 = :17c4 + binutils)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    image="$(cd "$(dirname "${3:?AppImage}")" && pwd)/$(basename "$3")"
    manifest="$(cd "$(dirname "${4:?package-manifest.json}")" && pwd)/$(basename "$4")"
    set +e
    docker run --rm --init --platform linux/arm64 --device /dev/fuse --cap-add SYS_ADMIN --cap-add SYS_PTRACE --security-opt apparmor:unconfined \
      -v "$ROOT:/work:ro" -v "$out:/out" -v "$image:/in/appimage.AppImage:ro" -v "$manifest:/in/package-manifest.json:ro" \
      "${UC_TLS_IMAGE:-uc-gui-go-linux-runtime:17c7}" python3 /work/apps/gui-go/e2e/linux_appimage_tls_run.py --out /out --appimage /in/appimage.AppImage \
      --manifest /in/package-manifest.json ${UC_TLS_E2E_ARGS:-} > "$out/run.log" 2>&1
    code=$?
    exit "$code" ;;
  appimage-helpers-e2e)  # 17c10: host helpers (xdg-open, gio, handler) started by the GUI under AppRun's environment. UC_HELPERS_IMAGE selects the distribution image; UC_HELPERS_DESKTOP generic|gnome
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    image="$(cd "$(dirname "${3:?AppImage}")" && pwd)/$(basename "$3")"
    manifest="$(cd "$(dirname "${4:?package-manifest.json}")" && pwd)/$(basename "$4")"
    set +e
    docker run --rm --init --platform linux/arm64 --device /dev/fuse --cap-add SYS_ADMIN --cap-add SYS_PTRACE --security-opt apparmor:unconfined \
      -v "$ROOT:/work:ro" -v "$out:/out" -v "$image:/in/appimage.AppImage:ro" -v "$manifest:/in/package-manifest.json:ro" \
      "${UC_HELPERS_IMAGE:?UC_HELPERS_IMAGE}" python3 /work/apps/gui-go/e2e/linux_appimage_helpers_run.py --out /out --appimage /in/appimage.AppImage \
      --manifest /in/package-manifest.json --desktop "${UC_HELPERS_DESKTOP:?UC_HELPERS_DESKTOP}" ${UC_HELPERS_E2E_ARGS:-} > "$out/run.log" 2>&1
    code=$?
    exit "$code" ;;
  appimage-real-e2e)  # 17c11: REAL browser / file manager / image viewer, non-portable. UC_REAL_IMAGE selects the distribution image; UC_HELPERS_DESKTOP generic|gnome; UC_REAL_ARGS browser options
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    image="$(cd "$(dirname "${3:?AppImage}")" && pwd)/$(basename "$3")"
    manifest="$(cd "$(dirname "${4:?package-manifest.json}")" && pwd)/$(basename "$4")"
    set +e
    # An --internal network: the daemon needs a real (non-loopback) interface (with `--network none` its session preparation fails: dev1/dev2), but the network has no
    # route out, so the browser cannot reach anything but the controlled loopback target. The Engine also fails its p2p bind when the host has no default route (dev4: `engine error 1101`,
    # reproduced in a bare container), so the runner adds `default dev eth0` (NET_ADMIN) inside this internal network: a fixture requirement, recorded as an open Engine observation. bwrap (WebKit's sandbox in Epiphany) needs seccomp and systempaths unconfined.
    net="uc17c11-internal-$$"; docker network create --internal "$net" >/dev/null
    docker run --rm --init --platform linux/arm64 --network "$net" --shm-size 1g --device /dev/fuse --cap-add SYS_ADMIN --cap-add SYS_PTRACE --cap-add NET_ADMIN --security-opt apparmor:unconfined \
      --security-opt seccomp:unconfined --security-opt systempaths=unconfined \
      -v "$ROOT:/work:ro" -v "$out:/out" -v "$image:/in/appimage.AppImage:ro" -v "$manifest:/in/package-manifest.json:ro" \
      "${UC_REAL_IMAGE:?UC_REAL_IMAGE}" python3 /work/apps/gui-go/e2e/linux_appimage_real_helpers_run.py --out /out --appimage /in/appimage.AppImage \
      --manifest /in/package-manifest.json --desktop "${UC_HELPERS_DESKTOP:?UC_HELPERS_DESKTOP}" ${UC_REAL_ARGS:-} > "$out/run.log" 2>&1
    code=$?
    docker network rm "$net" >/dev/null
    exit "$code" ;;
  appimage-native-wayland-e2e)  # 17c13: <outdir> <AppImage> <weston|sway> <probe mode>: a headless compositor in a container (weston: no wlr-layer-shell; sway-nolib: protocol present, host library removed)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    image="$(cd "$(dirname "${3:?AppImage}")" && pwd)/$(basename "$3")"
    comp="${4:?weston|sway}"; pmode="${5:?probe mode}"
    case "$comp" in weston) cimage=uc-gui-go-linux-weston:17c13-b ;; sway) cimage=uc-gui-go-linux-sway-nolib:17c13 ;; *) echo "bad compositor" >&2; exit 2 ;; esac
    set +e
    net="uc17c13-internal-$$"; docker network create --internal "$net" >/dev/null
    docker run --rm --init --platform linux/arm64 --network "$net" --shm-size 1g --device /dev/fuse --cap-add SYS_ADMIN --cap-add SYS_PTRACE --cap-add NET_ADMIN --security-opt apparmor:unconfined \
      --security-opt seccomp:unconfined --security-opt systempaths=unconfined \
      -v "$ROOT:/work:ro" -v "$out:/out" -v "$image:/in/appimage.AppImage:ro" \
      "$cimage" bash /work/apps/gui-go/e2e/linux/native_wayland_container.sh "$comp" "$pmode" /out/run > "$out/run.log" 2>&1
    code=$?
    docker network rm "$net" >/dev/null
    exit "$code" ;;
  appimage-proxy-e2e)  # 17c12: system proxy (real tinyproxy + controlled HTTPS target, internal network). UC_PROXY_IMAGE selects the image; UC_PROXY_ARGS passes runner options (--scenarios ...)
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    image="$(cd "$(dirname "${3:?AppImage}")" && pwd)/$(basename "$3")"
    manifest="$(cd "$(dirname "${4:?package-manifest.json}")" && pwd)/$(basename "$4")"
    set +e
    # --internal: no route out (nothing can leave the container); NET_ADMIN lets the runner add the default route the Engine needs (17c11 R1)
    net="uc17c12-internal-$$"; docker network create --internal "$net" >/dev/null
    docker run --rm --init --platform linux/arm64 --network "$net" --device /dev/fuse --cap-add SYS_ADMIN --cap-add SYS_PTRACE --cap-add NET_ADMIN --security-opt apparmor:unconfined \
      -v "$ROOT:/work:ro" -v "$out:/out" -v "$image:/in/appimage.AppImage:ro" -v "$manifest:/in/package-manifest.json:ro" \
      "${UC_PROXY_IMAGE:-uc-gui-go-linux-proxy:17c12-ubuntu}" python3 /work/apps/gui-go/e2e/${UC_PROXY_RUNNER:-linux_appimage_proxy_run.py} --out /out --appimage /in/appimage.AppImage \
      --manifest /in/package-manifest.json ${UC_PROXY_ARGS:-} > "$out/run.log" 2>&1
    code=$?
    docker network rm "$net" >/dev/null
    exit "$code" ;;
  appimage-content-check)  # 17c7: extract the AppImage (kept in <outdir>/squashfs-root) and run the mechanical content assertions + the static dlopen audit
    out="$(mkdir -p "${2:?outdir}" && cd "$2" && pwd)"
    image="$(cd "$(dirname "${3:?AppImage}")" && pwd)/$(basename "$3")"
    manifest="$(cd "$(dirname "${4:?package-manifest.json}")" && pwd)/$(basename "$4")"
    docker run --rm --platform linux/arm64 -e UC_CONTENT_CHECK_ARGS="${UC_CONTENT_CHECK_ARGS:-}" -v "$ROOT:/work:ro" -v "$out:/out" -v "$image:/in/appimage.AppImage:ro" -v "$manifest:/in/package-manifest.json:ro" "$IMAGE" bash -c '
      set -e; cd /out; cp /in/appimage.AppImage ./x.AppImage; chmod +x ./x.AppImage; ./x.AppImage --appimage-extract > extract.log 2>&1
      python3 -I /work/apps/gui-go/e2e/linux/audit_dlopen.py /out/squashfs-root /out/dlopen-audit.json
      python3 -I /work/apps/gui-go/e2e/linux/appimage_content_check.py /out/squashfs-root /in/package-manifest.json /out/content-check.json ${UC_CONTENT_CHECK_ARGS:-}' ;;
  *) echo "usage: run.sh build|xvfb|package|daemon-release|release-e2e-build|package-release|package-appimage [outdir]" >&2; exit 2 ;;
esac
