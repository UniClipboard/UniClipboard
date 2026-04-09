# Phase 84: CLI/GUI/Daemon Auth Architecture Unification - Research

**Researched:** 2026-04-02
**Domain:** Daemon HTTP API authentication, session token lifecycle, CLI-GUI-daemon calling pattern consolidation
**Confidence:** HIGH

## Summary

The project has two divergent authentication calling patterns: the frontend (GUI) properly exchanges a local bearer token for a short-lived JWT session via `POST /auth/connect` and uses that JWT for all subsequent requests, while the CLI sends the raw bearer token directly with every request as `Authorization: Bearer`. The daemon currently accepts both patterns on L2+ routes (CLI's direct bearer in headers, GUI's JWT in query params). Phase 84 unifies both clients to use the same session token exchange flow, removes direct bearer token acceptance on protected routes, and ensures CLI and GUI maintain independent token scopes.

## User Constraints (from Phase Description)

### Locked Decisions

1. **Daemon is the sole entry point** -- CLI and GUI must only talk to daemon, not each other
2. **"First knock" is separate from "real business"** -- Local key file (bearer token) used only for one-time identity verification; daemon issues short-lived JWT session token
3. **CLI and GUI get independent tokens** -- Do not let CLI reuse GUI's tokens or vice versa; independent registration, independent renewal
4. **Shared "local-to-daemon" layer does exactly 4 things** -- Find daemon, read local key, exchange for short-lived token
5. **Daemon maintains ONE authentication method** -- Must NOT accept both "old key direct connect" AND "new token" simultaneously
6. **No cross-client token borrowing** -- CLI cannot borrow GUI's current login state

### Out of Scope (Deferred Ideas)

- Cross-machine remote authentication (v0.4.0 is local-only)
- Plugin system for auth providers
- Changing the underlying cryptographic primitives (HS256 JWT, bearer token file format)

## Phase Requirements

> Phase 84 has not yet defined specific requirement IDs. This section will be populated when REQUIREMENTS.md is updated.

## Current Architecture Analysis

### How CLI Currently Calls Daemon

**Location:** `src-tauri/crates/uc-cli/src/daemon_client.rs`

```rust
// DaemonHttpClient::new() reads bearer token from disk
let token_path = resolve_token_path();
let token = std::fs::read_to_string(&token_path)?;

// Every request sends Bearer token directly
self.http.get(format!("{}{}", self.base_url, path))
    .header(AUTHORIZATION, format!("Bearer {}", self.token))
    .send()
```

**Key characteristics:**

- Reads token from `$XDG_RUNTIME_DIR/uniclipboard-daemon.token` (profile-aware)
- Sends raw bearer token with **every** request
- **No PID registration** -- daemon cannot track which CLI process is making requests
- **No rate limiting by PID** -- CLI is not in the PID whitelist
- **No token expiry/renewal** -- bearer token is effectively permanent
- Falls back to `UNICLIPBOARD_DAEMON_TOKEN_PATH` env var

### How GUI Currently Calls Daemon

**Location:** `src/api/daemon/client.ts` + `src/lib/daemon-auth.ts`

```
1. Frontend polls Tauri command get_daemon_connection_info
   --> Returns { baseUrl, wsUrl, token: "<bearer-token>", pid: <tauri-pid> }

2. daemonClient.initialize(config)
   - Stores baseUrl, wsUrl, bearer token, PID

3. daemonClient.refreshSession()  --> POST /auth/connect
   - Body: { pid, clientType: "gui" }
   - Also sends token in URL: ?token=<bearer-token>
   - Response: { sessionToken: "<jwt>", expiresInSecs: 300, refreshAtSecs: 240 }

4. All subsequent requests use: ?auth=Session <jwt>  (URL query param)
   - daemonClient.request() auto-refreshes before expiry
   - Auto-retries once on 401

5. Keep-alive: refreshes every 4 minutes (240 seconds)
```

**Key characteristics:**

- **PID registration** -- GUI PID registered via `/auth/connect`
- **Rate limiting** -- GUI is tracked in PID whitelist
- **Short-lived JWT** -- 5-minute TTL, proactive refresh at 4 minutes
- **Independent session** -- GUI gets its own JWT, separate from CLI

### How Daemon Currently Validates Requests

**Location:** `src-tauri/crates/uc-daemon/src/security/middleware.rs`

```rust
// auth_extractor_middleware for L2+ routes:
// 1. Extracts from Authorization header OR ?auth= query param
let auth_value = auth_header.map(str::to_owned)
    .or(auth_query);  // auth_query parses ?auth= URL param

// 2. Accepts BOTH "Session <jwt>" AND bare token
let token = auth_value
    .strip_prefix("Session ")
    .unwrap_or(auth_value.as_str());  // <-- accepts bare token too!

// 3. JWT verify only runs on the token
let claims = SessionTokenClaims::verify(token, &state.security.jwt_secret)
```

**Current state:**

- `middleware.rs` accepts bare tokens (no "Session " prefix) -- but bearer tokens fail JWT verification since they're not valid JWTs
- `connect_handler` in `connect.rs` accepts bearer token in body OR `Authorization: Bearer` header
- `DaemonApiState::is_authorized()` in `server.rs` accepts bare bearer tokens (used by CLI via header)
- **Both bearer and JWT are currently accepted on L2+ routes**

### Architecture Gap Summary

| Aspect              | CLI                    | GUI                      |
| ------------------- | ---------------------- | ------------------------ |
| Auth method         | Bearer token in header | JWT session via /connect |
| PID registered      | No                     | Yes                      |
| Rate limiting       | No (unlimited)         | Yes (PID-based)          |
| Token expiry        | None (permanent)       | 5 min TTL                |
| Token renewal       | None                   | Auto, every 4 min        |
| Daemon auth pattern | Direct Bearer (L2+)    | JWT via query param      |
| Token scope         | Shared with GUI        | Independent              |

**The core problem:** The daemon's L2+ middleware and route handlers accept bearer tokens directly (CLI path), while the GUI uses the proper session exchange flow. This means:

1. CLI bypasses PID whitelist and rate limiting
2. CLI tokens never expire
3. CLI and GUI tokens are not isolated (same bearer token file)

## Standard Stack

### Core Libraries

| Library               | Version                       | Purpose                           | Why Standard             |
| --------------------- | ----------------------------- | --------------------------------- | ------------------------ |
| `jsonwebtoken = "10"` | (verify from Cargo.toml)      | HS256 JWT signing/verification    | Phase 75 already uses it |
| `reqwest`             | (already in uc-daemon-client) | HTTP client for CLI               | Already in use           |
| `rand`                | (already in workspace)        | Random bytes for token generation | Already in use           |

### No New Dependencies

The phase reuses existing infrastructure:

- `POST /auth/connect` endpoint (Phase 75) -- already exists
- `SessionTokenClaims` (Phase 75) -- already exists
- `SecurityState` with PID whitelist and rate limiter (Phase 75) -- already exists
- `daemon.token` file reading (Phase 45) -- already exists

### Installation

```bash
# No new packages needed -- all infrastructure already in place
```

## Architecture Patterns

### Recommended Unified Auth Flow

```
CLI Process                              Daemon
     |                                      |
     |  1. Read daemon.token (bearer)        |
     |                                      |
     |  2. POST /auth/connect  --------->   |
     |     { pid: <cli-pid>,                |
     |       clientType: "cli",              |
     |       token: "<bearer>" }            |
     |                                      |  [Validate bearer token]
     |                                      |  [Register PID in whitelist]
     |                                      |  [Sign JWT session token]
     |  <-- { sessionToken: "<jwt>",        |
     |         expiresInSecs: 300 }          |
     |                                      |
     |  3. All subsequent requests:          |
     |     ?auth=Session <jwt>  --------->   |
     |                                      |  [Verify JWT]
     |                                      |  [Check PID whitelist]
     |                                      |  [Check rate limit]
     |                                      |  [Serve request]
```

### Shared Auth Layer (The "4 Things")

A new shared module (likely in `uc-daemon-client` or a new `uc-local-auth` crate) should encapsulate:

```rust
// Responsibilities (only 4):
// 1. Find daemon endpoint (HTTP address resolution)
pub fn resolve_daemon_endpoint() -> Result<DaemonEndpoint>

// 2. Read local key file (bearer token from daemon.token)
pub fn read_local_key() -> Result<String>

// 3. Exchange for short-lived token (POST /auth/connect)
pub async fn exchange_session_token(
    http: &Client,
    endpoint: &DaemonEndpoint,
    pid: u32,
    client_type: &str,
) -> Result<SessionToken>

// 4. (Token management -- renewal, storage, disposal)
//    This is NOT just 4 things -- token renewal is complex.
//    Consider: the "4 things" applies to the SHARED infrastructure,
//    while renewal differs per client (GUI refreshes proactively,
//    CLI refreshes on demand or per-command).
```

### Token Scope Isolation

```
daemon.token (file on disk)
    |
    +--> GUI Process (PID X)
    |        Exchange --> JWT_GUI (PID X, clientType: "gui")
    |        Used for: All GUI HTTP/WS requests
    |
    +--> CLI Process (PID Y)
             Exchange --> JWT_CLI (PID Y, clientType: "cli")
             Used for: All CLI commands
```

Key: `JWT_GUI.jti != JWT_CLI.jti` -- different tokens, different PIDs. Daemon maintains separate rate limit counters per PID.

## Don't Hand-Roll

| Problem                  | Don't Build          | Use Instead                                                         | Why                                            |
| ------------------------ | -------------------- | ------------------------------------------------------------------- | ---------------------------------------------- |
| PID-based rate limiting  | Custom PID tracking  | Existing `SecurityState::allowed_pids` + `SlidingWindowRateLimiter` | Already implemented, tested in Phase 75        |
| Session token signing    | Custom signing       | `SessionTokenClaims::sign()` with HS256                             | Already implemented, tested in Phase 75        |
| Token expiry             | Custom TTL tracking  | `expiresAt` field in `SessionTokenClaims`                           | Already validated                              |
| Token renewal            | Custom refresh logic | Proactive refresh (GUI) + on-demand (CLI)                           | Consistent with daemon-client pattern          |
| Bearer token file access | Custom file I/O      | `load_or_create_auth_token()` in `uc-daemon/src/api/auth.rs`        | Already handles permissions, profile-awareness |

## Common Pitfalls

### Pitfall 1: Accepting Both Auth Methods Creates Security Gap

**What goes wrong:** If daemon continues accepting both bearer tokens (old CLI pattern) AND JWT tokens (new unified pattern), the security benefit is negated. Attackers can bypass PID whitelist and rate limiting by using the old bearer token path.

**Why it happens:** Fear of breaking CLI during migration. Keeping both paths "temporarily" leads to permanent dual-path.

**How to avoid:** Remove bearer token acceptance from L2+ middleware in one atomic change. CLI migration happens simultaneously.

**Warning signs:** Code review shows `parse_bearer_token` used in route handlers, or middleware accepts both `Bearer` and `Session` prefixes.

### Pitfall 2: CLI Session Token Cache Across Commands

**What goes wrong:** Each CLI command invocation creates a new process, so session tokens cannot be cached in-process memory. If CLI exchanges a new JWT on every command, it registers a new PID each time (or uses the same PID, depending on how it's obtained).

**Why it happens:** CLI is stateless between commands. Unlike the GUI which has a long-running process, each `uniclipboard-cli status` is a fresh invocation.

**How to avoid:** Use `std::process::id()` for PID. Cache the exchanged JWT in a temporary file with appropriate permissions. Consider `--no-cache` flag for scripts. The daemon's PID whitelist naturally handles this because each CLI invocation's PID is different -- but the rate limiter tracks by PID string, so every invocation creates a new rate limit window. This is actually fine since each command is independent.

### Pitfall 3: Token File Permission Escalation

**What goes wrong:** If the `daemon.token` file has overly permissive file permissions (world-readable), any local process can read the bearer token and exchange it for a session JWT.

**Why it happens:** Token file created with default umask (644 or similar).

**How to avoid:** Existing code already sets 0600 permissions (`repair_token_permissions` in `auth.rs`). Verify this runs on every token read, not just creation. Test on platforms with different umask defaults.

### Pitfall 4: GUI Borrowing CLI's Token (or Vice Versa)

**What goes wrong:** If both GUI and CLI use the same PID or same token file location without profile awareness, they could share tokens.

**Why it happens:** Profile awareness gaps. The `daemon.token` file already has profile suffixes (`uniclipboard-daemon.token`, `uniclipboard-daemon-a.token`, etc.), but the profile must be consistent.

**How to avoid:** Both clients already use profile-aware path resolution via `UC_PROFILE` env var. The key is ensuring daemon-client and CLI use the same resolution logic. Currently they do (both call `resolve_daemon_token_path_from`).

### Pitfall 5: WebSocket Auth Still Uses Bearer Token

**What goes wrong:** Phase 75 notes WS upgrade validates session JWT, but the frontend WS bootstrap may still pass connection info that includes the bearer token.

**Why it happens:** The WS URL doesn't carry the bearer token -- it carries the session JWT. But `get_daemon_connection_info` returns the bearer token, and if WS connection logic accidentally uses bearer instead of JWT, there's a gap.

**How to avoid:** Ensure WS connection uses `daemonClient.currentSession.token` (JWT), not `payload.token` (bearer). Current code in `daemon-ws-bootstrap.ts` line 59 does `await daemonClient.refreshSession()` before `daemonWs.connect()`. The WS connection reads from `daemonClient.currentSession.token`. This is correct -- verify during migration.

## Code Examples

### Daemon Endpoint (daemon-client side)

**Source:** `src-tauri/crates/uc-daemon-client/src/http/mod.rs` (existing pattern)

```rust
// Current: CLI reads token file, sends bearer with every request
// Future: CLI uses this pattern for session exchange

pub async fn exchange_session_token(
    http: &Client,
    connection_state: &DaemonConnectionState,
    pid: u32,
    client_type: &str,
) -> Result<SessionToken> {
    let connection = connection_state.get()
        .ok_or_else(|| anyhow!("daemon connection info unavailable"))?;

    let url = format!("{}/auth/connect", connection.base_url);
    let response = http
        .post(&url)
        .header(AUTHORIZATION, format!("Bearer {}", connection.token))
        .json(&serde_json::json!({
            "pid": pid,
            "clientType": client_type
        }))
        .send()
        .await?;

    // ... parse response, return SessionToken
}
```

### Frontend Client (TypeScript)

**Source:** `src/api/daemon/client.ts` (existing pattern -- already correct)

```typescript
// doRefreshSession() in DaemonClient already:
const body = new URLSearchParams({
  token: config.token, // bearer token from daemon.token
  pid: String(config.pid), // GUI's PID
  clientType: 'gui', // <-- distinguishes GUI from CLI
})

// request() uses ?auth=Session <jwt> for all calls
url.searchParams.set('auth', `Session ${this.session.token}`)
```

### CLI New Pattern (after unification)

The CLI would use `uc-daemon-client`'s session infrastructure:

```rust
// In uc-cli commands:
use uc_daemon_client::{DaemonConnectionState, http::exchange_session_token};

// 1. Resolve daemon endpoint (already exists in local_daemon.rs)
// 2. Read bearer token (already exists in daemon_client.rs)
// 3. Exchange for JWT session (NEW -- use uc-daemon-client pattern)
let session = exchange_session_token(&http_client, &conn_state, pid, "cli").await?;

// 4. All subsequent calls use authorized_daemon_request()
let req = authorized_daemon_request(&http_client, &conn_state, Method::GET, path, pid).await?;
let response = req.send().await?;
```

## Open Questions

1. **Should the shared auth layer be a new crate (`uc-local-auth`) or live in `uc-daemon-client`?**
   - `uc-daemon-client` is the natural home since it already has `exchange_session_token` and `authorized_daemon_request`
   - But `uc-daemon-client` is currently used only by the Tauri process; adding CLI as a consumer is fine
   - Recommendation: Extend `uc-daemon-client` with CLI-friendly wrappers, rather than creating a new crate

2. **Should CLI cache the exchanged JWT in a file for the profile's lifetime?**
   - PRO: Each CLI command doesn't need to exchange a new token (faster, less load on daemon)
   - CON: Token file with JWT would need secure permissions; if daemon restarts, JWT becomes invalid but cached file persists
   - Recommendation: Don't cache JWT in file. Each CLI command exchanges fresh. If performance is a concern, add a `--token-cache` flag for advanced users with explicit security warnings.

3. **Should `clientType` be an enum (`"gui"` | `"cli"`) or a free string?**
   - Current code uses a free string
   - Recommendation: Keep as free string for now (no validation needed); daemon can add validation later if it wants to differentiate behavior per client type

4. **Should the daemon's bearer token validation be removed from ALL routes, including `/auth/connect`?**
   - `/auth/connect` needs to accept bearer token (that's the whole point -- first knock)
   - All L2+ routes should reject bearer tokens
   - Current `middleware.rs` does accept bare tokens but they fail JWT verification since bearer isn't a valid JWT
   - Recommendation: Explicitly reject bearer tokens in middleware (make the rejection explicit rather than relying on JWT decode failure)

5. **What about the `DaemonApiState::is_authorized()` helper that accepts bearer tokens?**
   - It's used in route handlers for pre-auth checks
   - After removing bearer from L2+, this helper may become unused
   - Need to audit all call sites before deletion

## State of the Art

| Old Approach                         | Current Approach                                     | When Changed | Impact                              |
| ------------------------------------ | ---------------------------------------------------- | ------------ | ----------------------------------- |
| CLI sends bearer token directly      | CLI should exchange bearer for JWT via /auth/connect | Phase 84     | PID tracking, rate limiting for CLI |
| GUI exchanges bearer for JWT         | Already correct                                      | Phase 75     | No change                           |
| Bearer + JWT both accepted on L2+    | Should only accept JWT on L2+                        | Phase 84     | Hardened security                   |
| CLI uses separate `DaemonHttpClient` | CLI should use shared `uc-daemon-client`             | Phase 84     | DRY, consistent behavior            |

**Deprecated/outdated:**

- `DaemonApiState::is_authorized()` with bearer token support -- will become dead code after Phase 84
- `daemon_client.rs` in `uc-cli` separate HTTP client implementation -- replaced by `uc-daemon-client`

## Environment Availability

Step 2.6: SKIPPED (no external dependencies beyond the project's own code/config)

The phase involves only code refactoring and architectural consolidation:

- No new tools, CLIs, or runtimes required
- No external services or databases
- All infrastructure already exists in the codebase

## Validation Architecture

### Test Framework

| Property             | Value                                                                  |
| -------------------- | ---------------------------------------------------------------------- |
| Framework            | Vitest (frontend) + Rust `#[tokio::test]` (backend)                    |
| Config               | `vitest.config.ts` for frontend; inline `#[cfg(test)]` for Rust        |
| Quick run (frontend) | `bun test -- src/__tests__/lib/daemon-auth.test.ts`                    |
| Quick run (backend)  | `cd src-tauri && cargo test -p uc-daemon security -- --test-threads=4` |
| Full suite (backend) | `cd src-tauri && cargo test -p uc-daemon`                              |

### Phase Requirements -> Test Map

> Phase 84 has no formally defined requirement IDs yet. Below maps proposed behaviors to test types.

| Req ID             | Behavior                                                      | Test Type   | Automated Command                                             | File Exists?                |
| ------------------ | ------------------------------------------------------------- | ----------- | ------------------------------------------------------------- | --------------------------- |
| (proposed) AUTH-01 | CLI uses POST /auth/connect exchange instead of direct bearer | unit        | `cargo test -p uc-cli daemon_client`                          | ✅ `daemon_client.rs`       |
| (proposed) AUTH-02 | CLI PID registered in daemon PID whitelist                    | integration | `cargo test -p uc-daemon security_middleware`                 | ✅ `security_middleware.rs` |
| (proposed) AUTH-03 | CLI rate limited same as GUI                                  | unit        | `cargo test -p uc-daemon rate_limiter`                        | ✅ `rate_limiter.rs`        |
| (proposed) AUTH-04 | Daemon L2+ routes reject bare bearer tokens                   | integration | `cargo test -p uc-daemon security_middleware -- auth_connect` | ✅ `security_middleware.rs` |
| (proposed) AUTH-05 | CLI and GUI get independent session tokens                    | unit        | `bun test -- src/__tests__/lib/daemon-client.test.ts`         | ✅ `daemon-client.test.ts`  |
| (proposed) AUTH-06 | Bearer token only accepted at /auth/connect                   | integration | `cargo test -p uc-daemon api_auth`                            | ✅ `api_auth.rs`            |

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/tests/api_auth.rs` -- verify /auth/connect behavior (partially exists, needs CLI clientType coverage)
- [ ] `src/__tests__/lib/daemon-auth.test.ts` -- verify independent CLI/GUI session scopes (needs CLI-focused tests added)
- [ ] `src-tauri/crates/uc-cli/tests/cli_auth.rs` -- new integration tests for CLI auth flow (does not exist -- create)

### Sampling Rate

- **Per task commit:** Frontend: `bun test -- src/__tests__/lib/daemon-auth.test.ts src/__tests__/lib/daemon-client.test.ts`; Backend: `cd src-tauri && cargo test -p uc-daemon security_middleware`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon && cd ../.. && bun test -- src/__tests__/lib/daemon-auth.test.ts`
- **Phase gate:** Full suite green before `/gsd:verify-work`

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-daemon/src/security/middleware.rs` -- Current auth middleware accepting both bearer and JWT
- `src-tauri/crates/uc-daemon/src/security/connect.rs` -- POST /auth/connect endpoint (Phase 75)
- `src-tauri/crates/uc-daemon/src/security/claims.rs` -- SessionTokenClaims JWT structure (Phase 75)
- `src-tauri/crates/uc-cli/src/daemon_client.rs` -- CLI's current bearer-token-on-every-request pattern
- `src/api/daemon/client.ts` -- Frontend's correct session exchange pattern
- `src/lib/daemon-auth.ts` -- Frontend auth bootstrap flow
- `src-tauri/crates/uc-daemon-client/src/http/mod.rs` -- daemon-client session token exchange (model for CLI unification)
- `.planning/phases/75-daemon-security-middleware/75-01-PLAN.md` -- Phase 75 design decisions
- `.planning/phases/75-daemon-security-middleware/75-02-PLAN.md` -- Phase 75 middleware wiring

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon/src/api/server.rs` -- DaemonApiState with `is_authorized()` bearer acceptance
- `src-tauri/crates/uc-daemon/src/api/routes.rs` -- L1/L2 route split
- `src/__tests__/lib/daemon-auth.test.ts` -- Frontend auth test coverage
- `src/__tests__/lib/daemon-client.test.ts` -- Frontend client test coverage

### Tertiary (LOW confidence)

- Project-specific patterns not verified against external documentation

## Metadata

**Confidence breakdown:**

| Area           | Level | Reason                                                                            |
| -------------- | ----- | --------------------------------------------------------------------------------- |
| Standard Stack | HIGH  | All libraries already in use; Phase 75 established the JWT infrastructure         |
| Architecture   | HIGH  | Clear understanding of both calling patterns from source code inspection          |
| Pitfalls       | HIGH  | Derived from architectural analysis; all pitfalls have known mitigations          |
| CLI Pattern    | HIGH  | `daemon_client.rs` fully read and understood                                      |
| GUI Pattern    | HIGH  | `client.ts`, `daemon-auth.ts`, `daemon-ws-bootstrap.ts` fully read and understood |

**Research date:** 2026-04-02
**Valid until:** 2026-05-02 (architecture is stable; implementation details may shift)
