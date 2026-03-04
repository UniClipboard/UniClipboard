---
phase: 04
slug: optimize-blob-at-rest-storage-format-without-backward-compatibility
status: approved
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-04
---

# Phase 04 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                            |
| ---------------------- | ------------------------------------------------------------------------------------------------ |
| **Framework**          | Rust built-in `#[test]` + `#[tokio::test]`                                                       |
| **Config file**        | `src-tauri/Cargo.toml` (workspace)                                                               |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-core -- aad && cargo test -p uc-infra -- encrypted_blob_store` |
| **Full suite command** | `cd src-tauri && cargo test --workspace --no-fail-fast`                                          |
| **Estimated runtime**  | ~30 seconds                                                                                      |

---

## Sampling Rate

- **After every task commit:** Run quick command (aad + encrypted_blob_store tests)
- **After every plan wave:** Run full workspace test suite
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement               | Test Type          | Automated Command                                               | File Exists | Status   |
| -------- | ---- | ---- | ------------------------- | ------------------ | --------------------------------------------------------------- | ----------- | -------- |
| 04-01-01 | 01   | 1    | BLOB-01, BLOB-02, BLOB-03 | unit               | `cargo test -p uc-core -- aad --no-fail-fast`                   | ✅          | ✅ green |
| 04-01-02 | 01   | 1    | BLOB-03                   | unit               | `cargo check -p uc-infra`                                       | ✅          | ✅ green |
| 04-02-01 | 02   | 2    | BLOB-01, BLOB-02          | unit + integration | `cargo test -p uc-infra -- encrypted_blob_store --no-fail-fast` | ✅          | ✅ green |
| 04-02-02 | 02   | 2    | BLOB-04                   | integration        | `cargo test -p uc-tauri --test spool_cleanup_test`              | ✅          | ✅ green |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Test Coverage Detail

### BLOB-01: Binary format (UCBL header replacing JSON)

| Test                                     | File                      | What it verifies                                                 |
| ---------------------------------------- | ------------------------- | ---------------------------------------------------------------- |
| `test_serialize_parse_roundtrip`         | `encrypted_blob_store.rs` | serialize then parse recovers nonce and ciphertext               |
| `test_parse_rejects_truncated_data`      | `encrypted_blob_store.rs` | data < 29 bytes rejected with "truncated" error                  |
| `test_parse_rejects_wrong_magic`         | `encrypted_blob_store.rs` | wrong magic bytes rejected with "invalid" error                  |
| `test_parse_rejects_wrong_version`       | `encrypted_blob_store.rs` | unsupported version byte rejected                                |
| `test_serialize_produces_correct_header` | `encrypted_blob_store.rs` | header layout: magic(4B) + version(1B) + nonce(24B) + ciphertext |
| `test_encrypted_store_encrypts_on_put`   | `encrypted_blob_store.rs` | stored data starts with UCBL magic, not JSON                     |

### BLOB-02: zstd compression before encryption

| Test                                   | File                      | What it verifies                                                                         |
| -------------------------------------- | ------------------------- | ---------------------------------------------------------------------------------------- |
| `test_encrypted_store_encrypts_on_put` | `encrypted_blob_store.rs` | stored ciphertext is valid zstd-compressed plaintext                                     |
| `test_roundtrip_with_compression`      | `encrypted_blob_store.rs` | put(plaintext) -> get() returns identical plaintext through compress/decompress pipeline |
| `test_put_returns_compressed_size`     | `encrypted_blob_store.rs` | returns Some(N) where N > 0 and N >= BLOB_HEADER_SIZE                                    |

### BLOB-03: compressed_size tracked in DB

| Test                                           | File                                     | What it verifies                                  |
| ---------------------------------------------- | ---------------------------------------- | ------------------------------------------------- |
| `test_for_blob_v2_is_deterministic`            | `aad.rs`                                 | AAD v2 determinism                                |
| `test_for_blob_v2_includes_blob_id_and_prefix` | `aad.rs`                                 | "uc:blob:v2\|" prefix format                      |
| `test_for_blob_v2_differs_from_v1`             | `aad.rs`                                 | v1 and v2 produce different output                |
| `test_for_blob_v2_differs_by_blob_id`          | `aad.rs`                                 | different blob IDs produce different AAD          |
| (compilation)                                  | `schema.rs`, `blob.rs`, `blob_mapper.rs` | compressed_size field propagates through DB layer |

### BLOB-04: Old blobs wiped on upgrade

| Test                                                                  | File                    | What it verifies                         |
| --------------------------------------------------------------------- | ----------------------- | ---------------------------------------- |
| `blob_cleanup_purges_old_files_and_creates_sentinel_when_no_sentinel` | `spool_cleanup_test.rs` | all files deleted, sentinel created      |
| `blob_cleanup_is_idempotent_when_sentinel_already_exists`             | `spool_cleanup_test.rs` | V2 blobs preserved when sentinel present |
| `blob_cleanup_is_graceful_noop_when_blob_dir_absent`                  | `spool_cleanup_test.rs` | no error when blob dir missing           |

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. No new test framework or fixtures needed.

---

## Manual-Only Verifications

All phase behaviors have automated verification.

---

## Validation Sign-Off

- [x] All tasks have automated verify commands
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] All requirement IDs covered (BLOB-01 through BLOB-04)
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-03-04

---

## Validation Audit 2026-03-04

| Metric     | Count |
| ---------- | ----- |
| Gaps found | 1     |
| Resolved   | 1     |
| Escalated  | 0     |
