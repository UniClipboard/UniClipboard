---
phase: 11
slug: command-contract-hardening
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 11 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                         |
| ---------------------- | --------------------------------------------- |
| **Framework**          | Rust built-in `#[test]` + `cargo test`        |
| **Config file**        | `src-tauri/crates/uc-tauri/Cargo.toml`        |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-tauri 2>&1` |
| **Full suite command** | `cd src-tauri && cargo test --workspace 2>&1` |
| **Estimated runtime**  | ~30 seconds                                   |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-tauri 2>&1`
- **After every plan wave:** Run `cd src-tauri && cargo test --workspace 2>&1`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement              | Test Type | Automated Command                                               | File Exists | Status     |
| -------- | ---- | ---- | ------------------------ | --------- | --------------------------------------------------------------- | ----------- | ---------- |
| 11-01-01 | 01   | 0    | CONTRACT-01, CONTRACT-03 | unit      | `cd src-tauri && cargo test -p uc-tauri -- models`              | ❌ W0       | ⬜ pending |
| 11-01-02 | 01   | 1    | CONTRACT-01              | unit      | `cd src-tauri && cargo test -p uc-tauri -- models::dto`         | ❌ W0       | ⬜ pending |
| 11-01-03 | 01   | 1    | CONTRACT-03              | unit      | `cd src-tauri && cargo test -p uc-tauri -- commands::clipboard` | ✅          | ⬜ pending |
| 11-01-04 | 01   | 1    | CONTRACT-03              | unit      | `cd src-tauri && cargo test -p uc-tauri -- commands::setup`     | ✅          | ⬜ pending |
| 11-01-05 | 01   | 2    | CONTRACT-01, CONTRACT-03 | unit      | `cd src-tauri && cargo test -p uc-tauri -- models`              | ❌ W0       | ⬜ pending |
| 11-02-01 | 02   | 0    | CONTRACT-02, CONTRACT-04 | unit      | `cd src-tauri && cargo test -p uc-tauri -- commands::error`     | ❌ W0       | ⬜ pending |
| 11-02-02 | 02   | 1    | CONTRACT-02              | unit      | `cd src-tauri && cargo test -p uc-tauri -- commands::error`     | ❌ W0       | ⬜ pending |
| 11-02-03 | 02   | 1    | CONTRACT-04              | unit      | `cd src-tauri && cargo test -p uc-tauri -- commands::clipboard` | ✅          | ⬜ pending |
| 11-02-04 | 02   | 2    | CONTRACT-02              | unit      | `cd src-tauri && cargo test -p uc-tauri -- commands`            | ✅          | ⬜ pending |
| 11-02-05 | 02   | 2    | CONTRACT-02, CONTRACT-04 | unit      | `cd src-tauri && cargo test -p uc-tauri -- commands`            | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-tauri/src/models/mod.rs` — stub `PairedDeviceDto`, `SettingsDto`, `SetupStateDto`, `LifecycleStatusDto` structs with serialization tests (for CONTRACT-01, CONTRACT-03)
- [ ] `src-tauri/crates/uc-tauri/src/commands/error.rs` — stub `CommandError` enum with `NotFound`, `InternalError`, `Timeout`, `Cancelled`, `ValidationError`, `Conflict` variants + serialization tests (for CONTRACT-02, CONTRACT-04)

_Both stubs must compile (even with empty impls) before Wave 1 tasks begin._

---

## Manual-Only Verifications

| Behavior                                                       | Requirement | Why Manual                                            | Test Instructions                                                                    |
| -------------------------------------------------------------- | ----------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Frontend setup flow remains functional after double-encode fix | CONTRACT-03 | Requires running Tauri app and exercising setup UI    | Run `bun tauri dev`, complete the setup flow, verify no JSON parse errors in console |
| TypeScript error handling discriminated union works            | CONTRACT-02 | Requires frontend to consume new `CommandError` shape | Trigger a not-found scenario in UI and verify the error message renders correctly    |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
