#!/usr/bin/env bash
# Reruns the whole 17c4 AppImage verification into a NEW directory (nothing existing is overwritten) and writes every step's
# complete output to <outdir>/logs. Run from a clean checkout; the daemon must have been built with `run.sh daemon-release`
# (its build-evidence.txt is checked against this checkout's daemon inputs by package_linux.py).
#   run_17c4.sh <outdir>
# Needs: Docker with the images uc-gui-go-linux-build:17c2, uc-gui-go-linux-runtime:17c4 and uc-gui-go-linux-keyring:17c4, bun on the host.
set -uo pipefail
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
step package-v2 "$R" package-appimage "$out/v2" --update-marker v2-installed || exit 1
step package-negative "$R" package-appimage "$out/negative" --negative-control-no-relocation || exit 1
step feed "$R" appimage-feed "$out/feed" "$out/v2/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage.tar.gz" || exit 1
step e2e-full "$R" appimage-e2e "$out/e2e-full" full "$out/v1/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage" "$out/feed" "$out/v1/pkg/package-manifest.json"; full=$?
step e2e-negative "$R" appimage-e2e "$out/e2e-negative" negative "$out/negative/pkg/NEGCONTROL-UniClipboard_1.1.1_aarch64.AppImage"; neg=$?
step frontend-release bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=0 bun --bun run --cwd apps/gui-go build" || exit 1
step package-release "$R" package-release "$out/release" || exit 1
step e2e-smoke "$R" appimage-e2e "$out/e2e-smoke" smoke "$out/release/packages/UniClipboard_1.1.1_aarch64.AppImage"; smoke=$?
echo "full=$full negative=$neg smoke=$smoke" | tee -a "$out/steps.txt"
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' -o -name '*.tar.gz' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' -o -name 'uniclipboard' \) -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS.txt )
[ "$full" = 0 ] && [ "$neg" = 0 ] && [ "$smoke" = 0 ]
