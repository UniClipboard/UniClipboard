#!/usr/bin/env bash
#
# Single-machine LAN-only pairing smoke test — exercises the new mDNS
# discovery channel introduced in Phase 3 without any rendezvous
# round-trips.
#
# Forks two `uniclipboard-cli` processes under separate `--profile`
# names with `settings.network.allow_relay_fallback = false` (a.k.a.
# LAN-only Mode). Under that flag:
#   * sponsor adapter's `issue_invitation` skips the rendezvous POST
#     entirely (no metadata leak), mints the invitation code locally,
#     and starts only the window-scoped mDNS publisher.
#   * joiner adapter's `resolve_invitation` short-circuits past the
#     cloud branch and awaits the LAN browse exclusively.
#
# A successful run proves the LAN code path end-to-end on a single host
# without network egress, which is the canonical "first pair, no WAN"
# regression case for Phase 3 / Phase 4 changes.
#
# Reruns are idempotent: both profiles' data dirs are wiped on entry.
#
# Requirements:
#   - macOS (profile data dirs live under ~/Library/Application Support)
#   - --dev mode to avoid Keychain collisions between the two profiles
#   - python3 (used to patch settings.json)
#   - Built CLI binary; default at target/debug/uniclipboard-cli
#
# Notes:
#   - Multicast on macOS loopback is restricted; swarm-discovery uses
#     real interfaces, so this test will fail if every physical NIC is
#     down. Make sure Wi-Fi or Ethernet is up.

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

if ! command -v python3 >/dev/null 2>&1; then
    echo "ERROR: python3 is required to patch settings.json" >&2
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
    [[ -n "${BOB_OUT:-}" ]] && rm -f "$BOB_OUT"
}
trap cleanup EXIT

# Force LAN-only mode by patching settings.json after `init` has
# written the baseline file. `#[serde(default)]` on every nested
# struct means a partial patch round-trips cleanly.
patch_lan_only() {
    local settings_file=$1
    if [[ ! -f "$settings_file" ]]; then
        echo "ERROR: settings file not found at $settings_file" >&2
        exit 1
    fi
    python3 - "$settings_file" <<'PY'
import json, sys, pathlib
path = pathlib.Path(sys.argv[1])
with path.open("r") as f:
    settings = json.load(f)
network = settings.setdefault("network", {})
network["allow_relay_fallback"] = False
with path.open("w") as f:
    json.dump(settings, f, indent=2)
PY
}

# Assert a log substring is present in a captured output file.
# Used for both "must contain" (mDNS markers) and side checks.
assert_log_contains() {
    local file=$1
    local needle=$2
    local who=$3
    if ! grep -F -q "$needle" "$file"; then
        echo "FAIL: expected $who output to contain: $needle" >&2
        echo "--- $who tail ---" >&2
        tail -40 "$file" | sed "s/^/  $who | /" >&2
        echo "------------------" >&2
        exit 1
    fi
}

# Assert a log substring is NOT present — used to prove the cloud
# channel really stayed silent.
assert_log_lacks() {
    local file=$1
    local needle=$2
    local who=$3
    if grep -F -q "$needle" "$file"; then
        echo "FAIL: $who output unexpectedly contained: $needle" >&2
        echo "    (this means a cloud-channel call slipped through" >&2
        echo "     despite LAN-only Mode — adapter regression?)" >&2
        grep -F -n "$needle" "$file" | head -5 | sed "s/^/  $who | /" >&2
        exit 1
    fi
}

echo "==> Wiping previous profile state"
/bin/rm -rf "$ALICE_DIR" "$BOB_DIR"

echo "==> alice: init (writes baseline settings.json)"
"$CLI" $COMMON_FLAGS --profile alice init \
    --passphrase "$PASSPHRASE" \
    --device-name "alice (mdns-e2e)"

echo "==> alice: patch settings.json → allow_relay_fallback = false"
patch_lan_only "$ALICE_DIR/settings.json"

echo "==> bob: init (writes baseline settings.json)"
# `init` for bob also writes a fresh settings.json that bob's later
# `join` will read. Without this step bob would still try the cloud
# branch on dial.
# BOB_DIR was wiped above, so `init` should always succeed here — do
# not mask failures with `|| true` or by discarding output. Let `set -e`
# abort with the real error if init fails, so the cause is visible.
"$CLI" $COMMON_FLAGS --profile bob init \
    --passphrase "$PASSPHRASE" \
    --device-name "bob (mdns-e2e)"
if [[ ! -f "$BOB_DIR/settings.json" ]]; then
    echo "ERROR: bob settings.json missing after init" >&2
    exit 1
fi

# Bob can't run `join` on a profile that already initialized a space
# (it would refuse with "already paired"). Clear the vault + DB but
# keep settings.json (so the LAN-only patch survives).
echo "==> bob: reset vault but keep settings.json"
find "$BOB_DIR" -mindepth 1 -maxdepth 1 ! -name 'settings.json' -exec /bin/rm -rf {} +

echo "==> bob: patch settings.json → allow_relay_fallback = false"
patch_lan_only "$BOB_DIR/settings.json"

echo "==> alice: invite (background, captures stderr+stdout)"
ALICE_OUT="$(mktemp -t uc_alice_invite_mdns.XXXXXX)"
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

# Sanity check: invitation codes generated locally are 8 chars from the
# Crockford alphabet, joined by a hyphen — `XXXX-XXXX`. If we got a
# different shape, settings.json wasn't applied (server still minted).
if [[ ! "$CODE" =~ ^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$ ]]; then
    echo "FAIL: code '$CODE' does not match local-mint format" >&2
    echo "    (expected Crockford XXXX-XXXX; this indicates settings.json" >&2
    echo "     wasn't applied and a cloud-minted code leaked through —" >&2
    echo "     check alice log for 'cloud channel issued')" >&2
    exit 1
fi

echo "==> bob: join --code $CODE (LAN-only)"
BOB_OUT="$(mktemp -t uc_bob_join_mdns.XXXXXX)"
set +e
"$CLI" $COMMON_FLAGS --profile bob join \
    --code "$CODE" \
    --passphrase "$PASSPHRASE" > "$BOB_OUT" 2>&1
BOB_EXIT=$?
set -e
echo "    bob exited with $BOB_EXIT"

echo "==> Waiting for alice invite to settle"
set +e
wait "$ALICE_PID"
ALICE_EXIT=$?
set -e
echo "    alice exited with $ALICE_EXIT"

# ── Log-based assertions ────────────────────────────────────────────────
#
# These prove the new code paths actually ran. Tail of each side is
# printed below so a maintainer can diagnose failures without digging
# for tmp files.

echo "--- alice tail ---"
tail -30 "$ALICE_OUT" | sed 's/^/  alice | /'
echo "--- bob tail ---"
tail -30 "$BOB_OUT" | sed 's/^/  bob | /'
echo "-----------------"

# Sponsor side must have taken the LAN-only fast path and started an
# mDNS publisher. Either log line is acceptable depending on which
# branch the adapter hit — both indicate `runtime_consts::lan_only()`
# returned true.
LAN_ONLY_LOG="LAN-only mode: minted invitation locally"
MDNS_PUB_LOG="mDNS pairing announce live"
assert_log_contains "$ALICE_OUT" "$LAN_ONLY_LOG" "alice"
assert_log_contains "$ALICE_OUT" "$MDNS_PUB_LOG" "alice"

# Sponsor side must NOT have called the cloud `create_pairing`. If
# this triggers, `runtime_consts::lan_only()` is returning false even
# though settings.json says otherwise — likely a regression in the
# bootstrap → install_lan_only path.
assert_log_lacks "$ALICE_OUT" "cloud channel issued invitation" "alice"

# Joiner side must have taken the LAN-only branch in `resolve_invitation`
# and matched on the mDNS resolver.
assert_log_contains "$BOB_OUT" "LAN-only mode: skipping cloud channel" "bob"
assert_log_contains "$BOB_OUT" "mDNS pairing resolver matched" "bob"

if [[ $BOB_EXIT -ne 0 ]]; then
    echo "FAIL: bob exit status $BOB_EXIT (LAN-only join failed)" >&2
    exit 1
fi
if [[ $ALICE_EXIT -ne 0 ]]; then
    echo "FAIL: alice exit status $ALICE_EXIT (LAN-only invite failed)" >&2
    exit 1
fi

echo "PASS: LAN-only pairing end-to-end verified — no rendezvous traffic, mDNS round-trip OK"
