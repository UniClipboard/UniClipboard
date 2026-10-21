#!/usr/bin/env bash
# Checks that icons/tray-cat@2x.png is byte-identical to the tray image shipped in v1.1.2.
set -euo pipefail
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
expected=21e459b6f6fd8fa4de3335370f8198a400ae7d555550388cf48d239d4e34a93b
embedded=$(shasum -a 256 "$repo/apps/gui-go/icons/tray-cat@2x.png" | cut -d' ' -f1)
shipped=$(git -C "$repo" show v1.1.2:apps/gui/src-tauri/icons/tray-icon@2x.png | shasum -a 256 | cut -d' ' -f1)
echo "expected $expected"
echo "embedded $embedded"
echo "v1.1.2   $shipped"
[ "$embedded" = "$expected" ] && [ "$shipped" = "$expected" ]
