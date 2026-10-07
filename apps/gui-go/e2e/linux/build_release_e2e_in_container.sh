#!/usr/bin/env bash
# Runs INSIDE the uc-gui-go-linux-build image (repository at /work, cache volume at /cache).
# Builds the GUI in the SHIPPED form plus the test control plane: tags `gtk3,production,release,e2e`. `release` gives the
# release-no-profile environment checks (no UC_PROFILE, no isolation knobs); `e2e` adds the control file and the update-feed
# override that the AppImage E2E needs. The frontend bundle must be built with VITE_GUI_GO_E2E=1 (the image has no bun).
set -euo pipefail
cd /work
export GOPATH=/cache/gopath GOFLAGS=-mod=mod
OUT=/cache/out-release; mkdir -p "$OUT"
(cd packages/desktop-host-go && go generate ./buildinfo)
read -r BUNDLE_ID PRODUCT < <(python3 -c 'import json;c=json.load(open("apps/gui-go/app.json"));print(c["identifier"],c["productName"])')
(cd apps/gui-go && CGO_ENABLED=1 go build -tags gtk3,production,release,e2e -trimpath -buildvcs=false \
  -ldflags "-w -s -X main.updaterPublicKey= -X main.productName=$PRODUCT -X main.bundleID=$BUNDLE_ID" -o "$OUT/gui-go-release-e2e" .)
git -C /work rev-parse HEAD > "$OUT/gui-go-release-e2e.head"
echo gtk3,production,release,e2e > "$OUT/gui-go-release-e2e.tags"
sha256sum "$OUT/gui-go-release-e2e" | tee "$OUT/gui-go-release-e2e.sha256"
