---
phase: 11-command-contract-hardening
plan: '02'
subsystem: api
tags: [tauri-commands, error-handling, typed-errors, serde, contract-hardening]

# Dependency graph
requires:
  - phase: 11-command-contract-hardening
    plan: '01'
    provides: Typed DTO returns for setup/pairing/lifecycle commands
provides:
  - CommandError enum with 6 typed variants (NotFound, InternalError, Timeout, Cancelled, ValidationError, Conflict)
  - All 5 command modules return Result<T, CommandError> instead of Result<T, String>
  - spawn_blocking cancel/panic distinction via CommandError::Cancelled vs InternalError
  - get_settings returns typed Result<Settings, CommandError> (no serde_json::Value)
  - Settings payload shape regression test
affects: [frontend-error-handling, 12-01-PLAN]

# Tech tracking
tech-stack:
  added: []
  patterns: [typed-command-errors, tagged-serde-error-enum, cancel-vs-panic-distinction]

key-files:
  created:
    - src-tauri/crates/uc-tauri/tests/command_error_test.rs
  modified:
    - src-tauri/crates/uc-tauri/src/commands/error.rs
    - src-tauri/crates/uc-tauri/src/commands/mod.rs
    - src-tauri/crates/uc-tauri/src/commands/clipboard.rs
    - src-tauri/crates/uc-tauri/src/commands/pairing.rs
    - src-tauri/crates/uc-tauri/src/commands/setup.rs
    - src-tauri/crates/uc-tauri/src/commands/lifecycle.rs
    - src-tauri/crates/uc-tauri/src/commands/settings.rs
    - src-tauri/crates/uc-tauri/tests/models_serialization_test.rs

key-decisions:
  - 'CommandError tests placed in integration test file (tests/command_error_test.rs) due to pre-existing encryption.rs lib test compile failures'
  - 'Settings get_settings diagnostic logging adapted for Option<String> device_name field'

patterns-established:
  - 'CommandError::internal() convenience constructor for wrapping any Display as InternalError'
  - 'Cancelled vs InternalError distinction for spawn_blocking join errors using is_cancelled()'
  - 'ValidationError for user-input/mode validation failures, NotFound for missing entities'

requirements-completed: [CONTRACT-01, CONTRACT-02, CONTRACT-04]

# Metrics
duration: 10min
completed: 2026-03-06
---

# Phase 11 Plan 02: Typed Command Error Taxonomy Summary

**CommandError enum with 6 variants replacing opaque String errors across all command handlers, with cancel/panic distinction and typed Settings return**

## Performance

- **Duration:** 10 min
- **Started:** 2026-03-06T12:51:26Z
- **Completed:** 2026-03-06T13:01:15Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments

- CommandError enum with 6 variants serializing to `{code: "...", message: "..."}` for frontend discriminated union handling
- All 5 command modules (clipboard, pairing, setup, lifecycle, settings) migrated from `Result<T, String>` to `Result<T, CommandError>`
- spawn_blocking in clipboard.rs distinguishes cancellation (`CommandError::Cancelled`) from panic (`CommandError::InternalError`) using `is_cancelled()`
- get_settings returns `Result<Settings, CommandError>` directly -- no intermediate `serde_json::Value` conversion
- 10 integration tests: 5 for CommandError serialization/display, 1 for Settings payload shape, 4 pre-existing DTO tests still passing

## Task Commits

Each task was committed atomically:

1. **Task 1: Define CommandError enum in error.rs with serialization tests** - `32e4ba6a` (feat)
2. **Task 2: Migrate all command handlers from String errors to CommandError; fix get_settings typed return** - `18cfbe31` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/commands/error.rs` - CommandError enum with 6 variants, internal() convenience constructor
- `src-tauri/crates/uc-tauri/src/commands/mod.rs` - Re-exports CommandError instead of map_err
- `src-tauri/crates/uc-tauri/tests/command_error_test.rs` - 5 serialization/display tests
- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` - All returns use CommandError; spawn_blocking uses Cancelled/InternalError
- `src-tauri/crates/uc-tauri/src/commands/pairing.rs` - All 10 handlers use CommandError
- `src-tauri/crates/uc-tauri/src/commands/setup.rs` - All 8 handlers use CommandError::internal
- `src-tauri/crates/uc-tauri/src/commands/lifecycle.rs` - Both handlers use CommandError
- `src-tauri/crates/uc-tauri/src/commands/settings.rs` - get_settings returns Settings directly; parse error uses ValidationError
- `src-tauri/crates/uc-tauri/tests/models_serialization_test.rs` - Added Settings payload shape test

## Decisions Made

- CommandError tests placed in integration test file (`tests/command_error_test.rs`) instead of inline `#[cfg(test)]` due to pre-existing broken imports in `encryption.rs` test module that prevent `--lib` test compilation (same issue as 11-01)
- Settings diagnostic logging adapted for `Option<String>` device_name field (using `.is_some()` and `.as_deref().map(|s| s.len())` instead of `.is_empty()` and `.len()`)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Settings device_name is Option<String>, not String**

- **Found during:** Task 2
- **Issue:** get_settings diagnostic logging used `.is_empty()` and `.len()` assuming String, but `general.device_name` is `Option<String>`
- **Fix:** Changed to `.is_some()` and `.as_deref().map(|s| s.len())` for Option-compatible access
- **Files modified:** `src-tauri/crates/uc-tauri/src/commands/settings.rs`
- **Verification:** `cargo check -p uc-tauri` compiles clean
- **Committed in:** 18cfbe31 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minor type adaptation, no scope change.

## Issues Encountered

- Pre-existing broken imports in `encryption.rs` test module continue to prevent `--lib` test compilation. All new tests use integration test files as workaround (consistent with 11-01 approach).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All command surfaces in scope now return typed DTOs with typed errors
- Frontend can use `error.code` as discriminant for structured error handling
- Phase 11 complete; Phase 12 (Lifecycle Governance Baseline) can proceed

---

_Phase: 11-command-contract-hardening_
_Completed: 2026-03-06_
