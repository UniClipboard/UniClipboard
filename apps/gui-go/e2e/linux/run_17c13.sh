#!/usr/bin/env bash
# Final verification of slice 17c13 (AppImage native Wayland + Layer Shell) on packages built from ONE clean commit. Every step's complete output and exit code goes to
# <outdir>/logs and <outdir>/steps.txt; nothing is deleted or overwritten (the outdir must be empty).
#   run_17c13.sh <outdir>
# Container part only: build + package (final v1, X11HOOK- differential control, NOLAYER- missing-library control, v2 update target), content checks, the Weston
# (no wlr-layer-shell) and sway-without-library controls, and the regressions the changed GTK hook can affect (host helpers, TLS, portable + update gate, full AppImage run).
# The two native hosts (Fedora niri, Omarchy Hyprland) are driven afterwards with the SAME final packages by `native_wayland_probe.py` (see the architecture document); their
# runs live next to this directory and are not part of this script. The proxy matrix of 17c12 is NOT repeated (see the document for why).
# Needs: UC_FEED_INPUTS (pubkey.b64 + good.sig.b64), the images of 17c4/17c7/17c10/17c13, bun, Docker.
set -uo pipefail
export UC_LINUX_IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c2}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
FEED_SRC="${UC_FEED_INPUTS:?UC_FEED_INPUTS: a directory with pubkey.b64 and good.sig.b64}"
out="${1:?outdir}"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs" "$out/inputs"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty: the run must be reproducible from a commit" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/inputs/head.txt"; git -C "$ROOT" status --porcelain > "$out/inputs/status.txt"; git -C "$ROOT" diff HEAD > "$out/inputs/dirty.diff"
[ ! -s "$out/inputs/dirty.diff" ] || { echo "dirty.diff is not empty" >&2; exit 2; }
cp "$FEED_SRC/pubkey.b64" "$FEED_SRC/good.sig.b64" "$out/inputs/" && shasum -a 256 "$out/inputs/pubkey.b64" "$out/inputs/good.sig.b64" > "$out/inputs/feed-inputs.sha256"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
step daemon-sha docker run --rm --platform linux/arm64 -v uc-gui-go-linux-cache:/cache ubuntu:24.04 sh -c 'sha256sum /cache/out-release/uniclipd; head -4 /cache/out-release/build-evidence.txt'
grep -q ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6 "$out/logs/daemon-sha.log" || { echo "daemon SHA-256 differs from the pinned release daemon" >&2; exit 1; }
for img in uc-gui-go-linux-weston:17c13-b uc-gui-go-linux-sway-nolib:17c13 uc-gui-go-linux-runtime:17c4 uc-gui-go-linux-runtime:17c7 uc-gui-go-linux-runtime-fedora:17c7 uc-gui-go-linux-runtime-helpers:17c10-ubuntu uc-gui-go-linux-runtime-helpers:17c10-fedora; do
  docker image inspect "$img" --format '{{.Id}}' > "$out/inputs/image-$(echo "$img" | tr ':/' '__').id" || { echo "missing image $img" >&2; exit 1; }
done
step frontend-e2e bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go build" || exit 1
step build-gui "$R" release-e2e-build || exit 1
step package-v1 "$R" package-appimage "$out/v1" || exit 1
V1="$out/v1/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage"; M1="$out/v1/pkg/package-manifest.json"
shasum -a 256 "$V1" "$M1" "$out/v1/pkg/uniclipboard" | tee "$out/inputs/package.sha256"
step package-x11hook "$R" package-appimage "$out/x11hook" --negative-control-keep-x11-hook || exit 1
step package-nolayer "$R" package-appimage "$out/nolayer" --negative-control-no-layer-shell || exit 1
step package-v2 "$R" package-appimage "$out/v2" --update-marker v2-installed || exit 1
V2="$out/v2/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage"
shasum -a 256 "$out"/x11hook/pkg/*.AppImage "$out"/nolayer/pkg/*.AppImage "$V2" "$V2.tar.gz" | tee -a "$out/inputs/package.sha256"
step content-v1 "$R" appimage-content-check "$out/content-v1" "$V1" "$M1"; content=$?
step content-x11hook env UC_CONTENT_CHECK_ARGS=--expect-x11-hook "$R" appimage-content-check "$out/content-x11hook" "$out"/x11hook/pkg/X11HOOK-*.AppImage "$out/x11hook/pkg/package-manifest.json"; cx=$?
step content-nolayer env UC_CONTENT_CHECK_ARGS=--expect-no-layer-shell "$R" appimage-content-check "$out/content-nolayer" "$out"/nolayer/pkg/NOLAYER-*.AppImage "$out/nolayer/pkg/package-manifest.json"; cn=$?
step weston "$R" appimage-native-wayland-e2e "$out/weston-no-layer-protocol" "$V1" weston native-no-layer-protocol; weston=$?
step sway-nolib "$R" appimage-native-wayland-e2e "$out/sway-missing-library" "$out"/nolayer/pkg/NOLAYER-*.AppImage sway native-missing-library; sway=$?
step feed "$R" appimage-feed "$out/feed" "$V2.tar.gz" || exit 1
rcs=""
for combo in ubuntu-generic ubuntu-gnome fedora-generic fedora-gnome; do
  d="${combo%-*}"; m="${combo#*-}"
  step "helpers-$combo" env UC_HELPERS_IMAGE="uc-gui-go-linux-runtime-helpers:17c10-$d" UC_HELPERS_DESKTOP="$m" "$R" appimage-helpers-e2e "$out/helpers-$combo" "$V1" "$M1"; rcs="$rcs helpers-$combo=$?"
done
step tls-ubuntu env UC_TLS_IMAGE=uc-gui-go-linux-runtime:17c7 "$R" appimage-tls-e2e "$out/tls-ubuntu" "$V1" "$M1"; tlsu=$?
step tls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 "$R" appimage-tls-e2e "$out/tls-fedora" "$V1" "$M1"; tlsf=$?
step e2e-portable "$R" appimage-portable-e2e "$out/e2e-portable" "$V1" "$out/feed" "$M1"; portable=$?
step e2e-full "$R" appimage-e2e "$out/e2e-full" full "$V1" "$out/feed" "$M1"; full=$?
echo "content=$content content-x11hook=$cx content-nolayer=$cn weston=$weston sway-nolib=$sway regress:$rcs tls-ubuntu=$tlsu tls-fedora=$tlsf portable=$portable full=$full" | tee -a "$out/steps.txt"
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' -o -name 'content-check.json' -o -name 'native-wayland-result.json' \) -not -path '*/squashfs-root/*' -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS.txt )
bad=0
for f in "$out"/helpers-*/appimage-assertions.json "$out"/tls-*/appimage-assertions.json "$out"/e2e-portable/appimage-assertions.json "$out"/e2e-full/appimage-assertions.json; do
  python3 -I -c "import json,sys;sys.exit(0 if json.load(open(sys.argv[1])).get('passed') is True else 1)" "$f" || { echo "NOT PASSED: $f" | tee -a "$out/steps.txt"; bad=1; }
done
for f in "$out"/content-*/content-check.json "$out"/weston-no-layer-protocol/run/probe/native-wayland-result.json "$out"/sway-missing-library/run/probe/native-wayland-result.json; do
  python3 -I -c "import json,sys;sys.exit(0 if json.load(open(sys.argv[1])).get('passed') is True else 1)" "$f" || { echo "NOT PASSED: $f" | tee -a "$out/steps.txt"; bad=1; }
done
exit $bad
