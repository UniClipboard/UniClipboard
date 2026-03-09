---
phase: 12-lifecycle-governance-baseline
plan: 01
subsystem: infra
tags: [tokio, cancellation-token, joinset, task-lifecycle, graceful-shutdown, tauri]

# Dependency graph
requires:
  - phase: 10-boundary-repair-baseline
    provides: AppRuntime structure, bootstrap wiring pattern
provides:
  - TaskRegistry with spawn/shutdown/child_token for centralized task lifecycle
  - Cooperative cancellation via CancellationToken replacing ctrl_c signals
  - Tauri app exit hook triggering graceful shutdown
affects: [lifecycle-governance, platform-runtime, shutdown]

# Tech tracking
tech-stack:
  added: [tokio-util (CancellationToken)]
  patterns: [TaskRegistry spawn pattern, cooperative cancellation with select!, build+run exit hook]

key-files:
  created:
    - src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs
    - src-tauri/crates/uc-tauri/tests/task_registry_test.rs
  modified:
    - src-tauri/crates/uc-tauri/Cargo.toml
    - src-tauri/crates/uc-tauri/src/bootstrap/mod.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/src/main.rs

key-decisions:
  - 'TaskRegistry spawns wrapped in single async orchestration block since start_background_tasks is sync'
  - 'Integration tests used instead of lib tests due to pre-existing test compilation failures in other modules'

patterns-established:
  - 'TaskRegistry spawn pattern: registry.spawn(name, |token| async { ... }) with cooperative cancellation'
  - 'Tauri exit hook pattern: .build() + .run() with RunEvent::ExitRequested triggering token.cancel()'

requirements-completed: [LIFE-01, LIFE-02]

# Metrics
duration: 8min
completed: 2026-03-06
---

# Phase 12 Plan 01: Task Registry Summary

**TaskRegistry with CancellationToken + JoinSet for centralized async task lifecycle, cooperative cancellation replacing ctrl_c, and Tauri exit hook for graceful shutdown**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-06T13:45:05Z
- **Completed:** 2026-03-06T13:53:39Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- TaskRegistry struct with spawn/shutdown/child_token/token/task_count methods and 4 unit tests
- All 7 long-lived spawn sites in wiring.rs migrated to registry.spawn() with cooperative cancellation
- Tauri app exit changed from .run() to .build()/.run() with RunEvent::ExitRequested triggering token cancellation
- Clipboard receive and pairing event loops use CancellationToken instead of tokio::signal::ctrl_c()

## Task Commits

Each task was committed atomically:

1. **Task 1: Create TaskRegistry with CancellationToken + JoinSet and unit tests** - `c7c8a55d` (feat)
2. **Task 2: Migrate spawn sites to TaskRegistry and wire Tauri exit hook** - `26eb9637` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs` - TaskRegistry struct with spawn/shutdown/child_token methods
- `src-tauri/crates/uc-tauri/tests/task_registry_test.rs` - 4 integration tests for TaskRegistry behaviors
- `src-tauri/crates/uc-tauri/Cargo.toml` - Added tokio-util dependency
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` - Exported task_registry module
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Added TaskRegistry field and accessor to AppRuntime
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Migrated all spawn sites to use TaskRegistry
- `src-tauri/src/main.rs` - Changed to build/run pattern with exit hook, passes TaskRegistry to background tasks

## Decisions Made

- Used a single async_runtime::spawn orchestration block in start_background_tasks to bridge sync function with async registry.spawn() calls
- Placed tests in integration test files due to pre-existing compilation failures in other uc-tauri test modules (WatcherControlPort, IdentityStoreError imports)
- Spooler and blob worker tasks receive CancellationToken but don't actively select on it since they exit naturally when their channels close; the janitor and event loops actively select on token.cancelled()

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] tokio-util "sync" feature does not exist in v0.7.17**

- **Found during:** Task 1
- **Issue:** Research recommended `features = ["sync"]` but CancellationToken is in default features for tokio-util 0.7.17
- **Fix:** Changed dependency to `tokio-util = "0.7"` without feature flags
- **Verification:** cargo check passes, CancellationToken imports resolve
- **Committed in:** c7c8a55d (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minor dependency specification correction. No scope creep.

## Issues Encountered

- Pre-existing test compilation failures in uc-tauri (WatcherControlPort, UiPort, AutostartPort, IdentityStoreError moved between crates) prevented running `cargo test --lib` for uc-tauri; used integration tests instead.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- TaskRegistry infrastructure ready for any future spawned tasks
- Phase 12 plans 02/03 can build on this lifecycle foundation for staged state injection and encryption session dedup

---

_Phase: 12-lifecycle-governance-baseline_
_Completed: 2026-03-06_
