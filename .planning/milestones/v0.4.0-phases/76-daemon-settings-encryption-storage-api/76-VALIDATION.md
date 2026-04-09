---
phase: 76
slug: daemon-settings-encryption-storage-api
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-30
---

# Phase 76 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                               |
| ---------------------- | --------------------------------------------------- |
| **Framework**          | cargo test (Rust)                                   |
| **Config file**        | src-tauri/Cargo.toml                                |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon`           |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon -p uc-app` |
| **Estimated runtime**  | ~30 seconds                                         |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon -p uc-app`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement            | Test Type | Automated Command                                    | File Exists | Status     |
| -------- | ---- | ---- | ---------------------- | --------- | ---------------------------------------------------- | ----------- | ---------- |
| 76-01-01 | 01   | 1    | Settings GET/PUT       | unit      | `cd src-tauri && cargo test -p uc-daemon settings`   | ❌ W0       | ⬜ pending |
| 76-01-02 | 01   | 1    | L3/L4 permissions      | unit      | `cd src-tauri && cargo test -p uc-daemon permission` | ❌ W0       | ⬜ pending |
| 76-02-01 | 02   | 1    | Encryption state       | unit      | `cd src-tauri && cargo test -p uc-daemon encryption` | ❌ W0       | ⬜ pending |
| 76-02-02 | 02   | 1    | Encryption unlock/lock | unit      | `cd src-tauri && cargo test -p uc-daemon encryption` | ❌ W0       | ⬜ pending |
| 76-03-01 | 03   | 2    | Storage stats          | unit      | `cd src-tauri && cargo test -p uc-daemon storage`    | ❌ W0       | ⬜ pending |
| 76-03-02 | 03   | 2    | Clear cache            | unit      | `cd src-tauri && cargo test -p uc-daemon storage`    | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] Test stubs for settings, encryption, storage endpoints in uc-daemon test modules
- [ ] Shared test fixtures for authenticated requests with L2/L3/L4 permissions

_Existing infrastructure covers test framework setup._

---

## Manual-Only Verifications

| Behavior                | Requirement                    | Why Manual                    | Test Instructions                                          |
| ----------------------- | ------------------------------ | ----------------------------- | ---------------------------------------------------------- |
| Encryption WS broadcast | encryption.session-ready event | Requires WS client connection | Connect WS, POST /encryption/unlock, verify event received |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
