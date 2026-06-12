#!/usr/bin/env bash
# Compile and run the B1 iOS demo on an iOS simulator.
#
# Prereq: crates/uc-mobile/scripts/build-ios-xcframework.sh has been run
# (needs target/uniffi-bindings/ and the ios-sim static lib).
#
# The demo is a plain arm64-simulator command-line binary spawned with
# `simctl spawn` — enough to prove "Swift on iOS calls Rust through UniFFI"
# without scaffolding a full Xcode app project.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

BINDINGS_DIR="target/uniffi-bindings"
DEMO_BIN="target/ios-demo/uc-mobile-demo"
SIM_LIB_DIR="target/aarch64-apple-ios-sim/release"

[ -f "$BINDINGS_DIR/uc_mobile.swift" ] || {
  echo "missing $BINDINGS_DIR/uc_mobile.swift — run build-ios-xcframework.sh first" >&2
  exit 1
}

echo "==> compile demo for arm64 iOS simulator"
mkdir -p "$(dirname "$DEMO_BIN")"
xcrun -sdk iphonesimulator swiftc \
  -target arm64-apple-ios15.0-simulator \
  crates/uc-mobile/ios-demo/main.swift \
  "$BINDINGS_DIR/uc_mobile.swift" \
  -I "$BINDINGS_DIR/include" \
  -L "$SIM_LIB_DIR" -luc_mobile \
  -o "$DEMO_BIN"

echo "==> find or boot a simulator"
UDID="$(xcrun simctl list devices booted | sed -n 's/.*(\([0-9A-F-]\{36\}\)) (Booted).*/\1/p' | head -1)"
if [ -z "$UDID" ]; then
  # Pick the first available iPhone and boot it headless (no Simulator.app UI
  # needed for simctl spawn).
  UDID="$(xcrun simctl list devices available | grep -m1 'iPhone' | sed -n 's/.*(\([0-9A-F-]\{36\}\)).*/\1/p')"
  [ -n "$UDID" ] || { echo "no available iPhone simulator found" >&2; exit 1; }
  echo "    booting $UDID"
  xcrun simctl boot "$UDID"
  BOOTED_BY_SCRIPT=1
else
  BOOTED_BY_SCRIPT=0
fi

echo "==> run on simulator $UDID"
# Extra arguments (e.g. a connect URI for the B2 daemon probes) are passed
# through to the demo binary.
set +e
xcrun simctl spawn "$UDID" "$REPO_ROOT/$DEMO_BIN" "$@"
STATUS=$?
set -e

if [ "$BOOTED_BY_SCRIPT" = "1" ]; then
  xcrun simctl shutdown "$UDID" || true
fi
exit "$STATUS"
