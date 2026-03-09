---
phase: 17
slug: dynamic-theme-switching-with-shadcn-preset-import-and-enhanced-color-preview
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-08
---

# Phase 17 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                 |
| ---------------------- | --------------------------------------------------------------------- |
| **Framework**          | Vitest ^4.0.17                                                        |
| **Config file**        | No standalone `vitest.config.*`; uses package.json `"test": "vitest"` |
| **Quick run command**  | `bun run test --run`                                                  |
| **Full suite command** | `bun run test --run && bun run build`                                 |
| **Estimated runtime**  | ~45 seconds                                                           |

---

## Sampling Rate

- **After every task commit:** Run `bun run test --run`
- **After every plan wave:** Run `bun run test --run && bun run build`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 45 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement                                                             | Test Type | Automated Command                                                                | File Exists | Status     |
| ------- | ---- | ---- | ----------------------------------------------------------------------- | --------- | -------------------------------------------------------------------------------- | ----------- | ---------- |
| P17-01  | 01   | 1    | TS preset registry (light/dark token sets)                              | unit      | `bun run test --run src/lib/__tests__/theme-engine.test.ts`                      | ❌ W0       | ⬜ pending |
| P17-02  | 01   | 1    | Runtime injection applies selected preset by mode                       | unit      | `bun run test --run src/lib/__tests__/theme-engine.test.ts`                      | ❌ W0       | ⬜ pending |
| P17-03  | 01   | 1    | Static theme CSS imports removed from globals.css                       | unit      | `bun run test --run src/styles/__tests__/theme-migration.test.ts`                | ❌ W0       | ⬜ pending |
| P17-05  | 01   | 1    | Persisted `theme_color` fallback and startup apply behavior             | unit      | `bun run test --run src/contexts/__tests__/SettingContext.theme.test.tsx`        | ❌ W0       | ⬜ pending |
| P17-06  | 01   | 1    | Transition CSS exists for token-driven surfaces                         | unit      | `bun run test --run src/styles/__tests__/theme-migration.test.ts`                | ❌ W0       | ⬜ pending |
| P17-04  | 02   | 2    | Appearance swatch is 3-4 dot preview per preset                         | component | `bun run test --run src/components/setting/__tests__/AppearanceSection.test.tsx` | ❌ W0       | ⬜ pending |
| P17-07  | 02   | 2    | Swatch interaction calls updateGeneralSetting and contract stays stable | component | `bun run test --run src/components/setting/__tests__/AppearanceSection.test.tsx` | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src/lib/__tests__/theme-engine.test.ts` — runtime token injection and fallback behavior
- [ ] `src/contexts/__tests__/SettingContext.theme.test.tsx` — mode + persisted theme application
- [ ] `src/styles/__tests__/theme-migration.test.ts` — global CSS migration assertions
- [ ] `src/components/setting/__tests__/AppearanceSection.test.tsx` — swatch rendering + click persistence contract

_If none: "Existing infrastructure covers all phase requirements."_

---

## Manual-Only Verifications

| Behavior                                                                      | Requirement | Why Manual                                                                                          | Test Instructions                                                                                                                                         |
| ----------------------------------------------------------------------------- | ----------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Startup visual continuity without flash when opening app with persisted theme | P17-05      | JSDOM tests cannot reliably validate first paint behavior                                           | 1. Set a non-default theme color. 2. Restart app. 3. Verify first visible frame is not fallback white/black flash and final colors match selected preset. |
| Transition feel and responsiveness (~200ms) while switching theme presets     | P17-06      | Perceived UX smoothness is subjective and timing in tests is not equivalent to real render pipeline | 1. Open Settings -> Appearance. 2. Switch several presets in both light/dark modes. 3. Verify transition is smooth and not sluggish.                      |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 45s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
