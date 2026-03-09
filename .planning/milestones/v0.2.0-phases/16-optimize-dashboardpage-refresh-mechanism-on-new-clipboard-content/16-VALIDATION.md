---
phase: 16
slug: optimize-dashboardpage-refresh-mechanism-on-new-clipboard-content
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-08
---

# Phase 16 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                            |
| ---------------------- | ---------------------------------------------------------------- |
| **Framework**          | Vitest ^4.0.17 (frontend), cargo test (backend)                  |
| **Config file**        | No vitest.config.\* found — uses package.json `"test": "vitest"` |
| **Quick run command**  | `bun run test --run`                                             |
| **Full suite command** | `bun run test --run && cd src-tauri && cargo test`               |
| **Estimated runtime**  | ~30 seconds                                                      |

---

## Sampling Rate

- **After every task commit:** Run `bun run test --run`
- **After every plan wave:** Run `bun run test --run && cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement                                          | Test Type   | Automated Command                                                | File Exists | Status     |
| ------- | ---- | ---- | ---------------------------------------------------- | ----------- | ---------------------------------------------------------------- | ----------- | ---------- |
| P16-01  | 01   | 1    | prependItem reducer dedup + insert at head           | unit        | `bun run test src/store/slices/__tests__/clipboardSlice.test.ts` | ❌ W0       | ⬜ pending |
| P16-02  | 01   | 1    | removeItem reducer removes by entry_id               | unit        | `bun run test src/store/slices/__tests__/clipboardSlice.test.ts` | ❌ W0       | ⬜ pending |
| P16-03  | 01   | 1    | get_clipboard_entry backend command                  | integration | `cd src-tauri && cargo test get_clipboard_entry`                 | ❌ W0       | ⬜ pending |
| P16-04  | 01   | 1    | ClipboardEvent NewContent with origin field          | unit        | `cd src-tauri && cargo test clipboard_event`                     | Partial     | ⬜ pending |
| P16-05  | 02   | 2    | useClipboardEvents routes local to prepend           | unit        | `bun run test src/hooks/__tests__/useClipboardEvents.test.ts`    | ❌ W0       | ⬜ pending |
| P16-06  | 02   | 2    | useClipboardEvents routes remote to throttled reload | unit        | `bun run test src/hooks/__tests__/useClipboardEvents.test.ts`    | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src/store/slices/__tests__/clipboardSlice.test.ts` — stubs for P16-01, P16-02
- [ ] `src/hooks/__tests__/useClipboardEvents.test.ts` — stubs for P16-05, P16-06
- [ ] Backend test for `get_clipboard_entry` command in clipboard.rs test module — covers P16-03
- [ ] Update existing `ClipboardEvent` serde test to cover `origin` field — covers P16-04

_If none: "Existing infrastructure covers all phase requirements."_

---

## Manual-Only Verifications

| Behavior                             | Requirement     | Why Manual                                  | Test Instructions                                                                                                |
| ------------------------------------ | --------------- | ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Scroll position preserved on prepend | Scroll behavior | DOM scroll state not testable in unit tests | 1. Open Dashboard, scroll down. 2. Copy text to clipboard. 3. Verify scroll position unchanged, new item at top. |
| Remote sync batches throttled        | Remote throttle | Requires multi-device sync setup            | 1. Sync 5+ items from remote device rapidly. 2. Verify single full reload (not 5 separate queries).              |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
