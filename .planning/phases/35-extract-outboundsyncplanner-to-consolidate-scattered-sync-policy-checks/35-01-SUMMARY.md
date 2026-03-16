---
phase: 35-extract-outboundsyncplanner-to-consolidate-scattered-sync-policy-checks
plan: 01
subsystem: sync
tags: [rust, uc-app, outbound-sync, file-sync, clipboard-sync, tdd, pure-function]

# Dependency graph
requires:
  - phase: 33-fix-file-sync-eventual-consistency
    provides: FileCandidate pre-computation pattern and file sync architecture
  - phase: 28-file-sync-foundation
    provides: FileTransferMapping, FileSyncSettings types
provides:
  - OutboundSyncPlanner struct with plan() method in uc-app::usecases::sync_planner
  - FileCandidate, OutboundSyncPlan, ClipboardSyncIntent, FileSyncIntent types
  - 12 unit tests covering all sync eligibility scenarios
affects:
  - 35-02 (will use OutboundSyncPlanner in AppRuntime.on_clipboard_changed integration)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - 'Pure domain service pattern: planner performs NO filesystem I/O; runtime pre-computes FileCandidate with path+size'
    - 'extracted_paths_count parameter for detecting all_files_excluded when metadata() failures occur before planner'
    - 'Infallible async method returning safe defaults on settings load failure'

key-files:
  created:
    - src-tauri/crates/uc-app/src/usecases/sync_planner/mod.rs
    - src-tauri/crates/uc-app/src/usecases/sync_planner/types.rs
    - src-tauri/crates/uc-app/src/usecases/sync_planner/planner.rs
  modified:
    - src-tauri/crates/uc-app/src/usecases/mod.rs

key-decisions:
  - 'all_files_excluded guard only triggers when file_sync_attempted (LocalCapture + file_sync_enabled) AND extracted_paths_count > 0 AND eligible_files is empty — prevents false suppression when file_sync is disabled'
  - 'LocalRestore preserves existing behavior (clipboard: Some) — only RemotePush is skipped'
  - 'file_sync_attempted flag computed from settings at plan() call time, not stored on struct, keeping planner stateless'

patterns-established:
  - 'OutboundSyncPlanner: domain service taking Arc<dyn SettingsPort>, returns infallible OutboundSyncPlan'
  - 'FileCandidate: value object with pre-computed path+size from runtime; planner never calls std::fs'

requirements-completed:
  - SYNCPLAN-01
  - SYNCPLAN-02

# Metrics
duration: 7min
completed: 2026-03-16
---

# Phase 35 Plan 01: OutboundSyncPlanner Types and Implementation Summary

**Pure domain service OutboundSyncPlanner consolidating all outbound sync eligibility decisions into a single plan() call with 12 unit tests, zero filesystem I/O**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-16T07:02:46Z
- **Completed:** 2026-03-16T07:09:38Z
- **Tasks:** 1 (TDD: RED + GREEN in single implementation pass)
- **Files modified:** 4

## Accomplishments

- Implemented `OutboundSyncPlanner::plan()` consolidating RemotePush skip, LocalRestore passthrough, file_sync_enabled guard, file size filtering, transfer_id generation, and all_files_excluded guard
- Defined `FileCandidate`, `OutboundSyncPlan`, `ClipboardSyncIntent`, `FileSyncIntent` types as pure value objects
- 12 unit tests covering all 9 required scenarios plus boundary cases (exact limit, 1-byte over, transfer_id consistency)
- Planner contains zero `std::fs` calls — all file sizes come from `FileCandidate.size` pre-computed by runtime

## Task Commits

1. **Task 1: OutboundSyncPlanner implementation with TDD** - `5a27ef83` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/sync_planner/types.rs` — FileCandidate, OutboundSyncPlan, ClipboardSyncIntent, FileSyncIntent types
- `src-tauri/crates/uc-app/src/usecases/sync_planner/planner.rs` — OutboundSyncPlanner struct, plan() method, 12 unit tests
- `src-tauri/crates/uc-app/src/usecases/sync_planner/mod.rs` — module declaration and pub use re-exports
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — added `pub mod sync_planner` and re-exports

## Decisions Made

- **all_files_excluded guard scoped to file_sync_attempted:** When `file_sync_enabled = false`, `extracted_paths_count` is irrelevant. Gate the guard on `origin == LocalCapture && file_sync_enabled` to avoid falsely suppressing clipboard sync when file sync is disabled but snapshot contained file paths.
- **LocalRestore preserves clipboard sync:** Matches existing behavior in sync_outbound.rs line 177 where only RemotePush is skipped. LocalRestore → clipboard: Some, files: [].
- **Infallible plan() on settings failure:** Returns `clipboard: Some, files: []` as safe defaults — clipboard sync allowed, no file sync risk.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed all_files_excluded false-positive when file_sync is disabled**

- **Found during:** Task 1 (running tests)
- **Issue:** `test_local_capture_file_sync_disabled` failed — when `file_sync_enabled = false`, the planner still checked `extracted_paths_count > 0 && eligible_files.is_empty()`, triggering all_files_excluded even though file sync was never attempted
- **Fix:** Added `file_sync_attempted` boolean guard (`origin == LocalCapture && file_sync_enabled`) before the all_files_excluded check
- **Files modified:** `planner.rs`
- **Verification:** All 12 tests pass after fix
- **Committed in:** `5a27ef83` (same task commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 - Bug)
**Impact on plan:** Essential correctness fix discovered during test execution. No scope creep.

## Issues Encountered

- Import paths `uc_core::clipboard::change::ClipboardChangeOrigin` and `uc_core::clipboard::system::SystemClipboardSnapshot` were private modules — resolved by using public re-exports `uc_core::ClipboardChangeOrigin` and `uc_core::SystemClipboardSnapshot` (via `pub use clipboard::*` in lib.rs)
- `FileTransferMapping` path was `uc_core::network::protocol::FileTransferMapping` (not `::clipboard::`) — resolved by checking protocol/mod.rs

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `OutboundSyncPlanner` is ready for integration into `AppRuntime.on_clipboard_changed()` (Plan 35-02)
- All types pub-exported from `uc_app::usecases::sync_planner`
- No new crate dependencies added

---

_Phase: 35-extract-outboundsyncplanner-to-consolidate-scattered-sync-policy-checks_
_Completed: 2026-03-16_
