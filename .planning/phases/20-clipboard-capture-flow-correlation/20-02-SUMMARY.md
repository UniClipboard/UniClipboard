---
phase: 20-clipboard-capture-flow-correlation
plan: 02
subsystem: observability
tags: [tracing, flow-correlation, spans, instrumentation, uc-tauri, uc-app]

requires:
  - phase: 20-clipboard-capture-flow-correlation
    plan: 01
    provides: FlowId newtype and stage constants in uc-observability
provides:
  - flow_id root span on clipboard change detection with stage=detect
  - Stage sub-spans (normalize, persist_event, cache_representations, select_policy, persist_entry) in CaptureClipboardUseCase
  - flow_id propagation into spawned outbound_sync task
affects: [21-sync-flow-observability, 22-seq-integration]

tech-stack:
  added: []
  patterns:
    [
      manual info_span with flow_id + stage fields,
      sync span via .entered(),
      async span via .instrument(),
    ]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs

key-decisions:
  - 'Replaced #[tracing::instrument] with manual span to support runtime-computed flow_id field'
  - 'outbound_sync span carries flow_id but no stage field (Phase 21 adds publish stage)'

patterns-established:
  - 'Root flow span pattern: generate FlowId, create info_span with %flow_id and stage field, wrap body in async move + .instrument()'
  - 'Stage span pattern: wrap pipeline step in async block + .instrument(info_span!("name", stage = stages::CONST))'
  - 'Sync stage span pattern: use info_span!().entered() guard for synchronous code blocks'

requirements-completed: [FLOW-01, FLOW-02, FLOW-03, FLOW-04]

duration: 3min
completed: 2026-03-11
---

# Phase 20 Plan 02: Capture Pipeline Instrumentation Summary

**flow_id correlation and six stage spans across clipboard capture pipeline from detection through persistence**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-10T16:53:30Z
- **Completed:** 2026-03-10T16:56:30Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Root span in on_clipboard_changed generates FlowId and tags stage=detect, enabling end-to-end flow correlation
- Five stage sub-spans in CaptureClipboardUseCase (normalize, persist_event, cache_representations, select_policy, persist_entry) label each pipeline step
- Spawned outbound_sync task carries flow_id for cross-task correlation
- All 227 existing tests pass unchanged across uc-observability and uc-app

## Task Commits

Each task was committed atomically:

1. **Task 1: Add flow_id root span to AppRuntime::on_clipboard_changed** - `e0bb7a47` (feat)
2. **Task 2: Add stage spans to CaptureClipboardUseCase::execute_with_origin** - `95e32739` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Root capture span with flow_id + detect stage, outbound_sync span with flow_id propagation
- `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` - Five stage spans wrapping normalize, persist_event, cache_representations, select_policy, persist_entry

## Decisions Made

- Replaced `#[tracing::instrument]` attribute with manual span creation to support runtime-computed flow_id field values
- outbound_sync span intentionally carries flow_id but no stage field -- Phase 21 will add the publish stage

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Full capture pipeline now instrumented with flow_id and stage spans
- Phase 21 can add sync/publish stage spans building on the same FlowId pattern
- Phase 22 Seq integration will be able to filter/correlate by flow_id field

---

_Phase: 20-clipboard-capture-flow-correlation_
_Completed: 2026-03-11_
