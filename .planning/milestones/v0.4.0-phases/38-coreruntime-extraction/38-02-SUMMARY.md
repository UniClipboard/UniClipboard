---
phase: 38-coreruntime-extraction
plan: 02
subsystem: infra
tags: [rust, tauri, refactor, runtime, stale-emitter-fix, shared-cell]

# Dependency graph
requires:
  - phase: 38-01
    provides: TaskRegistry in uc-app, lifecycle adapters in uc-app
provides:
  - CoreRuntime struct in uc-app/src/runtime.rs (Tauri-free, 7 fields)
  - AppRuntime wraps Arc<CoreRuntime> with only app_handle and watcher_control
  - HostEventSetupPort uses shared RwLock cell (stale emitter fix structural change)
  - emitter_cell shared between CoreRuntime and build_setup_orchestrator
affects: [38-03-PLAN, HostEventSetupPort SC#4 integration test, uc-bootstrap extraction]

# Tech tracking
tech-stack:
  added: []
  patterns:
    [
      'Shared-cell pattern: Arc<RwLock<Arc<dyn Port>>> allows read-through after bootstrap swap',
      'CoreRuntime in uc-app with pub(crate) fields and facade accessors; AppRuntime proxies all methods',
      'Thin facade: AppRuntime delegates to CoreRuntime, keeping only Tauri-specific fields directly',
    ]

key-files:
  created:
    - src-tauri/crates/uc-app/src/runtime.rs
  modified:
    - src-tauri/crates/uc-app/src/lib.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs

key-decisions:
  - 'CoreRuntime::new() accepts pre-built Arc<RwLock<Arc<dyn HostEventEmitterPort>>> — caller creates the cell, CoreRuntime never wraps internally'
  - 'emitter_cell created once in with_setup() and shared with both build_setup_orchestrator and CoreRuntime::new() — same Arc, no copies'
  - 'app_handle_cell() accessor added to AppRuntime to expose Arc<RwLock<Option<AppHandle>>> for TauriSessionReadyEmitter in UseCases'
  - 'Used NoopEmitter (not LoggingSessionReadyEmitter) for emitter_cell_reflects_swap test — LoggingSessionReadyEmitter does not implement HostEventEmitterPort'

patterns-established:
  - 'Shared-cell pattern established as the foundation for stale-emitter fix — full HostEventSetupPort SC#4 test is in Plan 03'

requirements-completed: [RNTM-01]

# Metrics
duration: 11min
completed: 2026-03-18
---

# Phase 38 Plan 02: CoreRuntime Struct and AppRuntime Refactoring Summary

**CoreRuntime created in uc-app as a Tauri-free struct holding all domain state; AppRuntime refactored to wrap Arc<CoreRuntime> with only app_handle and watcher_control; HostEventSetupPort fixed to use shared RwLock cell eliminating stale emitter bug**

## Performance

- **Duration:** 11 min
- **Started:** 2026-03-18T05:55:23Z
- **Completed:** 2026-03-18T06:06:32Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- CoreRuntime struct in uc-app/src/runtime.rs with 7 fields (deps, event_emitter, lifecycle_status, setup_orchestrator, clipboard_integration_mode, task_registry, storage_paths)
- CoreRuntime::new() accepts pre-built `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` shared cell — no internal wrapping
- emitter_cell(), event_emitter(), set_event_emitter() methods for shared-cell pattern
- All facade accessors on CoreRuntime: device_id, is_encryption_ready, encryption_state, settings_port, wiring_deps, clipboard_integration_mode, task_registry, setup_orchestrator, lifecycle_status, storage_paths
- Unit test emitter_cell_reflects_swap passes — proves shared-cell swap propagation
- AppRuntime reduced from 9 fields to 3: core (Arc<CoreRuntime>), app_handle, watcher_control
- All facade methods on AppRuntime proxy to CoreRuntime
- HostEventSetupPort changed from `emitter: Arc<dyn HostEventEmitterPort>` (snapshot) to `emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` (shared cell)
- with_setup() creates emitter_cell once and passes it to both build_setup_orchestrator and CoreRuntime::new()
- uc-app/src/lib.rs: added `pub mod runtime` and `pub use runtime::CoreRuntime`
- All 491 tests pass (290 in uc-app, 203 in uc-tauri, 3 ignored)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create CoreRuntime struct in uc-app** - `e52a880a` (feat)
2. **Task 2: Refactor AppRuntime to wrap CoreRuntime and fix stale emitter** - `0cdacfcf` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/runtime.rs` - CoreRuntime with 7 fields, constructor, emitter cell accessors, facade methods, emitter swap unit test (created)
- `src-tauri/crates/uc-app/src/lib.rs` - Added `pub mod runtime` and `pub use runtime::CoreRuntime`
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - AppRuntime struct reduced to 3 fields; with_setup creates emitter_cell; build_setup_orchestrator updated; all UseCases methods updated to use wiring_deps(); app_handle_cell() added
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` - HostEventSetupPort changed to use shared emitter_cell

## Decisions Made

- CoreRuntime::new() accepts pre-built shared cell (not bare emitter) to enforce the invariant that the cell is created once at the call site
- emitter_cell created in with_setup() before both consumers to guarantee both CoreRuntime and HostEventSetupPort share the identical Arc
- app_handle_cell() method added as a clean accessor for the Arc<RwLock<Option<AppHandle>>> without exposing the private field
- NoopEmitter used in emitter_cell_reflects_swap test since LoggingSessionReadyEmitter does not implement HostEventEmitterPort

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] SetupState::FirstRun does not exist**

- **Found during:** Task 1 (CoreRuntime unit test)
- **Issue:** Plan's test template used `uc_core::setup::SetupState::FirstRun` but actual enum has no `FirstRun` variant — it uses `Welcome`
- **Fix:** Changed to `uc_core::setup::SetupState::Welcome`
- **Files modified:** src-tauri/crates/uc-app/src/runtime.rs
- **Verification:** cargo test -p uc-app -- emitter_cell exits 0
- **Committed in:** e52a880a (Task 1 commit)

**2. [Rule 1 - Bug] Multi-line field access not captured by replace-all**

- **Found during:** Task 2 (cargo check failure)
- **Issue:** In ClipboardChangeHandler impl, `self` and `.deps` were on separate lines — `replace_all` of `self.deps.` did not match the multi-line pattern
- **Fix:** Manually fixed `self\n                .deps\n                .clipboard...` to use `self.wiring_deps().clipboard...`
- **Files modified:** src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
- **Verification:** cargo check -p uc-tauri exits 0
- **Committed in:** 0cdacfcf (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 bugs)
**Impact on plan:** Both fixes necessary for compilation. No scope creep.

## Issues Encountered

None beyond the two auto-fixed deviations above.

## Next Phase Readiness

- CoreRuntime is now a stable Tauri-free struct in uc-app — prerequisite for Plan 03
- HostEventSetupPort structural change (shared cell) is in place
- Plan 03 Task 2 adds the SC#4 integration test for HostEventSetupPort read-through behavior
- All 491 tests pass; no blockers for Plan 03

---

_Phase: 38-coreruntime-extraction_
_Completed: 2026-03-18_
