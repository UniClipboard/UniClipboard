---
phase: 43
slug: unify-gui-and-cli-business-flows-to-eliminate-per-entrypoint-feature-adaptation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-19
---

# Phase 43 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                        |
| ---------------------- | ------------------------------------------------------------ |
| **Framework**          | Rust `cargo test` + crate integration tests                  |
| **Config file**        | none                                                         |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-cli --test cli_smoke`      |
| **Full suite command** | `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-cli` |
| **Estimated runtime**  | ~120 seconds                                                 |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-cli --test cli_smoke`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-cli`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type        | Automated Command                                                                | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ---------------- | -------------------------------------------------------------------------------- | ----------- | ---------- |
| 43-01-01 | 01   | 1    | PH43-01     | integration      | `cd src-tauri && cargo test -p uc-cli --test cli_smoke`                          | ✅ W0       | ⬜ pending |
| 43-01-02 | 01   | 1    | PH43-02     | integration      | `cd src-tauri && cargo test -p uc-tauri clipboard_commands_stats_favorites_test` | ✅          | ⬜ pending |
| 43-02-01 | 02   | 1    | PH43-03     | unit/integration | `cd src-tauri && cargo test -p uc-app pairing`                                   | ✅ partial  | ⬜ pending |
| 43-02-02 | 02   | 2    | PH43-04     | integration      | `cd src-tauri && cargo test -p uc-app --test setup_flow_integration_test`        | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-app/tests/shared_flow_clipboard_test.rs` — proves one app-layer clipboard flow drives both CLI and GUI callers
- [ ] `src-tauri/crates/uc-app/tests/shared_flow_pairing_snapshot_test.rs` — covers paired/discovered/connected peer aggregation outside Tauri commands
- [ ] `src-tauri/crates/uc-cli/tests/cli_flow_parity_test.rs` — verifies CLI command output still works after shared runtime/helper extraction
- [ ] `src-tauri/crates/uc-tauri/tests/shared_flow_command_contract_test.rs` — asserts Tauri commands remain thin wrappers over shared flow accessors

---

## Manual-Only Verifications

| Behavior                       | Requirement | Why Manual                                            | Test Instructions                                      |
| ------------------------------ | ----------- | ----------------------------------------------------- | ------------------------------------------------------ |
| CLI output format parity       | PH43-01     | Requires visual verification of CLI output formatting | Run CLI commands and compare output to expected format |
| Cross-surface flow integration | PH43-02     | GUI-only test environment setup                       | Execute GUI workflow and verify same code path is hit  |

_If none: "All phase behaviors have automated verification."_

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
