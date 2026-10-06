#!/usr/bin/env bash
# Reruns the whole 17c6 pinned-runtime AppImage verification (the 17c5 portable scenario and the 17c4 non-portable regression are part of it) into a NEW directory (nothing existing is overwritten) and writes every step's
# complete output to <outdir>/logs. Run from a clean checkout; the daemon must have been built with `run.sh daemon-release`
# (its build-evidence.txt is checked against this checkout's daemon inputs by package_linux.py).
#   run_17c6.sh <outdir>
# Needs: Docker with the images uc-gui-go-linux-build:17c2, uc-gui-go-linux-runtime:17c4 and uc-gui-go-linux-keyring:17c4, bun on the host.
set -uo pipefail
export UC_LINUX_IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c2}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
out="${1:?outdir}"; [ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty: the run must be reproducible from a commit" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/HEAD.txt"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
step frontend-e2e bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go build" || exit 1
step build-gui "$R" release-e2e-build || exit 1
step package-v1 "$R" package-appimage "$out/v1" || exit 1
# 17c6: v2 is packed with NO network in its container: with the pinned runtime cached by package-v1, appimagetool cannot (and must not) download one.
step defect-repro-no-runtime-file docker run --rm --network none --platform linux/arm64 -e ARCH=aarch64 -v uc-gui-go-linux-cache:/cache:ro "$UC_LINUX_IMAGE" bash -c 'mkdir -p /ad && printf "#!/bin/sh\n" > /ad/AppRun && chmod +x /ad/AppRun && printf "[Desktop Entry]\nType=Application\nName=T\nExec=x\nIcon=t\nCategories=Utility;\n" > /ad/t.desktop && printf x > /ad/t.png && /cache/tools/appimagetool --appimage-extract-and-run -v --no-appstream /ad /tmp/x.AppImage; echo rc=$?' 
step package-v2-offline env UC_PACKAGE_DOCKER_ARGS="--network none" "$R" package-appimage "$out/v2" --update-marker v2-installed || exit 1
step runtime-pin-negative "$ROOT/apps/gui-go/e2e/linux/runtime_pin_negative.sh" "$out/runtime-pin-negative"; rpn=$?
step package-negative "$R" package-appimage "$out/negative" --negative-control-no-relocation || exit 1
step feed "$R" appimage-feed "$out/feed" "$out/v2/pkg/E2E-UniClipboard_1.1.1_arm64.AppImage.tar.gz" || exit 1
step probe-portable-home docker run --rm --platform linux/arm64 --device /dev/fuse --cap-add SYS_ADMIN --security-opt apparmor:unconfined -v uc-gui-go-linux-cache:/cache:ro -v "$ROOT:/work:ro" uc-gui-go-linux-runtime:17c4 bash /work/apps/gui-go/e2e/linux/probe_portable_home.sh
step probe-pixbuf-mime docker run --rm --platform linux/arm64 --device /dev/fuse --cap-add SYS_ADMIN --security-opt apparmor:unconfined -v "$ROOT:/work:ro" -v "$out/v1/pkg:/in:ro" uc-gui-go-linux-runtime:17c4 bash /work/apps/gui-go/e2e/linux/probe_pixbuf_mime.sh /in/E2E-UniClipboard_1.1.1_arm64.AppImage
step e2e-portable "$R" appimage-portable-e2e "$out/e2e-portable" "$out/v1/pkg/E2E-UniClipboard_1.1.1_arm64.AppImage" "$out/feed" "$out/v1/pkg/package-manifest.json"; portable=$?
step e2e-full "$R" appimage-e2e "$out/e2e-full" full "$out/v1/pkg/E2E-UniClipboard_1.1.1_arm64.AppImage" "$out/feed" "$out/v1/pkg/package-manifest.json"; full=$?
step e2e-negative "$R" appimage-e2e "$out/e2e-negative" negative "$out/negative/pkg/NEGCONTROL-UniClipboard_1.1.1_arm64.AppImage"; neg=$?
step frontend-release bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=0 bun --bun run --cwd apps/gui-go build" || exit 1
step package-release "$R" package-release "$out/release" || exit 1
step e2e-smoke "$R" appimage-e2e "$out/e2e-smoke" smoke "$out/release/packages/UniClipboard_1.1.1_arm64.AppImage"; smoke=$?
step runtime-identity python3 -I "$ROOT/apps/gui-go/e2e/linux/runtime_pin_check.py" "$out/v1/pkg/package-manifest.json" "$out/v2/pkg/package-manifest.json" "$out/negative/pkg/package-manifest.json" "$out/release/packages/package-manifest.json"; ident=$?
echo "portable=$portable full=$full negative=$neg smoke=$smoke runtime-pin-negative=$rpn runtime-identity=$ident" | tee -a "$out/steps.txt"
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' -o -name '*.tar.gz' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' -o -name 'uniclipboard' \) -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS.txt )
[ "$portable" = 0 ] && [ "$full" = 0 ] && [ "$neg" = 0 ] && [ "$smoke" = 0 ] && [ "$rpn" = 0 ] && [ "$ident" = 0 ]
