---
phase: 43-unify-gui-and-cli-business-flows-to-eliminate-per-entrypoint-feature-adaptation
plan: '01'
subsystem: bootstrap
tags: [cli, bootstrap, runtime, unification]

# Dependency graph
requires:
  - phase: 41-daemon-and-cli-skeletons
    provides: CLI command skeletons with repeated bootstrap pattern
provides:
  - Unified build_cli_runtime() helper combining 4-step bootstrap
  - Simplified CLI commands using shared runtime builder
  - Single entry point for CLI clipboard operations
affects: [future CLI commands, daemon bootstrap]

# Tech tracking
tech-stack:
  added: []
  patterns: [shared runtime builder pattern, unified CLI/GUI entrypoint]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs
    - src-tauri/crates/uc-bootstrap/src/lib.rs
    - src-tauri/crates/uc-cli/src/commands/clipboard.rs
    - src-tauri/crates/uc-cli/src/commands/devices.rs
    - src-tauri/crates/uc-cli/src/commands/space_status.rs

key-decisions:
  - 'Return CoreRuntime (not CoreUseCases) from build_cli_runtime due to lifetime constraints'

patterns-established:
  - 'Unified CLI runtime: single build_cli_runtime() call + CoreUseCases::new(&runtime)'

requirements-completed: [PH43-01, PH43-02]

# Metrics
duration: 5min
completed: 2026-03-19
---

# Phase 43 Plan 01: CLI Bootstrap Unification Summary

**Unified CLI runtime helper combining build_cli_context_with_profile, get_storage_paths, and build_non_gui_runtime into single function**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-19T06:30:00Z
- **Completed:** 2026-03-19T06:35:00Z
- **Tasks:** 4
- **Files modified:** 5

## Accomplishments

- Created `build_cli_runtime()` helper in uc-bootstrap that combines the 4-step bootstrap sequence
- Updated all 5 CLI commands (3 clipboard + 1 devices + 1 space-status) to use the unified helper
- Reduced code duplication from ~15 lines repeated 5 times to 2 lines per command

## Task Commits

Each task was committed atomically:

1. **Task 1: Add build_cli_runtime() helper in uc-bootstrap** - `bc456276` (feat)
2. **Task 2: Update CLI clipboard commands to use build_cli_runtime** - `3d3a2846` (refactor)
3. **Task 3: Update CLI devices command to use build_cli_runtime** - `ce908406` (refactor)
4. **Task 4: Update CLI space_status command to use build_cli_runtime** - `e0ca6b32` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` - Added build_cli_runtime() function
- `src-tauri/crates/uc-bootstrap/src/lib.rs` - Exported build_cli_runtime
- `src-tauri/crates/uc-cli/src/commands/clipboard.rs` - Simplified 3 functions
- `src-tauri/crates/uc-cli/src/commands/devices.rs` - Simplified 1 function
- `src-tauri/crates/uc-cli/src/commands/space_status.rs` - Simplified 1 function

## Decisions Made

- Return CoreRuntime instead of CoreUseCases from build_cli_runtime due to CoreUseCases holding a lifetime reference to CoreRuntime (cannot return owning struct containing borrowed reference)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## Next Phase Readiness

- Plan 02 ready to continue CLI/GUI flow unification
- build_cli_runtime() available for any future CLI commands

---

_Phase: 43-unify-gui-and-cli-business-flows-to-eliminate-per-entrypoint-feature-adaptation_
_Completed: 2026-03-19_
