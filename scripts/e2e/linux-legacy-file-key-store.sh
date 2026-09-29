#!/usr/bin/env bash
# Linux E2E for profiles whose KEK lives in the pre-1.0 file key store.
#
# Restores the tracked v0.19.3 development profile and starts the daemon built
# from this checkout against private session buses. Nothing touches the host's
# session bus, keyring, or user data: every XDG directory points into an
# isolated work root, and only processes started here are stopped on exit.
#
# Usage (from the repository root, on non-WSL Linux):
#   scripts/e2e/linux-legacy-file-key-store.sh [--skip-build]
#
# Environment:
#   UC_E2E_REQUIRE_SECRET_SERVICE=1  fail instead of reporting the cases that
#                                    need an isolated Secret Service as not run
#   UC_E2E_EXPECTED_RESULTS=<tsv>    compare results with expected outcomes
#                                    (test, passed|failed, reason); used to
#                                    confirm that a baseline fails as designed
#   UC_E2E_SOURCE_NOTE=<text>        provenance recorded in environment.txt
#
# Outputs go to ${ARTIFACTS_DIR:-${UC_E2E_ARTIFACT_DIR:-target/legacy-file-key-store}}:
# test output, redacted daemon logs, environment facts and SHA256SUMS. The
# isolated work root is kept for inspection.
set -uo pipefail

if [[ ! -f Cargo.toml || ! -d tests/e2e ]]; then
  echo "run from the repository root" >&2
  exit 2
fi
if [[ "$(uname -s)" != "Linux" ]]; then
  echo "requires Linux" >&2
  exit 2
fi
# Same markers as the daemon's own WSL detection (uc-platform capability.rs).
if grep -qE 'Microsoft|WSL' /proc/version 2>/dev/null || [[ -n "${WSL_DISTRO_NAME:-}${WSL_INTEROP:-}" ]]; then
  # WSL always selects the file key store, so the Secret Service path is never exercised.
  echo "WSL does not exercise the system keyring path; use a native Linux host" >&2
  exit 2
fi
if ! command -v dbus-daemon >/dev/null; then
  echo "dbus-daemon is required" >&2
  exit 2
fi

SKIP_BUILD=0
[[ "${1:-}" == "--skip-build" ]] && SKIP_BUILD=1

REPO_ROOT="$(pwd)"
ARTIFACTS="${ARTIFACTS_DIR:-${UC_E2E_ARTIFACT_DIR:-$REPO_ROOT/target/legacy-file-key-store}}"
mkdir -p "$ARTIFACTS"
ARTIFACTS="$(cd "$ARTIFACTS" && pwd)"
WORK_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/uc-legacy-file-key-store.XXXXXX")"
chmod 700 "$WORK_ROOT"

PIDS=()
cleanup() {
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT

# Build with the host's real toolchain configuration before isolating XDG paths.
if [[ "$SKIP_BUILD" -eq 0 ]]; then
  cargo build -p uc-daemon -p uc-cli 2>&1 | tail -n 5 | tee "$ARTIFACTS/build.log"
  [[ "${PIPESTATUS[0]}" -eq 0 ]] || { echo "build failed" >&2; exit 1; }
fi

for directory in data cache state config runtime services; do
  mkdir -p "$WORK_ROOT/$directory"
done
chmod 700 "$WORK_ROOT/runtime"
export XDG_DATA_HOME="$WORK_ROOT/data"
export XDG_CACHE_HOME="$WORK_ROOT/cache"
export XDG_STATE_HOME="$WORK_ROOT/state"
export XDG_CONFIG_HOME="$WORK_ROOT/config"
export XDG_RUNTIME_DIR="$WORK_ROOT/runtime"
unset WAYLAND_DISPLAY
export DISPLAY=":99"
export UC_E2E_RUST_LOG="info"
# Keep restored profiles and their logs in the isolated root for collection.
export UC_E2E_KEEP_PROFILES=1

# A bus whose only service directory is empty: org.freedesktop.secrets is not activatable.
start_bus() {
  local name="$1"
  local config="$WORK_ROOT/$name.conf"
  cat > "$config" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$WORK_ROOT/runtime/$name.sock</listen>
  <servicedir>$WORK_ROOT/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
  local pid
  pid="$(dbus-daemon --config-file="$config" --fork --print-pid)" || return 1
  PIDS+=("$pid")
  echo "unix:path=$WORK_ROOT/runtime/$name.sock"
}

ABSENT_BUS="$(start_bus absent)" || { echo "failed to start the private bus" >&2; exit 1; }
export UC_E2E_ABSENT_SECRET_SERVICE_BUS="$ABSENT_BUS"
export DBUS_SESSION_BUS_ADDRESS="$ABSENT_BUS"

PRESENT_STATUS="not verified: gnome-keyring-daemon is not installed"
if command -v gnome-keyring-daemon >/dev/null; then
  PRESENT_BUS="$(start_bus present)" || { echo "failed to start the second bus" >&2; exit 1; }
  # An isolated login keyring under XDG_DATA_HOME, unlocked with a throwaway password.
  printf 'isolated-e2e-keyring' \
    | DBUS_SESSION_BUS_ADDRESS="$PRESENT_BUS" gnome-keyring-daemon --unlock --components=secrets \
      --foreground >>"$ARTIFACTS/secret-service.log" 2>&1 &
  PIDS+=("$!")
  owned=false
  for _ in $(seq 1 50); do
    if dbus-send --bus="$PRESENT_BUS" --print-reply --dest=org.freedesktop.DBus \
      /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets \
      2>/dev/null | grep -q 'boolean true'; then
      owned=true
      break
    fi
    sleep 0.2
  done
  if [[ "$owned" == true ]]; then
    export UC_E2E_PRESENT_SECRET_SERVICE_BUS="$PRESENT_BUS"
    PRESENT_STATUS="isolated gnome-keyring-daemon on a private bus"
  else
    PRESENT_STATUS="not verified: gnome-keyring-daemon did not start (see secret-service.log)"
  fi
fi

{
  echo "commit=$(git -C "$REPO_ROOT" rev-parse HEAD)"
  echo "worktree_diff_sha256=$(git -C "$REPO_ROOT" diff --binary HEAD | sha256sum | cut -d' ' -f1)"
  echo "kernel=$(uname -srm)"
  echo "distribution=$(. /etc/os-release 2>/dev/null && echo "${PRETTY_NAME:-unknown}")"
  echo "dbus_daemon=$(dbus-daemon --version | head -n 1)"
  echo "secret_service_present_case=$PRESENT_STATUS"
  echo "source_note=${UC_E2E_SOURCE_NOTE:-}"
} > "$ARTIFACTS/environment.txt"

if [[ "${UC_E2E_REQUIRE_SECRET_SERVICE:-}" == "1" && -z "${UC_E2E_PRESENT_SECRET_SERVICE_BUS:-}" ]]; then
  echo "an isolated Secret Service is required but unavailable: $PRESENT_STATUS" | tee "$ARTIFACTS/results.tsv" >&2
  exit 1
fi

TESTS=(
  legacy_file_key_store_starts_without_a_secret_service
  missing_legacy_key_store_fails_closed_without_an_empty_file_store
  a_wrong_legacy_kek_asks_for_the_passphrase_without_being_overwritten
  an_unreadable_legacy_key_store_fails_closed_and_stays_intact
  an_unrecognized_source_record_fails_closed
)
PRESENT_TESTS=(
  legacy_file_key_store_stays_authoritative_when_a_secret_service_appears
  entries_split_between_the_file_store_and_a_secret_service_fail_closed
  a_secret_service_holding_every_legacy_entry_keeps_authority
  a_stale_secret_service_kek_asks_for_the_passphrase_and_leaves_the_file_store_intact
  a_confirmed_system_source_is_not_replaced_when_the_secret_service_disappears
  a_confirmed_file_source_is_kept_when_the_secret_service_holds_a_different_key
  an_unconfirmed_run_records_nothing_and_changes_no_key
)
[[ -n "${UC_E2E_PRESENT_SECRET_SERVICE_BUS:-}" ]] && TESTS+=("${PRESENT_TESTS[@]}")

FAILED=0
: > "$ARTIFACTS/results.tsv"
# A harness that does not compile proves nothing about the product: stop here
# so no case can be counted as an expected failure.
if ! cargo test --manifest-path tests/e2e/Cargo.toml --test legacy_file_key_store --no-run \
  > "$ARTIFACTS/harness-build.log" 2>&1; then
  for test_name in "${TESTS[@]}"; do
    printf '%s\t%s\n' "$test_name" "error (harness did not compile)" | tee -a "$ARTIFACTS/results.tsv"
  done
  FAILED=1
else
  for test_name in "${TESTS[@]}"; do
    cargo test --manifest-path tests/e2e/Cargo.toml --test legacy_file_key_store \
      -- --ignored --exact "$test_name" --test-threads=1 > "$ARTIFACTS/$test_name.log" 2>&1
    # passed/failed only when exactly this case ran; anything else is a harness error.
    if grep -q 'test result: ok. 1 passed' "$ARTIFACTS/$test_name.log"; then
      result=passed
    elif grep -q 'test result: FAILED. 0 passed; 1 failed' "$ARTIFACTS/$test_name.log"; then
      result=failed
      FAILED=1
    else
      result=error
      FAILED=1
    fi
    printf '%s\t%s\n' "$test_name" "$result" | tee -a "$ARTIFACTS/results.tsv"
  done
fi
if [[ -z "${UC_E2E_PRESENT_SECRET_SERVICE_BUS:-}" ]]; then
  for test_name in "${PRESENT_TESTS[@]}"; do
    printf '%s\t%s\n' "$test_name" "not run ($PRESENT_STATUS)" | tee -a "$ARTIFACTS/results.tsv"
  done
fi

if [[ -n "${UC_E2E_EXPECTED_RESULTS:-}" ]]; then
  # Exit status reflects whether every case matched its expected outcome.
  FAILED=0
  : > "$ARTIFACTS/expectation.tsv"
  while IFS=$'\t' read -r test_name expected reason; do
    [[ -z "$test_name" || "$test_name" == \#* ]] && continue
    actual="$(awk -F'\t' -v name="$test_name" '$1 == name { print $2 }' "$ARTIFACTS/results.tsv")"
    verdict=matched
    [[ "$actual" == "$expected" ]] || { verdict=mismatched; FAILED=1; }
    printf '%s\t%s\t%s\t%s\t%s\n' "$test_name" "$expected" "${actual:-missing}" "$verdict" "$reason" \
      | tee -a "$ARTIFACTS/expectation.tsv"
  done < "$UC_E2E_EXPECTED_RESULTS"
fi

# Redacted daemon logs: fixture content is synthetic, but host paths are not shared.
mkdir -p "$ARTIFACTS/logs"
find "$XDG_STATE_HOME" -type f -name 'uniclipboard-daemon.json.*' -print0 \
  | while IFS= read -r -d '' log; do
      profile="$(basename "$(dirname "$(dirname "$log")")")"
      sed -e "s#$WORK_ROOT#<isolated-root>#g" -e "s#$HOME#<home>#g" "$log" \
        > "$ARTIFACTS/logs/$profile-$(basename "$log")"
    done
for log in "$ARTIFACTS"/*.log; do
  sed -i -e "s#$WORK_ROOT#<isolated-root>#g" -e "s#$HOME#<home>#g" "$log"
done
echo "work_root=<isolated-root> (kept at the path printed below)" >> "$ARTIFACTS/environment.txt"
(cd "$ARTIFACTS" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)

echo "isolated work root kept at $WORK_ROOT"
exit "$FAILED"
