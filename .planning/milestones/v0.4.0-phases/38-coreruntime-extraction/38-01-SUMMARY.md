---
phase: 38-coreruntime-extraction
plan: 01
subsystem: infra
tags: [rust, tokio, tauri, refactor, lifecycle, task-registry]

# Dependency graph
requires:
  - phase: 37-wiring-decomposition
    provides: assembly.rs with zero tauri imports; BackgroundRuntimeDeps isolated in wiring.rs
provides:
  - TaskRegistry struct in uc-app/src/task_registry.rs with 4 tests
  - InMemoryLifecycleStatus, LoggingLifecycleEventEmitter, LoggingSessionReadyEmitter, DeviceNameAnnouncer in uc-app/src/usecases/app_lifecycle/adapters.rs
  - resolve_pairing_device_name inlined in uc-app (no uc-tauri import)
  - uc-tauri/src/adapters/lifecycle.rs reduced to TauriSessionReadyEmitter only
affects: [38-02-PLAN, CoreRuntime creation, uc-app runtime composition]

# Tech tracking
tech-stack:
  added: [tokio-util = "0.7" added to uc-app/Cargo.toml]
  patterns:
    [
      Pure (non-Tauri) types extracted to uc-app; Tauri-specific types remain in uc-tauri; re-export pattern used in uc-tauri for backward compat,
    ]

key-files:
  created:
    - src-tauri/crates/uc-app/src/task_registry.rs
    - src-tauri/crates/uc-app/src/usecases/app_lifecycle/adapters.rs
  modified:
    - src-tauri/crates/uc-app/Cargo.toml
    - src-tauri/crates/uc-app/src/lib.rs
    - src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs
    - src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs

key-decisions:
  - "tokio-util dependency added to uc-app without 'sync' feature flag — the locked version (0.7.17) includes CancellationToken in default features, not under a 'sync' feature tag"
  - 'resolve_pairing_device_name inlined into uc-app/src/usecases/app_lifecycle/adapters.rs rather than importing from uc-tauri to eliminate cross-crate dependency'
  - 'uc-tauri/src/bootstrap/task_registry.rs replaced with re-export `pub use uc_app::task_registry::TaskRegistry` for backward compatibility'

patterns-established:
  - 'Tauri-free types belong in uc-app; types that require AppHandle belong in uc-tauri/adapters'
  - 'Re-export pattern: uc-tauri re-exports from uc-app for backward compat when moving types to lower-level crate'

requirements-completed: [RNTM-01]

# Metrics
duration: 18min
completed: 2026-03-18
---

# Phase 38 Plan 01: TaskRegistry and Lifecycle Adapter Migration Summary

**TaskRegistry and four pure lifecycle adapters (InMemoryLifecycleStatus, LoggingLifecycleEventEmitter, LoggingSessionReadyEmitter, DeviceNameAnnouncer) extracted from uc-tauri into uc-app as prerequisite for CoreRuntime creation in Plan 02**

## Performance

- **Duration:** 18 min
- **Started:** 2026-03-18T07:13:24Z
- **Completed:** 2026-03-18T07:31:44Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments

- TaskRegistry with all 4 tests now lives in uc-app/src/task_registry.rs; uc-tauri re-exports for backward compat
- Four lifecycle adapters (InMemoryLifecycleStatus, LoggingLifecycleEventEmitter, LoggingSessionReadyEmitter, DeviceNameAnnouncer) now in uc-app/src/usecases/app_lifecycle/adapters.rs
- resolve_pairing_device_name inlined into uc-app — no remaining uc-tauri import in these types
- uc-tauri/src/adapters/lifecycle.rs trimmed to TauriSessionReadyEmitter only (its 2 tests retained)
- Both uc-app and uc-tauri compile cleanly; all 490 tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Move TaskRegistry from uc-tauri to uc-app** - `da22df76` (feat)
2. **Task 2: Move lifecycle adapters from uc-tauri to uc-app** - `5d7f6129` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/task_registry.rs` - TaskRegistry with full implementation and 4 tests (created)
- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/adapters.rs` - Four pure lifecycle adapters + tests (created)
- `src-tauri/crates/uc-app/Cargo.toml` - Added tokio-util = "0.7"
- `src-tauri/crates/uc-app/src/lib.rs` - Added `pub mod task_registry`
- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` - Added `pub mod adapters` and re-exports
- `src-tauri/crates/uc-app/src/usecases/mod.rs` - Added re-exports for moved types
- `src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs` - Replaced with re-export `pub use uc_app::task_registry::TaskRegistry`
- `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs` - Trimmed to TauriSessionReadyEmitter + 2 tests only
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Updated imports from `crate::adapters::lifecycle::*` to `uc_app::usecases::*`

## Decisions Made

- tokio-util added without 'sync' feature — locked version 0.7.17 includes CancellationToken in default features
- resolve_pairing_device_name inlined into adapters.rs rather than importing from assembly.rs to keep uc-app free of uc-tauri dependency
- uc-tauri re-export pattern used for TaskRegistry to avoid breaking existing import paths

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed incorrect tokio-util feature specification**

- **Found during:** Task 1 (TaskRegistry move)
- **Issue:** Plan specified `features = ["sync"]` but tokio-util 0.7.17 (locked version) does not have a "sync" feature — CancellationToken is in default features
- **Fix:** Changed to `tokio-util = "0.7"` without feature flags
- **Files modified:** src-tauri/crates/uc-app/Cargo.toml
- **Verification:** cargo check -p uc-app exits 0
- **Committed in:** da22df76 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed missing trait import in test module**

- **Found during:** Task 2 (lifecycle adapters move)
- **Issue:** Test module in adapters.rs called `emitter.emit_ready()` without importing `SessionReadyEmitter` trait into scope
- **Fix:** Added `use crate::usecases::app_lifecycle::{LifecycleEventEmitter, LifecycleStatusPort, SessionReadyEmitter};` to test module
- **Files modified:** src-tauri/crates/uc-app/src/usecases/app_lifecycle/adapters.rs
- **Verification:** cargo test -p uc-app -- adapters exits 0 (4 tests pass)
- **Committed in:** 5d7f6129 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 bugs)
**Impact on plan:** Both fixes necessary for compilation. No scope creep.

## Issues Encountered

None beyond the two auto-fixed deviations above.

## Next Phase Readiness

- uc-app now has TaskRegistry and all pure lifecycle adapters — prerequisites for Plan 02 (CoreRuntime creation)
- Both crates compile cleanly; 490 tests pass
- No blockers for Plan 02

---

_Phase: 38-coreruntime-extraction_
_Completed: 2026-03-18_
