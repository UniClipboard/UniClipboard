#!/usr/bin/env bash
# Runs INSIDE the uc-gui-go-linux-build image with the repository mounted at /work and a cache volume at /cache.
# Builds the Linux E2E binaries: the Rust daemon (debug, like build.sh e2e), the Go CLI, and the Go GUI with the
# `gtk3,e2e` tags. The frontend bundle must already exist (apps/gui-go/frontend/dist, built on the host with
# VITE_GUI_GO_E2E=1): the image has no bun. Outputs go to /cache/out (the host reads them from the volume).
set -euo pipefail
cd /work
git config --global --add safe.directory /work
export CARGO_HOME=/cache/cargo RUSTUP_HOME=/cache/rustup CARGO_TARGET_DIR=/cache/target GOPATH=/cache/gopath GOFLAGS=-mod=mod
export PATH="$CARGO_HOME/bin:$PATH"
OUT=/cache/out; mkdir -p "$OUT"
if ! command -v cargo >/dev/null; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
fi
if [[ "${SKIP_DAEMON:-0}" != 1 ]]; then
  cargo build --locked -p uc-daemon
  cp /cache/target/debug/uniclipd "$OUT/uniclipd"
fi
(cd packages/desktop-host-go && go generate ./buildinfo)
(cd apps/cli-go && CGO_ENABLED=0 go build -o "$OUT/uniclip" ./cmd/uniclip)
mkdir -p apps/gui-go/assets && cp apps/gui/src-tauri/icons/tray-icon@2x.png apps/gui-go/assets/
read -r BUNDLE_ID PRODUCT < <(python3 -c 'import json;c=json.load(open("apps/gui/src-tauri/tauri.conf.json"));print(c["identifier"],c["productName"])')
(cd apps/gui-go && CGO_ENABLED=1 go build -tags gtk3,e2e -ldflags "-X main.updaterPublicKey= -X main.productName=$PRODUCT -X main.bundleID=$BUNDLE_ID.e2e" -o "$OUT/gui-go" .)
(cd apps/gui-go && CGO_ENABLED=1 go vet -tags gtk3,production,release . && echo "vet gtk3,production,release ok")
ldd "$OUT/gui-go" | awk '{print $1}' | sort > "$OUT/gui-go.ldd.txt"
file "$OUT/gui-go" "$OUT/uniclip" ${SKIP_DAEMON:+} > "$OUT/file.txt" || true
git -C /work rev-parse HEAD > "$OUT/head.txt"
echo built "$OUT"
