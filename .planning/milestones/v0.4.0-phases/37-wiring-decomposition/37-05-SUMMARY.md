---
phase: 37-wiring-decomposition
plan: 05
subsystem: pairing
tags: [setup, pairing, event-subscription, race-condition, failure-propagation, join-space]

requires:
  - phase: 37-wiring-decomposition
    provides: 'action_executor.rs with EnsurePairing handler and start_pairing_verification_listener'
  - phase: 37-UAT
    provides: 'root cause analysis for Tests 2 and 4'

provides:
  - 'Reliable pairing event subscription ordering: setup subscribes before initiating pairing'
  - 'StreamClosedByPeer bridged to PairingFailed for active sessions without explicit close'
  - 'Reject path test: A reject initial request -> B returns to JoinSpaceSelectDevice(PairingRejected)'
  - 'Low-latency accept test: subscription fix verified via immediate PairingChallenge delivery'

affects:
  - 38-coreruntime-extraction
  - pairing-join-space-reliability

tech-stack:
  added: []
  patterns:
    - 'Subscribe-before-initiate: pairing event subscription must be established before initiate_pairing to avoid lost events on same-device/low-latency paths'
    - 'app_closed_tx watch channel: guards StreamClosedByPeer→PairingFailed bridge from firing on explicit application-initiated session closes'

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs
    - src-tauri/crates/uc-platform/src/adapters/pairing_stream/service.rs
    - src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs

key-decisions:
  - 'Subscribe to pairing domain events BEFORE calling initiate_pairing to eliminate the race window where PairingVerificationRequired/PairingFailed could be emitted before the setup listener was registered'
  - 'Bridge StreamClosedByPeer to PairingFailed only when app_closed flag is false — prevents spurious failures on explicit close while catching unexpected peer disconnects'
  - 'app_closed_tx flag set BEFORE shutdown_tx signal so run_session can observe application intent even in the race where read_loop sees EOF before the shutdown signal'
  - 'Test 5 not fixed directly — its completion depends on the full pairing chain (Tests 2/4) being fixed first; left as downstream verification'

requirements-completed:
  - RNTM-02

duration: 55min
completed: 2026-03-18
---

# Phase 37 Plan 05: Join-Space Event Reliability Summary

**Subscribe-before-initiate fix and StreamClosedByPeer failure bridge eliminate ProcessingJoinSpace stall and silent reject for join-space pairing flow**

## Performance

- **Duration:** ~55 min
- **Started:** 2026-03-18T00:00:00Z
- **Completed:** 2026-03-18T01:00:00Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments

- Moved pairing event subscription ahead of `initiate_pairing` in `ensure_pairing_session`, closing the race window where `PairingVerificationRequired` / `PairingFailed` could be emitted before the setup listener was registered
- Added `app_closed_tx` watch channel to `SessionHandle` and bridged `StreamClosedByPeer` → `PairingFailed` when the session was not explicitly closed, ensuring peerB exits `ProcessingJoinSpace` even when peerA drops the connection without sending a Reject frame
- Added two new integration tests: reject-path returning `PairingRejected` error and low-latency verification advancing to `JoinSpaceConfirmPeer`

## Task Commits

1. **Task 1: Subscribe pairing events before initiating session** - `bc3eea71` (fix)
2. **Task 2: Bridge StreamClosedByPeer to PairingFailed in pairing stream** - `a9f9472b` (fix)
3. **Task 3: Add join-space reject and low-latency verification tests** - `616d2911` (test)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs` - Subscribe before initiate; renamed listener to `start_pairing_verification_listener_with_rx`
- `src-tauri/crates/uc-platform/src/adapters/pairing_stream/service.rs` - `app_closed_tx` field, failure bridge for `StreamClosedByPeer`, 2 new tests
- `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs` - 2 new join-space integration tests

## Decisions Made

- Renamed `start_pairing_verification_listener` to `start_pairing_verification_listener_with_rx` to make the caller's responsibility explicit: the caller must subscribe and pass the receiver to avoid any race window
- Used a `watch::Sender<bool>` (`app_closed_tx`) rather than an `AtomicBool` to allow cross-task observation without `Arc` overhead; the watch channel's broadcast semantics match the one-writer/one-reader pattern

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Initial test for `pairing_stream_clean_close_without_protocol_termination_emits_pairing_failed` was written to test EOF before any frame (which goes through `run_incoming_session` error path, not `run_session`); corrected to test EOF after a valid first frame so `run_session` runs and the bridge is exercised
- Initial explicit-close test was failing because of a timing race where the read_loop could see EOF before the shutdown signal; resolved by adding the `app_closed_tx` flag which is set before the shutdown signal, so `run_session` can detect application intent regardless of signal ordering

## Next Phase Readiness

- Join-space backend is now reliable: subscription ordering and failure propagation are both fixed
- UAT Tests 2 and 4 have backend fixes and unit-test coverage; manual re-verification needed
- Test 5 (join-space completion) is unblocked on the backend; depends on Tests 2/4 fixes being confirmed in a subsequent UAT run
- Phase 38 (coreruntime-extraction) can proceed without dependency on this fix

---

_Phase: 37-wiring-decomposition_
_Completed: 2026-03-18_
