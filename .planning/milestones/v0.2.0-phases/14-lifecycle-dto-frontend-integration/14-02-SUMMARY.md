---
phase: 14-lifecycle-dto-frontend-integration
plan: 02
subsystem: ui+ipc-contract
tags: [react, typescript, tauri, dto, contract-tests]

# Dependency graph
requires:
  - phase: 14-lifecycle-dto-frontend-integration
    provides: Frontend DTO alignment and lifecycle commands
provides:
  - Restored lifecycle status banner driven by LifecycleStatusDto
  - Backend contract tests for LifecycleStatusDto and CommandError
  - Frontend DTO contract tests for lifecycle API wrapper
affects: [15-clipboard-management-command-wiring]

# Tech tracking
tech-stack:
  added: []
  patterns: ['Frontend tests validate IPC DTO contracts for lifecycle status and CommandError']

key-files:
  created:
    - src-tauri/crates/uc-tauri/tests/lifecycle_command_contract_test.rs
    - src/api/__tests__/lifecycle.test.ts
  modified:
    - src/pages/DashboardPage.tsx

key-decisions:
  - 'Dashboard shows a generic initializing banner for Idle/Pending lifecycle states and a retryable error banner for WatcherFailed/NetworkFailed.'
  - 'LifecycleStatusDto and CommandError JSON contracts are guarded by dedicated uc-tauri tests in addition to existing model/enum tests.'
  - 'Frontend lifecycle API is covered by Vitest contract tests that assume invokeWithTrace returns typed DTOs and discriminated CommandError objects.'

requirements-completed: [CONTRACT-01, CONTRACT-03]

# Metrics
duration: TBD
completed: TBD
---

# Phase 14 Plan 02: Restore lifecycle status UI and add contract tests Summary

**Lifecycle status UI is restored using LifecycleStatusDto, and backend/frontend tests now lock down the JSON and DTO contracts for get_lifecycle_status and CommandError.**

## Accomplishments

- Updated `src/pages/DashboardPage.tsx` to show a lifecycle status banner driven by `LifecycleStatusDto.state`, covering both boot-in-progress (Idle/Pending) and failure (WatcherFailed/NetworkFailed) states.
- Added `src-tauri/crates/uc-tauri/tests/lifecycle_command_contract_test.rs` to assert the JSON shape of `LifecycleStatusDto` and `CommandError` for the `get_lifecycle_status` command family.
- Added `src/api/__tests__/lifecycle.test.ts` to validate the frontend lifecycle API wrapper and DTO types, ensuring `getLifecycleStatus` and `retryLifecycle` are wired to the correct commands and that `CommandError` matches the discriminated union contract.

## Deviations from Plan

### Auto-fixed Issues

None. All changes stayed within the planned scope of lifecycle UI and DTO contract testing; existing broader TypeScript and Rust warnings/errors were left untouched.

## Issues Encountered

- Full workspace builds and tests still surface pre-existing TypeScript/Vitest configuration issues unrelated to lifecycle DTO wiring; these were not modified as part of this plan.

## Next Phase Readiness

- Lifecycle UI on the dashboard now visibly reflects backend lifecycle state, using `LifecycleStatusDto` as the single source of truth.
- Backend and frontend tests provide a safety net for future changes to lifecycle status and `CommandError` contracts, supporting upcoming clipboard command wiring work in Phase 15.
