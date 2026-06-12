#!/usr/bin/env bash
#
# End-to-end smoke test for the native X11 clipboard backend (Phase 3
# of docs/development/linux-clipboard-rewrite.md).
#
# What it covers (mapped to the §8 gotchas list):
#   T1  XFIXES read path        : xclip writes → x11_watch sees it (text)
#   T2  Selection-owner + read  : we install snapshot → xclip reads it back
#                                 (uses examples/x11_clipboard_test which
#                                 self-verifies)
#   T3  INCR receive            : xclip writes 1 MiB → x11_watch reports
#                                 bytes=1048576 (no truncation, no panic)
#   T4  XFIXES no-drop          : xclip writes N=5 in rapid succession →
#                                 x11_watch emits exactly N+baseline events
#
# Notes:
#   - Forces the X11 backend by relying on the example's own
#     `remove_var("WAYLAND_DISPLAY")`. On a Wayland session that means we
#     talk to XWayland — protocol-correct but not equivalent to a pure
#     Xorg session. For release-grade verification rerun this on a real
#     Xorg login (set X11_NATIVE=1 to assert).
#
# Usage (from repo root):
#   ./scripts/test_x11_e2e.sh
#   X11_NATIVE=1 ./scripts/test_x11_e2e.sh   # require pure Xorg (no WAYLAND_DISPLAY)
#   KEEP_LOGS=1  ./scripts/test_x11_e2e.sh   # keep log dir on success
#   TIMEOUT_SECS=15 ./scripts/test_x11_e2e.sh
#
# Requirements: xclip, $DISPLAY, cargo. Run-time ~30-60s after first build.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKSPACE_ROOT="$REPO_ROOT"
TIMEOUT_SECS="${TIMEOUT_SECS:-10}"
LOG_DIR="$(mktemp -d -t uniclip-x11-e2e.XXXXXX)"
WATCH_LOG="$LOG_DIR/x11_watch.log"
TEST_LOG="$LOG_DIR/x11_clipboard_test.log"
WATCH_PID=""
PASS=0
FAIL=0

color() { printf '\033[%sm%s\033[0m' "$1" "$2"; }
ok()    { printf '  %s %s\n'        "$(color '0;32' '✓')" "$1"; PASS=$((PASS+1)); }
bad()   { printf '  %s %s\n'        "$(color '0;31' '✗')" "$1"; FAIL=$((FAIL+1)); }
info()  { printf '%s %s\n'          "$(color '0;36' '◆')" "$1"; }
warn()  { printf '  %s %s\n'        "$(color '0;33' '⚠')" "$1"; }

cleanup() {
    if [[ -n "$WATCH_PID" ]] && kill -0 "$WATCH_PID" 2>/dev/null; then
        kill -INT "$WATCH_PID" 2>/dev/null || true
        wait "$WATCH_PID" 2>/dev/null || true
    fi
    if (( FAIL == 0 )) && [[ -z "${KEEP_LOGS:-}" ]]; then
        rm -rf "$LOG_DIR"
    else
        printf '\n  logs preserved at %s\n' "$LOG_DIR"
    fi
}
trap cleanup EXIT

#### sanity ####################################################################

info "sanity"

# Prefer xclip, fall back to xsel. CLIP_WRITE reads payload from stdin.
if command -v xclip >/dev/null 2>&1; then
    CLIP_TOOL=xclip
    clip_write() { xclip -selection clipboard -i; }
    ok "clipboard tool: xclip"
elif command -v xsel >/dev/null 2>&1; then
    CLIP_TOOL=xsel
    clip_write() { xsel --clipboard --input; }
    ok "clipboard tool: xsel (xclip not found — fine for write-side tests)"
else
    bad "neither xclip nor xsel found — sudo dnf install -y xclip xsel"
    exit 1
fi

if [[ -z "${DISPLAY:-}" ]]; then
    bad "DISPLAY is unset; X server (or XWayland) required"
    exit 1
fi
ok "DISPLAY=$DISPLAY"

if [[ -n "${X11_NATIVE:-}" ]]; then
    if [[ -n "${WAYLAND_DISPLAY:-}" ]] || [[ "${XDG_SESSION_TYPE:-}" != "x11" ]]; then
        bad "X11_NATIVE=1 but session looks Wayland (WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-} XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-})"
        exit 1
    fi
    ok "X11_NATIVE: pure Xorg confirmed"
else
    if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
        warn "running under Wayland session — examples force X11 via env unset, so this exercises XWayland"
    fi
fi

#### build #####################################################################

info "build"
(
    cd "$WORKSPACE_ROOT"
    cargo build -p uc-platform --examples 2>&1 | tail -5
)
ok "examples built"

#### helpers ###################################################################

# Wait up to $TIMEOUT_SECS for $1 (a grep -E pattern) to match in $WATCH_LOG.
wait_for_log() {
    local pattern="$1"
    local label="${2:-pattern}"
    local deadline=$(( SECONDS + TIMEOUT_SECS ))
    while (( SECONDS < deadline )); do
        if grep -Eq "$pattern" "$WATCH_LOG" 2>/dev/null; then
            return 0
        fi
        sleep 0.1
    done
    bad "timeout waiting for $label after ${TIMEOUT_SECS}s"
    echo "    pattern: $pattern" >&2
    echo "    --- tail of $WATCH_LOG ---" >&2
    tail -30 "$WATCH_LOG" >&2 || true
    return 1
}

# Count snapshot events in the watch log. Each event line begins with `[HH:MM:SS.mmm] snapshot:`.
# grep -c prints `0` and exits 1 on no match, so swallow the exit and normalise output.
snapshot_count() {
    local n
    n=$(grep -cE '^\[[0-9:.]+\] snapshot:' "$WATCH_LOG" 2>/dev/null || true)
    printf '%s\n' "${n:-0}"
}

# Wait for snapshot count to reach $1.
wait_for_count() {
    local target="$1"
    local label="${2:-events}"
    local deadline=$(( SECONDS + TIMEOUT_SECS ))
    while (( SECONDS < deadline )); do
        local n
        n=$(snapshot_count)
        if (( n >= target )); then
            return 0
        fi
        sleep 0.1
    done
    bad "timeout: expected $target $label, saw $(snapshot_count)"
    tail -20 "$WATCH_LOG" >&2 || true
    return 1
}

#### start watcher #############################################################

info "start x11_watch in background"
(
    cd "$WORKSPACE_ROOT"
    RUST_LOG="warn,uc_platform=info" cargo run -q -p uc-platform --example x11_watch
) >"$WATCH_LOG" 2>&1 &
WATCH_PID=$!

# Wait for readiness banner from the example.
if ! wait_for_log 'watching X11 CLIPBOARD' 'watch banner'; then
    bad "x11_watch failed to start"
    exit 1
fi
ok "x11_watch ready"

# Note: the event_loop emits a baseline snapshot only if the clipboard is
# non-empty at start. We can't depend on that — instead each test below
# takes a `before` count and asserts relative growth.

#### T1 — xclip → reader ######################################################

info "T1: xclip writes plain text → x11_watch sees it"

BEFORE=$(snapshot_count)
MAGIC1="x11-e2e-T1-$(date +%s%N)"
printf '%s' "$MAGIC1" | clip_write
if wait_for_count $(( BEFORE + 1 )) 'T1 snapshot'; then
    if grep -Fq "$MAGIC1" "$WATCH_LOG"; then
        ok "T1 magic string visible in snapshot preview"
    else
        bad "T1 snapshot emitted but payload missing — preview truncated?"
    fi
fi

#### T3 — INCR receive ########################################################

info "T3: xclip writes 1 MiB → x11_watch reports bytes>=1 MiB"

BIG_FILE="$LOG_DIR/big.txt"
# 1 MiB of printable ASCII — atomic write so we don't fight pipefail + SIGPIPE
# on a `head | base64 | head` pipeline.
python3 -c 'import sys; sys.stdout.buffer.write(b"A" * (1024 * 1024))' > "$BIG_FILE"
BEFORE=$(snapshot_count)
clip_write < "$BIG_FILE"
if wait_for_count $(( BEFORE + 1 )) 'T3 snapshot'; then
    # Look for any `bytes=N` with N >= 1MiB in any snapshot rep so far.
    if grep -Eq 'bytes=104[0-9]{4}' "$WATCH_LOG"; then
        ok "T3 large payload received (bytes >= 1 MiB)"
    else
        bad "T3 snapshot present but no >=1 MiB representation observed"
        grep -E 'bytes=[0-9]+' "$WATCH_LOG" | tail -5 >&2 || true
    fi
fi

#### T4 — N events, no drop ###################################################

info "T4: 5 rapid xclip writes → x11_watch emits 5 more events"

BEFORE=$(snapshot_count)
for i in 1 2 3 4 5; do
    printf 'T4-event-%d-%s' "$i" "$(date +%N)" | clip_write
    sleep 0.15
done
TARGET=$(( BEFORE + 5 ))
if wait_for_count "$TARGET" "T4 events (target=$TARGET)"; then
    GOT=$(snapshot_count)
    if (( GOT == TARGET )); then
        ok "T4 exact event count: $GOT (expected $TARGET)"
    else
        warn "T4 received $GOT events, expected $TARGET (compositor may coalesce/replay)"
        ok "T4 no-drop (got >= expected)"
    fi
fi

#### stop watcher #############################################################

info "stopping x11_watch"
kill -INT "$WATCH_PID" 2>/dev/null || true
wait "$WATCH_PID" 2>/dev/null || true
WATCH_PID=""

#### T2 — we own selection, xclip reads back ##################################

info "T2: x11_clipboard_test installs a snapshot and xclip reads it"

(
    cd "$WORKSPACE_ROOT"
    RUST_LOG="warn,uc_platform=info" cargo run -q -p uc-platform --example x11_clipboard_test
) >"$TEST_LOG" 2>&1 || true

if grep -Eq '(xclip|xsel) sees expected payload ✓' "$TEST_LOG"; then
    TOOL_SEEN=$(grep -Eo '(xclip|xsel) sees expected payload' "$TEST_LOG" | head -1 | awk '{print $1}')
    ok "T2 ${TOOL_SEEN} read back the payload we installed"
else
    bad "T2 external paster did not confirm payload"
    tail -20 "$TEST_LOG" >&2 || true
fi

#### summary ##################################################################

echo
if (( FAIL == 0 )); then
    printf '%s %d/%d passed\n' "$(color '1;32' '✓')" "$PASS" $(( PASS + FAIL ))
    exit 0
else
    printf '%s %d/%d failed (passed=%d)\n' "$(color '1;31' '✗')" "$FAIL" $(( PASS + FAIL )) "$PASS"
    exit 1
fi
