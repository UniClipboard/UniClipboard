#!/usr/bin/env bash
# Supplement for a run_17c7.sh run whose HARNESS (not the product) was fixed afterwards: it re-checks with the SAME retained packages, feed and images of that run
# (no product rebuild), from the current clean checkout, into a NEW directory. The product packages' SHA-256 are compared with the original run's SHA256SUMS.txt.
#   run_17c7_supplement.sh <final run dir> <outdir>
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
base="$(cd "${1:?final run dir}" && pwd)"
out="${2:?outdir}"; [ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/HEAD.txt"; cp "$base/HEAD.txt" "$out/original-run-HEAD.txt"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
( cd "$base" && shasum -a 256 -c SHA256SUMS.txt > "$out/logs/original-sha256-verify.log" 2>&1 ); sums=$?
echo "original packages unchanged (shasum -c SHA256SUMS.txt): rc=$sums" | tee -a "$out/steps.txt"
V1="$base/v1/pkg/E2E-UniClipboard_1.1.1_aarch64.AppImage"; M1="$base/v1/pkg/package-manifest.json"
NT="$base/negtls/pkg/NEGTLS-UniClipboard_1.1.1_aarch64.AppImage"; MN="$base/negtls/pkg/package-manifest.json"
OLD="${UC_OLD_PKG_DIR:-/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c6/final-3ed97b642/v1/pkg}"
step content-v1 "$R" appimage-content-check "$out/content-v1" "$V1" "$M1"; content=$?
step content-negtls env UC_CONTENT_CHECK_ARGS=--expect-no-tls-module "$R" appimage-content-check "$out/content-negtls" "$NT" "$MN"; contentneg=$?
step tls-ubuntu "$R" appimage-tls-e2e "$out/tls-ubuntu" "$V1" "$M1"; tlsu=$?
step tls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 "$R" appimage-tls-e2e "$out/tls-fedora" "$V1" "$M1"; tlsf=$?
step control-negtls-ubuntu env UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-negtls-ubuntu" "$NT" "$MN"; cnu=$?
step control-negtls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-negtls-fedora" "$NT" "$MN"; cnf=$?
step control-17c6-ubuntu env UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-17c6-ubuntu" "$OLD/E2E-UniClipboard_1.1.1_aarch64.AppImage" "$OLD/package-manifest.json"; c6u=$?
step control-17c6-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 UC_TLS_E2E_ARGS="--expect-tls absent" "$R" appimage-tls-e2e "$out/control-17c6-fedora" "$OLD/E2E-UniClipboard_1.1.1_aarch64.AppImage" "$OLD/package-manifest.json"; c6f=$?
c6f_ok=1; python3 -I - "$out/control-17c6-fedora/appimage-assertions.json" <<'PY' && c6f_ok=0
import json, sys
r = json.load(open(sys.argv[1]))
bad = [c['check'] for c in r['checks'] if not c['ok']]
sys.exit(0 if (not r['passed'] and bad and all(b.startswith('T1 the real bundled daemon') for b in bad)) else 1)
PY
[ "$c6f" != 0 ] || c6f_ok=1
echo "sums=$sums content=$content content-negtls=$contentneg tls-ubuntu=$tlsu tls-fedora=$tlsf control-negtls-ubuntu=$cnu control-negtls-fedora=$cnf control-17c6-ubuntu=$c6u control-17c6-fedora-rc=$c6f(expected non-zero) control-17c6-fedora-startup-failure-observed=$([ $c6f_ok = 0 ] && echo yes || echo NO)" | tee -a "$out/steps.txt"
[ "$sums" = 0 ] && [ "$content" = 0 ] && [ "$contentneg" = 0 ] && [ "$tlsu" = 0 ] && [ "$tlsf" = 0 ] && [ "$cnu" = 0 ] && [ "$cnf" = 0 ] && [ "$c6u" = 0 ] && [ "$c6f_ok" = 0 ]
