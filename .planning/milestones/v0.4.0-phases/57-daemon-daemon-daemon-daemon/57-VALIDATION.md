---
phase: 57
slug: daemon-daemon-daemon-daemon
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-25
---

# Phase 57 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                       |
| ---------------------- | ------------------------------------------- |
| **Framework**          | cargo test (Rust) + vitest (Frontend)       |
| **Config file**        | `src-tauri/Cargo.toml` / `vitest.config.ts` |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon`   |
| **Full suite command** | `cd src-tauri && cargo test`                |
| **Estimated runtime**  | ~30 seconds                                 |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command                         | File Exists | Status     |
| ------- | ---- | ---- | ----------- | --------- | ----------------------------------------- | ----------- | ---------- |
| TBD     | TBD  | TBD  | TBD         | unit      | `cd src-tauri && cargo test -p uc-daemon` | ⬜          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- Existing infrastructure covers all phase requirements.

---

## Manual-Only Verifications

| Behavior                                                  | Requirement | Why Manual                                         | Test Instructions                                                      |
| --------------------------------------------------------- | ----------- | -------------------------------------------------- | ---------------------------------------------------------------------- |
| Clipboard watcher captures OS clipboard changes in daemon | D-02        | Requires running daemon + OS clipboard interaction | Start daemon, copy text to clipboard, verify daemon logs capture event |
| GUI receives clipboard updates via WS                     | D-06        | Requires running daemon + GUI                      | Start daemon + GUI, copy text, verify GUI clipboard list updates       |
| Write-back loop prevention                                | D-09        | Requires daemon clipboard write + observation      | Daemon writes to clipboard, verify no infinite loop triggered          |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
