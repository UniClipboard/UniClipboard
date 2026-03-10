---
phase: 19
slug: dual-output-logging-foundation
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-03-10
---

# Phase 19 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                                          |
| ---------------------- | -------------------------------------------------------------------------------------------------------------- |
| **Framework**          | cargo test (built-in)                                                                                          |
| **Config file**        | `src-tauri/Cargo.toml` (workspace)                                                                             |
| **Quick run command**  | `cd src-tauri && cargo test --package uc-observability`                                                        |
| **Full suite command** | `cd src-tauri && cargo test --package uc-observability && cargo test --package uc-tauri -- bootstrap::tracing` |
| **Estimated runtime**  | ~30 seconds                                                                                                    |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test --package uc-observability`
- **After every plan wave:** Run `cd src-tauri && cargo test --package uc-observability && cargo test --package uc-tauri -- bootstrap::tracing`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                                                                                         | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | ------------------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| 19-01-01 | 01   | 1    | LOG-01,03   | unit        | `cd src-tauri && cargo test --package uc-observability -- profile`                                                        | TDD         | ⬜ pending |
| 19-01-02 | 01   | 1    | LOG-02      | unit        | `cd src-tauri && cargo test --package uc-observability -- format`                                                         | TDD         | ⬜ pending |
| 19-01-03 | 01   | 1    | LOG-01      | unit        | `cd src-tauri && cargo test --package uc-observability -- init`                                                           | TDD         | ⬜ pending |
| 19-02-01 | 02   | 2    | LOG-01,04   | integration | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing && cargo test --package uc-observability`            | Existing+   | ⬜ pending |
| 19-02-02 | 02   | 2    | LOG-04      | doc check   | `test -f docs/architecture/logging-architecture.md && grep -c "UC_LOG_PROFILE" docs/architecture/logging-architecture.md` | N/A         | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

Plan 01 uses TDD (`tdd="true"`) — tests are created as part of the RED phase within each task. No separate Wave 0 scaffolding is needed. Test files live within the uc-observability crate:

- `src-tauri/crates/uc-observability/src/profile.rs` — `#[cfg(test)] mod tests` for LogProfile (LOG-03, LOG-04)
- `src-tauri/crates/uc-observability/src/format.rs` — `#[cfg(test)] mod tests` for FlatJsonFormat (LOG-02)
- `src-tauri/crates/uc-observability/src/init.rs` — `#[cfg(test)] mod tests` for init/builder functions (LOG-01)

---

## Manual-Only Verifications

| Behavior                            | Requirement | Why Manual              | Test Instructions                                                                             |
| ----------------------------------- | ----------- | ----------------------- | --------------------------------------------------------------------------------------------- |
| Pretty console output looks correct | LOG-01      | Visual formatting check | Run `bun tauri dev`, verify terminal shows human-readable colored output                      |
| JSON file appears in log directory  | LOG-01      | File system side effect | Run app, check platform log dir for `uniclipboard.json.YYYY-MM-DD`                            |
| Profile switch via UC_LOG_PROFILE   | LOG-04      | Env var integration     | Run `UC_LOG_PROFILE=debug_clipboard bun tauri dev`, verify clipboard targets show trace-level |
| Sentry layer receives events        | LOG-01      | Requires SENTRY_DSN     | Set SENTRY_DSN env var, run app, verify Sentry dashboard shows events (requires account)      |
| Legacy log::\* outputs to Webview   | LOG-01      | Browser DevTools check  | Run `bun tauri dev`, open DevTools Console, verify log::\* macro output appears               |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify commands targeting correct packages
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covered by TDD approach (tests created in RED phase)
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
