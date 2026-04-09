---
phase: 37-wiring-decomposition
plan: '04'
subsystem: ui
tags: [react, pairing, race-condition, testing, vitest]

requires:
  - phase: 37-03
    provides: assembly.rs and AppHandle removal from start_background_tasks

provides:
  - Synchronous activeSessionIdRef write before acceptP2PPairing eliminates verification event race
  - Regression tests covering accept->immediate verification timing in PairingDialog.test.tsx

affects:
  - 38-coreruntime-extraction

tech-stack:
  added: []
  patterns:
    - 'Synchronous ref write before async call pattern: when a ref must be current before an async microtask/event fires, write the ref synchronously before issuing the call rather than relying on useEffect to sync it afterward'

key-files:
  created: []
  modified:
    - src/components/PairingNotificationProvider.tsx
    - src/components/__tests__/PairingDialog.test.tsx

key-decisions:
  - 'Synchronously write activeSessionIdRef.current before calling acceptP2PPairing to close the race window — useEffect-based ref sync is too late when backend emits verification immediately'
  - 'Roll back both the ref and the state on acceptP2PPairing failure to avoid leaving a stale session that would incorrectly process future events'
  - 'Add regression tests in existing PairingDialog.test.tsx rather than a new file to keep the test target unified and avoid verification target divergence'

patterns-established:
  - 'Race-free ref pattern: for refs that guard event handlers, always write the ref synchronously in the event-triggering code path rather than relying on useEffect to propagate state changes'

requirements-completed:
  - RNTM-02

duration: 15min
completed: '2026-03-18'
---

# Phase 37 Plan 04: PairingNotificationProvider Accept->Verification Race Fix Summary

**Synchronous activeSessionIdRef write before acceptP2PPairing eliminates silent verification event discard on peerA's PIN dialog, with regression tests covering the accept->immediate verification timing scenario**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-03-18T02:20:00Z
- **Completed:** 2026-03-18T02:35:00Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Fixed the race condition in `PairingNotificationProvider` where clicking Accept set `activeSessionId` via state (async), causing the `useEffect`-driven ref sync to arrive too late when the backend emitted a `verification` event immediately after `acceptP2PPairing`
- Added synchronous `activeSessionIdRef.current = event.sessionId` write before calling `acceptP2PPairing`, closing the window entirely
- Added rollback for both `activeSessionIdRef.current` and `activeSessionId` state on `acceptP2PPairing` failure to prevent stale session leaks
- Added two regression tests in `PairingDialog.test.tsx` covering: (1) immediate verification after accept showing PIN dialog, and (2) failed accept rolling back session so subsequent verification is ignored

## Task Commits

1. **Task 1: 同步建立 active session，消除 accept 时序竞态** - `047aca2f` (fix)
2. **Task 2: 新增 accept 后 immediate verification 回归测试** - `4f283081` (test)

## Files Created/Modified

- `src/components/PairingNotificationProvider.tsx` - Added synchronous ref write and failure rollback in the Accept click handler
- `src/components/__tests__/PairingDialog.test.tsx` - Extended mock to include `acceptP2PPairing`, `rejectP2PPairing`, `onSpaceAccessCompleted`, and sonner `toast` with `.error()` sub-method; added `PairingNotificationProvider` describe block with 2 regression tests

## Decisions Made

- **Synchronous ref write before async call**: The race window exists because `setActiveSessionId` schedules a state update, then `useEffect` observes it and sets the ref — this takes at least one render cycle. Writing the ref synchronously closes the window with zero additional complexity.
- **Dual rollback on failure**: On `acceptP2PPairing` rejection, both `activeSessionIdRef.current = null` and `setActiveSessionId(null)` are called to keep ref and state in sync; omitting either would leave a dangling session.
- **Tests in existing file**: Plan explicitly required tests in `PairingDialog.test.tsx`; adding a second describe block keeps verification co-located with related mock setup.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added `toast.error` and `toast.success` to sonner mock**

- **Found during:** Task 2 (writing regression tests)
- **Issue:** `vi.mock('sonner', () => ({ toast: vi.fn() }))` mocked `toast` as a bare function, but `PairingNotificationProvider` calls `toast.error()` on accept failure — the mock lacked sub-methods, causing an unhandled `TypeError` in tests
- **Fix:** Extended the mock to produce a function with `.error` and `.success` vi.fn() properties attached before export
- **Files modified:** `src/components/__tests__/PairingDialog.test.tsx`
- **Verification:** All 3 tests pass (`vitest run`)
- **Committed in:** `4f283081` (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (Rule 2 — missing critical mock method)
**Impact on plan:** Fix necessary for test correctness; no scope creep.

## Issues Encountered

- `bun test` uses Bun's native test runner which does not support `vi.hoisted`; tests must be run with `npx vitest run` as configured in `package.json` (`"test": "vitest"`)

## Next Phase Readiness

- peerA's accept->verification race is closed; UAT Test 3 root cause addressed
- PIN dialog should now reliably appear on peerA after clicking Accept
- Phase 38 (CoreRuntime extraction) can proceed; no blockers from this plan

---

_Phase: 37-wiring-decomposition_
_Completed: 2026-03-18_
