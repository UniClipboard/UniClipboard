---
phase: 22-seq-local-visualization
plan: 01
subsystem: observability
tags: [seq, clef, tracing, reqwest, tokio, batching]

# Dependency graph
requires:
  - phase: 19-observability-foundation
    provides: FlatJsonFormat, LogProfile, build_console_layer/build_json_layer pattern
  - phase: 20-capture-flow-correlation
    provides: FlowId, stages constants, span field patterns
provides:
  - CLEFFormat formatter producing Seq-compatible CLEF JSON
  - collect_span_fields shared helper (eliminates duplication between formatters)
  - SeqLayer tracing layer with mpsc channel transport
  - Background sender with dual-trigger batching (count=100, time=2s)
  - build_seq_layer builder function (env-driven, zero overhead when disabled)
  - SeqGuard for graceful shutdown with flush
affects: [22-02-bootstrap-integration]

# Tech tracking
tech-stack:
  added: [reqwest 0.12 (rustls-tls), tokio (sync/time/rt/macros)]
  patterns:
    [
      CLEF format for Seq ingestion,
      env-driven optional layer pattern,
      mpsc channel between tracing layer and HTTP sender,
    ]

key-files:
  created:
    - src-tauri/crates/uc-observability/src/clef_format.rs
    - src-tauri/crates/uc-observability/src/span_fields.rs
    - src-tauri/crates/uc-observability/src/seq/mod.rs
    - src-tauri/crates/uc-observability/src/seq/sender.rs
    - src-tauri/crates/uc-observability/src/seq/layer.rs
  modified:
    - src-tauri/crates/uc-observability/src/format.rs
    - src-tauri/crates/uc-observability/src/lib.rs
    - src-tauri/crates/uc-observability/Cargo.toml

key-decisions:
  - 'SeqGuard drop uses std::thread::spawn for block_on to avoid runtime-in-runtime panic'
  - 'SeqLayer uses Layer trait directly (not FormatEvent) to avoid fmt layer dependency'
  - 'CLEF has no conflict resolution (simpler than FlatJsonFormat) since it targets Seq only'

patterns-established:
  - 'collect_span_fields: shared span traversal helper for any FormatEvent impl'
  - 'env-driven optional layer: build_seq_layer returns Option, zero overhead when UC_SEQ_URL unset'
  - 'mpsc + background task pattern for non-blocking log delivery'

requirements-completed: [SEQ-01, SEQ-02, SEQ-03, SEQ-04, SEQ-05]

# Metrics
duration: 24min
completed: 2026-03-11
---

# Phase 22 Plan 01: Seq Core Implementation Summary

**CLEFFormat formatter, shared span-field helper, and Seq ingestion module with background batching sender in uc-observability**

## Performance

- **Duration:** 24 min
- **Started:** 2026-03-11T06:11:13Z
- **Completed:** 2026-03-11T06:35:13Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments

- CLEFFormat produces valid CLEF JSON with @t, @l (Seq level names), @m and flattened span fields
- Extracted collect_span_fields helper shared between FlatJsonFormat and CLEFFormat, eliminating duplication
- Seq ingestion module with SeqLayer, background sender (dual-trigger batching), and SeqGuard for graceful shutdown
- build_seq_layer returns None when UC_SEQ_URL unset (zero overhead), Some when configured
- 43 tests pass including 5 new CLEF tests, 5 new Seq tests, and all existing format/profile/init tests

## Task Commits

Each task was committed atomically:

1. **Task 1: CLEFFormat formatter and shared span-field extraction** - `5be24e89` (feat)
2. **Task 2: Seq sender, layer, and build_seq_layer builder** - `c3dd6432` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-observability/src/clef_format.rs` - CLEF JSON formatter with @t/@l/@m and Seq level mapping
- `src-tauri/crates/uc-observability/src/span_fields.rs` - Shared span-field collection helper
- `src-tauri/crates/uc-observability/src/seq/mod.rs` - build_seq_layer builder, public API
- `src-tauri/crates/uc-observability/src/seq/sender.rs` - Background HTTP sender with dual-trigger batching
- `src-tauri/crates/uc-observability/src/seq/layer.rs` - SeqLayer implementing Layer trait with CLEF formatting
- `src-tauri/crates/uc-observability/src/format.rs` - Refactored to use collect_span_fields
- `src-tauri/crates/uc-observability/src/lib.rs` - Added clef_format, span_fields, seq modules and re-exports
- `src-tauri/crates/uc-observability/Cargo.toml` - Added reqwest and tokio dependencies

## Decisions Made

- SeqGuard drop uses std::thread::spawn wrapping block_on to avoid "cannot block_on inside a runtime" panic when dropped within async context
- SeqLayer implements Layer trait directly rather than using FormatEvent through fmt::layer(), giving direct control over event formatting and mpsc channel send
- CLEF format has no conflict resolution for span/event field name collisions (simpler than FlatJsonFormat) since it targets Seq only

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed SeqGuard drop causing runtime-in-runtime panic**

- **Found during:** Task 2 (Seq sender tests)
- **Issue:** SeqGuard::drop called block_on directly on current runtime handle, causing panic in tokio async tests
- **Fix:** Wrapped block_on call in std::thread::spawn to execute on a separate OS thread
- **Files modified:** src-tauri/crates/uc-observability/src/seq/sender.rs
- **Verification:** test_seq_guard_signals_shutdown passes without panic
- **Committed in:** c3dd6432 (Task 2 commit)

**2. [Rule 3 - Blocking] Added tokio macros feature for select! and #[tokio::test]**

- **Found during:** Task 2 (compilation)
- **Issue:** tokio::select! and #[tokio::test] require the macros feature which was not in the initial dependency spec
- **Fix:** Added "macros" to tokio features list in Cargo.toml
- **Files modified:** src-tauri/crates/uc-observability/Cargo.toml
- **Verification:** All tests compile and pass
- **Committed in:** c3dd6432 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correct operation. No scope creep.

## Issues Encountered

None beyond the auto-fixed deviations above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- CLEFFormat, SeqLayer, and build_seq_layer are ready for bootstrap integration (Plan 02)
- Plan 02 can compose build_seq_layer alongside existing console and JSON layers
- UC_SEQ_URL and UC_SEQ_API_KEY env vars are the configuration interface

---

_Phase: 22-seq-local-visualization_
_Completed: 2026-03-11_
