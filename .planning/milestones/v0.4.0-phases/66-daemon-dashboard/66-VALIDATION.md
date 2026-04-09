---
phase: 66
slug: daemon-dashboard
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-27
---

# Phase 66 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                             |
| ---------------------- | ------------------------------------------------- |
| **Framework**          | cargo test (Rust) + vitest (Frontend)             |
| **Config file**        | `src-tauri/Cargo.toml` / `vitest.config.ts`       |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon`         |
| **Full suite command** | `cd src-tauri && cargo test && cd .. && bun test` |
| **Estimated runtime**  | ~30 seconds                                       |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon`
- **After every plan wave:** Run `cd src-tauri && cargo test && cd .. && bun test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                            | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | -------------------------------------------- | ----------- | ---------- |
| 66-01-01 | 01   | 1    | D-03/D-04   | unit        | `cd src-tauri && cargo test -p uc-daemon ws` | ⬜ W0       | ⬜ pending |
| 66-01-02 | 01   | 1    | D-01/D-02   | unit        | `cd src-tauri && cargo test -p uc-daemon`    | ⬜ W0       | ⬜ pending |
| 66-02-01 | 02   | 2    | D-05/D-06   | integration | `bun test -- src/hooks`                      | ⬜ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] Test stubs for `is_supported_topic()` completeness validation
- [ ] Test stubs for `build_snapshot_event()` exhaustiveness
- [ ] Frontend hook test for reconnection refresh trigger

_Existing infrastructure covers most phase requirements — Wave 0 adds targeted stubs._

---

## Manual-Only Verifications

| Behavior                                                 | Requirement | Why Manual                    | Test Instructions                                                     |
| -------------------------------------------------------- | ----------- | ----------------------------- | --------------------------------------------------------------------- |
| Dashboard auto-refresh on clipboard change (daemon mode) | D-03/D-04   | Requires running daemon + GUI | 1. Start daemon, 2. Copy text, 3. Verify Dashboard updates            |
| Reconnection compensation refresh                        | D-05/D-06   | Requires network disruption   | 1. Start daemon+GUI, 2. Kill daemon WS, 3. Restart, 4. Verify refresh |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
