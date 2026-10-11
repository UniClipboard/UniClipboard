#!/bin/zsh
# Captures the macOS window-material preview over a synthetic striped backdrop, so the screenshot shows
# what the window material samples without exposing any other application's windows.
#
#   apps/gui-go/cmd/macos-glass-preview/capture/capture-over-backdrop.sh <out-dir> <name> [preview flags...]
#
# Runs the preview built with -tags private_mac_apis unless PUBLIC=1. The Vite dev server named by the
# preview flags (default http://127.0.0.1:1531) must already be running. The region capture is refused
# unless the preview and the backdrop are the two front-most windows in that region. Needs the Screen
# Recording permission for the terminal; it changes no system setting.
set -euo pipefail

OUT=${1:?output directory}
NAME=${2:?capture name}
shift 2
HERE=${0:A:h}
PREVIEW_DIR=${HERE:h}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$OUT"

for tool in Backdrop FrontCheck WindowList; do
  swiftc -O "$HERE/$tool.swift" -o "$TMP/$tool"
done
TAGS=(-tags private_mac_apis)
[[ ${PUBLIC:-0} == 1 ]] && TAGS=()
(cd "$PREVIEW_DIR/../.." && go build "${TAGS[@]}" -o "$TMP/glass-preview" ./cmd/macos-glass-preview)

"$TMP/glass-preview" "$@" > "$OUT/$NAME.log" 2>&1 &
PREVIEW=$!
trap 'kill $PREVIEW 2>/dev/null; rm -rf "$TMP"' EXIT

# Window bounds come from CGWindowList (no capture permission needed for these fields).
bounds() { "$TMP/WindowList" glass-preview | awk '/X =/{gsub(/;/,"");x=$3} /Y =/{gsub(/;/,"");y=$3} /Width/{gsub(/;/,"");w=$3} /Height/{gsub(/;/,"");h=$3} /}/{print x-20, y-20, w+40, h+40; exit}'; }
for _ in {1..10}; do
  sleep 1
  "$TMP/WindowList" glass-preview | grep -q glass-preview && break
done
sleep 2
ID=$("$TMP/WindowList" glass-preview | awk 'NR==1{print $1}')
[[ -n $ID ]] || { echo "preview window not found" >&2; exit 1; }
screencapture -x -o -l "$ID" "$OUT/$NAME-window.png"

read -r X Y W H < <(bounds)
"$TMP/Backdrop" "$ID" "$X" "$Y" "$W" "$H" 10 ${ACTIVATE:-} > /dev/null &
sleep 6
FRONT=($("$TMP/FrontCheck" "$X" "$Y" "$W" "$H" | awk '{print $2}' | head -2))
if [[ ${FRONT[1]} == glass-preview && ${FRONT[2]} == Backdrop ]]; then
  screencapture -x -R "$X,$Y,$W,$H" "$OUT/$NAME-over-backdrop.png"
  echo "captured $OUT/$NAME-over-backdrop.png"
else
  echo "refused: the front windows in the region are ${FRONT[*]}" >&2
fi
wait %2 2>/dev/null || true
grep "material=" "$OUT/$NAME.log" || true
