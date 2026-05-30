#!/usr/bin/env bash
#
# Single-machine end-to-end switch-space smoke test.
#
# Spawns three logical actions across two profiles:
#   * alice: initialize space A1 (passphrase A) + issue invitation (B1) +
#            wait for joiner.
#   * bob:   initialize its own space B1 (passphrase B) on a different
#            profile, accumulating local clipboard state.
#   * bob:   `switch-space` against alice's invitation — runs the 4-phase
#            re-encryption migration so bob abandons B1 and joins A1.
#
# Assertions:
#   * bob's `switch-space` exits 0.
#   * alice's invite exits 0 (sponsor saw a successful handshake).
#
# Requirements:
#   * macOS (profile data dirs live under ~/Library/Application Support).
#   * Network access to the production rendezvous service.
#   * --dev mode to avoid Keychain collisions between the two profiles.
#
# 与 `test_pair_e2e.sh` 共用 alice/bob 两个 profile 和清理流程；本脚本是
# switch-space 路径的对照版本（旧的是 join 路径）。

set -euo pipefail

CLI="${CLI:-./src-tauri/target/debug/uniclip}"
PASSPHRASE_ALICE="${PASSPHRASE_ALICE:-hunter22hunter22}"
PASSPHRASE_BOB="${PASSPHRASE_BOB:-bobsfirstpasspass}"
SEED_TEXT="${SEED_TEXT:-secret-clipboard-message-from-bob}"
WAIT_SECS="${WAIT_SECS:-30}"
COMMON_FLAGS="--dev"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "ERROR: this script is macOS-only (paths under ~/Library/Application Support)." >&2
    exit 2
fi

if [[ ! -x "$CLI" ]]; then
    echo "ERROR: CLI binary not found at $CLI" >&2
    echo "Build first: cargo build -p uc-cli --bin uniclip" >&2
    exit 2
fi

APP_ROOT="$HOME/Library/Application Support"
ALICE_DIR="$APP_ROOT/app.uniclipboard.desktop-alice"
BOB_DIR="$APP_ROOT/app.uniclipboard.desktop-bob"

cleanup() {
    if [[ -n "${ALICE_PID:-}" ]] && kill -0 "$ALICE_PID" 2>/dev/null; then
        kill -INT "$ALICE_PID" 2>/dev/null || true
        wait "$ALICE_PID" 2>/dev/null || true
    fi
    [[ -n "${ALICE_OUT:-}" ]] && rm -f "$ALICE_OUT"
}
trap cleanup EXIT

echo "==> Wiping previous profile state"
/bin/rm -rf "$ALICE_DIR" "$BOB_DIR"

echo "==> alice: init (passphrase A)"
"$CLI" $COMMON_FLAGS --profile alice init \
    --passphrase "$PASSPHRASE_ALICE" \
    --device-name "alice (e2e)"

echo "==> bob: init (passphrase B — bob's original space, will be migrated away)"
"$CLI" $COMMON_FLAGS --profile bob init \
    --passphrase "$PASSPHRASE_BOB" \
    --device-name "bob (e2e)"

echo "==> bob: dev seed-clipboard (encrypted under passphrase B's master key)"
"$CLI" $COMMON_FLAGS --profile bob dev seed-clipboard \
    --text "$SEED_TEXT"

echo "==> bob: dev dump-clipboard (sanity — should show seeded text decrypted under B's key)"
PRE_DUMP="$("$CLI" $COMMON_FLAGS --profile bob dev dump-clipboard --limit 5)"
echo "$PRE_DUMP" | sed 's/^/    bob_pre | /'
if ! echo "$PRE_DUMP" | grep -qF "$SEED_TEXT"; then
    echo "FAIL: bob's pre-switch dump did not contain the seeded text" >&2
    exit 1
fi

echo "==> alice: invite (background)"
ALICE_OUT="$(mktemp -t uc_alice_invite.XXXXXX)"
"$CLI" $COMMON_FLAGS --profile alice invite > "$ALICE_OUT" 2>&1 &
ALICE_PID=$!

echo "==> Waiting up to ${WAIT_SECS}s for INVITATION_CODE line from alice"
CODE=""
for ((i = 0; i < WAIT_SECS * 2; i++)); do
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

echo "==> bob: switch-space --code $CODE --new-passphrase <alice's>"
set +e
"$CLI" $COMMON_FLAGS --profile bob switch-space \
    --code "$CODE" \
    --new-passphrase "$PASSPHRASE_ALICE"
BOB_EXIT=$?
set -e
echo "    bob exited with $BOB_EXIT"

echo "==> Waiting for alice invite to settle"
set +e
wait "$ALICE_PID"
ALICE_EXIT=$?
set -e
echo "    alice exited with $ALICE_EXIT"

echo "--- alice tail ---"
tail -10 "$ALICE_OUT" | sed 's/^/  alice | /'
echo "------------------"

if [[ $BOB_EXIT -ne 0 ]]; then
    echo "FAIL: bob switch-space exit status $BOB_EXIT" >&2
    exit 1
fi
if [[ $ALICE_EXIT -ne 0 ]]; then
    echo "FAIL: alice exit status $ALICE_EXIT" >&2
    exit 1
fi

echo "==> bob: dev dump-clipboard (post-switch — must still show seeded text decrypted under A's master key)"
POST_DUMP="$("$CLI" $COMMON_FLAGS --profile bob dev dump-clipboard --limit 5)"
echo "$POST_DUMP" | sed 's/^/    bob_post | /'
if ! echo "$POST_DUMP" | grep -qF "$SEED_TEXT"; then
    echo "FAIL: bob's post-switch dump did not contain the seeded text — re-encryption broke data" >&2
    exit 1
fi

echo "PASS: single-machine switch-space end-to-end verified (data round-trip preserved)"
