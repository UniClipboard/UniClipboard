---
phase: 8
slug: optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 8 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                  |
| ---------------------- | ---------------------------------------------------------------------- |
| **Framework**          | cargo test (built-in)                                                  |
| **Config file**        | src-tauri/Cargo.toml                                                   |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-core -p uc-infra -p uc-app -- --lib` |
| **Full suite command** | `cd src-tauri && cargo test`                                           |
| **Estimated runtime**  | ~30 seconds                                                            |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-core -p uc-infra -p uc-app -- --lib`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                            | File Exists           | Status  |
| -------- | ---- | ---- | ----------- | ----------- | ------------------------------------------------------------ | --------------------- | ------- |
| 08-01-01 | 01   | 1    | V3-CODEC    | unit        | `cd src-tauri && cargo test -p uc-core payload_v3 -x`        | No - Wave 0           | pending |
| 08-01-02 | 01   | 1    | V3-WIRE     | unit        | `cd src-tauri && cargo test -p uc-infra chunked_transfer -x` | Partial (V2 exists)   | pending |
| 08-01-03 | 01   | 1    | V3-COMPRESS | unit        | `cd src-tauri && cargo test -p uc-infra chunked_transfer -x` | No - Wave 0           | pending |
| 08-01-04 | 01   | 1    | V3-LARGE    | unit        | `cd src-tauri && cargo test -p uc-infra chunked_transfer -x` | Partial (1MB V2 test) | pending |
| 08-02-01 | 02   | 2    | V3-ARC      | unit        | `cd src-tauri && cargo test -p uc-app sync_outbound -x`      | No - Wave 0           | pending |
| 08-02-02 | 02   | 2    | V3-OUTBOUND | integration | `cd src-tauri && cargo test -p uc-app sync_outbound -x`      | Partial (V2 e2e)      | pending |
| 08-02-03 | 02   | 2    | V3-INBOUND  | integration | `cd src-tauri && cargo test -p uc-app sync_inbound -x`       | Partial (V2 path)     | pending |
| 08-02-04 | 02   | 2    | V3-NOENC    | unit        | `cd src-tauri && cargo test -p uc-app sync_outbound -x`      | Yes (existing)        | pending |
| 08-02-05 | 02   | 2    | V3-NOLEAK   | smoke       | `cd src-tauri && cargo check 2>&1`                           | N/A                   | pending |
| 08-03-01 | 03   | 3    | V3-ARC      | unit        | `cd src-tauri && cargo test -p uc-app sync_outbound -x`      | No - Wave 0           | pending |

_Status: pending / green / red / flaky_

---

## Wave 0 Requirements

- [ ] `uc-core/src/network/protocol/clipboard_payload_v3.rs` — V3 binary codec module with round-trip tests
- [ ] V3 chunked transfer tests in `uc-infra/src/clipboard/chunked_transfer.rs` — compression on/off, large payload
- [ ] Updated test mocks in sync_outbound.rs/sync_inbound.rs for `Arc<[u8]>` port signature

_Existing infrastructure partially covers: V2 chunked transfer tests, sync_outbound/sync_inbound e2e tests, encryption not-ready regression test._

---

## Manual-Only Verifications

| Behavior                                        | Requirement              | Why Manual                                  | Test Instructions                                                                                                                           |
| ----------------------------------------------- | ------------------------ | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| End-to-end large image sync between two devices | V3-OUTBOUND + V3-INBOUND | Requires two running Tauri instances on LAN | 1. Start two devices, 2. Copy a 5MB+ image on device A, 3. Verify it appears on device B clipboard, 4. Check tracing spans show V3 protocol |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
