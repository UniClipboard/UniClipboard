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
# the iOS + simulator rustup targets (aarch64-apple-darwin ships with the host
# toolchain):
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
#
# Outputs (under target/, not checked in):
#   target/uniffi-bindings/uc_mobile.swift          Swift binding source
#   target/uniffi-bindings/include/                 C header + modulemap
#   target/UniClipboardCore.xcframework             device + simulator + macOS slices
# With UC_MOBILE_BUILD_ZIP=1 (CI release path) also:
#   target/UniClipboardCore.xcframework.zip         zipped framework (SwiftPM url)
#   target/UniClipboardCore.checksum.txt            sha256 of the zip (integrity)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

BINDINGS_DIR="target/uniffi-bindings"
XCFRAMEWORK_OUT="target/UniClipboardCore.xcframework"
XCFRAMEWORK_ZIP="target/UniClipboardCore.xcframework.zip"
CHECKSUM_OUT="target/UniClipboardCore.checksum.txt"

# CI knobs — both default OFF so a plain local run behaves exactly as before:
#   UC_MOBILE_BUILD_LOCKED=1  pass --locked to cargo (reproducible CI builds; the
#                             repo lockfile must already cover the mobile graph).
#   UC_MOBILE_BUILD_ZIP=1     also emit a checksummed .zip for the SwiftPM
#                             binaryTarget(url:checksum:) the iOS app consumes
#                             (see build-mobile-core.yml / docs runbook).
LOCKED="${UC_MOBILE_BUILD_LOCKED:+--locked}"

echo "==> [1/6] host cdylib + Swift bindings (uniffi-bindgen library mode)"
cargo build -p uc-mobile $LOCKED
rm -rf "$BINDINGS_DIR"
cargo run -p uc-mobile --features bindgen-cli --bin uniffi-bindgen $LOCKED -- \
  generate --library target/debug/libuc_mobile.dylib \
  --language swift --out-dir "$BINDINGS_DIR"

# xcodebuild expects a directory with module.modulemap; uniffi names it
# uc_mobileFFI.modulemap.
mkdir -p "$BINDINGS_DIR/include"
cp "$BINDINGS_DIR/uc_mobileFFI.h" "$BINDINGS_DIR/include/"
cp "$BINDINGS_DIR/uc_mobileFFI.modulemap" "$BINDINGS_DIR/include/module.modulemap"

echo "==> [2/6] device static lib (aarch64-apple-ios, release)"
cargo build -p uc-mobile --release --target aarch64-apple-ios $LOCKED

# Seam 1: the mobile tree must use the ring rustls provider exclusively.
# aws-lc-rs would drag a cmake/clang native build into iOS cross-compilation
# and double the crypto footprint (spike plan §5 hard assertion).
if cargo tree -p uc-mobile --target aarch64-apple-ios -i aws-lc-rs 2>/dev/null | grep -q aws-lc-rs; then
  echo "FAIL: aws-lc-rs leaked into the uc-mobile dependency tree" >&2
  exit 1
fi
echo "    aws-lc-rs absent from mobile tree: OK"

echo "==> [3/6] simulator static lib (aarch64-apple-ios-sim, release)"
cargo build -p uc-mobile --release --target aarch64-apple-ios-sim $LOCKED

echo "==> [4/6] simulator static lib (x86_64-apple-ios, release)"
# The simulator slice must be universal (arm64 + x86_64): an Apple Silicon Mac
# runs the arm64 simulator, but `-destination 'generic/platform=iOS Simulator'`
# and Release builds compile BOTH arches, and a single-arch slice fails to link
# the missing one. (x86_64 iOS == the simulator; there is no x86_64 device.)
cargo build -p uc-mobile --release --target x86_64-apple-ios $LOCKED

echo "==> [5/6] macOS static lib (aarch64-apple-darwin, release)"
# Not shipped in the app (uc-ios is iOS-only), but the SwiftPM A/B parity
# harness runs via `swift test` on the macOS host, which needs a host slice to
# link against. The Rust logic is identical across slices, so the host slice is
# a faithful oracle for the platform-independent parse/codec parity tests.
cargo build -p uc-mobile --release --target aarch64-apple-darwin $LOCKED

# lipo the two simulator arches into one universal static lib; an xcframework
# slice may itself be a fat archive covering multiple arches of a platform.
SIM_UNIVERSAL_DIR="target/uniffi-sim-universal"
mkdir -p "$SIM_UNIVERSAL_DIR"
lipo -create \
  target/aarch64-apple-ios-sim/release/libuc_mobile.a \
  target/x86_64-apple-ios/release/libuc_mobile.a \
  -output "$SIM_UNIVERSAL_DIR/libuc_mobile.a"

echo "==> [6/6] xcframework"
rm -rf "$XCFRAMEWORK_OUT"
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libuc_mobile.a \
  -headers "$BINDINGS_DIR/include" \
  -library "$SIM_UNIVERSAL_DIR/libuc_mobile.a" \
  -headers "$BINDINGS_DIR/include" \
  -library target/aarch64-apple-darwin/release/libuc_mobile.a \
  -headers "$BINDINGS_DIR/include" \
  -output "$XCFRAMEWORK_OUT"

# Size report (spike plan §5 wants a budget gate in CI later; for now, print).
# build-mobile-core.yml mirrors these into the job summary (D5 warn-only gate).
echo "==> slice sizes"
du -sh target/aarch64-apple-ios/release/libuc_mobile.a \
       "$SIM_UNIVERSAL_DIR/libuc_mobile.a" \
       target/aarch64-apple-darwin/release/libuc_mobile.a \
       "$XCFRAMEWORK_OUT"
echo "OK: $XCFRAMEWORK_OUT"

# Optional release packaging (CI sets UC_MOBILE_BUILD_ZIP=1). Kept in this script
# — not the workflow — so the local `swift package compute-checksum` a developer
# runs against `target/...zip` matches exactly what CI publishes (no zip-logic
# drift between the two paths).
if [[ -n "${UC_MOBILE_BUILD_ZIP:-}" ]]; then
  echo "==> [zip] packaging xcframework for SwiftPM binaryTarget(url:checksum:)"
  rm -f "$XCFRAMEWORK_ZIP"
  # ditto preserves the xcframework's directory layout + symlinks; `zip -r` can
  # corrupt framework bundles. --keepParent keeps the top-level .xcframework dir
  # so the unzipped artifact is directly usable.
  ( cd target && ditto -c -k --keepParent UniClipboardCore.xcframework UniClipboardCore.xcframework.zip )
  # SwiftPM's binaryTarget(checksum:) is the SHA-256 of the zip bytes, so this is
  # the canonical value — but the iOS update script re-derives it via
  # `swift package compute-checksum` (run inside its own package) to write into
  # Package.swift; this checksum.txt is the download-integrity anchor.
  shasum -a 256 "$XCFRAMEWORK_ZIP" | awk '{print $1}' > "$CHECKSUM_OUT"
  echo "    zip:      $XCFRAMEWORK_ZIP ($(du -sh "$XCFRAMEWORK_ZIP" | cut -f1))"
  echo "    checksum: $(cat "$CHECKSUM_OUT")  ($CHECKSUM_OUT)"
fi
