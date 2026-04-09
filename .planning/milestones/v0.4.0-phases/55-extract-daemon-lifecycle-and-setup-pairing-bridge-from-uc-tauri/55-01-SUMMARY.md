---
phase: 55-extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri
plan: "01"
subsystem: infra
tags: [rust, daemon, lifecycle, process-management, uc-daemon-client]

# Dependency graph
requires:
  - phase: 54-extract-daemon-client
    provides: uc-daemon-client crate foundation with connection and realtime modules
provides:
  - daemon_lifecycle module in uc-daemon-client with GuiOwnedDaemonState, OwnedDaemonChild, SpawnReason, DaemonExitCleanupError
  - Self-contained TerminateDaemonError and terminate_local_daemon_pid in uc-daemon-client
affects: [55-02, uc-tauri bootstrap migration]

# Tech tracking
tech-stack:
  added: [thiserror = "2.0" in uc-daemon-client]
  patterns: [Inline dependency extraction — copy module to target crate and inline its private dependencies rather than creating cross-crate imports]

key-files:
  created:
    - src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs
  modified:
    - src-tauri/crates/uc-daemon-client/src/lib.rs
    - src-tauri/crates/uc-daemon-client/Cargo.toml

key-decisions:
  - "thiserror added to uc-daemon-client since daemon_lifecycle uses #[derive(Error)] on DaemonExitCleanupError"
  - "terminate_local_daemon_pid uses TerminateDaemonError (not DaemonBootstrapError) to avoid coupling uc-daemon-client to uc-tauri error types"
  - "TerminateDaemonError is pub(crate) scoped — callers see DaemonExitCleanupError details via .to_string(), not the inner type"

patterns-established:
  - "Module inline extraction: when migrating a module, inline its private dependencies (functions imported via super::) rather than adding cross-crate imports"

requirements-completed: []

# Metrics
duration: 5min
completed: 2026-03-24
---

# Phase 55 Plan 01: Create daemon_lifecycle module in uc-daemon-client Summary

**GuiOwnedDaemonState and daemon exit cleanup logic migrated from uc-tauri bootstrap to uc-daemon-client with inlined terminate_local_daemon_pid and self-contained TerminateDaemonError**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-24T11:34:51Z
- **Completed:** 2026-03-24T11:39:55Z
- **Tasks:** 4 (create file, update lib.rs, verify build, commit)
- **Files modified:** 4

## Accomplishments

- Created `uc-daemon-client/src/daemon_lifecycle.rs` with full `GuiOwnedDaemonState` implementation
- Inlined `terminate_local_daemon_pid` with `TerminateDaemonError` to avoid cross-crate coupling
- Added `pub mod daemon_lifecycle` and re-exports to `lib.rs`
- Added `thiserror = "2.0"` to `uc-daemon-client/Cargo.toml`
- All 3 unit tests pass: `record_spawned_tracks_pid_and_reason`, `begin_exit_cleanup_is_idempotent_until_finished`, `clear_removes_owned_child_snapshot`

## Task Commits

Each task was committed atomically:

1. **Task 1-4: Create daemon_lifecycle module** - `9a28c03a` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs` - GuiOwnedDaemonState, OwnedDaemonChild, SpawnReason, DaemonExitCleanupError, terminate_local_daemon_pid, TerminateDaemonError
- `src-tauri/crates/uc-daemon-client/src/lib.rs` - Added `pub mod daemon_lifecycle` and re-exports
- `src-tauri/crates/uc-daemon-client/Cargo.toml` - Added thiserror = "2.0"

## Decisions Made

- Added `thiserror = "2.0"` as a new dependency because `DaemonExitCleanupError` uses `#[derive(Error)]` from thiserror (was not previously in `uc-daemon-client`'s dependencies).
- `terminate_local_daemon_pid` uses `TerminateDaemonError` (a plain struct wrapping `String`) instead of the original `DaemonBootstrapError::IncompatibleDaemon` to keep `uc-daemon-client` decoupled from `uc-tauri` error types.
- `TerminateDaemonError` is not re-exported from `lib.rs` (stays as `pub(crate)`) since it's an internal implementation detail of the shutdown sequence.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added thiserror dependency to uc-daemon-client Cargo.toml**

- **Found during:** Step 1 (creating daemon_lifecycle.rs)
- **Issue:** `daemon_lifecycle.rs` uses `#[derive(thiserror::Error)]` on `DaemonExitCleanupError`, but `thiserror` was not listed in `uc-daemon-client/Cargo.toml`
- **Fix:** Added `thiserror = "2.0"` to the `[dependencies]` section of `uc-daemon-client/Cargo.toml`
- **Files modified:** `src-tauri/crates/uc-daemon-client/Cargo.toml`
- **Verification:** `cargo check -p uc-daemon-client` exits with code 0
- **Committed in:** `9a28c03a` (task commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary missing dependency — no scope creep.

## Issues Encountered

- Worktree `agent-a25e2598` was based on `main` (87677b87), not `phase55` branch. The `uc-daemon-client` crate and `daemon_lifecycle.rs` source only exist on `phase55`. Reset worktree to `origin/phase55` before execution.

## Next Phase Readiness

- `uc-daemon-client` now has `daemon_lifecycle` module ready for plan 55-02 to update `uc-tauri/bootstrap/` call sites to import from `uc-daemon-client` instead of the local module.
- `cargo check -p uc-daemon-client` passes with all 3 tests.

## Self-Check: PASSED

- FOUND: `src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs`
- FOUND: `src-tauri/crates/uc-daemon-client/src/lib.rs`
- FOUND: task commit `9a28c03a`

---
*Phase: 55-extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri*
*Completed: 2026-03-24*
