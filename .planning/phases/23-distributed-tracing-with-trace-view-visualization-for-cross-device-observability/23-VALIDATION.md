---
phase: 23
slug: distributed-tracing-with-trace-view-visualization-for-cross-device-observability
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-03-11
---

# Phase 23 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                  |
| ---------------------- | ------------------------------------------------------ |
| **Framework**          | Rust built-in test + cargo test                        |
| **Config file**        | src-tauri/Cargo.toml                                   |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-observability --lib` |
| **Full suite command** | `cd src-tauri && cargo test`                           |
| **Estimated runtime**  | ~30 seconds                                            |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-observability --lib`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement                 | Test Type | Automated Command                                                                                                                 | File Exists                                          | Status   |
| -------- | ---- | ---- | --------------------------- | --------- | --------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | -------- |
| 23-01-01 | 01   | 0    | device_id injection         | unit      | `cd src-tauri && cargo test -p uc-observability --lib seq::layer::tests::injects_device_id_when_layer_has_value`                  | `src-tauri/crates/uc-observability/src/seq/layer.rs` | ✅ green |
| 23-01-02 | 01   | 0    | device_id absent gracefully | unit      | `cd src-tauri && cargo test -p uc-observability --lib seq::layer::tests::missing_device_id_is_handled_gracefully`                 | `src-tauri/crates/uc-observability/src/seq/layer.rs` | ✅ green |
| 23-01-03 | 01   | 0    | CLEF device_id field name   | unit      | `cd src-tauri && cargo test -p uc-observability --lib seq::layer::tests::uses_clef_field_name_device_id`                          | `src-tauri/crates/uc-observability/src/seq/layer.rs` | ✅ green |
| 23-01-04 | 01   | 0    | build_seq_layer signature   | unit      | `cd src-tauri && cargo test -p uc-observability --lib`                                                                            | ✅ Existing                                          | ✅ green |
| 23-02-01 | 02   | 1    | origin_flow_id warning      | unit      | `cd src-tauri && cargo test -p uc-tauri --lib bootstrap::wiring::tests::clipboard_receive_loop_warns_when_origin_flow_id_missing` | `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`  | ✅ green |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky · ⚠ manual-only_

---

## Wave 0 Requirements

- [x] `uc-observability/src/seq/layer.rs` tests added for device_id injection, absent handling, and CLEF field name
- [x] Existing `seq/mod.rs` tests validate `build_seq_layer(..., None)` signature path
- [x] `uc-tauri` inbound receive-loop warning test covers `origin_flow_id: None` path

_Existing infrastructure covers test framework setup._

---

## Manual-Only Verifications

| Behavior                  | Requirement                    | Why Manual                    | Test Instructions                                                                    |
| ------------------------- | ------------------------------ | ----------------------------- | ------------------------------------------------------------------------------------ |
| Seq LAN accessibility     | docker-compose bind 0.0.0.0    | Requires Docker + network     | 1. `docker compose -f docker-compose.seq.yml up` 2. Access from second device on LAN |
| Signal expressions work   | Seq saved searches             | Requires running Seq instance | 1. Import signal JSON 2. Copy on Device A 3. Verify flow appears in Seq signal       |
| Cross-device flow linkage | origin_flow_id cross-reference | Requires two devices          | 1. Copy on A 2. Paste on B 3. Click origin_flow_id link in Seq                       |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** automated coverage complete (manual environment checks remain)

## Validation Audit 2026-03-11

| Metric     | Count |
| ---------- | ----- |
| Gaps found | 4     |
| Resolved   | 4     |
| Escalated  | 0     |
