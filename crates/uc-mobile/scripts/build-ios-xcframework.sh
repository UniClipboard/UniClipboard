#!/usr/bin/env bash
# Build UniClipboardCore.xcframework + UniFFI Swift bindings for uc-mobile.
#
# Spike B1 pipeline (see .planning/research/uc-mobile-spike-plan.md §5):
#   1. host cdylib            -> uniffi-bindgen library mode -> Swift bindings
#   2. aarch64-apple-ios      -> static lib (device slice)
#   3. aarch64-apple-ios-sim  -> static lib (simulator slice)
#   4. xcodebuild -create-xcframework
#
# Run from anywhere; all paths resolve from the repo root. Requires Xcode and
# the two rustup targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim \
#     --toolchain 1.95.0-aarch64-apple-darwin
#
# Outputs (under target/, not checked in):
#   target/uniffi-bindings/uc_mobile.swift          Swift binding source
#   target/uniffi-bindings/include/                 C header + modulemap
#   target/UniClipboardCore.xcframework             device + simulator slices

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

BINDINGS_DIR="target/uniffi-bindings"
XCFRAMEWORK_OUT="target/UniClipboardCore.xcframework"

echo "==> [1/4] host cdylib + Swift bindings (uniffi-bindgen library mode)"
cargo build -p uc-mobile
rm -rf "$BINDINGS_DIR"
cargo run -p uc-mobile --features bindgen-cli --bin uniffi-bindgen -- \
  generate --library target/debug/libuc_mobile.dylib \
  --language swift --out-dir "$BINDINGS_DIR"

# xcodebuild expects a directory with module.modulemap; uniffi names it
# uc_mobileFFI.modulemap.
mkdir -p "$BINDINGS_DIR/include"
cp "$BINDINGS_DIR/uc_mobileFFI.h" "$BINDINGS_DIR/include/"
cp "$BINDINGS_DIR/uc_mobileFFI.modulemap" "$BINDINGS_DIR/include/module.modulemap"

echo "==> [2/4] device static lib (aarch64-apple-ios, release)"
cargo build -p uc-mobile --release --target aarch64-apple-ios

echo "==> [3/4] simulator static lib (aarch64-apple-ios-sim, release)"
cargo build -p uc-mobile --release --target aarch64-apple-ios-sim

echo "==> [4/4] xcframework"
rm -rf "$XCFRAMEWORK_OUT"
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libuc_mobile.a \
  -headers "$BINDINGS_DIR/include" \
  -library target/aarch64-apple-ios-sim/release/libuc_mobile.a \
  -headers "$BINDINGS_DIR/include" \
  -output "$XCFRAMEWORK_OUT"

# Size report (spike plan §5 wants a budget gate in CI later; for now, print).
echo "==> slice sizes"
du -sh target/aarch64-apple-ios/release/libuc_mobile.a \
       target/aarch64-apple-ios-sim/release/libuc_mobile.a \
       "$XCFRAMEWORK_OUT"
echo "OK: $XCFRAMEWORK_OUT"
