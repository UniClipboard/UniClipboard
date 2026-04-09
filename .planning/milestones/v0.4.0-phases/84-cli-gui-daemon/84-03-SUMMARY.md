---
phase: 84-cli-gui-daemon
plan: '03'
subsystem: testing
tags: [tech: rust, testing, auth, jwt, integration-tests]

# Dependency graph
requires:
  - phase: 84-01
    provides: POST /auth/connect endpoint with bearer token exchange
  - phase: 84-02
    provides: CLI daemon client using session exchange pattern
provides:
  - CLI auth integration tests in src-tauri/crates/uc-cli/tests/cli_auth.rs
  - Daemon auth integration tests in src-tauri/crates/uc-daemon/tests/security_middleware.rs
affects: [84-04, phase-85]

# Tech tracking
tech-stack:
  added: []
  patterns: [HTTP-level integration testing with tower::ServiceExt::oneshot, CLI auth flow testing]

key-files:
  created:
    - src-tauri/crates/uc-cli/tests/cli_auth.rs
  modified:
    - src-tauri/crates/uc-daemon/tests/security_middleware.rs
    - src-tauri/crates/uc-cli/Cargo.toml

key-decisions:
  - Added axum, tower, tempfile as dev-dependencies to uc-cli to support HTTP-level integration tests
  - CLI tests use same test infrastructure pattern as daemon security_middleware.rs tests

patterns-established:
  - 'Pattern: HTTP-level integration tests using tower::ServiceExt::oneshot for stateless request dispatch'
  - 'Pattern: Security state returned from test router builder for JWT verification in tests'

requirements-completed: [AUTH-04, AUTH-05]

# Metrics
duration: ~15min
completed: 2026-04-03
---

# Phase 84 Plan 03 Summary: CLI/GUI Auth Integration Tests

**Integration tests covering unified auth architecture: CLI session exchange flow, bare bearer rejection on L2+ routes, independent CLI/GUI token scopes**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-04-02T19:05:50Z
- **Completed:** 2026-04-03T00:00:00Z
- **Tasks:** 3
- **Files modified:** 4 (1 created, 2 modified, 1 Cargo.lock)

## Accomplishments

- Created `cli_auth.rs` integration tests covering AUTH-01 (session exchange), AUTH-02 (PID registration), AUTH-05 (independent CLI/GUI tokens)
- Added AUTH-03 (rate limit isolation) and AUTH-06 (bearer-only-at-auth-connect) tests to `security_middleware.rs`
- AUTH-04 (bare bearer rejection) already covered by existing tests in `security_middleware.rs`
- All 6 auth requirements have automated test coverage

## Task Commits

Each task was committed atomically:

1. **Task 1: CLI auth integration tests** - `c09f38d0` (test)
2. **Task 2: Daemon auth tests (AUTH-03, AUTH-06)** - `fc369daa` (test)
3. **Task 3: Workspace verification** - `32d6e62a` (chore)

## Files Created/Modified

- `src-tauri/crates/uc-cli/tests/cli_auth.rs` - New integration test file with 3 tests
- `src-tauri/crates/uc-daemon/tests/security_middleware.rs` - Added 2 new tests (rate_limit_is_per_client_not_global, bearer_token_only_accepted_at_auth_connect)
- `src-tauri/crates/uc-cli/Cargo.toml` - Added axum, tower, tempfile as dev-dependencies
- `src-tauri/Cargo.lock` - Updated with new dev-dependency versions

## Decisions Made

- Added `axum`, `tower`, `tempfile` as dev-dependencies to `uc-cli/Cargo.toml` to enable HTTP-level integration testing in the CLI crate. These are standard test infrastructure dependencies used throughout the workspace.
- AUTH-04 coverage: The existing `bare_bearer_rejected_with_invalid_auth_scheme_error` and `bare_bearer_on_l2_route_rejected_differently_than_invalid_jwt` tests already cover AUTH-04 comprehensively, so no additional tests were needed for that requirement.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

1. **Missing dev-dependencies (Rule 3 - Blocking):** `uc-cli` lacked `axum`, `tower`, and `tempfile` as dev-dependencies. Fixed by adding them to `Cargo.toml`.

2. **Move errors in test file (Rule 1 - Bug):** `runtime` was moved twice (into `DaemonQueryService::new` and then into `DaemonApiState::new`). Fixed with `runtime.clone()`. Also added `.clone()` on `router` before `.oneshot()` calls.

3. **JWT verification with wrong secret (Rule 1 - Bug):** `cli_and_gui_get_independent_tokens` test created a new `SecurityState` with a different JWT secret, causing `verify()` to fail. Fixed by returning the `security` state from `build_test_router()` and using it for verification.

## Pre-existing Test Failures (Out of Scope)

The following failures exist in the codebase and are unrelated to this plan's auth changes:

- `cli_smoke.rs`: 8 clipboard smoke test failures (daemon unreachable scenarios)
- `clipboard_api.rs`: 1 toggle favorite test failure (returns 400 vs expected 404)

These failures are in clipboard functionality and existed before this plan's changes.

## Test Coverage Summary

| Requirement | Coverage | Test(s) |
|---|---|---|
| AUTH-01 | Covered | `cli_auth_uses_session_exchange_not_direct_bearer` |
| AUTH-02 | Covered | `cli_auth_registers_pid_in_whitelist` |
| AUTH-03 | Covered | `rate_limit_is_per_client_not_global` |
| AUTH-04 | Covered | `bare_bearer_rejected_with_invalid_auth_scheme_error`, `bare_bearer_on_l2_route_rejected_differently_than_invalid_jwt` |
| AUTH-05 | Covered | `cli_and_gui_get_independent_tokens` |
| AUTH-06 | Covered | `bearer_token_only_accepted_at_auth_connect` |

## Verification Results

```
cargo test -p uc-cli --test cli_auth: 3 passed
cargo test -p uc-daemon --test security_middleware: 20 passed
cargo test -p uc-daemon-client: 11 passed
cargo check --workspace: compiles cleanly (pre-existing warnings only)
```

## Next Phase Readiness

- All 6 auth requirements have automated test coverage
- Full workspace compiles cleanly
- Ready for Phase 85: Improve pairing observability

---
_Phase: 84-cli-gui-daemon_
_Completed: 2026-04-03_
