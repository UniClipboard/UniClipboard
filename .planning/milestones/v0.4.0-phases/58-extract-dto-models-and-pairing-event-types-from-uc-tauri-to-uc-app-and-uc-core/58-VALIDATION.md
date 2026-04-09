---
phase: 58
slug: extract-dto-models-and-pairing-event-types-from-uc-tauri-to-uc-app-and-uc-core
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-25
---

# Phase 58 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                         |
| ---------------------- | ------------------------------------------------------------- |
| **Framework**          | cargo test (Rust)                                             |
| **Config file**        | src-tauri/Cargo.toml                                          |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-tauri -p uc-app -p uc-core` |
| **Full suite command** | `cd src-tauri && cargo test`                                  |
| **Estimated runtime**  | ~30 seconds                                                   |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-tauri -p uc-app -p uc-core`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                             | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | ------------------------------------------------------------- | ----------- | ---------- |
| 58-01-01 | 01   | 1    | D-03        | unit        | `cd src-tauri && cargo test -p uc-app list_entry_projections` | ✅          | ⬜ pending |
| 58-01-02 | 01   | 1    | D-03        | integration | `cd src-tauri && cargo test -p uc-tauri models_serialization` | ✅          | ⬜ pending |
| 58-02-01 | 02   | 1    | D-01        | unit        | `cd src-tauri && cargo test -p uc-tauri daemon_command`       | ✅          | ⬜ pending |
| 58-03-01 | 03   | 2    | D-02        | compilation | `cd src-tauri && cargo check -p uc-tauri`                     | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

_Existing infrastructure covers all phase requirements._

---

## Manual-Only Verifications

_All phase behaviors have automated verification._

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
