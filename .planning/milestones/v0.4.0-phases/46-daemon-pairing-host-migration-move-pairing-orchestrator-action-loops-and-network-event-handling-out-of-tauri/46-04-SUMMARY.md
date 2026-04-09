---
phase: 46
plan: 4
subsystem: setup
tags: [daemon, pairing, setup, bootstrap, facade]
requires:
  - phase: 46-03
    provides: daemon-backed setup pairing facade adapter scaffold
provides:
  - app-layer setup pairing facade contract
  - setup assembly wiring via facade abstraction
  - setup runtime removal of concrete pairing orchestrator dependency
affects: [setup_flow, uc-bootstrap, daemon_pairing_host_migration]
tech-stack:
  added: []
  patterns: [app-layer facade port, bootstrap-to-app trait wiring]
key-files:
  created:
    - src-tauri/crates/uc-app/src/usecases/setup/pairing_facade.rs
  modified:
    - src-tauri/crates/uc-app/src/usecases/setup/mod.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs
    - src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs
    - src-tauri/crates/uc-bootstrap/src/assembly.rs
key-decisions:
  - 'SetupPairingFacadePort lives in uc-app and PairingOrchestrator implements it for bootstrap/non-daemon call sites.'
  - 'SetupAssemblyPorts placeholder uses a no-op facade instead of constructing a concrete PairingOrchestrator.'
patterns-established:
  - 'Setup-facing pairing contracts belong to uc-app even when adapters are supplied later by uc-tauri.'
  - 'Bootstrap can wrap setup dependencies in app-owned trait objects without importing uc-tauri adapters.'
requirements-completed: [PH46-06]
duration: 11min
completed: 2026-03-20
---

# Phase 46 Plan 4: Gap Closure For Setup Pairing Facade Extraction Summary

**App-layer setup pairing facade extracted into uc-app and wired through uc-bootstrap so setup orchestration no longer owns a concrete PairingOrchestrator**

## Performance

- **Duration:** 11 min
- **Started:** 2026-03-20T02:52:50Z
- **Completed:** 2026-03-20T03:03:36Z
- **Tasks:** 1
- **Files modified:** 6

## Accomplishments

- Added `SetupPairingFacadePort` in `uc-app` with the setup-specific pairing contract and a `PairingOrchestrator` adapter implementation.
- Switched setup action execution and setup orchestration to `Arc<dyn SetupPairingFacadePort>` so setup runtime no longer depends on a concrete pairing orchestrator field.
- Rewired `uc-bootstrap` setup assembly to carry `setup_pairing_facade`, including a no-op placeholder implementation for non-GUI/test construction.

## Task Commits

Each task was committed atomically:

1. **Task 1: Move the setup pairing facade contract into uc-app and wire setup assembly to use it** - `5c14d96b` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/setup/pairing_facade.rs` - App-layer setup pairing trait and `PairingOrchestrator` adapter implementation.
- `src-tauri/crates/uc-app/src/usecases/setup/mod.rs` - Exports the setup pairing facade contract.
- `src-tauri/crates/uc-app/src/usecases/mod.rs` - Re-exports the setup pairing facade for cross-crate wiring.
- `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs` - Delegates setup pairing subscribe/initiate/accept/reject calls through the facade trait.
- `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs` - Removes the concrete setup pairing dependency from orchestrator construction.
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` - Carries `setup_pairing_facade` in `SetupAssemblyPorts`, adds a placeholder facade, and verifies the new boundary with a unit test.

## Decisions Made

- Moved the setup pairing contract into `uc-app` so `uc-bootstrap` can compose setup without depending on a trait defined in `uc-tauri`.
- Implemented `SetupPairingFacadePort` directly for `PairingOrchestrator` so existing bootstrap and test call sites can coerce to the app-layer trait without adding extra adapter glue in this plan.
- Kept the `uc-tauri` daemon-backed adapter untouched for Plan 46-05, limiting this plan to the app/composition boundary change.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

Plan 46-05 can now bind the `uc-tauri` daemon-backed setup adapter to the app-layer `SetupPairingFacadePort` without further changes to setup orchestration internals.

## Self-Check: PASSED

- FOUND: `.planning/phases/46-daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri/46-04-SUMMARY.md`
- FOUND: `5c14d96b`
