#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TARGET_DIR="${UC_ENGINE_UNIFFI_TARGET_DIR:-${CARGO_TARGET_DIR:-$REPO_ROOT/target}}"
DIST_ROOT="${UC_ENGINE_UNIFFI_DIST_DIR:-$TARGET_DIR/uc-engine-uniffi-dist}"
DIST_DIR="$DIST_ROOT/ios"
STAGE_DIR="$TARGET_DIR/uc-engine-uniffi-ios-package"
BINDINGS_DIR="$STAGE_DIR/bindings"
INCLUDE_DIR="$BINDINGS_DIR/include"
DEVICE_DIR="$STAGE_DIR/device"
SIMULATOR_ARM64_DIR="$STAGE_DIR/simulator-arm64"
SIMULATOR_X86_64_DIR="$STAGE_DIR/simulator-x86_64"
SIMULATOR_DIR="$STAGE_DIR/simulator"
XCFRAMEWORK="$DIST_DIR/UniClipboardEngine.xcframework"
XCFRAMEWORK_ZIP="$DIST_DIR/UniClipboardEngine.xcframework.zip"
CHECKSUM_FILE="$DIST_DIR/UniClipboardEngine.checksum.txt"
CARGO_LOCKED=()

if [[ -n "${UC_ENGINE_UNIFFI_BUILD_LOCKED:-}" ]]; then
  CARGO_LOCKED+=(--locked)
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS packaging requires macOS and Xcode" >&2
  exit 1
fi

export CARGO_TARGET_DIR="$TARGET_DIR"
cd "$REPO_ROOT"
rm -rf "$STAGE_DIR" "$DIST_DIR"
mkdir -p \
  "$INCLUDE_DIR" \
  "$DEVICE_DIR" \
  "$SIMULATOR_ARM64_DIR" \
  "$SIMULATOR_X86_64_DIR" \
  "$SIMULATOR_DIR" \
  "$DIST_DIR"

echo "==> Generate Swift bindings from the host library"
cargo build -p uc-engine-uniffi --release "${CARGO_LOCKED[@]}"
cargo run -p uc-engine-uniffi --release --features bindgen-cli \
  --bin uc-engine-uniffi-bindgen "${CARGO_LOCKED[@]}" -- \
  generate --library "$TARGET_DIR/release/libuc_engine_uniffi.dylib" \
  --language swift --out-dir "$BINDINGS_DIR"
cp "$BINDINGS_DIR/uc_engine_uniffiFFI.h" "$INCLUDE_DIR/"
cp "$BINDINGS_DIR/uc_engine_uniffiFFI.modulemap" "$INCLUDE_DIR/module.modulemap"

echo "==> Build iOS device and simulator libraries"
cargo build -p uc-engine-uniffi --release --target aarch64-apple-ios "${CARGO_LOCKED[@]}"
cargo build -p uc-engine-uniffi --release --target aarch64-apple-ios-sim "${CARGO_LOCKED[@]}"
cargo build -p uc-engine-uniffi --release --target x86_64-apple-ios "${CARGO_LOCKED[@]}"
cp "$TARGET_DIR/aarch64-apple-ios/release/libuc_engine_uniffi.a" "$DEVICE_DIR/"
cp "$TARGET_DIR/aarch64-apple-ios-sim/release/libuc_engine_uniffi.a" \
  "$SIMULATOR_ARM64_DIR/"
cp "$TARGET_DIR/x86_64-apple-ios/release/libuc_engine_uniffi.a" \
  "$SIMULATOR_X86_64_DIR/"
xcrun strip -S "$DEVICE_DIR/libuc_engine_uniffi.a"
xcrun strip -S "$SIMULATOR_ARM64_DIR/libuc_engine_uniffi.a"
xcrun strip -S "$SIMULATOR_X86_64_DIR/libuc_engine_uniffi.a"
lipo -create \
  "$SIMULATOR_ARM64_DIR/libuc_engine_uniffi.a" \
  "$SIMULATOR_X86_64_DIR/libuc_engine_uniffi.a" \
  -output "$SIMULATOR_DIR/libuc_engine_uniffi.a"

echo "==> Create XCFramework"
xcodebuild -create-xcframework \
  -library "$DEVICE_DIR/libuc_engine_uniffi.a" \
  -headers "$INCLUDE_DIR" \
  -library "$SIMULATOR_DIR/libuc_engine_uniffi.a" \
  -headers "$INCLUDE_DIR" \
  -output "$XCFRAMEWORK"

cp "$BINDINGS_DIR/uc_engine_uniffi.swift" "$DIST_DIR/"
ditto -c -k --keepParent "$XCFRAMEWORK" "$XCFRAMEWORK_ZIP"
shasum -a 256 "$XCFRAMEWORK_ZIP" | awk '{print $1}' > "$CHECKSUM_FILE"

VERSION="$(cargo pkgid -p uc-engine-uniffi)"
VERSION="${VERSION##*#}"
COMMIT="$(git rev-parse HEAD)"
printf 'core-v%s\n' "$VERSION" > "$DIST_DIR/core-version.txt"
printf '%s\n' "$COMMIT" > "$DIST_DIR/source-commit.txt"

echo "OK: $XCFRAMEWORK"
echo "OK: $XCFRAMEWORK_ZIP"
echo "OK: $CHECKSUM_FILE"
