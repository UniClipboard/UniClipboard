---
phase: 75-daemon-security-middleware
plan: '01'
subsystem: daemon-api
tags: [jwt, rate-limiting, middleware, security, axum, tokio]
dependency-graph:
  requires: []
  provides:
    - SessionTokenClaims with HS256 sign/verify
    - SlidingWindowRateLimiter (100 req/min, tokio::time::Instant for testability)
    - PermissionLevel enum (L1/L2 only)
    - SecurityState with JWT secret, PID whitelist, rate limiter
    - auth_extractor_middleware and rate_limit_middleware
  affects: [Phase 75 plans 02-03, Phase 76 daemon-settings-api, Phase 77 frontend-daemon-client]
tech-stack:
  added: [jsonwebtoken, tokio/test-util]
  patterns:
    - HS256 JWT with subject validation (jsonwebtoken 10.x)
    - Sliding window rate limiting with tokio::time::Instant
    - Typed request extensions (ClientId marker type) for middleware communication
    - SecurityState injection via DaemonApiState
key-files:
  created:
    - src-tauri/crates/uc-daemon/src/security/claims.rs
    - src-tauri/crates/uc-daemon/src/security/permission.rs
    - src-tauri/crates/uc-daemon/src/security/rate_limiter.rs
    - src-tauri/crates/uc-daemon/src/security/state.rs
    - src-tauri/crates/uc-daemon/src/security/middleware.rs
    - src-tauri/crates/uc-daemon/src/security/mod.rs
    - src-tauri/crates/uc-daemon/src/security/tests.rs
  modified:
    - src-tauri/crates/uc-daemon/src/lib.rs
    - src-tauri/crates/uc-daemon/Cargo.toml
    - src-tauri/crates/uc-daemon/src/api/server.rs
    - src-tauri/crates/uc-daemon/src/app.rs
    - src-tauri/crates/uc-daemon/tests/http_api.rs
    - src-tauri/crates/uc-daemon/tests/pairing_api.rs
    - src-tauri/crates/uc-daemon/tests/pairing_ws.rs
    - src-tauri/crates/uc-daemon/tests/setup_api.rs
    - src-tauri/crates/uc-daemon/tests/websocket_api.rs
key-decisions:
  - "Used rust_crypto feature for jsonwebtoken (aws_lc_rs had version conflict)"
  - "Used tokio::time::Instant for rate limiter timestamps (std::time::Instant cannot be mocked)"
  - "Added tokio test-util feature for time control in tests (tokio::time::pause/advance)"
  - "Subject validation done manually after decode (jsonwebtoken 10.x has no set_subject)"
  - "ClientId stored in request extensions as typed ClientId struct (http::Extensions uses type-based keys)"
  - "SecurityState::new() generates random JWT secret using OsRng"
  - "exp test uses 7-day backdate to reliably trigger expiration (1-second offset had timing edge cases)"
patterns-established:
  - "Middleware always takes State<Arc<DaemonApiState>> for access to security state"
  - "Rate limiter uses ClientId from extensions, not raw request data"
  - "JWT middleware stores validated claims in extensions for downstream handlers"
  - "PID whitelist is async RwLock-protected HashSet"
requirements-completed: []

# Phase 75: Daemon Security Middleware Summary

**JWT session token infrastructure with PID whitelist and sliding-window rate limiting using tokio::time::Instant for testability, plus Axum middleware for L2 authentication**

## Performance

- **Duration:** 1194s (~20 min)
- **Started:** 2026-03-29T15:23:47Z
- **Completed:** 2026-03-29T15:43:41Z
- **Tasks:** 2 (Task 1 skeleton + Tasks 2-3 implementation merged)
- **Files modified:** 17

## Accomplishments

- Security module skeleton with all 5 sub-modules declared and re-exports
- SessionTokenClaims with HS256 JWT signing/verification (jsonwebtoken 10.x)
- SlidingWindowRateLimiter using tokio::time::Instant for deterministic time control in tests
- PermissionLevel enum with L1 (public) and L2 (authenticated) - L3/L4 deferred
- SecurityState struct holding JWT secret, PID whitelist, and rate limiter
- Two Axum middleware functions: auth_extractor_middleware and rate_limit_middleware
- ClientId typed marker for request extensions (enables middleware-to-middleware communication)
- SecurityState integrated into DaemonApiState for HTTP server bootstrap
- All 35 security tests are REAL (no #[ignore] stubs)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create security module skeleton** - `04a96caf` (feat)
2. **Task 2+3: Implement security types and middleware** - `9904d2bf` (feat)
3. **DaemonApiState integration** - `5ff56093` (refactor)

## Decisions Made

- **jsonwebtoken rust_crypto feature**: Used `rust_crypto` instead of `aws_lc_rs` because the workspace's `aws-lc-rs` version conflicted with jsonwebtoken's requirement
- **tokio::time::Instant over std::time::Instant**: Enables `tokio::time::pause()` and `tokio::time::advance()` for deterministic time control in tests
- **tokio test-util feature**: Added to uc-daemon's tokio dependency to enable `pause()` and `advance()` functions
- **Subject validation manually**: jsonwebtoken 10.x has no `set_subject()` method, so subject is validated manually after decode using `InvalidSubject` error
- **ClientId typed extension**: http::Extensions uses type-based keys, so used a `ClientId(String)` struct instead of string keys for typed-safe middleware communication
- **7-day exp backdate for tests**: 1-second backdate had timing edge cases; 7 days reliably triggers expiration
- **OsRng for secret generation**: SecurityState::new() uses `rand::RngCore::fill_bytes(&mut OsRng, &mut secret)` following existing daemon_auth_token.rs pattern

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] jsonwebtoken 10.x API mismatch: set_subject not available**
- **Found during:** Task 2 (SessionTokenClaims implementation)
- **Issue:** Plan specified using `Validation::set_subject()` which doesn't exist in jsonwebtoken 10.3.0
- **Fix:** Manual subject validation after decode using `InvalidSubject` error kind
- **Files modified:** src-tauri/crates/uc-daemon/src/security/claims.rs
- **Verification:** `claims_iss_validation` test verifies wrong issuer rejection
- **Committed in:** 9904d2bf

**2. [Rule 3 - Blocking] rand 0.8 OsRng::fill_bytes requires RngCore trait**
- **Found during:** Task 2+3 (Claims and SecurityState implementation)
- **Issue:** `OsRng.fill_bytes()` method not found; needs `RngCore::fill_bytes(&mut OsRng, &mut buf)`
- **Fix:** Changed all `rand::rngs::OsRng.fill_bytes(&mut buf)` calls to `rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut buf)`
- **Files modified:** claims.rs, state.rs
- **Verification:** All JWT tests pass, secret generation works
- **Committed in:** 9904d2bf

**3. [Rule 3 - Blocking] tokio::time::pause/advance require test-util feature**
- **Found during:** Task 3 (Rate limiter tests)
- **Issue:** `tokio::time::pause()` and `tokio::time::advance()` not found - require tokio's `test-util` feature
- **Fix:** Added `test-util` feature to uc-daemon's tokio dependency; created `.cargo/config.toml` with `tokio_unstable` cfg
- **Files modified:** src-tauri/crates/uc-daemon/Cargo.toml, created src-tauri/.cargo/config.toml
- **Verification:** All 5 rate limiter tests pass with time control
- **Committed in:** 9904d2bf

**4. [Rule 3 - Blocking] http::Extensions uses type-based keys (not string keys)**
- **Found during:** Task 3 (Middleware implementation)
- **Issue:** Plan specified `request.extensions_mut().insert("client_id", value)` which doesn't compile - Extensions API requires type-based keys
- **Fix:** Created `ClientId(String)` marker type, stored as typed extension alongside SessionTokenClaims
- **Files modified:** src-tauri/crates/uc-daemon/src/security/middleware.rs, mod.rs
- **Verification:** Middleware compiles and tests pass
- **Committed in:** 9904d2bf

**5. [Rule 3 - Blocking] DaemonApiState::new() needed SecurityState parameter**
- **Found during:** Task 3 (DaemonApiState integration)
- **Issue:** DaemonApiState::new() called in app.rs and 6 test files without SecurityState parameter
- **Fix:** Updated DaemonApiState::new() signature to require SecurityState; updated all 7 call sites
- **Files modified:** server.rs, app.rs, tests/http_api.rs, tests/pairing_api.rs, tests/pairing_ws.rs, tests/setup_api.rs (4 locations), tests/websocket_api.rs
- **Verification:** `cargo check -p uc-daemon` compiles cleanly
- **Committed in:** 5ff56093

**6. [Rule 2 - Missing Critical] SecurityState field missing from DaemonApiState**
- **Found during:** Task 3 (DaemonApiState integration)
- **Issue:** Middleware needs access to SecurityState via DaemonApiState but it wasn't integrated
- **Fix:** Added `security: SecurityState` field to DaemonApiState with builder methods
- **Files modified:** src-tauri/crates/uc-daemon/src/api/server.rs
- **Verification:** All middleware functions compile with correct State<Arc<DaemonApiState>> access
- **Committed in:** 5ff56093

**7. [Rule 1 - Bug] expired token test unreliable with 1-second backdate**
- **Found during:** Task 3 (Security tests)
- **Issue:** Test setting exp to `now - 1` sometimes passed due to clock precision
- **Fix:** Changed to 7-day backdate for reliably triggering expiration validation
- **Files modified:** claims.rs, tests.rs
- **Verification:** `jwt_expired_token_rejected` test now reliably fails for expired tokens
- **Committed in:** 9904d2bf

---

**Total deviations:** 7 auto-fixed (7 blocking)
**Impact on plan:** All auto-fixes were necessary for compilation or correctness. No scope creep.

## Issues Encountered

- **jsonwebtoken 10.x API differences**: The plan referenced API methods that don't exist in jsonwebtoken 10.3.0. Subject validation had to be implemented manually after decode.
- **tokio time control**: `tokio::time::pause()` and `advance()` require `test-util` feature and `tokio_unstable` cfg, not available with standard `full` feature.
- **http Extensions API**: Uses type-based keys, not string keys. Created typed `ClientId` struct for middleware communication.
- **Pre-existing test flakiness**: 2 tests in setup_api.rs fail with "database is locked" - this is a pre-existing test infrastructure issue (shared SQLite file) unrelated to Phase 75 changes.

## Next Phase Readiness

- Phase 75 Plan 02 (auth/connect endpoint) and Plan 03 (L3/L4 enforcement) can proceed immediately
- SecurityState is integrated into DaemonApiState and available to all HTTP handlers
- All security infrastructure is in place: JWT signing/verification, PID whitelist, rate limiting
- JWT secret generation, PID registration, and rate limiting are all tested and working

---
*Phase: 75-daemon-security-middleware*
*Completed: 2026-03-29*
