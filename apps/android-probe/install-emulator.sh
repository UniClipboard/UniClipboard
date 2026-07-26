#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APK="$($PROJECT_DIR/build-emulator.sh | tail -n 1)"

adb install -r "$APK"
