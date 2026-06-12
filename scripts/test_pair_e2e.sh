#!/usr/bin/env bash
#
# Single-machine end-to-end pairing smoke test (Slice 1).
#
# Spawns two uniclipboard-cli processes under separate --profile names:
#   * alice: initialize space (A1) + issue invitation (B1) + wait for joiner
#   * bob:   redeem invitation (B2)
#
# Assertion:
#   bob exits 0 AND alice exits 0 (Success outcome fired).
#
# Requirements:
#   - macOS (profile data dirs live under ~/Library/Application Support)
#   - Network access to the production rendezvous service
#   - --dev mode to avoid Keychain collisions between the two profiles
#
# Rerun: the script wipes both profiles' data dirs on entry, so repeat
# runs are idempotent.

set -euo pipefail

CLI="${CLI:-./target/debug/uniclipboard-cli}"
PASSPHRASE="${PASSPHRASE:-hunter22hunter22}"
WAIT_SECS="${WAIT_SECS:-30}"
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
# NB: uc-platform's `DirsAppDirsAdapter` joins APP_DIR_NAME with the
# profile via a hyphen (`app.uniclipboard.desktop-<profile>`), so the
# script's cleanup must match that exact scheme — NOT the underscore
# form used by `apply_profile_suffix` elsewhere.
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
# Use /bin/rm explicitly: some users alias `rm` to `trash`, which errors
# out (non-zero) when the path doesn't exist and would kill `set -e`.
/bin/rm -rf "$ALICE_DIR" "$BOB_DIR"

echo "==> alice: init"
"$CLI" $COMMON_FLAGS --profile alice init \
    --passphrase "$PASSPHRASE" \
    --device-name "alice (e2e)"

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

echo "==> bob: join --code $CODE"
set +e
"$CLI" $COMMON_FLAGS --profile bob join \
    --code "$CODE" \
    --passphrase "$PASSPHRASE"
BOB_EXIT=$?
set -e
echo "    bob exited with $BOB_EXIT"

echo "==> Waiting for alice invite to settle"
set +e
wait "$ALICE_PID"
ALICE_EXIT=$?
set -e
echo "    alice exited with $ALICE_EXIT"

# Keep last few lines of alice's output visible so failures are self-
# diagnosing without the maintainer reaching for the tmpfile.
echo "--- alice tail ---"
tail -10 "$ALICE_OUT" | sed 's/^/  alice | /'
echo "------------------"

if [[ $BOB_EXIT -ne 0 ]]; then
    echo "FAIL: bob exit status $BOB_EXIT" >&2
    exit 1
fi
if [[ $ALICE_EXIT -ne 0 ]]; then
    echo "FAIL: alice exit status $ALICE_EXIT" >&2
    exit 1
fi

echo "PASS: single-machine pairing end-to-end verified"
