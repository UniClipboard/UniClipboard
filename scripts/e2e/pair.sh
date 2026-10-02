#!/usr/bin/env bash
#
# Single-machine end-to-end pairing smoke test (Slice 1).
#
# Spawns two uniclip processes under separate --profile names. The two paths
# may point at different Engine versions for release interoperability checks:
#   * alice: initialize space (A1) + issue invitation (B1) + wait for joiner
#   * bob:   redeem invitation (B2)
#
# Assertions:
#   pairing succeeds, then an exact text payload is received in each direction.
#
# Requirements:
#   - macOS (profile data dirs live under ~/Library/Application Support)
#   - Network access to the production rendezvous service
#   - --dev mode to avoid Keychain collisions between the two profiles
#
# Rerun: the script wipes both profiles' data dirs on entry, so repeat
# runs are idempotent.

set -euo pipefail

ALICE_CLI="${ALICE_CLI:-${CLI:-./target/debug/uniclip}}"
BOB_CLI="${BOB_CLI:-${CLI:-./target/debug/uniclip}}"
PASSPHRASE="${PASSPHRASE:-hunter22hunter22}"
WAIT_SECS="${WAIT_SECS:-30}"
COMMON_FLAGS="--dev"
PROFILE_PREFIX="${PROFILE_PREFIX:-engine-interop}"

if [[ ! "$PROFILE_PREFIX" =~ ^[A-Za-z0-9-]+$ ]]; then
    echo "ERROR: PROFILE_PREFIX must contain only letters, digits, and hyphens." >&2
    exit 2
fi
ALICE_PROFILE="${PROFILE_PREFIX}-alice"
BOB_PROFILE="${PROFILE_PREFIX}-bob"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "ERROR: this script is macOS-only (paths under ~/Library/Application Support)." >&2
    exit 2
fi

for cli in "$ALICE_CLI" "$BOB_CLI"; do
    if [[ ! -x "$cli" ]]; then
        echo "ERROR: CLI binary not found at $cli" >&2
        echo "Build first: cargo build -p uc-cli --bin uniclip" >&2
        exit 2
    fi
done

APP_ROOT="$HOME/Library/Application Support"
# NB: uc-platform's `DirsAppDirsAdapter` joins APP_DIR_NAME with the
# profile via a hyphen (`app.uniclipboard.desktop-<profile>`), so the
# script's cleanup must match that exact scheme — NOT the underscore
# form used by `apply_profile_suffix` elsewhere.
ALICE_DIR="$APP_ROOT/app.uniclipboard.desktop-$ALICE_PROFILE"
BOB_DIR="$APP_ROOT/app.uniclipboard.desktop-$BOB_PROFILE"

cleanup() {
    if [[ -n "${ALICE_PID:-}" ]] && kill -0 "$ALICE_PID" 2>/dev/null; then
        kill -INT "$ALICE_PID" 2>/dev/null || true
        wait "$ALICE_PID" 2>/dev/null || true
    fi
    if [[ -n "${WATCH_PID:-}" ]] && kill -0 "$WATCH_PID" 2>/dev/null; then
        kill -INT "$WATCH_PID" 2>/dev/null || true
        wait "$WATCH_PID" 2>/dev/null || true
    fi
    "$ALICE_CLI" $COMMON_FLAGS --profile "$ALICE_PROFILE" stop >/dev/null 2>&1 || true
    "$BOB_CLI" $COMMON_FLAGS --profile "$BOB_PROFILE" stop >/dev/null 2>&1 || true
    [[ -n "${ALICE_OUT:-}" ]] && rm -f "$ALICE_OUT"
    [[ -n "${WATCH_OUT:-}" ]] && rm -f "$WATCH_OUT"
    [[ -n "${WATCH_ERR:-}" ]] && rm -f "$WATCH_ERR"
}
trap cleanup EXIT

echo "==> Wiping previous profile state"
# Use /bin/rm explicitly: some users alias `rm` to `trash`, which errors
# out (non-zero) when the path doesn't exist and would kill `set -e`.
/bin/rm -rf "$ALICE_DIR" "$BOB_DIR"

echo "==> alice: init"
"$ALICE_CLI" $COMMON_FLAGS --profile "$ALICE_PROFILE" init \
    --passphrase "$PASSPHRASE" \
    --device-name "alice (e2e)"

echo "==> alice: invite (background)"
ALICE_OUT="$(mktemp -t uc_alice_invite.XXXXXX)"
"$ALICE_CLI" $COMMON_FLAGS --profile "$ALICE_PROFILE" invite > "$ALICE_OUT" 2>&1 &
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
"$BOB_CLI" $COMMON_FLAGS --profile "$BOB_PROFILE" join \
    --code "$CODE" \
    --passphrase "$PASSPHRASE"
BOB_EXIT=$?
set -e
echo "    bob exited with $BOB_EXIT"

if [[ $BOB_EXIT -ne 0 ]] && kill -0 "$ALICE_PID" 2>/dev/null; then
    kill -TERM "$ALICE_PID" 2>/dev/null || true
fi

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

verify_transfer() {
    local sender_cli="$1"
    local receiver_cli="$2"
    local sender_profile="$3"
    local receiver_profile="$4"
    local payload="$5"

    WATCH_OUT="$(mktemp -t uc_interop_watch.XXXXXX)"
    WATCH_ERR="$(mktemp -t uc_interop_watch_err.XXXXXX)"
    "$receiver_cli" $COMMON_FLAGS --profile "$receiver_profile" --json watch \
        > "$WATCH_OUT" 2> "$WATCH_ERR" &
    WATCH_PID=$!

    local ready=""
    for ((i = 0; i < WAIT_SECS * 2; i++)); do
        if grep -Fq 'WATCH_READY' "$WATCH_ERR" 2>/dev/null; then
            ready="true"
            break
        fi
        if ! kill -0 "$WATCH_PID" 2>/dev/null; then
            echo "ERROR: receiver watch exited before becoming ready" >&2
            sed 's/^/  watch | /' "$WATCH_ERR" >&2
            return 1
        fi
        sleep 0.5
    done
    if [[ -z "$ready" ]]; then
        echo "ERROR: receiver watch did not become ready" >&2
        return 1
    fi

    "$sender_cli" $COMMON_FLAGS --profile "$sender_profile" --json send "$payload"
    local received=""
    for ((i = 0; i < WAIT_SECS * 2; i++)); do
        if grep -Fq "$payload" "$WATCH_OUT" 2>/dev/null; then
            received="true"
            break
        fi
        sleep 0.5
    done
    kill -INT "$WATCH_PID" 2>/dev/null || true
    wait "$WATCH_PID" 2>/dev/null || true
    WATCH_PID=""
    if [[ -z "$received" ]]; then
        echo "ERROR: exact interop payload was not received: $payload" >&2
        sed 's/^/  watch | /' "$WATCH_OUT" >&2
        return 1
    fi
}

verify_transfer "$ALICE_CLI" "$BOB_CLI" "$ALICE_PROFILE" "$BOB_PROFILE" \
    "old-to-new-$PROFILE_PREFIX"
verify_transfer "$BOB_CLI" "$ALICE_CLI" "$BOB_PROFILE" "$ALICE_PROFILE" \
    "new-to-old-$PROFILE_PREFIX"

echo "PASS: cross-version pairing and bidirectional transfer verified"
