---
phase: 75-daemon-security-middleware
plan: "02"
subsystem: daemon-http-api
tags: [security, jwt, middleware, axum, integration-tests]
dependency_graph:
  requires: [75-01]
  provides: [auth-connect-endpoint, l1-l2-router-split, session-token-middleware]
  affects: [uc-daemon, tests/http_api, tests/pairing_api, tests/pairing_ws, tests/setup_api]
tech_stack:
  added: []
  patterns:
    - POST /auth/connect bearer-to-JWT exchange endpoint
    - L1/L2 router split (public vs protected)
    - Axum middleware chain with from_fn_with_state (auth_extractor -> rate_limit)
    - Option<ConnectInfo<SocketAddr>> for test-safe IP extraction
    - SecurityState::new_with_pid() + make_session_token_for_pid() for test fixtures
key_files:
  created:
    - src-tauri/crates/uc-daemon/tests/security_middleware.rs
  modified:
    - src-tauri/crates/uc-daemon/src/security/connect.rs
    - src-tauri/crates/uc-daemon/src/api/server.rs
    - src-tauri/crates/uc-daemon/src/security/state.rs
    - src-tauri/crates/uc-daemon/src/api/routes.rs
    - src-tauri/crates/uc-daemon/src/api/clipboard.rs
    - src-tauri/crates/uc-daemon/tests/http_api.rs
    - src-tauri/crates/uc-daemon/tests/pairing_api.rs
    - src-tauri/crates/uc-daemon/tests/pairing_ws.rs
    - src-tauri/crates/uc-daemon/tests/setup_api.rs
    - src-tauri/crates/uc-daemon/tests/websocket_api.rs
decisions:
  - "Option<ConnectInfo<SocketAddr>> used for IP rate limiting at /auth/connect so tests using tower::ServiceExt::oneshot compile and run without a real TCP connection; rate limiting only runs in production"
  - "SecurityState::new_with_pid() and make_session_token_for_pid() kept without #[cfg(test)] because integration tests in tests/ directory are separate crates and cannot see cfg(test) items"
  - "Pre-existing pairing_api test failures (5 tests) confirmed out-of-scope: map_daemon_pairing_error status code mapping bug existed before Phase 75"
metrics:
  duration: "~60min"
  completed: "2026-03-30"
  tasks: 3
  files_modified: 11
---

# Phase 75 Plan 02: Wire Security Middleware and /auth/connect Endpoint Summary

JWT session token exchange endpoint wired into daemon HTTP server with L1/L2 router split and auth_extractor + rate_limit middleware chain protecting all authenticated routes.

## What Was Built

### Task 1: POST /auth/connect Endpoint

`src/security/connect.rs` implements the bearer-token-to-JWT exchange flow:

1. IP-based rate limiting via `Option<ConnectInfo<SocketAddr>>` (None-safe for `oneshot` tests)
2. Bearer token validation against `state.auth_token`
3. PID registration into `state.security.allowed_pids`
4. HS256 JWT signing with `SessionTokenClaims` at LEVEL_L2
5. Returns `ConnectResponse { session_token, expires_in_secs, refresh_at_secs }`

Route registered via `auth_route::AUTH_CONNECT` constant from uc-core.

### Task 2: SecurityState Merged into DaemonApiState

`DaemonApiState` now carries `security: Arc<SecurityState>` as a field. Key changes:

- `DaemonApiState::new()` takes `Arc<SecurityState>` as fourth parameter
- `build_router()` return type fixed from `Router<DaemonApiState>` to `Router` (type is finalized by `.with_state()`)
- `run_http_server()` uses `into_make_service_with_connect_info::<SocketAddr>()` enabling `ConnectInfo` extraction in production

Two test fixture helpers added to `SecurityState` (not cfg(test) — needed by integration test binaries):
- `new_with_pid(pid)`: synchronous pre-registration using `try_write()` at construction time
- `make_session_token_for_pid(pid)`: generates a test JWT without HTTP round-trip

### Task 3: L1/L2 Router Split and Middleware Wiring

`routes.rs` split into two public functions:

- `router_l1(state)`: only `/health` — no middleware, no auth
- `router_l2_plus(state)`: all protected routes with middleware chain:
  ```
  .layer(rate_limit_middleware)      // outer -> runs SECOND
  .layer(auth_extractor_middleware)  // inner -> runs FIRST, sets ClientId extension
  ```

All per-handler `is_authorized(&headers)` checks removed from `routes.rs` and `clipboard.rs`. The `headers: HeaderMap` parameters were also removed from all affected handlers.

All integration test files updated from Bearer to Session tokens:
- `tests/http_api.rs`: added `get_session_token()` helper, pre-registers test PID
- `tests/pairing_api.rs`: `PairingApiFixture` stores JWT session token, `authed_request` uses `Session` prefix
- `tests/pairing_ws.rs`: `PairingWsHarness` tracks both bearer (for WS) and session_token (for HTTP L2)
- `tests/setup_api.rs`: all 4 fixture builders updated to JWT session token pattern
- `tests/websocket_api.rs`: SecurityState wrapped in Arc correctly

New integration test file `tests/security_middleware.rs` with 12 tests covering:
- `/auth/connect` returns 200 with valid bearer, 401 with wrong/missing bearer
- Protected routes return 401 without session token, 401 with raw bearer, 200 with valid session token
- Protected routes return 401 with tampered JWT, 403 with unregistered PID
- L1 `/health` accessible without any token, L2 `/status` and `/paired-devices` require token
- Session token response contains correct fields (3-part JWT, positive expiry values)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] build_router return type was Router<DaemonApiState> instead of Router**
- **Found during:** Task 2
- **Issue:** `build_router` returned `Router<DaemonApiState>` which blocked calling `into_make_service_with_connect_info` (not available on parameterized Router)
- **Fix:** Changed return type annotation to `Router` — the `.with_state(state)` call already finalizes the type
- **Files modified:** `src/api/server.rs`
- **Commit:** 6e00e715

**2. [Rule 2 - Missing functionality] Test fixtures needed SecurityState test helpers**
- **Found during:** Task 3 (compiling integration tests)
- **Issue:** `DaemonApiState::new` now requires `Arc<SecurityState>` but all 4 integration test files used the old constructor and Bearer tokens
- **Fix:** Added `new_with_pid()` and `make_session_token_for_pid()` to `SecurityState`; updated all test fixtures to JWT session token pattern
- **Files modified:** `src/security/state.rs`, all 5 test files
- **Commit:** 6e00e715

**3. [Rule 1 - Bug] ConnectInfo<SocketAddr> extractor fails in oneshot tests**
- **Found during:** Task 1 implementation analysis
- **Issue:** `ConnectInfo<SocketAddr>` as a required extractor panics when no real TCP connection exists (tower::ServiceExt::oneshot)
- **Fix:** Changed to `Option<ConnectInfo<SocketAddr>>` — rate limiting skipped when None (test context only)
- **Files modified:** `src/security/connect.rs`
- **Commit:** 67ddeec9

### Out-of-Scope Discoveries

5 pre-existing tests in `tests/pairing_api.rs` fail due to a status code mapping bug in `map_daemon_pairing_error` that existed before Phase 75. These are documented in `.planning/phases/75-daemon-security-middleware/75-VALIDATION.md` and are not caused by Plan 02 changes. Deferred to a future bug-fix commit.

## Known Stubs

None. All endpoints implemented with real logic. The `encryption_ready = false` in `/auth/connect` is intentional for Phase 75 scope (L3/L4 deferred to future phases) and is documented in the handler comment.

## Verification

All tests passing (excluding 5 pre-existing pairing_api failures):
- 112 unit tests (uc-daemon lib): PASS
- tests/http_api.rs (5 tests): PASS
- tests/security_middleware.rs (12 tests): PASS
- tests/pairing_ws.rs (7 tests): PASS
- tests/setup_api.rs (5 tests): PASS
- tests/websocket_api.rs (7 tests): PASS
- tests/api_auth.rs, tests/api_query.rs: PASS

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| 1 | 67ddeec9 | feat(75-02): implement POST /auth/connect endpoint for JWT session token exchange |
| 2 | 6e00e715 | feat(75-02): merge SecurityState into DaemonApiState and add cleanup loop |
| 3 | 37a0ec56 | feat(75-02): split L1/L2 routers, wire auth middleware, add security integration tests |

## Self-Check: PASSED
