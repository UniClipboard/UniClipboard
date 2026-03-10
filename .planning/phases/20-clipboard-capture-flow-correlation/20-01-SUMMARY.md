---
phase: 20-clipboard-capture-flow-correlation
plan: 01
subsystem: observability
tags: [uuid-v7, tracing, flow-correlation, uc-observability]

requires:
  - phase: 19-dual-output-logging-foundation
    provides: uc-observability crate with dual-output tracing infrastructure
provides:
  - FlowId newtype wrapping UUID v7 for capture pipeline correlation
  - Stage name constants (detect, normalize, persist_event, cache_representations, select_policy, persist_entry)
  - uc-app dependency on uc-observability for downstream instrumentation
affects: [20-02, 21-sync-flow-observability]

tech-stack:
  added: [uuid v7]
  patterns: [FlowId newtype for tracing span fields, stage constants as shared vocabulary]

key-files:
  created:
    - src-tauri/crates/uc-observability/src/flow.rs
    - src-tauri/crates/uc-observability/src/stages.rs
  modified:
    - src-tauri/crates/uc-observability/Cargo.toml
    - src-tauri/crates/uc-observability/src/lib.rs
    - src-tauri/crates/uc-app/Cargo.toml

key-decisions:
  - 'UUID v7 chosen for FlowId (time-ordered, monotonic) over v4 (random)'
  - 'Stage values are lowercase snake_case matching const names for queryability'

patterns-established:
  - 'FlowId newtype: wrap UUID behind domain type with Display for tracing %field usage'
  - 'Stage constants: shared &str constants in uc-observability for cross-crate span naming'

requirements-completed: [FLOW-01, FLOW-03]

duration: 2min
completed: 2026-03-11
---

# Phase 20 Plan 01: FlowId & Stage Constants Summary

**FlowId newtype (UUID v7) and six stage constants in uc-observability for clipboard capture flow correlation**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-10T16:50:08Z
- **Completed:** 2026-03-10T16:51:37Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- FlowId newtype with generate(), Display, Debug, Clone, Eq, Hash -- suitable for tracing span fields
- Six stage constants (detect, normalize, persist_event, cache_representations, select_policy, persist_entry)
- uc-app wired to depend on uc-observability for Plan 02 instrumentation
- 7 unit tests covering UUID v7 format, uniqueness, traits, and stage naming conventions

## Task Commits

Each task was committed atomically:

1. **Task 1: Create FlowId newtype and stage constants** - `608febea` (feat)
2. **Task 2: Add uc-observability dependency to uc-app** - `8c3cf1bf` (chore)

## Files Created/Modified

- `src-tauri/crates/uc-observability/src/flow.rs` - FlowId newtype wrapping UUID v7 with Display/Debug/Clone/Eq/Hash
- `src-tauri/crates/uc-observability/src/stages.rs` - Six stage name constants for capture pipeline
- `src-tauri/crates/uc-observability/src/lib.rs` - Public re-exports of flow and stages modules
- `src-tauri/crates/uc-observability/Cargo.toml` - Added uuid v7 dependency
- `src-tauri/crates/uc-app/Cargo.toml` - Added uc-observability dependency

## Decisions Made

- UUID v7 chosen for FlowId (time-ordered, monotonic) enabling temporal ordering of flows
- Stage values are lowercase snake_case matching const names for simple queryability in log tools

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- FlowId and stage constants ready for Plan 02 to instrument CaptureClipboardUseCase
- uc-app can now import uc_observability::FlowId and uc_observability::stages::\*

---

_Phase: 20-clipboard-capture-flow-correlation_
_Completed: 2026-03-11_
