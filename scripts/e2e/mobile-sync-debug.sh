#!/usr/bin/env bash
#
# P5a.9: SyncClipboard 协议本地端到端 e2e（基于 `uniclip mobile debug`
# 4 子命令，无 iPhone / 无 LAN / 无 daemon）。
#
# 验证范围:
#   facade → use case → ApplyInbound → CaptureClipboardUseCase → DB
#   facade → LatestClipboardSnapshotPort → DB → 出站
#   * 文本 PUT/GET round-trip + content_hash dedup
#   * 图片两步 PUT (file → doc) + GET 字节相同
#   * mime 推断 + --mime override
#   * File 类型两步 PUT (P5a.3.5 staging port 落地) → file-list rep 写入
#   * NotFound / daemon-running 拒绝 / JSON 模式
#
# 不在范围:
#   * OS 系统剪贴板 write（CLI fallback 是 NoopInboundWrite，留 P5a.10 真机）
#   * 真 LAN HTTP 路由层（middleware / Basic Auth；走 webserver 集成测试）
#   * iPhone 兼容性（留 P5a.10）
#
# Requirements:
#   * macOS（profile data dir 走 `~/Library/Application Support`）
#   * `--dev` 模式（避开 keychain、用 file-based secure storage）
#   * uniclip binary 已 build：`cargo build -p uc-cli --bin uniclip`

set -euo pipefail

CLI="${CLI:-./target/debug/uniclip}"
PROFILE="${PROFILE:-p5a9-debug-e2e}"
PASSPHRASE="${PASSPHRASE:-p5a9-debug-passphrase}"
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
TMPDIR_RUN="$(mktemp -d -t uc_p5a9_e2e.XXXXXX)"

PASS_COUNT=0
FAIL_COUNT=0

cleanup() {
    # 防 daemon 残留：万一有失败步骤把 daemon 留在跑（不应该，但保险）
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

# 断言一段输出包含某个字面量子串
assert_contains() {
    local label="$1" needle="$2" haystack="$3"
    if echo "$haystack" | grep -qF "$needle"; then
        ok "$label contains: '$needle'"
    else
        fail "$label missing '$needle'"
        echo "$haystack" | sed 's/^/      out| /' >&2
    fi
}

assert_not_contains() {
    local label="$1" needle="$2" haystack="$3"
    if echo "$haystack" | grep -qF "$needle"; then
        fail "$label unexpectedly contains '$needle'"
        echo "$haystack" | sed 's/^/      out| /' >&2
    else
        ok "$label does not contain: '$needle'"
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

# 从 JSON 输出里抽 entry_id（适用于 put-text --json）
extract_entry_id() {
    echo "$1" | grep -E '"entry_id":' | head -1 | sed -E 's/.*"entry_id": *"([^"]*)".*/\1/'
}

extract_existing_entry_id() {
    echo "$1" | grep -E '"existing_entry_id":' | head -1 | \
        sed -E 's/.*"existing_entry_id": *"([^"]*)".*/\1/'
}

# 从 plain 输出里抽 dataName（用于 get-doc）。grep 没命中时返回空字符串而非
# 让 pipefail 把整个脚本带掉。
extract_data_name() {
    local out
    out="$(echo "$1" | grep -E '^[│ ]*dataName:' || true)"
    echo "$out" | head -1 | sed -E 's/^[│ ]*dataName: *//' | tr -d ' '
}

# ── Setup ────────────────────────────────────────────────────────────────

step "Setup: wiping profile $PROFILE"
rm -rf "$PROFILE_DIR"
note "tmpdir: $TMPDIR_RUN"

step "Setup: ensure no daemon running for profile"
"$CLI" "${COMMON[@]}" stop >/dev/null 2>&1 || true

step "Setup: init space (--dev, file-based secure storage)"
"$CLI" "${COMMON[@]}" init --passphrase "$PASSPHRASE" --device-name "p5a9-e2e" >/dev/null
ok "init succeeded"

# ── Step 1: 文本 round-trip ──────────────────────────────────────────────

TEXT_A="hello P5a.9 e2e $(date +%s)"  # 唯一文本，避免与历史 entries 冲突

step "Step 1.1 — put-text first time (expect: applied + entry_id)"
OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-text "$TEXT_A")"
assert_contains "put-text outcome" '"outcome": "applied"' "$OUT"
ENTRY_ID_A="$(extract_entry_id "$OUT")"
if [[ -n "$ENTRY_ID_A" ]]; then
    ok "captured entry_id: $ENTRY_ID_A"
else
    fail "could not extract entry_id from JSON output"
    echo "$OUT" | sed 's/^/      out| /' >&2
fi

step "Step 1.2 — get-doc reflects TEXT_A"
OUT="$("$CLI" "${COMMON[@]}" mobile debug get-doc 2>&1)"
assert_contains "get-doc type" "type: Text" "$OUT"
assert_contains "get-doc text" "$TEXT_A" "$OUT"
assert_contains "get-doc has_data" "hasData: false" "$OUT"
assert_contains "get-doc hash" "hash:" "$OUT"

# ── Step 2: dedup (content_hash 命中) ────────────────────────────────────

step "Step 2 — put-text same TEXT_A (expect: duplicate_skipped + existing == entry_id_a)"
OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-text "$TEXT_A")"
assert_contains "second put-text outcome" '"outcome": "duplicate_skipped"' "$OUT"
EXISTING_ID="$(extract_existing_entry_id "$OUT")"
if [[ "$EXISTING_ID" == "$ENTRY_ID_A" ]]; then
    ok "existing_entry_id == ENTRY_ID_A ($EXISTING_ID)"
else
    fail "existing_entry_id ($EXISTING_ID) != ENTRY_ID_A ($ENTRY_ID_A)"
fi

# ── Step 3: 新文本替换 ───────────────────────────────────────────────────

TEXT_B="different content $(date +%s%N)"

step "Step 3.1 — put-text TEXT_B (expect: applied + new entry_id)"
OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-text "$TEXT_B")"
assert_contains "TEXT_B outcome" '"outcome": "applied"' "$OUT"
ENTRY_ID_B="$(extract_entry_id "$OUT")"
if [[ -n "$ENTRY_ID_B" && "$ENTRY_ID_B" != "$ENTRY_ID_A" ]]; then
    ok "new entry_id: $ENTRY_ID_B (≠ ENTRY_ID_A)"
else
    fail "ENTRY_ID_B ($ENTRY_ID_B) should differ from ENTRY_ID_A ($ENTRY_ID_A)"
fi

step "Step 3.2 — get-doc reflects TEXT_B (latest wins)"
OUT="$("$CLI" "${COMMON[@]}" mobile debug get-doc 2>&1)"
assert_contains "get-doc shows TEXT_B" "$TEXT_B" "$OUT"
assert_not_contains "get-doc no longer shows TEXT_A" "$TEXT_A" "$OUT"

# ── Step 4: 图片两步 PUT + GET 字节相同 ──────────────────────────────────

PNG_PATH="$TMPDIR_RUN/p5a9.png"
# 写一段 fake png 字节（不是真 PNG，不是 v1 关心的 —— mime 只看 ext）
printf 'fake png bytes for P5a.9 e2e %s' "$(date +%s%N)" > "$PNG_PATH"
PNG_SIZE=$(wc -c < "$PNG_PATH" | tr -d ' ')

step "Step 4.1 — put-file $PNG_PATH (expect: step1 buffered + step2 applied)"
OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-file "$PNG_PATH")"
# 两步 outcome 在嵌套 JSON 里:{"file":{"outcome":"buffered",...},"doc":{"outcome":"applied",...}}
assert_contains "step1 outcome" '"outcome": "buffered"' "$OUT"
assert_contains "step2 outcome" '"outcome": "applied"' "$OUT"

step "Step 4.2 — get-doc reflects Image meta + has_data=true + size=$PNG_SIZE"
OUT="$("$CLI" "${COMMON[@]}" mobile debug get-doc 2>&1)"
assert_contains "type Image" "type: Image" "$OUT"
assert_contains "has_data true" "hasData: true" "$OUT"
assert_contains "size matches PNG_SIZE" "size: $PNG_SIZE" "$OUT"

# 抽 daemon 派生的 dataName（与上传时的 file 名不一致 —— `clipboard_<entry前8>.png`）
DATANAME="$(extract_data_name "$OUT")"
if [[ -n "$DATANAME" ]]; then
    ok "extracted dataName: $DATANAME"
else
    fail "could not extract dataName from get-doc output"
    echo "$OUT" | sed 's/^/      out| /' >&2
fi

step "Step 4.3 — get-file --output → diff bytes identical"
OUT_PATH="$TMPDIR_RUN/p5a9-roundtrip.png"
"$CLI" "${COMMON[@]}" mobile debug get-file "$DATANAME" --output "$OUT_PATH" >/dev/null
if [[ -f "$OUT_PATH" ]]; then
    if diff -q "$PNG_PATH" "$OUT_PATH" >/dev/null; then
        ok "round-tripped bytes byte-identical ($PNG_SIZE bytes)"
    else
        fail "bytes differ between $PNG_PATH and $OUT_PATH"
    fi
else
    fail "get-file did not produce $OUT_PATH"
fi

# ── Step 5: --mime override 改变 mime 推断 ───────────────────────────────

WEBP_PATH="$TMPDIR_RUN/p5a9.txt"  # 故意用 .txt ext 但 override 成 image/webp
printf 'fake webp via override %s' "$(date +%s%N)" > "$WEBP_PATH"

step "Step 5 — put-file with --mime image/webp on .txt extension (override wins)"
OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-file "$WEBP_PATH" --mime image/webp)"
# 顶层 JSON 结构没有 mime 字段,但 step1 / step2 都应该 buffered + applied
assert_contains "override step1 outcome" '"outcome": "buffered"' "$OUT"
assert_contains "override step2 outcome" '"outcome": "applied"' "$OUT"

step "Step 5.1 — get-doc 显示 type=Image 而不是 File"
OUT="$("$CLI" "${COMMON[@]}" mobile debug get-doc 2>&1)"
assert_contains "override resulted in Image type" "type: Image" "$OUT"

# ── Step 6: NotFound — get-file 不存在的 dataName ────────────────────────

step "Step 6 — get-file <unknown> exits non-zero with NotFound message"
set +e
OUT="$("$CLI" "${COMMON[@]}" mobile debug get-file "no-such-file-xyzzy.bin" 2>&1)"
CODE=$?
set -e
assert_exit_nonzero "get-file unknown" "$CODE"
assert_contains "NotFound message" "404" "$OUT"

# ── Step 7: daemon-running 拒绝 ──────────────────────────────────────────

step "Step 7 — start daemon then debug command must refuse"
"$CLI" "${COMMON[@]}" start >/dev/null 2>&1
sleep 2
set +e
OUT="$("$CLI" "${COMMON[@]}" mobile debug put-text "should be refused" 2>&1)"
CODE=$?
set -e
assert_exit_nonzero "debug put-text while daemon running" "$CODE"
assert_contains "refusal mentions daemon" "daemon" "$OUT"
"$CLI" "${COMMON[@]}" stop >/dev/null 2>&1 || true

# ── Step 8: File 类型 happy path (P5a.3.5 staging port 落地) ──────────────

# 用 .bin ext → 推断 mime application/octet-stream → 走 File 分支
DOC_PATH="$TMPDIR_RUN/p5a9.bin"
DOC_BYTES="fake binary file $(date +%s%N)"  # 唯一字节避开 dedup
printf '%s' "$DOC_BYTES" > "$DOC_PATH"

step "Step 8 — put-file .bin (item_type=File) → 两步均 applied,file-list rep 落库"
OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-file "$DOC_PATH")"
assert_contains "step1 buffered"        '"outcome": "buffered"' "$OUT"
assert_contains "step2 applied"         '"outcome": "applied"'  "$OUT"

# get-doc 应看到 type=File + dataName 指向 staging 文件
DOC="$("$CLI" "${COMMON[@]}" mobile debug get-doc 2>&1)"
assert_contains "get-doc reports File" "type: File" "$DOC"
DATANAME_FILE="$(extract_data_name "$DOC")"
if [[ -n "$DATANAME_FILE" ]]; then
    ok "get-doc dataName present: $DATANAME_FILE"
else
    fail "get-doc dataName missing for File type"
    echo "$DOC" | sed 's/^/      out| /' >&2
fi

# P5a.3.5 后:get-file 经 staging.read_by_uri 直接返真文件字节(不是 URI list)
OUT_FILE="$TMPDIR_RUN/p5a9-file-out.bin"
"$CLI" "${COMMON[@]}" mobile debug get-file "$DATANAME_FILE" --output "$OUT_FILE" >/dev/null 2>&1 \
    || fail "get-file for File-type dataName failed"
if [[ -f "$OUT_FILE" ]]; then
    ok "get-file --output wrote real bytes"
    # 字节级比对原 payload —— 这是 P5a.3.5 出站真字节回读的关键断言
    if [[ "$(cat "$OUT_FILE")" == "$DOC_BYTES" ]]; then
        ok "get-file bytes match original payload (真字节 round-trip)"
    else
        fail "get-file bytes != original (got $(wc -c < "$OUT_FILE") bytes, want ${#DOC_BYTES})"
    fi
else
    fail "get-file --output did not write a file"
fi

# ── Step 9: JSON 模式覆盖 4 子命令 ───────────────────────────────────────

step "Step 9 — JSON output is parseable for all 4 debug subcommands"
# 用唯一文本 + 唯一图片绕开 dedup, 让 latest entry 在 get-file 步骤一定是 Image。
TEXT_J="json-mode-check $(date +%s%N)"
PNG_J_PATH="$TMPDIR_RUN/p5a9-json.png"
printf 'json-mode unique png %s' "$(date +%s%N)" > "$PNG_J_PATH"

OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-text "$TEXT_J")"
assert_contains "put-text JSON has outcome" '"outcome":' "$OUT"

OUT="$("$CLI" "${COMMON[@]}" --json mobile debug get-doc)"
assert_contains "get-doc JSON has item_type" '"item_type":' "$OUT"
assert_contains "get-doc JSON has hash"      '"hash":'      "$OUT"

# 这次 put-file 用一个**新**字节的 PNG，保证 latest 必是 Image（不是 Text/duplicate）
OUT="$("$CLI" "${COMMON[@]}" --json mobile debug put-file "$PNG_J_PATH")"
assert_contains "put-file JSON has file"     '"file":'                 "$OUT"
assert_contains "put-file JSON has doc"      '"doc":'                  "$OUT"
assert_contains "put-file step2 applied"     '"outcome": "applied"'    "$OUT"

# get-file 需要一个真存在的 dataName —— 复用上一步 put-file 后的 latest
LATEST="$("$CLI" "${COMMON[@]}" mobile debug get-doc 2>&1)"
DATANAME_J="$(extract_data_name "$LATEST")"
if [[ -n "$DATANAME_J" ]]; then
    OUT="$("$CLI" "${COMMON[@]}" --json mobile debug get-file "$DATANAME_J" \
        --output "$TMPDIR_RUN/json-out.bin")"
    assert_contains "get-file JSON has data_name" '"data_name":'   "$OUT"
    assert_contains "get-file JSON has mime"      '"mime":'        "$OUT"
    assert_contains "get-file JSON has size"      '"size":'        "$OUT"
else
    fail "could not derive dataName for JSON-mode get-file step (latest get-doc had no dataName line)"
    echo "$LATEST" | sed 's/^/      latest| /' >&2
fi

# ── Final ────────────────────────────────────────────────────────────────

if [[ ${FAIL_COUNT} -eq 0 ]]; then
    echo
    echo "============================================="
    echo "PASS: all $PASS_COUNT assertions OK"
    echo "  P5a.9 SyncClipboard 协议本地端到端 e2e 验证通过"
    echo "  (CLI fallback apply_inbound 真 capture + DB round-trip)"
    echo "============================================="
fi
