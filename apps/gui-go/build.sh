#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
if [[ "$(uname -s)" != Darwin ]]; then
  echo "Go GUI PoC currently supports local macOS builds only" >&2
  exit 2
fi
MODE="${1:-manual}"
case "$MODE" in
  manual) APP=UniClipboardGo; TAGS=production; E2E=0 ;;
  e2e) APP=UniClipboardGoE2E; TAGS=e2e; E2E=1 ;;
  *) echo "Usage: apps/gui-go/build.sh [manual|e2e]" >&2; exit 2 ;;
esac
mkdir -p target/gui-go
TRIPLE="$(rustc --print host-tuple)"
# stage-daemon owns the daemon and native helper build and staging for this invocation.
node scripts/stage-daemon.mjs --debug
(cd packages/desktop-host-go && go generate ./buildinfo)
(cd apps/cli-go && go build -o ../../target/gui-go/uniclip ./cmd/uniclip)
VITE_APP_VERSION="$(python3 -c 'import json;print(json.load(open("apps/gui-go/app.json"))["version"])')" VITE_GUI_GO_E2E="$E2E" bun --bun run --cwd apps/gui-go/frontend build
# The release signer key comes from apps/gui-go/app.json, the single source of the app identity.
# E2E builds leave it empty and use the local test feed override instead.
PUBKEY=""
if [[ "$E2E" == 0 ]]; then
  PUBKEY="$(python3 -c 'import json;print(json.load(open("apps/gui-go/app.json"))["updater"]["pubkey"])')"
fi
# Bundle identity, minimum macOS version and the login item name come from apps/gui-go/app.json.
read -r BUNDLE_ID PRODUCT VERSION MIN_MACOS < <(python3 -c 'import json;c=json.load(open("apps/gui-go/app.json"));print(c["identifier"],c["productName"],c["version"],c["minimumSystemVersion"])')
# The single-instance scope includes the bundle identifier, so the E2E build never shares an instance with the real app.
GO_BUNDLE_ID="$BUNDLE_ID"
if [[ "$MODE" == e2e ]]; then GO_BUNDLE_ID="$BUNDLE_ID.e2e"; fi
(cd apps/gui-go && go build -tags "$TAGS" -ldflags "-X main.updaterPublicKey=$PUBKEY -X main.productName=$PRODUCT -X main.bundleID=$GO_BUNDLE_ID" -o "../../target/gui-go/$APP-binary" .)
BUNDLE="$ROOT/target/gui-go/$APP.app"
mkdir -p "$BUNDLE/Contents/MacOS"
cp "target/gui-go/$APP-binary" "$BUNDLE/Contents/MacOS/gui-go"
cp "target/sidecar-staging/uniclip-quick-panel-$TRIPLE" "$BUNDLE/Contents/MacOS/uniclip-quick-panel"
cp "target/sidecar-staging/uniclipd-$TRIPLE" "$BUNDLE/Contents/MacOS/uniclipd"
cp apps/gui-go/Info.plist "$BUNDLE/Contents/Info.plist"
PLIST="$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" -c "Set :CFBundleName $PRODUCT" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $VERSION" -c "Set :CFBundleVersion $VERSION" -c "Add :LSMinimumSystemVersion string $MIN_MACOS" "$PLIST"
if [[ "$MODE" == e2e ]]; then
  # Only the test build gets its own identity, so it can never collide with a real install.
  # The name is what System Settings lists for a registered login item, so it must not read as the real app.
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID.e2e" -c "Set :CFBundleName $PRODUCT E2E Test Build" "$PLIST"
fi
codesign --force --deep --sign - "$BUNDLE"
echo "Built $BUNDLE"
