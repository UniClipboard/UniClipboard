---
phase: 60-extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app
plan: 01
subsystem: infra
tags: [rust, file-transfer, orchestrator, emitter-cell, uc-app, uc-bootstrap]

# Dependency graph
requires:
  - phase: 37-wiring-decomposition
    provides: assembly.rs composition root pattern, HostEventSetupPort emitter_cell pattern
  - phase: 38-coreruntime-extraction
    provides: CoreRuntime emitter_cell field and set_event_emitter swap mechanism
provides:
  - FileTransferOrchestrator struct in uc-app with emitter_cell pattern
  - build_file_transfer_orchestrator builder function in uc-bootstrap assembly.rs
  - BackgroundRuntimeDeps.file_transfer_orchestrator field (non-Optional Arc)
  - WiredDependencies.emitter_cell shared cell propagated from wire time
affects:
  - 60-02 (next plan: migrate wiring.rs callers to use the orchestrator)
  - uc-tauri/bootstrap/wiring.rs (currently destructures BackgroundRuntimeDeps)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "emitter_cell pattern: Arc<RwLock<Arc<dyn HostEventEmitterPort>>> shared at wire time, auto-sees emitter swap"
    - "WireTimeLoggingEmitter: minimal inline logging emitter for pre-bootstrap placeholder"
    - "FileTransferOrchestrator: struct wrapping 9 lifecycle orchestration methods with &self (interior mutability)"

key-files:
  created:
    - src-tauri/crates/uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs
  modified:
    - src-tauri/crates/uc-app/src/usecases/file_sync/mod.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-bootstrap/src/assembly.rs
    - src-tauri/crates/uc-bootstrap/src/lib.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs

key-decisions:
  - "FileTransferOrchestrator holds emitter_cell (Arc<RwLock<Arc<dyn HostEventEmitterPort>>>) matching HostEventSetupPort pattern — automatically sees TauriEventEmitter after swap with no Option/expect"
  - "WireTimeLoggingEmitter created inline in assembly.rs to avoid importing LoggingHostEventEmitter from non_gui_runtime (keeps assembly.rs zero-tauri)"
  - "emitter_cell added to WiredDependencies — plan assumed it existed at wire_dependencies time but it was not; now created there and propagated to callers"
  - "BackgroundRuntimeDeps.file_transfer_orchestrator is Arc (not Option) — emitter_cell pattern makes deferred construction unnecessary"

patterns-established:
  - "File transfer orchestration: methods take &self via Mutex/RwLock interior mutability, no &mut self needed"
  - "Wire-time emitter_cell: created in wire_dependencies_with_identity_store, included in WiredDependencies for caller reuse"

requirements-completed:
  - PH60-01
  - PH60-02
  - PH60-03

# Metrics
duration: 15min
completed: 2026-03-25
---

# Phase 60 Plan 01: Extract File Transfer Orchestrator Summary

**FileTransferOrchestrator struct in uc-app with emitter_cell pattern, BackgroundRuntimeDeps field, and wire-time emitter_cell in WiredDependencies**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-25T13:11:28Z
- **Completed:** 2026-03-25T13:26:45Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Created `FileTransferOrchestrator` in `uc-app` with all 9 lifecycle methods as `&self` methods using interior mutability
- Struct holds `emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` matching `HostEventSetupPort` pattern — auto-sees emitter swaps
- Added `file_transfer_orchestrator: Arc<FileTransferOrchestrator>` to `BackgroundRuntimeDeps` (non-Optional, per D-06)
- Added `build_file_transfer_orchestrator()` builder function in `assembly.rs` taking `emitter_cell` (per D-05)
- Created shared `emitter_cell` inside `wire_dependencies_with_identity_store` and propagated via `WiredDependencies`
- 8 unit tests pass including emitter_cell swap visibility test

## Task Commits

Each task was committed atomically:

1. **Task 1: Create FileTransferOrchestrator in uc-app with emitter_cell pattern** - `aa8eb96a` (feat)
2. **Task 2: Wire FileTransferOrchestrator into BackgroundRuntimeDeps via assembly** - `e28ff554` (feat)

**Plan metadata:** *(this commit)*

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs` - New FileTransferOrchestrator struct with all 9 methods, EarlyCompletionCache, EarlyCompletionInfo, FileTransferStatusPayload, 8 unit tests
- `src-tauri/crates/uc-app/src/usecases/file_sync/mod.rs` - Added pub mod and re-exports for FileTransferOrchestrator, EarlyCompletionCache, EarlyCompletionInfo
- `src-tauri/crates/uc-app/src/usecases/mod.rs` - Added re-exports for new types in file_sync block
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` - Added WireTimeLoggingEmitter, emitter_cell to WiredDependencies, file_transfer_orchestrator to BackgroundRuntimeDeps, build_file_transfer_orchestrator builder
- `src-tauri/crates/uc-bootstrap/src/lib.rs` - Added build_file_transfer_orchestrator to re-exports
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Fixed BackgroundRuntimeDeps destructure to include new field

## Decisions Made

- **emitter_cell pattern for orchestrator**: Used `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` matching existing `HostEventSetupPort` in assembly.rs, enabling automatic emitter swap visibility without Option or deferred construction.
- **WireTimeLoggingEmitter inline**: Added minimal logging emitter inline in assembly.rs rather than importing from non_gui_runtime.rs to maintain assembly.rs's zero-tauri constraint.
- **emitter_cell in WiredDependencies**: The plan assumed emitter_cell was created inside `wire_dependencies_with_identity_store` but it wasn't. Added it there and propagated via `WiredDependencies` so all consumers share the same cell.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] emitter_cell not available in wire_dependencies scope**

- **Found during:** Task 2 (Wire FileTransferOrchestrator into BackgroundRuntimeDeps)
- **Issue:** The plan assumed `emitter_cell` was created inside `wire_dependencies_with_identity_store` and could be used to construct the orchestrator there. In reality, `emitter_cell` was created externally in `build_non_gui_runtime_with_setup` (non_gui_runtime.rs) and `AppRuntime::with_setup()` (uc-tauri). Without it at wire time, the orchestrator field in `BackgroundRuntimeDeps` couldn't be populated.
- **Fix:** Created `WireTimeLoggingEmitter` inline in assembly.rs, created `emitter_cell` inside `wire_dependencies_with_identity_store` with this placeholder, added `emitter_cell` field to `WiredDependencies` for callers to reuse. This is architecturally correct: the cell is created once at wire time, all consumers share it, and emitter swaps propagate to all holders.
- **Files modified:** assembly.rs (WireTimeLoggingEmitter, emitter_cell creation, WiredDependencies.emitter_cell field)
- **Verification:** uc-bootstrap cargo check passes, all 21 tests pass
- **Committed in:** `e28ff554` (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 missing critical)
**Impact on plan:** Fix is necessary for the plan's design to work. No scope creep — `emitter_cell` in `WiredDependencies` is consumed by callers that already needed it.

## Issues Encountered

- Pre-existing `uc-cli` compilation error (`missing lease_ttl_ms`) found during verification — confirmed pre-existing via git stash check. Not caused by our changes.

## Next Phase Readiness

- `FileTransferOrchestrator` is in uc-app and accessible from all runtime paths
- `BackgroundRuntimeDeps.file_transfer_orchestrator` is available for Plan 02 to wire into wiring.rs callers
- `WiredDependencies.emitter_cell` available for callers (build_non_gui_runtime_with_setup, AppRuntime::with_setup) to optionally use the pre-created cell
- Plan 02 will migrate wiring.rs to use the orchestrator instead of standalone functions and the old EarlyCompletionCache

## Self-Check: PASSED

- `file_transfer_orchestrator.rs`: FOUND
- `SUMMARY.md`: FOUND
- Commit `aa8eb96a` (Task 1): FOUND
- Commit `e28ff554` (Task 2): FOUND
- `cargo check -p uc-app`: PASSED
- `cargo check -p uc-bootstrap`: PASSED
- `cargo test -p uc-app file_transfer`: 8 passed

---
*Phase: 60-extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app*
*Completed: 2026-03-25*
