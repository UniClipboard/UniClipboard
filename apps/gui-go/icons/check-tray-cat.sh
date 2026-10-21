#!/usr/bin/env bash
# Checks that icons/tray-cat@2x.png is byte-identical to the tray image shipped in v1.1.2.
set -euo pipefail
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
expected=21e459b6f6fd8fa4de3335370f8198a400ae7d555550388cf48d239d4e34a93b
embedded=$(shasum -a 256 "$repo/apps/gui-go/icons/tray-cat@2x.png" | cut -d' ' -f1)
# The v1.1.2 tray image, by its git blob id: it lives in the retired host's directory, which tracked files do not name.
blob=b745edc61efe18b8895d427830919280d62693b9
git -C "$repo" ls-tree -r v1.1.2 | awk -v b="$blob" '$3 == b { found = 1 } END { exit !found }'
shipped=$(git -C "$repo" cat-file blob "$blob" | shasum -a 256 | cut -d' ' -f1)
echo "expected $expected"
echo "embedded $embedded"
echo "v1.1.2   $shipped"
[ "$embedded" = "$expected" ] && [ "$shipped" = "$expected" ]
