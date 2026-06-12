#!/usr/bin/env bash
#
# Single-machine end-to-end clipboard sync smoke test (Slice 2, Phase
# 2 wire-level + Phase 3 CLI envelope).
#
# Builds on `test_pair_e2e.sh`'s flow (alice/bob profiles, --dev mode,
# real rendezvous), then exercises the iroh clipboard ALPN via
# `ClipboardSyncFacade::dispatch_snapshot` (Phase 3 · T9):
#   * alice: init (A1) + invite (B1)
#   * bob:   join (B2)
#   * bob:   watch in background (subscribes + decodes V3 envelope)
#   * alice: send "hello phase3"
#   * verify bob's watch JSON contains `"text":"<expected>"` within
#     DELIVER_SECS (Phase 3 schema — Phase 2 `plaintext_utf8` retired
#     per T10)
#   * exercise repeat dispatch — facade-level (without daemon
#     persistence dedup) still lands twice; daemon-side dedup is
#     exercised in the Rust T12 daemon e2e, not here.
#
# Requirements:
#   - macOS (profile data dirs live under ~/Library/Application Support)
#   - Network access to the production rendezvous service
#   - --dev mode to avoid Keychain collisions between the two profiles
#
# Rerun: wipes both profiles' data dirs on entry; idempotent.

set -euo pipefail

CLI="${CLI:-./target/debug/uniclipboard-cli}"
PASSPHRASE="${PASSPHRASE:-hunter22hunter22}"
PAIR_WAIT_SECS="${PAIR_WAIT_SECS:-30}"
DELIVER_SECS="${DELIVER_SECS:-10}"
COMMON_FLAGS="--dev"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "ERROR: this script is macOS-only (paths under ~/Library/Application Support)." >&2
    exit 2
fi

if [[ ! -x "$CLI" ]]; then
    echo "ERROR: CLI binary not found at $CLI" >&2
    echo "Build first: cargo build -p uc-cli --bin uniclipboard-cli" >&2
    exit 2
fi

APP_ROOT="$HOME/Library/Application Support"
ALICE_DIR="$APP_ROOT/app.uniclipboard.desktop-alice"
BOB_DIR="$APP_ROOT/app.uniclipboard.desktop-bob"

ALICE_OUT=""
BOB_WATCH_OUT=""
BOB_WATCH_ERR=""
ALICE_PID=""
BOB_WATCH_PID=""

cleanup() {
    if [[ -n "$BOB_WATCH_PID" ]] && kill -0 "$BOB_WATCH_PID" 2>/dev/null; then
        kill -INT "$BOB_WATCH_PID" 2>/dev/null || true
        wait "$BOB_WATCH_PID" 2>/dev/null || true
    fi
    if [[ -n "$ALICE_PID" ]] && kill -0 "$ALICE_PID" 2>/dev/null; then
        kill -INT "$ALICE_PID" 2>/dev/null || true
        wait "$ALICE_PID" 2>/dev/null || true
    fi
    [[ -n "$ALICE_OUT" ]] && rm -f "$ALICE_OUT"
    [[ -n "$BOB_WATCH_OUT" ]] && rm -f "$BOB_WATCH_OUT"
    [[ -n "$BOB_WATCH_ERR" ]] && rm -f "$BOB_WATCH_ERR"
}
trap cleanup EXIT

echo "==> Wiping previous profile state"
/bin/rm -rf "$ALICE_DIR" "$BOB_DIR"

# ─── Pair (mirrors test_pair_e2e.sh) ─────────────────────────────────────────

echo "==> alice: init"
"$CLI" $COMMON_FLAGS --profile alice init \
    --passphrase "$PASSPHRASE" \
    --device-name "alice (clipboard e2e)"

echo "==> alice: invite (background)"
ALICE_OUT="$(mktemp -t uc_alice_invite.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile alice invite > "$ALICE_OUT" 2>&1 &
ALICE_PID=$!

echo "==> Waiting up to ${PAIR_WAIT_SECS}s for INVITATION_CODE"
CODE=""
for ((i = 0; i < PAIR_WAIT_SECS * 2; i++)); do
    if [[ -s "$ALICE_OUT" ]] && CODE="$(grep -E '^INVITATION_CODE=' "$ALICE_OUT" 2>/dev/null | head -1 | cut -d= -f2)"; then
        if [[ -n "$CODE" ]]; then
            break
        fi
    fi
    if ! kill -0 "$ALICE_PID" 2>/dev/null; then
        echo "ERROR: alice invite exited before printing INVITATION_CODE" >&2
        sed 's/^/  alice | /' "$ALICE_OUT" >&2
        exit 1
    fi
    sleep 0.5
done

if [[ -z "$CODE" ]]; then
    echo "ERROR: timed out waiting for invitation code" >&2
    sed 's/^/  alice | /' "$ALICE_OUT" >&2
    exit 1
fi
echo "    got code: $CODE"

echo "==> bob: join --code $CODE"
set +e
"$CLI" $COMMON_FLAGS --profile bob join \
    --code "$CODE" \
    --passphrase "$PASSPHRASE" \
    --device-name "bob (clipboard e2e)"
BOB_JOIN_EXIT=$?
set -e
echo "    bob join exited with $BOB_JOIN_EXIT"

if [[ $BOB_JOIN_EXIT -ne 0 ]]; then
    echo "FAIL: bob join failed; aborting clipboard test" >&2
    exit 1
fi

echo "==> Waiting for alice invite to settle"
set +e
wait "$ALICE_PID"
ALICE_INVITE_EXIT=$?
set -e
ALICE_PID=""
echo "    alice invite exited with $ALICE_INVITE_EXIT"

if [[ $ALICE_INVITE_EXIT -ne 0 ]]; then
    echo "FAIL: alice invite failed; aborting clipboard test" >&2
    sed 's/^/  alice | /' "$ALICE_OUT" >&2
    exit 1
fi

# ─── Phase 2 clipboard sync ──────────────────────────────────────────────────

echo "==> bob: watch (background, --json line-delimited)"
BOB_WATCH_OUT="$(mktemp -t uc_bob_watch.XXXXXX)"
BOB_WATCH_ERR="$(mktemp -t uc_bob_watch_err.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile bob --json watch > "$BOB_WATCH_OUT" 2> "$BOB_WATCH_ERR" &
BOB_WATCH_PID=$!

# Wait for the WATCH_READY handshake marker on stderr — emitted by the
# CLI right after `subscribe_inbound_notices()` returns, so any send
# fired after this point is guaranteed to land on a live subscriber.
# Without this marker, sleep-based syncs race the watch's
# `build_assembly + try_resume_session + refresh_presence` warmup
# (typically 2-5s on a cold profile) and notices fire to a not-yet-
# connected public broadcast → the test asserts a missed delivery.
WATCH_READY_SECS="${WATCH_READY_SECS:-30}"
echo "==> Waiting up to ${WATCH_READY_SECS}s for bob watch to signal WATCH_READY"
for ((i = 0; i < WATCH_READY_SECS * 2; i++)); do
    if grep -q '^WATCH_READY$' "$BOB_WATCH_ERR" 2>/dev/null; then
        echo "    bob ready after $((i / 2)).$((i % 2 * 5))s"
        break
    fi
    if ! kill -0 "$BOB_WATCH_PID" 2>/dev/null; then
        echo "ERROR: bob watch exited before signaling WATCH_READY" >&2
        echo "--- bob stderr ---" >&2
        cat "$BOB_WATCH_ERR" >&2
        echo "------------------" >&2
        exit 1
    fi
    sleep 0.5
done
if ! grep -q '^WATCH_READY$' "$BOB_WATCH_ERR" 2>/dev/null; then
    echo "FAIL: bob watch did not reach WATCH_READY within ${WATCH_READY_SECS}s" >&2
    echo "--- bob stderr ---" >&2
    cat "$BOB_WATCH_ERR" >&2
    echo "------------------" >&2
    exit 1
fi

# Verdict 1: alice send "hello phase2" → bob watch sees it within DELIVER_SECS
EXPECTED_1="hello phase2 from alice"
echo "==> alice: send \"$EXPECTED_1\""
ALICE_SEND_OUT="$(mktemp -t uc_alice_send.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile alice --json send "$EXPECTED_1" > "$ALICE_SEND_OUT" 2>/dev/null
ALICE_SEND_EXIT=$?
set -e
if [[ $ALICE_SEND_EXIT -ne 0 ]]; then
    echo "FAIL: alice send exited $ALICE_SEND_EXIT" >&2
    cat "$ALICE_SEND_OUT" >&2
    rm -f "$ALICE_SEND_OUT"
    exit 1
fi
# Sanity: outcome must show at least one acceptance (not all offline / error)
if ! grep -q '"total_accepted": *[1-9]' "$ALICE_SEND_OUT"; then
    echo "FAIL: alice send outcome shows no accepted targets" >&2
    cat "$ALICE_SEND_OUT" >&2
    rm -f "$ALICE_SEND_OUT"
    exit 1
fi
rm -f "$ALICE_SEND_OUT"

echo "==> Waiting up to ${DELIVER_SECS}s for bob watch to receive it"
for ((i = 0; i < DELIVER_SECS * 2; i++)); do
    if grep -q "\"text\":\"$EXPECTED_1\"" "$BOB_WATCH_OUT" 2>/dev/null; then
        echo "    received within $((i / 2)).$((i % 2 * 5))s"
        break
    fi
    sleep 0.5
done

if ! grep -q "\"text\":\"$EXPECTED_1\"" "$BOB_WATCH_OUT" 2>/dev/null; then
    echo "FAIL: bob did not receive verdict 1 within ${DELIVER_SECS}s" >&2
    echo "--- bob stdout dump ---" >&2
    cat "$BOB_WATCH_OUT" >&2
    echo "--- bob stderr dump ---" >&2
    cat "$BOB_WATCH_ERR" >&2
    echo "-----------------------" >&2
    exit 1
fi

# Verdict 2: stdin pipe path
EXPECTED_2="from stdin pipe"
echo "==> alice: echo \"$EXPECTED_2\" | send"
set +e
echo "$EXPECTED_2" | "$CLI" $COMMON_FLAGS --profile alice --json send > /dev/null 2>&1
ALICE_PIPE_EXIT=$?
set -e
if [[ $ALICE_PIPE_EXIT -ne 0 ]]; then
    echo "FAIL: alice stdin send exited $ALICE_PIPE_EXIT" >&2
    exit 1
fi

for ((i = 0; i < DELIVER_SECS * 2; i++)); do
    if grep -q "\"text\":\"$EXPECTED_2\"" "$BOB_WATCH_OUT" 2>/dev/null; then
        break
    fi
    sleep 0.5
done
if ! grep -q "\"text\":\"$EXPECTED_2\"" "$BOB_WATCH_OUT" 2>/dev/null; then
    echo "FAIL: bob did not receive verdict 2 (stdin) within ${DELIVER_SECS}s" >&2
    echo "--- bob watch dump ---" >&2
    cat "$BOB_WATCH_OUT" >&2
    echo "----------------------" >&2
    exit 1
fi

# Verdict 3: repeat dispatch — Phase 2 has no dedup, both must land
EXPECTED_3="duplicate-content-phase2-fixture"
echo "==> alice: send \"$EXPECTED_3\" twice (Phase 2 no dedup → expect 2 receipts)"
"$CLI" $COMMON_FLAGS --profile alice --json send "$EXPECTED_3" > /dev/null 2>&1
"$CLI" $COMMON_FLAGS --profile alice --json send "$EXPECTED_3" > /dev/null 2>&1

for ((i = 0; i < DELIVER_SECS * 2; i++)); do
    COUNT="$(grep -c "\"text\":\"$EXPECTED_3\"" "$BOB_WATCH_OUT" 2>/dev/null || echo 0)"
    if [[ "$COUNT" -ge 2 ]]; then
        break
    fi
    sleep 0.5
done
COUNT="$(grep -c "\"text\":\"$EXPECTED_3\"" "$BOB_WATCH_OUT" 2>/dev/null || echo 0)"
if [[ "$COUNT" -lt 2 ]]; then
    echo "FAIL: expected 2 receipts of duplicate content, got $COUNT" >&2
    echo "  (Phase 2 receiver does not dedup; both dispatches must land.)" >&2
    echo "--- bob watch dump ---" >&2
    cat "$BOB_WATCH_OUT" >&2
    echo "----------------------" >&2
    exit 1
fi

# Diagnostic dump of all received notices for human eyeballing
echo "--- bob received notices (--json line-delimited) ---"
sed 's/^/  bob | /' "$BOB_WATCH_OUT"
echo "----------------------------------------------------"

echo "PASS: clipboard sync e2e verified (3 verdicts: text / stdin / repeat-no-dedup)"
