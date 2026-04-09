---
phase: 70
slug: cli-start-stop-commands-for-daemon-lifecycle-management
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-28
---

# Phase 70 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                  |
| ---------------------- | -------------------------------------- |
| **Framework**          | cargo test (Rust)                      |
| **Config file**        | src-tauri/Cargo.toml                   |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-cli` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-cli` |
| **Estimated runtime**  | ~10 seconds                            |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-cli`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-cli`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type | Automated Command                            | File Exists | Status     |
| -------- | ---- | ---- | ----------- | --------- | -------------------------------------------- | ----------- | ---------- |
| 70-01-01 | 01   | 1    | start cmd   | unit      | `cd src-tauri && cargo test -p uc-cli start` | ❌ W0       | ⬜ pending |
| 70-01-02 | 01   | 1    | stop cmd    | unit      | `cd src-tauri && cargo test -p uc-cli stop`  | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-cli/src/commands/start.rs` — start command with tests
- [ ] `src-tauri/crates/uc-cli/src/commands/stop.rs` — stop command with tests

_Existing test infrastructure in uc-cli covers framework requirements._

---

## Manual-Only Verifications

| Behavior                 | Requirement | Why Manual                             | Test Instructions                                                |
| ------------------------ | ----------- | -------------------------------------- | ---------------------------------------------------------------- |
| Background daemon spawn  | D-04        | Requires real daemon binary            | Run `uniclipboard-cli start`, verify daemon PID file created     |
| Foreground log streaming | D-07        | Requires real daemon binary + terminal | Run `uniclipboard-cli start -f`, verify logs appear              |
| Stop running daemon      | D-09/D-10   | Requires running daemon                | Run `uniclipboard-cli stop` after start, verify PID file removed |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
