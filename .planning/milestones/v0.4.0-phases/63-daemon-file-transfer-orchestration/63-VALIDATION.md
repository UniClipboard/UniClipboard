---
phase: 63
slug: daemon-file-transfer-orchestration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-26
---

# Phase 63 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                     |
| ---------------------- | ----------------------------------------- |
| **Framework**          | cargo test (Rust)                         |
| **Config file**        | src-tauri/Cargo.toml                      |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon` |
| **Full suite command** | `cd src-tauri && cargo test`              |
| **Estimated runtime**  | ~60 seconds                               |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type | Automated Command                         | File Exists | Status     |
| -------- | ---- | ---- | ----------- | --------- | ----------------------------------------- | ----------- | ---------- |
| 63-01-01 | 01   | 1    | TBD         | unit      | `cd src-tauri && cargo test -p uc-daemon` | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- Existing infrastructure covers all phase requirements.

---

## Manual-Only Verifications

| Behavior                           | Requirement | Why Manual                        | Test Instructions                                                     |
| ---------------------------------- | ----------- | --------------------------------- | --------------------------------------------------------------------- |
| File transfer lifecycle end-to-end | TBD         | Requires two peers with file sync | Start peerA and peerB, copy file on A, verify transfer completes on B |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
