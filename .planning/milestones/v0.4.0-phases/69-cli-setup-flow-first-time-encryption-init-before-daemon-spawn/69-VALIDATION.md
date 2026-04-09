---
phase: 69
slug: cli-setup-flow-first-time-encryption-init-before-daemon-spawn
status: draft
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-28
---

# Phase 69 — Validation Strategy

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

| Task ID  | Plan | Wave | Requirement               | Test Type  | Automated Command                                   | File Exists                | Status     |
| -------- | ---- | ---- | ------------------------- | ---------- | --------------------------------------------------- | -------------------------- | ---------- |
| 69-01-00 | 01   | 1    | PH69-01, PH69-02          | unit       | `cd src-tauri && cargo test -p uc-cli -- new_space` | Wave 0 (created in Task 0) | ⬜ pending |
| 69-01-01 | 01   | 1    | PH69-01                   | unit       | `cd src-tauri && cargo test -p uc-cli -- new_space` | Covered by 69-01-00        | ⬜ pending |
| 69-01-02 | 01   | 1    | PH69-01, PH69-02, PH69-03 | regression | `cd src-tauri && cargo test -p uc-cli`              | Yes (existing tests)       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

Task 0 in Plan 01 creates behavioral tests covering:

- [ ] `new_space_already_initialized_returns_error` — EncryptionState::Initialized returns Err(EXIT_ERROR) (REQ PH69-02)
- [ ] `new_space_uninitialized_allows_init` — EncryptionState::Uninitialized returns Ok(()) (REQ PH69-01)

Test file: `src-tauri/crates/uc-cli/tests/setup_cli.rs` (extends existing test harness)

---

## Manual-Only Verifications

| Behavior                                | Requirement | Why Manual                     | Test Instructions                                                                                     |
| --------------------------------------- | ----------- | ------------------------------ | ----------------------------------------------------------------------------------------------------- |
| Post-setup prompts daemon start command | PH69-03     | UX verification of output text | After new space completes, verify CLI outputs guidance to run `uniclipboard-daemon` then `setup host` |

_PH69-03 (next-step hint display) is inherently a UI text check — automated guard tests cover the control flow, manual verification covers the exact wording._

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 10s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
