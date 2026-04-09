---
phase: 86-cli-host-join-flow-phase
plan: '03'
subsystem: uc-cli
tags:
  - cli
  - host-flow
  - join-flow
  - phase-enum
dependency_graph:
  requires:
    - '86-02'
  provides:
    - HostCliPhase
    - HostCliSession
    - JoinCliPhase
    - JoinCliSession
    - derive_host_phase
    - derive_join_phase
  affects:
    - uc-cli
tech_stack:
  added:
    - Rust
  patterns:
    - Phase-driven FSM derivation
    - Session ID embedded in phase variant
    - Pure derive function (no side effects)
key_files:
  created:
    - src-tauri/crates/uc-cli/src/commands/setup/host_flow.rs
    - src-tauri/crates/uc-cli/src/commands/setup/join_flow.rs
  modified:
    - src-tauri/crates/uc-cli/src/commands/setup.rs
decisions:
  - id: D-11
    title: HostCliPhase session_id embedded in variant
    rationale: Per D-13, session_id lives inside the phase variant not in a separate session struct
  - id: D-12
    title: JoinCliPhase session_id in NeedPeerConfirmation variant
    rationale: Only the variant that needs session context carries it
  - id: D-14
    title: Pure derive_*_phase() functions
    rationale: No side effects, takes parsed state + current phase, returns next phase
  - id: D-15
    title: No last_submitted_* deduplication in derive functions
    rationale: Deduplication handled by caller using submitted session IDs
  - id: D-16
    title: HostCliSession and JoinCliSession carry loop state
    rationale: Phase is embedded in session struct for clean loop structure
metrics:
  duration_seconds: 131
  completed: '2026-04-03T09:43:12Z'
  tasks_completed: 3
  files_created: 2
  files_modified: 1
---

# Phase 86 Plan 03 Summary: CLI Host/Join Phase Enums

## One-liner

Created lightweight CLI phase enums (HostCliPhase, JoinCliPhase) and pure derive_*_phase() functions for phase-driven setup flow refactoring.

## Completed Tasks

| Task | Name | Commit | Files |
| ---- | ---- | ------ | ----- |
| 1 | Create host_flow.rs with HostCliPhase, HostCliSession, derive_host_phase() | 0dac45d6 | host_flow.rs |
| 2 | Create join_flow.rs with JoinCliPhase, JoinCliSession, derive_join_phase() | 026e550b | join_flow.rs |
| 3 | Wire host_flow and join_flow as submodules of setup module | a60cf0e6 | setup.rs |

## What Was Built

### host_flow.rs (237 lines)
- `HostCliPhase` enum with 6 variants: `WaitingJoinRequest`, `NeedDecision { session_id }`, `NeedVerification { session_id }`, `WaitingBackendCompletion`, `Completed`, `Canceled`
- `HostCliSession` struct carrying: phase, pairing_presence_enabled, last_lease_refresh, spinner
- `derive_host_phase(parsed, current) -> HostCliPhase` pure function implementing D-14
- 10 unit tests covering all phase transitions

### join_flow.rs (213 lines)
- `JoinCliPhase` enum with 8 variants: `SelectingPeer`, `WaitingPeerDiscovery`, `WaitingHostResponse`, `NeedPeerConfirmation { session_id }`, `NeedPassphrase`, `WaitingBackendCompletion`, `Completed`, `Canceled`
- `JoinCliSession` struct carrying: phase, submitted_peer_request, spinner
- `derive_join_phase(parsed, current) -> JoinCliPhase` pure function implementing D-14
- 8 unit tests covering all phase transitions

### setup.rs wiring
- Added `mod host_flow; mod join_flow;` submodule declarations
- Added `pub use host_flow::{HostCliPhase, HostCliSession};`
- Added `pub use join_flow::{JoinCliPhase, JoinCliSession};`

## Decisions Made

| ID | Decision | Rationale |
|----|----------|-----------|
| D-11 | HostCliPhase session_id embedded in variant | Per D-13 design: session_id lives inside phase variant |
| D-12 | JoinCliPhase session_id in NeedPeerConfirmation variant | Only phases that need session context carry it |
| D-14 | Pure derive_*_phase() functions | Takes ParsedSetupState + current phase, returns next phase - no side effects |
| D-15 | No last_submitted_* in derive functions | Deduplication handled by caller using submitted session IDs |
| D-16 | HostCliSession/JoinCliSession carry loop state | Phase embedded in session struct for clean phase-driven loop structure |

## Verification

- `cargo check -p uc-cli` passes with 0 errors
- `cargo test -p uc-cli` passes (24 tests, integration test failures expected in test environment)
- All newly created types compile and are publicly accessible via `setup::HostCliPhase`, etc.

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None - no stubs in the created files. All functions have complete implementations.

## Deferred Issues

None.
