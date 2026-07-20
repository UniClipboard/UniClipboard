#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT_DIR="$REPO_ROOT/apps/ios-probe"
GENERATED_PROJECT="$REPO_ROOT/target/ios-probe-project/EngineProbe.xcodeproj"
DERIVED_DATA="$REPO_ROOT/target/ios-probe-derived"
DEVICE_ID="${1:-00008140-0011499C2650801C}"

cd "$REPO_ROOT"
IPHONEOS_DEPLOYMENT_TARGET=17.0 \
  CARGO_TARGET_DIR="$REPO_ROOT/target/ios-probe-cargo" \
  cargo build -p uc-ios-probe-core --release --target aarch64-apple-ios --locked

mkdir -p "$(dirname "$GENERATED_PROJECT")"
GEM_HOME="/opt/homebrew/Cellar/cocoapods/1.16.2_2/libexec" \
  ruby "$PROJECT_DIR/project.rb" "$GENERATED_PROJECT"

xcodebuild \
  -project "$GENERATED_PROJECT" \
  -scheme EngineProbe \
  -configuration Debug \
  -destination "id=$DEVICE_ID" \
  -derivedDataPath "$DERIVED_DATA" \
  -allowProvisioningUpdates \
  build

echo "$DERIVED_DATA/Build/Products/Debug-iphoneos/EngineProbe.app"
