---
phase: 15
slug: clipboard-management-command-wiring
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 15 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                           |
| ---------------------- | ------------------------------- |
| **Framework**          | vitest                          |
| **Config file**        | vite.config.ts + vitest section |
| **Quick run command**  | `bun test`                      |
| **Full suite command** | `bun run test:coverage`         |
| **Estimated runtime**  | ~120 seconds                    |

---

## Sampling Rate

- **After every task commit:** Run `bun test src-tauri` or most specific affected test file
- **After every plan wave:** Run `bun test`
- **Before `/gsd:verify-work`:** Full suite (`bun run test:coverage`) must be green
- **Max feedback latency:** 300 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type                     | Automated Command                                          | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------------------------- | ---------------------------------------------------------- | ----------- | ---------- |
| 15-01-01 | 01   | 1    | CONTRACT-03 | rust unit + dto serialization | `cd src-tauri && cargo test clipboard_commands::stats`     | ❌ W0       | ⬜ pending |
| 15-01-02 | 01   | 1    | CONTRACT-03 | rust unit + dto serialization | `cd src-tauri && cargo test clipboard_commands::favorites` | ❌ W0       | ⬜ pending |
| 15-01-03 | 01   | 1    | CONTRACT-03 | frontend contract test        | `bun test src/api/clipboardItems.test.ts`                  | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-tauri/src/commands/clipboard_stats.rs` — tests for DTO + command behavior
- [ ] `src-tauri/crates/uc-tauri/src/commands/clipboard_favorites.rs` — tests for DTO + command behavior
- [ ] `src/api/clipboardItems.test.ts` — vitest contract tests for stats/item/favorite wiring
- [ ] `bun run test:coverage` — ensure CONTRACT-03 coverage includes new commands

_If none: "Existing infrastructure covers all phase requirements."_

---

## Manual-Only Verifications

| Behavior                          | Requirement | Why Manual                                                                      | Test Instructions                                                                                                                                               |
| --------------------------------- | ----------- | ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Stats in passive mode stay stable | CONTRACT-03 | Requires real clipboard traffic and passive mode switch                         | Start app in passive mode, perform local copy operations, confirm stats do not increment when passive, switch back to active and confirm stats resume updating. |
| Favorites UX across devices       | CONTRACT-03 | Multi-device behavior with encryption session is hard to simulate automatically | Run two devices with the same profile, favorite/unfavorite entries on one side and confirm UI + behavior on the other side stay in sync.                        |

_If none: "All phase behaviors have automated verification."_

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 300s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
