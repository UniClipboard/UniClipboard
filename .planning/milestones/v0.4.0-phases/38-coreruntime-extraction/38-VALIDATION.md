---
phase: 38
slug: coreruntime-extraction
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-18
---

# Phase 38 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                              |
| ---------------------- | -------------------------------------------------- |
| **Framework**          | Cargo test (built-in)                              |
| **Config file**        | src-tauri/Cargo.toml workspace                     |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-app`             |
| **Full suite command** | `cd src-tauri && cargo test -p uc-app -p uc-tauri` |
| **Estimated runtime**  | ~30 seconds                                        |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo check -p uc-app && cargo check -p uc-tauri`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-app -p uc-tauri`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type     | Automated Command                                                                 | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ------------- | --------------------------------------------------------------------------------- | ----------- | ---------- |
| 38-01-01 | 01   | 1    | RNTM-01     | compile check | `cd src-tauri && cargo check -p uc-app`                                           | ❌ W0       | ⬜ pending |
| 38-01-02 | 01   | 1    | RNTM-01     | unit          | `cd src-tauri && cargo test -p uc-app task_registry`                              | ❌ W0       | ⬜ pending |
| 38-01-03 | 01   | 1    | RNTM-01     | unit          | `cd src-tauri && cargo test -p uc-app in_memory_lifecycle`                        | ❌ W0       | ⬜ pending |
| 38-01-04 | 01   | 1    | RNTM-01     | unit          | `cd src-tauri && cargo test -p uc-app logging_lifecycle`                          | ❌ W0       | ⬜ pending |
| 38-01-05 | 01   | 1    | RNTM-01     | unit          | `cd src-tauri && cargo test -p uc-app logging_session_ready`                      | ❌ W0       | ⬜ pending |
| 38-02-01 | 02   | 2    | RNTM-05     | unit          | `cd src-tauri && cargo test -p uc-tauri build_setup_orchestrator`                 | ❌ W0       | ⬜ pending |
| 38-02-02 | 02   | 2    | RNTM-05     | unit (SC#4)   | `cd src-tauri && cargo test -p uc-app setup_state_emission_survives_emitter_swap` | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-app/src/runtime.rs` — CoreRuntime struct (created in this phase)
- [ ] `src-tauri/crates/uc-app/src/task_registry.rs` — moved from uc-tauri; existing tests migrate with it
- [ ] `src-tauri/crates/uc-app/src/usecases/app_lifecycle/adapters.rs` — moved from uc-tauri; existing tests migrate
- [ ] SC#4 test `setup_state_emission_survives_emitter_swap` — new test for stale emitter fix

_Existing tests in uc-tauri/src/adapters/lifecycle.rs (5 tests) and uc-tauri/src/bootstrap/task_registry.rs (4 tests) migrate with their code — not new Wave 0 additions._

---

## Manual-Only Verifications

| Behavior                        | Requirement | Why Manual                         | Test Instructions                                                                                                                  |
| ------------------------------- | ----------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| GUI setup flow works end-to-end | SC#4        | Requires running Tauri app with UI | 1. `bun tauri dev` 2. Complete first-run setup 3. Verify encryption unlock works 4. Check setup state transitions emit to frontend |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
