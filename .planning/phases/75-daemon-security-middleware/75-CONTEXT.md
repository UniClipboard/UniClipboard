# Phase 75: Daemon Security Middleware - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)

<domain>
## Phase Boundary

Add production-grade security middleware to the daemon HTTP API: JWT session tokens (5min TTL) exchanged from bearer token, optional PID whitelist verification, per-client rate limiting (100 req/min), and L1-L4 permission level enforcement on all endpoints.

This phase hardens the daemon API for direct frontend access by adding layered security beyond the existing bearer token.

</domain>

<decisions>
## Implementation Decisions

### JWT Session Token System

- POST `/auth/connect` endpoint: exchanges bearer token + client info for short-lived session token
- Session token: HS256 signed, 5-minute TTL
- Token claims: iss ("uniclipboard-daemon"), sub ("frontend"), iat, exp, pid, client_type, jti, access_level, encryption_ready
- Frontend uses `Authorization: Session <session_token>` for subsequent requests
- JWT secret: 32 random bytes generated at daemon startup (not persisted)

### PID Whitelist Verification

- SecurityState maintains `allowed_pids: RwLock<HashSet<u32>>`
- Tauri process PID auto-registered on daemon startup (if GUI-managed)
- Frontend PID registered during `/auth/connect`
- Optional X-Client-PID header checked against whitelist
- Unregistered PIDs rejected with 403 Forbidden

### Rate Limiting

- 100 requests per minute per client (identified by PID or IP)
- Implementation: `HashMap<String, Vec<Instant>>` with sliding window
- 429 Too Many Requests on exceeded limit

### Permission Levels (L1-L4)

- L1 Public: health check, status query — no auth required
- L2 Authenticated: list entries, read settings, stats — valid token required
- L3 Sensitive: view content, modify data, encryption ops — encryption session must be ready
- L4 Dangerous: delete all, reset settings — additional confirmation required

### Auth Flow

1. Frontend calls Tauri `daemon_connect_info` → gets baseUrl, wsUrl, bearerToken
2. Frontend POST `/auth/connect` with bearer token + PID
3. Daemon validates bearer, registers PID, returns session token
4. Frontend uses session token for all subsequent HTTP/WS requests
5. Token refresh every 4 minutes (before 5-minute expiry)

### Security Middleware Stack

- axum middleware layer order: rate_limiter → auth_extractor → permission_checker
- L1 routes bypass auth middleware entirely
- Auth middleware extracts and validates bearer OR session token

### Claude's Discretion

- jsonwebtoken crate version and configuration
- Rate limiter cleanup strategy (stale entries)
- Whether to use tower middleware or axum extractors
- Session token storage structure in daemon state
- Error response detail level for security endpoints

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Auth

- `src-tauri/crates/uc-daemon/src/api/` — Current bearer token validation in daemon
- `src-tauri/crates/uc-core/src/security/` — Security domain models
- `src-tauri/crates/uc-daemon/src/daemon_auth_token.rs` — DaemonAuthToken type

### Daemon API Structure

- `src-tauri/crates/uc-daemon/src/api/routes.rs` — Route registration and middleware application
- `src-tauri/crates/uc-daemon/src/api/handlers/` — Handler patterns
- `src-tauri/crates/uc-daemon/src/state.rs` — DaemonState/RuntimeState

### Wire Protocol

- `src-tauri/crates/uc-core/src/daemon_api_strings.rs` — API string constants

</canonical_refs>

<specifics>
## Specific Ideas

- Bearer token file already exists at `~/.config/uniclipboard/daemon.token` with permission 600
- Daemon already binds to 127.0.0.1 only (loopback isolation)
- The auth/connect flow is new — currently frontend accesses daemon only through Tauri bridge
- Firewall rules (pfctl, iptables) are optional/deferred
- PID verification is defense-in-depth against local malware, not a hard security boundary

</specifics>

<deferred>
## Deferred Ideas

- Audit logging (Layer 4) — optional, can be added later
- Firewall rules for additional network isolation
- Token rotation for long-running sessions
- Multi-user session isolation for shared computers

</deferred>

---

_Phase: 75-daemon-security-middleware_
_Context gathered: 2026-03-29 via PRD Express Path_
