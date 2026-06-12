#!/usr/bin/env bash
# B2 end-to-end orchestration: real daemon + iOS-simulator demo.
#
#   1. build uniclipd + uniclip (debug)
#   2. start a daemon under an isolated UC_PROFILE
#   3. uniclip init + mobile-sync setup -> connect URI (LAN listener on 42720)
#   4. run the simulator demo with that URI (simulator shares the host
#      network stack, so http://127.0.0.1:42720 reaches the daemon directly)
#   5. tear the daemon down and delete the profile's data dir
#
# Prereq: crates/uc-mobile/scripts/build-ios-xcframework.sh (bindings + sim lib).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

PROFILE="b2demo"
DATA_DIR="$HOME/Library/Application Support/app.uniclipboard.desktop-$PROFILE"

# The mobile LAN port is fixed (42720, not profile-scoped) — refuse to run
# against someone else's daemon.
if nc -z 127.0.0.1 42720 2>/dev/null; then
  echo "FAIL: port 42720 is already in use (another daemon's mobile LAN listener?)" >&2
  exit 1
fi

echo "==> [1/5] build uniclipd + uniclip"
cargo build -p uc-daemon -p uc-cli 2>&1 | tail -1

DAEMON_PID=""
cleanup() {
  if [ -n "$DAEMON_PID" ] && kill -0 "$DAEMON_PID" 2>/dev/null; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  rm -rf "$DATA_DIR"
}
trap cleanup EXIT

echo "==> [2/5] start daemon (UC_PROFILE=$PROFILE)"
rm -rf "$DATA_DIR"
DAEMON_LOG="$(mktemp -t uc-b2demo-daemon)"
UC_PROFILE="$PROFILE" RUST_LOG=warn target/debug/uniclipd >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!
echo "    daemon pid $DAEMON_PID, log $DAEMON_LOG"

uniclip() { UC_PROFILE="$PROFILE" target/debug/uniclip "$@"; }

echo "==> [3/5] init space + register mobile device"
for i in $(seq 1 30); do
  if uniclip init --passphrase b2-demo-passphrase --device-name b2-demo >/dev/null 2>&1; then
    break
  fi
  if [ "$i" = 30 ]; then echo "FAIL: daemon never became ready for init" >&2; exit 1; fi
  sleep 1
done

SETUP_JSON="$(uniclip --json mobile-sync setup --non-interactive \
  --label B2Demo --ip 127.0.0.1 --accept-network-risk)"
# The CLI JSON exposes base_url/username/password but not the connect URI
# itself; rebuild it per the spec (docs/architecture/mobile-sync-connect-uri.md
# §3.1 field order, base64url no-pad) so the demo exercises the real
# scan→parse entry path.
CONNECT_URI="$(printf '%s' "$SETUP_JSON" | python3 -c '
import base64, json, sys
d = json.load(sys.stdin)
payload = json.dumps(
    {"v": 1, "url": d["base_url"], "user": d["username"], "pwd": d["password"]},
    separators=(",", ":"),
)
p = base64.urlsafe_b64encode(payload.encode()).decode().rstrip("=")
print(f"uniclipboard://connect?v=1&svc=mobile-sync&p={p}")
')"
echo "    connect URI acquired ($(printf '%s' "$SETUP_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)["base_url"])'))"

# Listener can come up asynchronously after setup; wait for it.
for i in $(seq 1 20); do
  if nc -z 127.0.0.1 42720 2>/dev/null; then break; fi
  if [ "$i" = 20 ]; then echo "FAIL: mobile LAN listener never bound 42720" >&2; exit 1; fi
  sleep 0.5
done

echo "==> [4/5] run simulator demo against the live daemon"
./crates/uc-mobile/scripts/run-ios-demo.sh "$CONNECT_URI"

echo "==> [5/5] teardown"
# handled by the EXIT trap
echo "B2 ORCHESTRATION OK"
