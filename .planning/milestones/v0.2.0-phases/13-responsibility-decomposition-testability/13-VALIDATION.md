---
phase: 13
slug: responsibility-decomposition-testability
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 13 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                        |
| ---------------------- | ---------------------------------------------------------------------------- |
| **Framework**          | cargo test (Rust built-in)                                                   |
| **Config file**        | `src-tauri/Cargo.toml` workspace members                                     |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-app --lib -- --test-threads=1`             |
| **Full suite command** | `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-core -p uc-platform` |
| **Estimated runtime**  | ~30 seconds                                                                  |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-app --lib -- --test-threads=1`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-core -p uc-platform`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                                          | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | -------------------------------------------------------------------------- | ----------- | ---------- |
| 13-01-01 | 01   | 1    | DECOMP-01   | integration | `cd src-tauri && cargo test -p uc-app --test setup_flow_integration_test`  | ✅          | ⬜ pending |
| 13-01-02 | 01   | 1    | DECOMP-01   | unit        | `cd src-tauri && cargo test -p uc-app --lib pairing::orchestrator::tests`  | ✅          | ⬜ pending |
| 13-02-01 | 02   | 2    | DECOMP-02   | unit        | `cd src-tauri && cargo test -p uc-app --lib deps::tests`                   | ✅          | ⬜ pending |
| 13-02-02 | 02   | 2    | DECOMP-02   | unit        | `cd src-tauri && cargo test -p uc-tauri --test usecases_accessor_test`     | ✅          | ⬜ pending |
| 13-03-01 | 03   | 2    | DECOMP-03   | unit        | `cd src-tauri && cargo test -p uc-app --lib testing`                       | ❌ W0       | ⬜ pending |
| 13-03-02 | 03   | 2    | DECOMP-04   | integration | `cd src-tauri && cargo test -p uc-app --test setup_flow_integration_test`  | ✅          | ⬜ pending |
| 13-03-03 | 03   | 2    | DECOMP-04   | unit        | `cd src-tauri && cargo test -p uc-app --lib pairing`                       | ✅          | ⬜ pending |
| 13-03-04 | 03   | 2    | DECOMP-04   | integration | `cd src-tauri && cargo test -p uc-tauri --test bootstrap_integration_test` | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-app/src/testing.rs` — shared noop/mock module stubs (covers DECOMP-03)
- [ ] Verify all existing tests pass before starting decomposition: `cd src-tauri && cargo test -p uc-app -p uc-tauri`

_Existing infrastructure covers most phase requirements. Wave 0 only needs the shared testing module._

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
