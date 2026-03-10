---
phase: 20-clipboard-capture-flow-correlation
plan: 03
subsystem: observability
tags: [tracing, spans, stages, structured-logging]

# Dependency graph
requires:
  - phase: 20-clipboard-capture-flow-correlation
    provides: stage constants module and capture pipeline instrumentation (plans 01, 02)
provides:
  - SPOOL_BLOBS stage constant in uc-observability
  - Distinct spool_blobs stage span in capture pipeline
affects: [20-verification, 21-sync-observability]

# Tech tracking
tech-stack:
  added: []
  patterns: [stage-per-operation span separation]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-observability/src/stages.rs
    - src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs

key-decisions:
  - 'Split cache_representations into two sequential loops rather than one loop with two spans, maintaining clear stage boundaries'

patterns-established:
  - 'Each pipeline stage gets its own constant and span, even when operations share the same loop structure'

requirements-completed: [FLOW-03]

# Metrics
duration: 2min
completed: 2026-03-11
---

# Phase 20 Plan 03: Spool Blobs Stage Span Summary

**Added SPOOL_BLOBS stage constant and split spool_blobs into its own distinct stage span in the capture pipeline**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-10T23:50:23Z
- **Completed:** 2026-03-10T23:52:01Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments

- Added SPOOL_BLOBS constant to uc-observability stages module with passing tests
- Split combined cache_representations block into two distinct stage spans
- cache_representations now covers only cache put operations
- spool_blobs covers only spool queue enqueue logic, closing FLOW-03 gap

## Task Commits

Each task was committed atomically:

1. **Task 1: Add SPOOL_BLOBS constant and split spool_blobs into its own stage span** - `e1686c88` (feat)

**Plan metadata:** (pending)

## Files Created/Modified

- `src-tauri/crates/uc-observability/src/stages.rs` - Added SPOOL_BLOBS constant and updated both test cases
- `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` - Split combined block into cache_representations and spool_blobs stage spans

## Decisions Made

- Split the combined async block into two sequential loops iterating over the same data, rather than using nested spans within a single loop. This keeps each stage span clean and independently measurable in structured logs.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All FLOW-03 gaps are now closed
- spool_blobs appears as a distinct named span with stage=spool_blobs in structured logs
- Ready for Phase 20 verification completion

## Self-Check: PASSED

- [x] stages.rs exists with SPOOL_BLOBS constant
- [x] capture_clipboard.rs exists with split spans
- [x] Commit e1686c88 found in git log

---

_Phase: 20-clipboard-capture-flow-correlation_
_Completed: 2026-03-11_
