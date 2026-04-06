# Security Audit Report — Phase 75

**Date**: 2026-03-30
**Scope**: Daemon HTTP API (uc-daemon), Frontend Daemon Client (src/api/daemon/, src/lib/)
**Phase**: M003 S05 — Frontend-Daemon Integration Testing & Security Audit

---

## 1. Token Leakage Check

### Check: No session tokens stored in localStorage, sessionStorage, or cookies

| File                       | Finding                                                                                                                       |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `src/api/daemon/client.ts` | Session token stored **in-memory only** (`this.session`). `destroy()` clears it.                                              |
| `src/lib/daemon-auth.ts`   | Uses `daemonClient` in-memory session. No persistence.                                                                        |
| `src/lib/daemon-ws.ts`     | WebSocket auth token passed via URL query param (`?auth=Session%20TOKEN`) at connection time only — ephemeral, not persisted. |

**grep results** (non-test files):

- `src/components/feedback/FeedbackDialog.tsx`: `localStorage` used only for user email (not tokens) ✅
- `src/components/clipboard/ClipboardContent.tsx`: `localStorage` used only for panel layout preferences ✅
- `src/i18n/index.ts`: `localStorage` used only for language preference ✅

**Result**: ✅ PASS — No session tokens stored in persistent browser storage. Session tokens exist only in memory (`daemonClient.session`).

---

## 2. Bearer Token Placement

### Check: Authorization header used for HTTP, URL query param used only for WebSocket

| Location                       | Auth Method                                                  | Assessment                                                            |
| ------------------------------ | ------------------------------------------------------------ | --------------------------------------------------------------------- |
| `src/api/daemon/client.ts:172` | `Authorization: Bearer <token>` (initial `/auth/connect`)    | ✅ Correct                                                            |
| `src/api/daemon/client.ts:214` | `Authorization: Session <session>` (all subsequent requests) | ✅ Correct                                                            |
| `src/lib/daemon-ws.ts:203`     | `?auth=Session%20TOKEN` URL query param                      | ⚠️ Acceptable — browsers prohibit custom headers on WebSocket upgrade |

**WebSocket auth rationale**: Browser WebSocket API (`new WebSocket(url)`) does not support custom headers during the upgrade handshake. The session token is passed in the URL query parameter (`?auth=Session%20TOKEN`) at connection time. This is:

- **Acceptable** because the daemon runs on the local loopback interface only (`127.0.0.1`)
- **Recorded** as a known limitation — loopback traffic does not cross network boundaries
- **Defended** by: (1) JWT signature verification on the server, (2) PID whitelist, (3) rate limiting

**Result**: ✅ PASS for HTTP — ⚠️ ACCEPTABLE for WebSocket (browser API limitation, loopback-only)

---

## 3. Rate Limiting

### Check: 101 requests in <1 minute → 429 on 101st

**Configuration** (`src-tauri/crates/uc-daemon/src/security/rate_limiter.rs`):

```rust
const MAX_REQUESTS: u32 = 100;
const WINDOW_SECS: u64 = 60;
```

**Implementation**: Sliding-window rate limiter using `tokio::time::Instant`. Per-client tracking by `client_id` (PID string from validated JWT claims).

**Coverage**:
| Endpoint | Rate Limited By | Notes |
|----------|----------------|-------|
| `POST /auth/connect` | Client IP (via `ConnectInfo<SocketAddr>`) | Pre-auth; IP from TCP stack, not caller input |
| All L2+ endpoints | PID from validated JWT | After auth; PID verified from signed JWT |

**Unit tests** (`src-tauri/crates/uc-daemon/src/security/rate_limiter.rs`):

- ✅ `under_limit_allows` — 100 requests allowed
- ✅ `over_limit_rejects` — 101st rejected
- ✅ `per_client_isolation` — client-b unaffected by client-a exhaustion
- ✅ `window_sliding_allows_after_expiry` — window expiry restores quota
- ✅ `cleanup_removes_stale_entries` — background cleanup prevents memory growth

**Result**: ✅ PASS — 100 req/min per client enforced. Background cleanup task runs every 5 minutes.

---

## 4. Permission Enforcement

### Check: L2/L3/L4 enforcement as documented

**L2 Enforcement (Phase 75 — implemented)**:
| Check | Implementation | Status |
|-------|--------------|--------|
| Missing session token | `auth_extractor_middleware` → 401 `missing_session_token` | ✅ |
| Invalid/expired JWT | `SessionTokenClaims::verify` → 401 `invalid_session_token` | ✅ |
| PID not in whitelist | `is_pid_allowed(pid)` → 403 `pid_not_allowed` | ✅ |

**Middleware chain** (`src-tauri/crates/uc-daemon/src/api/routes.rs`):

```rust
// Layer order (innermost = runs first):
.layer(rate_limit_middleware)        // SECOND — checks rate limit after auth
.layer(auth_extractor_middleware)   // FIRST — validates JWT + PID
```

**L3/L4 Enforcement (Phase 75 — NOT implemented)**:
| Check | Status | Notes |
|-------|--------|-------|
| L3: encryption session required | ❌ NOT ENFORCED | Deferred to Phase 76+ |
| L4: dangerous endpoint confirmation | ⚠️ PARTIAL | `POST /storage/clear-cache` requires `confirmed: true` (L4 confirmation) |

**L4 confirmation check** (`src-tauri/crates/uc-daemon/src/api/storage.rs`):

```rust
if !req.confirmed {
    return (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": ClearCacheErrorResponse {
                code: "confirmation_required".to_string(),
                message: "confirmed field must be set to true"
            }
        })),
    )
}
```

**Result**: ✅ PASS for L2 — ✅ PASS for L4 (`/storage/clear-cache` confirmed field) — ❌ NOT IMPLEMENTED for L3 (encryption state gating) — documented as Phase 76 scope

---

## 5. PID Verification

### Check: Requests with wrong PID → rejection

**PID whitelist** (`src-tauri/crates/uc-daemon/src/security/state.rs`):

```rust
pub async fn is_pid_allowed(&self, pid: u32) -> bool {
    let pids = self.allowed_pids.read().await;
    pids.contains(&pid)
}
```

**PID registration flow**:

1. `POST /auth/connect` → bearer token validated → `security.register_pid(req.pid)` called
2. JWT issued with `pid` claim (from request body)
3. Subsequent requests → JWT verified → `is_pid_allowed(claims.pid)` checked

**Trust model** (documented in `src-tauri/crates/uc-daemon/src/security/connect.rs`):

> The PID from the request body is trusted because: (1) The bearer token has already been validated; (2) The frontend runs on the same machine; (3) PID verification is defense-in-depth against local malware, not a hard security boundary; (4) The bearer token file has filesystem permissions (600).

**Test coverage** (`src-tauri/crates/uc-daemon/src/security/state.rs`):

- ✅ `register_and_check_pid` — registered PID accepted
- ✅ `unregistered_pid_rejected` — unregistered PID rejected

**Result**: ✅ PASS — PID whitelist enforced. Trust model documented.

---

## 6. CORS Configuration

### Check: No wildcard CORS headers on daemon HTTP responses

**Findings**:

| Component                 | CORS Status                   | Notes                                                |
| ------------------------- | ----------------------------- | ---------------------------------------------------- |
| `uc-daemon` HTTP API      | No CORS middleware configured | ✅ Loopback-only, no external origin possible        |
| `uc-daemon` WebSocket     | No CORS middleware configured | ✅ Loopback-only                                     |
| `uc-tauri` Tauri commands | CORS headers set in `main.rs` | ✅ `is_allowed_cors_origin` restricts to dev origins |

**Daemon Cargo.toml**: No `tower-http/cors` or CORS dependencies present. ✅

**Result**: ✅ PASS — No wildcard CORS. Daemon is loopback-only and not accessible from external origins.

---

## 7. Cryptographic Security

### Check: Key generation, storage, and JWT signing

| Item                    | Finding                                        | Assessment                         |
| ----------------------- | ---------------------------------------------- | ---------------------------------- |
| JWT signing             | HS256, 32-byte secret from `rand::rngs::OsRng` | ✅ Cryptographically secure        |
| Session token TTL       | 300s (5 minutes)                               | ✅ Short-lived                     |
| Refresh threshold       | 240s (4 minutes)                               | ✅ Proactive refresh before expiry |
| Bearer token generation | 32 random bytes, hex-encoded                   | ✅ Cryptographically secure        |
| Token file permissions  | `chmod 600` enforced on Unix                   | ✅ Filesystem-level protection     |
| JTI (JWT ID)            | 16 random bytes, hex-encoded, UUID v4 format   | ✅ Unique per token                |

**Token expiration test** (`src-tauri/crates/uc-daemon/src/security/claims.rs`):

```rust
#[test]
fn claims_expired_token_rejected() {
    let mut claims = SessionTokenClaims::new(...);
    claims.exp = chrono::Utc::now().timestamp() - 86400 * 7; // 7 days ago
    let token = claims.sign(&secret).expect("sign should succeed");
    let result = SessionTokenClaims::verify(&token, &secret);
    assert!(result.is_err(), "expired token should be rejected");
}
```

**Result**: ✅ PASS — All cryptographic operations use secure randomness and appropriate key sizes.

---

## 8. Known Limitations (Documented)

| Limitation                         | Location             | Risk   | Mitigation                                        |
| ---------------------------------- | -------------------- | ------ | ------------------------------------------------- |
| L3 not enforced                    | `routes.rs` comment  | Medium | Deferred to Phase 76; documented in code          |
| WebSocket auth via URL query param | `daemon-ws.ts:203`   | Low    | Loopback-only; JWT signature verified server-side |
| PID trust model                    | `connect.rs` comment | Low    | Defense-in-depth; bearer token validated first    |
| No pre-auth rate limiting          | `middleware.rs`      | Low    | IP-based rate limiting on `/auth/connect`         |

---

## Summary

| Check                                               | Result             | Notes                                 |
| --------------------------------------------------- | ------------------ | ------------------------------------- |
| Token leakage (localStorage/sessionStorage/cookies) | ✅ PASS            | In-memory only                        |
| Bearer token placement (HTTP header)                | ✅ PASS            | Authorization header used             |
| WebSocket auth (URL query param)                    | ⚠️ ACCEPTABLE      | Browser API limitation; loopback-only |
| Rate limiting (100 req/min)                         | ✅ PASS            | Sliding window, per-client            |
| L2 permission enforcement                           | ✅ PASS            | JWT + PID whitelist                   |
| L3 permission enforcement                           | ❌ NOT IMPLEMENTED | Phase 76 scope                        |
| L4 confirmation (clear-cache)                       | ✅ PASS            | `confirmed: true` required            |
| PID verification                                    | ✅ PASS            | Whitelist enforced                    |
| CORS wildcard                                       | ✅ PASS            | No CORS; loopback-only                |
| Cryptographic security                              | ✅ PASS            | Secure RNG, short TTLs                |

**Overall Assessment**: ✅ PASS with documented limitations

**Critical Issues**: 0
**High Issues**: 0
**Medium Issues**: 1 (L3 not enforced — documented as Phase 76 scope)
**Low Issues**: 3 (WebSocket URL auth, PID trust model, pre-auth rate limiting — all documented and accepted)

---

## 9. Browser-Compatible WebSocket Auth (M003 S07)

### Check: `?auth=Session%20TOKEN` query parameter

The browser WebSocket API (`new WebSocket(url)`) does not support custom headers on the upgrade request. The session token is therefore passed as a URL query parameter.

**Implementation** (`src/lib/daemon-ws.ts:197-204`):

```typescript
private _openSocket(): void {
  const token = daemonClient.currentSession?.token
  const url = this._wsUrl!
  // Browser-compatible: no custom headers on WS upgrade
  const authUrl = token ? `${url}?auth=${encodeURIComponent(`Session ${token}`)}` : url
  const ws = this._wsFactory(authUrl)
  // ...
}
```

**Daemon extraction** (`src-tauri/crates/uc-daemon/src/api/ws.rs`):

- Tries `Authorization: Session <token>` header first (non-browser clients)
- Falls back to `?auth=Session%20<token>` query parameter (browser clients)

**Security considerations**:

- Loopback-only: daemon only binds to `127.0.0.1` — traffic never leaves the machine
- JWT signature verified server-side regardless of how token was passed
- PID whitelist enforced after auth
- Rate limiting enforced per-client

### Proof Harness

The `scripts/verify-direct-daemon-ws.mjs` script provides automated verification:

```bash
# Self-test (no daemon required)
node scripts/verify-direct-daemon-ws.mjs --self-test

# Live mode (requires running daemon)
DAEMON_BASE_URL=http://127.0.0.1:<port> \
DAEMON_TOKEN=$(cat ~/Library/Application\ Support/uniclipboard/daemon.token) \
node scripts/verify-direct-daemon-ws.mjs --live
```

**Coverage**:

- Bearer→session exchange (HTTP POST /auth/connect)
- WebSocket open with `?auth=Session%20TOKEN`
- Topic subscribe and snapshot event receipt
- Reconnect after disconnect
- Token redaction in diagnostics

See `docs/uat/direct-daemon-ws.md` for full UAT runbook.

---

## 10. Consumer-Level WebSocket Coverage

### Check: Frontend consumer receives corrected WebSocket envelopes

The `useClipboardEventStream` hook (`src/hooks/__tests__/useClipboardEventStream.test.tsx`) verifies that:

1. Corrected daemon event envelopes (snake_case from Rust → camelCase normalized) reach the frontend consumer
2. The hook handles `clipboard.new-content`, `clipboard.deleted`, and remote invalidation events
3. Throttling logic correctly deduplicates rapid remote events

**Test coverage**:
| Test | What It Verifies |
|------|-----------------|
| `loads single local item and emits onLocalItem` | Envelope with `origin=local` triggers callback |
| `throttles remote invalidation` | Duplicate remote events within 300ms are deduped |
| `forwards delete events` | `clipboard.deleted` envelope triggers `onDeleted` callback |

**Run consumer tests**:

```bash
npx vitest run src/hooks/__tests__/useClipboardEventStream.test.tsx
```

---

## Summary

| Check                                               | Result             | Notes                                 |
| --------------------------------------------------- | ------------------ | ------------------------------------- |
| Token leakage (localStorage/sessionStorage/cookies) | ✅ PASS            | In-memory only                        |
| Bearer token placement (HTTP header)                | ✅ PASS            | Authorization header used             |
| WebSocket auth (URL query param)                    | ⚠️ ACCEPTABLE      | Browser API limitation; loopback-only |
| Rate limiting (100 req/min)                         | ✅ PASS            | Sliding window, per-client            |
| L2 permission enforcement                           | ✅ PASS            | JWT + PID whitelist                   |
| L3 permission enforcement                           | ❌ NOT IMPLEMENTED | Phase 76 scope                        |
| L4 confirmation (clear-cache)                       | ✅ PASS            | `confirmed: true` required            |
| PID verification                                    | ✅ PASS            | Whitelist enforced                    |
| CORS wildcard                                       | ✅ PASS            | No CORS; loopback-only                |
| Cryptographic security                              | ✅ PASS            | Secure RNG, short TTLs                |
| Browser WS auth (M003 S07)                          | ✅ PASS            | ?auth= query param; loopback-only     |
| Consumer WS coverage (M003 S07)                     | ✅ PASS            | useClipboardEventStream tests pass    |

**Overall Assessment**: ✅ PASS with documented limitations

**Critical Issues**: 0
**High Issues**: 0
**Medium Issues**: 1 (L3 not enforced — documented as Phase 76 scope)
**Low Issues**: 3 (WebSocket URL auth, PID trust model, pre-auth rate limiting — all documented and accepted)

---

## Verification Commands

```bash
# Token leakage check
grep -rn "localStorage.setItem.*token\|sessionStorage.setItem.*token" src/ --include="*.ts" --include="*.tsx"

# Authorization header check
grep -rn "Authorization.*Bearer\|Authorization.*Session" src/api/daemon/ --include="*.ts"

# Rate limiter config
grep -n "MAX_REQUESTS\|WINDOW_SECS" src-tauri/crates/uc-daemon/src/security/rate_limiter.rs

# PID whitelist check
grep -n "is_pid_allowed" src-tauri/crates/uc-daemon/src/security/

# CORS check
grep -rn "cors\|tower-http" src-tauri/crates/uc-daemon/

# Confirmation check
grep -rn "confirmed" src-tauri/crates/uc-daemon/src/api/storage.rs

# Browser WS auth check
grep -n "auth.*encodeURIComponent\|encodeURIComponent.*auth" src/lib/daemon-ws.ts

# Consumer-level WS test
npx vitest run src/hooks/__tests__/useClipboardEventStream.test.tsx

# Live daemon WS proof harness
node scripts/verify-direct-daemon-ws.mjs --self-test
```
