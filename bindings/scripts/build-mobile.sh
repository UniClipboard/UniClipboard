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

  echo "==> Converting UnicCore.xcframework slices to app-extension-safe dynamic frameworks"
  DYN_TMP="$(mktemp -d)"
  trap 'rm -rf "$DYN_TMP"' EXIT
  cat > "$DYN_TMP/keep.m" <<'EOF'
#import <Foundation/Foundation.h>
void UnicCoreDynamicKeep(void) {}
EOF

  write_fw_plist() {
    local plist_path="$1"
    local platform_name="$2"
    cat > "$plist_path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>UnicCore</string>
  <key>CFBundleIdentifier</key><string>app.uniclipboard.UnicCore</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>UnicCore</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleSupportedPlatforms</key><array><string>$platform_name</string></array>
  <key>MinimumOSVersion</key><string>16.0</string>
</dict>
</plist>
EOF
  }

  # 1. Physical device slice (ios-arm64)
  IOS_SDK="$(xcrun --sdk iphoneos --show-sdk-path)"
  DEV_FW="$IOS_OUT/ios-arm64/UnicCore.framework"
  clang -target arm64-apple-ios16.0 -isysroot "$IOS_SDK" -fapplication-extension -dynamiclib \
    -install_name "@rpath/UnicCore.framework/UnicCore" \
    -Wl,-force_load,"$DEV_FW/UnicCore" \
    -framework Foundation -framework Security -framework Network -framework CoreFoundation -lresolv \
    "$DYN_TMP/keep.m" -o "$DYN_TMP/UnicCore-ios-arm64"
  mv "$DYN_TMP/UnicCore-ios-arm64" "$DEV_FW/UnicCore"
  write_fw_plist "$DEV_FW/Info.plist" "iPhoneOS"

  # 2. Simulator slice (ios-arm64_x86_64-simulator)
  SIM_SDK="$(xcrun --sdk iphonesimulator --show-sdk-path)"
  SIM_FW="$IOS_OUT/ios-arm64_x86_64-simulator/UnicCore.framework"
  SIM_ARCHS="$(lipo -archs "$SIM_FW/UnicCore")"
  SIM_DYLIBS=()
  for arch in $SIM_ARCHS; do
    lipo -thin "$arch" "$SIM_FW/UnicCore" -output "$DYN_TMP/static-sim-$arch.a"
    clang -target "${arch}-apple-ios16.0-simulator" -isysroot "$SIM_SDK" -fapplication-extension -dynamiclib \
      -install_name "@rpath/UnicCore.framework/UnicCore" \
      -Wl,-force_load,"$DYN_TMP/static-sim-$arch.a" \
      -framework Foundation -framework Security -framework Network -framework CoreFoundation -lresolv \
      "$DYN_TMP/keep.m" -o "$DYN_TMP/UnicCore-sim-$arch"
    SIM_DYLIBS+=("$DYN_TMP/UnicCore-sim-$arch")
  done
  lipo -create "${SIM_DYLIBS[@]}" -output "$DYN_TMP/UnicCore-sim-universal"
  mv "$DYN_TMP/UnicCore-sim-universal" "$SIM_FW/UnicCore"
  write_fw_plist "$SIM_FW/Info.plist" "iPhoneSimulator"

  find "$IOS_OUT" -name "UnicCore" -type f -exec file {} + -exec shasum -a 256 {} +
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

  ANDROID_ABI_TARGETS="${UNIC_ANDROID_ABI_TARGETS:-android/arm64,android/amd64}"
  case "$ANDROID_ABI_TARGETS" in
    android/arm64|android/amd64|android/arm64,android/amd64) ;;
    *) echo "error: invalid UNIC_ANDROID_ABI_TARGETS: $ANDROID_ABI_TARGETS" >&2; exit 1 ;;
  esac

  echo "==> Building Android AAR ($ANDROID_OUT) using NDK $ANDROID_NDK_HOME ($ANDROID_ABI_TARGETS)"
  rm -f "$ANDROID_OUT"
  (
    cd "$BINDINGS_DIR"
    "$BIN_DIR/gomobile" bind \
      -target="$ANDROID_ABI_TARGETS" \
      -androidapi=24 -tags=sqlite_fts5 \
      -trimpath -ldflags="$LDFLAGS" \
      -o "$ANDROID_OUT" \
      github.com/uniclipboard/unic/bindings/mobile
  )
  shasum -a 256 "$ANDROID_OUT"
fi
