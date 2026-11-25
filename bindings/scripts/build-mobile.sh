#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
BINDINGS_DIR="$ROOT_DIR/bindings"
BIN_DIR="$BINDINGS_DIR/.bin"

PLATFORM="${UNIC_MOBILE_RELEASE_PLATFORM:-all}"
VARIANT="${UNIC_MOBILE_VARIANT:-test}"

case "$PLATFORM" in all|ios|android) ;; *) echo 'error: invalid mobile platform' >&2; exit 1 ;; esac
case "$VARIANT" in test|production) ;; *) echo 'error: invalid mobile variant' >&2; exit 1 ;; esac

LDFLAGS='-s -w'
if [[ $VARIANT == production ]]; then
  if [[ $PLATFORM == all ]]; then
    echo 'error: production requires exactly one mobile platform' >&2
    exit 1
  fi
  version=$(python3 -B "$ROOT_DIR/tools/release/mobile.py" config | python3 -c 'import json, sys; print(json.load(sys.stdin)[sys.argv[1]]["version"])' "$PLATFORM")
  obs=github.com/uniclipboard/unic/internal/observability
  LDFLAGS+=" -X $obs.ProductVersion=$version"
  host=${UNIC_POSTHOG_HOST:-}
  token=${UNIC_POSTHOG_PROJECT_TOKEN:-}
  if [[ -n $host || -n $token ]]; then
    if [[ ( $host != https://us.i.posthog.com && $host != https://eu.i.posthog.com ) || ! $token =~ ^phc_[A-Za-z0-9_-]+$ ]]; then
      echo 'error: expected a PostHog Cloud host and ingestion-only project token' >&2
      exit 1
    fi
    LDFLAGS+=" -X $obs.PostHogHost=$host -X $obs.PostHogToken=$token"
  fi
fi

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

if [[ $PLATFORM != android ]]; then
  echo "==> Building iOS XCFramework ($IOS_OUT)"
  rm -rf "$IOS_OUT"
  (
    cd "$BINDINGS_DIR"
    "$BIN_DIR/gomobile" bind \
      -target=ios,iossimulator \
      -iosversion=16.0 \
      -trimpath -ldflags="$LDFLAGS" \
      -o "$IOS_OUT" \
      github.com/uniclipboard/unic/bindings/mobile
  )
  find "$IOS_OUT" -name "UnicCore" -type f -exec shasum -a 256 {} +
fi

if [[ $PLATFORM != ios ]]; then
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
      -androidapi=24 -tags=sqlite_fts5 \
      -trimpath -ldflags="$LDFLAGS" \
      -o "$ANDROID_OUT" \
      github.com/uniclipboard/unic/bindings/mobile
  )
  shasum -a 256 "$ANDROID_OUT"
fi
