# Phase 59: Secure daemon resource endpoints with scoped token auth - Context

**Gathered:** 2026-03-25
**Status:** Ready for planning
**Revision:** R2 (post Codex security review — TLS constraint, multi-layer rate limiting, audit expansion, token hardening)

<domain>
## Phase Boundary

Migrate blob/thumbnail resource serving from Tauri's in-process `uc://` protocol handler to daemon HTTP endpoints with production-grade security designed for future `0.0.0.0` exposure. GUI retains `uc://` as a transparent proxy layer (via daemon-client), so frontend code requires zero changes.

**In scope:**

- Daemon exposes `GET /uc/blob/:id` and `GET /uc/thumbnail/:id` axum route handlers
- Scoped resource token system: dedicated scope for resource reads, not shared with management bearer token
- Token lifecycle: 15-second TTL, lazy refresh on expired token before request
- Multi-layer rate limiting: per-token, per-IP, and token issuance throttling
- Encryption session binding: resource tokens bound to encryption session epoch, invalidated on relock
- Response security headers: `Cache-Control: no-store`, `Content-Disposition` as appropriate
- Endpoint hardening: GET-only, no request body, bounded concurrency, response size ceilings, timeout budgets
- Comprehensive audit logging: all security-relevant events (successes and failures)
- `uc-daemon-client` gains resource fetch methods (`fetch_blob`, `fetch_thumbnail`) with scoped token cache + refresh
- Tauri `uc://` protocol handler refactored: removes direct use-case calls, proxies through `uc-daemon-client` resource methods
- Update all import paths, no re-export stubs

**Explicitly NOT in scope:**

- Changing frontend resource loading code (`<img src="uc://...">` stays as-is)
- Removing `register_asynchronous_uri_scheme_protocol("uc", ...)` — it stays as the GUI proxy layer
- New content types or resource routes beyond blob/thumbnail
- 0.0.0.0 binding itself (security is designed for it, actual binding is a future phase)
- TLS implementation (hard prerequisite for 0.0.0.0 — see D-15)

</domain>

<decisions>
## Implementation Decisions

### Architecture approach

- **D-01:** Do NOT extract `parse_uc_request()` or `UcRoute` from uc-tauri to uc-core. Daemon uses axum native path extractors (`/uc/blob/:id`), no shared route enum needed. Tauri keeps its existing `protocol.rs` for the proxy layer's URI parsing.

### Daemon resource endpoints

- **D-02:** Daemon exposes `/uc/blob/:id` and `/uc/thumbnail/:id` as axum route handlers. Handlers call existing use cases (`resolve_blob_resource`, `resolve_thumbnail_resource`) from uc-app via CoreUseCases.

### Security — scoped token auth

- **D-03:** Resource access requires a dedicated scoped token, separate from the management bearer token. Read scope for resources must NOT be conflated with general API read permissions. Token has 15-second TTL.
- **D-04:** Lazy refresh with single-flight — daemon-client checks token expiry before each resource request; if expired, refreshes first then proceeds. No background timer. **Concurrent requests sharing an expired token MUST coalesce into a single refresh call** (single-flight pattern): one in-progress refresh is shared by all waiting resource requests, preventing thundering herd on `POST /uc/auth/token`. (Codex R2 F-2)
- **D-05:** Daemon issues scoped resource tokens via a token endpoint (e.g., `POST /uc/auth/token`). Management bearer token is required to obtain a resource token.

### Security — token minimum requirements (Codex R1 F-7)

- **D-05a:** Resource tokens MUST meet these minimum security properties:
  - **Unforgeable:** Cryptographically signed (mechanism: Claude's discretion — HMAC, JWT, or opaque)
  - **Audience-restricted:** Token valid only for resource endpoints, rejected by management API
  - **Scoped:** Explicit `resource:read` scope claim. **Scope granularity trade-off (Codex R2 F-1):** Per-resource tokens (`blob:<id>`) rejected due to performance impact (Dashboard loads 50+ thumbnails simultaneously). Coarse `resource:read` scope is acceptable because: (a) BlobId/RepresentationId are UUIDs — not guessable, (b) 15s TTL limits exposure window, (c) multi-layer rate limiting prevents enumeration, (d) comprehensive audit trail detects abuse. If future 0.0.0.0 exposure requires finer granularity, upgrade to per-resource signed URLs.
  - **Identifiable:** Contains subject/device identity and unique token ID for replay tracking
  - **Temporal:** Contains issued-at and expiry timestamps; server rejects expired tokens
  - **Session-bound:** Bound to encryption session epoch at issuance (see D-08)
  - **Error semantics:** 401 for missing/invalid/expired token, 403 for insufficient scope or locked encryption session. Error responses MUST NOT leak resource existence (no difference between "not found" and "forbidden" for unauthenticated requests).

### Security — rate limiting (Codex R1 F-2)

- **D-06:** Multi-layer rate limiting:
  - **Inner layer (per-token):** thumbnail 100 req/s, blob 20 req/s — limits well-behaved clients
  - **Middle layer (per-IP):** Global per-source-IP rate limit across all tokens — prevents fresh-token minting bypass
  - **Outer layer (token issuance):** `POST /uc/auth/token` endpoint itself is rate-limited per management token — prevents token flood from stolen management credentials
  - Per-token limits are an inner control, NOT the primary DoS defense. Per-IP and issuance throttling are the primary controls.

### Security — additional layers

- **D-07:** Origin validation is a **browser-only supplementary hardening check**, not a security boundary. Authorization relies solely on cryptographically verified scoped tokens. Origin header is validated when present (reject unexpected origins), but absence of Origin does not block authenticated requests. (Codex R1 F-3: demoted from security control to supplementary check)
- **D-08:** Encryption session binding — resource tokens are bound to the encryption session epoch/ID at issuance time. Resource endpoints return 403 if: (a) encryption session is not unlocked, OR (b) the token's session epoch does not match the current session epoch. All previously issued resource tokens are implicitly invalidated on relock, rekey, or session transition. (Codex R1 F-4: strengthened from global gate to epoch-bound)
- **D-09:** Response headers:
  - `Cache-Control: no-store` — prevent disk caching of sensitive content
  - `X-Content-Type-Options: nosniff` — prevent MIME sniffing attacks (Codex R3 F-1)
  - `Content-Disposition`: `attachment` by default for generic blobs (prevents inline rendering of attacker-controlled content); `inline` only for vetted thumbnail/image responses with server-derived MIME type
  - Server MUST set explicit `Content-Type` from the use case result's `mime_type`; default to `application/octet-stream` when unknown

### Security — endpoint hardening (Codex R1 F-6)

- **D-09a:** Resource endpoints MUST enforce:
  - **GET-only:** Reject all other HTTP methods with 405
  - **No request body:** Reject requests with non-empty body
  - **Bounded concurrency:** Per-endpoint concurrent request cap (value: Claude's discretion)
  - **Timeout budgets:** Per-request timeout for use case execution (value: Claude's discretion)
  - **Response size ceiling:** Maximum response payload size; oversized responses MUST be rejected with 413 (never truncated — truncation produces malformed binary payloads). Fully audited. (Codex R3 F-2: fail-closed only, no truncation)
  - **No Range requests:** Reject Range headers initially (streaming/partial content is future scope)
  - **Transport-layer limits (Codex R3 F-3):** Server-level connection protections against slowloris/header-bloat: header read timeout, connection idle timeout, max header bytes, bounded keep-alive policy. Transport-layer rejections must be logged/metricked. (Concrete values: Claude's discretion)

### Security — audit (Codex R1 F-5)

- **D-10:** Comprehensive security audit logging covering ALL security-relevant events:
  - **Successful reads:** timestamp, token identity, resource type, resource ID, request origin, response size
  - **Authentication failures:** invalid/expired/missing token attempts with source IP and presented credentials (redacted)
  - **Authorization failures:** valid token but insufficient scope, locked encryption session, session epoch mismatch
  - **Rate limit violations:** which limit layer triggered (per-token, per-IP, issuance), source IP, token identity
  - **Token lifecycle:** issuance, refresh, and implicit invalidation (relock/rekey events)
  - **Malformed requests:** invalid resource IDs, rejected HTTP methods, oversized bodies
  - All audit entries include: actor identity (token subject or "unauthenticated"), source IP, decision outcome, reason code
  - Secrets (token values, blob content) MUST be redacted from audit entries

### Security — TLS prerequisite (Codex R1 F-1)

- **D-15:** **HARD CONSTRAINT:** Resource endpoints MUST NOT be exposed on non-loopback interfaces (`0.0.0.0`) without TLS. This phase designs security for network exposure but does NOT implement TLS. Any future phase enabling `0.0.0.0` binding MUST implement TLS (or mutual TLS) as a prerequisite — this is non-negotiable. On `127.0.0.1`, token-based auth is the primary security layer.

### GUI proxy layer

- **D-11:** Tauri `uc://` protocol handler stays but is refactored: instead of calling use cases directly, it proxies through `uc-daemon-client` resource methods. Frontend `<img src="uc://blob/xxx">` continues working unchanged.
- **D-12:** Tauri backend does NOT use reqwest directly — all daemon communication goes through `uc-daemon-client` crate.
- **D-13:** Tauri side holds a scoped resource token in memory with lazy refresh (15s TTL). Token obtained from daemon on first resource request.

### Migration strategy

- **D-14:** Direct delete + update imports, no re-export stubs (consistent with Phase 58 D-05).

### Claude's Discretion

- Exact axum middleware structure for rate limiting and auth
- Token signing mechanism (HMAC, JWT, or opaque) — must satisfy D-05a minimum requirements
- Audit log storage format and location
- Whether to batch-sign URLs or sign per-request in the Tauri proxy
- `Content-Disposition` strategy (attachment vs inline per resource type)
- Concrete values for: per-IP rate limits, concurrency caps, timeout budgets, response size ceilings
- Token issuance rate limit values

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Current uc:// protocol implementation

- `src-tauri/crates/uc-tauri/src/protocol.rs` — UcRoute, UcRequestError, parse_uc_request() (stays for Tauri proxy layer)
- `src-tauri/src/main.rs` lines 74-262 — resolve_uc_request(), resolve_uc_blob_request(), resolve_uc_thumbnail_request(), CORS helpers, response builders (refactored to proxy)

### Daemon HTTP server

- `src-tauri/crates/uc-daemon/src/api/` — Existing daemon HTTP routes (pattern reference for new resource routes)
- `src-tauri/crates/uc-daemon/src/api/ws.rs` — WebSocket auth pattern (bearer token check reference)

### daemon-client

- `src-tauri/crates/uc-daemon-client/src/http/` — Existing HTTP client methods (pattern for new resource fetch methods)
- `src-tauri/crates/uc-daemon-client/src/lib.rs` — Crate exports

### Use cases (already exist, no changes needed)

- `src-tauri/crates/uc-app/src/usecases/clipboard/get_entry_resource.rs` — resolve_blob_resource use case
- `src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` — EntryProjectionDto with uc:// URL construction

### Auth infrastructure

- `src-tauri/crates/uc-daemon/src/` — Daemon auth token file and bearer token verification (Phase 45)

### Prior phase decisions

- Phase 45 context — daemon auth token architecture
- Phase 58 context — D-05: no re-export stubs migration strategy

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `resolve_blob_resource` / `resolve_thumbnail_resource` use cases in uc-app — already exist, daemon route handlers just call them
- daemon HTTP auth middleware — bearer token check already implemented, extend for scoped tokens
- `uc-daemon-client` HTTP client infrastructure — reqwest client with base_url and token management

### Established Patterns

- daemon routes use axum with `DaemonApiState` extension for accessing runtime services
- daemon-client methods return typed results with error handling
- Tauri protocol handler uses `tauri::async_runtime::spawn` for async resolution

### Integration Points

- `src-tauri/src/main.rs` lines 423-429 — `register_asynchronous_uri_scheme_protocol("uc", ...)` — refactor resolve logic to daemon-client proxy
- `src-tauri/crates/uc-daemon/src/api/server.rs` — Add new resource routes to axum router
- `src-tauri/crates/uc-daemon-client/src/http/` — Add resource fetch methods with scoped token management
- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` — May need to initialize scoped token state

</code_context>

<specifics>
## Specific Ideas

- **Security-first design:** User explicitly requires all security measures designed for future `0.0.0.0` exposure, even though current binding is `127.0.0.1`.
- **Scoped token isolation:** Resource read scope must be strictly separated from management API scope. This is a non-negotiable security requirement.
- **Audit trail mandatory:** Every security-relevant event must be logged — not just successful reads but also failures, rate limit triggers, and token lifecycle events.
- **15-second token TTL:** Short-lived tokens minimize exposure window. Lazy refresh keeps implementation simple.
- **TLS is a hard gate:** No network exposure without TLS. This constraint must be enforced at the infrastructure level in any future 0.0.0.0 phase.

</specifics>

<deferred>
## Deferred Ideas

### Reviewed Todos (not folded)

- "修复 setup 配对确认提示缺失" — UI bug, unrelated to resource endpoint security. Belongs in a separate UI fix phase.

### Future considerations

- **TLS for daemon HTTP server** — HARD PREREQUISITE for 0.0.0.0 binding (D-15). Must be implemented before any non-loopback exposure. Consider mutual TLS for device-authenticated access.
- Actual `0.0.0.0` binding — blocked on TLS implementation
- Cross-device resource access via daemon — depends on 0.0.0.0 + TLS
- Range/partial content requests for large blobs — initially rejected (D-09a), revisit when streaming is needed
- Sender-constrained tokens / proof-of-possession — additional hardening for network exposure scenario

</deferred>

---

_Phase: 59-secure-daemon-resource-endpoints_
_Context gathered: 2026-03-25_
_Revised: 2026-03-25 (post Codex security review R3 — 12 findings total, all addressed)_
