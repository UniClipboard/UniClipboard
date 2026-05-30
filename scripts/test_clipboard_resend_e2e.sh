#!/usr/bin/env bash
#
# Single-machine end-to-end test for `uniclip send --resend` — ADR-005
# Stage 1a CLI surface.
#
# Mirrors `test_clipboard_e2e.sh`'s alice/bob pairing recipe, then
# walks the 7 verdicts that cover the resend command:
#
#   1. bob OFFLINE, alice resend     → no acceptance / no duplicate
#   2. bob ONLINE (watch) → resend   → totalAccepted=1, bob receives text
#   3. resend already-delivered      → NoEligibleTargets, exit != 0
#   4. resend --peer <ghost-id>      → TargetNotTrusted,  exit != 0
#   5. resend --peer <bob-id>        → filter mode bypasses diff,
#      (on a delivered entry)         totalAccepted=1, bob receipts >= 2
#   6. resend on non-existent entry  → EntryNotFound,    exit != 0
#   7. `send <text> --resend <id>`    → clap mutex,       exit == 2
#
# Requirements:
#   - Linux or macOS (paths auto-detected; macOS uses
#     `~/Library/Application Support`, Linux uses `~/.local/share`).
#   - Network access to the production rendezvous service.
#   - `--dev` mode (avoids keyring collisions between profiles).
#
# Rerun: wipes both profiles' data dirs on entry; idempotent.

set -euo pipefail

CLI="${CLI:-./src-tauri/target/debug/uniclip}"
PASSPHRASE="${PASSPHRASE:-hunter22hunter22}"
PAIR_WAIT_SECS="${PAIR_WAIT_SECS:-30}"
WATCH_READY_SECS="${WATCH_READY_SECS:-30}"
DELIVER_SECS="${DELIVER_SECS:-10}"
FIXTURE_TEXT="${FIXTURE_TEXT:-fixture-resend-A}"
COMMON_FLAGS="--dev"

# ─── Platform-specific data-dir root ─────────────────────────────────────────

case "$(uname -s)" in
    Darwin)
        APP_ROOT="$HOME/Library/Application Support"
        ;;
    Linux)
        APP_ROOT="$HOME/.local/share"
        ;;
    *)
        echo "ERROR: unsupported platform $(uname -s)" >&2
        exit 2
        ;;
esac

ALICE_DIR="$APP_ROOT/app.uniclipboard.desktop-alice"
BOB_DIR="$APP_ROOT/app.uniclipboard.desktop-bob"

if [[ ! -x "$CLI" ]]; then
    echo "ERROR: CLI binary not found at $CLI" >&2
    echo "Build first: cargo build -p uc-cli --bin uniclip" >&2
    exit 2
fi

# ─── Cleanup ─────────────────────────────────────────────────────────────────

ALICE_INVITE_OUT=""
SEED_OUT=""
BOB_WATCH_OUT=""
BOB_WATCH_ERR=""
ALICE_INVITE_PID=""
BOB_WATCH_PID=""

cleanup() {
    if [[ -n "$BOB_WATCH_PID" ]] && kill -0 "$BOB_WATCH_PID" 2>/dev/null; then
        kill -INT "$BOB_WATCH_PID" 2>/dev/null || true
        sleep 0.5
        kill -KILL "$BOB_WATCH_PID" 2>/dev/null || true
        wait "$BOB_WATCH_PID" 2>/dev/null || true
    fi
    if [[ -n "$ALICE_INVITE_PID" ]] && kill -0 "$ALICE_INVITE_PID" 2>/dev/null; then
        kill -INT "$ALICE_INVITE_PID" 2>/dev/null || true
        wait "$ALICE_INVITE_PID" 2>/dev/null || true
    fi
    [[ -n "$ALICE_INVITE_OUT" ]] && rm -f "$ALICE_INVITE_OUT"
    [[ -n "$SEED_OUT" ]] && rm -f "$SEED_OUT"
    [[ -n "$BOB_WATCH_OUT" ]] && rm -f "$BOB_WATCH_OUT"
    [[ -n "$BOB_WATCH_ERR" ]] && rm -f "$BOB_WATCH_ERR"
}
trap cleanup EXIT

echo "==> Wiping previous profile state"
/bin/rm -rf "$ALICE_DIR" "$BOB_DIR"

# ─── Pair (mirrors test_pair_e2e.sh / test_clipboard_e2e.sh) ────────────────

echo "==> alice: init"
"$CLI" $COMMON_FLAGS --profile alice init \
    --passphrase "$PASSPHRASE" \
    --device-name "alice (resend e2e)" >/dev/null

echo "==> alice: invite (background)"
ALICE_INVITE_OUT="$(mktemp -t uc_alice_invite.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile alice invite > "$ALICE_INVITE_OUT" 2>&1 &
ALICE_INVITE_PID=$!

echo "==> Waiting up to ${PAIR_WAIT_SECS}s for INVITATION_CODE"
CODE=""
for ((i = 0; i < PAIR_WAIT_SECS * 2; i++)); do
    if [[ -s "$ALICE_INVITE_OUT" ]] && grep -q '^INVITATION_CODE=' "$ALICE_INVITE_OUT" 2>/dev/null; then
        CODE="$(grep -E '^INVITATION_CODE=' "$ALICE_INVITE_OUT" | head -1 | cut -d= -f2)"
        if [[ -n "$CODE" ]]; then
            break
        fi
    fi
    if ! kill -0 "$ALICE_INVITE_PID" 2>/dev/null; then
        echo "ERROR: alice invite exited before printing INVITATION_CODE" >&2
        sed 's/^/  alice | /' "$ALICE_INVITE_OUT" >&2
        exit 1
    fi
    sleep 0.5
done
if [[ -z "$CODE" ]]; then
    echo "ERROR: timed out waiting for invitation code" >&2
    sed 's/^/  alice | /' "$ALICE_INVITE_OUT" >&2
    exit 1
fi
echo "    got code: $CODE"

echo "==> bob: join --code $CODE"
set +e
"$CLI" $COMMON_FLAGS --profile bob join \
    --code "$CODE" \
    --passphrase "$PASSPHRASE" \
    --device-name "bob (resend e2e)" >/dev/null
BOB_JOIN_EXIT=$?
set -e
echo "    bob join exited with $BOB_JOIN_EXIT"
if [[ $BOB_JOIN_EXIT -ne 0 ]]; then
    echo "FAIL: bob join failed; aborting" >&2
    exit 1
fi

echo "==> Waiting for alice invite to settle"
set +e
wait "$ALICE_INVITE_PID"
ALICE_INVITE_EXIT=$?
set -e
ALICE_INVITE_PID=""
echo "    alice invite exited with $ALICE_INVITE_EXIT"
if [[ $ALICE_INVITE_EXIT -ne 0 ]]; then
    echo "FAIL: alice invite failed" >&2
    sed 's/^/  alice | /' "$ALICE_INVITE_OUT" >&2
    exit 1
fi

# Extract bob's device id from the "Pairing completed" block in alice's
# invite log. The line shape (with the dim │ prefix) is:
#   ` │  peer_device_id: 76312f92-75db-4b59-a2e3-30a8745df965`
BOB_DEVICE_ID="$(grep -E 'peer_device_id:' "$ALICE_INVITE_OUT" | head -1 | sed -E 's/.*peer_device_id:[[:space:]]*([a-zA-Z0-9_-]+).*/\1/')"
if [[ -z "$BOB_DEVICE_ID" ]]; then
    echo "FAIL: could not extract bob device_id from alice invite log" >&2
    sed 's/^/  alice | /' "$ALICE_INVITE_OUT" >&2
    exit 1
fi
echo "    bob device_id: $BOB_DEVICE_ID"

# ─── Seed an entry on alice ─────────────────────────────────────────────────

echo "==> alice: dev seed-clipboard --text \"$FIXTURE_TEXT\""
SEED_OUT="$(mktemp -t uc_seed.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile alice dev seed-clipboard --text "$FIXTURE_TEXT" > "$SEED_OUT" 2>&1
ENTRY_ID="$(grep -E '^SEED_ENTRY_ID=' "$SEED_OUT" | head -1 | cut -d= -f2)"
if [[ -z "$ENTRY_ID" ]]; then
    echo "FAIL: alice dev seed-clipboard didn't emit SEED_ENTRY_ID" >&2
    cat "$SEED_OUT" >&2
    exit 1
fi
echo "    seeded entry: $ENTRY_ID"

# ─── Verdicts ────────────────────────────────────────────────────────────────

# Verdict 1: bob OFFLINE → no acceptance (in-deadline pending is acceptable
# as long as nothing actually delivered).
echo ""
echo "==> Verdict 1: bob OFFLINE, alice resend → no delivery"
V1_OUT="$(mktemp -t uc_v1.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice --json send --resend "$ENTRY_ID" > "$V1_OUT" 2>&1
V1_EXIT=$?
set -e
echo "    exit=$V1_EXIT"
if grep -qE '"totalAccepted":[[:space:]]*[1-9]' "$V1_OUT" \
    || grep -qE '"totalDuplicate":[[:space:]]*[1-9]' "$V1_OUT"; then
    echo "FAIL: verdict 1 reported a real delivery while bob is offline" >&2
    cat "$V1_OUT" >&2
    exit 1
fi
rm -f "$V1_OUT"
echo "    PASS — no acceptance / duplicate"

# Verdict 2: start bob watch, resend → 1 accepted; bob receives the text.
echo ""
echo "==> Verdict 2: bob ONLINE (watch), alice resend → delivered"
BOB_WATCH_OUT="$(mktemp -t uc_bob_watch.XXXXXX)"
BOB_WATCH_ERR="$(mktemp -t uc_bob_watch_err.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile bob --json watch > "$BOB_WATCH_OUT" 2> "$BOB_WATCH_ERR" &
BOB_WATCH_PID=$!
echo "    bob watch pid=$BOB_WATCH_PID; waiting up to ${WATCH_READY_SECS}s for WATCH_READY"
for ((i = 0; i < WATCH_READY_SECS * 2; i++)); do
    if grep -q '^WATCH_READY$' "$BOB_WATCH_ERR" 2>/dev/null; then
        echo "    bob ready after $((i / 2)).$((i % 2 * 5))s"
        break
    fi
    if ! kill -0 "$BOB_WATCH_PID" 2>/dev/null; then
        echo "FAIL: bob watch exited before signaling WATCH_READY" >&2
        cat "$BOB_WATCH_ERR" >&2
        exit 1
    fi
    sleep 0.5
done
if ! grep -q '^WATCH_READY$' "$BOB_WATCH_ERR" 2>/dev/null; then
    echo "FAIL: bob watch did not reach WATCH_READY within ${WATCH_READY_SECS}s" >&2
    cat "$BOB_WATCH_ERR" >&2
    exit 1
fi

V2_OUT="$(mktemp -t uc_v2.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice --json send --resend "$ENTRY_ID" > "$V2_OUT" 2>&1
V2_EXIT=$?
set -e
echo "    exit=$V2_EXIT"
if [[ $V2_EXIT -ne 0 ]]; then
    echo "FAIL: verdict 2 expected exit 0, got $V2_EXIT" >&2
    cat "$V2_OUT" >&2
    exit 1
fi
if ! grep -qE '"totalAccepted":[[:space:]]*1' "$V2_OUT"; then
    echo "FAIL: verdict 2 expected totalAccepted=1" >&2
    cat "$V2_OUT" >&2
    exit 1
fi
for ((i = 0; i < DELIVER_SECS * 2; i++)); do
    if grep -q "\"text\":\"$FIXTURE_TEXT\"" "$BOB_WATCH_OUT" 2>/dev/null; then
        break
    fi
    sleep 0.5
done
if ! grep -q "\"text\":\"$FIXTURE_TEXT\"" "$BOB_WATCH_OUT" 2>/dev/null; then
    echo "FAIL: bob did not receive the resent text within ${DELIVER_SECS}s" >&2
    cat "$BOB_WATCH_OUT" >&2
    exit 1
fi
rm -f "$V2_OUT"
echo "    PASS — totalAccepted=1, bob received the text"

# Verdict 3: resend already-delivered → NoEligibleTargets.
echo ""
echo "==> Verdict 3: resend already-delivered → NoEligibleTargets"
V3_OUT="$(mktemp -t uc_v3.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice send --resend "$ENTRY_ID" > "$V3_OUT" 2>&1
V3_EXIT=$?
set -e
echo "    exit=$V3_EXIT"
if [[ $V3_EXIT -eq 0 ]]; then
    echo "FAIL: verdict 3 expected non-zero exit, got 0" >&2
    cat "$V3_OUT" >&2
    exit 1
fi
if ! grep -q "All trusted peers have already received this entry" "$V3_OUT"; then
    echo "FAIL: verdict 3 didn't surface the NoEligibleTargets message" >&2
    cat "$V3_OUT" >&2
    exit 1
fi
rm -f "$V3_OUT"
echo "    PASS — NoEligibleTargets"

# Verdict 4: --peer pointing at a non-trusted device → TargetNotTrusted.
echo ""
echo "==> Verdict 4: --peer ghost-device-xyz → TargetNotTrusted"
V4_OUT="$(mktemp -t uc_v4.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice send --resend "$ENTRY_ID" --peer "ghost-device-xyz" > "$V4_OUT" 2>&1
V4_EXIT=$?
set -e
echo "    exit=$V4_EXIT"
if [[ $V4_EXIT -eq 0 ]]; then
    echo "FAIL: verdict 4 expected non-zero exit, got 0" >&2
    cat "$V4_OUT" >&2
    exit 1
fi
if ! grep -q "is not a trusted peer" "$V4_OUT"; then
    echo "FAIL: verdict 4 didn't surface the TargetNotTrusted message" >&2
    cat "$V4_OUT" >&2
    exit 1
fi
rm -f "$V4_OUT"
echo "    PASS — TargetNotTrusted"

# Verdict 5: --peer <bob> on already-delivered entry → filter mode bypasses
# the diff and dispatches once more.
echo ""
echo "==> Verdict 5: --peer <bob> on delivered entry → filter mode dispatches"
V5_OUT="$(mktemp -t uc_v5.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice --json send --resend "$ENTRY_ID" --peer "$BOB_DEVICE_ID" > "$V5_OUT" 2>&1
V5_EXIT=$?
set -e
echo "    exit=$V5_EXIT"
if [[ $V5_EXIT -ne 0 ]]; then
    echo "FAIL: verdict 5 expected exit 0, got $V5_EXIT" >&2
    cat "$V5_OUT" >&2
    exit 1
fi
if ! grep -qE '"totalAccepted":[[:space:]]*1' "$V5_OUT"; then
    echo "FAIL: verdict 5 expected totalAccepted=1" >&2
    cat "$V5_OUT" >&2
    exit 1
fi
for ((i = 0; i < DELIVER_SECS * 2; i++)); do
    count="$(grep -c "\"text\":\"$FIXTURE_TEXT\"" "$BOB_WATCH_OUT" 2>/dev/null || echo 0)"
    if [[ "$count" -ge 2 ]]; then
        break
    fi
    sleep 0.5
done
count="$(grep -c "\"text\":\"$FIXTURE_TEXT\"" "$BOB_WATCH_OUT" 2>/dev/null || echo 0)"
if [[ "$count" -lt 2 ]]; then
    echo "FAIL: bob expected to receive the text >= 2 times, got $count" >&2
    cat "$BOB_WATCH_OUT" >&2
    exit 1
fi
rm -f "$V5_OUT"
echo "    PASS — totalAccepted=1, bob receipts=$count"

# Verdict 6: non-existent entry id → EntryNotFound.
echo ""
echo "==> Verdict 6: non-existent entry id → EntryNotFound"
V6_OUT="$(mktemp -t uc_v6.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice send --resend "nonexistent-id-12345" > "$V6_OUT" 2>&1
V6_EXIT=$?
set -e
echo "    exit=$V6_EXIT"
if [[ $V6_EXIT -eq 0 ]]; then
    echo "FAIL: verdict 6 expected non-zero exit, got 0" >&2
    cat "$V6_OUT" >&2
    exit 1
fi
if ! grep -q "not found in local storage" "$V6_OUT"; then
    echo "FAIL: verdict 6 didn't surface the EntryNotFound message" >&2
    cat "$V6_OUT" >&2
    exit 1
fi
rm -f "$V6_OUT"
echo "    PASS — EntryNotFound"

# Verdict 7: clap mutex — positional <text> conflicts with --resend.
echo ""
echo "==> Verdict 7: send <text> --resend <id> → clap mutex"
V7_OUT="$(mktemp -t uc_v7.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice send hello --resend foo > "$V7_OUT" 2>&1
V7_EXIT=$?
set -e
echo "    exit=$V7_EXIT"
if [[ $V7_EXIT -ne 2 ]]; then
    echo "FAIL: verdict 7 expected clap exit code 2, got $V7_EXIT" >&2
    cat "$V7_OUT" >&2
    exit 1
fi
if ! grep -q "cannot be used with" "$V7_OUT"; then
    echo "FAIL: verdict 7 didn't surface the clap conflict message" >&2
    cat "$V7_OUT" >&2
    exit 1
fi
rm -f "$V7_OUT"
echo "    PASS — clap mutex"

# Stop bob watch (cleanup() also handles it, but stopping now lets the
# success line stand alone at the bottom of stdout).
if [[ -n "$BOB_WATCH_PID" ]] && kill -0 "$BOB_WATCH_PID" 2>/dev/null; then
    kill -INT "$BOB_WATCH_PID" 2>/dev/null || true
    sleep 0.5
    kill -KILL "$BOB_WATCH_PID" 2>/dev/null || true
    wait "$BOB_WATCH_PID" 2>/dev/null || true
fi
BOB_WATCH_PID=""

echo ""
echo "PASS: clipboard resend e2e verified (7 verdicts)"
