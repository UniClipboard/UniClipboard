---
phase: 11-command-contract-hardening
plan: '01'
subsystem: api
tags: [tauri-commands, dto, serde, serialization, contract-hardening]

# Dependency graph
requires:
  - phase: 10-boundary-repair-baseline
    provides: Command layer access restricted to runtime.usecases() pattern
provides:
  - LifecycleStatusDto in uc-tauri models with camelCase serialization
  - Setup commands return typed SetupState (no double-encode)
  - list_paired_devices returns Vec<PairedPeer> DTO (no domain model leakage)
  - get_lifecycle_status returns typed LifecycleStatusDto (no JSON string)
  - Frontend setup.ts uses direct Tauri invoke results (no JSON.parse shim)
  - Serialization regression tests for all command-layer DTOs
affects: [11-02-PLAN, frontend-api-consumers]

# Tech tracking
tech-stack:
  added: []
  patterns: [typed-command-returns, dto-mapping-layer, serialization-regression-tests]

key-files:
  created:
    - src-tauri/crates/uc-tauri/tests/models_serialization_test.rs
  modified:
    - src-tauri/crates/uc-tauri/src/models/mod.rs
    - src-tauri/crates/uc-tauri/src/commands/setup.rs
    - src-tauri/crates/uc-tauri/src/commands/pairing.rs
    - src-tauri/crates/uc-tauri/src/commands/lifecycle.rs
    - src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs
    - src/api/setup.ts

key-decisions:
  - 'LifecycleStatusDto wraps LifecycleState enum in a struct with camelCase serde, not re-exporting the enum directly'
  - 'Added Deserialize to LifecycleState enum to support DTO round-trip testing'
  - 'Tests placed in integration test file (tests/) instead of inline cfg(test) due to pre-existing encryption.rs test compile failures'

patterns-established:
  - 'DTO serialization regression tests: verify both camelCase new DTOs and snake_case existing DTOs'
  - 'Command return types must be typed DTOs, not JSON strings'

requirements-completed: [CONTRACT-01, CONTRACT-03]

# Metrics
duration: 10min
completed: 2026-03-06
---

# Phase 11 Plan 01: DTO Mapping Layer Summary

**Typed command return contracts replacing double-encoded JSON strings and domain model leakage across setup, pairing, and lifecycle commands**

## Performance

- **Duration:** 10 min
- **Started:** 2026-03-06T12:39:11Z
- **Completed:** 2026-03-06T12:49:28Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- All 8 setup commands now return typed `Result<SetupState, String>` instead of double-encoded JSON strings
- `list_paired_devices` returns `Vec<PairedPeer>` DTO instead of leaking `PairedDevice` domain model
- `get_lifecycle_status` returns typed `LifecycleStatusDto` instead of JSON-encoded string
- Frontend `setup.ts` simplified by removing `decodeSetupState` JSON.parse shim
- 4 serialization regression tests covering camelCase and snake_case DTO conventions

## Task Commits

Each task was committed atomically:

1. **Task 1: Add LifecycleStatusDto to models and write serialization tests** - `160eb578` (feat)
2. **Task 2: Fix domain model leakage in setup.rs, pairing.rs, lifecycle.rs and update frontend setup.ts** - `278bc390` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/models/mod.rs` - Added LifecycleStatusDto struct with camelCase serde and from_state constructor
- `src-tauri/crates/uc-tauri/tests/models_serialization_test.rs` - 4 serialization regression tests for all DTOs
- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` - Added Deserialize derive to LifecycleState enum
- `src-tauri/crates/uc-tauri/src/commands/setup.rs` - Removed encode_setup_state; all commands return Result<SetupState, String>
- `src-tauri/crates/uc-tauri/src/commands/pairing.rs` - list_paired_devices returns Vec<PairedPeer> with map_paired_device_to_peer
- `src-tauri/crates/uc-tauri/src/commands/lifecycle.rs` - get_lifecycle_status returns LifecycleStatusDto
- `src/api/setup.ts` - Removed decodeSetupState; all functions use direct invoke result

## Decisions Made

- LifecycleStatusDto wraps the LifecycleState enum in a struct (with `state` field) rather than re-exporting the enum, to maintain the camelCase DTO convention and allow future field additions
- Added `Deserialize` derive to LifecycleState to enable DTO round-trip in tests
- Serialization tests placed in `tests/models_serialization_test.rs` (integration test) rather than inline `#[cfg(test)]` because pre-existing broken imports in `encryption.rs` test module prevent `--lib` test compilation

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] LifecycleState missing Deserialize derive**

- **Found during:** Task 1
- **Issue:** LifecycleStatusDto needs Deserialize for round-trip testing, but LifecycleState only had Serialize
- **Fix:** Added `serde::Deserialize` to LifecycleState derive in uc-app
- **Files modified:** `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs`
- **Verification:** Tests compile and pass
- **Committed in:** 160eb578 (Task 1 commit)

**2. [Rule 3 - Blocking] Pre-existing test compilation failure in encryption.rs**

- **Found during:** Task 1
- **Issue:** `cargo test -p uc-tauri --lib` fails due to broken imports in encryption.rs test module (out of scope)
- **Fix:** Created integration test file `tests/models_serialization_test.rs` instead of inline tests
- **Files modified:** `src-tauri/crates/uc-tauri/tests/models_serialization_test.rs`
- **Verification:** `cargo test -p uc-tauri --test models_serialization_test` passes (4/4)
- **Committed in:** 160eb578 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both necessary for task completion. Test placement differs from plan but achieves same coverage. No scope creep.

## Issues Encountered

- Pre-existing broken imports in `src-tauri/crates/uc-tauri/src/commands/encryption.rs` test module prevent ALL `--lib` tests from compiling. Logged to `deferred-items.md`.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- DTO mapping layer established; plan 11-02 can proceed with settings command hardening
- All command surfaces in scope now return typed DTOs
- Serialization test patterns established for future DTO additions

---

_Phase: 11-command-contract-hardening_
_Completed: 2026-03-06_
