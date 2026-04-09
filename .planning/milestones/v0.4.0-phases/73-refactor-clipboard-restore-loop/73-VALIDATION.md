---
phase: 73
slug: refactor-clipboard-restore-loop-prevention
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-29
---

# Phase 73 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                    |
| ---------------------- | ------------------------------------------------------------------------ |
| **Framework**          | cargo test (Rust)                                                        |
| **Config file**        | src-tauri/Cargo.toml                                                     |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-app --lib clipboard_write_coordinator` |
| **Full suite command** | `cd src-tauri && cargo test`                                             |
| **Estimated runtime**  | ~60 seconds                                                              |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-app --lib clipboard_write_coordinator`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement    | Test Type   | Automated Command                                                        | File Exists | Status     |
| -------- | ---- | ---- | -------------- | ----------- | ------------------------------------------------------------------------ | ----------- | ---------- |
| 73-01-01 | 01   | 1    | D-01/D-02/D-03 | unit        | `cd src-tauri && cargo test -p uc-app --lib clipboard_write_coordinator` | ❌ W0       | ⬜ pending |
| 73-01-02 | 01   | 1    | D-12/D-15/D-16 | unit        | `cd src-tauri && cargo test -p uc-app --lib clipboard_write_coordinator` | ❌ W0       | ⬜ pending |
| 73-02-01 | 02   | 2    | D-05/D-06/D-07 | integration | `cd src-tauri && cargo test -p uc-app`                                   | ✅          | ⬜ pending |
| 73-02-02 | 02   | 2    | D-08           | integration | `cd src-tauri && cargo test -p uc-daemon`                                | ✅          | ⬜ pending |
| 73-02-03 | 02   | 2    | D-11/D-13/D-14 | build       | `cd src-tauri && cargo check`                                            | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs` — unit tests for ClipboardWriteCoordinator write pipeline
- [ ] Mock implementations of ClipboardChangeOriginPort and SystemClipboardPort for coordinator tests

_Existing test infrastructure covers remaining phase requirements._

---

## Manual-Only Verifications

| Behavior                                | Requirement | Why Manual                            | Test Instructions                                                                                        |
| --------------------------------------- | ----------- | ------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| Clipboard restore from Dashboard UI     | D-06        | Requires Tauri runtime + OS clipboard | 1. Copy text, 2. Click restore in Dashboard, 3. Verify clipboard updated, 4. Verify no duplicate capture |
| File copy to clipboard via context menu | D-07        | Requires Tauri runtime + OS clipboard | 1. Right-click file entry, 2. Copy to clipboard, 3. Verify clipboard contains file paths                 |
| Inbound sync writes to OS clipboard     | D-08        | Requires two paired devices           | 1. Copy on peer A, 2. Verify peer B clipboard updated, 3. Verify no loop-back capture on B               |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
