#!/usr/bin/env bash
# Build the Go `uniclip` (apps/cli-go) for one release target triple.
#
# Usage: build-go-cli.sh <target-triple> <output-path>
#
# The triple is the Rust one used by build-cli.yml, so the Go binary can be
# packaged by scripts/ci/package-cli.sh next to the matching `uniclipd`.
# Fails when internal/buildinfo is stale against Cargo.toml or the daemon
# contract revision.
set -euo pipefail

TARGET=$1
OUTPUT=$2

case "$TARGET" in
  aarch64-apple-darwin) GOOS=darwin GOARCH=arm64 ;;
  x86_64-apple-darwin) GOOS=darwin GOARCH=amd64 ;;
  aarch64-unknown-linux-musl | aarch64-unknown-linux-gnu) GOOS=linux GOARCH=arm64 ;;
  x86_64-unknown-linux-musl | x86_64-unknown-linux-gnu) GOOS=linux GOARCH=amd64 ;;
  aarch64-pc-windows-msvc) GOOS=windows GOARCH=arm64 ;;
  x86_64-pc-windows-msvc) GOOS=windows GOARCH=amd64 ;;
  *)
    echo "ERROR: unsupported target triple: $TARGET" >&2
    exit 2
    ;;
esac

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUTPUT_ABS=$(cd "$(dirname "$OUTPUT")" && pwd)/$(basename "$OUTPUT")
cd "$REPO_ROOT/apps/cli-go"

go generate ./internal/buildinfo
if ! git diff --quiet -- internal/buildinfo; then
  echo "ERROR: apps/cli-go/internal/buildinfo is stale; run 'go generate ./internal/buildinfo' and commit" >&2
  git --no-pager diff -- internal/buildinfo >&2
  exit 1
fi

# Static, reproducible build: no cgo, no local paths, no symbol tables.
CGO_ENABLED=0 GOOS=$GOOS GOARCH=$GOARCH go build -trimpath -buildvcs=false -ldflags "-s -w" -o "$OUTPUT_ABS" ./cmd/uniclip
echo "built $OUTPUT_ABS for $GOOS/$GOARCH"
