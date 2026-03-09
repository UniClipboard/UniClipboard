---
phase: 12
slug: lifecycle-governance-baseline
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 12 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                           |
| ---------------------- | --------------------------------------------------------------- |
| **Framework**          | cargo test (Rust, built-in)                                     |
| **Config file**        | src-tauri/Cargo.toml (workspace)                                |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-tauri --lib -- task_registry` |
| **Full suite command** | `cd src-tauri && cargo test`                                    |
| **Estimated runtime**  | ~30 seconds                                                     |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-tauri --lib`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement      | Test Type   | Automated Command                                                                               | File Exists | Status     |
| -------- | ---- | ---- | ---------------- | ----------- | ----------------------------------------------------------------------------------------------- | ----------- | ---------- |
| 12-01-01 | 01   | 0    | LIFE-01, LIFE-02 | unit        | `cd src-tauri && cargo test -p uc-tauri -- task_registry::tests`                                | ❌ W0       | ⬜ pending |
| 12-01-02 | 01   | 1    | LIFE-01          | unit        | `cd src-tauri && cargo test -p uc-tauri -- task_registry::tests::shutdown_cancels_all`          | ❌ W0       | ⬜ pending |
| 12-01-03 | 01   | 1    | LIFE-02          | unit        | `cd src-tauri && cargo test -p uc-tauri -- task_registry::tests::timeout_aborts`                | ❌ W0       | ⬜ pending |
| 12-02-01 | 02   | 2    | LIFE-03          | unit        | `cd src-tauri && cargo test -p uc-app -- staged_paired_device_store::tests`                     | Partial     | ⬜ pending |
| 12-02-02 | 02   | 2    | LIFE-04          | manual-only | `grep -r "impl EncryptionSessionPort" crates/ --include="*.rs" \| grep -v test \| grep -v mock` | N/A         | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs` — new file with TaskRegistry struct + unit tests
- [ ] Add `tokio-util = { version = "0.7", features = ["sync"] }` to uc-tauri/Cargo.toml

_Wave 0 establishes TaskRegistry infrastructure before any integration work begins._

---

## Manual-Only Verifications

| Behavior                                             | Requirement | Why Manual                                 | Test Instructions                                                                                                                       |
| ---------------------------------------------------- | ----------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| Only one EncryptionSessionPort impl in non-test code | LIFE-04     | Static code analysis, not runtime behavior | Run `grep -r "impl EncryptionSessionPort" src-tauri/crates/ --include="*.rs" \| grep -v test \| grep -v mock` — expect exactly 1 result |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
