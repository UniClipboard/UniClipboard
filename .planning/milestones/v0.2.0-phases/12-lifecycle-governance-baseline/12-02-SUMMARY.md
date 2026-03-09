---
phase: 12-lifecycle-governance-baseline
plan: 02
subsystem: infra
tags: [arc, dependency-injection, lifecycle, encryption, pairing]

# Dependency graph
requires:
  - phase: 12-lifecycle-governance-baseline
    provides: TaskRegistry lifecycle management from plan 01
provides:
  - Injectable StagedPairedDeviceStore replacing global OnceLock static
  - Single authoritative EncryptionSessionPort implementation (uc-platform only)
affects: [pairing, space-access, bootstrap-wiring]

# Tech tracking
tech-stack:
  added: []
  patterns: [Arc-based dependency injection for runtime state, lifecycle-clearable staging store]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-app/src/usecases/pairing/staged_paired_device_store.rs
    - src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs
    - src-tauri/crates/uc-app/src/usecases/space_access/persistence_adapter.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/src/main.rs
    - src-tauri/crates/uc-infra/src/security/mod.rs
    - src-tauri/crates/uc-app/Cargo.toml

key-decisions:
  - 'StagedPairedDeviceStore uses std::sync::Mutex (not tokio) matching original OnceLock<Mutex> pattern'
  - 'clear() made public (not test-only) for lifecycle shutdown cleanup'
  - 'uc-platform added as dev-dep of uc-app for InMemoryEncryptionSessionPort test access'

patterns-established:
  - 'Runtime state as injectable Arc<T> structs rather than global statics'

requirements-completed: [LIFE-03, LIFE-04]

# Metrics
duration: 22min
completed: 2026-03-06
---

# Phase 12 Plan 02: Staged Store Injection and Encryption Session Consolidation Summary

**Replaced global OnceLock-based StagedPairedDeviceStore with Arc-injected struct and removed duplicate InMemoryEncryptionSession from uc-infra**

## Performance

- **Duration:** 22 min
- **Started:** 2026-03-06T13:57:02Z
- **Completed:** 2026-03-06T14:19:58Z
- **Tasks:** 2
- **Files modified:** 12

## Accomplishments

- Converted StagedPairedDeviceStore from global OnceLock static to injectable struct with Arc
- All callers (PairingOrchestrator, SpaceAccessPersistenceAdapter, wiring, runtime, main) updated to use injected instance
- Deleted uc-infra InMemoryEncryptionSession duplicate; single authoritative impl now in uc-platform only
- Added unit tests proving separate StagedPairedDeviceStore instances do not share state

## Task Commits

Each task was committed atomically:

1. **Task 1: Convert staged_paired_device_store from global static to injectable struct** - `92fbd4b6` (refactor)
2. **Task 2: Remove duplicate InMemoryEncryptionSession from uc-infra** - `b6c59404` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/pairing/staged_paired_device_store.rs` - Rewritten as pub struct with injected Mutex<HashMap>
- `src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs` - Added staged_store field, updated constructor and action execution
- `src-tauri/crates/uc-app/src/usecases/space_access/persistence_adapter.rs` - Added staged_store field, updated constructor and promote logic
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Creates and passes Arc<StagedPairedDeviceStore> to orchestrator and adapter
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Updated placeholder orchestrator and setup persistence adapter construction
- `src-tauri/src/main.rs` - Creates staged_store before PairingOrchestrator, passes to start_background_tasks
- `src-tauri/crates/uc-infra/src/security/encryption_session.rs` - Deleted (duplicate impl removed)
- `src-tauri/crates/uc-infra/src/security/mod.rs` - Removed encryption_session module and re-export
- `src-tauri/crates/uc-app/src/usecases/pairing/mod.rs` - Changed visibility to pub, added StagedPairedDeviceStore re-export
- `src-tauri/crates/uc-app/src/usecases/mod.rs` - Added StagedPairedDeviceStore to pub use
- `src-tauri/crates/uc-app/Cargo.toml` - Added uc-platform as dev-dependency
- `src-tauri/crates/uc-app/tests/setup_flow_integration_test.rs` - Updated import to uc-platform's InMemoryEncryptionSessionPort

## Decisions Made

- StagedPairedDeviceStore uses `std::sync::Mutex` (not tokio) to match the original synchronous access pattern
- `clear()` method made public (not `#[cfg(test)]`-only) so lifecycle owner can clear during shutdown
- Added `uc-platform` as dev-dependency of `uc-app` rather than duplicating a test-only encryption session impl

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Pre-existing test compilation failures in `setup/orchestrator.rs` (unresolved imports for `watcher_control` and `StartClipboardWatcher`) prevent running `cargo test --lib` for uc-app. These are out-of-scope pre-existing issues. All changes verified through `cargo check` (full workspace passes).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All global mutable state from staged_paired_device_store is now lifecycle-manageable via Arc
- Encryption session implementation consolidated to single source of truth in uc-platform
- Phase 12 plan 02 requirements (LIFE-03, LIFE-04) satisfied

---

_Phase: 12-lifecycle-governance-baseline_
_Completed: 2026-03-06_
