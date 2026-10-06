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
(cd packages/desktop-host-go && go generate ./buildinfo)
(cd apps/cli-go && go build -o ../../target/gui-go/uniclip ./cmd/uniclip)
VITE_GUI_GO_E2E="$E2E" bun --bun run --cwd apps/gui-go build
(cd apps/gui-go && go build -tags "$TAGS" -o "../../target/gui-go/$APP-binary" .)
BUNDLE="$ROOT/target/gui-go/$APP.app"
mkdir -p "$BUNDLE/Contents/MacOS"
cp "target/gui-go/$APP-binary" "$BUNDLE/Contents/MacOS/gui-go"
cp apps/gui-go/Info.plist "$BUNDLE/Contents/Info.plist"
if [[ "$MODE" == e2e ]]; then
  /usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier app.uniclipboard.gui-go.poc.e2e' "$BUNDLE/Contents/Info.plist"
fi
codesign --force --deep --sign - "$BUNDLE"
echo "Built $BUNDLE"
