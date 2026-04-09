---
phase: 62
slug: daemon-inbound-clipboard-sync
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-25
---

# Phase 62 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                     |
| ---------------------- | ------------------------------------------------------------------------- |
| **Framework**          | cargo test (Rust unit tests in-module)                                    |
| **Config file**        | src-tauri/Cargo.toml workspace                                            |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon workers::inbound_clipboard_sync` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon`                                 |
| **Estimated runtime**  | ~15 seconds                                                               |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon workers::inbound_clipboard_sync`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type  | Automated Command                                                                | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ---------- | -------------------------------------------------------------------------------- | ----------- | ---------- |
| 62-01-01 | 01   | 1    | PH62-01     | unit       | `cd src-tauri && cargo test -p uc-daemon workers::inbound_clipboard_sync::tests` | ❌ W0       | ⬜ pending |
| 62-01-02 | 01   | 1    | PH62-02     | unit       | same                                                                             | ❌ W0       | ⬜ pending |
| 62-01-03 | 01   | 1    | PH62-03     | unit       | same                                                                             | ❌ W0       | ⬜ pending |
| 62-01-04 | 01   | 1    | PH62-04     | unit       | same                                                                             | ❌ W0       | ⬜ pending |
| 62-01-05 | 01   | 1    | PH62-05     | structural | same                                                                             | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs` — covers PH62-01 through PH62-05
- [ ] Update `src-tauri/crates/uc-daemon/src/workers/mod.rs` — add `pub mod inbound_clipboard_sync`

_Existing cargo test infrastructure covers all phase requirements._

---

## Manual-Only Verifications

_All phase behaviors have automated verification._

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
