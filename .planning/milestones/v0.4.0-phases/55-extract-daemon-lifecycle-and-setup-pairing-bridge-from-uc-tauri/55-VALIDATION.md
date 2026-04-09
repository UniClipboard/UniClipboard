---
phase: 55
slug: extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-24
---

# Phase 55 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                                              |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------ |
| **Framework**          | Rust `#[test]` / `#[tokio::test]` (built-in)                                                                       |
| **Config file**        | None — uses Cargo.toml dev-dependencies                                                                            |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon-client daemon_lifecycle -- --test-threads=1`                              |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon-client -- --test-threads=1 && cargo test -p uc-tauri -- --test-threads=1` |
| **Estimated runtime**  | ~30 seconds                                                                                                        |

---

## Sampling Rate

- **After every task commit:** Run `cargo check -p uc-daemon-client && cargo check -p uc-tauri`
- **After every plan wave:** Run full suite command above
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement                                  | Test Type   | Automated Command                                                                                      | File Exists                                                           | Status     |
| -------- | ---- | ---- | -------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- | ---------- | ---------- |
| 55-01-01 | 01   | 1    | daemon_lifecycle.rs migrated                 | build check | `cargo check -p uc-daemon-client`                                                                      | W0                                                                    | ⬜ pending |
| 55-01-02 | 01   | 1    | terminate_local_daemon_pid moved             | build check | `cargo check -p uc-daemon-client`                                                                      | W0                                                                    | ⬜ pending |
| 55-01-03 | 01   | 1    | lib.rs updated with new module               | build check | `cargo check -p uc-daemon-client`                                                                      | W0                                                                    | ⬜ pending |
| 55-02-01 | 02   | 1    | uc-daemon-client unit tests pass             | unit        | `cargo test -p uc-daemon-client daemon_lifecycle -- --test-threads=1`                                  | ✅                                                                    | ⬜ pending |
| 55-03-01 | 03   | 2    | main.rs import updated                       | grep        | `rg 'uc_tauri::bootstrap.*GuiOwnedDaemonState' src-tauri/src/main.rs`                                  | ✅                                                                    | ⬜ pending |
| 55-03-02 | 03   | 2    | run.rs re-imports terminate_local_daemon_pid | grep        | `rg 'uc_daemon_client.*terminate_local_daemon_pid' src-tauri/crates/uc-tauri/src/bootstrap/run.rs`     | ✅                                                                    | ⬜ pending |
| 55-03-03 | 03   | 2    | mod.rs stale entries removed                 | grep        | `rg 'daemon_lifecycle                                                                                  | setup_pairing_bridge' src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` | ✅         | ⬜ pending |
| 55-03-04 | 03   | 2    | daemon_exit_cleanup imports updated          | grep        | `rg 'uc_daemon_client.*daemon_lifecycle' src-tauri/crates/uc-tauri/tests/daemon_exit_cleanup.rs`       | ✅                                                                    | ⬜ pending |
| 55-03-05 | 03   | 2    | daemon_bootstrap_contract imports updated    | grep        | `rg 'uc_daemon_client.*daemon_lifecycle' src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs` | ✅                                                                    | ⬜ pending |
| 55-04-01 | 04   | 2    | setup_pairing_bridge.rs deleted              | grep        | `rg 'setup_pairing_bridge' src-tauri/crates/uc-tauri/src/bootstrap/`                                   | ✅ empty                                                              | ⬜ pending |
| 55-04-02 | 04   | 2    | daemon_lifecycle.rs deleted from uc-tauri    | grep        | `rg 'uc-tauri/bootstrap/daemon_lifecycle.rs' src-tauri/`                                               | ✅ empty                                                              | ⬜ pending |
| 55-05-01 | 05   | 2    | Full uc-tauri compiles                       | build check | `cargo check -p uc-tauri`                                                                              | ✅                                                                    | ⬜ pending |
| 55-05-02 | 05   | 2    | No stale bootstrap::daemon_lifecycle refs    | grep        | `rg 'uc_tauri::bootstrap.*daemon_lifecycle' src-tauri/src/ src-tauri/crates/uc-tauri/src/`             | ✅ empty                                                              | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. The 3 unit tests in `daemon_lifecycle.rs` migrate with the file. No Wave 0 gaps.

---

## Manual-Only Verifications

All phase behaviors have automated verification via `cargo check` and grep commands. No manual-only verifications.

---

## Validation Sign-Off

- [ ] All tasks have automated verify via cargo check or grep
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
