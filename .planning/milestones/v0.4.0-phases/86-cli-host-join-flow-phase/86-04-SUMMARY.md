---
phase: 86-cli-host-join-flow-phase
plan: '04'
subsystem: infra
tags: [rust, cli, phase-driven, session-management]

# Dependency graph
requires:
  - phase: '86-03'
    provides: HostCliPhase, JoinCliPhase structs with phase enums
provides:
  - Phase-driven loop structure for run_pair and run_connect
  - HostCliSession and JoinCliSession carry loop state through polling cycles
affects:
  - Phase 86 (CLI host/join flow refactor - final plan)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Phase-driven loop: poll -> parse -> derive phase -> match action -> sleep
    - HostCliSession/JoinCliSession carry phase, spinner, and presence state
    - on_phase_changed handles UI-only concerns (log + spinner clear)
    - Action failures return EXIT_ERROR immediately (D-18)

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-cli/src/commands/setup.rs

key-decisions:
  - "Explicit Result<(), i32> type annotation on action_result to avoid type inference conflicts"
  - "Cancel paths return EXIT_ERROR for loop exit (not EXIT_SUCCESS) to match error semantics"
  - "Used is_err() instead of if let Err for action_result error checking"

patterns-established:
  - "Phase-driven CLI loops: session struct holds phase + UI state; derive_*_phase computes next phase; match dispatches action; on_*_phase_changed updates UI"

requirements-completed:
  - REQ-86-04

# Metrics
duration: 18.4min
completed: 2026-04-03
---

# Phase 86 Plan 04: Phase-driven run_pair and run_connect Loops

**Phase-driven loops for run_pair and run_connect: poll -> parse -> derive phase -> match action -> sleep, with HostCliSession/JoinCliSession carrying loop state**

## Performance

- **Duration:** 18.4 min (1106s)
- **Started:** 2026-04-03T09:44:30Z
- **Completed:** 2026-04-03T10:02:56Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- Replaced old event-driven state machine in `run_pair` with phase-driven loop using `HostCliSession`, `derive_host_phase`, and `on_host_phase_changed`
- Replaced old event-driven state machine in `run_connect` with phase-driven loop using `JoinCliSession`, `derive_join_phase`, and `on_join_phase_changed`
- Phase transitions handled by `derive_*_phase` pure functions; `on_*_phase_changed` only updates UI (spinner, logging)
- Action failures return `EXIT_ERROR` immediately (D-18); success paths fall through to sleep and continue polling
- Added `derive_host_phase` and `derive_join_phase` to re-exports from setup module

## Task Commits

1. **Task 1: Rewrite run_pair as phase-driven loop** - `684d981e` (feat)
2. **Task 2: Rewrite run_connect as phase-driven loop** - `684d981e` (part of same commit)

**Plan metadata:** `684d981e` (feat: complete plan)

## Files Created/Modified

- `src-tauri/crates/uc-cli/src/commands/setup.rs` - Phase-driven host and join flows with session structs, phase derivation, and on_phase_changed handlers

## Decisions Made

- Explicit `Result<(), i32>` type annotation on `action_result` to avoid Rust type inference conflicts with nested `if` blocks containing `return` statements
- Cancel paths (`user declined`) return `exit_codes::EXIT_ERROR` for loop exit, not `exit_codes::EXIT_SUCCESS`, since they represent abnormal loop termination rather than successful completion
- Used `action_result.is_err()` instead of `if let Err(code)` pattern for error checking after the match expression

## Deviations from Plan

**Total deviations:** 3 auto-fixed (all Rule 3 - blocking issues)
**Impact on plan:** All auto-fixes were type-level compilation errors. No scope creep; plan goals fully achieved.

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Missing derive_host_phase and derive_join_phase re-exports**
- **Found during:** Task 1 (run_pair rewrite)
- **Issue:** `derive_host_phase` and `derive_join_phase` not found in scope - were not included in the pub use re-exports from host_flow/join_flow submodules
- **Fix:** Added `derive_host_phase` and `derive_join_phase` to the pub use statements
- **Files modified:** src-tauri/crates/uc-cli/src/commands/setup.rs
- **Verification:** `cargo check -p uc-cli` passes
- **Committed in:** 684d981e (Task 1 commit)

**2. [Rule 3 - Blocking] Mismatched types in match arms due to return-in-if pattern**
- **Found during:** Task 1 (run_pair rewrite)
- **Issue:** `if let Err(...) { return Err(exit_codes::EXIT_ERROR); }` pattern inside match arms caused Rust type inference to expect `i32` at the return position, producing "expected i32, found Result<_, i32>" errors. Also, `confirm_peer_trust()` returns `Result<SetupActionResponse, Error>` not `Result<(), Error>`
- **Fix:** Restructured match arms to use `Result<(), i32>` consistently; changed `if let Err(...) { return Err(...) }` to `if expr.is_err() { return i32 }` pattern; used explicit type annotation `let action_result: Result<(), i32> = match ...`
- **Files modified:** src-tauri/crates/uc-cli/src/commands/setup.rs
- **Verification:** `cargo check -p uc-cli` passes with no type errors
- **Committed in:** 684d981e (Task 1 commit)

**3. [Rule 3 - Blocking] Same type inference issue in run_connect**
- **Found during:** Task 2 (run_connect rewrite)
- **Issue:** Same `if let Err { return Err(...) }` type mismatch pattern in join flow match arms
- **Fix:** Applied same restructuring as run_pair: explicit type annotation, `is_err()` pattern, consistent `Result<(), i32>` returns
- **Files modified:** src-tauri/crates/uc-cli/src/commands/setup.rs
- **Verification:** `cargo check -p uc-cli` passes
- **Committed in:** 684d981e (Task 2 commit, same as Task 1)

## Issues Encountered

- **Test compilation pre-existing failure:** `tests/setup_cli.rs` includes `mod setup;` which brings in `src/commands/setup.rs` with its `mod host_flow; mod join_flow;` submodule declarations. The test file doesn't provide these submodule files. This was already broken before this plan's execution (verified by stashing changes and testing original code). Not related to this plan's changes.

## Next Phase Readiness

- Phase 86 CLI host/join flow refactor is complete (4/4 plans)
- `run_pair` and `run_connect` now use phase-driven loop architecture consistent with D-16 through D-19
- All 4 plans of Phase 86 executed; phase can be closed

---
*Phase: 86-cli-host-join-flow-phase*
*Completed: 2026-04-03*
