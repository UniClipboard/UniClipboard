---
phase: 21
slug: sync-flow-correlation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-11
---

# Phase 21 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                     |
| ---------------------- | ------------------------------------------------------------------------- |
| **Framework**          | Rust built-in `#[test]` + tokio::test                                     |
| **Config file**        | `src-tauri/Cargo.toml` (workspace test config)                            |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-observability && cargo test -p uc-core` |
| **Full suite command** | `cd src-tauri && cargo test`                                              |
| **Estimated runtime**  | ~30 seconds                                                               |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-observability && cargo test -p uc-core`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type | Automated Command                                       | File Exists     | Status     |
| -------- | ---- | ---- | ----------- | --------- | ------------------------------------------------------- | --------------- | ---------- |
| 21-01-01 | 01   | 1    | FLOW-05     | unit      | `cd src-tauri && cargo test -p uc-observability stages` | Extend existing | ⬜ pending |
| 21-01-02 | 01   | 1    | FLOW-05     | unit      | `cd src-tauri && cargo test -p uc-core protocol`        | Extend existing | ⬜ pending |
| 21-01-03 | 01   | 1    | FLOW-05     | manual    | Verify via log inspection during `bun tauri dev`        | N/A             | ⬜ pending |
| 21-01-04 | 01   | 1    | FLOW-05     | manual    | Verify via log inspection during `bun tauri dev`        | N/A             | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements.

---

## Manual-Only Verifications

| Behavior                            | Requirement | Why Manual                                              | Test Instructions                                                                                                                                             |
| ----------------------------------- | ----------- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Outbound spans carry stage fields   | FLOW-05     | Requires running app and triggering clipboard sync      | 1. Run `bun tauri dev` 2. Copy text to clipboard 3. Check terminal logs for `stage=outbound_prepare` and `stage=outbound_send` in outbound spans              |
| Inbound spans carry flow_id + stage | FLOW-05     | Requires running app with peer device sending clipboard | 1. Run `bun tauri dev` on two devices 2. Copy text on device A 3. Check device B terminal logs for `flow_id` and `stage=inbound_decode`/`stage=inbound_apply` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
