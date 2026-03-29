# Phase 77: Frontend Daemon HTTP Client & Auth Module - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)

<domain>
## Phase Boundary

Create the frontend DaemonClient class with automatic session token management, connection initialization via Tauri invoke for bootstrap info, and typed request/error handling. This replaces Tauri invoke() as the primary API transport for subsequent migration phases.

</domain>

<decisions>
## Implementation Decisions

### DaemonClient Class (src/api/daemon/client.ts)

- Singleton `daemonClient` instance exported from module
- `initialize()` method: calls Tauri `daemon_connect_info` to get baseUrl, wsUrl, pid
- `refreshSession()` method: POST `/auth/connect` with bearer token + PID → session token
- `request<T>(endpoint, options?)` method: auto-refreshes expired session, sends with `Authorization: Session <token>`
- `destroy()` method: cleans up keep-alive timer
- Session keep-alive: refresh every 4 minutes (before 5-minute expiry)
- `isSessionExpired()` helper for pre-request validation

### DaemonConfig Interface

- baseUrl: string (e.g., "http://127.0.0.1:xxxxx")
- wsUrl: string (e.g., "ws://127.0.0.1:xxxxx/ws")
- pid: number (Tauri process PID)
- token: string (bearer token)
- expiresAt: number

### SessionToken Interface

- token: string (JWT session token)
- expiresAt: number
- encryptionReady: boolean

### Auth Module (src/lib/daemon-auth.ts)

- `loadDaemonAuth()` → calls Tauri invoke for connection config
- `verifyAuthState()` → checks daemon connection + encryption status
- `waitForEncryptionReady(timeout)` → polls until encryption session ready

### Error Handling

- DaemonApiError class with code, message, details from error response
- Error codes: UNAUTHORIZED, FORBIDDEN, NOT_FOUND, RATE_LIMITED, ENCRYPTION_NOT_READY, CONFIRMATION_REQUIRED, INTERNAL_ERROR
- Auto-retry on 401 with session refresh (once)

### Tauri Bootstrap Command (new)

- `daemon_connect_info` Tauri command in uc-tauri returns { baseUrl, wsUrl, token, pid }
- Reads daemon port and token from filesystem
- Provides PID of current Tauri process

### Claude's Discretion

- Whether to use fetch API directly or a lightweight HTTP client wrapper
- React context/provider pattern for DaemonClient lifecycle
- Token storage approach (in-memory only, never persisted in frontend)
- Retry/backoff strategy for failed requests
- TypeScript generic patterns for typed responses

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Frontend API Layer

- `src/api/` — Current Tauri invoke API layer (patterns to replace)
- `src/lib/` — Utility modules location

### Tauri Commands

- `src-tauri/crates/uc-tauri/src/commands/` — Current command patterns
- `src-tauri/crates/uc-daemon-client/` — Existing daemon client (Rust side)

### Daemon Connection Info

- `src-tauri/crates/uc-daemon/src/daemon_auth_token.rs` — Token type
- `src-tauri/crates/uc-daemon/src/api/` — Daemon API structure

### Frontend State

- `src/store/` — Redux store (may need daemon connection state slice)

</canonical_refs>

<specifics>
## Specific Ideas

- The frontend currently accesses backend exclusively via Tauri invoke() — this phase introduces an alternative HTTP transport
- DaemonClient should be usable without Tauri for testing (inject config manually)
- The auth module bridges Tauri (for bootstrap info only) and daemon HTTP (for all API calls)
- Connection lifecycle should handle daemon restart gracefully (detect connection failure, re-initialize)

</specifics>

<deferred>
## Deferred Ideas

- WebSocket client (Phase 79)
- Migrating specific API calls (Phase 78)
- Removing Tauri commands (Phase 80)
- Offline/disconnected state handling

</deferred>

---

_Phase: 77-frontend-daemon-http-client-auth_
_Context gathered: 2026-03-29 via PRD Express Path_
