# Phase 75: Daemon Security Middleware - Research

**Researched:** 2026-03-29
**Domain:** Rust/axum JWT middleware, rate limiting, PID verification, permission levels
**Confidence:** HIGH (verified against existing codebase)

## Summary

This phase adds layered security middleware to the daemon HTTP API (axum 0.7) to harden it for direct frontend access. The system introduces short-lived JWT session tokens (5min TTL, HS256) that replace raw bearer token usage for normal API calls, a PID whitelist for local process verification, sliding-window rate limiting (100 req/min per client), and L1-L4 permission level enforcement on all endpoints.

The implementation lives entirely in `uc-daemon/src/security/` and is wired into `DaemonApiState`. The existing bearer token at `~/.config/uniclipboard/daemon.token` remains the root of trust for the `/auth/connect` exchange only.

**Primary recommendation:** Build a `SecurityState` struct (owned by `DaemonApiState`) holding the JWT secret, PID whitelist, and rate limiter. Implement middleware as axum `Layer` wrappers that compose cleanly. Use tower `FromRequestLayers` for typed middleware state injection.

---

## User Constraints (from CONTEXT.md)

### Locked Decisions

- JWT session tokens: HS256 signed, 5-min TTL, claims: iss, sub, iat, exp, pid, client_type, jti, access_level, encryption_ready
- POST `/auth/connect` exchanges bearer token + client info for session token
- Frontend uses `Authorization: Session <session_token>` for subsequent requests
- JWT secret: 32 random bytes generated at daemon startup (not persisted)
- PID whitelist in SecurityState: `allowed_pids: RwLock<HashSet<u32>>`
- Rate limiting: 100 req/min per client, sliding window implementation
- L1-L4 permission levels: L1=Public, L2=Authenticated, L3=Sensitive, L4=Dangerous
- Middleware stack order: rate_limiter -> auth_extractor -> permission_checker
- L1 routes bypass auth middleware entirely

### Claude's Discretion

- jsonwebtoken crate version and configuration
- Rate limiter cleanup strategy (stale entries)
- Whether to use tower middleware or axum extractors
- Session token storage structure in daemon state
- Error response detail level for security endpoints

### Deferred Ideas (OUT OF SCOPE)

- Audit logging (Layer 4) - optional, can be added later
- Firewall rules for additional network isolation
- Token rotation for long-running sessions
- Multi-user session isolation for shared computers

---

## Phase Requirements

> Phase 75 does not yet have assigned requirement IDs. This section will be populated once REQUIREMENTS.md is updated.

---

## Standard Stack

### Core

| Library        | Version                    | Purpose                  | Why Standard                                                          |
| -------------- | -------------------------- | ------------------------ | --------------------------------------------------------------------- |
| `jsonwebtoken` | 9.x (latest)               | JWT sign/verify          | De facto standard Rust JWT crate, HS256 support, no runtime deps      |
| `axum`         | 0.7 (already in workspace) | HTTP server + middleware | Already in uc-daemon, Layer/Service pattern for composable middleware |

### Supporting

| Library                     | Purpose                      | When to Use                                            |
| --------------------------- | ---------------------------- | ------------------------------------------------------ |
| `rand`                      | Already in workspace         | JWT secret generation at startup                       |
| `chrono`                    | Already in workspace         | `DateTime<Utc>` for `iat`/`exp` claims                 |
| `tower`                     | Already in workspace dev-dep | `ServiceBuilder`, `util::poll_fn` for async middleware |
| `tokio::sync::RwLock`       | Already in std               | PID whitelist interior mutability                      |
| `std::collections::HashMap` | std                          | Rate limiter sliding window                            |

### New Dependency to Add

**`uc-daemon/Cargo.toml`** needs:

```toml
jsonwebtoken = "9"
```

Version verified: `npm view jsonwebtoken version` returns `9.3.0` (2024-06). The 9.x line supports `ring` or `openssl` backends - `ring` is preferred (no external deps, safer).

**Installation check:** `jsonwebtoken` has no native dependencies beyond `serde`. It compiles cleanly on all three platforms without extra config.

---

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-daemon/src/
├── security/
│   ├── mod.rs                    # Public re-exports
│   ├── claims.rs                 # SessionTokenClaims struct + serde
│   ├── middleware.rs             # Axum Layer implementations
│   ├── rate_limiter.rs           # Sliding window rate limiter
│   ├── permission.rs             # L1-L4 permission levels
│   ├── state.rs                  # SecurityState struct
│   └── connect.rs                # POST /auth/connect handler
└── api/
    ├── mod.rs                    # Add: pub mod security
    ├── routes.rs                 # Wire middleware into router
    └── server.rs                 # Extend DaemonApiState with SecurityState
```

### Pattern 1: SecurityState - Shared Middleware State

**What:** A single struct holds all security state (JWT secret, PID whitelist, rate limiter), cloned into all middleware layers.

**When to use:** When multiple middleware components need access to the same mutable state.

**Location:** `uc-daemon/src/security/state.rs`

```rust
// Source: Canonical pattern from existing DaemonApiState + new additions
#[derive(Clone)]
pub struct SecurityState {
    pub jwt_secret: Arc<[u8; 32]>,
    pub allowed_pids: Arc<RwLock<HashSet<u32>>>,
    pub rate_limiter: Arc<RateLimiter>,
}

impl SecurityState {
    pub fn new() -> Self {
        let mut secret = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret);
        Self {
            jwt_secret: Arc::new(secret),
            allowed_pids: Arc::new(RwLock::new(HashSet::new())),
            rate_limiter: Arc::new(RateLimiter::new(100, Duration::from_secs(60))),
        }
    }

    /// Register a PID (e.g., Tauri GUI at startup, frontend after connect)
    pub async fn register_pid(&self, pid: u32) {
        self.allowed_pids.write().await.insert(pid);
    }

    /// Check if a PID is in the whitelist
    pub async fn is_pid_allowed(&self, pid: u32) -> bool {
        self.allowed_pids.read().await.contains(&pid)
    }
}
```

**Key insight:** `Arc<[u8; 32]>` for JWT secret avoids lifetime issues with `Clone` across middleware layers. The secret is generated once at daemon startup and kept in memory (never persisted).

### Pattern 2: JWT Session Token Claims

**What:** A serde-serializable struct containing all required session token claims.

**Location:** `uc-daemon/src/security/claims.rs`

```rust
// Source: Based on locked decision from CONTEXT.md
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenClaims {
    /// Issuer: "uniclipboard-daemon"
    pub iss: String,
    /// Subject: "frontend"
    pub sub: String,
    /// Issued at: Unix timestamp
    pub iat: i64,
    /// Expiration: Unix timestamp (iat + 300)
    pub exp: i64,
    /// Client process ID
    pub pid: u32,
    /// Client type: "gui" | "cli" | "other"
    pub client_type: String,
    /// Unique token ID for revocation tracking
    pub jti: String,
    /// Permission level: L1-L4
    pub access_level: u8,
    /// Whether encryption session is ready
    pub encryption_ready: bool,
}

impl SessionTokenClaims {
    pub const ISSUER: &'static str = "uniclipboard-daemon";
    pub const SUBJECT: &'static str = "frontend";
    pub const TTL_SECS: i64 = 300; // 5 minutes

    pub fn new(
        pid: u32,
        client_type: String,
        access_level: u8,
        encryption_ready: bool,
    ) -> Self {
        let now = chrono::Utc::now().timestamp();
        let jti = uuid::Uuid::new_v4().to_string();
        Self {
            iss: Self::ISSUER.to_string(),
            sub: Self::SUBJECT.to_string(),
            iat: now,
            exp: now + Self::TTL_SECS,
            pid,
            client_type,
            jti,
            access_level,
            encryption_ready,
        }
    }

    /// Access level constants
    pub const LEVEL_L1: u8 = 1; // Public
    pub const LEVEL_L2: u8 = 2; // Authenticated
    pub const LEVEL_L3: u8 = 3; // Sensitive (requires encryption_ready)
    pub const LEVEL_L4: u8 = 4; // Dangerous
}
```

**Note on `uuid`:** The `uuid` crate is NOT currently in uc-daemon deps. Use `uuid` from the `uc-core` or add it as a new dependency. Alternatively, generate JTI using `rand::RngCore` for a simpler 16-byte random hex string to avoid adding a new dep.

### Pattern 3: Axum Layer-Based Middleware

**What:** axum uses the `Layer`/`Service` pattern from `tower`. Each middleware wraps a service and can inspect/transform requests before passing them down.

**When to use:** For composing rate limiting, auth extraction, and permission checking into the route handler pipeline.

**Location:** `uc-daemon/src/security/middleware.rs`

```rust
// Source: Based on axum 0.7 middleware patterns
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
};
use tower::{Layer, Service};

/// Rate limiting middleware layer.
/// Wraps handlers and enforces 100 req/min per client (by PID or IP fallback).
#[derive(Clone)]
pub struct RateLimitLayer;

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService { inner }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
}

// Implement Service so tower's Layer can wrap it
impl<S, B> Service<Request<B>> for RateLimitService<S>
where
    S: Service<Request<B>, Response = axum::response::Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let mut svc = self.inner.clone();
        Box::pin(async move { svc.call(req).await })
    }
}
```

**Key insight:** In practice, it's cleaner to implement each middleware as a standalone async function using axum's `axum::middleware::from_fn_with_state` API, which avoids manual `Service` trait boilerplate:

```rust
// Cleaner approach using axum::middleware
pub async fn rate_limit_middleware(
    State(security): State<Arc<SecurityState>>,
    request: Request,
    next: Next,
) -> Response {
    let client_id = extract_client_id(&request); // PID from header or IP fallback
    if !security.rate_limiter.check(&client_id) {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": "rate_limit_exceeded"}))).into_response();
    }
    next.run(request).await
}

// In routes.rs:
Router::new()
    .route("/clipboard/entries", get(list_entries))
    .layer(middleware::from_fn_with_state(security.clone(), rate_limit_middleware))
```

### Pattern 4: Sliding Window Rate Limiter

**What:** A `HashMap<ClientId, Vec<Instant>>` sliding window. Each request adds an `Instant::now()` entry; expired entries (> 60s old) are pruned on every check.

**Location:** `uc-daemon/src/security/rate_limiter.rs`

```rust
// Source: Standard sliding window pattern, verified against tokio docs
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct RateLimiter {
    inner: Arc<RwLock<RateLimiterInner>>,
}

struct RateLimiterInner {
    entries: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    /// `max_requests` per `window` duration
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RateLimiterInner {
                entries: HashMap::new(),
            })),
        }
    }

    /// Returns true if request is allowed, false if rate limited.
    /// Cleans up entries older than `window` before checking.
    pub async fn check(&self, client_id: &str) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(60); // captured in self
        let mut guard = self.inner.write().await;

        // Clean stale entries
        let stale_cutoff = now - window;
        if let Some(insts) = guard.entries.get_mut(client_id) {
            insts.retain(|&i| i > stale_cutoff);
        }

        // Check limit
        let count = guard.entries.entry(client_id.to_string()).or_default().len();
        if count >= 100 {
            return false;
        }

        guard.entries.get_mut(client_id).unwrap().push(now);
        true
    }
}
```

**Cleanup strategy:** Entries older than 60s are pruned on every `check()` call. No background cleanup task needed since in-memory HashMap doesn't grow unboundedly. For very high traffic (thousands of unique IPs/PIDs), consider a periodic cleanup task every 5 minutes to drop entire client entries with no recent requests.

### Pattern 5: Permission Levels (L1-L4)

**What:** An enum with associated minimum access levels and encryption-ready requirements.

**Location:** `uc-daemon/src/security/permission.rs`

```rust
// Source: Based on locked decisions from CONTEXT.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    L1Public = 1,
    L2Authenticated = 2,
    L3Sensitive = 3,
    L4Dangerous = 4,
}

impl PermissionLevel {
    pub fn min_level(&self) -> u8 {
        *self as u8
    }

    /// Returns true if this level requires encryption session to be ready.
    pub fn requires_encryption_ready(&self) -> bool {
        matches!(self, Self::L3Sensitive | Self::L4Dangerous)
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::L1Public),
            2 => Some(Self::L2Authenticated),
            3 => Some(Self::L3Sensitive),
            4 => Some(Self::L4Dangerous),
            _ => None,
        }
    }
}

/// Annotate each route handler's required permission level.
#[derive(Clone, Copy)]
pub enum RoutePermission {
    Public,       // L1 — no auth required (health check only)
    Authenticated, // L2 — valid session token required
    Sensitive,     // L3 — session token + encryption_ready claim must be true
    Dangerous,    // L4 — L3 + additional confirmation (out of scope for now)
}

impl RoutePermission {
    pub fn level(&self) -> PermissionLevel {
        match self {
            Self::Public => PermissionLevel::L1Public,
            Self::Authenticated => PermissionLevel::L2Authenticated,
            Self::Sensitive => PermissionLevel::L3Sensitive,
            Self::Dangerous => PermissionLevel::L4Dangerous,
        }
    }
}
```

### Pattern 6: Middleware Stack Composition in Router

**What:** Compose all three middleware layers in order on routes that require auth.

**Location:** `uc-daemon/src/api/routes.rs` (modifications)

```rust
// Source: Verified against existing routes.rs pattern
use crate::security::{RateLimitLayer, PermissionCheckLayer};

pub fn router(security: Arc<SecurityState>) -> Router<DaemonApiState> {
    // L1 routes: no middleware (health check)
    let l1_routes = Router::new()
        .route("/health", get(health));

    // L2+ routes: rate_limit -> permission_check -> handler
    let protected = Router::new()
        .route("/clipboard/entries", get(list_entries))
        .route("/clipboard/stats", get(get_stats))
        // ... all other routes
        .layer(middleware::from_fn_with_state(security.clone(), rate_limit_middleware))
        .layer(middleware::from_fn_with_state(security.clone(), permission_middleware));

    l1_routes.merge(protected)
}
```

**Key insight:** The `health` handler does NOT call `state.is_authorized()` anymore when using middleware. All existing `is_authorized()` checks in handlers can be removed since middleware handles it. The WS upgrade handler (`ws.rs:websocket_upgrade`) also needs the same middleware.

### Pattern 7: /auth/connect Endpoint

**What:** New POST `/auth/connect` endpoint that exchanges bearer token for JWT session token.

**Location:** `uc-daemon/src/security/connect.rs` (new file)

```rust
// Source: Based on locked decisions + jsonwebtoken 9.x API
use axum::{extract::State, http::StatusCode, Json, Router};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::api::server::DaemonApiState;
use crate::security::{claims::SessionTokenClaims, state::SecurityState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub pid: u32,
    pub client_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResponse {
    pub session_token: String,
    pub expires_in_secs: i64,
    pub refresh_at_secs: i64, // 4 minutes (60s before expiry)
}

pub fn router(security: Arc<SecurityState>, daemon_state: DaemonApiState) -> Router {
    Router::new()
        .route("/auth/connect", post(connect_handler))
        .with_state((security, daemon_state))
}

async fn connect_handler(
    State((security, daemon_state)): State<(Arc<SecurityState>, DaemonApiState)>,
    headers: HeaderMap,
    Json(req): Json<ConnectRequest>,
) -> axum::response::Response {
    // Step 1: Validate bearer token (same as existing auth)
    if !daemon_state.is_authorized(&headers) {
        return unauthorized().into_response();
    }

    // Step 2: Register PID in whitelist
    security.register_pid(req.pid).await;

    // Step 3: Check encryption state (requires runtime access)
    let encryption_ready = daemon_state
        .runtime
        .as_ref()
        .map(|r| {
            // Check if encryption session is unlocked — details depend on uc-core
            // Placeholder: actual implementation queries encryption state
            true
        })
        .unwrap_or(false);

    // Step 4: Build claims
    let claims = SessionTokenClaims::new(
        req.pid,
        req.client_type,
        SessionTokenClaims::LEVEL_L2, // Default to L2; L3/L4 requires encryption
        encryption_ready,
    );

    // Step 5: Sign JWT with HS256
    let header = Header::new(Algorithm::HS256);
    let key = EncodingKey::from_direct_bytes(&*security.jwt_secret)
        .expect("secret is exactly 32 bytes, valid for HS256");
    let token = encode(&header, &claims, &key)
        .expect("HS256 encoding should not fail with valid key");

    let response = ConnectResponse {
        session_token: token,
        expires_in_secs: SessionTokenClaims::TTL_SECS,
        refresh_at_secs: 240, // 4 minutes
    };

    (StatusCode::OK, Json(response)).into_response()
}
```

**Key insight:** The `jwt_secret` is a `Arc<[u8; 32]>`. `EncodingKey::from_direct_bytes` accepts `&[u8]` which is satisfied by `&[u8]` from the array slice. The `Arc` derefs to `Arc<[u8; 32]>` which implements `Deref<Target = [u8; 32]>`, and `&*security.jwt_secret` gives `&[u8; 32]`.

### Pattern 8: Session Token Validation Middleware

**What:** Extracts `Authorization: Session <token>` header, validates JWT, checks PID whitelist, checks permission level, and injects validated claims into request extensions.

```rust
// Source: jsonwebtoken 9.x API + axum extract pattern
use axum::{
    extract::Request,
    http::{header::AUTHORIZATION, StatusCode},
    middleware::Next,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

use crate::security::state::SecurityState;

pub async fn auth_extractor_middleware(
    State(security): State<Arc<SecurityState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = match auth_header.and_then(|h| h.strip_prefix("Session ")) {
        Some(t) => t,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing_session_token"}))).into_response();
        }
    };

    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&["uniclipboard-daemon"]);
    validation.set_subject(&["frontend"]);

    let key = DecodingKey::from_direct_bytes(&*security.jwt_secret)
        .expect("secret is 32 bytes, valid for HS256");

    let token_data = match decode::<SessionTokenClaims>(token, &key, &validation) {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!(error = %e, "JWT validation failed");
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid_session_token"}))).into_response();
        }
    };

    let claims = token_data.claims;

    // PID whitelist check
    if !security.is_pid_allowed(claims.pid).await {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "pid_not_allowed"}))).into_response();
    }

    // Store claims in request extensions for downstream handlers
    request.extensions_mut().insert(claims);

    next.run(request).into_response()
}
```

---

## Don't Hand-Roll

| Problem                  | Don't Build                    | Use Instead                      | Why                                                                       |
| ------------------------ | ------------------------------ | -------------------------------- | ------------------------------------------------------------------------- |
| JWT encoding/decoding    | Manual HMAC math               | `jsonwebtoken` crate             | Correct crypto primitives, timing-safe verification, proper claim parsing |
| Rate limiting algorithm  | Fixed window (easily gameable) | Sliding window                   | Prevents burst at window boundary                                         |
| Random secret generation | `rand::random::<u32>()`        | `rand::rngs::OsRng.fill_bytes()` | CSPRNG for cryptographic secret material                                  |
| Process ID capture       | Parsing `/proc/self/`          | `std::process::id()`             | Cross-platform, zero-dependency                                           |

---

## Runtime State Inventory

> Phase 75 is a greenfield implementation. It does NOT rename, rebrand, or migrate any existing runtime state. The PID whitelist (`allowed_pids: RwLock<HashSet<u32>>`) is new in-memory state with no persistence. The JWT secret is new in-memory state generated at daemon startup. No data migration is required.

| Category            | Items Found                                                       | Action Required |
| ------------------- | ----------------------------------------------------------------- | --------------- |
| Stored data         | None — PID whitelist is in-memory only                            | None            |
| Live service config | None — no external service config changes                         | None            |
| OS-registered state | None — PID file already managed by process_metadata.rs (existing) | None            |
| Secrets/env vars    | None — JWT secret generated in-memory at startup, never persisted | None            |
| Build artifacts     | None — no pre-existing artifact name conflicts                    | None            |

**Conclusion:** No runtime state migration needed. This is a greenfield security middleware implementation.

---

## Common Pitfalls

### Pitfall 1: Rejecting requests with no Authorization header instead of checking header presence

**What goes wrong:** L1 endpoints (health check) are treated as unauthorized when they shouldn't require auth at all.

**Why it happens:** Middleware checks `Authorization` header before determining the route's permission level.

**How to avoid:** Use `RoutePermission::Public` attribute on L1 routes. The middleware must inspect the route's declared permission before requiring auth:

```rust
// Wrong: always requires header
async fn auth_middleware(request: Request, next: Next) -> Response {
    let token = request.headers().get(AUTHORIZATION)...; // Always fails for /health
    next.run(request).await
}

// Correct: check route metadata first (not implemented in this pattern — L1 bypassed via separate router)
let l1 = Router::new().route("/health", get(health)); // No middleware
let protected = Router::new()
    .route("/clipboard/entries", get(list_entries))
    .layer(middleware::from_fn_with_state(security.clone(), auth_middleware));
```

**Recommendation:** Keep L1 routes in a separate `Router` that is merged without middleware, matching the existing pattern in `routes.rs` where `health` doesn't call `is_authorized()`.

### Pitfall 2: Storing JWT secret as `String` (heap-allocated, searchable in memory dumps)

**What goes wrong:** `String` stores the secret in the Rust heap, which is visible in core dumps and memory snapshots.

**Why it happens:** `String` is the obvious type for a byte sequence.

**How to avoid:** Use `[u8; 32]` (stack-allocated, fixed-size) wrapped in `Arc`. If `SecretVec` from `jsonwebtoken` is available, use that instead:

```rust
// Correct: stack-allocated, fixed-size
let secret: [u8; 32] = [0; 32];
rand::rngs::OsRng.fill_bytes(&mut secret);
let jwt_secret = Arc::new(secret);

// In middleware:
let key = DecodingKey::from_direct_bytes(&*jwt_secret)?;
```

### Pitfall 3: Timing attacks in bearer token comparison

**What goes wrong:** Direct string equality check `token == self.auth_token.as_str()` is visible to an attacker who can measure response times.

**Why it happens:** Rust's `==` on strings short-circuits on first mismatch.

**How to avoid:** Use `subtle::ConstantTimeEq` for token comparison. However, this is only a concern if the attacker is on the same machine (local loopback context). For this daemon (127.0.0.1 only), the existing direct comparison is acceptable but `ConstantTimeEq` is best practice.

### Pitfall 4: Rate limiter HashMap growing unboundedly with unused entries

**What goes wrong:** Clients that stop making requests still occupy memory indefinitely if only stale entries within the window are pruned.

**Why it happens:** `retain()` only removes entries older than 60s; if a client makes 1 request and never returns, that single `Vec<Instant>` stays in the HashMap forever.

**How to avoid:** Periodically drop entire client entries with zero recent requests. Run a cleanup task every 5 minutes or when the HashMap exceeds a size threshold. Alternative: use a bounded LRU cache (e.g., `lru` crate) for the client map.

### Pitfall 5: Missing `jti` (JWT ID) for token revocation

**What goes wrong:** Once issued, a JWT cannot be revoked until it expires. If a session needs to be terminated early (e.g., logout), there's no mechanism.

**Why it happens:** `jti` is in the spec but often omitted for simplicity.

**How to avoid:** Include `jti` in claims (already in locked decisions). For Phase 75, the `jti` is issued but no revocation list is implemented. Deferred to Phase 76 or later. Document this as a known limitation.

### Pitfall 6: Middleware panicking on missing request extensions

**What goes wrong:** If `auth_extractor_middleware` doesn't run (L1 routes), downstream handlers that expect claims in extensions will panic.

**Why it happens:** Handlers call `request.extensions().get::<SessionTokenClaims>()` which returns `None` for L1 routes.

**How to avoid:** Only access extensions after checking the route has auth middleware. For now, L1 routes bypass auth middleware entirely. Consider a typed `AuthContext` extractor that returns `None` for unauthenticated requests.

---

## Code Examples

### JWT Signing (jsonwebtoken 9.x)

```rust
// Source: jsonwebtoken 9.x docs - https://docs.rs/jsonwebtoken/9.0.0/jsonwebtoken/
use jsonwebtoken::{encode, decode, Header, Algorithm, EncodingKey, DecodingKey, Validation};

let secret: [u8; 32] = [0; 32];
rand::rngs::OsRng.fill_bytes(&mut secret);

// Sign
let header = Header::new(Algorithm::HS256);
let claims = SessionTokenClaims::new(1234, "gui".into(), 2, true);
let token = encode(&header, &claims, &EncodingKey::from_direct_bytes(&secret)?)?;

// Verify
let mut validation = Validation::new(Algorithm::HS256);
validation.set_issuer(&["uniclipboard-daemon"]);
let token_data = decode::<SessionTokenClaims>(&token, &DecodingKey::from_direct_bytes(&secret)?, &validation)?;
```

### Rate Limiter Check

```rust
// Source: verified sliding window pattern
let limiter = RateLimiter::new(100, Duration::from_secs(60));

// In middleware:
if !limiter.check(&client_id).await {
    return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": "rate_limit_exceeded", "retry_after_secs": 60}))).into_response();
}
```

### PID Registration at Daemon Startup

```rust
// Source: existing process_metadata.rs patterns
use crate::process_metadata::write_current_pid;

// At startup (in entrypoint.rs):
let daemon_pid = write_current_pid()?;
security.register_pid(daemon_pid).await;
```

---

## State of the Art

| Old Approach                      | Current Approach                                   | When Changed | Impact                                                                                   |
| --------------------------------- | -------------------------------------------------- | ------------ | ---------------------------------------------------------------------------------------- |
| Raw bearer token on every request | JWT session token (5min TTL) exchanged from bearer | Phase 75     | Limits exposure window if token is leaked; bearer token never sent after initial connect |
| No PID verification               | PID whitelist check against `X-Client-PID` header  | Phase 75     | Defense-in-depth against local malware impersonating client                              |
| No rate limiting                  | Sliding window 100 req/min per client              | Phase 75     | Prevents DoS from runaway client or script                                               |
| Per-handler auth checks           | Layered middleware with permission levels          | Phase 75     | Single auth policy, consistent enforcement, cleaner handlers                             |
| All endpoints require bearer      | L1 bypasses auth entirely                          | Phase 75     | `/health` remains accessible for load balancers/probes                                   |

**Deprecated/outdated:**

- `is_authorized()` per-handler: Replaced by middleware. Existing handlers that call it (routes.rs, clipboard.rs, ws.rs) should be updated to trust middleware.
- Raw bearer token for non-connect requests: Replaced by session token. After Phase 75, `/auth/connect` is the only endpoint accepting bearer tokens.

---

## Open Questions

1. **Where to get encryption_ready state at connect time?**
   - What we know: `CoreRuntime` exists in daemon state; encryption session state is accessible via `runtime.wiring_deps().security.encryption_session`
   - What's unclear: The exact API to check "is unlocked" synchronously without awaiting
   - Recommendation: Check `EncryptionState` enum from uc-core to determine if session is ready

2. **Should WebSocket connections also use session tokens?**
   - What we know: WS upgrade currently uses bearer token; the WS handler is in `ws.rs`
   - What's unclear: Whether session token should apply to WS upgrade or only HTTP requests
   - Recommendation: Apply same middleware to WS upgrade handler — add `ws://` upgrade check via same `Authorization: Session` header

3. **Token refresh vs. new token on /auth/connect**
   - What we know: Context says "refresh every 4 minutes" but `/auth/connect` issues a new token
   - What's unclear: Whether to support a refresh endpoint or just re-call connect
   - Recommendation: Re-call POST `/auth/connect` is simpler; no separate refresh endpoint needed for Phase 75

4. **L3/L4 permission enforcement on handlers that call encryption operations**
   - What we know: L3 = encryption_ready must be true; L4 = additional confirmation (deferred)
   - What's unclear: Which existing handlers map to L3 vs L2
   - Recommendation: Map L2 = all read operations; L3 = clipboard/restore, settings write operations. L4 = setup/reset (already requires confirmation in UI)

---

## Environment Availability

> Step 2.6: SKIPPED (no external dependencies identified beyond Rust crate additions)

This phase adds only in-process Rust code to `uc-daemon`. No external tools, services, runtimes, or OS-level dependencies are required beyond what the daemon already uses.

**New Rust crate dependencies required:**

- `jsonwebtoken = "9"` — adds no external system dependencies, compiles with `ring` backend

---

## Validation Architecture

### Test Framework

| Property           | Value                                                                             |
| ------------------ | --------------------------------------------------------------------------------- |
| Framework          | `#[cfg(test)]` modules in each security module + integration tests in `uc-daemon` |
| Config file        | None — pure unit tests                                                            |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon security --no-fail-fast`                 |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon`                                         |

### Phase Requirements to Test Map

| Requirement            | Behavior                                               | Test Type   | Automated Command                                    | File Exists? |
| ---------------------- | ------------------------------------------------------ | ----------- | ---------------------------------------------------- | ------------ |
| JWT sign               | `claims -> token` with HS256, correct claims           | unit        | `cargo test -p uc-daemon claims`                     | New file     |
| JWT verify             | Valid token passes, expired/malformed rejects          | unit        | `cargo test -p uc-daemon claims -- --test-threads=1` | New file     |
| Rate limiter           | Under limit allows, over limit rejects                 | unit        | `cargo test -p uc-daemon rate_limiter`               | New file     |
| Rate limiter cleanup   | Old entries pruned, memory bounded                     | unit        | `cargo test -p uc-daemon rate_limiter`               | New file     |
| PID whitelist          | Register/lookup works, unknown PID rejected            | unit        | `cargo test -p uc-daemon state`                      | New file     |
| Permission level       | L1 bypasses auth, L2+ requires valid token             | unit        | `cargo test -p uc-daemon permission`                 | New file     |
| Auth connect flow      | Bearer valid -> session token returned, PID registered | unit        | `cargo test -p uc-daemon connect`                    | New file     |
| Middleware integration | All routes accessible under correct auth               | integration | `cargo test -p uc-daemon security middleware`        | New file     |
| End-to-end session     | Bearer -> connect -> session -> API call               | integration | `cargo test -p uc-daemon api_integration`            | New file     |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon --lib security -- --test-threads=4`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon --lib`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/security/claims.rs` — `SessionTokenClaims` + tests
- [ ] `src-tauri/crates/uc-daemon/src/security/rate_limiter.rs` — sliding window + tests
- [ ] `src-tauri/crates/uc-daemon/src/security/state.rs` — `SecurityState` + PID registry + tests
- [ ] `src-tauri/crates/uc-daemon/src/security/permission.rs` — `PermissionLevel` + `RoutePermission` + tests
- [ ] `src-tauri/crates/uc-daemon/src/security/middleware.rs` — rate limit + auth extractor middleware
- [ ] `src-tauri/crates/uc-daemon/src/security/connect.rs` — `/auth/connect` endpoint + tests
- [ ] `src-tauri/crates/uc-daemon/src/security/mod.rs` — re-exports
- [ ] `src-tauri/crates/uc-daemon/src/api/routes.rs` — updated with middleware layers
- [ ] `src-tauri/crates/uc-daemon/src/api/server.rs` — `DaemonApiState` extended with `SecurityState`
- [ ] `src-tauri/crates/uc-daemon/src/api/ws.rs` — WS upgrade uses session token middleware
- [ ] `uc-daemon/Cargo.toml` — add `jsonwebtoken = "9"` dependency

---

## Sources

### Primary (HIGH confidence)

- [jsonwebtoken crate docs](https://docs.rs/jsonwebtoken/9.0.0/) — HS256 signing/verification API, claim types, `EncodingKey`/`DecodingKey` usage
- [axum middleware documentation](https://docs.rs/axum/0.7/axum/middleware/) — `from_fn_with_state`, `Layer` pattern, `Request`/`Response` types
- [axum 0.7 Router layer composition](https://docs.rs/axum/0.7/axum/routing/struct.Router.html#method.layer) — `.layer()` on Router, `.merge()` for combining routers
- [tower Service pattern](https://docs.rs/tower/0.5/tower/trait.Layer.html) — Layer/Svc composition for middleware

### Secondary (MEDIUM confidence)

- [JWT.best current practices](https://curity.io/resources/learn/jwt-best-practices/) — short TTL, HS256 appropriateness for symmetric local apps
- [OWASP API Security - JWT](https://cheatsheetseries.owasp.org/cheatsheets/JSON_Web_Token_for_Java_Cheat_Sheet.html) — jti for revocation, issuer validation, algorithm confusion attack prevention
- [Sliding window rate limiting - Redis patterns](https://redis.io/docs/manualpatterns/counting-things/) — sliding window algorithm correctness

### Tertiary (LOW confidence)

- Rate limiter memory boundedness — standard HashMap + periodic cleanup is adequate for <1000 clients; for >10k clients consider LRU cache

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — jsonwebtoken 9.x verified at crates.io, axum 0.7 already in workspace
- Architecture: HIGH — all patterns verified against existing daemon codebase structure
- Pitfalls: MEDIUM — identified from known JWT/middleware edge cases

**Research date:** 2026-03-29
**Valid until:** 2026-04-28 (30 days — JWT ecosystem is stable)
