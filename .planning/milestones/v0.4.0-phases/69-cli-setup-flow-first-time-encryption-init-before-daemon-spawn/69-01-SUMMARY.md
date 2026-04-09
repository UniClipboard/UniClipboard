---
phase: 69-cli-setup-flow-first-time-encryption-init-before-daemon-spawn
plan: 01
subsystem: cli
tags: [rust, cli, encryption, setup, uc-cli, uc-bootstrap, uc-app]

# Dependency graph
requires:
  - phase: uc-bootstrap
    provides: build_cli_runtime() for direct encryption init without daemon

provides:
  - run_new_space() rewritten to use build_cli_runtime() + CoreUseCases::initialize_encryption()
  - new_space_encryption_guard() pure function for Initialized state rejection
  - Behavioral tests for guard logic in setup_cli.rs

affects:
  - CLI setup flow first-time UX (no more macOS Keychain popup from daemon startup)
  - Any future CLI commands that need direct encryption init without daemon

# Tech tracking
tech-stack:
  added: []
  patterns:
    - CLI commands using build_cli_runtime() for daemon-free encryption operations (same pattern as space_status.rs)
    - Pure guard functions for state validation extracted from async flows for testability

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-cli/src/commands/setup.rs
    - src-tauri/crates/uc-cli/tests/setup_cli.rs

key-decisions:
  - "run_new_space() uses build_cli_runtime() directly (no daemon) for first-time encryption init, matching space_status.rs pattern"
  - "new_space_encryption_guard() extracted as pub fn for behavioral testability without async runtime"
  - "Initializing state treated same as Uninitialized by guard (allowed to proceed) - only Initialized is rejected"

patterns-established:
  - "CLI encryption init pattern: build_cli_runtime() -> encryption_state() -> guard -> CoreUseCases::initialize_encryption()"

requirements-completed: [PH69-01, PH69-02, PH69-03]

# Metrics
duration: 4min
completed: 2026-03-28
---

# Phase 69 Plan 01: CLI Setup Flow First-Time Encryption Init Summary

**Rewrote run_new_space() to initialize encryption locally via build_cli_runtime() + CoreUseCases::initialize_encryption(), eliminating daemon startup and macOS Keychain popup during first-time space creation**

## Performance

- **Duration:** 4min
- **Started:** 2026-03-28T09:17:21Z
- **Completed:** 2026-03-28T09:21:46Z
- **Tasks:** 3 (0 + 1 + 2)
- **Files modified:** 2

## Accomplishments

- Added `new_space_encryption_guard()` pure function to setup.rs that rejects if EncryptionState::Initialized
- Rewrote `run_new_space()` to use `build_cli_runtime()` + `CoreUseCases::initialize_encryption()` without any daemon involvement
- Added behavioral tests verifying guard contract: Initialized -> Err(EXIT_ERROR), Uninitialized -> Ok(())
- `run_host()`, `run_join()`, `run_reset()` unchanged (still require daemon)

## Task Commits

1. **Task 0: Add behavioral tests for new_space encryption guard** - `9c49d69c` (test)
2. **Task 1: Rewrite run_new_space() to use local CLI runtime instead of daemon** - `2efd7bc1` (feat)
3. **Task 2: Verify full test suite and cross-crate compilation** - (no code changes, verification only)

## Files Created/Modified

- `src-tauri/crates/uc-cli/src/commands/setup.rs` - Added `new_space_encryption_guard()`, rewrote `run_new_space()` to use build_cli_runtime() + CoreUseCases::initialize_encryption(), added uc_app/uc_core imports
- `src-tauri/crates/uc-cli/tests/setup_cli.rs` - Added `new_space_already_initialized_returns_error` and `new_space_uninitialized_allows_init` behavioral tests

## Decisions Made

- Used `build_cli_runtime()` pattern from `space_status.rs` (already established in codebase) rather than inventing a new approach
- Treated `EncryptionState::Initializing` as allowed (same as Uninitialized) in the guard — only `Initialized` is rejected
- Updated next-step hint from "run setup host" to "run uniclipboard-daemon first, then setup host" to reflect the new two-step flow

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

`cli_smoke.rs` test failures (6 tests) were verified as pre-existing failures unrelated to this plan's changes. Confirmed by running tests before and after stash - identical failures in both cases.

## Next Phase Readiness

- First-time setup flow is now daemon-free for space creation
- `setup host` still requires daemon (by design — pairing is a network operation)
- No blockers for subsequent phases

---
*Phase: 69-cli-setup-flow-first-time-encryption-init-before-daemon-spawn*
*Completed: 2026-03-28*
