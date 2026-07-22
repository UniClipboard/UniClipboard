#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT_DIR="$REPO_ROOT/apps/android-probe"
TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/uni-desktop-android-probe-target}"

cd "$REPO_ROOT"
CARGO_TARGET_DIR="$TARGET_DIR" cargo ndk \
  -t arm64-v8a \
  -o "$PROJECT_DIR/app/src/main/jniLibs" \
  build -p uc-mobile-probe-core --release --locked

cd "$PROJECT_DIR"
./gradlew --offline :app:assembleDebug

echo "$PROJECT_DIR/app/build/outputs/apk/debug/app-debug.apk"
