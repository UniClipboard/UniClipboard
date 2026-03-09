---
phase: 10
slug: boundary-repair-baseline
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 10 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                              |
| ---------------------- | ---------------------------------- |
| **Framework**          | cargo test (Rust built-in)         |
| **Config file**        | `src-tauri/Cargo.toml` (workspace) |
| **Quick run command**  | `cd src-tauri && cargo check`      |
| **Full suite command** | `cd src-tauri && cargo test`       |
| **Estimated runtime**  | ~60 seconds                        |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo check`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type           | Automated Command                            | File Exists               | Status     |
| -------- | ---- | ---- | ----------- | ------------------- | -------------------------------------------- | ------------------------- | ---------- |
| 10-01-01 | 01   | 1    | BOUND-01    | compile-time        | `cd src-tauri && cargo check`                | N/A — compiler enforces   | ⬜ pending |
| 10-01-02 | 01   | 1    | BOUND-02    | compile-time        | `cd src-tauri && cargo check`                | N/A — compiler enforces   | ⬜ pending |
| 10-02-01 | 02   | 1    | BOUND-03    | compile-time        | `cd src-tauri && cargo check -p uc-platform` | N/A — Cargo.toml enforces | ⬜ pending |
| 10-03-01 | 03   | 2    | BOUND-04    | compile-time + grep | `cd src-tauri && cargo check`                | N/A — compiler enforces   | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. The primary validation mechanism is the Rust compiler, which enforces boundary violations as compile errors. No additional test files are needed.

---

## Manual-Only Verifications

| Behavior                                      | Requirement | Why Manual                             | Test Instructions                                                                                                                                                           |
| --------------------------------------------- | ----------- | -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Non-domain ports not re-exported from uc-core | BOUND-04    | Grep verification complements compiler | `grep -rn 'AutostartPort\|UiPort\|AppDirsPort\|WatcherControlPort\|IdentityStorePort\|ObservabilityPort' src-tauri/crates/uc-core/src/ports/mod.rs` should return 0 results |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
