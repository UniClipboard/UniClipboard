---
phase: 75-daemon-security-middleware
plan: '03'
subsystem: auth
tags: [jwt, websocket, axum, session-token, pid-whitelist, rate-limiting, rust]

# Dependency graph
requires:
  - phase: 75-02
    provides: SecurityState merged into DaemonApiState, POST /auth/connect endpoint, JWT session token infrastructure

provides:
  - WebSocket upgrade handler uses session token JWT validation instead of bearer token
  - WS upgrade checks PID whitelist via state.security.is_pid_allowed()
  - WS upgrade applies rate limiting by PID from validated JWT claims
  - JWT session token claim verification integration tests in security_middleware.rs
  - websocket_api.rs tests updated to use session token authentication

affects:
  - pairing_ws (uses websocket connection patterns)
  - uc-tauri (daemon API client)
  - future phases using daemon WS API

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "WS upgrade security: validate JWT, check PID whitelist, apply rate limiting before upgrade"
    - "Session token verification in WS handlers via state.security.jwt_secret (merged DaemonApiState)"
    - "Test pattern: SecurityState::new_with_pid() + make_session_token_for_pid() for WS test setup"

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-daemon/src/api/ws.rs
    - src-tauri/crates/uc-daemon/tests/security_middleware.rs
    - src-tauri/crates/uc-daemon/tests/websocket_api.rs

key-decisions:
  - "WS upgrade validates Session JWT not Bearer token — consistent with L2 HTTP middleware pattern"
  - "Rate limit WS upgrades by PID from validated JWT claims (trustworthy, not caller-controlled)"
  - "Pass SessionTokenClaims to handle_connection() for audit logging and future per-connection auth"

patterns-established:
  - "WS auth pattern: strip_prefix('Session ') -> SessionTokenClaims::verify() -> is_pid_allowed() -> rate_limiter.check()"
  - "Test setup pattern: SecurityState::new_with_pid(pid) + make_session_token_for_pid(pid) for WS tests"

requirements-completed: []

# Metrics
duration: 10min
completed: 2026-03-29
---

# Phase 75 Plan 03: WebSocket Session Token Security Summary

**WebSocket upgrade handler now validates JWT session tokens, PID whitelist, and rate limits by PID — replacing the old bearer token check with the same auth pattern as L2 HTTP routes**

## Performance

- **Duration:** ~10 min
- **Started:** 2026-03-29T16:30:00Z
- **Completed:** 2026-03-29T16:40:00Z
- **Tasks:** 2 (+ 1 auto-fix deviation)
- **Files modified:** 3

## Accomplishments

- Replaced `state.is_authorized(&headers)` bearer token check in `websocket_upgrade` with full JWT session token validation chain
- WS upgrade now follows the same auth pattern as L2 HTTP middleware: verify JWT -> check PID whitelist -> rate limit by PID
- Added 2 new integration tests verifying JWT session token claim content after /auth/connect round-trip
- Fixed all 4 pre-existing websocket_api.rs test failures caused by the bearer-to-session-token migration

## Task Commits

Each task was committed atomically:

1. **Task 1: Update WebSocket upgrade to use session token validation** - `bcf437b2` (feat)
2. **Task 2: Add WebSocket integration tests to security_middleware.rs** - `1abdba73` (test)
3. **[Rule 1 - Bug] Fix websocket_api.rs tests broken by bearer-to-session migration** - `90094e86` (fix)

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/api/ws.rs` - WS upgrade handler now uses JWT session token validation (SessionTokenClaims::verify, is_pid_allowed, rate_limiter.check), helper functions ws_unauthorized/ws_forbidden/ws_rate_limited added
- `src-tauri/crates/uc-daemon/tests/security_middleware.rs` - Added session_token_contains_correct_pid and session_token_for_gui_client_type tests with JWT round-trip verification
- `src-tauri/crates/uc-daemon/tests/websocket_api.rs` - Updated spawn_server() to pre-register PID and generate session token; connect_with_token() now uses "Session" prefix

## Decisions Made

- Rate limit WS upgrades by PID from validated JWT claims, not by IP. The PID comes from a verified JWT so it is trustworthy (not caller-controlled), consistent with how authenticated HTTP routes are rate-limited.
- Pass `claims: SessionTokenClaims` into `handle_connection()` so the authenticated client identity is available for audit logging and future per-connection authorization.
- Use `state.security` field directly in the WS handler (merged DaemonApiState from 75-02) rather than extracting security state separately.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed websocket_api.rs tests broken by bearer-to-session token migration**
- **Found during:** Post-Task 1 test run
- **Issue:** `websocket_api.rs` tests used `format!("Bearer {}", token)` to connect to the WS endpoint. After Task 1 changed ws.rs to require `Session <JWT>`, these 4 tests all failed with HTTP 401.
- **Fix:** Updated `spawn_server()` to use `SecurityState::new_with_pid(pid)` and `make_session_token_for_pid(pid)` to generate a pre-registered JWT session token. Updated `connect_with_token()` to use `"Session <token>"` prefix. Renamed `upgrade_rejected_without_valid_bearer_token` to `upgrade_rejected_without_session_token`.
- **Files modified:** `src-tauri/crates/uc-daemon/tests/websocket_api.rs`
- **Verification:** All 5 websocket_api tests pass
- **Committed in:** `90094e86`

---

**Total deviations:** 1 auto-fixed (Rule 1 - Bug)
**Impact on plan:** Necessary fix — the existing tests were testing the old bearer auth behavior which was intentionally replaced by this plan. No scope creep.

## Issues Encountered

- `pairing_api.rs` tests (5 failures) were pre-existing failures unrelated to this plan — not investigated or fixed per scope boundary rules.

## Next Phase Readiness

- Phase 75 security middleware complete: JWT session token auth covers both HTTP (L2 middleware) and WebSocket upgrade paths
- All three plans (75-01, 75-02, 75-03) complete
- `cargo check -p uc-daemon` passes cleanly
- 19 security/websocket tests pass

---
*Phase: 75-daemon-security-middleware*
*Completed: 2026-03-29*
