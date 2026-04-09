---
phase: 64
slug: tauri-sync-retirement
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-26
---

# Phase 64 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                             |
| ---------------------- | ------------------------------------------------- |
| **Framework**          | cargo test (Rust) + vitest (Frontend)             |
| **Config file**        | `src-tauri/Cargo.toml` / `vitest.config.ts`       |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-tauri`          |
| **Full suite command** | `cd src-tauri && cargo test && cd .. && bun test` |
| **Estimated runtime**  | ~120 seconds                                      |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-tauri`
- **After every plan wave:** Run `cd src-tauri && cargo test && cd .. && bun test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                        | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | ---------------------------------------- | ----------- | ---------- |
| 64-01-01 | 01   | 1    | N/A         | compilation | `cd src-tauri && cargo check`            | N/A         | ⬜ pending |
| 64-01-02 | 01   | 1    | N/A         | unit        | `cd src-tauri && cargo test -p uc-tauri` | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

_Existing infrastructure covers all phase requirements — removal/cleanup phase uses existing test suites to verify no regressions._

---

## Manual-Only Verifications

| Behavior                                   | Requirement | Why Manual             | Test Instructions                                                            |
| ------------------------------------------ | ----------- | ---------------------- | ---------------------------------------------------------------------------- |
| Clipboard sync works end-to-end via daemon | N/A         | Requires two devices   | Start peerA (full mode) + peerB (passive mode), copy on A, verify paste on B |
| File sync works end-to-end via daemon      | N/A         | Requires file transfer | Copy file on A, verify file arrives on B                                     |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
