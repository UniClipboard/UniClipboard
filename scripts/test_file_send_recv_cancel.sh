#!/usr/bin/env bash
#
# End-to-end verification of `uniclip send -f` + `uniclip recv` + Ctrl-C
# cancel pipeline (P1-7 verdict for the cancel work).
#
# Pair alice/bob via the same flow as test_clipboard_e2e.sh, then:
#   1. CANCEL CASE:  alice serves big.bin → bob recv → SIGINT bob mid-fetch
#                    → expect "Cancelled" outcome + no file at target.
#   2. SUCCESS CASE: alice serves small.bin → bob recv to completion
#                    → file landed + bytes match.
#
# Requirements: macOS, network (rendezvous), --dev mode.

set -eu
# Note: deliberately no `pipefail` — diagnostic tails (`tail | sed`) here
# can take SIGPIPE when the reader closes early; we don't want that to
# abort the script.

CLI="${CLI:-./src-tauri/target/debug/uniclip}"
PASSPHRASE="${PASSPHRASE:-hunter22hunter22}"
PAIR_WAIT_SECS="${PAIR_WAIT_SECS:-30}"
BIG_SIZE_MB="${BIG_SIZE_MB:-800}"
SMALL_SIZE_KB="${SMALL_SIZE_KB:-32}"
CANCEL_DELAY_SECS="${CANCEL_DELAY_SECS:-4}"
COMMON_FLAGS="--dev"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "ERROR: macOS only." >&2; exit 2
fi
if [[ ! -x "$CLI" ]]; then
    echo "ERROR: CLI not built at $CLI" >&2; exit 2
fi

APP_ROOT="$HOME/Library/Application Support"
ALICE_DIR="$APP_ROOT/app.uniclipboard.desktop-alice"
BOB_DIR="$APP_ROOT/app.uniclipboard.desktop-bob"
WORK="$(mktemp -d -t uc_filetest.XXXXXX)"
BIG_FILE="$WORK/big.bin"
SMALL_FILE="$WORK/small.bin"
BOB_OUT_DIR="$WORK/inbox"
mkdir -p "$BOB_OUT_DIR"

ALICE_INVITE_PID=""
ALICE_SEND_PID=""
BOB_RECV_PID=""
ALICE_INVITE_OUT=""
ALICE_SEND_OUT=""
BOB_RECV_OUT=""
BOB_RECV_ERR=""

cleanup() {
    for pid in "$BOB_RECV_PID" "$ALICE_SEND_PID" "$ALICE_INVITE_PID"; do
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            kill -INT "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$WORK" "$ALICE_INVITE_OUT" "$ALICE_SEND_OUT" "$BOB_RECV_OUT" "$BOB_RECV_ERR" 2>/dev/null || true
}
trap cleanup EXIT

echo "==> Wiping profile state"
/bin/rm -rf "$ALICE_DIR" "$BOB_DIR"

echo "==> Generating fixtures: big=${BIG_SIZE_MB}MB, small=${SMALL_SIZE_KB}KB"
dd if=/dev/urandom of="$BIG_FILE" bs=1m count="$BIG_SIZE_MB" status=none
dd if=/dev/urandom of="$SMALL_FILE" bs=1k count="$SMALL_SIZE_KB" status=none
BIG_HASH=$(shasum -a 256 "$BIG_FILE" | awk '{print $1}')
SMALL_HASH=$(shasum -a 256 "$SMALL_FILE" | awk '{print $1}')

# ─── pair ───────────────────────────────────────────────────────────────
echo "==> alice: init"
"$CLI" $COMMON_FLAGS --profile alice init \
    --passphrase "$PASSPHRASE" \
    --device-name "alice (file-cancel e2e)" > /dev/null

echo "==> alice: invite (background)"
ALICE_INVITE_OUT="$(mktemp -t uc_alice_invite.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile alice invite > "$ALICE_INVITE_OUT" 2>&1 &
ALICE_INVITE_PID=$!

CODE=""
for ((i=0; i<PAIR_WAIT_SECS*2; i++)); do
    if [[ -s "$ALICE_INVITE_OUT" ]]; then
        CODE=$(grep -E '^INVITATION_CODE=' "$ALICE_INVITE_OUT" | head -1 | cut -d= -f2 || true)
        [[ -n "$CODE" ]] && break
    fi
    kill -0 "$ALICE_INVITE_PID" 2>/dev/null || { echo "alice invite died early"; sed 's/^/  alice | /' "$ALICE_INVITE_OUT"; exit 1; }
    sleep 0.5
done
[[ -z "$CODE" ]] && { echo "timeout waiting INVITATION_CODE"; sed 's/^/  alice | /' "$ALICE_INVITE_OUT"; exit 1; }
echo "    code: $CODE"

echo "==> bob: join"
"$CLI" $COMMON_FLAGS --profile bob join \
    --code "$CODE" --passphrase "$PASSPHRASE" \
    --device-name "bob (file-cancel e2e)" > /dev/null
wait "$ALICE_INVITE_PID"; ALICE_INVITE_PID=""

# ─── Case 1: CANCEL ─────────────────────────────────────────────────────
echo
echo "==> CASE 1: cancel mid-transfer"

echo "    bob: recv --out $BOB_OUT_DIR (background, JSON)"
BOB_RECV_OUT="$(mktemp -t uc_bob_recv.XXXXXX)"
BOB_RECV_ERR="$(mktemp -t uc_bob_recv_err.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile bob --json recv --out "$BOB_OUT_DIR" \
    > "$BOB_RECV_OUT" 2> "$BOB_RECV_ERR" &
BOB_RECV_PID=$!

# Give bob's session + subscribe time to settle. recv has no
# WATCH_READY marker; use a fixed cushion + tail the log.
echo "    waiting 15s for bob to subscribe + probe"
sleep 15
if ! kill -0 "$BOB_RECV_PID" 2>/dev/null; then
    echo "FAIL: bob recv died early"; cat "$BOB_RECV_ERR"; exit 1
fi

echo "    alice: send -f $BIG_FILE (background, stays alive after dispatch)"
ALICE_SEND_OUT="$(mktemp -t uc_alice_send.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile alice --json send -f "$BIG_FILE" \
    > "$ALICE_SEND_OUT" 2>&1 &
ALICE_SEND_PID=$!

# Fixed delay before SIGINT. Big file (~800MB default) on local-host
# iroh-blobs takes a noticeable number of seconds, so a 4-second hold
# from alice's send fire reliably catches bob mid-fetch.
echo "    holding ${CANCEL_DELAY_SECS}s, then SIGINT bob"
sleep "$CANCEL_DELAY_SECS"

# Make sure bob is still running before we SIGINT — if the transfer
# already finished, we're testing the wrong thing.
if ! kill -0 "$BOB_RECV_PID" 2>/dev/null; then
    echo "    --- bob recv stderr ---"
    cat "$BOB_RECV_ERR" | sed 's/^/      bob   | /'
    echo "FAIL CASE 1: bob already exited before SIGINT (transfer finished too fast — bump BIG_SIZE_MB)" >&2
    exit 1
fi

kill -INT "$BOB_RECV_PID"
echo "    waiting up to 20s for bob to exit"
# Capture exit safely — `wait` under `set -e` would abort the script
# before $? could be assigned.
BOB_RECV_EXIT=0
for ((i=0; i<40; i++)); do
    if ! kill -0 "$BOB_RECV_PID" 2>/dev/null; then
        set +e; wait "$BOB_RECV_PID"; BOB_RECV_EXIT=$?; set -e
        break
    fi
    sleep 0.5
done
if kill -0 "$BOB_RECV_PID" 2>/dev/null; then
    echo "FAIL CASE 1: bob did not exit within 20s of SIGINT — force-killing" >&2
    kill -KILL "$BOB_RECV_PID" 2>/dev/null || true
    set +e; wait "$BOB_RECV_PID"; set -e
    BOB_RECV_PID=""
    cat "$BOB_RECV_ERR" | sed 's/^/      bob   | /'
    exit 1
fi
BOB_RECV_PID=""
echo "    bob recv exit=$BOB_RECV_EXIT (expected 1 for cancel path)"

# Verdicts:
# 1. JSON outcome (if printed before SIGINT processed) shows
#    "outcome":"cancelled" — or stderr trace mentions 'Cancelled'.
# 2. partial file is cleaned (target_path absent).
echo "    --- bob recv stdout ---"
sed 's/^/      bob | /' "$BOB_RECV_OUT" | head -40
echo "    --- bob recv stderr (last 40 lines) ---"
tail -40 "$BOB_RECV_ERR" | sed 's/^/      bob | /'

CANCELLED_MARKER=0
if grep -q '"outcome":"cancelled"\|"outcome": *"cancelled"' "$BOB_RECV_OUT"; then
    CANCELLED_MARKER=1
fi
if grep -qi "Cancelled\|cancel_inbound_transfer" "$BOB_RECV_ERR"; then
    CANCELLED_MARKER=1
fi

if [[ $CANCELLED_MARKER -eq 0 ]]; then
    echo "FAIL CASE 1: no Cancelled marker in bob output" >&2
    exit 1
fi

EXPECTED_PARTIAL="$BOB_OUT_DIR/big.bin"
if [[ -f "$EXPECTED_PARTIAL" ]]; then
    echo "FAIL CASE 1: partial file still present at $EXPECTED_PARTIAL ($(stat -f%z "$EXPECTED_PARTIAL") bytes)" >&2
    exit 1
fi
echo "    PASS CASE 1: cancelled + partial cleaned"

# Kill alice send (still serving as passive provider)
if kill -0 "$ALICE_SEND_PID" 2>/dev/null; then
    kill -INT "$ALICE_SEND_PID"; wait "$ALICE_SEND_PID" 2>/dev/null || true
fi
ALICE_SEND_PID=""

# Verify alice's iroh router noticed the connection drop (verbose log
# should show provider-side teardown). Best-effort signal — don't fail
# the test if log grep misses; print for human review.
echo "    --- alice send tail (last 30 lines) ---"
tail -30 "$ALICE_SEND_OUT" | sed 's/^/      alice | /'

# ─── Case 2: SUCCESS ────────────────────────────────────────────────────
echo
echo "==> CASE 2: successful small-file transfer"

rm -rf "$BOB_OUT_DIR"; mkdir -p "$BOB_OUT_DIR"
> "$BOB_RECV_OUT"; > "$BOB_RECV_ERR"
"$CLI" $COMMON_FLAGS --profile bob --json recv --out "$BOB_OUT_DIR" \
    > "$BOB_RECV_OUT" 2> "$BOB_RECV_ERR" &
BOB_RECV_PID=$!

echo "    waiting 8s for bob recv warmup"
sleep 8
if ! kill -0 "$BOB_RECV_PID" 2>/dev/null; then
    echo "FAIL CASE 2: bob recv died early"; cat "$BOB_RECV_ERR"; exit 1
fi

> "$ALICE_SEND_OUT"
"$CLI" $COMMON_FLAGS --profile alice --json send -f "$SMALL_FILE" \
    > "$ALICE_SEND_OUT" 2>&1 &
ALICE_SEND_PID=$!

# bob recv exits on success; wait up to 30s
RECV_DONE=0
for ((i=0; i<60; i++)); do
    if ! kill -0 "$BOB_RECV_PID" 2>/dev/null; then
        RECV_DONE=1; break
    fi
    sleep 0.5
done
if [[ $RECV_DONE -eq 0 ]]; then
    echo "FAIL CASE 2: bob recv did not exit within 30s" >&2
    kill -INT "$BOB_RECV_PID"; wait "$BOB_RECV_PID" 2>/dev/null || true
    cat "$BOB_RECV_ERR"; exit 1
fi
wait "$BOB_RECV_PID" 2>/dev/null; BOB_RECV_PID=""

DELIVERED="$BOB_OUT_DIR/small.bin"
if [[ ! -f "$DELIVERED" ]]; then
    echo "FAIL CASE 2: receiver file missing at $DELIVERED" >&2
    sed 's/^/      bob | /' "$BOB_RECV_OUT" >&2
    exit 1
fi
DELIVERED_HASH=$(shasum -a 256 "$DELIVERED" | awk '{print $1}')
if [[ "$DELIVERED_HASH" != "$SMALL_HASH" ]]; then
    echo "FAIL CASE 2: hash mismatch — sent=$SMALL_HASH delivered=$DELIVERED_HASH" >&2
    exit 1
fi
echo "    PASS CASE 2: small file received intact ($DELIVERED_HASH)"

if kill -0 "$ALICE_SEND_PID" 2>/dev/null; then
    kill -INT "$ALICE_SEND_PID"; wait "$ALICE_SEND_PID" 2>/dev/null || true
fi
ALICE_SEND_PID=""

echo
echo "==> ALL PASS: cancel pipeline E2E verified"
