# Phase 14 Research — Lifecycle DTO Frontend Integration

## Objective

Understand what is required to align the frontend lifecycle APIs with the backend `LifecycleStatusDto` command contract and restore the lifecycle status UI, satisfying CONTRACT-01 and CONTRACT-03.

## Inputs

- Project State: .planning/STATE.md
- Requirements: .planning/REQUIREMENTS.md (CONTRACT-01, CONTRACT-03)
- Phase description: Align frontend lifecycle APIs with backend `LifecycleStatusDto` and restore lifecycle status UI.

## Key Questions

1. How does the backend currently expose lifecycle status (`get_lifecycle_status` command, DTO shape, error contracts)?
2. What is the desired frontend contract for lifecycle status (TypeScript types, discriminated unions, error handling)?
3. Where does the current mismatch exist between frontend models and backend `LifecycleStatusDto`?
4. What UI flows depend on lifecycle status (e.g., encryption initialization, pairing, runtime lifecycle) and how are they broken today?
5. How should tests be structured to guarantee command contract alignment (both DTO shape and serialization details like camelCase)?

## Findings

### Backend LifecycleStatusDto Contract

- Phase 11 introduced `LifecycleStatusDto` as an explicit DTO wrapping a `LifecycleState` enum with camelCase serde conventions.
- Command errors use `CommandError` with `tag=code` and `content=message`, enabling discriminated union handling on the frontend.
- `get_lifecycle_status` likely returns a JSON payload similar to:
  - `{ "state": "Uninitialized" | "Initialized" | "Running" | "ShuttingDown" | ... }`
  - Errors use `{ "code": "SomeErrorCode", "message": "Human readable" }`.

### Frontend Contract Gaps

- Existing frontend types may still be using legacy domain models instead of the new DTO.
- There may be hard-coded string states or mismatched casing (e.g., snake_case vs camelCase).
- Error handling may not treat `CommandError` as a discriminated union, leading to fragile UI logic.

### UI Integration Points

- Lifecycle status UI must surface at least:
  - Whether encryption has been initialized.
  - Whether pairing/setup is complete and runtime tasks are active.
  - Any shutdown or error state that requires user action.
- These likely appear in:
  - A status indicator component on the main dashboard.
  - Settings/onboarding flows that gate actions based on lifecycle state.

### Requirements Mapping

- **CONTRACT-01**: Command responses must use explicit DTOs instead of domain models.
  - Phase 11 handled backend DTO creation; Phase 14 ensures the frontend only consumes DTOs and not internal domain models.
  - All TypeScript types and IPC bridges must reflect DTOs (no direct domain model usage).
- **CONTRACT-03**: Serialization must remain frontend-compatible with tests.
  - Ensure camelCase JSON for fields and consistent discriminated error encoding.
  - Add tests that assert the frontend contracts against real command responses.

### Risks and Constraints

- Changing TypeScript types and IPC contracts may break existing callers; migration should be done in a staged way, but Phase 14 scope is limited to lifecycle status.
- UI must not leak internal implementation details; only DTO and high-level states should be visible.
- Tests must be robust enough to catch regressions when backend DTOs evolve.

## Validation Architecture

- Use existing test infrastructure from earlier phases (Rust integration tests + potential frontend tests) to assert contract.
- Add targeted tests that:
  - Call `get_lifecycle_status` and assert DTO shape (including camelCase field names).
  - Decode responses in TypeScript and assert they conform to the new contract.
  - Verify error handling for `CommandError` when lifecycle status cannot be retrieved.

## Recommended Implementation Strategy

1. Inventory current frontend lifecycle status usage and types.
2. Define/confirm TypeScript DTO for `LifecycleStatusDto` and `CommandError`.
3. Align IPC wiring so that frontend sees DTOs exactly as emitted by backend.
4. Refactor UI components to use the new DTO and restore lifecycle status displays.
5. Add integration tests to lock down command contract (DTO shape + error structure).
