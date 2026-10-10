#!/usr/bin/env bash
# Reruns the whole 17c7 verification (cross-distribution runtime dependencies: WebView HTTPS through the bundled GIO TLS module, libdbus, libglvnd
# origin) into a NEW directory and writes every step's complete output to <outdir>/logs. Run from a clean checkout; the release daemon must have been built
# with `run.sh daemon-release` (its build-evidence.txt is checked against this checkout's daemon inputs by package_linux.py).
#   run_17c7.sh <outdir>
# Needs: Docker with uc-gui-go-linux-build:17c2, uc-gui-go-linux-runtime:17c4, uc-gui-go-linux-keyring:17c4; network for the image builds; bun on the host.
# Optional: UC_OLD_PKG_DIR = a 17c6 package dir (v1/pkg) for the historic red controls.
set -uo pipefail
export UC_LINUX_IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c2}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
OLD="${UC_OLD_PKG_DIR:-/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c6/final-3ed97b642/v1/pkg}"
out="${1:?outdir}"; [ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty: the run must be reproducible from a commit" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/HEAD.txt"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
E2E="$ROOT/apps/gui-go/e2e/linux"
# The images are built HERE and the run stops when a build fails: an old tag must never stand in for the fixed environment.
step image-ubuntu docker build --platform linux/arm64 -t uc-gui-go-linux-runtime:17c7 -f "$E2E/Dockerfile.17c7-ubuntu-runtime" "$E2E" || exit 1
step image-fedora docker build --platform linux/arm64 -t uc-gui-go-linux-runtime-fedora:17c7 -f "$E2E/Dockerfile.17c7-fedora" "$E2E" || exit 1
mkdir -p "$out/images"
for img in uc-gui-go-linux-runtime:17c7 uc-gui-go-linux-runtime-fedora:17c7; do
  n="$(echo "$img" | tr ':/' '__')"
  docker image inspect "$img" --format '{{.Id}}' > "$out/images/$n.id" || exit 1
  docker run --rm --platform linux/arm64 "$img" sh -c 'cat /etc/os-release; echo ---; test -s /etc/machine-id && echo "machine-id present"; command -v dbus-launch readelf; echo ---; (rpm -qa 2>/dev/null || dpkg-query -W) | sort' > "$out/images/$n.os-and-packages.txt" 2>&1 || exit 1
done
step frontend-e2e bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go/frontend build" || exit 1
step build-gui "$R" release-e2e-build || exit 1
step package-v1 "$R" package-appimage "$out/v1" || exit 1
step package-v2 "$R" package-appimage "$out/v2" --update-marker v2-installed || exit 1
step package-negtls "$R" package-appimage "$out/negtls" --negative-control-no-tls-module || exit 1
step package-negative "$R" package-appimage "$out/negative" --negative-control-no-relocation || exit 1
step feed "$R" appimage-feed "$out/feed" "$out/v2/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage.tar.gz" || exit 1
V1="$out/v1/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage"; M1="$out/v1/pkg/package-manifest.json"
NT="$out/negtls/pkg/NEGTLS-UniClipboard_1.1.1_aarch64.AppImage"; MN="$out/negtls/pkg/package-manifest.json"
step content-v1 "$R" appimage-content-check "$out/content-v1" "$V1" "$M1"; content=$?
step content-negtls env UC_CONTENT_CHECK_ARGS=--expect-no-tls-module "$R" appimage-content-check "$out/content-negtls" "$NT" "$MN"; contentneg=$?
step tls-ubuntu "$R" appimage-tls-e2e "$out/tls-ubuntu" "$V1" "$M1"; tlsu=$?
step tls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 "$R" appimage-tls-e2e "$out/tls-fedora" "$V1" "$M1"; tlsf=$?
step control-negtls-ubuntu env UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-negtls-ubuntu" "$NT" "$MN"; cnu=$?
step control-negtls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-negtls-fedora" "$NT" "$MN"; cnf=$?
OLDPKG="$OLD/E2E-UniClipboard_1.1.1_aarch64.AppImage"; OLDM="$OLD/package-manifest.json"
step control-17c6-ubuntu env UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-17c6-ubuntu" "$OLDPKG" "$OLDM"; c6u=$?
# On Fedora the 17c6 package does not even start (its bundled libdbus breaks the host dbus-launch). That is NOT TLS evidence. The expected outcome is asserted:
# the step must exit non-zero AND the failing stage must be the daemon start (T1), checked below from the assertions file.
step control-17c6-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-17c6-fedora" "$OLDPKG" "$OLDM"; c6f=$?
# Regression of what the AppDir change can affect: portable (unprivileged, no Secret Service), full (non-portable, real update/restart), negative control, release smoke.
step e2e-portable "$R" appimage-portable-e2e "$out/e2e-portable" "$V1" "$out/feed" "$M1"; portable=$?
step e2e-full "$R" appimage-e2e "$out/e2e-full" full "$V1" "$out/feed" "$M1"; full=$?
step e2e-negative "$R" appimage-e2e "$out/e2e-negative" negative "$out/negative/pkg/NEGCONTROL-UniClipboard_1.1.1_aarch64.AppImage"; neg=$?
step frontend-release bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=0 bun --bun run --cwd apps/gui-go/frontend build" || exit 1
step package-release "$R" package-release "$out/release" || exit 1
step e2e-smoke "$R" appimage-e2e "$out/e2e-smoke" smoke "$out/release/packages/UniClipboard_1.1.1_aarch64.AppImage"; smoke=$?
step runtime-identity python3 -I "$ROOT/apps/gui-go/e2e/linux/runtime_pin_check.py" "$M1" "$out/v2/pkg/package-manifest.json" "$out/negative/pkg/package-manifest.json" "$out/release/packages/package-manifest.json"; ident=$?
c6f_ok=1; python3 -I - "$out/control-17c6-fedora/appimage-assertions.json" <<'PY' && c6f_ok=0
import json, sys
r = json.load(open(sys.argv[1]))
bad = [c['check'] for c in r['checks'] if not c['ok']]
sys.exit(0 if (not r['passed'] and bad and all(b.startswith('T1 the real bundled daemon') for b in bad)) else 1)
PY
[ "$c6f" != 0 ] || c6f_ok=1
echo "control-17c6-fedora-expected-startup-failure-observed=$([ $c6f_ok = 0 ] && echo yes || echo NO)" | tee -a "$out/steps.txt"
echo "content=$content content-negtls=$contentneg tls-ubuntu=$tlsu tls-fedora=$tlsf control-negtls-ubuntu=$cnu control-negtls-fedora=$cnf control-17c6-ubuntu=$c6u control-17c6-fedora(expected non-zero)=$c6f portable=$portable full=$full negative=$neg smoke=$smoke runtime-identity=$ident" | tee -a "$out/steps.txt"
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' -o -name '*.tar.gz' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' -o -name 'content-check.json' -o -name 'uniclipboard' \) -not -path '*/squashfs-root/*' -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS.txt )
[ "$content" = 0 ] && [ "$contentneg" = 0 ] && [ "$tlsu" = 0 ] && [ "$tlsf" = 0 ] && [ "$cnu" = 0 ] && [ "$cnf" = 0 ] && [ "$c6u" = 0 ] && [ "$c6f_ok" = 0 ] && [ "$portable" = 0 ] && [ "$full" = 0 ] && [ "$neg" = 0 ] && [ "$smoke" = 0 ] && [ "$ident" = 0 ]
