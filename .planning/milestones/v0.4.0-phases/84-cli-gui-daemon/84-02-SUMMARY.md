# Phase 84 Plan 02 Summary: CLI Session Exchange Pattern

## One-liner

CLI daemon client migrated to session exchange pattern using POST /auth/connect with CLI PID registration and independent JWT tokens per invocation.

## Objective

Migrate CLI from sending raw bearer tokens directly to using the POST /auth/connect session exchange flow. CLI uses `std::process::id()` for PID, exchanges bearer for JWT via /auth/connect, and uses "Session " prefix for all subsequent requests. No JWT file caching.

## Key Decisions

| Decision | Rationale | Outcome |
| -------- | --------- | ------- |
| GUI token caching preserved via `get_session_token` | Long-running GUI process benefits from cache; prevents redundant /auth/connect calls | Existing GUI tests pass unchanged |
| CLI uses `exchange_cli_session_token` (no cache) | Each CLI command is a fresh process; per D-07 no JWT file caching | Fresh token on every invocation |
| `authorized_daemon_request_with_type` routes by client_type | GUI gets cached tokens via `get_session_token`; CLI/others get fresh tokens via `exchange_session_token` | Unified API, correct per-client behavior |

## Tasks Executed

### Task 1: Extend uc-daemon-client session exchange with client_type parameter

**Status:** Deviation applied (auto-fix)

**What was found:** The `authorized_daemon_request_with_type` function was calling `exchange_session_token` directly for ALL client types, bypassing the `SESSION_TOKEN_CACHE`. This broke the existing GUI integration tests (which pre-populate the cache and expect it to be used).

**Fix:** Modified `authorized_daemon_request_with_type` to:
- Route `"gui"` client type to `get_session_token` (which uses caching)
- Route other types (e.g., `"cli"`) to `exchange_session_token` directly (no cache, fresh token)

```rust
let session_token = if client_type == "gui" {
    get_session_token(http, connection_state, pid).await?
} else {
    exchange_session_token(http, connection_state, pid, client_type).await?
};
```

**Files modified:**
- `src-tauri/crates/uc-daemon-client/src/http/mod.rs`

**Commit:** [see below]

---

### Task 2: Rewrite CLI daemon_client.rs to use session exchange pattern

**Status:** Already implemented (no changes needed)

The CLI daemon client was already fully migrated in the previous plan execution:
- `DaemonHttpClient` reads bearer token from `daemon.token` file
- CLI PID captured via `std::process::id()` on construction
- `exchange_cli_session_token` called via POST /auth/connect with `clientType: "cli"`
- In-memory session token cached in `RwLock<Option<String>>` for the command's lifetime
- All HTTP requests use `Authorization: Session <jwt>` header
- No JWT file caching (per D-07)

**Tests updated:** The existing tests needed to be updated to handle the two-phase request pattern (session exchange first, then actual API call):

- `set_pairing_gui_lease_uses_session_auth_not_bearer`: Updated server to accept two connections (first for /auth/connect, second for /pairing/gui/lease)
- `submit_setup_passphrase_tolerates_slow_success_response`: Updated server to handle session exchange on first connection, slow response on second
- `verify_setup_passphrase_tolerates_slow_success_response`: Same pattern as above

**Test fixes applied:**
- Changed `Self::read_full_request` to `read_full_request` (free function, not method) — fixed compilation error
- Moved `std::sync::Arc` import into test module to eliminate unused import warning
- Added `#[cfg(test)]` to `from_parts` and `resolve_base_url` (test-only helpers) to suppress dead_code warnings
- Added `#[allow(dead_code)]` to `expires_in_secs` field in `ConnectResponse` (deserialized but not used in caller)

**Files modified:**
- `src-tauri/crates/uc-cli/src/daemon_client.rs`

**Commit:** [see below]

## Verification Results

```
cd src-tauri && cargo test -p uc-cli daemon_client
test result: ok. 9 passed; 0 failed

cd src-tauri && cargo test -p uc-daemon-client
test result: ok. 11 passed; 0 failed

cd src-tauri && cargo check -p uc-cli && cargo check -p uc-daemon-client
Finished successfully (no warnings in production builds)
```

### Success Criteria

- [x] AUTH-01: CLI uses POST /auth/connect exchange instead of direct bearer
- [x] AUTH-02: CLI PID (std::process::id()) registered in daemon whitelist via /auth/connect
- [x] AUTH-03: CLI rate limited same as GUI (via PID-based rate limiter)
- [x] AUTH-05: CLI and GUI get independent session tokens (different jti, different PID)
- [x] D-07: No JWT caching in file — each CLI command exchanges fresh token
- [x] D-01: CLI uses POST /auth/connect with clientType: "cli"
- [x] `cargo test -p uc-cli daemon_client` passes
- [x] `cargo check -p uc-cli && cargo check -p uc-daemon-client` compile cleanly

## Metrics

- **Duration:** ~8 minutes
- **Tasks completed:** 2
- **Files modified:** 2 (`mod.rs`, `daemon_client.rs`)
- **Tests passing:** 20 total (9 CLI + 11 daemon-client)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed GUI test failures due to cache bypass**

- **Found during:** Running `cargo test -p uc-daemon-client`
- **Issue:** `authorized_daemon_request_with_type` called `exchange_session_token` directly for ALL client types, bypassing the `SESSION_TOKEN_CACHE`. This caused 7 existing GUI integration tests to fail because they pre-populate the cache.
- **Fix:** Added conditional logic: `"gui"` routes to `get_session_token` (caching), others route to `exchange_session_token` (no cache).
- **Files modified:** `src-tauri/crates/uc-daemon-client/src/http/mod.rs`
- **Commit:** [see below]

**2. [Rule 1 - Bug] Fixed CLI test compilation errors**

- **Found during:** Running `cargo test -p uc-cli`
- **Issue:** Test used `Self::read_full_request` (method syntax) but `read_full_request` is a free function (not a method). Also, tests didn't handle the two-phase session exchange flow.
- **Fix:** Changed `Self::read_full_request` to `read_full_request`. Updated test servers to handle two connections (first for /auth/connect, second for actual API call).
- **Files modified:** `src-tauri/crates/uc-cli/src/daemon_client.rs`
- **Commit:** [see below]

**3. [Rule 2 - Missing] Suppressed dead_code warnings**

- **Found during:** `cargo check`
- **Issue:** `expires_in_secs` field, `std::sync::Arc` import, `from_parts` and `resolve_base_url` (test-only) produced warnings in production builds.
- **Fix:** Added `#[allow(dead_code)]` to field, moved `Arc` to test module, added `#[cfg(test)]` to test-only helpers.
- **Files modified:** Both files
- **Commit:** [see below]

## Known Stubs

None.

## Self-Check: PASSED

- [x] SUMMARY.md created at `.planning/phases/84-cli-gui-daemon/84-02-SUMMARY.md`
- [x] Code commit `51b18b64` found in git log
- [x] Docs commit `1d73395b` found in git log
- [x] All 20 tests passing (9 CLI + 11 daemon-client)
- [x] Both packages compile cleanly (no warnings in production builds)
