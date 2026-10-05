#!/usr/bin/env bash
#
# Build the user-facing CLI (Go, apps/cli-go) where the E2E suites and the
# scripts in this directory resolve it: $CARGO_TARGET_DIR/debug/uniclip
# (default target/debug/uniclip, uniclip.exe on Windows).
#
# The daemon the CLI talks to is the Rust uniclipd, built separately:
#   cargo build -p uc-daemon [--features uc-daemon/e2e-rendezvous]
# The development-only commands (`dev seed-clipboard`, `mobile debug`, ...) live in
# the Rust development CLI, not in this binary:
#   cargo build -p uc-dev-cli --features uc-dev-cli/dev-tools
#
# Usage: scripts/e2e/build-cli.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in
    /* | [A-Za-z]:*) ;;
    *) TARGET_DIR="$ROOT/$TARGET_DIR" ;;
esac

EXE=""
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) EXE=".exe" ;;
esac

if ! command -v go >/dev/null 2>&1; then
    echo "ERROR: go not found on PATH (apps/cli-go/go.mod pins the version and toolchain)" >&2
    exit 2
fi

mkdir -p "$TARGET_DIR/debug"
cd "$ROOT/apps/cli-go"
# Keep internal/buildinfo in step with Cargo.toml and the daemon contract.
go generate ./internal/buildinfo
go build -buildvcs=false -o "$TARGET_DIR/debug/uniclip$EXE" ./cmd/uniclip
echo "built $TARGET_DIR/debug/uniclip$EXE"
