---
phase: 19
slug: dual-output-logging-foundation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 19 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                 |
| ---------------------- | --------------------------------------------------------------------- |
| **Framework**          | cargo test (built-in)                                                 |
| **Config file**        | `src-tauri/Cargo.toml` (workspace)                                    |
| **Quick run command**  | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing` |
| **Full suite command** | `cd src-tauri && cargo test`                                          |
| **Estimated runtime**  | ~30 seconds                                                           |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing`
- **After every plan wave:** Run `cd src-tauri && cargo test --package uc-tauri`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type | Automated Command                                                                                            | File Exists | Status     |
| -------- | ---- | ---- | ----------- | --------- | ------------------------------------------------------------------------------------------------------------ | ----------- | ---------- |
| 19-01-01 | 01   | 0    | LOG-01      | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing`                                        | ✅ Partial  | ⬜ pending |
| 19-01-02 | 01   | 0    | LOG-02      | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing_json`                                   | ❌ W0       | ⬜ pending |
| 19-01-03 | 01   | 0    | LOG-03      | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing_profiles`                               | ❌ W0       | ⬜ pending |
| 19-01-04 | 01   | 0    | LOG-04      | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing_profiles::tests::test_profile_from_env` | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/tracing_json.rs` — unit tests for FlatJsonFormat with mock spans (LOG-02)
- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/tracing_profiles.rs` — unit tests for LogProfile enum and filter construction (LOG-03, LOG-04)
- [ ] Update `tracing-subscriber` features in Cargo.toml to include `json`

_Existing infrastructure covers LOG-01 partially via existing tracing tests._

---

## Manual-Only Verifications

| Behavior                            | Requirement | Why Manual              | Test Instructions                                                                             |
| ----------------------------------- | ----------- | ----------------------- | --------------------------------------------------------------------------------------------- |
| Pretty console output looks correct | LOG-01      | Visual formatting check | Run `bun tauri dev`, verify terminal shows human-readable colored output                      |
| JSON file appears in log directory  | LOG-01      | File system side effect | Run app, check platform log dir for `uniclipboard.json.YYYY-MM-DD`                            |
| Profile switch via UC_LOG_PROFILE   | LOG-04      | Env var integration     | Run `UC_LOG_PROFILE=debug_clipboard bun tauri dev`, verify clipboard targets show trace-level |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
