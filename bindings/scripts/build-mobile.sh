#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
BINDINGS_DIR="$ROOT_DIR/bindings"
BIN_DIR="$BINDINGS_DIR/.bin"

IOS_OUT="$ROOT_DIR/app/expo/modules/unic-engine/ios/Frameworks/UnicCore.xcframework"
ANDROID_OUT="$ROOT_DIR/app/expo/modules/unic-engine/android/libs/unic-engine.aar"

mkdir -p "$BIN_DIR" "$(dirname "$IOS_OUT")" "$(dirname "$ANDROID_OUT")"

GO_TOOLCHAIN_BIN="$(cd "$BINDINGS_DIR" && go env GOROOT)/bin"
export PATH="$BIN_DIR:$GO_TOOLCHAIN_BIN:$PATH"
export GOTOOLCHAIN=local

echo "==> Installing gomobile & gobind into $BIN_DIR"
(
  cd "$BINDINGS_DIR"
  GOBIN="$BIN_DIR" go install golang.org/x/mobile/cmd/gomobile golang.org/x/mobile/cmd/gobind
)

GOMOBILE_PKG_DIR="$(go env GOPATH)/pkg/gomobile"
if [[ ! -d "$GOMOBILE_PKG_DIR" ]]; then
  mkdir -p "$GOMOBILE_PKG_DIR"
fi

export GOWORK=off

echo "==> Building iOS XCFramework ($IOS_OUT)"
rm -rf "$IOS_OUT"
(
  cd "$BINDINGS_DIR"
  "$BIN_DIR/gomobile" bind \
    -target=ios,iossimulator \
    -iosversion=16.0 \
    -o "$IOS_OUT" \
    github.com/uniclipboard/unic/bindings/mobile
)

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -n 1 || true)"
fi
if [[ -z "$ANDROID_NDK_HOME" || ! -d "$ANDROID_NDK_HOME" ]]; then
  echo "error: Android NDK not found under $ANDROID_HOME/ndk" >&2
  exit 1
fi
export ANDROID_NDK_HOME

echo "==> Building Android AAR ($ANDROID_OUT) using NDK $ANDROID_NDK_HOME"
rm -f "$ANDROID_OUT"
(
  cd "$BINDINGS_DIR"
  "$BIN_DIR/gomobile" bind \
    -target=android/arm64,android/amd64 \
    -androidapi=24 \
    -o "$ANDROID_OUT" \
    github.com/uniclipboard/unic/bindings/mobile
)

echo "==> Artifacts built:"
du -sh "$IOS_OUT" "$ANDROID_OUT"
shasum -a 256 "$ANDROID_OUT"
find "$IOS_OUT" -name "UnicCore" -type f -exec shasum -a 256 {} +
