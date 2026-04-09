# Phase 72: Migrate restore-clipboard to daemon — eliminate cross-process origin desync - Research

**Researched:** 2026-03-29
**Domain:** Daemon HTTP API extension — clipboard restore route migration
**Confidence:** HIGH

## Summary

The cross-process origin desync bug exists because the GUI Tauri process calls `ClipboardChangeOriginPort::set_next_origin(LocalRestore)` in its own address space, but the daemon's `ClipboardWatcherWorker` has a **separate** `InMemoryClipboardChangeOrigin` instance. When the GUI writes to the OS clipboard, the daemon's watcher fires, checks its (unset) origin tracker, and misclassifies the write as `LocalCapture`. This causes a duplicate DB entry and a spurious outbound sync.

The fix is to move the restore OS-clipboard write into the daemon process, where the same `clipboard_change_origin` Arc is shared between `DaemonClipboardChangeHandler`, `InboundClipboardSyncWorker`, and `FileSyncOrchestratorWorker`. The `RestoreClipboardSelectionUseCase` is already Tauri-free and lives in `uc-app`, so the daemon can call it directly from a new HTTP route handler without any new infrastructure.

The GUI Tauri command `restore_clipboard_entry` becomes a thin proxy: it calls `POST /clipboard/restore/{entry_id}` via `DaemonHttpClient` (following the existing pairing/setup pattern), waits for 200 OK, then emits the frontend clipboard event. This is a three-part change: (1) new daemon route, (2) new daemon client method, (3) rewired Tauri command.

**Primary recommendation:** Add `POST /clipboard/restore/{entry_id}` to the daemon HTTP API. The route handler calls `CoreUseCases::restore_clipboard_selection()` and `CoreUseCases::touch_clipboard_entry()` — both are already accessible from `DaemonApiState` via `state.runtime`. The GUI command becomes a thin proxy using the established `DaemonPairingClient`/`DaemonSetupClient` pattern in `uc-daemon-client`.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**D-01:** Restore goes through HTTP POST endpoint (`POST /clipboard/restore/{entry_id}`) following the established daemon mutation pattern (loopback + bearer auth)

**D-02:** Response is minimal: 200 OK + `{ success: true }` or error code. The `clipboard.new_content` WS event (already emitted by ClipboardWatcherWorker after OS clipboard write) handles frontend state updates

**D-03:** Restore triggers outbound sync to peers — behavior preserved from current implementation. The daemon's `DaemonClipboardChangeHandler` will correctly identify the origin as `LocalRestore` and `OutboundSyncPlanner` allows sync for both `LocalCapture` and `LocalRestore`

**D-04:** No Full mode fallback retained. All restore operations go exclusively through daemon HTTP API. The GUI `restore_clipboard_entry` Tauri command becomes a thin proxy that calls the daemon endpoint via `DaemonHttpClient`

**D-05:** Direct `RestoreClipboardSelectionUseCase` invocation and local `ClipboardChangeOriginPort` usage removed from uc-tauri command layer

### Claude's Discretion

- Daemon-side implementation details: whether to reuse existing `RestoreClipboardSelectionUseCase` directly or create a new daemon-specific handler
- Error mapping between daemon HTTP errors and Tauri command errors for frontend compatibility
- Whether `touch_clipboard_entry` (active_time bump) stays in the daemon endpoint or moves elsewhere

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

## Standard Stack

### Core

| Library    | Version   | Purpose                        | Why Standard                                    |
| ---------- | --------- | ------------------------------ | ----------------------------------------------- |
| axum       | workspace | HTTP route handler             | All existing daemon routes use axum             |
| reqwest    | workspace | HTTP client (GUI side)         | All existing daemon client calls use reqwest    |
| serde_json | workspace | Request/response serialization | Established pattern across all daemon API types |
| tokio      | workspace | Async runtime                  | Project-wide async executor                     |

### Supporting

| Library | Version   | Purpose                    | When to Use                                                  |
| ------- | --------- | -------------------------- | ------------------------------------------------------------ |
| tracing | workspace | Structured logging + spans | Every handler follows `info_span!` + `.instrument()` pattern |

**No new dependencies required.** All needed crates are already in the workspace.

## Architecture Patterns

### Daemon Route Handler Pattern (from existing routes.rs)

```rust
// Source: src-tauri/crates/uc-daemon/src/api/routes.rs (handle_unpair_device)
async fn restore_clipboard_entry(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let usecases = CoreUseCases::new(runtime.as_ref());
    // touch first (following existing uc-tauri command order)
    match usecases.touch_clipboard_entry().execute(&EntryId::from(entry_id.clone())).await {
        Ok(true) => {}
        Ok(false) => return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(e) => return internal_error(e).into_response(),
    }

    match usecases.restore_clipboard_selection().execute(&EntryId::from(entry_id)).await {
        Ok(()) => (StatusCode::OK, Json(json!({"success": true}))).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}
```

**Key insight:** `CoreUseCases::new(runtime.as_ref())` is the established pattern for creating use cases from a daemon route — already used in `handle_unpair_device`. No new wiring needed.

**Key insight 2:** `RestoreClipboardSelectionUseCase::restore_snapshot()` already calls `clipboard_change_origin.set_next_origin(LocalRestore, ...)` before `local_clipboard.write_snapshot(snapshot)`. Since the daemon's `clipboard_change_origin` is the **same Arc** as used by `DaemonClipboardChangeHandler`, the origin is correctly set in-process before the OS clipboard write occurs.

**Key insight 3:** `RestoreClipboardSelectionUseCase` checks `mode.allow_os_write()`. The daemon's `CoreRuntime` is built with `ClipboardIntegrationMode::Full` (daemon always owns the clipboard), so `allow_os_write()` returns `true`.

### Daemon Client Method Pattern (from existing DaemonSetupClient)

```rust
// Source: src-tauri/crates/uc-daemon-client/src/http/setup.rs
pub async fn restore_clipboard_entry(&self, entry_id: &str) -> Result<()> {
    let path = format!("/clipboard/restore/{entry_id}");
    let request = self.authorized_request(Method::POST, &path)?;
    let response = request
        .send()
        .await
        .with_context(|| format!("failed to call daemon clipboard restore route {path}"))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_else(|_| "<failed to read body>".to_string());
    Err(anyhow::anyhow!(
        "daemon clipboard restore request {path} failed with status {}: {}",
        status, body
    ))
}
```

### GUI Tauri Command Proxy Pattern (from existing setup.rs commands)

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/setup.rs (pattern)
#[tauri::command]
pub async fn restore_clipboard_entry(
    runtime: State<'_, Arc<AppRuntime>>,
    daemon_connection: State<'_, DaemonConnectionState>,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    let span = info_span!("command.clipboard.restore_entry", entry_id = %entry_id);
    async move {
        DaemonClipboardClient::new(daemon_connection.inner().clone())
            .restore_clipboard_entry(&entry_id)
            .await
            .map(|()| true)
            .map_err(CommandError::internal)
    }
    .instrument(span)
    .await
}
```

**Note:** The `runtime` state parameter may be dropped from the signature when the command becomes a pure daemon proxy, since no local use cases are called. However, the existing test infrastructure in `clipboard.rs` that uses `AppRuntime` may need adjustment.

### Recommended Project Structure Changes

```
src-tauri/crates/
├── uc-core/src/network/daemon_api_strings.rs   # Add http_route module with CLIPBOARD_RESTORE constant
├── uc-daemon/src/api/routes.rs                 # Add restore_clipboard_entry route handler
│   └── router() function                       # Register .route("/clipboard/restore/:entry_id", post(...))
├── uc-daemon-client/src/http/                  # Add clipboard.rs client module
│   └── mod.rs                                  # Export DaemonClipboardClient
└── uc-tauri/src/commands/clipboard.rs          # Rewire restore_clipboard_entry to daemon proxy
```

### Anti-Patterns to Avoid

- **Calling `set_next_origin()` in the GUI before calling the daemon endpoint:** The whole point is to do this in-daemon. The GUI proxy must NOT touch `ClipboardChangeOriginPort` at all.
- **Creating a new `InMemoryClipboardChangeOrigin` in the route handler:** Must use `state.runtime.deps.clipboard.clipboard_change_origin` (the shared Arc).
- **Adding an outbound sync call in the daemon route handler:** The daemon's `DaemonClipboardChangeHandler` fires when `ClipboardWatcherWorker` detects the OS clipboard write; it will call `OutboundSyncPlanner` and dispatch outbound sync automatically (D-03). Double-sync if route handler also does it.
- **Keeping `ClipboardIntegrationMode` check in the proxy command:** Mode check is now irrelevant; restore always goes through daemon.
- **Emitting `clipboard://event` from the GUI command before daemon confirmation:** The `clipboard.new_content` WS event from the daemon (via `ClipboardWatcherWorker` detecting the write) is the authoritative frontend update. The GUI command can optionally emit a secondary event after success, but this is non-critical.

## Don't Hand-Roll

| Problem               | Don't Build                 | Use Instead                                  | Why                                                     |
| --------------------- | --------------------------- | -------------------------------------------- | ------------------------------------------------------- |
| HTTP auth             | Custom auth middleware      | `state.is_authorized(&headers)`              | Already in every route handler — one line               |
| Use case construction | Manual Arc wiring           | `CoreUseCases::new(runtime.as_ref())`        | Established pattern in `handle_unpair_device`           |
| HTTP client builder   | New reqwest client per call | `authorized_daemon_request()` helper         | Already handles bearer token injection                  |
| Error response shapes | Custom JSON error format    | `internal_error()`, `unauthorized()` helpers | These are in routes.rs as private fns, consistent shape |
| Route registration    | Hardcoded string in router  | `daemon_api_strings` constant                | Phase 56.1 pattern: all route strings go in uc-core     |

## Common Pitfalls

### Pitfall 1: RestoreClipboardSelectionUseCase in Passive mode

**What goes wrong:** Daemon's `CoreRuntime` is constructed with `ClipboardIntegrationMode::Full` (it IS the clipboard owner). But if mode is somehow `Passive`, `restore_snapshot()` returns an error and the clipboard write never happens.
**Why it happens:** Confusing the daemon's mode (always `Full`) with the GUI's mode (currently `Passive`).
**How to avoid:** Confirm `build_non_gui_runtime_with_emitter` sets `ClipboardIntegrationMode::Full`. Verified: daemon's runtime uses `Full` mode throughout.
**Warning signs:** `"System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)"` error from the route handler.

### Pitfall 2: Double outbound sync

**What goes wrong:** Route handler calls outbound sync AND `DaemonClipboardChangeHandler` also fires outbound sync when it detects the clipboard write.
**Why it happens:** Not understanding the watcher's behavior with `LocalRestore` origin.
**How to avoid:** The daemon route handler does NOT call `SyncOutboundClipboardUseCase` directly. The watcher-handler chain handles this: `ClipboardWatcherWorker` detects the write, `DaemonClipboardChangeHandler` checks origin (finds `LocalRestore`), calls `OutboundSyncPlanner`, dispatches sync.
**Warning signs:** Peer device receives the same clipboard content twice in quick succession.

### Pitfall 3: GUI command missing DaemonConnectionState parameter

**What goes wrong:** The existing `restore_clipboard_entry` command only takes `State<'_, Arc<AppRuntime>>`. After migration it also needs `State<'_, DaemonConnectionState>`.
**Why it happens:** Mechanical oversight when rewiring the command signature.
**How to avoid:** Follow the `setup.rs` command pattern — add `daemon_connection: State<'_, DaemonConnectionState>` parameter. `DaemonConnectionState` is already registered in Tauri state.
**Warning signs:** `state not managed for field 'DaemonConnectionState'` runtime panic.

### Pitfall 4: Frontend event missing after restore

**What goes wrong:** After daemon restore, the frontend doesn't update the clipboard history view.
**Why it happens:** The old command emitted `clipboard://event` directly. The new flow relies on the WS `clipboard.new_content` event from the daemon. If the GUI is in Passive mode and the WS bridge is connected, the event arrives via `DaemonWsBridge`. But if the WS bridge is temporarily disconnected or there's a race, the GUI may miss the event.
**How to avoid:** Per D-02, the WS event handles frontend update. The GUI command can also emit a fallback `clipboard://event` after receiving 200 OK (matching current behavior). This is belt-and-suspenders.
**Warning signs:** Clipboard entry visually stays highlighted as "current" in the wrong position after restore.

### Pitfall 5: Test in clipboard.rs tests `restore_clipboard_entry_impl` directly

**What goes wrong:** Existing unit tests in `commands/clipboard.rs` test `restore_clipboard_entry_impl` which calls the use case directly. After migration, this function is removed; tests must be updated or removed.
**Why it happens:** The `restore_clipboard_entry_impl` helper and its tests become dead code.
**How to avoid:** Integration tests for the daemon route go in `uc-daemon` (or a dedicated integration test). The GUI proxy command tests can be simpler (mock HTTP server response).
**Warning signs:** Compilation errors on `super::restore_clipboard_entry_impl` in test module.

### Pitfall 6: `touch_clipboard_entry` returns false for missing entry

**What goes wrong:** If `touch_clipboard_entry.execute()` returns `Ok(false)` (entry not found), the route handler should return 404, not 200.
**Why it happens:** The current Tauri command returns `NotFound` in this case; the daemon route must preserve this behavior.
**How to avoid:** Check `Ok(false)` → return 404 with `{"error": "not_found"}` before proceeding to `restore_snapshot`.
**Warning signs:** Restore of deleted entries silently succeeds with no clipboard write.

## Code Examples

### Route Registration in routes.rs

```rust
// Source: src-tauri/crates/uc-daemon/src/api/routes.rs (router() fn)
pub fn router() -> Router<DaemonApiState> {
    Router::new()
        // ... existing routes ...
        .route("/clipboard/restore/:entry_id", post(restore_clipboard_entry_handler))
}
```

### Existing `CoreUseCases::new` Usage from Route (canonical precedent)

```rust
// Source: src-tauri/crates/uc-daemon/src/api/routes.rs handle_unpair_device
let usecases = CoreUseCases::new(runtime.as_ref());
match usecases.unpair_device().execute(payload.peer_id).await {
    Ok(()) => StatusCode::NO_CONTENT.into_response(),
    Err(error) => { /* ... */ }
}
```

### daemon_api_strings new module (Phase 56.1 pattern)

```rust
// To add in: src-tauri/crates/uc-core/src/network/daemon_api_strings.rs
pub mod http_route {
    pub const CLIPBOARD_RESTORE: &str = "/clipboard/restore";
}
```

### Existing `authorized_daemon_request` Pattern (uc-daemon-client)

```rust
// Source: src-tauri/crates/uc-daemon-client/src/http/mod.rs
pub fn authorized_daemon_request(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    method: Method,
    path: &str,
) -> Result<RequestBuilder> {
    let connection = connection_state.get().ok_or_else(|| anyhow!("daemon connection info not available"))?;
    let url = format!("{}{}", connection.base_url, path);
    Ok(http.request(method, url).header(AUTHORIZATION, format!("Bearer {}", connection.token)))
}
```

## Runtime State Inventory

This phase is a code migration, not a rename/refactor. No stored data carries a string key that needs changing. The origin fix is purely in-process runtime behavior.

| Category            | Items Found                                                   | Action Required |
| ------------------- | ------------------------------------------------------------- | --------------- |
| Stored data         | None — clipboard entries and representations stay unchanged   | None            |
| Live service config | None — no external service config references the restore path | None            |
| OS-registered state | None                                                          | None            |
| Secrets/env vars    | None                                                          | None            |
| Build artifacts     | None                                                          | None            |

## Environment Availability

This phase is purely Rust code changes — no external tools beyond the existing build chain.

| Dependency | Required By        | Available | Version   | Fallback |
| ---------- | ------------------ | --------- | --------- | -------- |
| cargo      | Build              | ✓         | workspace | —        |
| axum       | Daemon HTTP server | ✓         | workspace | —        |
| reqwest    | Daemon HTTP client | ✓         | workspace | —        |

## Validation Architecture

### Test Framework

| Property           | Value                                                                                                 |
| ------------------ | ----------------------------------------------------------------------------------------------------- |
| Framework          | Rust built-in test + tokio::test                                                                      |
| Config file        | src-tauri/Cargo.toml (workspace)                                                                      |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon`                                                             |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon && cargo test -p uc-daemon-client && cargo test -p uc-tauri` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                                          | Test Type | Automated Command                                                                    | File Exists? |
| ------- | --------------------------------------------------------------------------------- | --------- | ------------------------------------------------------------------------------------ | ------------ |
| PH72-01 | Daemon route returns 200 with `{success:true}` on valid entry_id                  | unit      | `cd src-tauri && cargo test -p uc-daemon restore_clipboard`                          | ❌ Wave 0    |
| PH72-02 | Daemon route returns 404 when entry not found                                     | unit      | `cd src-tauri && cargo test -p uc-daemon restore_clipboard_not_found`                | ❌ Wave 0    |
| PH72-03 | Daemon route requires bearer auth (401 without token)                             | unit      | `cd src-tauri && cargo test -p uc-daemon restore_clipboard_unauthorized`             | ❌ Wave 0    |
| PH72-04 | GUI command calls daemon HTTP endpoint (not use case directly)                    | unit      | `cd src-tauri && cargo test -p uc-tauri restore_clipboard_proxies_to_daemon`         | ❌ Wave 0    |
| PH72-05 | `RestoreClipboardSelectionUseCase` existing test suite passes                     | unit      | `cd src-tauri && cargo test -p uc-app restore_clipboard_selection`                   | ✅           |
| PH72-06 | `ClipboardChangeOriginPort::set_next_origin(LocalRestore)` called before OS write | unit      | `cd src-tauri && cargo test -p uc-app restore_snapshot_clears_origin_on_write_error` | ✅           |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon && cargo check -p uc-tauri`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon && cargo test -p uc-daemon-client && cargo test -p uc-tauri`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/api/routes.rs` — integration test for `/clipboard/restore/:entry_id` endpoint (3 cases: success, 404, 401)
- [ ] `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — test that `restore_clipboard_entry` uses daemon HTTP client, not direct use case

## Open Questions

1. **Where to put DaemonClipboardClient in uc-daemon-client?**
   - What we know: Existing clients are `DaemonPairingClient` (pairing.rs), `DaemonSetupClient` (setup.rs), `DaemonQueryClient` (query.rs), all under `http/`
   - What's unclear: Whether to add `clipboard.rs` as a new file or extend an existing client
   - Recommendation: Add `http/clipboard.rs` with `DaemonClipboardClient` struct, following the `DaemonSetupClient` pattern exactly. Export from `http/mod.rs` and re-export from `lib.rs`.

2. **Whether to emit fallback `clipboard://event` from GUI proxy command**
   - What we know: D-02 says WS event handles frontend update. But WS may have brief gaps.
   - What's unclear: Whether the current frontend relies on the Tauri event or the WS event for the immediate visual update after restore.
   - Recommendation: Keep the `forward_clipboard_event` call in the GUI proxy command (after 200 OK) as belt-and-suspenders. It matches current behavior and guards against WS delivery races. If the WS event also arrives, the frontend deduplicates by entry_id.

3. **`touch_clipboard_entry` order: before or after `restore_snapshot`?**
   - What we know: Current uc-tauri command calls `touch` BEFORE `restore_snapshot`. This order is deliberate — touch confirms entry exists before attempting the clipboard write.
   - Recommendation: Preserve the same order in the daemon route handler: touch first (404 on false), then restore.

## Sources

### Primary (HIGH confidence)

All findings are from direct code inspection of the repository:

- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` lines 532-619 — current restore implementation
- `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs` — `RestoreClipboardSelectionUseCase` full implementation
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — existing route patterns (`handle_unpair_device` as the closest precedent for `CoreUseCases::new` usage)
- `src-tauri/crates/uc-daemon/src/api/server.rs` — `DaemonApiState` struct (confirms `runtime: Option<Arc<CoreRuntime>>` is available in all route handlers)
- `src-tauri/crates/uc-daemon/src/main.rs` lines 119-134 — shared `clipboard_change_origin` Arc construction and wiring to `DaemonClipboardChangeHandler`
- `src-tauri/crates/uc-daemon-client/src/http/setup.rs` — canonical pattern for daemon HTTP client methods
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — existing constant module structure
- `src-tauri/crates/uc-app/src/usecases/mod.rs` lines 330-354 — `restore_clipboard_selection()` and `touch_clipboard_entry()` accessors on `CoreUseCases`
- `src-tauri/crates/uc-app/src/runtime.rs` lines 35-62 — `CoreRuntime` struct confirming `clipboard_integration_mode` field

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all libraries already in workspace, no new deps
- Architecture: HIGH — direct code reading, patterns fully confirmed
- Pitfalls: HIGH — code-verified via reading `restore_snapshot()` implementation, existing daemon watcher flow, test suite

**Research date:** 2026-03-29
**Valid until:** 2026-04-28 (stable codebase, no fast-moving dependencies)
