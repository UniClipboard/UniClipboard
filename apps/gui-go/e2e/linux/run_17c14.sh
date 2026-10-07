#!/usr/bin/env bash
# 17c14 tray E2E driver (host side, Docker). Builds the gtk3,e2e GUI twice inside uc-gui-go-linux-build:17c9-wm:
#   before  the tray files of the commit given as $2 (default HEAD): the negative control, expected to fail checks 5 and 8
#   after   the working tree (must be committed by the caller for provenance; dirty state is archived)
# and runs apps/gui-go/e2e/linux_tray_run.py against each under Xvfb + a private D-Bus session.
#   run_17c14.sh <outdir> [<before-commit>]      needs network (production rendezvous service) and /cache/out/{uniclipd,uniclip}
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
out="${1:?outdir}"; BEFORE="${2:-HEAD}"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
git -C "$ROOT" rev-parse HEAD > "$out/head.txt"; git -C "$ROOT" status --porcelain > "$out/status.txt"; git -C "$ROOT" diff HEAD > "$out/dirty.diff"
git -C "$ROOT" rev-parse "$BEFORE" > "$out/before-commit.txt"
GITCOMMON="$(cd "$ROOT" && cd "$(git rev-parse --git-common-dir)" && pwd -P)"
docker run --rm --platform linux/arm64 -v "$ROOT:/work:ro" --mount "type=bind,src=$GITCOMMON,dst=$GITCOMMON,readonly" -e GIT_OPTIONAL_LOCKS=0 \
  -v uc-gui-go-linux-cache:/cache -v "$out:/out" -e RUNS="${RUNS:-before after}" -e BEFORE="$(cat "$out/before-commit.txt")" uc-gui-go-linux-build:17c9-wm bash -c '
set -uo pipefail
export GOPATH=/cache/gopath GOFLAGS=-mod=mod CGO_ENABLED=1
git config --global --add safe.directory /work
LD="-X main.updaterPublicKey= -X main.productName=UniClipboard -X main.bundleID=app.uniclipboard.desktop.e2e"
build() { # name srcdir
  mkdir -p /out/bin/$1 && (cd $2/apps/gui-go && go build -tags gtk3,e2e -ldflags "$LD" -o /out/bin/$1/gui-go . && CGO_ENABLED=0 go build -o /out/bin/$1/daemonget ./e2e/linux/daemonget) > /out/build-$1.log 2>&1; echo "build $1 rc=$?"; }
# after: the working tree (read-only mount: build from a copy so go can write)
mkdir -p /src/after && cp -a /work/apps /work/packages /src/after/ && build after /src/after
# before: the same tree with the tray files of the base commit
mkdir -p /src/before && cp -a /src/after/apps /src/after/packages /src/before/
for f in tray.go tray_devices.go; do git -C /work show $BEFORE:apps/gui-go/$f > /src/before/apps/gui-go/$f; done
rm -f /src/before/apps/gui-go/tray_publish_*.go
build before /src/before
for n in before after; do
  cp /cache/out/uniclipd /cache/out/uniclip /out/bin/$n/
  sha256sum /out/bin/$n/* > /out/bin/$n/SHA256SUMS.txt
done
cat /cache/out/head.txt > /out/daemon-cli-built-from.txt
for n in ${RUNS:-before after}; do
  mkdir -p /out/run-$n
  Xvfb :99 -screen 0 1280x800x24 -nolisten tcp & X=$!
  export DISPLAY=:99; for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
  dbus-run-session -- python3 /work/apps/gui-go/e2e/linux_tray_run.py --out /out/run-$n --binaries /out/bin/$n --tag $n > /out/run-$n/stdout.txt 2>&1
  echo "run $n rc=$?" | tee -a /out/steps.txt
  kill $X 2>/dev/null; wait $X 2>/dev/null
done
'
