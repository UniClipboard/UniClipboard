#!/usr/bin/env bash
# Reruns the 17c10 verification (AppImage host helpers: xdg-open / gio / handler under AppRun's environment) into a NEW directory and writes every step's complete
# output to <outdir>/logs. Run from a clean checkout; the release daemon must be the 17c5 build (`run.sh daemon-release`, build-evidence.txt checked by package_linux.py).
#   run_17c10.sh <outdir> baseline|final
#     baseline  build + package + the four helper runs ({Ubuntu 24.04, Fedora 44} x {generic, gnome}); on a checkout WITHOUT the fix the A provenance checks are expected to fail
#     final     the same, plus the regression runs that the host-helper environment must not break: WebView HTTPS (17c7 TLS E2E, both distributions, trusted/untrusted CA),
#               portable-mode E2E (17c5, with the update feed), static content check
# Needs: Docker with uc-gui-go-linux-build:17c2, uc-gui-go-linux-runtime:17c7, uc-gui-go-linux-runtime-fedora:17c7; network for the image builds; bun on the host.
set -uo pipefail
export UC_LINUX_IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c2}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
E2E="$ROOT/apps/gui-go/e2e/linux"
out="${1:?outdir}"; kind="${2:?baseline|final}"
case "$kind" in baseline|final) ;; *) echo "mode must be baseline or final" >&2; exit 2 ;; esac
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs" "$out/inputs"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty: the run must be reproducible from a commit" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/inputs/head.txt"; git -C "$ROOT" status --porcelain > "$out/inputs/status.txt"; git -C "$ROOT" diff HEAD > "$out/inputs/dirty.diff"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
# The daemon is the one built in 17c5: refuse anything else before packaging (package_linux.py additionally checks the build evidence against this checkout).
step daemon-sha docker run --rm --platform linux/arm64 -v uc-gui-go-linux-cache:/cache ubuntu:24.04 sh -c 'sha256sum /cache/out-release/uniclipd; cat /cache/out-release/build-evidence.txt | head -4'
grep -q ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6 "$out/logs/daemon-sha.log" || { echo "daemon SHA-256 differs from the 17c5 release daemon" >&2; exit 1; }
# Images are built HERE; a failed build stops the run (an old tag must never stand in for the fixed environment). Their ids are archived.
step image-ubuntu docker build --platform linux/arm64 -t uc-gui-go-linux-runtime-helpers:17c10-ubuntu -f "$E2E/Dockerfile.17c10-ubuntu" "$E2E" || exit 1
step image-fedora docker build --platform linux/arm64 -t uc-gui-go-linux-runtime-helpers:17c10-fedora -f "$E2E/Dockerfile.17c10-fedora" "$E2E" || exit 1
mkdir -p "$out/images"
for img in uc-gui-go-linux-runtime-helpers:17c10-ubuntu uc-gui-go-linux-runtime-helpers:17c10-fedora uc-gui-go-linux-runtime:17c7 uc-gui-go-linux-runtime-fedora:17c7 "$UC_LINUX_IMAGE"; do
  n="$(echo "$img" | tr ':/' '__')"
  docker image inspect "$img" --format '{{.Id}}' > "$out/images/$n.id" || exit 1
done
for d in ubuntu fedora; do
  docker run --rm --platform linux/arm64 "uc-gui-go-linux-runtime-helpers:17c10-$d" sh -c 'cat /etc/os-release; echo ---; xdg-open --version; gio version; echo ---; (rpm -qa 2>/dev/null || dpkg-query -W) | sort' > "$out/images/helpers-$d.os-and-packages.txt" 2>&1 || exit 1
done
step frontend-e2e bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go build" || exit 1
grep -rl "__ucE2eOpenUrl" "$ROOT/apps/gui-go/frontend/dist/assets" > "$out/inputs/dist-contains-openurl-hook.txt" || { echo "dist lacks the E2E open-URL hook: stale frontend build" >&2; exit 1; }
(cd "$ROOT/apps/gui-go/frontend/dist" && find . -type f | LC_ALL=C sort | xargs shasum -a 256 > "$out/inputs/dist.sha256")
step build-gui "$R" release-e2e-build || exit 1
step package-v1 "$R" package-appimage "$out/v1" || exit 1
V1="$out/v1/pkg/E2E-UniClipboard_1.1.1_arm64.AppImage"; M1="$out/v1/pkg/package-manifest.json"
cp "$V1" "$out/inputs/appimage-as-run.AppImage"; shasum -a 256 "$V1" "$out/inputs/appimage-as-run.AppImage" > "$out/inputs/appimage.sha256"
rcs=""
for combo in ubuntu-generic ubuntu-gnome fedora-generic fedora-gnome; do
  d="${combo%-*}"; m="${combo#*-}"
  step "helpers-$combo" env UC_HELPERS_IMAGE="uc-gui-go-linux-runtime-helpers:17c10-$d" UC_HELPERS_DESKTOP="$m" "$R" appimage-helpers-e2e "$out/helpers-$combo" "$V1" "$M1"; rcs="$rcs helpers-$combo=$?"
done
echo "helpers:$rcs" | tee -a "$out/steps.txt"
if [ "$kind" = final ]; then
  step package-v2 "$R" package-appimage "$out/v2" --update-marker v2-installed || exit 1
  step feed "$R" appimage-feed "$out/feed" "$out/v2/pkg/E2E-UniClipboard_1.1.1_arm64.AppImage.tar.gz" || exit 1
  step content-v1 "$R" appimage-content-check "$out/content-v1" "$V1" "$M1"; content=$?
  step tls-ubuntu env UC_TLS_IMAGE=uc-gui-go-linux-runtime:17c7 "$R" appimage-tls-e2e "$out/tls-ubuntu" "$V1" "$M1"; tlsu=$?
  step tls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 "$R" appimage-tls-e2e "$out/tls-fedora" "$V1" "$M1"; tlsf=$?
  step e2e-portable "$R" appimage-portable-e2e "$out/e2e-portable" "$V1" "$out/feed" "$M1"; portable=$?
  echo "content=$content tls-ubuntu=$tlsu tls-fedora=$tlsf portable=$portable" | tee -a "$out/steps.txt"
fi
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' -o -name 'content-check.json' \) -not -path '*/squashfs-root/*' -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS.txt )
for f in "$out"/helpers-*/appimage-assertions.json; do python3 -I -c "import json,sys;r=json.load(open(sys.argv[1]));print(sys.argv[1].split('/')[-2], 'checks', len(r['checks']), 'failed', sum(not c['ok'] for c in r['checks']), 'passed', r['passed'])" "$f"; done | tee -a "$out/steps.txt"
