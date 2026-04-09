---
phase: 84-cli-gui-daemon
plan: 01
status: complete
wave: 1
completed: 2026-04-02
---

# Plan 01 Summary: Daemon Middleware Hardening

**Phase:** 84 - CLI/GUI/Daemon Auth Architecture Unification
**Wave:** 1 (no dependencies)
**Completed:** 2026-04-02

## What Was Built

Daemon L2+ middleware now explicitly rejects bare bearer tokens with a clear error code instead of silently failing JWT decode.

## Changes

| File                           | Change                                                                                                                                                       |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `middleware.rs`                | Replaced `unwrap_or(auth_value.as_str())` with explicit `else` branch returning `"invalid_auth_scheme"` error + hint about `/auth/connect`                   |
| `server.rs`                    | Removed dead `DaemonApiState::is_authorized()` method and unused `parse_bearer_token` import                                                                 |
| `tests/security_middleware.rs` | Added 3 Phase 84 tests: bare bearer gets `invalid_auth_scheme`, empty session gets `missing_session_token`, error codes are distinguishable from invalid JWT |

## Test Results

- `cargo test -p uc-daemon --test security_middleware` — **18 passed** (15 existing + 3 new)
- `cargo test -p uc-daemon --test api_auth` — **passed**
- Pre-existing failures: `clipboard_api` (1 failure), `pairing_api` (9 failures), `pairing_host` (3 failures) — unrelated to Phase 84

## Key Files Created/Modified

- `src-tauri/crates/uc-daemon/src/security/middleware.rs` — explicit bearer rejection
- `src-tauri/crates/uc-daemon/src/api/server.rs` — dead code removed
- `src-tauri/crates/uc-daemon/tests/security_middleware.rs` — 3 new tests

## Decisions

- D-04: Explicit rejection with `"invalid_auth_scheme"` error code (not silent JWT decode failure)
- Removed dead `is_authorized()` from server.rs after bearer acceptance removal

## Requirement Coverage

- AUTH-04: Daemon L2+ routes reject bare bearer tokens ✅
- AUTH-06: Bearer token only at /auth/connect ✅
