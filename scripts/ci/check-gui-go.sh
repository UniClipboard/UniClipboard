#!/usr/bin/env bash
# Compile gate for the Go/Wails desktop host (apps/gui-go) and the shared desktop-host-go module.
#
#   scripts/ci/check-gui-go.sh [linux|windows|all]
#
# linux:   vet and link the shipped tags (gtk3,production,release) plus the E2E tags (gtk3,e2e). Needs the GTK3 and
#          WebKitGTK 4.1 development packages, because the Wails host links them through cgo: it cannot run on macOS.
# windows: vet and link windows/amd64 and windows/arm64 with the shipped tags (production,release), CGO disabled. This
#          proves the code compiles and links for Windows; it does not prove it runs.
#
# Needs apps/gui-go/frontend/dist (`bun --bun run --cwd apps/gui-go build`): main.go embeds it.
# Writes the linked binaries, SHA256SUMS and manifest.txt to $UC_GATE_OUT (default target/gui-go-gate). Nothing is
# published or signed, and the binaries are evidence of a successful link, not release candidates.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
MODE="${1:-all}"
case "$MODE" in linux | windows | all) ;; *) echo "usage: $0 [linux|windows|all]" >&2; exit 2 ;; esac
OUT="${UC_GATE_OUT:-$ROOT/target/gui-go-gate}"
[[ -d apps/gui-go/frontend/dist ]] || { echo "apps/gui-go/frontend/dist is missing: run 'bun --bun run --cwd apps/gui-go build' first" >&2; exit 1; }
rm -rf "$OUT" && mkdir -p "$OUT"

GIT=(git -c safe.directory='*')
HEAD_SHA="$("${GIT[@]}" rev-parse HEAD)"

# The generated build info must match the Rust workspace it is derived from (version and daemon API revision).
(cd packages/desktop-host-go && go generate ./buildinfo)
"${GIT[@]}" diff --exit-code -- packages/desktop-host-go/buildinfo ||
  { echo "packages/desktop-host-go/buildinfo is stale: run 'go generate ./buildinfo' there and commit it" >&2; exit 1; }
(cd packages/desktop-host-go && go vet ./...)

if [[ "$MODE" == linux || "$MODE" == all ]]; then
  (cd apps/gui-go && CGO_ENABLED=1 go vet -tags gtk3,production,release ./... &&
    CGO_ENABLED=1 go vet -tags gtk3,e2e . &&
    CGO_ENABLED=1 go build -tags gtk3,production,release -trimpath -buildvcs=false -o "$OUT/linux-$(go env GOARCH)/uniclipboard" .)
fi

if [[ "$MODE" == windows || "$MODE" == all ]]; then
  for arch in amd64 arm64; do
    (cd apps/gui-go && GOOS=windows GOARCH="$arch" CGO_ENABLED=0 go vet -tags production,release ./... &&
      GOOS=windows GOARCH="$arch" CGO_ENABLED=0 go build -tags production,release -trimpath -buildvcs=false \
        -ldflags '-H windowsgui' -o "$OUT/windows-$arch/gui-go.exe" .)
  done
fi

(cd "$OUT" && find . -type f ! -name SHA256SUMS ! -name manifest.txt -print0 | sort -z | xargs -0 sha256sum >SHA256SUMS)
{
  echo "mode=$MODE"
  echo "source_head=$HEAD_SHA"
  echo "source_dirty=$([ -n "$("${GIT[@]}" status --porcelain)" ] && echo true || echo false)"
  echo "host=$(uname -sm)"
  go version
} >"$OUT/manifest.txt"
cat "$OUT/SHA256SUMS" "$OUT/manifest.txt"
