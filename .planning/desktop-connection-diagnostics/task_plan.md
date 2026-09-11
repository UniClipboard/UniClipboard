# Desktop Connection Diagnostics Integration

## Goal

Complete the desktop S6b integration so the daemon alone owns Engine capture, GUI and CLI share daemon control, exports include truthful Engine coverage, and offline export remains available.

## Next Step

Desktop integration is implemented, validated, and committed in reviewable groups.

## Current Phase

Phase 5

## Phases

### Phase 1: Contract and ownership discovery
- [x] Read repository and Engine plan constraints
- [x] Map existing daemon diagnostics flow and Engine handle ownership
- [x] Confirm the exact Engine revision can be pinned from the remote
- **Status:** complete

### Phase 2: Daemon capability and transport
- [x] Add one daemon-owned controller for capture, source registration, host lifecycle, and export preparation
- [x] Add authenticated daemon DTOs/routes/client methods with restart-safe status semantics
- [x] Prove start, duplicate start, stop, query, restart, and unavailable outcomes
- **Status:** complete

### Phase 3: Export and clients
- [x] Include Engine managed logs and coverage manifest in online export
- [x] Keep existing offline export available and mark missing live confirmation
- [x] Expose matching CLI controls and results
- **Status:** complete

### Phase 4: GUI
- [x] Add a compact diagnostics setting using daemon state as authority
- [x] Handle reload, response loss, daemon restart, and unavailable state
- [x] Verify DOM rendering and interaction; actual browser visual check skipped because the in-app control interface was unavailable
- **Status:** complete

### Phase 5: Validation and delivery
- [x] Run focused Rust and frontend tests
- [x] Run final workspace checks and repository gates against the remote immutable pin
- [x] Pin an immutable Engine revision
- [x] Record real platform/device items as passed or skipped
- [x] Commit desktop work
- **Status:** complete

### Phase 6: Diagnostic content acceptance
- [x] Exercise a real local connection followed by an authentication failure
- [x] Verify client, server, connection-attempt, recovery, and final-outcome records
- [x] Verify the Desktop archive preserves Engine diagnostic records field-for-field
- [x] Keep candidate-update recovery, cross-device, and unavailable platforms explicitly pending
- **Status:** complete

## Key Questions

1. Where is the process-wide Engine observability handle retained after daemon startup?
2. Should online export extend the current Engine facade ZIP operation or use the desktop offline packager as the single archive owner?
3. Which immutable remote Engine revision will Desktop pin? Resolved: full commit `9708c2786a604e76b19ab8dc63b2e923c0e9e35f`.

## Decisions Made

| Decision | Rationale |
|---|---|
| Daemon is the only capture owner | GUI and CLI are lightweight clients and must observe one real process state. |
| Extend existing diagnostics endpoints | Avoid a second local protocol and preserve authentication. |
| Keep offline export independent of daemon | Existing logs must remain exportable when live flush/status cannot be obtained. |

## Errors Encountered

| Error | Attempt | Resolution |
|---|---:|---|
| Shell expanded unmatched `vitest.config.*` while listing files | 1 | Use quoted or explicit file searches instead of an unmatched shell glob. |
| Local Engine override rewrote `Cargo.lock` during the red test | 1 | Reversed only the generated lockfile diff; future local-Engine checks must snapshot and restore the lock or use an isolated manifest source. |
| `DaemonDiagnosticArchive` async trait was not object-safe because `async-trait` was dev-only | 1 | Promote the repository's existing `async-trait` dependency to the webserver production dependency set. |
| Offline archive test still expected the old root-level ZIP entry | 1 | Update the assertion to the unified `logs/` layout used by online and offline packages. |
| Frontend test worker failed under the system Node 22.4 module loader | 1 | Use the installed Node 22.22.1 runtime required by the current jsdom dependency; the same test then passed. |
| `npx --package=node@24` stalled while installing its architecture package | 1 | Terminate the task-owned installer processes and use the already installed compatible Node runtime. |
| Local UI started but the in-app browser control interface was unavailable | 1 | Keep the visual check marked skipped; rely on focused DOM interaction tests and do not substitute a separate browser surface. |
| `Arc::clone` inferred the diagnostics trait object too early at the builder call | 1 | Use ordinary `clone()` so Rust can coerce the concrete shared owner to the trait object at the argument boundary. |
| Full frontend suite has three failing main-window bootstrap tests | 1 | Reproduced the same three failures in a clean worktree at `2ad7f6976`; classify as baseline and keep focused diagnostics UI tests as change evidence. |
| Repository lint reports three errors in pre-existing planning/E2E files | 1 | Confirm none of the files changed in this task; keep as baseline failures and require changed-file lint to pass. |
| One Cargo command supplied multiple test-name filters | 1 | Cargo accepts one positional filter; rerun the already cached affected package suites without positional filters. |

## External Action Completed

- Engine branch `worktree/brave-harbor-fc60` was pushed to its existing upstream with user authorization.
- GitHub directly resolved full commit `9708c2786a604e76b19ab8dc63b2e923c0e9e35f`.
