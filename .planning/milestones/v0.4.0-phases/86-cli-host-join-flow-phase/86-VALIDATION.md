---
phase: 86
slug: cli-host-join-flow-phase
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-03
---

# Phase 86 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                            |
| ---------------------- | ---------------------------------------------------------------- |
| **Framework**          | Rust built-in `#[test]` + `#[cfg(test)]`                         |
| **Config file**        | None — inline in source files                                    |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-cli --lib -- --test-threads=1` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-cli -p uc-daemon-client`       |
| **Estimated runtime**  | ~30 seconds                                                      |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p uc-cli --lib -- --test-threads=1 2>&1 | tail -20`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-cli -p uc-daemon-client`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement                           | Test Type   | Automated Command                                             | File Exists     | Status     |
| -------- | ---- | ---- | ------------------------------------- | ----------- | ------------------------------------------------------------- | --------------- | ---------- |
| 86-01-01 | P01  | 1    | REQ-86-01: Phase 0 if/else fix        | unit        | `cargo test -p uc-cli --lib setup::tests -- --test-threads=1` | ✅ setup.rs     | ⬜ pending |
| 86-02-01 | P01  | 1    | REQ-86-02: ParsedSetupState           | unit        | `cargo test -p uc-daemon-client --lib setup::`                | ❌ new file     | ⬜ pending |
| 86-03-01 | P02  | 1    | REQ-86-03: HostCliPhase enum          | unit        | `cargo test -p uc-cli --lib host_flow::`                      | ❌ new file     | ⬜ pending |
| 86-03-02 | P02  | 1    | REQ-86-03: JoinCliPhase enum          | unit        | `cargo test -p uc-cli --lib join_flow::`                      | ❌ new file     | ⬜ pending |
| 86-04-01 | P03  | 1    | REQ-86-04: Phase-driven loop run_pair | integration | `cargo test -p uc-cli integration_tests::`                    | ⚠️ setup_cli.rs | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-daemon-client/src/setup/parsed_state.rs` — Unit tests for `parse_setup_state` (SetupHint, SetupVariant, ParsedSetupState)
- [ ] `src-tauri/crates/uc-daemon-client/src/setup/mod.rs` — Module re-exports
- [ ] `src-tauri/crates/uc-cli/src/commands/setup/host_flow.rs` — Unit tests for `derive_host_phase` and `HostCliPhase`
- [ ] `src-tauri/crates/uc-cli/src/commands/setup/join_flow.rs` — Unit tests for `derive_join_phase` and `JoinCliPhase`
- [ ] Framework verification: `cargo test -p uc-daemon-client -p uc-cli` compiles without errors

_If none: "Existing infrastructure covers all phase requirements."_

---

## Manual-Only Verifications

| Behavior                                           | Requirement | Why Manual                  | Test Instructions                                                      |
| -------------------------------------------------- | ----------- | --------------------------- | ---------------------------------------------------------------------- |
| Interactive spinner behavior in run_pair loop      | REQ-86-01   | Requires terminal emulation | Run `cargo test -p uc-cli --test setup_cli` and observe spinner output |
| Phase transition debug log printed once per change | REQ-86-01   | Debug output verification   | Run with `RUST_LOG=debug` and observe state_signature change logging   |

_If none: "All phase behaviors have automated verification."_

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** {pending / approved YYYY-MM-DD}
