---
phase: 85-improve-pairing-observability-across-daemon-event-routing-and-ui-state-transitions
plan: '02'
subsystem: observability
tags: [frontend, pairing, setup, diagnostics, realtime, structured-logging]

requires:
  - phase: 85-01
    provides: PairingRoutingRecord backend contract and session-centered daemon emission logs

provides:
  - logPairingRouting() shared routing diagnostic helper at the useDaemonEvents hook boundary
  - logProviderDecision() session-aware pairing lifecycle observability in PairingNotificationProvider
  - logSetupRouting() applied/dropped/session_switched decisions in onSetupStateChanged
  - logStoreDecision() started/running/skipped/scheduled/failure/space_access_ignored in setupRealtimeStore

affects:
  - 85-03
  - any future pairing or setup UI debugging work

tech-stack:
  added: []
  patterns:
    - 'logPairingRouting(): routed/ignored/unsupported at hook boundary — single helper over repeated inline logs'
    - 'logProviderDecision(): accepted/rejected/ignored/canceled/success/failure per session in Provider'
    - 'logSetupRouting(): setup.ts event lifecycle — applied/dropped/session_switched with reason field'
    - 'logStoreDecision(): store sync lifecycle — stale generation, retry, space_access_ignored visibility'

key-files:
  created: []
  modified:
    - src/hooks/useDaemonEvents.ts
    - src/hooks/__tests__/useDaemonEvents.test.ts
    - src/components/PairingNotificationProvider.tsx
    - src/components/__tests__/PairingDialog.test.tsx
    - src/api/setup.ts
    - src/store/setupRealtimeStore.ts

key-decisions:
  - 'logPairingRouting/logProviderDecision/logSetupRouting/logStoreDecision all use console.debug — consistent with no-Sentry fallback, same pattern as backend tracing::debug'
  - 'Provider diagnostics use session_id + active_session_id fields to make session_mismatch drops fully diagnosable'
  - 'setup.ts stateKey extracted from state object via Object.keys() to name the state in dedupe logs without emitting full payload'
  - 'space_access_ignored logged with setup_already_completed reason to document sponsor vs joiner role difference'
  - 'stale generation logs include gen= field so parallel sync attempts can be traced by generation number'
  - 'No secrets logged: code, fingerprint, passphrase, short_code fields never appear in any log helper'

requirements-completed:
  - PH85-01
  - PH85-02
  - PH85-04
  - PH85-05

duration: 7min
completed: 2026-04-04
---

# Phase 85 Plan 02: Frontend Pairing/Setup Event Consumer Observability Summary

**Four structured diagnostic helpers making all frontend pairing/setup routing decisions, session filtering, dedupe drops, and retry scheduling visible instead of silent**

## Performance

- **Duration:** 7 min
- **Started:** 2026-04-04T13:15:34Z
- **Completed:** 2026-04-04T13:22:30Z
- **Tasks:** 3
- **Files modified:** 6

## Accomplishments

- Added `logPairingRouting()` free function in `useDaemonEvents.ts` — records `routed`, `ignored`, `unsupported` at every pairing/setup event path including malformed payload detection for `setup.spaceAccessCompleted`; added 4 new routing-diagnostic tests
- Added `logProviderDecision()` in `PairingNotificationProvider.tsx` — records `accepted`, `rejected`, `ignored`, `canceled`, `success`, `failure` with `session_id` and `active_session_id` fields at every provider decision point; added 3 new session-aware diagnostic tests
- Added `logSetupRouting()` in `setup.ts` — records `applied`, `dropped` (missing_session_id, duplicate_state_event), `session_switched` in `onSetupStateChanged`; extracts state key name for dedupe logs without emitting payloads
- Added `logStoreDecision()` in `setupRealtimeStore.ts` — records `started`, `running`, `skipped` (with stale gen reason), `failure`, `scheduled`, `space_access_ignored` across all async lifecycle continuations

## Task Commits

1. **Task 1: Harden the shared frontend hook boundary** - `d6814b18` (feat)
2. **Task 2: Instrument PairingNotificationProvider decision points** - `3f9164ca` (feat)
3. **Task 3: Make setup realtime store dedupe and ignore paths visible** - `dcfd6945` (feat)

## Files Created/Modified

- `src/hooks/useDaemonEvents.ts` - logPairingRouting() helper + routed/ignored/unsupported at all pairing/setup paths
- `src/hooks/__tests__/useDaemonEvents.test.ts` - 4 new routing-diagnostic tests
- `src/components/PairingNotificationProvider.tsx` - logProviderDecision() + diagnostics at every accept/reject/ignore/cancel/success/failure path
- `src/components/__tests__/PairingDialog.test.tsx` - 3 new provider-diagnostic tests
- `src/api/setup.ts` - logSetupRouting() + applied/dropped/session_switched in onSetupStateChanged
- `src/store/setupRealtimeStore.ts` - logStoreDecision() + lifecycle diagnostics across ensureSetupRealtimeSync

## Decisions Made

- All four log helpers use `console.debug` — consistent across layers, zero-overhead when DevTools are closed, no Sentry dependency required
- Provider session mismatch logs include both `session_id` (incoming event) and `active_session_id` (current session) to make drop cause immediately clear
- `setup.ts` extracts `stateKey` via `Object.keys()` for dedupe logs — avoids emitting full state payload while still naming the dropped state
- `space_access_ignored` is a distinct decision reason (not generic `dropped`) to document that this is intentional sponsor-side behavior
- Stale generation logs include `gen=` field to allow tracing which sync attempt was superseded in concurrent initialization scenarios

## Deviations from Plan

None — plan executed exactly as written. All three tasks implemented at their specified decision points with the pattern established in task 1 extended uniformly to tasks 2 and 3.

## Known Stubs

None. All diagnostic helpers are fully wired.

---

_Phase: 85-improve-pairing-observability-across-daemon-event-routing-and-ui-state-transitions_
_Completed: 2026-04-04_

## Self-Check: PASSED

- FOUND: 85-02-SUMMARY.md
- FOUND: src/hooks/useDaemonEvents.ts
- FOUND: src/components/PairingNotificationProvider.tsx
- FOUND: src/api/setup.ts
- FOUND: src/store/setupRealtimeStore.ts
- FOUND: d6814b18 (Task 1 commit)
- FOUND: 3f9164ca (Task 2 commit)
- FOUND: dcfd6945 (Task 3 commit)
