---
phase: 37
slug: wiring-decomposition
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-17
---

# Phase 37 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                             |
| ---------------------- | ----------------------------------------------------------------- |
| **Framework**          | Rust built-in test + tokio::test                                  |
| **Config file**        | src-tauri/Cargo.toml (test profiles)                              |
| **Quick run command**  | `cd src-tauri && cargo check -p uc-core`                          |
| **Full suite command** | `cd src-tauri && cargo test -p uc-core && cargo test -p uc-tauri` |
| **Estimated runtime**  | ~30 seconds                                                       |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo check -p uc-core`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-core && cargo test -p uc-tauri`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type     | Automated Command                                                                                   | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ------------- | --------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| 37-01-01 | 01   | 1    | RNTM-02     | cargo check   | `cd src-tauri && cargo check -p uc-core`                                                            | ✅          | ⬜ pending |
| 37-01-02 | 01   | 1    | RNTM-02     | lint/grep     | `grep -c 'tauri::' src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` should output 0             | ❌ W0       | ⬜ pending |
| 37-01-03 | 01   | 1    | RNTM-02     | contract test | `cd src-tauri && cargo test -p uc-tauri test_pairing_`                                              | ❌ W0       | ⬜ pending |
| 37-01-04 | 01   | 1    | RNTM-02     | lint/grep     | `grep -c 'tauri::' src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` should output 0 | ✅          | ⬜ pending |
| 37-01-05 | 01   | 1    | RNTM-02     | cargo check   | `cd src-tauri && cargo check -p uc-tauri`                                                           | ✅          | ⬜ pending |
| 37-01-06 | 01   | 1    | RNTM-02     | unit test     | `cd src-tauri && cargo test -p uc-tauri test_logging_emitter`                                       | ❌ W0       | ⬜ pending |
| 37-01-07 | 01   | 1    | RNTM-02     | manual        | `bun tauri dev` + clipboard + pairing manual test                                                   | manual-only | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` — doesn't exist yet; created during split
- [ ] New contract tests for PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent in `adapters/host_event_emitter.rs`
- [ ] New unit tests for LoggingEventEmitter with new HostEvent variants
- [ ] CI grep lint rule for assembly.rs — verify zero tauri imports

_Existing test infrastructure covers cargo check and cargo test commands._

---

## Manual-Only Verifications

| Behavior                           | Requirement | Why Manual                              | Test Instructions                                                                                               |
| ---------------------------------- | ----------- | --------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| GUI behavior unchanged after split | RNTM-02     | Full integration requires Tauri runtime | 1. Run `bun tauri dev` 2. Test clipboard sync between devices 3. Test pairing flow 4. Verify settings save/load |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
