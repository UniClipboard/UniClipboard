#!/usr/bin/env bash
# Lists generated Tauri commands that the Go host does not implement yet.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
tmp="$(mktemp -d)"
grep -oE '__TAURI_INVOKE(<[^>]*>)?\("[a-z_]+"' "$ROOT/apps/gui/src/lib/ipc-bindings.generated.ts" | grep -oE '"[a-z_]+"' | tr -d '"' | sort -u > "$tmp/all"
grep -ohE '^\s+"[a-z_]+":' "$ROOT"/apps/gui-go/host_commands_*.go | tr -dc 'a-z_\n' | sort -u > "$tmp/impl"
echo "unimplemented: $(comm -23 "$tmp/all" "$tmp/impl" | wc -l) of $(wc -l < "$tmp/all" | tr -d ' ')"
comm -23 "$tmp/all" "$tmp/impl"
rm -rf "$tmp"
