---
phase: 34-optimize-joinpickdevice-page-event-driven-discovery-with-scanning-ux
plan: 01
subsystem: ui
tags: [react, hooks, tauri, p2p, i18n, testing, vitest]

# Dependency graph
requires:
  - phase: 34-optimize-joinpickdevice-page-event-driven-discovery-with-scanning-ux
    provides: CONTEXT.md and RESEARCH.md with design decisions and API contracts

provides:
  - useDeviceDiscovery hook with event-driven peer discovery (3 Tauri listeners + initial load + 10s timeout)
  - ScanPhase state machine (scanning | hasDevices | empty)
  - Updated JoinPickDeviceStepProps with scanPhase/onRescan/DiscoveredPeer
  - Updated JoinPickDeviceStep and SetupPage using new hook
  - 11 passing hook unit tests
  - CSS ripple animation (.animate-ripple)
  - Bilingual i18n keys for scanning compact indicator and troubleshooting tips

affects:
  - 34-02 (UI components that consume the hook and scan phase)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - useRef for stable callbacks (synced via useEffect) to avoid effect re-subscriptions
    - Active-guard pattern (cancelled boolean) for async cleanup in event listener hooks
    - ScanPhase state machine: scanning -> hasDevices | empty, with reset on deactivation

key-files:
  created:
    - src/hooks/useDeviceDiscovery.ts
    - src/hooks/__tests__/useDeviceDiscovery.test.ts
  modified:
    - src/pages/setup/types.ts
    - src/pages/setup/JoinPickDeviceStep.tsx
    - src/pages/SetupPage.tsx
    - src/pages/setup/__tests__/joinPickDeviceErrorMessage.test.tsx
    - src/pages/setup/__tests__/joinPickPeerIdDisplay.test.tsx
    - src/App.css
    - src/i18n/locales/en-US.json
    - src/i18n/locales/zh-CN.json

key-decisions:
  - 'useDeviceDiscovery stores raw deviceName (string | null) from backend — no fallback mapping in hook'
  - 'onError callback stored in useRef synced via useEffect (not during render) to satisfy react-hooks/refs ESLint rule'
  - 'Hook effect depends only on [active] — no teardown on options identity change'
  - 'Hook does NOT import sonner/toast — all error UI delegated to caller via onError'
  - 'Deactivation (active=false) triggers state reset (peers=[], scanPhase=scanning) in effect cleanup for clean re-entry'
  - 'SetupPage migrated from 3s polling interval to event-driven useDeviceDiscovery hook'

patterns-established:
  - 'Pattern: useRef for stable callbacks — sync in useEffect, not during render'
  - 'Pattern: active-guard boolean (cancelled) for async listener setup/teardown'

requirements-completed: [SCAN-01, SCAN-02, SCAN-03, SCAN-04]

# Metrics
duration: 19min
completed: 2026-03-16
---

# Phase 34 Plan 01: useDeviceDiscovery Hook Foundation Summary

**Event-driven device discovery hook with ScanPhase state machine, replacing 3s polling with 3 Tauri listeners, 11 unit tests, CSS ripple animation, and bilingual i18n keys**

## Performance

- **Duration:** 19 min
- **Started:** 2026-03-16T05:43:35Z
- **Completed:** 2026-03-16T06:02:40Z
- **Tasks:** 3
- **Files modified:** 9

## Accomplishments

- Created `useDeviceDiscovery` hook with event-driven discovery (onP2PPeerDiscoveryChanged, onP2PPeerConnectionChanged, onP2PPeerNameUpdated) and initial load via getP2PPeers, with 10-second empty-state timeout
- Migrated SetupPage from 3-second polling interval to event-driven hook; updated JoinPickDeviceStep to use ScanPhase state machine instead of isScanningInitial boolean
- Wrote 11 passing hook unit tests covering: initial load, timeout transition, event-driven updates, re-entry reset, error handling with onError callback, cleanup on unmount, and raw deviceName storage

## Task Commits

Each task was committed atomically:

1. **Task 1: Create useDeviceDiscovery hook and update types** - `8e055f06` (feat)
2. **Task 2: Write useDeviceDiscovery hook unit tests** - `a4fe7913` (test)
3. **Task 3: Add CSS ripple animation and i18n keys** - `ef8b0cfd` (feat)
4. **Fix: Remove unused test variables** - `b3c1a1af` (fix)

## Files Created/Modified

- `src/hooks/useDeviceDiscovery.ts` - New hook: event-driven discovery with ScanPhase state machine
- `src/hooks/__tests__/useDeviceDiscovery.test.ts` - 11 unit tests for hook state machine
- `src/pages/setup/types.ts` - Updated JoinPickDeviceStepProps with scanPhase/onRescan/DiscoveredPeer; re-exports ScanPhase
- `src/pages/setup/JoinPickDeviceStep.tsx` - Migrated from isScanningInitial to scanPhase state machine; fallback name in render layer
- `src/pages/SetupPage.tsx` - Replaced polling logic with useDeviceDiscovery hook
- `src/pages/setup/__tests__/joinPickDeviceErrorMessage.test.tsx` - Updated to new prop interface (onRescan, scanPhase)
- `src/pages/setup/__tests__/joinPickPeerIdDisplay.test.tsx` - Updated to new prop interface (deviceName, scanPhase=hasDevices)
- `src/App.css` - Added @keyframes ripple-out and .animate-ripple class
- `src/i18n/locales/en-US.json` - Added scanning.compact and empty.tips keys
- `src/i18n/locales/zh-CN.json` - Added scanning.compact and empty.tips keys

## Decisions Made

- Raw deviceName from backend stored as `string | null` in hook — render layer applies localized fallback via `tCommon('unknownDevice')`
- `onError` stored in `useRef` and synced via `useEffect` (not during render) to satisfy `react-hooks/refs` ESLint rule
- Effect depends only on `[active]` — options identity changes do not cause listener teardown/re-subscribe
- Hook explicitly does not import sonner — separation of concerns between hook (data) and UI (toast)
- Deactivation reset: cleanup function sets peers=[] and scanPhase=scanning before removing listeners, preventing stale state on re-entry

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed TypeScript/ESLint errors in dependent files after interface update**

- **Found during:** Task 1 (Create useDeviceDiscovery hook and update types)
- **Issue:** JoinPickDeviceStep.tsx, SetupPage.tsx, and 2 test files used the old interface (onRefresh, isScanningInitial, name field on peers)
- **Fix:** Updated all affected files to use new interface (onRescan, scanPhase, DiscoveredPeer with deviceName). Migrated SetupPage from polling to useDeviceDiscovery hook
- **Files modified:** src/pages/setup/JoinPickDeviceStep.tsx, src/pages/SetupPage.tsx, both test files
- **Verification:** npx tsc --noEmit passes with 0 errors
- **Committed in:** 8e055f06 (Task 1 commit)

**2. [Rule 3 - Blocking] Fixed react-hooks/refs ESLint error for ref mutation during render**

- **Found during:** Task 1 commit (pre-commit hook)
- **Issue:** `onErrorRef.current = options?.onError` directly in render body violates react-hooks/refs rule
- **Fix:** Moved ref sync into a `useEffect(() => { onErrorRef.current = options?.onError })` call
- **Files modified:** src/hooks/useDeviceDiscovery.ts
- **Verification:** Pre-commit hook passes
- **Committed in:** 8e055f06

**3. [Rule 1 - Bug] Fixed TypeScript unused variable errors in test file**

- **Found during:** Task 2 cleanup (final tsc check)
- **Issue:** `_capturedConnectionCb` and `_capturedNameCb` declared but never read — TypeScript noUnusedLocals
- **Fix:** Removed unused variables; connection/name callback capture not needed for test assertions
- **Files modified:** src/hooks/**tests**/useDeviceDiscovery.test.ts
- **Verification:** npx tsc --noEmit passes
- **Committed in:** b3c1a1af

---

**Total deviations:** 3 auto-fixed (2 Rule 1 bug-fixes, 1 Rule 3 blocking)
**Impact on plan:** All auto-fixes necessary for correctness and build compliance. No scope creep.

## Issues Encountered

- `vi.useFakeTimers()` at describe-level caused all `waitFor` calls to timeout (waitFor uses real setTimeout internally). Fixed by moving fake timers to individual timeout-testing tests only.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Hook, types, CSS, and i18n foundation complete
- Plan 02 can now build the updated JoinPickDeviceStep UI consuming scanPhase and DiscoveredPeer
- ripple animation class (.animate-ripple) and troubleshooting tips i18n keys ready for use in UI components

---

_Phase: 34-optimize-joinpickdevice-page-event-driven-discovery-with-scanning-ux_
_Completed: 2026-03-16_

## Self-Check: PASSED

- FOUND: src/hooks/useDeviceDiscovery.ts
- FOUND: src/hooks/**tests**/useDeviceDiscovery.test.ts
- FOUND: .planning/phases/34-.../34-01-SUMMARY.md
- FOUND commits: 8e055f06, a4fe7913, ef8b0cfd, b3c1a1af
- No sonner import in hook
- CSS ripple-out animation present in App.css
- TypeScript: 0 errors
- 11 tests passing
