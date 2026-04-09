---
phase: 36-event-emitter-abstraction
plan: 02
subsystem: infra
tags: [rust, tauri, event-emitter, hexagonal-architecture, ports-adapters]

# Dependency graph
requires:
  - phase: 36-event-emitter-abstraction plan 01
    provides: HostEventEmitterPort trait, TauriEventEmitter, LoggingEventEmitter adapters in uc-core and uc-tauri

provides:
  - AppRuntime.event_emitter field (RwLock<Arc<dyn HostEventEmitterPort>>) alongside app_handle
  - All in-scope background emit sites migrated to HostEventEmitterPort
  - emit_pending_status in file_transfer_wiring.rs uses dyn HostEventEmitterPort
  - Obsolete P2P event types and transfer progress forwarding function deleted

affects: [37-wiring-decomposition, daemon-mode, cli-mode]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - RwLock<Arc<dyn Port>> for late-init fields that start with a stub and get swapped to real impl
    - LoggingEventEmitter as bootstrap stub, swapped to TauriEventEmitter after AppHandle available
    - Emitter parameter threading: functions receive Arc<dyn HostEventEmitterPort>, never construct their own

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs
    - src-tauri/src/main.rs
    - src-tauri/crates/uc-tauri/src/events/mod.rs

key-decisions:
  - 'event_emitter field uses RwLock<Arc<dyn HostEventEmitterPort>> (not bare Arc) to allow post-setup swap from LoggingEventEmitter to TauriEventEmitter'
  - 'app_handle field KEPT alongside event_emitter for out-of-scope callers (commands/pairing.rs, commands/clipboard.rs, apply_autostart, setup orchestrator)'
  - 'run_clipboard_receive_loop app_handle parameter marked _app_handle (unused) since all its emit sites were migrated'
  - 'Out-of-scope file_transfer_wiring.rs functions (handle_transfer_progress, handle_transfer_completed, handle_transfer_failed, spawn_timeout_sweep, reconcile_on_startup) remain on AppHandle — deferred to Phase 37 wiring decomposition'
  - 'P2PPeerDiscoveryEvent, P2PPeerConnectionEvent, P2PPeerNameUpdatedEvent, forward_transfer_progress_event deleted with zero remaining external references'

patterns-established:
  - 'Pattern: Bootstrap stub — initialize with LoggingEventEmitter, swap to TauriEventEmitter in setup callback via RwLock write'
  - 'Pattern: Emitter threading — pass Arc<dyn HostEventEmitterPort> as parameter, clone for each closure capture'
  - "Pattern: Test helpers — use TauriEventEmitter(app_handle) in tests that verify events are received; LoggingEventEmitter for tests that don't"

requirements-completed: [EVNT-04]

# Metrics
duration: 60min
completed: 2026-03-17
---

# Phase 36 Plan 02: Wire HostEventEmitterPort into Background Tasks Summary

**AppRuntime gains event_emitter: RwLock<Arc<dyn HostEventEmitterPort>> field; all in-scope background emit sites (peer discovery, peer connection, transfer progress, transfer completed, inbound clipboard error/recovered, transfer status changed, emit_pending_status) migrated from AppHandle to HostEventEmitterPort; obsolete P2P event types and forwarding functions deleted**

## Performance

- **Duration:** ~60 min
- **Started:** 2026-03-17T09:44:14Z
- **Completed:** 2026-03-17T10:44:00Z
- **Tasks:** 2 (plus 2 atomic commits for deferred docs and type deletion)
- **Files modified:** 6

## Accomplishments

- AppRuntime struct now has both `app_handle` (preserved for out-of-scope callers) and `event_emitter: RwLock<Arc<dyn HostEventEmitterPort>>` (for in-scope background tasks)
- Bootstrap pattern established: LoggingEventEmitter as initial stub, swapped to TauriEventEmitter via `set_event_emitter()` after Tauri setup provides AppHandle
- All in-scope emit sites in wiring.rs and file_transfer_wiring.rs migrated to HostEventEmitterPort
- Obsolete event types (P2PPeerDiscoveryEvent, P2PPeerConnectionEvent, P2PPeerNameUpdatedEvent, forward_transfer_progress_event) deleted with zero remaining references
- Full workspace test suite passes (1173 tests)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add event_emitter field to AppRuntime, migrate clipboard emit path** - `fc1d05e7` (refactor)
2. **Task 2: Wire HostEventEmitterPort into background task emit sites** - `aca36060` (refactor)
3. **Deferred docs + type deletion** - `013f652d` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Added event_emitter field, accessor/setter, clipboard emit migration
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Added event_emitter parameter to start_background_tasks/run_pairing_event_loop/run_clipboard_receive_loop; migrated all in-scope emit sites
- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` - Migrated emit_pending_status to dyn HostEventEmitterPort; added Phase 37 deferral comment
- `src-tauri/src/main.rs` - Pass LoggingEventEmitter at construction, TauriEventEmitter swap in setup; pass event_emitter() to start_background_tasks
- `src-tauri/crates/uc-tauri/src/events/mod.rs` - Removed dead re-exports for P2P types and transfer progress
- `src-tauri/crates/uc-tauri/src/events/p2p_peer.rs` - Deleted (zero remaining references)
- `src-tauri/crates/uc-tauri/src/events/transfer_progress.rs` - Deleted (zero remaining references)

## Decisions Made

- `event_emitter` field uses `RwLock<Arc<dyn HostEventEmitterPort>>` not bare `Arc` because AppRuntime is created BEFORE the Tauri setup callback. It needs to start with `LoggingEventEmitter` (safe fallback) and swap to `TauriEventEmitter` when AppHandle becomes available.
- `app_handle` field retained alongside `event_emitter` — commands/pairing.rs, commands/clipboard.rs, apply_autostart, and the setup orchestrator still depend on it.
- Out-of-scope file_transfer_wiring.rs functions kept on `AppHandle<R>` — migrating them requires restructuring the closure capture pattern in wiring.rs (Phase 37 work).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Fixed test call sites for run_pairing_event_loop and run_clipboard_receive_loop**

- **Found during:** Task 2 (after adding event_emitter parameter to functions)
- **Issue:** Three test call sites in wiring.rs tests used old signatures (missing event_emitter argument)
- **Fix:** Updated test calls to pass `LoggingEventEmitter` (for tests without event verification) or `TauriEventEmitter::new(app_handle.clone())` (for tests verifying emitted events)
- **Files modified:** src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
- **Verification:** `cargo test -p uc-tauri` passes (197 tests)
- **Committed in:** aca36060 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (missing critical — test signatures)
**Impact on plan:** Required for compilation. No scope creep.

## Issues Encountered

- Pre-existing flaky test `test_put_and_get_blob` in uc-platform occasionally fails in parallel workspace test runs (blob storage race condition). Passes when run in isolation. Unrelated to event emitter changes.

## Next Phase Readiness

- EVNT-04 complete: background task emit sites use HostEventEmitterPort
- Remaining file_transfer_wiring.rs emit sites (handle_transfer_progress, handle_transfer_completed, handle_transfer_failed, spawn_timeout_sweep, reconcile_on_startup) documented as deferred to Phase 37 (wiring decomposition)
- Phase 37 can now restructure closure capture patterns, knowing the emitter threading pattern is established

---

_Phase: 36-event-emitter-abstraction_
_Completed: 2026-03-17_
