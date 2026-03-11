---
phase: 21-sync-flow-correlation
plan: 02
subsystem: observability
tags: [tracing, flow-correlation, sync, spans, structured-logging]

# Dependency graph
requires:
  - phase: 21-sync-flow-correlation/01
    provides: stage constants (OUTBOUND_PREPARE, OUTBOUND_SEND, INBOUND_DECODE, INBOUND_APPLY), origin_flow_id field on ClipboardMessage
provides:
  - stage fields on outbound.prepare and outbound.send spans
  - flow_id generation at inbound receive loop
  - origin_flow_id propagation from capture through outbound to inbound
  - inbound.apply span wrapping representation selection through clipboard write
affects: [22-seq-sink]

# Tech tracking
tech-stack:
  added: []
  patterns:
    [flow_id inheritance via tracing span context, stage field instrumentation on sync spans]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/src/commands/clipboard.rs
    - src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs

key-decisions:
  - 'origin_flow_id converted to String before async move block to avoid double-move of FlowId in runtime.rs'

patterns-established:
  - 'Stage fields on sync spans: same pattern as local capture stages from Phase 20'
  - 'FlowId generated at receive loop (not in use case) so all child spans inherit it via tracing context'

requirements-completed: [FLOW-05]

# Metrics
duration: 8min
completed: 2026-03-11
---

# Phase 21 Plan 02: Sync Flow Instrumentation Summary

**Outbound and inbound sync spans instrumented with stage fields, FlowId generation at receive loop, and origin_flow_id cross-device propagation**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-11T03:35:16Z
- **Completed:** 2026-03-11T03:43:16Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Outbound sync spans carry stage=outbound_prepare and stage=outbound_send fields
- Inbound sync spans carry stage=inbound_decode and new inbound.apply span with stage=inbound_apply
- Each inbound message receives a unique FlowId at the receive loop level, inherited by all child spans
- origin_flow_id propagated from capture flow through ClipboardMessage to inbound receive span
- All tests passing (unit + e2e)

## Task Commits

Each task was committed atomically:

1. **Task 1: Instrument outbound sync with stage fields and origin_flow_id** - `b6fdf987` (feat)
2. **Task 2: Instrument inbound sync with flow_id, stage fields, and apply span** - `dc6b0b5b` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - Added origin_flow_id parameter, stage fields on outbound spans
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` - Added stage field on inbound.decode, new inbound.apply span
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Passes flow_id string into outbound sync execute
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - FlowId generation and origin_flow_id on receive loop span
- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` - Updated execute call for new parameter
- `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs` - Updated execute calls for new parameter

## Decisions Made

- Converted FlowId to String before the async move block in runtime.rs to avoid double-move (FlowId used both in info_span and in spawn_blocking closure)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated additional call site in commands/clipboard.rs**

- **Found during:** Task 1 (outbound sync instrumentation)
- **Issue:** execute() signature change required updating a call site in clipboard.rs commands not listed in plan
- **Fix:** Added `None` as third argument for origin_flow_id at the restore command call site
- **Files modified:** src-tauri/crates/uc-tauri/src/commands/clipboard.rs
- **Verification:** cargo check -p uc-tauri passed
- **Committed in:** b6fdf987 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary to maintain compilation. No scope creep.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Sync flow correlation complete -- both outbound and inbound paths have full flow_id + stage instrumentation
- Ready for Phase 22 (Seq sink) which will consume these structured span fields for log aggregation

---

_Phase: 21-sync-flow-correlation_
_Completed: 2026-03-11_
