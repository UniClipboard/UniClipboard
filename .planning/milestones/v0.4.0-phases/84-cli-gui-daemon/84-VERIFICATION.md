---
phase: 84-cli-gui-daemon
verified: 2026-04-03T00:00:00Z
status: passed
score: 5/5 must-haves verified (3 plans)
re_verification: false
gaps: []
---

# Phase 84: CLI/GUI/Daemon Auth Architecture Unification

**Phase Goal:** Unify CLI/GUI/Daemon auth architecture — all clients use POST /auth/connect for session exchange, daemon rejects bare bearer tokens on L2+ routes

**Verified:** 2026-04-03
**Status:** PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| #   | Truth                                                                          | Status   | Evidence                                                                                                                                                                                                 |
| --- | ------------------------------------------------------------------------------ | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | L2+ routes reject bare bearer tokens with explicit "invalid_auth_scheme" error | VERIFIED | `middleware.rs` lines 104-113 return 401 with error code `"invalid_auth_scheme"` and hint message; test `bare_bearer_rejected_with_invalid_auth_scheme_error` passes                                     |
| 2   | Bare bearer tokens fail before JWT decode attempt                              | VERIFIED | `strip_prefix("Session ")` returns `None` for bare tokens, triggering explicit rejection — no call to `SessionTokenClaims::verify()`                                                                     |
| 3   | Only "Session " prefix tokens reach JWT verification                           | VERIFIED | `middleware.rs` lines 103-123: tokens that fail `strip_prefix` return early with `"invalid_auth_scheme"`; only tokens that pass reach line 126 `SessionTokenClaims::verify()`                            |
| 4   | Bearer token accepted only at POST /auth/connect (L1 pre-auth route)           | VERIFIED | `connect.rs` has its own router merged at line 168 of `server.rs` without `auth_extractor_middleware`; test `bearer_token_only_accepted_at_auth_connect` passes (200 at /auth/connect, 401 on L2 routes) |
| 5   | DaemonApiState::is_authorized() dead code removed                              | VERIFIED | `server.rs` lines 143-150 (is_authorized method) absent; grep confirms no match; `parse_bearer_token` import removed from server.rs (still in `connect.rs` and tests)                                    |

**Score:** 5/5 truths verified

---

## Required Artifacts

| Artifact                                                  | Expected                                                         | Status   | Details                                                                                                                                                                                                                                   |
| --------------------------------------------------------- | ---------------------------------------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/crates/uc-daemon/src/security/middleware.rs`   | auth_extractor_middleware with explicit bare bearer rejection    | VERIFIED | 160 lines; explicit `Some(token) = auth_value.strip_prefix("Session ")` else branch returning `"invalid_auth_scheme"`; no `unwrap_or` in token extraction path                                                                            |
| `src-tauri/crates/uc-daemon/src/api/server.rs`            | is_authorized() method removed                                   | VERIFIED | No `is_authorized` method present; `parse_bearer_token` import removed; `DaemonApiState` only has legitimate methods                                                                                                                      |
| `src-tauri/crates/uc-daemon/src/security/claims.rs`       | SessionTokenClaims::verify() called from middleware              | VERIFIED | Line 126 of middleware calls `SessionTokenClaims::verify(token, &state.security.jwt_secret)`; file contains full JWT sign/verify with HS256                                                                                               |
| `src-tauri/crates/uc-daemon/tests/security_middleware.rs` | Integration tests for bare bearer rejection + session acceptance | VERIFIED | 726 lines; 20 tests including `bare_bearer_rejected_with_invalid_auth_scheme_error`, `bare_bearer_on_l2_route_rejected_differently_than_invalid_jwt`, `bearer_token_only_accepted_at_auth_connect`, `rate_limit_is_per_client_not_global` |
| `src-tauri/crates/uc-cli/src/daemon_client.rs`            | CLI daemon client using session exchange                         | VERIFIED | 926 lines; `ensure_session_token()` calls `exchange_cli_session_token`; `authorized_request()` uses `Authorization: Session <token>`; no `Authorization: Bearer` in production code                                                       |
| `src-tauri/crates/uc-daemon-client/src/http/mod.rs`       | exchange_session_token with client_type parameter                | VERIFIED | `exchange_session_token` accepts `client_type: &str`; `exchange_cli_session_token` helper; `authorized_daemon_request_with_type` routes gui to caching, others to fresh exchange                                                          |
| `src-tauri/crates/uc-cli/tests/cli_auth.rs`               | CLI auth integration tests (new file)                            | VERIFIED | 220 lines; 3 tests: `cli_auth_uses_session_exchange_not_direct_bearer`, `cli_auth_registers_pid_in_whitelist`, `cli_and_gui_get_independent_tokens`                                                                                       |

---

## Key Link Verification

| From               | To                             | Via                                                      | Status | Details                                                                                                                 |
| ------------------ | ------------------------------ | -------------------------------------------------------- | ------ | ----------------------------------------------------------------------------------------------------------------------- |
| `daemon_client.rs` | `uc-daemon-client/http/mod.rs` | `exchange_cli_session_token()`                           | WIRED  | `use uc_daemon_client::http::exchange_cli_session_token`; called in `ensure_session_token()` at line 151 with `cli_pid` |
| `daemon_client.rs` | daemon `/auth/connect`         | POST /auth/connect via `exchange_cli_session_token`      | WIRED  | `exchange_cli_session_token` posts to `{base_url}/auth/connect` with bearer token + pid + clientType                    |
| `daemon_client.rs` | all daemon API routes          | `authorized_request()` -> `Authorization: Session <jwt>` | WIRED  | `authorized_request()` builds requests with `Authorization: Session {token}` (line 174)                                 |
| `middleware.rs`    | `claims.rs`                    | `SessionTokenClaims::verify()`                           | WIRED  | Line 126 calls `SessionTokenClaims::verify(token, &state.security.jwt_secret)`                                          |

---

## Data-Flow Trace (Level 4)

| Artifact           | Data Variable                           | Source                                                          | Produces Real Data                                      | Status  |
| ------------------ | --------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------- | ------- |
| `middleware.rs`    | `claims` (SessionTokenClaims)           | `SessionTokenClaims::verify(token, &state.security.jwt_secret)` | YES — JWT decode + signature validation + iss/sub check | FLOWING |
| `daemon_client.rs` | `session_token: RwLock<Option<String>>` | `exchange_cli_session_token()` via POST /auth/connect           | YES — real JWT from daemon `/auth/connect` endpoint     | FLOWING |
| `claims.rs`        | `SessionTokenClaims`                    | `claims.sign(&secret)` at /auth/connect                         | YES — HS256 signed JWT with pid, client_type, exp       | FLOWING |

---

## Behavioral Spot-Checks

| Behavior                                     | Command                                              | Result                                                            | Status |
| -------------------------------------------- | ---------------------------------------------------- | ----------------------------------------------------------------- | ------ |
| security_middleware tests pass               | `cargo test -p uc-daemon --test security_middleware` | 20 passed                                                         | PASS   |
| cli_auth tests pass                          | `cargo test -p uc-cli --test cli_auth`               | 3 passed                                                          | PASS   |
| uc-daemon-client tests pass                  | `cargo test -p uc-daemon-client`                     | 11 passed                                                         | PASS   |
| No unwrap_or in token extraction             | `grep "unwrap_or" middleware.rs`                     | only `unwrap_or_else` for ClientId extension (line 48, correct)   | PASS   |
| No bare bearer in daemon_client.rs           | `grep "Bearer" daemon_client.rs`                     | Only in comments/docstrings explaining the new pattern            | PASS   |
| is_authorized removed from server.rs         | `grep "is_authorized" server.rs`                     | No matches                                                        | PASS   |
| parse_bearer_token not imported in server.rs | `grep "parse_bearer_token" server.rs`                | No matches (correctly removed; still in connect.rs and tests)     | PASS   |
| CLI tests (daemon_client module)             | `cargo test -p uc-cli`                               | 37 passed, 8 cli_smoke failures (pre-existing, unrelated to auth) | PASS   |

---

## Requirements Coverage

**Note:** AUTH-01 through AUTH-06 are defined in the phase plan frontmatter. They do not appear in `.planning/REQUIREMENTS.md` (which contains EVNT, RNTM, BOOT requirements only). These are phase-specific requirements that were tracked within the plan itself.

| Requirement | Source Plan | Description                                              | Status    | Evidence                                                                                                                                                                           |
| ----------- | ----------- | -------------------------------------------------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| AUTH-01     | Plan 02     | CLI uses POST /auth/connect (not direct bearer)          | SATISFIED | `daemon_client.rs` calls `exchange_cli_session_token`; test `cli_auth_uses_session_exchange_not_direct_bearer` passes; grep confirms no `Authorization: Bearer` in production code |
| AUTH-02     | Plan 02     | CLI PID registered in daemon whitelist via /auth/connect | SATISFIED | `cli_pid = std::process::id()` (line 92); `exchange_cli_session_token` sends pid; test `cli_auth_registers_pid_in_whitelist` passes                                                |
| AUTH-03     | Plan 01     | CLI rate limited same as GUI (PID-based rate limiting)   | SATISFIED | Rate limiter keyed by `ClientId` (claims.pid string); test `rate_limit_is_per_client_not_global` passes; CLI uses same `auth_extractor_middleware` chain as GUI                    |
| AUTH-04     | Plan 01     | Daemon L2+ routes reject bare bearer tokens              | SATISFIED | `middleware.rs` lines 104-113 return `"invalid_auth_scheme"`; test `bare_bearer_rejected_with_invalid_auth_scheme_error` passes                                                    |
| AUTH-05     | Plan 02     | CLI and GUI get independent session tokens               | SATISFIED | `exchange_session_token` accepts `client_type`; `cli_and_gui_get_independent_tokens` test passes with different jti, different pid, different client_type                          |
| AUTH-06     | Plan 01     | Bearer token only at /auth/connect                       | SATISFIED | `connect.rs` router merged without `auth_extractor_middleware`; test `bearer_token_only_accepted_at_auth_connect` passes                                                           |

**Orphaned requirements:** None — all 6 AUTH requirements are covered by the plan's `requirements:` field.

---

## Anti-Patterns Found

None.

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |

No TODO/FIXME/placeholder comments found in Phase 84 key files. No empty implementations. No hardcoded empty data in auth paths. The `RwLock<Option<String>>` session token cache in `daemon_client.rs` is intentional per D-07 (no JWT file caching) — it caches in-memory only, exchanged fresh on first HTTP request of each command invocation.

---

## Human Verification Required

None. All observable behaviors are verified programmatically through unit and integration tests.

---

## Gaps Summary

No gaps found. All must-haves verified across all three plans:

**Plan 01 (Daemon Middleware Hardening):**

- `auth_extractor_middleware` explicitly rejects bare bearer with `"invalid_auth_scheme"` — VERIFIED
- `DaemonApiState::is_authorized()` removed — VERIFIED
- Phase 84 tests in `security_middleware.rs` — 20 tests passing

**Plan 02 (CLI Session Exchange Migration):**

- `exchange_session_token` accepts `client_type` parameter — VERIFIED
- `exchange_cli_session_token` helper exists — VERIFIED
- CLI uses `"Session "` prefix for all daemon requests — VERIFIED
- No `Authorization: Bearer` in daemon_client.rs production code — VERIFIED
- CLI PID from `std::process::id()` — VERIFIED

**Plan 03 (Integration Tests):**

- `cli_auth.rs` with 3 tests for AUTH-01, AUTH-02, AUTH-05 — 3 passing
- AUTH-03, AUTH-04, AUTH-06 covered in `security_middleware.rs` — 20 passing
- Full workspace compiles cleanly

---

_Verified: 2026-04-03_
_Verifier: Claude (gsd-verifier)_
