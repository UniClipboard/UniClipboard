# Phase 76: Daemon Settings, Encryption & Storage HTTP API - Research

**Researched:** 2026-03-30
**Domain:** Daemon HTTP API extension — settings, encryption state, storage stats
**Confidence:** HIGH

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Settings API:**

- GET `/settings` — return all settings (general, sync, security, privacy), permission L2
- PUT `/settings` — update settings, permission L3 (modifying settings is sensitive)
- Response type: SettingsResponse { general, sync, security, privacy }

**Encryption API:**

- GET `/encryption/state` — return current encryption state, permission L2
- POST `/encryption/unlock` — unlock encryption session with passphrase, permission L3
- POST `/encryption/lock` — lock encryption session, permission L3
- EncryptionStateResponse: { state: "uninitialized"|"locked"|"unlocked", has_keyslot, requires_passphrase }
- On successful unlock: broadcast WS event `encryption.session-ready`

**Storage API:**

- GET `/storage/stats` — return storage statistics, permission L2
- POST `/storage/clear-cache` — clear cache (requires confirmation), permission L4 (dangerous)
- StorageStatsResponse: { total_size_bytes, blob_count, database_size_bytes, cache_size_bytes, spool_size_bytes }
- ClearCache requires `{ confirmed: true }` in request body

**Permission Levels:**

- L2: settings read, encryption state read, storage stats
- L3: settings write, encryption unlock/lock
- L4: clear cache (dangerous, requires confirmation)

### Claude's Discretion

- How to map existing uc-app settings use cases to HTTP handlers
- Settings update request type granularity (full replacement vs partial patch)
- Whether encryption unlock needs additional rate limiting beyond global rate limiter
- Storage stats calculation approach (synchronous vs background)

### Deferred Ideas (OUT OF SCOPE)

- Per-field settings validation (complex business rules)
- Settings change history/audit trail
- Encryption key rotation API
- Storage quota enforcement API
  </user_constraints>

---

## Summary

Phase 76 adds three new API modules to the daemon HTTP server: settings read/write, encryption state management, and storage statistics/cache control. This phase is the first to require L3/L4 permission enforcement, which Phase 75 explicitly deferred. The permission system already has `PermissionLevel` enum defined (L1, L2) but L3 and L4 are not yet implemented — this phase must add them.

The implementation pattern is clear from Phase 74 (clipboard API): create a new `src/api/` submodule (e.g., `settings.rs`, `encryption.rs`, `storage.rs`) that follows the same handler structure as `clipboard.rs`. Use `CoreUseCases::new(runtime.as_ref())` pattern to access use cases. Register routes in `router_l2_plus()` in `routes.rs`.

The settings and storage use cases already exist in `uc-app` and are used by Tauri commands. Encryption unlock via passphrase requires a new use case — `AutoUnlockEncryptionSession` uses keyring (no passphrase), `InitializeEncryption` sets up a new passphrase. A new `UnlockEncryptionSessionWithPassphrase` use case must be built following the same pattern (load keyslot, derive KEK from passphrase, unwrap master key, set in session).

**Primary recommendation:** Follow the `clipboard.rs` module pattern exactly. Add L3/L4 to `permission.rs`, implement per-route permission middleware, and create three new handler modules. The encryption unlock use case is the only net-new domain logic.

---

## Standard Stack

### Core (existing — no new dependencies needed)

| Library               | Version   | Purpose                                   | Why Standard                                             |
| --------------------- | --------- | ----------------------------------------- | -------------------------------------------------------- |
| axum                  | workspace | HTTP routing and handlers                 | Already used by daemon server                            |
| serde / serde_json    | workspace | DTO serialization                         | Consistent with all other API DTOs                       |
| uc-app (CoreUseCases) | workspace | Settings, storage, encryption use cases   | Hexagonal architecture — all business logic in use cases |
| uc-core               | workspace | Domain models (Settings, EncryptionState) | Settings model already has Serialize/Deserialize         |
| tokio                 | workspace | Async runtime                             | Already used throughout                                  |
| tracing               | workspace | Structured logging                        | Required per CLAUDE.md                                   |

### Installation

No new dependencies. All required crates are already in the workspace.

---

## Architecture Patterns

### Route Module Pattern (from Phase 74)

Each new API domain gets its own file: `src/api/settings.rs`, `src/api/encryption.rs`, `src/api/storage.rs`. Each exposes a `pub fn router() -> Router<DaemonApiState>` that is `.merge()`d into `router_l2_plus()` in `routes.rs`.

```rust
// Source: src-tauri/crates/uc-daemon/src/api/clipboard.rs
pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))
}
```

### Handler Pattern (from clipboard.rs and routes.rs)

```rust
// Source: src-tauri/crates/uc-daemon/src/api/clipboard.rs
async fn get_settings_handler(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.get_settings().execute().await {
        Ok(settings) => Json(settings).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}
```

### L3/L4 Permission Enforcement Pattern

Phase 75 defined `PermissionLevel` enum with only `L1Public` and `L2Authenticated`. Phase 76 must extend this enum and implement enforcement. Two approaches are viable:

**Option A (Recommended): Per-route middleware using `from_fn_with_state`**
Create a separate `router_l3_plus` and `router_l4` (analogous to `router_l2_plus`) that add an extra middleware layer after auth extraction. Each layer checks the permission level before forwarding to the handler.

**Option B: Handler-level permission check**
Each L3/L4 handler calls a helper like `require_permission(L3, &state, &extensions)?` at the top. Simpler to implement, slightly less centralized.

Per the CONTEXT.md decision to use "permission L3/L4", Option A (sub-router pattern) is cleaner — it matches the L2 pattern exactly and enforces at the routing layer rather than handler level.

```rust
// Extended PermissionLevel in permission.rs
pub enum PermissionLevel {
    L1Public = 1,
    L2Authenticated = 2,
    L3Sensitive = 3,    // NEW: requires encryption initialized
    L4Dangerous = 4,    // NEW: requires explicit confirmation field
}
```

### Encryption State Mapping

The daemon tracks encryption state via two orthogonal properties:

- `EncryptionState` (from `EncryptionStatePort`): `Uninitialized` | `Initialized`
- `EncryptionSessionPort::is_ready()`: whether master key is loaded in memory

These map to the CONTEXT.md wire format:

| EncryptionState | is_ready() | Wire value      | has_keyslot | requires_passphrase |
| --------------- | ---------- | --------------- | ----------- | ------------------- |
| Uninitialized   | false      | "uninitialized" | false       | false               |
| Initialized     | false      | "locked"        | true        | true                |
| Initialized     | true       | "unlocked"      | true        | false               |

### Unlock With Passphrase — New Use Case

No existing use case does passphrase-based unlock without initialization. The new use case `UnlockEncryptionSessionWithPassphrase` must:

1. Load `EncryptionState` — if `Uninitialized`, return error (can't unlock uninitialized)
2. Get `KeyScope` from `KeyScopePort`
3. Load `KeySlot` from `KeyMaterialPort::load_keyslot()`
4. Derive `KEK` from passphrase using `EncryptionPort::derive_kek()` with KDF params from keyslot
5. Unwrap master key using `EncryptionPort::unwrap_master_key()`
6. Set master key via `EncryptionSessionPort::set_master_key()`

This is a subset of `InitializeEncryption::execute()` — specifically steps 2-6 of the initialization flow without generating new keys or persisting state.

### Lock Encryption Session

Lock is simpler: call `EncryptionSessionPort::clear()` to zeroize the in-memory master key.

### WS Event on Unlock

On successful unlock, broadcast `encryption.session-ready` event to connected WS clients via `state.event_tx`. The existing `DaemonApiEventEmitter` / `event_emitter.rs` handles WS broadcast. The new event constant must be added to `daemon_api_strings::ws_event`.

### StorageStatsResponse Field Mapping

CONTEXT.md specifies: `{ total_size_bytes, blob_count, database_size_bytes, cache_size_bytes, spool_size_bytes }`

The existing `StorageStatsResult` from `GetStorageStats` has: `{ database_bytes, vault_bytes, cache_bytes, logs_bytes, total_bytes, data_dir }`.

Mapping:

- `total_size_bytes` ← `total_bytes`
- `database_size_bytes` ← `database_bytes`
- `cache_size_bytes` ← `cache_bytes`
- `spool_size_bytes` ← computed separately from `spool_dir` (currently in `AppPaths` but NOT in `GetStorageStats` — see pitfall below)
- `blob_count` — NOT in `GetStorageStats` (must query `ClipboardEntryRepositoryPort` or count blobs in vault_dir)

The DTO for daemon response is a new struct `StorageStatsResponse` (daemon-transport layer), not re-using `StorageStatsResult` directly, to match the CONTEXT.md wire shape.

### Settings Update — Full Replacement vs Partial Patch

The existing `UpdateSettings::execute(Settings)` takes a full `Settings` struct. For the HTTP API, a PUT with full replacement is the simplest approach (matching Tauri's `update_settings` command which also takes a full `Settings` value).

The Tauri `update_settings` handler does several OS-level side effects (autostart, keyboard shortcut re-registration, device name announcement). The daemon handler should NOT perform these platform-level effects — it should only call `UpdateSettings::execute()`. OS-level effects are Tauri-specific concerns.

One concern: schema version validation. `UpdateSettings::execute()` validates `schema_version == CURRENT_SCHEMA_VERSION`. The HTTP PUT body from the frontend should include the current schema version.

---

## Don't Hand-Roll

| Problem                  | Don't Build                   | Use Instead                                             | Why                                       |
| ------------------------ | ----------------------------- | ------------------------------------------------------- | ----------------------------------------- |
| Settings persistence     | Custom settings store         | `CoreUseCases::get_settings()` / `update_settings()`    | Already wired with `SettingsPort` adapter |
| Storage size computation | `std::fs::metadata` recursion | `CoreUseCases::get_storage_stats()` and `clear_cache()` | Handles missing dirs gracefully           |
| Encryption key loading   | Manual key unwrap             | `AutoUnlockEncryptionSession` pattern (adapted)         | Covers all error cases                    |
| HTTP error formatting    | Custom error structs          | `internal_error()` helper from `routes.rs`              | Consistent error format across all routes |
| WS broadcast             | Direct channel clone          | `state.event_tx.send()`                                 | Already-wired broadcast channel           |

---

## Common Pitfalls

### Pitfall 1: spool_size_bytes Not in GetStorageStats

**What goes wrong:** `GetStorageStats::execute()` does not include `spool_dir` size. The CONTEXT.md response includes `spool_size_bytes`.

**Why it happens:** `GetStorageStats` was designed for the Tauri storage panel which didn't need spool details. `AppPaths` has a `spool_dir` field but it's not measured.

**How to avoid:** Either extend `GetStorageStats` to include `spool_dir` (preferred — keeps storage logic in one place), or compute it separately in the daemon handler using `CacheFsPort::dir_size(&paths.spool_dir)`. The daemon API state has access to `runtime.storage_paths` via `CoreRuntime`.

### Pitfall 2: blob_count Not in GetStorageStats

**What goes wrong:** `StorageStatsResult` does not include blob count. CONTEXT.md response shape includes `blob_count`.

**Why it happens:** The Tauri UI never needed a blob count. `GetStorageStats` computes only directory sizes.

**How to avoid:** `blob_count` can be derived from `ClipboardEntryRepositoryPort::list_entries(limit=10000, offset=0)` and taking `.len()`. A simpler option: use `vault_dir` file count via `CacheFsPort::read_dir()`. Most precise: add a `count_blobs()` method to `BlobRepositoryPort`. For Phase 76, compute it from `list_entries` count (matches existing pattern in clipboard.rs stats endpoint).

### Pitfall 3: L3/L4 Not Yet Enforced

**What goes wrong:** If Phase 76 adds L3/L4 routes without implementing the middleware enforcement, all L3/L4 routes silently degrade to L2 (authenticated only), allowing any authenticated client to call dangerous L4 operations.

**Why it happens:** Phase 75 explicitly deferred L3/L4 — the enum has only L1 and L2.

**How to avoid:** Phase 76 MUST implement L3 and L4 enforcement in `permission.rs` and the middleware chain. The middleware extension must be added before registering any L3/L4 routes.

**L3 enforcement:** Check `runtime.is_encryption_ready()` — L3 routes should only proceed if encryption session is unlocked.

**L4 enforcement:** Check request body for `{ confirmed: true }` field. Return 400 with `{ code: "confirmation_required" }` if absent or false. This is the "clear-cache requires confirmation" pattern from CONTEXT.md.

### Pitfall 4: Settings Update Side Effects in Daemon Context

**What goes wrong:** Copy-pasting the Tauri `update_settings` command into the daemon handler and bringing along OS-level effects (autostart registration, keyboard shortcut re-registration).

**Why it happens:** The Tauri command has several post-save side effects that are Tauri-specific.

**How to avoid:** The daemon handler calls ONLY `CoreUseCases::update_settings().execute(parsed_settings)`. No autostart, no keyboard shortcuts, no device name announcement. These are Tauri concerns that operate through `AppRuntime` / `AppHandle`, not `CoreRuntime`.

### Pitfall 5: Encryption Unlock Passphrase in Logs

**What goes wrong:** Logging the passphrase string accidentally (e.g., `tracing::debug!("Unlock request: {:?}", payload)` where payload contains the passphrase).

**Why it happens:** Developer debug logging.

**How to avoid:** The request DTO for `/encryption/unlock` must NOT derive `Debug` or must redact the passphrase field. Log only the operation result, not the payload.

### Pitfall 6: WS Event Type String for encryption.session-ready

**What goes wrong:** Using a hardcoded string `"encryption.session-ready"` directly in the handler instead of a constant in `daemon_api_strings`.

**Why it happens:** New feature, easy to forget the constant convention.

**How to avoid:** Add `ws_event::ENCRYPTION_SESSION_READY = "encryption.session-ready"` to `uc-core/src/network/daemon_api_strings.rs` with a value assertion test (PH561 pattern).

---

## Code Examples

### Route Registration Pattern (from routes.rs)

```rust
// Source: src-tauri/crates/uc-daemon/src/api/routes.rs
pub fn router_l2_plus(state: DaemonApiState) -> Router<DaemonApiState> {
    let router = Router::new()
        .merge(crate::api::clipboard::router())
        .merge(crate::api::settings::router())      // NEW
        .merge(crate::api::encryption::router())     // NEW
        .merge(crate::api::storage::router())        // NEW
        // ... existing routes
```

### Settings GET Handler

```rust
// Pattern from src-tauri/crates/uc-daemon/src/api/clipboard.rs
async fn get_settings_handler(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.get_settings().execute().await {
        Ok(settings) => {
            let ts = chrono::Utc::now().timestamp_millis();
            Json(json!({ "data": settings, "ts": ts })).into_response()
        }
        Err(e) => internal_error(e).into_response(),
    }
}
```

### Encryption State Handler

```rust
// EncryptionState from uc-core::security::state::EncryptionState
// is_ready from CoreRuntime::is_encryption_ready()
async fn get_encryption_state_handler(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let enc_state = runtime.encryption_state().await;
    let session_ready = runtime.is_encryption_ready().await;

    let (state_str, has_keyslot, requires_passphrase) = match enc_state {
        Ok(EncryptionState::Uninitialized) => ("uninitialized", false, false),
        Ok(EncryptionState::Initialized) if session_ready => ("unlocked", true, false),
        Ok(EncryptionState::Initialized) => ("locked", true, true),
        Ok(EncryptionState::Initializing) => ("locked", true, true),
        Err(e) => return internal_error(anyhow::anyhow!("{}", e)).into_response(),
    };

    Json(json!({
        "state": state_str,
        "has_keyslot": has_keyslot,
        "requires_passphrase": requires_passphrase,
    })).into_response()
}
```

### WS Event Broadcast on Unlock

```rust
// Source: src-tauri/crates/uc-daemon/src/api/event_emitter.rs pattern
let _ = state.event_tx.send(DaemonWsEvent {
    topic: ws_topic::ENCRYPTION.to_string(),        // new topic constant
    event_type: ws_event::ENCRYPTION_SESSION_READY.to_string(),
    session_id: None,
    ts: chrono::Utc::now().timestamp_millis(),
    payload: serde_json::json!({}),
});
```

### Clear Cache — L4 Confirmation Check

```rust
#[derive(Deserialize)]
struct ClearCacheRequest {
    confirmed: bool,
}

async fn clear_cache_handler(
    State(state): State<DaemonApiState>,
    body: Result<Json<ClearCacheRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(body) = match body {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({
            "code": "confirmation_required",
            "message": "request body must contain { \"confirmed\": true }"
        }))).into_response(),
    };

    if !body.confirmed {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "code": "confirmation_required",
            "message": "set confirmed: true to proceed with cache clear"
        }))).into_response();
    }
    // ... proceed with clear_cache use case
}
```

---

## Runtime State Inventory

> This phase is a greenfield addition of new API endpoints, not a rename/refactor. No runtime state inventory needed.

---

## Environment Availability

> This phase is purely code/config changes — new HTTP handlers and use cases. No new external dependencies introduced.

---

## Validation Architecture

### Test Framework

| Property           | Value                                            |
| ------------------ | ------------------------------------------------ |
| Framework          | cargo test (Rust unit tests + integration tests) |
| Config file        | src-tauri/Cargo.toml (workspace)                 |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon`        |
| Full suite command | `cd src-tauri && cargo test`                     |

### Phase Requirements → Test Map

| ID       | Behavior                                                 | Test Type | Automated Command                                          | File Exists? |
| -------- | -------------------------------------------------------- | --------- | ---------------------------------------------------------- | ------------ |
| 76-S-01  | GET /settings returns settings JSON                      | unit      | `cargo test -p uc-daemon settings_get`                     | ❌ Wave 0    |
| 76-S-02  | PUT /settings persists updated settings                  | unit      | `cargo test -p uc-daemon settings_put`                     | ❌ Wave 0    |
| 76-S-03  | PUT /settings returns 400 on malformed body              | unit      | `cargo test -p uc-daemon settings_put_bad_request`         | ❌ Wave 0    |
| 76-E-01  | GET /encryption/state returns correct state string       | unit      | `cargo test -p uc-daemon encryption_state`                 | ❌ Wave 0    |
| 76-E-02  | POST /encryption/unlock with correct passphrase succeeds | unit      | `cargo test -p uc-daemon encryption_unlock`                | ❌ Wave 0    |
| 76-E-03  | POST /encryption/lock clears session                     | unit      | `cargo test -p uc-daemon encryption_lock`                  | ❌ Wave 0    |
| 76-E-04  | Successful unlock broadcasts WS event                    | unit      | `cargo test -p uc-daemon encryption_unlock_ws_event`       | ❌ Wave 0    |
| 76-ST-01 | GET /storage/stats returns size fields                   | unit      | `cargo test -p uc-daemon storage_stats`                    | ❌ Wave 0    |
| 76-ST-02 | POST /storage/clear-cache requires confirmed:true        | unit      | `cargo test -p uc-daemon storage_clear_cache_confirmation` | ❌ Wave 0    |
| 76-ST-03 | POST /storage/clear-cache without confirmed returns 400  | unit      | `cargo test -p uc-daemon storage_clear_cache_no_confirm`   | ❌ Wave 0    |
| 76-P-01  | L3 route rejects unauthenticated requests                | unit      | `cargo test -p uc-daemon l3_permission_enforcement`        | ❌ Wave 0    |
| 76-P-02  | daemon_api_strings constants assert correct values       | unit      | `cargo test -p uc-core daemon_api_strings`                 | ❌ Wave 0    |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/api/settings.rs` — covers 76-S-01..03
- [ ] `src-tauri/crates/uc-daemon/src/api/encryption.rs` — covers 76-E-01..04
- [ ] `src-tauri/crates/uc-daemon/src/api/storage.rs` — covers 76-ST-01..03
- [ ] `src-tauri/crates/uc-app/src/usecases/unlock_encryption_with_passphrase.rs` — new use case
- [ ] L3/L4 permission enforcement in `src-tauri/crates/uc-daemon/src/security/permission.rs`
- [ ] `ws_event::ENCRYPTION_SESSION_READY` constant in `uc-core/src/network/daemon_api_strings.rs`
- [ ] `ws_topic::ENCRYPTION` constant in `uc-core/src/network/daemon_api_strings.rs`

---

## Open Questions

1. **blob_count source**
   - What we know: `StorageStatsResult` does not include blob count. CONTEXT.md wants it.
   - What's unclear: Best source — vault dir file count vs `list_entries()` count vs a dedicated `BlobRepositoryPort` method.
   - Recommendation: Use `list_entries(10000, 0).len()` as an approximation (matches clipboard stats pattern). This is slightly wrong (counts clipboard entries not blobs), but good enough for a stats display. Flag for later improvement if precision is needed.

2. **spool_size_bytes source**
   - What we know: `AppPaths.spool_dir` exists, but `GetStorageStats` doesn't include it.
   - What's unclear: Should `GetStorageStats` be extended, or should daemon compute it separately?
   - Recommendation: Extend `GetStorageStats` to add `spool_bytes` field using the same `dir_size` pattern. This keeps all storage measurement in one place and avoids daemon-layer filesystem access.

3. **L3 permission: "requires encryption initialized" vs "requires encryption unlocked"**
   - What we know: CONTEXT.md says L3 = "settings write, encryption unlock/lock". Settings write should work even when encryption is locked (user may want to change UI settings without unlocking).
   - What's unclear: Should L3 require `session_ready` (unlocked) or just `Initialized` (passphrase configured)?
   - Recommendation: L3 = authenticated only (same as L2) — no encryption check needed for settings write or encryption unlock. L3 in this phase is a semantic label for "sensitive mutation" without additional auth check. The encryption lock/unlock operations themselves are their own gate. Only L4 adds an extra body confirmation check.

4. **`encryption` WS topic naming**
   - What we know: Existing WS topics follow lowercase-hyphenated convention (e.g., `space-access`, `file-transfer`).
   - Recommendation: Use `ws_topic::ENCRYPTION = "encryption"` (no hyphen needed — single word).

---

## Sources

### Primary (HIGH confidence)

- Direct code inspection: `src-tauri/crates/uc-daemon/src/api/` — existing route pattern
- Direct code inspection: `src-tauri/crates/uc-daemon/src/security/permission.rs` — L1/L2 defined, L3/L4 deferred
- Direct code inspection: `src-tauri/crates/uc-app/src/usecases/` — all referenced use cases
- Direct code inspection: `src-tauri/crates/uc-core/src/settings/model.rs` — Settings struct with Serialize/Deserialize
- Direct code inspection: `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — constant patterns
- Direct code inspection: `src-tauri/crates/uc-app/src/usecases/auto_unlock_encryption_session.rs` — unlock flow

### Secondary (MEDIUM confidence)

- CONTEXT.md analysis — all implementation decisions locked by user

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all libraries already in use
- Architecture: HIGH — established patterns from Phase 74/75 code inspection
- Pitfalls: HIGH — identified from direct code inspection of gaps (spool_size_bytes, blob_count not in GetStorageStats)

**Research date:** 2026-03-30
**Valid until:** 2026-04-30 (stable codebase, low churn)
