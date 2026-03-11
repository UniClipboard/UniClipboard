---
phase: 21-sync-flow-correlation
plan: 01
subsystem: observability
tags: [tracing, sync, correlation, serde, backward-compat]

# Dependency graph
requires:
  - phase: 20-clipboard-capture-flow-correlation
    provides: stage constant pattern in uc-observability, flow_id model
provides:
  - Four sync stage constants (OUTBOUND_PREPARE, OUTBOUND_SEND, INBOUND_DECODE, INBOUND_APPLY)
  - ClipboardMessage.origin_flow_id field for cross-device flow correlation
affects: [21-02-PLAN (instrumentation uses these constants and field)]

# Tech tracking
tech-stack:
  added: []
  patterns: [serde(default, skip_serializing_if) for backward-compatible Option fields]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-observability/src/stages.rs
    - src-tauri/crates/uc-core/src/network/protocol/clipboard.rs
    - src-tauri/crates/uc-core/src/network/protocol/protocol_message.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs

key-decisions:
  - 'origin_flow_id uses serde(default) + skip_serializing_if for zero-cost backward compatibility with older peers'

patterns-established:
  - 'Optional wire fields: Use #[serde(default, skip_serializing_if = "Option::is_none")] for backward-compatible protocol evolution'

requirements-completed: [FLOW-05]

# Metrics
duration: 9min
completed: 2026-03-11
---

# Phase 21 Plan 01: Sync Flow Correlation Contracts Summary

**Four sync stage constants and backward-compatible origin_flow_id field on ClipboardMessage for cross-device flow correlation**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-11T03:22:51Z
- **Completed:** 2026-03-11T03:32:12Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Added OUTBOUND_PREPARE, OUTBOUND_SEND, INBOUND_DECODE, INBOUND_APPLY stage constants to uc-observability
- Added origin_flow_id: Option<String> to ClipboardMessage with serde(default) backward compatibility
- Fixed all 10 ClipboardMessage construction sites (1 production + 9 test) across 5 files
- Added 2 new serde tests: backward compat (missing field = None) and roundtrip (present field survives)
- Full workspace test suite green: 886 passed, 0 failed

## Task Commits

Each task was committed atomically:

1. **Task 1: Add sync stage constants to uc-observability** - `3bd9da68` (feat)
2. **Task 2: Add origin_flow_id to ClipboardMessage and fix all construction sites** - `bf6b3c5e` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-observability/src/stages.rs` - Four new sync stage constants with updated tests
- `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` - origin_flow_id field + 2 new serde tests
- `src-tauri/crates/uc-core/src/network/protocol/protocol_message.rs` - Fixed 2 test construction sites
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - Fixed 1 production construction site
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` - Fixed 3 test construction sites
- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` - Fixed 2 test construction sites

## Decisions Made

- Used serde(default) + skip_serializing_if for origin_flow_id to maintain backward compatibility with older peers that don't send the field

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Stage constants ready for use in Plan 02 instrumentation spans
- origin_flow_id field ready to be populated by sync_outbound use case in Plan 02
- All tests green, no regressions

---

_Phase: 21-sync-flow-correlation_
_Completed: 2026-03-11_

## Self-Check: PASSED
