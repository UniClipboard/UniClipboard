---
phase: 13-responsibility-decomposition-testability
plan: 01
subsystem: testing, architecture
tags: [noop-mocks, dependency-injection, domain-grouping, hexagonal-architecture]

# Dependency graph
requires:
  - phase: 12-lifecycle-governance-baseline
    provides: TaskRegistry lifecycle and staged store injection patterns
provides:
  - Shared noop/mock test infrastructure in uc-app::testing module
  - Domain-grouped AppDeps sub-structs (ClipboardPorts, SecurityPorts, DevicePorts, StoragePorts, SystemPorts)
affects: [13-02, 13-03, uc-tauri-bootstrap, uc-app-usecases]

# Tech tracking
tech-stack:
  added: []
  patterns: [domain-grouped-dependency-structs, shared-test-infrastructure-module]

key-files:
  created:
    - src-tauri/crates/uc-app/src/testing.rs
  modified:
    - src-tauri/crates/uc-app/src/deps.rs
    - src-tauri/crates/uc-app/src/lib.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs

key-decisions:
  - 'testing.rs module is pub (not #[cfg(test)]) to allow integration tests in tests/ directory to import shared noops'
  - 'paired_device_repo merged into DevicePorts sub-struct since pairing is device-related'

patterns-established:
  - 'Domain sub-struct pattern: group related ports into ClipboardPorts, SecurityPorts, etc. for reduced coupling'
  - 'Shared test infrastructure: import noops from uc_app::testing instead of defining inline duplicates'

requirements-completed: [DECOMP-02, DECOMP-03]

# Metrics
duration: 8min
completed: 2026-03-06
---

# Phase 13 Plan 01: Test Infrastructure and AppDeps Restructuring Summary

**Shared noop test module with 14 consolidated noop implementations and AppDeps restructured from 30+ flat fields into 5 domain-grouped sub-structs**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-06T15:55:25Z
- **Completed:** 2026-03-06T16:03:51Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Consolidated duplicate noop implementations into `uc-app/src/testing.rs` (NoopPairedDeviceRepository, NoopDiscoveryPort, NoopNetworkControl, NoopSetupEventPort, NoopPairingTransport, NoopSpaceAccessTransport, NoopProofPort, NoopTimerPort, NoopSpaceAccessPersistence, NoopSessionReadyEmitter, NoopLifecycleStatus, NoopLifecycleEventEmitter)
- Created 5 domain sub-structs: ClipboardPorts (12 fields), SecurityPorts (6 fields), DevicePorts (3 fields), StoragePorts (5 fields), SystemPorts (2 fields)
- Updated all consumer sites in runtime.rs and wiring.rs to use `deps.group.field` access pattern
- All 155 lib tests and 7 integration tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Create shared testing module and consolidate duplicate noops** - `edda6880` (feat)
2. **Task 2: Group AppDeps into domain sub-structs and update all consumers** - `4c61f708` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/testing.rs` - Shared noop/mock implementations for 14 port traits
- `src-tauri/crates/uc-app/src/deps.rs` - Domain sub-structs and restructured AppDeps
- `src-tauri/crates/uc-app/src/lib.rs` - Re-exports for sub-struct types
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Updated all deps field access to sub-struct paths
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Updated AppDeps construction to use sub-struct syntax

## Decisions Made

- `testing.rs` is exposed as `pub mod testing` (not `#[cfg(test)]`) because integration tests in `tests/` need to import shared noops as an external crate dependency
- `paired_device_repo` was merged into `DevicePorts` since pairing is logically device-related
- Test-specific mocks (MockSetupStatusPort, SucceedEncryption, etc.) remain inline in their respective test modules since they are not simple noops

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed two remaining flat field accesses in runtime.rs**

- **Found during:** Task 2 verification
- **Issue:** `deps.clock` and `deps.clipboard_change_origin` were not updated to sub-struct paths, causing compile errors
- **Fix:** Changed to `deps.system.clock` and `deps.clipboard.clipboard_change_origin`
- **Files modified:** src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
- **Verification:** cargo check passes, all tests green
- **Committed in:** 4c61f708 (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor fix necessary for compilation correctness. No scope creep.

## Issues Encountered

- Pre-existing test compilation failures in `uc-tauri` and `uc-platform` `#[cfg(test)]` modules (missing trait imports for WatcherControlPort, UiPort, AutostartPort, IdentityStoreError). These are NOT caused by our changes and do not affect library compilation or uc-app test suite.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Ready for 13-02 (orchestrator decomposition) - domain sub-structs provide the foundation for splitting orchestrators
- AppDeps sub-structs make it clear which domain each use case depends on
- Shared test infrastructure reduces boilerplate for new test files

---

_Phase: 13-responsibility-decomposition-testability_
_Completed: 2026-03-06_
