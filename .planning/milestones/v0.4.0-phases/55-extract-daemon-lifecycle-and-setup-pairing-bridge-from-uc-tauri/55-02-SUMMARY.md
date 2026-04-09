---
phase: 55-extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri
plan: "02"
subsystem: infra
tags: [rust, daemon-lifecycle, uc-tauri, uc-daemon-client, refactor]

# Dependency graph
requires:
  - phase: 55-01
    provides: daemon_lifecycle.rs migrated to uc-daemon-client with TerminateDaemonError and unit tests

provides:
  - uc-tauri call sites updated to use uc_daemon_client::daemon_lifecycle
  - Local daemon_lifecycle.rs and setup_pairing_bridge.rs deleted from uc-tauri
  - terminate_local_daemon_pid re-exported from uc-tauri::bootstrap::run
  - All integration tests passing with new import paths

affects: [uc-tauri, uc-daemon-client, src-tauri/src/main.rs]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Re-export pattern: pub use uc_daemon_client::daemon_lifecycle::X in run.rs for backward compatibility"
    - "pub visibility on terminate_local_daemon_pid to allow cross-crate access from uc-tauri"

key-files:
  created: []
  modified:
    - src-tauri/src/main.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/mod.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/run.rs
    - src-tauri/crates/uc-tauri/tests/daemon_exit_cleanup.rs
    - src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs
    - src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs
  deleted:
    - src-tauri/crates/uc-tauri/src/bootstrap/daemon_lifecycle.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/setup_pairing_bridge.rs

key-decisions:
  - "Made terminate_local_daemon_pid pub (was pub(crate)) so uc-tauri can re-export via run.rs"
  - "Used pub use in run.rs for terminate_local_daemon_pid re-export to maintain existing call sites"
  - "Error mapping added at terminate_incompatible_daemon_from_pid_file call site to convert TerminateDaemonError -> DaemonBootstrapError"

patterns-established:
  - "Cross-crate pub use re-export pattern for migrated functions keeps existing call sites stable"

requirements-completed: []

# Metrics
duration: 8min
completed: 2026-03-24
---

# Phase 55 Plan 02: Update uc-tauri Call Sites and Delete Migrated Files Summary

**`daemon_lifecycle` fully extracted from uc-tauri: call sites updated to uc_daemon_client, local files deleted, all 12 tests passing**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-24T11:49:58Z
- **Completed:** 2026-03-24T11:58:11Z
- **Tasks:** 10 steps (single commit)
- **Files modified:** 8 (6 modified, 2 deleted)

## Accomplishments

- Updated `main.rs` to import `GuiOwnedDaemonState` directly from `uc_daemon_client::daemon_lifecycle`
- Updated `run.rs`: replaced `use super::daemon_lifecycle::{...}` with `uc_daemon_client::daemon_lifecycle` imports, removed 38-line local `terminate_local_daemon_pid` function, added `pub use` re-export
- Removed `pub mod daemon_lifecycle` and `pub mod setup_pairing_bridge` entries and their re-exports from `mod.rs`
- Updated both integration test files (`daemon_exit_cleanup.rs`, `daemon_bootstrap_contract.rs`) to use `uc_daemon_client::daemon_lifecycle`
- Deleted `daemon_lifecycle.rs` and `setup_pairing_bridge.rs` from `uc-tauri/src/bootstrap/`
- All 12 tests pass: 3 unit tests in uc-daemon-client, 6 exit_cleanup tests, 3 bootstrap_contract tests

## Task Commits

All changes committed atomically:

1. **Steps 1-10: Update all call sites, delete migrated files, verify** - `8cb6a201` (refactor)

## Files Created/Modified

- `src-tauri/src/main.rs` - Changed `GuiOwnedDaemonState` import to `uc_daemon_client::daemon_lifecycle`
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` - Removed `daemon_lifecycle` and `setup_pairing_bridge` mod declarations and re-exports
- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` - Updated imports, removed local `terminate_local_daemon_pid` fn, added `pub use` re-export
- `src-tauri/crates/uc-tauri/tests/daemon_exit_cleanup.rs` - Updated import path
- `src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs` - Updated import path
- `src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs` - Changed `pub(crate)` to `pub` on `terminate_local_daemon_pid`
- DELETED: `src-tauri/crates/uc-tauri/src/bootstrap/daemon_lifecycle.rs`
- DELETED: `src-tauri/crates/uc-tauri/src/bootstrap/setup_pairing_bridge.rs`

## Decisions Made

- Made `terminate_local_daemon_pid` `pub` (was `pub(crate)`) in uc-daemon-client because it needs to be accessible from uc-tauri crate for the `pub use` re-export in `run.rs`
- Used `pub use uc_daemon_client::daemon_lifecycle::terminate_local_daemon_pid` in `run.rs` to maintain any existing callers that reference the function via `run::terminate_local_daemon_pid`
- Added `.map_err(|e| DaemonBootstrapError::IncompatibleDaemon { details: e.to_string() })?` at the call site in `terminate_incompatible_daemon_from_pid_file` to convert `TerminateDaemonError` to `DaemonBootstrapError`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Made `terminate_local_daemon_pid` pub for cross-crate access**
- **Found during:** Step 2 (Update run.rs)
- **Issue:** `terminate_local_daemon_pid` was declared `pub(crate)` in uc-daemon-client but Plan 02 requires re-exporting it from uc-tauri via `pub use`. Cross-crate re-export requires `pub` visibility.
- **Fix:** Changed `pub(crate) fn terminate_local_daemon_pid` to `pub fn terminate_local_daemon_pid` in `uc-daemon-client/src/daemon_lifecycle.rs`
- **Files modified:** `src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs`
- **Verification:** `cargo check -p uc-tauri` exits 0; all integration tests pass
- **Committed in:** `8cb6a201`

---

**Total deviations:** 1 auto-fixed (Rule 1 - Bug: insufficient visibility for planned re-export)
**Impact on plan:** Required fix to enable the planned re-export architecture. No scope creep.

## Issues Encountered

- The current worktree (`worktree-agent-a9ea5a66`) was based on `main` and lacked Plan 01's work. Resolved by resetting to `worktree-agent-a25e2598` branch (which contained Plan 01 commits), then executing Plan 02 on top.

## Next Phase Readiness

- `uc-daemon-client` now owns all daemon lifecycle types and the terminate function
- `uc-tauri` depends on `uc-daemon-client` for these types (correct direction)
- All tests pass; codebase compiles cleanly
- Phase 55 extraction work is complete

---
*Phase: 55-extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri*
*Completed: 2026-03-24*
