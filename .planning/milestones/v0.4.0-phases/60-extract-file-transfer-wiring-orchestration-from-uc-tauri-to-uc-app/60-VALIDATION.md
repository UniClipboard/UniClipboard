---
phase: 60
slug: extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-25
---

# Phase 60 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                  |
| ---------------------- | -------------------------------------- |
| **Framework**          | cargo test (Rust)                      |
| **Config file**        | `src-tauri/Cargo.toml`                 |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-app` |
| **Full suite command** | `cd src-tauri && cargo test`           |
| **Estimated runtime**  | ~30 seconds                            |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-app`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type             | Automated Command                             | File Exists | Status     |
| -------- | ---- | ---- | ----------- | --------------------- | --------------------------------------------- | ----------- | ---------- |
| 60-01-01 | 01   | 1    | D-01/D-02   | compile + unit        | `cd src-tauri && cargo test -p uc-app`        | ❌ W0       | ⬜ pending |
| 60-01-02 | 01   | 1    | D-03/D-04   | compile               | `cd src-tauri && cargo check -p uc-app`       | ✅          | ⬜ pending |
| 60-01-03 | 01   | 2    | D-05/D-06   | compile               | `cd src-tauri && cargo check -p uc-bootstrap` | ✅          | ⬜ pending |
| 60-01-04 | 01   | 2    | D-07        | compile + integration | `cd src-tauri && cargo test`                  | ✅          | ⬜ pending |
| 60-01-05 | 01   | 2    | D-08/D-09   | compile               | `cd src-tauri && cargo check`                 | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- Existing infrastructure covers all phase requirements. The extraction is validated by compilation success and existing test suites passing.

---

## Manual-Only Verifications

| Behavior                                           | Requirement | Why Manual                  | Test Instructions                                                    |
| -------------------------------------------------- | ----------- | --------------------------- | -------------------------------------------------------------------- |
| File transfer progress/completion works end-to-end | D-07        | Requires two paired devices | Transfer a file between peers, verify progress events and completion |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
