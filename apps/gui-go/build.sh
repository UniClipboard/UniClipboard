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
cargo build --locked -p uc-daemon
# The native quick panel helper ships next to the GUI executable, like the Tauri bundle's externalBin.
cargo build --locked -p quick-panel --bin uniclip-quick-panel
(cd packages/desktop-host-go && go generate ./buildinfo)
(cd apps/cli-go && go build -o ../../target/gui-go/uniclip ./cmd/uniclip)
mkdir -p apps/gui-go/assets && cp apps/gui/src-tauri/icons/tray-icon@2x.png apps/gui-go/assets/
VITE_GUI_GO_E2E="$E2E" bun --bun run --cwd apps/gui-go build
# The release signer key comes from the Tauri updater config so both shells trust one key.
# E2E builds leave it empty and use the local test feed override instead.
PUBKEY=""
if [[ "$E2E" == 0 ]]; then
  PUBKEY="$(python3 -c 'import json;print(json.load(open("apps/gui/src-tauri/tauri.conf.json"))["plugins"]["updater"]["pubkey"])')"
fi
# Bundle identity and the login item name come from the Tauri configuration so both shells ship as the same app.
read -r BUNDLE_ID PRODUCT VERSION < <(python3 -c 'import json;c=json.load(open("apps/gui/src-tauri/tauri.conf.json"));print(c["identifier"],c["productName"],c["version"])')
(cd apps/gui-go && go build -tags "$TAGS" -ldflags "-X main.updaterPublicKey=$PUBKEY -X main.productName=$PRODUCT" -o "../../target/gui-go/$APP-binary" .)
BUNDLE="$ROOT/target/gui-go/$APP.app"
mkdir -p "$BUNDLE/Contents/MacOS"
cp "target/gui-go/$APP-binary" "$BUNDLE/Contents/MacOS/gui-go"
cp target/debug/uniclip-quick-panel "$BUNDLE/Contents/MacOS/uniclip-quick-panel"
cp apps/gui-go/Info.plist "$BUNDLE/Contents/Info.plist"
PLIST="$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" -c "Set :CFBundleName $PRODUCT" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $VERSION" -c "Set :CFBundleVersion $VERSION" "$PLIST"
if [[ "$MODE" == e2e ]]; then
  # Only the test build gets its own identity, so it can never collide with a real install.
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID.e2e" "$PLIST"
fi
codesign --force --deep --sign - "$BUNDLE"
echo "Built $BUNDLE"
