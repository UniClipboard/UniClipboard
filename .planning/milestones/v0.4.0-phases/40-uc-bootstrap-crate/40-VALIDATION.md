---
phase: 40
slug: uc-bootstrap-crate
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-18
---

# Phase 40 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                   |
| ---------------------- | ----------------------------------------------------------------------- |
| **Framework**          | cargo test (standard Rust test framework)                               |
| **Config file**        | none — standard cargo test                                              |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-bootstrap`                            |
| **Full suite command** | `cd src-tauri && cargo check && cargo test -p uc-bootstrap -p uc-tauri` |
| **Estimated runtime**  | ~30 seconds                                                             |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo check -p uc-bootstrap`
- **After every plan wave:** Run `cd src-tauri && cargo check && cargo test -p uc-bootstrap -p uc-tauri`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                                        | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | ------------------------------------------------------------------------ | ----------- | ---------- |
| 40-01-01 | 01   | 1    | BOOT-01     | build/check | `cd src-tauri && cargo check -p uc-bootstrap`                            | ❌ W0       | ⬜ pending |
| 40-01-02 | 01   | 1    | BOOT-05     | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_tracing_idempotent`  | ❌ W0       | ⬜ pending |
| 40-02-01 | 02   | 2    | BOOT-02     | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_build_cli_context`   | ❌ W0       | ⬜ pending |
| 40-02-02 | 02   | 2    | BOOT-03     | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_build_daemon_app`    | ❌ W0       | ⬜ pending |
| 40-03-01 | 03   | 3    | BOOT-04     | build/check | `cd src-tauri && cargo check -p uc-tauri`                                | ❌ implied  | ⬜ pending |
| 40-03-02 | 03   | 3    | RNTM-04     | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_usecases_accessible` | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-bootstrap/Cargo.toml` — crate scaffold with all dependencies
- [ ] `src-tauri/crates/uc-bootstrap/src/lib.rs` — crate entry with module declarations
- [ ] `src-tauri/Cargo.toml` — workspace members updated to include uc-bootstrap

_Crate must exist and `cargo check -p uc-bootstrap` must pass before any tests can run._

---

## Manual-Only Verifications

| Behavior                                            | Requirement | Why Manual                                    | Test Instructions                                                                                             |
| --------------------------------------------------- | ----------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| uc-tauri no longer imports uc-infra for composition | BOOT-04     | Semantic check — grep for composition imports | `grep -r "use uc_infra" src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` should return empty (file moved) |

_All other phase behaviors have automated verification via `cargo check` and `cargo test`._

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
