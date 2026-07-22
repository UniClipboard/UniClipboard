#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TARGET_DIR="${UC_ENGINE_UNIFFI_TARGET_DIR:-${CARGO_TARGET_DIR:-$REPO_ROOT/target}}"
DIST_ROOT="${UC_ENGINE_UNIFFI_DIST_DIR:-$TARGET_DIR/uc-engine-uniffi-dist}"
DIST_DIR="$DIST_ROOT/android"
STAGE_DIR="$TARGET_DIR/uc-engine-uniffi-android-package"
BINDINGS_DIR="$STAGE_DIR/kotlin"
JNI_DIR="$STAGE_DIR/jni"
GRADLE_BUILD_DIR="$STAGE_DIR/gradle-build"
ANDROID_PROJECT="$REPO_ROOT/crates/uc-engine-uniffi/android"
GRADLEW="$REPO_ROOT/apps/android-probe/gradlew"
AAR_OUT="$DIST_DIR/UniClipboardEngine.aar"
CHECKSUM_FILE="$DIST_DIR/UniClipboardEngine.checksum.txt"
CARGO_LOCKED=()

if [[ -n "${UC_ENGINE_UNIFFI_BUILD_LOCKED:-}" ]]; then
  CARGO_LOCKED+=(--locked)
fi

case "$(uname -s)" in
  Darwin) HOST_LIBRARY="$TARGET_DIR/release/libuc_engine_uniffi.dylib" ;;
  Linux) HOST_LIBRARY="$TARGET_DIR/release/libuc_engine_uniffi.so" ;;
  *) echo "Android packaging requires a macOS or Linux host" >&2; exit 1 ;;
esac

export CARGO_TARGET_DIR="$TARGET_DIR"
cd "$REPO_ROOT"
rm -rf "$STAGE_DIR" "$DIST_DIR"
mkdir -p "$BINDINGS_DIR" "$JNI_DIR" "$DIST_DIR"

echo "==> Generate Kotlin bindings from the host library"
cargo build -p uc-engine-uniffi --release "${CARGO_LOCKED[@]}"
cargo run -p uc-engine-uniffi --release --features bindgen-cli \
  --bin uc-engine-uniffi-bindgen "${CARGO_LOCKED[@]}" -- \
  generate --library "$HOST_LIBRARY" --language kotlin \
  --out-dir "$BINDINGS_DIR" --no-format

echo "==> Build Android native libraries"
cargo ndk -t arm64-v8a -t x86_64 \
  build -p uc-engine-uniffi --release "${CARGO_LOCKED[@]}"
mkdir -p "$JNI_DIR/arm64-v8a" "$JNI_DIR/x86_64"
cp "$TARGET_DIR/aarch64-linux-android/release/libuc_engine_uniffi.so" \
  "$JNI_DIR/arm64-v8a/"
cp "$TARGET_DIR/x86_64-linux-android/release/libuc_engine_uniffi.so" \
  "$JNI_DIR/x86_64/"

echo "==> Compile Kotlin bindings and assembleRelease"
UC_ENGINE_UNIFFI_KOTLIN_DIR="$BINDINGS_DIR" \
UC_ENGINE_UNIFFI_JNI_DIR="$JNI_DIR" \
UC_ENGINE_UNIFFI_GRADLE_BUILD_DIR="$GRADLE_BUILD_DIR" \
  "$GRADLEW" --no-daemon -p "$ANDROID_PROJECT" assembleRelease

cp "$GRADLE_BUILD_DIR/outputs/aar/UniClipboardEngine-release.aar" "$AAR_OUT"
cp "$BINDINGS_DIR/uniffi/uc_engine_uniffi/uc_engine_uniffi.kt" "$DIST_DIR/"
shasum -a 256 "$AAR_OUT" | awk '{print $1}' > "$CHECKSUM_FILE"

VERSION="$(cargo pkgid -p uc-engine-uniffi)"
VERSION="${VERSION##*#}"
COMMIT="$(git rev-parse HEAD)"
printf 'core-v%s\n' "$VERSION" > "$DIST_DIR/core-version.txt"
printf '%s\n' "$COMMIT" > "$DIST_DIR/source-commit.txt"
printf '%s\n' \
  'net.java.dev.jna:jna:5.14.0@aar' \
  'org.jetbrains.kotlin:kotlin-stdlib:2.1.20' \
  > "$DIST_DIR/runtime-dependencies.txt"
cat > "$DIST_DIR/UniClipboardEngine.pom" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>app.uniclipboard</groupId>
  <artifactId>uniclipboard-engine</artifactId>
  <version>$VERSION</version>
  <packaging>aar</packaging>
  <dependencies>
    <dependency>
      <groupId>net.java.dev.jna</groupId>
      <artifactId>jna</artifactId>
      <version>5.14.0</version>
      <type>aar</type>
      <scope>runtime</scope>
    </dependency>
    <dependency>
      <groupId>org.jetbrains.kotlin</groupId>
      <artifactId>kotlin-stdlib</artifactId>
      <version>2.1.20</version>
      <scope>runtime</scope>
    </dependency>
  </dependencies>
</project>
EOF

echo "OK: $AAR_OUT"
echo "OK: $CHECKSUM_FILE"
