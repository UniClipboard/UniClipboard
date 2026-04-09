---
phase: 50
slug: daemon-encryption-state-recovery
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-23
---

# Phase 50 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                 |
| ---------------------- | ----------------------------------------------------- |
| **Framework**          | cargo test (Rust)                                     |
| **Config file**        | `src-tauri/Cargo.toml`                                |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-app -- auto_unlock` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon -p uc-app`   |
| **Estimated runtime**  | ~30 seconds                                           |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-app -- auto_unlock`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon -p uc-app`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type          | Automated Command                                                | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ------------------ | ---------------------------------------------------------------- | ----------- | ---------- |
| 50-01-01 | 01   | 1    | D-07/D-08   | unit + integration | `cd src-tauri && cargo test -p uc-daemon -- encryption_recovery` | ❌ W0       | ⬜ pending |
| 50-01-02 | 01   | 1    | D-05/D-06   | unit               | `cd src-tauri && cargo test -p uc-daemon -- startup_failure`     | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- Existing `AutoUnlockEncryptionSession` has 7 unit tests covering core recovery logic
- New tests needed only for daemon startup integration (call site + failure handling)

_Existing infrastructure covers core use case requirements. Only daemon integration tests are new._

---

## Manual-Only Verifications

| Behavior                                     | Requirement | Why Manual                                | Test Instructions                                                                                                                                  |
| -------------------------------------------- | ----------- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| Daemon restarts and proof verification works | D-01/D-02   | Requires running daemon process lifecycle | 1. Start daemon with initialized encryption 2. Kill daemon 3. Restart daemon 4. Run `uniclipboard-cli status` — encryption session should be ready |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
