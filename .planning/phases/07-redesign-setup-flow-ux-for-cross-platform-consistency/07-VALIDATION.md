---
phase: 7
slug: redesign-setup-flow-ux-for-cross-platform-consistency
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 7 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                                  |
| ---------------------- | ------------------------------------------------------------------------------------------------------ |
| **Framework**          | Vitest 4.x + @testing-library/react                                                                    |
| **Config file**        | `vite.config.ts` (test section)                                                                        |
| **Quick run command**  | `bunx vitest run src/pages/__tests__/SetupFlow.test.tsx src/pages/setup/__tests__/ --reporter=verbose` |
| **Full suite command** | `bunx vitest run --reporter=verbose`                                                                   |
| **Estimated runtime**  | ~15 seconds                                                                                            |

---

## Sampling Rate

- **After every task commit:** Run `bunx vitest run src/pages/ --reporter=verbose`
- **After every plan wave:** Run `bunx vitest run --reporter=verbose`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type  | Automated Command                                                          | File Exists        | Status     |
| -------- | ---- | ---- | ----------- | ---------- | -------------------------------------------------------------------------- | ------------------ | ---------- |
| 07-01-01 | 01   | 1    | UX-01       | unit       | `bunx vitest run src/pages/setup/__tests__/StepLayout.test.tsx -x`         | No -- Wave 0       | ⬜ pending |
| 07-01-02 | 01   | 1    | UX-02       | unit       | `bunx vitest run src/pages/setup/__tests__/StepLayout.test.tsx -x`         | No -- Wave 0       | ⬜ pending |
| 07-01-03 | 01   | 1    | UX-04       | unit       | `bunx vitest run src/pages/setup/__tests__/StepDotIndicator.test.tsx -x`   | No -- Wave 0       | ⬜ pending |
| 07-01-04 | 01   | 1    | UX-07       | unit       | `bunx vitest run src/pages/setup/__tests__/ProcessingJoinStep.test.tsx -x` | No -- Wave 0       | ⬜ pending |
| 07-02-01 | 02   | 1    | UX-03       | unit       | `bunx vitest run src/pages/setup/__tests__/welcome-layout.test.tsx -x`     | Yes (needs update) | ⬜ pending |
| 07-02-02 | 02   | 1    | UX-06       | unit       | `bunx vitest run src/pages/__tests__/SetupFlow.test.tsx -x`                | Yes (needs update) | ⬜ pending |
| 07-02-03 | 02   | 1    | UX-05       | smoke      | `grep -r "lg:" src/pages/setup/ src/pages/SetupPage.tsx` (0 matches)       | Manual             | ⬜ pending |
| 07-03-01 | 03   | 2    | UX-08       | regression | `bunx vitest run src/pages/ --reporter=verbose -x`                         | Yes                | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src/pages/setup/__tests__/StepLayout.test.tsx` — stubs for UX-01, UX-02
- [ ] `src/pages/setup/__tests__/StepDotIndicator.test.tsx` — stubs for UX-04
- [ ] `src/pages/setup/__tests__/ProcessingJoinStep.test.tsx` — stubs for UX-07

_Existing infrastructure covers UX-03, UX-05, UX-06, UX-08._

---

## Manual-Only Verifications

| Behavior                           | Requirement | Why Manual                                                                     | Test Instructions                                                                 |
| ---------------------------------- | ----------- | ------------------------------------------------------------------------------ | --------------------------------------------------------------------------------- |
| No `lg:` breakpoints in setup flow | UX-05       | Grep-based smoke check, not a test framework assertion                         | Run `grep -r "lg:" src/pages/setup/ src/pages/SetupPage.tsx` and verify 0 matches |
| Cross-platform visual consistency  | UX-ALL      | Visual rendering differences cannot be automated without screenshot comparison | Build and run on Windows, macOS, Linux; verify identical layout                   |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
