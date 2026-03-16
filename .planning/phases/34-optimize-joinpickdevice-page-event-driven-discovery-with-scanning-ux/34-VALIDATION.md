---
phase: 34
slug: optimize-joinpickdevice-page-event-driven-discovery-with-scanning-ux
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-16
---

# Phase 34 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                           |
| ---------------------- | --------------------------------------------------------------- |
| **Framework**          | Vitest ^4.0.17 + @testing-library/react ^16.3.2                 |
| **Config file**        | `vite.config.ts` (test section), setup at `src/test/setup.ts`   |
| **Quick run command**  | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run` |
| **Full suite command** | `bun test --run`                                                |
| **Estimated runtime**  | ~15 seconds                                                     |

---

## Sampling Rate

- **After every task commit:** Run `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`
- **After every plan wave:** Run `bun test --run`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement           | Test Type | Automated Command                                                          | File Exists  | Status     |
| -------- | ---- | ---- | --------------------- | --------- | -------------------------------------------------------------------------- | ------------ | ---------- |
| 34-01-01 | 01   | 1    | hook-initial-load     | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ W0        | ⬜ pending |
| 34-01-02 | 01   | 1    | hook-discovery-add    | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ W0        | ⬜ pending |
| 34-01-03 | 01   | 1    | hook-discovery-remove | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ W0        | ⬜ pending |
| 34-01-04 | 01   | 1    | hook-scan-timeout     | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ W0        | ⬜ pending |
| 34-01-05 | 01   | 1    | hook-reset-scan       | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ W0        | ⬜ pending |
| 34-01-06 | 01   | 1    | hook-late-device      | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ W0        | ⬜ pending |
| 34-02-01 | 02   | 1    | ripple-animation      | unit      | `bun test src/pages/setup/__tests__/ --run`                                | ❌ W0        | ⬜ pending |
| 34-02-02 | 02   | 1    | polling-rewrite       | unit      | `bun test src/pages/__tests__/setup-peer-discovery-polling.test.tsx --run` | ✅ (rewrite) | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src/hooks/__tests__/useDeviceDiscovery.test.ts` — stubs for all hook behaviors (new file)
- [ ] `src/pages/__tests__/setup-peer-discovery-polling.test.tsx` — REWRITE to test event-driven behavior
- [ ] `src/pages/setup/__tests__/JoinPickDeviceStepScanning.test.tsx` — optional: scanning phase rendering

_Existing infrastructure (Vitest, @testing-library/react, framer-motion mock pattern) covers all needs — no new framework install required._

---

## Manual-Only Verifications

| Behavior                                | Requirement         | Why Manual                                      | Test Instructions                                                                |
| --------------------------------------- | ------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------- |
| Pulse ripple animation visual quality   | scanning-animation  | CSS animation rendering not testable with jsdom | Visually verify concentric rings expand smoothly in dev mode                     |
| Shrink-to-compact transition smoothness | scanning-transition | Framer Motion layout animation timing is visual | Verify transition from full pulse to compact indicator when first device appears |
| Device fade-in/out animation timing     | device-animation    | Animation timing is visual                      | Add/remove peers and verify smooth entry/exit animations                         |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
