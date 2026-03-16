---
phase: 35-extract-outboundsyncplanner-to-consolidate-scattered-sync-policy-checks
plan: 02
subsystem: infra
tags: [rust, sync, outbound-sync, planner, file-sync, clipboard-sync]

# Dependency graph
requires:
  - phase: 35-01
    provides: OutboundSyncPlanner, FileCandidate, FileSyncIntent, ClipboardSyncIntent, OutboundSyncPlan types
provides:
  - runtime.rs on_clipboard_changed() delegates all policy decisions to OutboundSyncPlanner::plan()
  - SyncOutboundFileUseCase without file_sync_enabled and max_file_size guards
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Runtime builds Vec<FileCandidate> with pre-computed sizes via std::fs::metadata() BEFORE calling planner (all fs I/O stays in platform layer)
    - Planner receives extracted_paths_count captured BEFORE metadata filtering to detect all_files_excluded
    - Dispatcher pattern: plan.clipboard and plan.files drive spawn decisions in runtime

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-app/src/usecases/file_sync/sync_outbound.rs

key-decisions:
  - 'runtime.rs retains extract_file_paths_from_snapshot() + std::fs::metadata() calls (platform layer owns all fs I/O)'
  - 'extracted_paths_count captured from resolved_paths.len() BEFORE metadata filter; passed to plan() for all_files_excluded detection'
  - 'SyncOutboundFileUseCase settings field kept (apply_file_sync_policy still needs it); only the two entry-guard loads removed'
  - 'Runtime-level unit test added directly testing the extracted_paths_count-before-filter invariant using a non-existent path'

patterns-established:
  - 'Platform layer (uc-tauri) owns all filesystem I/O; application layer (uc-app) receives pre-computed data'
  - 'Infallible planner: settings failure returns safe defaults rather than propagating errors into the event loop'

requirements-completed:
  - SYNCPLAN-03
  - SYNCPLAN-04

# Metrics
duration: 10min
completed: 2026-03-16
---

# Phase 35 Plan 02: Wire OutboundSyncPlanner and Remove Redundant Guards Summary

**OutboundSyncPlanner wired into runtime.rs replacing 3-stage inline policy; SyncOutboundFileUseCase stripped of file_sync_enabled and max_file_size guards**

## Performance

- **Duration:** ~10 min
- **Started:** 2026-03-16T08:11:00Z
- **Completed:** 2026-03-16T08:21:04Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Replaced 3-stage inline policy logic in `on_clipboard_changed()` with a single `OutboundSyncPlanner::plan()` call
- Runtime now builds `Vec<FileCandidate>` from APFS-resolved paths via `std::fs::metadata()` before calling the planner, keeping all fs I/O in the platform layer
- `extracted_paths_count` is captured from `resolved_paths.len()` before metadata filtering, enabling the planner to detect the `all_files_excluded` case correctly
- Removed `file_sync_enabled` guard and `max_file_size` guard from `SyncOutboundFileUseCase::execute()` since the planner now pre-conditions both
- Added runtime-level unit test verifying `extracted_paths_count` is captured before metadata filter (non-existent path scenario)
- All 277 uc-app + 191 uc-tauri tests pass; full project `cargo check` passes

## Task Commits

1. **Task 1: Wire OutboundSyncPlanner into runtime.rs on_clipboard_changed()** - `1681394b` (feat)
2. **Task 2: Remove redundant guards from SyncOutboundFileUseCase** - `c5362108` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Replaced inline 3-stage policy with OutboundSyncPlanner dispatch; added runtime unit test for the extracted_paths_count boundary
- `src-tauri/crates/uc-app/src/usecases/file_sync/sync_outbound.rs` - Removed file_sync_enabled guard (lines 54-67) and max_file_size guard (lines 103-122); removed 2 corresponding tests

## Decisions Made

- `extract_file_paths_from_snapshot()` stays in runtime.rs unchanged — APFS resolution and URI parsing belong in the platform layer
- `SyncOutboundFileUseCase` retains the `settings` field because `apply_file_sync_policy()` still needs it for per-peer filtering
- Unit test for the extracted_paths_count boundary added to `runtime.rs` tests module (non-tokio #[test]) since it only needs synchronous logic

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## Self-Check: PASSED

- runtime.rs exists and contains OutboundSyncPlanner wiring
- sync_outbound.rs exists with guards removed
- 35-02-SUMMARY.md created
- Commit 1681394b: Task 1 (wire planner)
- Commit c5362108: Task 2 (remove guards)

## Next Phase Readiness

Phase 35 is complete. The OutboundSyncPlanner is fully integrated:

- Plan 01 created the planner and types with comprehensive tests
- Plan 02 wired it into the runtime and cleaned up the use case

No blockers.

---

_Phase: 35-extract-outboundsyncplanner-to-consolidate-scattered-sync-policy-checks_
_Completed: 2026-03-16_
