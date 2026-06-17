#!/bin/bash
# Repro / regression harness for the #1029 X11 lazy-clipboard race.
#
# Drives `uniclip probe watch` (daemon-identical event loop) against
# lazy_owner.py, which owns CLIPBOARD immediately but only serves text/plain
# DELAY_MS later WITHOUT re-asserting ownership (no second XFIXES). The fixed
# 3x150ms retry lost any delay beyond ~300ms; the owner-lifetime backoff poll
# recovers anything within CHANGE_POLL_DEADLINE (3s).
#
# Verdict signal: `uniclip probe watch` prints its own "event #N" lines for
# every captured snapshot. lazy_owner.py serves the literal URL below, so a
# log line containing it == the copy was captured. Its absence == lost.
# (probe does not emit uc_platform tracing logs, so grepping for
# "clipboard change lost" / "recovered after retry" finds nothing — use the
# URL marker.)
#
# Requirements (see README.md):
#   - an XWayland / X11 display at :0 (e.g. niri + xwayland-satellite, or any
#     real X server). GNOME-on-Wayland reproduces via its XWayland bridge too.
#   - python3-xlib
#   - a debug uniclip built with dev-tools:
#       cargo build -p uc-cli --features dev-tools
#
# Usage:
#   ./repro.sh                       # default matrix: 100 1000 2000 2500 3500
#   ./repro.sh 1000:5 3500:6         # explicit <DELAY_MS>:<POST_OWNER_WAIT_S>
set +e

REPO="${UC_REPO:-$HOME/projects/UniClipboard}"
PROBE="${UC_PROBE:-$REPO/target/debug/uniclip}"
OWNER="$(dirname "$0")/lazy_owner.py"
URL_MARKER="http://example.com/lazy-chrome-url-AABBCC"

# Force the X11 reader even on a Wayland session (this is the #1029 path).
export DISPLAY="${DISPLAY:-:0}"
unset WAYLAND_DISPLAY

run_case() {
  local D=$1
  local WAIT=$2
  pkill -f lazy_owner.py 2>/dev/null; pkill -f "probe watch" 2>/dev/null; sleep 0.5
  RUST_LOG=uc_platform=info "$PROBE" probe watch -v >"/tmp/watch_${D}.log" 2>&1 </dev/null &
  local WPID=$!
  sleep 2                       # let the watcher subscribe XFIXES first
  python3 "$OWNER" "${D}" >"/tmp/owner_${D}.log" 2>&1 </dev/null &
  local OPID=$!
  sleep "${WAIT}"               # owner triggers; flips text/plain at D ms
  kill "$OPID" "$WPID" 2>/dev/null
  pkill -f lazy_owner.py 2>/dev/null; pkill -f "probe watch" 2>/dev/null; sleep 0.5
  echo "########## DELAY = ${D} ms (post-owner wait ${WAIT}s) ##########"
  if grep -aq "$URL_MARKER" "/tmp/watch_${D}.log"; then
    echo "RESULT: CAPTURED"
  else
    echo "RESULT: LOST (no captured event carried the lazy URL)"
  fi
}

cases=("$@")
if [ ${#cases[@]} -eq 0 ]; then
  cases=(100:5 1000:5 2000:5 2500:5 3500:6)
fi
for arg in "${cases[@]}"; do
  D="${arg%%:*}"
  W="${arg##*:}"
  [ "$W" = "$arg" ] && W=5      # bare "1000" -> default 5s wait
  run_case "$D" "$W"
  echo
done
echo "===== DONE ====="
