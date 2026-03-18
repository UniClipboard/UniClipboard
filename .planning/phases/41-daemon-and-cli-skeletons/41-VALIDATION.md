---
phase: 41
slug: daemon-and-cli-skeletons
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-18
---

# Phase 41 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                         |
| ---------------------- | ----------------------------------------------------------------------------- |
| **Framework**          | cargo test (Rust)                                                             |
| **Config file**        | `src-tauri/Cargo.toml` (workspace)                                            |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon -p uc-cli`                           |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon -p uc-cli -p uc-bootstrap -p uc-app` |
| **Estimated runtime**  | ~15 seconds                                                                   |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon -p uc-cli`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon -p uc-cli -p uc-bootstrap -p uc-app`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID                     | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status     |
| --------------------------- | ---- | ---- | ----------- | --------- | ----------------- | ----------- | ---------- |
| _Populated during planning_ |      |      |             |           |                   |             | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-daemon/` — new crate with Cargo.toml, lib.rs, main.rs
- [ ] `src-tauri/crates/uc-cli/` — new crate with Cargo.toml, main.rs
- [ ] Workspace members updated in `src-tauri/Cargo.toml`

_Existing test infrastructure (cargo test) covers all framework requirements._

---

## Manual-Only Verifications

| Behavior                     | Requirement | Why Manual                                 | Test Instructions                                                         |
| ---------------------------- | ----------- | ------------------------------------------ | ------------------------------------------------------------------------- |
| Graceful shutdown on SIGTERM | DAEM-03     | Requires signal sending to running process | Start daemon, send SIGTERM, verify clean exit (code 0) and socket cleanup |
| Stale socket cleanup         | DAEM-04     | Requires simulated crash scenario          | Create stale socket file, start daemon, verify it replaces the socket     |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
