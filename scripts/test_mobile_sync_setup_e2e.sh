#!/usr/bin/env bash
#
# CLI Redesign Step 1-5 (2026-05-06): mobile-sync setup / status / devices
# 命令拓扑 + 自定义凭据校验的本地端到端 e2e。
#
# 验证范围:
#   * `setup` 一键向导 (--non-interactive --json) → register_device 出
#     install_url + 一次性 password
#   * `setup` 缺 flag 拒绝 (--label / --advertise / --accept-network-risk)
#   * `status` / `devices list` / `disable` 综合行为
#   * `devices add` 自动凭据 + 自定义 username + --password-stdin pipe
#   * username/password 校验 5 种拒绝形态:
#     UsernameTaken / InvalidShape (短/数字头/连字符) / PasswordTooShort
#   * `devices revoke <id>` 显式 + JSON 模式无 id 拒绝
#
# 不在范围:
#   * 真 LAN HTTP 路由层(走 webserver 集成测试)
#   * 真机 iPhone 安装流程(留给用户)
#   * SyncClipboard 协议本地链路(走 test_mobile_sync_debug_e2e.sh)
#
# Requirements:
#   * macOS(profile data dir 走 `~/Library/Application Support`)
#   * `--dev` 模式(避开 keychain、用 file-based secure storage)
#   * uniclip binary 已 build:`cargo build -p uc-cli --bin uniclip`

set -euo pipefail

CLI="${CLI:-./src-tauri/target/debug/uniclip}"
PROFILE="${PROFILE:-redesign-setup-e2e}"
PASSPHRASE="${PASSPHRASE:-redesign-setup-passphrase}"
COMMON=("--dev" "--profile" "$PROFILE")

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "ERROR: this script is macOS-only (paths under ~/Library/Application Support)." >&2
    exit 2
fi

if [[ ! -x "$CLI" ]]; then
    echo "ERROR: CLI binary not found at $CLI" >&2
    echo "Build first: cargo build -p uc-cli --bin uniclip" >&2
    exit 2
fi

PROFILE_DIR="$HOME/Library/Application Support/app.uniclipboard.desktop-$PROFILE"
TMPDIR_RUN="$(mktemp -d -t uc_setup_e2e.XXXXXX)"

PASS_COUNT=0
FAIL_COUNT=0

cleanup() {
    "$CLI" "${COMMON[@]}" stop >/dev/null 2>&1 || true
    rm -rf "$TMPDIR_RUN"
    if [[ ${FAIL_COUNT} -gt 0 ]]; then
        echo
        echo "============================================="
        echo "FAIL: $FAIL_COUNT assertion(s) failed (passed: $PASS_COUNT)"
        echo "============================================="
        exit 1
    fi
}
trap cleanup EXIT

step()  { echo; echo "==> $*"; }
ok()    { echo "    ✓ $*"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail()  { echo "    ✗ FAIL: $*" >&2; FAIL_COUNT=$((FAIL_COUNT + 1)); }
note()  { echo "    · $*"; }

assert_contains() {
    local label="$1" needle="$2" haystack="$3"
    # `-- "$needle"` 阻止 grep 把以 `--` 起头的字面量(如 `--label is
    # required`)误当成自身 flag 解析。
    if echo "$haystack" | grep -qF -- "$needle"; then
        ok "$label contains: '$needle'"
    else
        fail "$label missing '$needle'"
        echo "$haystack" | sed 's/^/      out| /' >&2
    fi
}

assert_exit_zero() {
    local label="$1" code="$2"
    if [[ "$code" -eq 0 ]]; then
        ok "$label exit code 0"
    else
        fail "$label exit code $code (expected 0)"
    fi
}

assert_exit_nonzero() {
    local label="$1" code="$2"
    if [[ "$code" -ne 0 ]]; then
        ok "$label exit code $code (expected non-zero)"
    else
        fail "$label exit code 0 (expected non-zero)"
    fi
}

# 从 JSON 输出抽 device_id 字段。grep 没命中返回空字符串(不让 pipefail 终断)。
extract_device_id() {
    echo "$1" | grep -E '"device_id":' | head -1 | \
        sed -E 's/.*"device_id": *"([^"]*)".*/\1/' || true
}

extract_username() {
    echo "$1" | grep -E '"username":' | head -1 | \
        sed -E 's/.*"username": *"([^"]*)".*/\1/' || true
}

extract_password() {
    echo "$1" | grep -E '"password":' | head -1 | \
        sed -E 's/.*"password": *"([^"]*)".*/\1/' || true
}

# Run a command, capture stdout+stderr together, also capture exit code.
# Sets $LAST_OUT / $LAST_RC.
run_capture() {
    set +e
    LAST_OUT="$("$@" 2>&1)"
    LAST_RC=$?
    set -e
}

# Same but feeds stdin (one line). Used for --password-stdin.
run_capture_stdin() {
    local stdin_payload="$1"
    shift
    set +e
    LAST_OUT="$(echo "$stdin_payload" | "$@" 2>&1)"
    LAST_RC=$?
    set -e
}

# ── Setup ────────────────────────────────────────────────────────────────

step "Setup: wiping profile $PROFILE"
rm -rf "$PROFILE_DIR"
note "tmpdir: $TMPDIR_RUN"

step "Setup: ensure no daemon running for profile"
"$CLI" "${COMMON[@]}" stop >/dev/null 2>&1 || true

step "Setup: init space (--dev, file-based secure storage)"
"$CLI" "${COMMON[@]}" init --passphrase "$PASSPHRASE" --device-name "redesign-e2e" >/dev/null
ok "init succeeded"

# ── Step 1: setup --non-interactive --json happy path ────────────────────

step "Step 1 — setup --non-interactive --json happy path"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync setup \
    --non-interactive \
    --label "iPhone-A" \
    --advertise "127.0.0.1" \
    --port 42720 \
    --accept-network-risk
assert_exit_zero "setup happy path" "$LAST_RC"
assert_contains "device_id present" '"device_id":' "$LAST_OUT"
assert_contains "username present" '"username":' "$LAST_OUT"
assert_contains "password present" '"password":' "$LAST_OUT"
assert_contains "install_url present" '"install_url":' "$LAST_OUT"
assert_contains "qr_code_ascii present" '"qr_code_ascii":' "$LAST_OUT"
assert_contains "advertise_ip echoed" '"advertise_ip": "127.0.0.1"' "$LAST_OUT"
assert_contains "port echoed" '"port": 42720' "$LAST_OUT"
DEVICE_A_ID="$(extract_device_id "$LAST_OUT")"
note "iPhone-A device_id: $DEVICE_A_ID"

# ── Step 2: setup 缺 flag 拒绝 ───────────────────────────────────────────

step "Step 2.1 — setup --non-interactive without --label rejected"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync setup \
    --non-interactive \
    --advertise "127.0.0.1" \
    --accept-network-risk
assert_exit_nonzero "missing --label" "$LAST_RC"
assert_contains "error mentions --label" "--label is required" "$LAST_OUT"

step "Step 2.2 — setup --non-interactive without --advertise rejected"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync setup \
    --non-interactive \
    --label "X" \
    --accept-network-risk
assert_exit_nonzero "missing --advertise" "$LAST_RC"
assert_contains "error mentions --advertise" "--advertise is required" "$LAST_OUT"

step "Step 2.3 — setup --json without --accept-network-risk rejected"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync setup \
    --label "X" \
    --advertise "127.0.0.1"
assert_exit_nonzero "missing --accept-network-risk" "$LAST_RC"
assert_contains "error mentions risk" "--accept-network-risk is required" "$LAST_OUT"

# ── Step 3: status comprehensive view ────────────────────────────────────

step "Step 3 — status --json reflects 1 device + LAN enabled"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync status
assert_exit_zero "status" "$LAST_RC"
assert_contains "status enabled=true" '"enabled": true' "$LAST_OUT"
assert_contains "status lan_listen_enabled=true" '"lan_listen_enabled": true' "$LAST_OUT"
assert_contains "status device_count=1" '"device_count": 1' "$LAST_OUT"
assert_contains "status devices array contains iPhone-A" '"label": "iPhone-A"' "$LAST_OUT"

# ── Step 4: devices list ─────────────────────────────────────────────────

step "Step 4 — devices list --json shows iPhone-A"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices list
assert_exit_zero "devices list" "$LAST_RC"
assert_contains "list shows iPhone-A" '"label": "iPhone-A"' "$LAST_OUT"

# ── Step 5: devices add (auto credentials) ───────────────────────────────

step "Step 5 — devices add --label B (auto credentials)"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices add --label "iPhone-B"
assert_exit_zero "devices add auto" "$LAST_RC"
DEVICE_B_USERNAME="$(extract_username "$LAST_OUT")"
note "iPhone-B auto username: $DEVICE_B_USERNAME"
# Auto-minted username starts with `mobile_` per project convention.
if [[ "$DEVICE_B_USERNAME" == mobile_* ]]; then
    ok "auto username has mobile_ prefix"
else
    fail "auto username '$DEVICE_B_USERNAME' missing mobile_ prefix"
fi

# ── Step 6: devices add custom username + --password-stdin ───────────────

step "Step 6 — devices add --label C --username alice_001 --password-stdin"
CUSTOM_PW="MyStrongPassword123"
run_capture_stdin "$CUSTOM_PW" \
    "$CLI" "${COMMON[@]}" --json mobile-sync devices add \
    --label "iPhone-C" \
    --username "alice_001" \
    --password-stdin
assert_exit_zero "devices add custom" "$LAST_RC"
assert_contains "custom username echoed" '"username": "alice_001"' "$LAST_OUT"
assert_contains "custom password echoed" "\"password\": \"$CUSTOM_PW\"" "$LAST_OUT"

# ── Step 7: 5 种校验拒绝 ─────────────────────────────────────────────────

step "Step 7.1 — devices add --username alice_001 rejected (taken)"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices add \
    --label "iPhone-Dup" \
    --username "alice_001"
assert_exit_nonzero "duplicate username" "$LAST_RC"
assert_contains "error mentions taken" "already taken" "$LAST_OUT"

step "Step 7.2 — devices add --username ali rejected (too short)"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices add \
    --label "iPhone-Short" \
    --username "ali"
assert_exit_nonzero "username too short" "$LAST_RC"
assert_contains "error mentions invalid username" "Invalid username" "$LAST_OUT"

step "Step 7.3 — devices add --username 1abc12 rejected (digit-leading)"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices add \
    --label "iPhone-DigitHead" \
    --username "1abc12"
assert_exit_nonzero "username digit-leading" "$LAST_RC"
assert_contains "error mentions invalid username" "Invalid username" "$LAST_OUT"

step "Step 7.4 — devices add --username has-hyphen rejected (invalid char)"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices add \
    --label "iPhone-Hyphen" \
    --username "alice-002"
assert_exit_nonzero "username with hyphen" "$LAST_RC"
assert_contains "error mentions invalid username" "Invalid username" "$LAST_OUT"

step "Step 7.5 — devices add --password-stdin too short (<8) rejected"
run_capture_stdin "abc" \
    "$CLI" "${COMMON[@]}" --json mobile-sync devices add \
    --label "iPhone-WeakPw" \
    --password-stdin
assert_exit_nonzero "password too short" "$LAST_RC"
assert_contains "error mentions password length" "Password is too short" "$LAST_OUT"

# ── Step 8: status now shows 3 devices (A, B, C) ────────────────────────

step "Step 8 — status --json now shows 3 devices"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync status
assert_exit_zero "status after adds" "$LAST_RC"
assert_contains "device_count=3" '"device_count": 3' "$LAST_OUT"

# ── Step 9: devices revoke <id> 显式 ─────────────────────────────────────

step "Step 9 — devices revoke <id> (iPhone-A explicit id)"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices revoke "$DEVICE_A_ID"
assert_exit_zero "revoke explicit" "$LAST_RC"
assert_contains "revoked=true" '"revoked": true' "$LAST_OUT"
assert_contains "revoke echoes device_id" "\"device_id\": \"$DEVICE_A_ID\"" "$LAST_OUT"

# ── Step 10: devices revoke (no id) --json 拒绝 ─────────────────────────

step "Step 10 — devices revoke (no id) --json rejected"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync devices revoke
assert_exit_nonzero "revoke no id JSON" "$LAST_RC"
assert_contains "error mentions device-id required" "device-id" "$LAST_OUT"

# ── Step 11: status now 2 devices ───────────────────────────────────────

step "Step 11 — status --json now shows 2 devices"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync status
assert_exit_zero "status after revoke" "$LAST_RC"
assert_contains "device_count=2 after revoke" '"device_count": 2' "$LAST_OUT"

# ── Step 12: disable -> both flags off ──────────────────────────────────

step "Step 12 — disable --json (master + LAN both off)"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync disable
assert_exit_zero "disable" "$LAST_RC"
assert_contains "disable enabled=false" '"enabled": false' "$LAST_OUT"
assert_contains "disable lan_listen_enabled=false" '"lan_listen_enabled": false' "$LAST_OUT"

step "Step 12.1 — status reflects disabled state, devices retained"
run_capture "$CLI" "${COMMON[@]}" --json mobile-sync status
assert_exit_zero "status after disable" "$LAST_RC"
assert_contains "status enabled=false" '"enabled": false' "$LAST_OUT"
assert_contains "status lan_listen_enabled=false" '"lan_listen_enabled": false' "$LAST_OUT"
assert_contains "status devices retained" '"device_count": 2' "$LAST_OUT"

# ── Final ────────────────────────────────────────────────────────────────

echo
echo "============================================="
echo "PASS: all $PASS_COUNT assertions OK"
echo "  CLI Redesign (Step 1-5/5) — setup wizard / devices add / 校验 / status"
echo "  全部命令拓扑 + 自定义凭据规则 + 综合视图本地 e2e 验证通过"
echo "============================================="
