---
phase: 83
slug: toast
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 83 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                         |
| ---------------------- | ----------------------------- |
| **Framework**          | vitest (jest-compatible)      |
| **Config file**        | `vitest.config.ts` (existing) |
| **Quick run command**  | `bun test`                    |
| **Full suite command** | `bun test --run`              |
| **Estimated runtime**  | ~60 seconds                   |

---

## Sampling Rate

- **After every task commit:** Run `bun test`
- **After every plan wave:** Run `bun test --run`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement         | Test Type | Automated Command                                           | File Exists | Status     |
| -------- | ---- | ---- | ------------------- | --------- | ----------------------------------------------------------- | ----------- | ---------- |
| 83-01-01 | 01   | 1    | D-01/D-02           | unit      | `bun test src/__tests__/App.pairing-notifications.test.tsx` | ✅ W0       | ⬜ pending |
| 83-01-02 | 01   | 1    | D-10/D-11/D-12      | unit      | `bun test src/hooks/__tests__/useDaemonEvents.test.ts`      | ✅ W0       | ⬜ pending |
| 83-01-03 | 01   | 1    | D-04/D-05/D-06      | unit      | `bun test src/store/slices/devicesSlice.test.ts`            | ❌ W0       | ⬜ pending |
| 83-01-04 | 01   | 1    | D-07/D-08/D-09      | unit      | `bun test src/hooks/__tests__/useSetupFlow.test.ts`         | ❌ W0       | ⬜ pending |
| 83-02-01 | 02   | 2    | D-13/D-14/D-15/D-16 | unit      | `bun test src/api/__tests__/p2p-realtime-contract.test.ts`  | ✅ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src/store/slices/__tests__/devicesSlice.test.ts` — tests for discoveredPeers migration
- [ ] `src/hooks/__tests__/useSetupFlow.test.ts` — tests for SetupPage logic extraction
- [ ] `src/api/daemon/__tests__/events.test.ts` — tests for diffPeerSnapshots utility
- [ ] `src/hooks/__tests__/useDeviceDiscovery.test.ts` — updated tests for Redux-based discoveredPeers
- [ ] `src/hooks/__tests__/usePairingEvents.test.ts` — updated tests for type-safe payloads
- [ ] `vitest.config.ts` update if needed for coverage paths

---

## Manual-Only Verifications

| Behavior                  | Requirement    | Why Manual                                        | Test Instructions                                             |
| ------------------------- | -------------- | ------------------------------------------------- | ------------------------------------------------------------- |
| Pairing flow end-to-end   | D-01/D-02      | Requires daemon + WS; cannot unit test pairing UX | Manual: Start app, trigger pairing, verify notification fires |
| SetupPage step navigation | D-07/D-08/D-09 | Complex async state transitions                   | Manual: Go through full setup flow, verify steps advance      |
| discoveredPeers UI update | D-04/D-05/D-06 | Requires peer discovery on network                | Manual: Start another peer, verify appears in devices list    |

_If none: "All phase behaviors have automated verification."_

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
