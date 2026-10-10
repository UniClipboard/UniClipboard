#!/usr/bin/env bash
# AppImage acceptance for one architecture on the Docker host's native platform (docs/architecture/gui-go-linux-ci-packaging.md).
#
#   package_acceptance.sh <outdir> [--release-appimage <file> --release-manifest <package-manifest.json>]
#
# Needs: a clean commit, bun on the host, Docker, the release daemon and its build-evidence.txt in the cache volume
# (`run.sh daemon-release`), and these environment variables:
#   UC_LINUX_IMAGE          packaging build image (Dockerfile.package-build)
#   UC_LINUX_CACHE_VOLUME   Docker volume that holds /cache (cargo, tools, out-release)
#   UC_DOCKER_PLATFORM      linux/amd64 | linux/arm64: the platform of THIS host (never an emulated one)
#   UC_RUNTIME_IMAGES       space-separated clean-host images (Dockerfile.17c4-runtime: no GTK/WebKitGTK), e.g. the glibc floor
#                           distribution and a newer one; every image runs every scenario
#   UC_KEYRING_IMAGE        Secret Service image (Dockerfile.17c4-keyring)
#
# Builds the test-control-plane GUI (tags gtk3,production,release,e2e) and packages it as E2E-prefixed AppImages (v1, v2 with an update
# marker, and the no-relocation negative control), signs v2's updater archive with the isolated fixture key (e2e/updatetool), then per host
# image runs: full (launch with daemon and WebView, HTTPS, in-place update, refusal of an untrusted signature, autostart, data root),
# negative (the control must fail) and, when given, smoke on the release AppImage (no control plane by design).
# Nothing here is a release artifact and nothing is uploaded.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
out="${1:?outdir}"; shift
release_image=""; release_manifest=""
while [ $# -gt 0 ]; do
  case "$1" in
    --release-appimage) release_image="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
    --release-manifest) release_manifest="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
: "${UC_RUNTIME_IMAGES:?UC_RUNTIME_IMAGES}"; : "${UC_DOCKER_PLATFORM:?UC_DOCKER_PLATFORM}"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty: the run must be reproducible from a commit" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/HEAD.txt"
uname -m > "$out/host-machine.txt"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
fail=0
must() { "$@" || { echo "step failed, stopping: $*" >&2; exit 1; }; }

must step frontend-e2e bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go/frontend build"
must step build-cli env SKIP_DAEMON=1 "$R" build
must step build-gui "$R" release-e2e-build
must step package-v1 "$R" package-appimage "$out/v1"
must step package-v2 "$R" package-appimage "$out/v2" --update-marker v2-installed
must step package-negative "$R" package-appimage "$out/negative" --negative-control-no-relocation
v1="$(ls "$out"/v1/pkg/E2E-UniClipboard_*.AppImage)"; v2="$(ls "$out"/v2/pkg/E2E-UniClipboard_*.AppImage.tar.gz)"
neg="$(ls "$out"/negative/pkg/NEGCONTROL-UniClipboard_*.AppImage)"
must step feed "$R" appimage-feed "$out/feed" "$v2"
for host in $UC_RUNTIME_IMAGES; do
  tag="$(echo "$host" | tr ':/' '--')"
  step "e2e-full-$tag" env UC_RUNTIME_IMAGE="$host" "$R" appimage-e2e "$out/e2e-full-$tag" full "$v1" "$out/feed" "$out/v1/pkg/package-manifest.json" || fail=1
  step "e2e-negative-$tag" env UC_RUNTIME_IMAGE="$host" "$R" appimage-e2e "$out/e2e-negative-$tag" negative "$neg" || fail=1
  if [ -n "$release_image" ]; then
    step "e2e-smoke-$tag" env UC_RUNTIME_IMAGE="$host" "$R" appimage-e2e "$out/e2e-smoke-$tag" smoke "$release_image" "" "$release_manifest" || fail=1
  fi
done
echo "fail=$fail" | tee -a "$out/steps.txt"
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name '*.tar.gz' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' \) -print0 | sort -z | xargs -0 $(command -v sha256sum >/dev/null && echo sha256sum || echo 'shasum -a 256') > SHA256SUMS.txt )
exit "$fail"
