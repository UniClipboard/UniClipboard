---
phase: 65
slug: remove-gui-clipboard-watcher-delegate-clipboard-monitoring-exclusively-to-daemon
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-26
---

# Phase 65 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                         |
| ---------------------- | ----------------------------- |
| **Framework**          | cargo test (Rust)             |
| **Config file**        | `src-tauri/Cargo.toml`        |
| **Quick run command**  | `cd src-tauri && cargo check` |
| **Full suite command** | `cd src-tauri && cargo test`  |
| **Estimated runtime**  | ~60 seconds                   |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo check`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type    | Automated Command                                                  | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ------------ | ------------------------------------------------------------------ | ----------- | ---------- |
| 65-01-01 | 01   | 1    | D-01..D-05  | compile      | `cd src-tauri && cargo check -p uc-platform`                       | ✅          | ⬜ pending |
| 65-01-02 | 01   | 1    | D-06..D-08  | compile      | `cd src-tauri && cargo check -p uc-platform -p uc-core`            | ✅          | ⬜ pending |
| 65-02-01 | 02   | 2    | D-09..D-12  | compile+test | `cd src-tauri && cargo test -p uc-app -p uc-bootstrap -p uc-tauri` | ✅          | ⬜ pending |
| 65-02-02 | 02   | 2    | D-14        | compile      | `cd src-tauri && cargo check`                                      | ✅          | ⬜ pending |
| 65-02-03 | 02   | 2    | all         | full suite   | `cd src-tauri && cargo test`                                       | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. This is a deletion phase — cargo check/test validates that no remaining code references deleted items.

---

## Manual-Only Verifications

| Behavior                           | Requirement | Why Manual                     | Test Instructions                                                   |
| ---------------------------------- | ----------- | ------------------------------ | ------------------------------------------------------------------- |
| GUI starts and reaches Ready state | D-09        | Lifecycle state machine change | Launch `bun tauri dev`, verify app reaches dashboard without errors |
| Clipboard sync works via daemon    | D-14        | End-to-end daemon path         | Copy text on device A, verify it appears on device B                |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
