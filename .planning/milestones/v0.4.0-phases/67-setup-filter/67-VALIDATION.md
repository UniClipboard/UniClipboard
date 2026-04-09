---
phase: 67
slug: setup-filter
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-27
---

# Phase 67 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                           |
| ---------------------- | ----------------------------------------------- |
| **Framework**          | cargo test (Rust)                               |
| **Config file**        | src-tauri/Cargo.toml                            |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon --lib` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon --lib` |
| **Estimated runtime**  | ~30 seconds                                     |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon --lib`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon --lib`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type | Automated Command                                                  | File Exists | Status     |
| -------- | ---- | ---- | ----------- | --------- | ------------------------------------------------------------------ | ----------- | ---------- |
| 67-01-01 | 01   | 1    | D-01, D-06  | unit      | `cd src-tauri && cargo test -p uc-daemon --lib recover_encryption` | ✅          | ⬜ pending |
| 67-01-02 | 01   | 1    | D-09, D-10  | unit      | `cd src-tauri && cargo test -p uc-daemon --lib session_ready`      | ❌ W0       | ⬜ pending |
| 67-01-03 | 01   | 1    | D-05, D-07  | unit      | `cd src-tauri && cargo test -p uc-daemon --lib daemon_app`         | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] Unit tests for SessionReadyEmitter/oneshot signal pattern — stubs for D-09/D-10

_Existing test infrastructure (cargo test) covers most phase requirements._

---

## Manual-Only Verifications

| Behavior                           | Requirement | Why Manual                                    | Test Instructions                                                                         |
| ---------------------------------- | ----------- | --------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Device not visible before setup    | D-01        | Requires two physical devices on LAN          | Start daemon without setup, check from second device that first is not in discovered list |
| Device becomes visible after setup | D-09        | Requires setup flow completion on live daemon | Complete setup on first device, verify second device discovers it within 10s              |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
