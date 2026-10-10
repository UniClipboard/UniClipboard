#!/usr/bin/env bash
# Reruns the 17c11 verification into a NEW directory and writes every step's complete output (stdout+stderr) and exit code to <outdir>/logs, <outdir>/steps.txt.
# Run from a clean checkout; the release daemon must be the 17c5 build (`run.sh daemon-release`, build-evidence.txt checked by package_linux.py).
#   run_17c11.sh <outdir>
# Stages: build the images (a tag is REMOVED before its build, so a failed build can never leave an older image standing in; ids and `docker image inspect` are archived),
#         build + package ONE AppImage, then
#   real      the 17c11 real-application E2E, non-portable: {Ubuntu 24.04 (Epiphany/Nautilus/Loupe), Fedora 44 (Firefox/Nautilus/Loupe)} x {generic, gnome} (+ F7 record on gnome)
#   control   the same E2E on the 17c10 PRE-FIX AppImage (Fedora, gnome): the real applications must expose the defect (negative control, passed=false is the expected result)
#   engine    diagnosis of the Engine's default-route requirement (NOT a fix); xdg-open generic/GNOME dispatch lifecycle (foreground wait vs gio service)
#   regress   17c10 helpers x4 (sh recorder), 17c7 WebView TLS x2, 17c5 portable E2E, static content check
# Needs: UC_OLD_APPIMAGE (the pre-fix AppImage for the control stage), Docker with uc-gui-go-linux-build:17c2, uc-gui-go-linux-runtime:17c7, uc-gui-go-linux-runtime-fedora:17c7; network for the image builds; bun on the host.
set -uo pipefail
export UC_LINUX_IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c2}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
E2E="$ROOT/apps/gui-go/e2e/linux"
OLD_APPIMAGE="${UC_OLD_APPIMAGE:?UC_OLD_APPIMAGE: the 17c10 pre-fix AppImage (baseline-03d304fb5/inputs/appimage-as-run.AppImage; its package-manifest.json is expected at ../v1/pkg/ next to inputs/)}"
out="${1:?outdir}"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs" "$out/inputs" "$out/images"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty: the run must be reproducible from a commit" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/inputs/head.txt"; git -C "$ROOT" status --porcelain > "$out/inputs/status.txt"; git -C "$ROOT" diff HEAD > "$out/inputs/dirty.diff"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
step daemon-sha docker run --rm --platform linux/arm64 -v uc-gui-go-linux-cache:/cache ubuntu:24.04 sh -c 'sha256sum /cache/out-release/uniclipd; cat /cache/out-release/build-evidence.txt | head -4'
grep -q ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6 "$out/logs/daemon-sha.log" || { echo "daemon SHA-256 differs from the 17c5 release daemon" >&2; exit 1; }
build_image() { # tag dockerfile : remove the tag first, full output + the real exit code in logs/image-<tag>.log, id + inspect archived
  local tag="$1" file="$2" n; n="$(echo "$tag" | tr ':/' '__')"
  docker rmi -f "$tag" > "$out/logs/rmi-$n.log" 2>&1 || true
  step "image-$n" docker build --platform linux/arm64 --progress=plain -t "$tag" -f "$E2E/$file" "$E2E" || { echo "image build failed: $tag" >&2; exit 1; }
  docker image inspect "$tag" > "$out/images/$n.inspect.json" || exit 1
  docker image inspect "$tag" --format '{{.Id}}' > "$out/images/$n.id" || exit 1
}
build_image uc-gui-go-linux-real-apps:17c11-ubuntu Dockerfile.17c11-ubuntu
build_image uc-gui-go-linux-real-apps:17c11-fedora Dockerfile.17c11-fedora
build_image uc-gui-go-linux-runtime-helpers:17c10-ubuntu Dockerfile.17c10-ubuntu
build_image uc-gui-go-linux-runtime-helpers:17c10-fedora Dockerfile.17c10-fedora
for img in uc-gui-go-linux-runtime:17c7 uc-gui-go-linux-runtime-fedora:17c7 "$UC_LINUX_IMAGE"; do
  n="$(echo "$img" | tr ':/' '__')"; docker image inspect "$img" --format '{{.Id}}' > "$out/images/$n.id" || exit 1
done
for d in ubuntu fedora; do
  docker run --rm --platform linux/arm64 "uc-gui-go-linux-real-apps:17c11-$d" sh -c 'cat /etc/os-release; echo ---; (rpm -qa 2>/dev/null || dpkg-query -W) | sort' > "$out/images/real-apps-$d.os-and-packages.txt" 2>&1 || exit 1
done
step frontend-e2e bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go/frontend build" || exit 1
grep -rl "__ucE2eOpenUrl" "$ROOT/apps/gui-go/frontend/dist/assets" > "$out/inputs/dist-contains-openurl-hook.txt" || { echo "dist lacks the E2E open-URL hook: stale frontend build" >&2; exit 1; }
(cd "$ROOT/apps/gui-go/frontend/dist" && find . -type f | LC_ALL=C sort | xargs shasum -a 256 > "$out/inputs/dist.sha256")
step build-gui "$R" release-e2e-build || exit 1
step package-v1 "$R" package-appimage "$out/v1" || exit 1
V1="$out/v1/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage"; M1="$out/v1/pkg/package-manifest.json"
cp "$V1" "$out/inputs/appimage-as-run.AppImage"; shasum -a 256 "$V1" "$out/inputs/appimage-as-run.AppImage" > "$out/inputs/appimage.sha256"
FIREFOX_ARGS="--browser-exe firefox --browser-ua Firefox/ --browser-desktop org.mozilla.firefox.desktop"
rcs=""
for combo in ubuntu-gnome ubuntu-generic fedora-gnome fedora-generic; do
  d="${combo%-*}"; m="${combo#*-}"; extra="--f7"; [ "$d" = fedora ] && extra="--f7 $FIREFOX_ARGS"
  step "real-$combo" env UC_REAL_IMAGE="uc-gui-go-linux-real-apps:17c11-$d" UC_HELPERS_DESKTOP="$m" UC_REAL_ARGS="$extra" "$R" appimage-real-e2e "$out/real-$combo" "$V1" "$M1"; rcs="$rcs real-$combo=$?"
done
echo "real:$rcs" | tee -a "$out/steps.txt"
# negative control: the PRE-FIX AppImage (17c10 baseline) under the same real-application E2E; passed=false is the EXPECTED outcome
shasum -a 256 "$OLD_APPIMAGE" > "$out/inputs/old-appimage.sha256"
step control-prefix-fedora-gnome env UC_REAL_IMAGE=uc-gui-go-linux-real-apps:17c11-fedora UC_HELPERS_DESKTOP=gnome UC_REAL_ARGS="$FIREFOX_ARGS" "$R" appimage-real-e2e "$out/control-prefix-fedora-gnome" "$OLD_APPIMAGE" "$(dirname "$OLD_APPIMAGE")/../v1/pkg/package-manifest.json"; echo "control-prefix rc=$? (non-zero expected)" | tee -a "$out/steps.txt"
step engine-default-route "$E2E/diag_engine_default_route.sh" "$out/engine-default-route" "$V1" uc-gui-go-linux-real-apps:17c11-ubuntu
step xdg-open-dispatch-ubuntu "$E2E/diag_xdg_open_generic.sh" "$out/xdg-open-dispatch-ubuntu" uc-gui-go-linux-real-apps:17c11-ubuntu
step xdg-open-dispatch-fedora "$E2E/diag_xdg_open_generic.sh" "$out/xdg-open-dispatch-fedora" uc-gui-go-linux-real-apps:17c11-fedora
step package-v2 "$R" package-appimage "$out/v2" --update-marker v2-installed || exit 1
step feed "$R" appimage-feed "$out/feed" "$out/v2/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage.tar.gz" || exit 1
for combo in ubuntu-generic ubuntu-gnome fedora-generic fedora-gnome; do
  d="${combo%-*}"; m="${combo#*-}"
  step "helpers-$combo" env UC_HELPERS_IMAGE="uc-gui-go-linux-runtime-helpers:17c10-$d" UC_HELPERS_DESKTOP="$m" "$R" appimage-helpers-e2e "$out/helpers-$combo" "$V1" "$M1"; rcs="$rcs helpers-$combo=$?"
done
step content-v1 "$R" appimage-content-check "$out/content-v1" "$V1" "$M1"; content=$?
step tls-ubuntu env UC_TLS_IMAGE=uc-gui-go-linux-runtime:17c7 "$R" appimage-tls-e2e "$out/tls-ubuntu" "$V1" "$M1"; tlsu=$?
step tls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 "$R" appimage-tls-e2e "$out/tls-fedora" "$V1" "$M1"; tlsf=$?
step e2e-portable "$R" appimage-portable-e2e "$out/e2e-portable" "$V1" "$out/feed" "$M1"; portable=$?
echo "all:$rcs content=$content tls-ubuntu=$tlsu tls-fedora=$tlsf portable=$portable" | tee -a "$out/steps.txt"
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' -o -name 'content-check.json' \) -not -path '*/squashfs-root/*' -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS.txt )
for f in "$out"/real-*/appimage-assertions.json "$out"/control-prefix-*/appimage-assertions.json "$out"/helpers-*/appimage-assertions.json; do python3 -I -c "import json,sys;r=json.load(open(sys.argv[1]));print(sys.argv[1].split('/')[-2], 'checks', len(r['checks']), 'failed', sum(not c['ok'] for c in r['checks']), 'passed', r['passed'])" "$f"; done | tee -a "$out/steps.txt"
# Exit status: 0 only if every stage that must pass did (real-application matrix, regressions) AND the pre-fix control failed as expected.
bad=0
for f in "$out"/real-*/appimage-assertions.json "$out"/helpers-*/appimage-assertions.json "$out"/tls-*/appimage-assertions.json "$out"/e2e-portable/appimage-assertions.json "$out"/content-v1/content-check.json; do
  python3 -I -c "import json,sys;sys.exit(0 if json.load(open(sys.argv[1])).get('passed') is True else 1)" "$f" || { echo "NOT PASSED: $f" | tee -a "$out/steps.txt"; bad=1; }
done
python3 -I -c "import json,sys;sys.exit(1 if json.load(open(sys.argv[1])).get('passed') is True else 0)" "$out/control-prefix-fedora-gnome/appimage-assertions.json" || { echo "CONTROL DID NOT FAIL: the pre-fix package passed the real-application E2E" | tee -a "$out/steps.txt"; bad=1; }
exit $bad
