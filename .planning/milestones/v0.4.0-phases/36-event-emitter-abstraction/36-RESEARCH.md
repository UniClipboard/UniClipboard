# Phase 36: Event Emitter Abstraction - Research

**Researched:** 2026-03-17
**Domain:** Rust trait abstraction / Tauri event system / Hexagonal architecture port patterns
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Phase Boundary:** Define `HostEventEmitterPort` trait and two adapters (Tauri, Logging). Migrate **only** the three EVNT-04 component categories (clipboard watcher, peer discovery, sync scheduler) **plus** `AppRuntime`'s clipboard emit path to use the new port. All other `app.emit()` call sites in wiring.rs remain untouched.

**In-scope emit sites (exhaustive):**

- Clipboard watcher emit in `AppRuntime::on_clipboard_changed` (runtime.rs:1189-1205)
- Peer discovery changed events (wiring.rs:2336, 2364)
- Peer connection changed events (wiring.rs:2379, 2391, 2404)
- Peer name updated events (wiring.rs:2418)
- Sync/inbound clipboard events (wiring.rs:1376, 1860)
- File transfer completed (wiring.rs:2546)
- File transfer status changed (wiring.rs:1774, file_transfer_wiring.rs:85)
- Transfer progress (transfer_progress.rs:42)

**Out-of-scope emit sites (remain as direct app.emit()):**

- setup-state-changed (wiring.rs:204)
- space-access-completed / p2p-space-access-completed (wiring.rs:2115, 2119)
- pairing-verification (wiring.rs:2671, 3110, 3124, 3241, 3249, 3272)
- pairing-events-subscribe-failure/recovered (wiring.rs:3300, 3314)
- inbound-clipboard-subscribe-error/retry (wiring.rs:1418, 1434)
- emit_to for quick-panel/preview-panel
- Commands-layer emits (encryption.rs, pairing.rs, tray.rs)
- libp2p start-failed (events/mod.rs:48)
- clipboard monitor heartbeat (clipboard_monitor.rs:43)

**Trait design:**

- Single method: `fn emit(&self, event: HostEvent) -> Result<(), EmitError>`
- Trait name: `HostEventEmitterPort`
- Trait and all event types live in `uc-core/ports/`
- Components receive the port via `Arc<dyn HostEventEmitterPort>` constructor injection
- Trait is `Send + Sync`

**Event model identity:**

- `HostEvent` is a core semantic model, NOT a frontend protocol DTO
- HostEvent uses pure Rust types, no serde annotations, no camelCase rename
- TauriEventEmitter is solely responsible for converting HostEvent → Tauri event name string + serde-annotated payload struct
- TauriEventEmitter internally defines its own payload DTOs with `#[serde(rename_all = "camelCase")]` and `#[serde(tag = "type")]` as needed

**Event type system:**

- Strong-typed `HostEvent` enum with nested sub-enums per domain
- Only events for in-scope components are defined in Phase 36
- Event types newly defined in uc-core — NOT moved from uc-tauri
- Event name mapping is the adapter's internal responsibility

**Failure semantics:**

- Best-effort: warn + continue. Emit failure must never interrupt business flow
- Trait returns `Result<(), EmitError>` for observability; mandatory calling convention: log, then continue
- Non-GUI mode (LoggingEventEmitter) is infallible by design

**AppRuntime restructuring (in-scope):**

- `AppRuntime` gets a NEW `event_emitter: RwLock<Arc<dyn HostEventEmitterPort>>` field ALONGSIDE the existing `app_handle`
- `app_handle` field and `set_app_handle()` / `app_handle()` methods are KEPT for out-of-scope callers
- New `set_event_emitter()` / `event_emitter()` methods added for the port
- Emitter starts as LoggingEventEmitter at construction, swapped to TauriEventEmitter after setup callback
- AppRuntime::on_clipboard_changed uses the port instead of direct AppHandle read

**Migration strategy:**

- Strict EVNT-04 scope — only the three component categories + AppRuntime clipboard emit
- All other wiring.rs emit calls remain as direct `app.emit()`
- Existing `uc-tauri/events/` types referenced ONLY by in-scope sites are deleted; types still used by out-of-scope sites are preserved

**Commit split strategy (MANDATORY):**

1. `arch:` HostEventEmitterPort trait + HostEvent enums + EmitError (uc-core only) — `cargo check -p uc-core` passes
2. `impl:` TauriEventEmitter adapter + event contract tests (uc-tauri only) — `cargo check -p uc-tauri` passes
3. `impl:` LoggingEventEmitter adapter (uc-tauri or uc-infra) — `cargo check` passes
4. `refactor:` Wire emitter port into in-scope components + AppRuntime restructuring + delete obsolete code — `cargo test` passes

**Frontend compatibility verification:**

- Event contract tests are mandatory for every migrated event
- Each test asserts: exact event name string, JSON key naming (camelCase), required fields, tag values
- Tests live in TauriEventEmitter module (commit 2)
- Reference pattern: `test_setting_changed_event_camelcase_serialization` in events/mod.rs

**LoggingEventEmitter behavior:**

- Logs all events — filtering controlled by tracing level configuration
- Log levels vary: error events → `warn!`, key business events → `info!`, discovery changes → `debug!`
- Output uses structured tracing fields consistent with existing patterns
- Sensitive field policy: no raw keys, passphrases, decrypted content

### Claude's Discretion

- Exact HostEvent sub-enum variant names and field structures
- EmitError type design (simple string vs structured)
- Internal implementation of TauriEventEmitter's event name mapping (match arms, const table, etc.)
- Whether LoggingEventEmitter lives in uc-tauri/adapters or uc-infra
- Specific tracing level assignment per event variant in LoggingEventEmitter

### Deferred Ideas (OUT OF SCOPE)

- Migrate remaining wiring.rs emit calls (setup, pairing-verification, space-access, setting-changed) to HostEventEmitterPort — Phase 37+
- Define HostEvent variants for out-of-scope event domains — add when those components are migrated
- Event buffering/backpressure for daemon mode — future daemon milestone
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                                | Research Support                                                                                           |
| ------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| EVNT-01 | System can deliver host events through an abstract HostEventEmitterPort trait in uc-core/ports             | Port trait design, uc-core dependency constraints, existing SetupEventPort / TransferProgressPort patterns |
| EVNT-02 | GUI mode can emit events to Tauri frontend via TauriEventEmitter adapter implementing HostEventEmitterPort | TauriSessionReadyEmitter pattern, `tauri::Emitter`, payload DTO design with camelCase serialization        |
| EVNT-03 | Non-GUI modes can deliver events to logging output via LoggingEventEmitter adapter                         | LoggingLifecycleEventEmitter pattern, structured tracing macros, infallible Result path                    |
| EVNT-04 | Background tasks accept HostEventEmitterPort instead of AppHandle<R: Runtime>                              | All in-scope emit call sites catalogued, AppRuntime struct changes, wiring.rs closure capture patterns     |

</phase_requirements>

---

## Summary

Phase 36 introduces a `HostEventEmitterPort` trait in `uc-core/ports` so that background tasks (clipboard watcher, peer discovery, sync scheduler) no longer hold a direct `AppHandle` reference. Instead they hold `Arc<dyn HostEventEmitterPort>`, making them runtime-mode-agnostic. Two concrete adapters implement the port: `TauriEventEmitter` (wraps `AppHandle`, maps `HostEvent` → Tauri event name + camelCase DTO) and `LoggingEventEmitter` (writes structured `tracing` output, always returns `Ok`).

The main complexity is the `AppRuntime` restructuring: the existing `Arc<RwLock<Option<AppHandle>>>` post-init pattern is replaced by constructor-time injection of `Arc<dyn HostEventEmitterPort>`. This eliminates the post-init `set_app_handle()` call and removes the RwLock. The wiring.rs closure captures currently passing `app.clone()` into loops must be replaced with `Arc<dyn HostEventEmitterPort>` captures for the nine in-scope call sites.

The design is well-supported by existing patterns: `SetupEventPort`, `TransferProgressPort`, and `LoggingLifecycleEventEmitter` demonstrate the exact port + logging adapter combination. `TauriSessionReadyEmitter` demonstrates how to wrap `AppHandle<R>` behind a trait with async-trait. The test pattern from `events/mod.rs` and `transfer_progress.rs` provides the event contract test template.

**Primary recommendation:** Follow the four-commit split exactly as specified. Port + event types first (uc-core only), then TauriEventEmitter with tests (uc-tauri only), then LoggingEventEmitter, then migration wiring. This satisfies the Hexagonal Architecture Commit Boundary Rule from AGENTS.md.

---

## Standard Stack

### Core

| Library                | Version | Purpose                                   | Why Standard                                                  |
| ---------------------- | ------- | ----------------------------------------- | ------------------------------------------------------------- |
| `async-trait`          | 0.1     | Enable `async fn` in trait definitions    | Already in uc-core Cargo.toml; all ports use it               |
| `tracing`              | 0.1     | Structured logging in LoggingEventEmitter | Project logging standard; already optional feature in uc-core |
| `thiserror`            | 2.0     | EmitError derive                          | Already in uc-core Cargo.toml                                 |
| `serde` + `serde_json` | 1       | TauriEventEmitter payload DTOs            | Already in uc-tauri Cargo.toml                                |
| `tauri::Emitter`       | 2       | `app.emit()` in TauriEventEmitter         | Only available in uc-tauri layer                              |

### No New Dependencies Required

All required crates are already present in existing Cargo.toml files. No new dependencies need to be added.

---

## Architecture Patterns

### Recommended File Layout

```
src-tauri/crates/
├── uc-core/src/ports/
│   ├── host_event_emitter.rs      # HostEventEmitterPort trait + HostEvent enum + EmitError
│   └── mod.rs                     # pub mod host_event_emitter; + pub use re-exports
├── uc-tauri/src/adapters/
│   ├── mod.rs                     # pub mod host_event_emitter;
│   └── host_event_emitter.rs      # TauriEventEmitter + LoggingEventEmitter + payload DTOs + tests
```

### Pattern 1: Port Trait (uc-core, no Tauri dependency)

The `SetupEventPort` pattern is the direct template:

```rust
// Source: src-tauri/crates/uc-core/src/ports/setup_event_port.rs
// Pattern: single-method async trait, Send + Sync

// New file: src-tauri/crates/uc-core/src/ports/host_event_emitter.rs

/// Core semantic events delivered to the host environment.
/// NOT a frontend protocol DTO — no serde, no camelCase.
pub enum HostEvent {
    Clipboard(ClipboardHostEvent),
    PeerDiscovery(PeerDiscoveryHostEvent),
    PeerConnection(PeerConnectionHostEvent),
    Transfer(TransferHostEvent),
}

pub enum ClipboardHostEvent {
    NewContent { entry_id: String, origin: ClipboardOriginKind },
    InboundError { message_id: String, origin_device_id: String, error: String },
    InboundSubscribeRecovered { recovered_after_attempts: u32 },
}

pub enum ClipboardOriginKind { Local, Remote }

pub enum PeerDiscoveryHostEvent {
    Discovered { peer_id: String, device_name: Option<String>, addresses: Vec<String> },
    Lost { peer_id: String, device_name: Option<String> },
}

pub enum PeerConnectionHostEvent {
    Ready { peer_id: String, device_name: Option<String> },
    NotReady { peer_id: String, device_name: Option<String> },
    Connected { peer_id: String, device_name: String },
    Disconnected { peer_id: String, device_name: Option<String> },
    NameUpdated { peer_id: String, device_name: String },
}

pub enum TransferHostEvent {
    Progress(uc_core::ports::TransferProgress),
    Completed {
        transfer_id: String,
        filename: String,
        peer_id: String,
        file_size: u64,
        auto_pulled: bool,
        file_path: String,
    },
    StatusChanged {
        transfer_id: String,
        entry_id: String,
        status: String,
        reason: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("emit failed: {0}")]
    Failed(String),
}

#[async_trait::async_trait]
pub trait HostEventEmitterPort: Send + Sync {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError>;
}
```

**Key constraint:** `HostEvent` must NOT import `serde`, `tauri`, or any infrastructure type. `TransferProgress` is already defined in `uc-core/ports/transfer_progress.rs` and can be reused.

**Note on sync vs async:** `SetupEventPort` is async; `TransferProgressPort` is async. However, Tauri's `app.emit()` is synchronous (non-async). Using a synchronous `fn emit()` avoids unnecessary async overhead and matches the best-effort fire-and-forget semantics. The trait can be non-async since no adapter needs to `await`.

### Pattern 2: TauriEventEmitter Adapter (uc-tauri, with Tauri dependency)

Template is `TauriSessionReadyEmitter` from `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs`:

```rust
// Source: src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs (TauriSessionReadyEmitter)
// Source: src-tauri/crates/uc-tauri/src/events/transfer_progress.rs (payload DTO pattern)

use tauri::{AppHandle, Emitter, Runtime};
use uc_core::ports::host_event_emitter::{EmitError, HostEvent, HostEventEmitterPort};

pub struct TauriEventEmitter<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriEventEmitter<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> HostEventEmitterPort for TauriEventEmitter<R> {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        let (event_name, payload) = self.map_event(event);
        self.app
            .emit(event_name, payload)
            .map_err(|e| EmitError::Failed(e.to_string()))
    }
}
```

Internal payload DTOs use `#[serde(rename_all = "camelCase")]` and `#[serde(tag = "type")]`:

```rust
// Internal to TauriEventEmitter module — NOT exported from uc-core
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ClipboardPayload {
    NewContent { entry_id: String, preview: String, origin: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PeerDiscoveryPayload {
    peer_id: String,
    device_name: Option<String>,
    addresses: Vec<String>,
    discovered: bool,
}
```

Event name mapping (preserves existing frontend contracts):
| HostEvent | Tauri event name |
|-----------|-----------------|
| `Clipboard(NewContent)` | `"clipboard://event"` |
| `Clipboard(InboundError)` | `"inbound-clipboard-error"` |
| `Clipboard(InboundSubscribeRecovered)` | `"inbound-clipboard-subscribe-recovered"` |
| `PeerDiscovery(Discovered/Lost)` | `"p2p-peer-discovery-changed"` |
| `PeerConnection(Ready/NotReady/Connected/Disconnected)` | `"p2p-peer-connection-changed"` |
| `PeerConnection(NameUpdated)` | `"p2p-peer-name-updated"` |
| `Transfer(Progress)` | `"file-transfer://progress"` |
| `Transfer(Completed)` | `"file-transfer://completed"` |
| `Transfer(StatusChanged)` | `"file-transfer://status-changed"` |

**Important:** `TauriEventEmitter` is constructed with `AppHandle` directly (not `Arc<RwLock<Option<AppHandle>>>`). The old post-init pattern is eliminated — the emitter is only created after AppHandle is available and injected into AppRuntime at that point.

### Pattern 3: LoggingEventEmitter Adapter

Template is `LoggingLifecycleEventEmitter` from `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs`:

```rust
// Source: src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs

pub struct LoggingEventEmitter;

impl HostEventEmitterPort for LoggingEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Clipboard(ClipboardHostEvent::NewContent { entry_id, .. }) => {
                tracing::info!(event_type = "clipboard.new_content", entry_id = %entry_id);
            }
            HostEvent::PeerDiscovery(PeerDiscoveryHostEvent::Discovered { peer_id, .. }) => {
                tracing::debug!(event_type = "peer.discovered", peer_id = %peer_id);
            }
            // ... etc
        }
        Ok(())  // Always Ok — infallible by design
    }
}
```

### Pattern 4: AppRuntime Restructuring

Current state (runtime.rs:99-104, 197, 224-244):

```rust
// CURRENT: post-init setter pattern
pub struct AppRuntime {
    app_handle: Arc<std::sync::RwLock<Option<tauri::AppHandle>>>,
    // ...
}
impl AppRuntime {
    pub fn set_app_handle(&self, handle: tauri::AppHandle) { ... }
    pub fn app_handle(&self) -> RwLockReadGuard<'_, Option<tauri::AppHandle>> { ... }
}
```

Target state (Phase 36):

```rust
// NEW: event_emitter ADDED alongside app_handle (app_handle KEPT)
pub struct AppRuntime {
    app_handle: Arc<std::sync::RwLock<Option<tauri::AppHandle>>>,  // KEPT for out-of-scope callers
    event_emitter: std::sync::RwLock<Arc<dyn uc_core::ports::HostEventEmitterPort>>,  // NEW
    // ...
}
impl AppRuntime {
    pub fn set_app_handle(&self, handle: tauri::AppHandle) { ... }  // KEPT
    pub fn app_handle(&self) -> ... { ... }  // KEPT
    pub fn event_emitter(&self) -> Arc<dyn HostEventEmitterPort> { ... }  // NEW
    pub fn set_event_emitter(&self, emitter: Arc<dyn HostEventEmitterPort>) { ... }  // NEW
}
```

The emitter starts as `LoggingEventEmitter` at construction time, then `set_event_emitter()` swaps it to `TauriEventEmitter` after the Tauri setup callback provides the `AppHandle`. The existing `app_handle` field, `set_app_handle()`, and `app_handle()` are preserved for out-of-scope callers (pairing commands, clipboard commands, build_setup_orchestrator, app_lifecycle_coordinator).

**Critical subtlety:** `TauriSessionReadyEmitter` in `build_setup_orchestrator()` (runtime.rs:341-343) still uses the old `Arc<RwLock<Option<AppHandle>>>` pattern via `app_handle()`. This is OUT OF SCOPE for Phase 36 and continues working because `app_handle` is kept.

### Pattern 5: wiring.rs Closure Migration

Current pattern in wiring.rs:

```rust
let app_handle: Option<AppHandle> = ...;
if let Some(app) = app_handle.as_ref() {
    let payload = P2PPeerDiscoveryEvent { ... };
    if let Err(err) = app.emit("p2p-peer-discovery-changed", payload) {
        warn!(...);
    }
}
```

Replacement pattern:

```rust
let emitter: Arc<dyn HostEventEmitterPort> = ...;  // captured in closure
if let Err(err) = emitter.emit(HostEvent::PeerDiscovery(...)) {
    warn!(error = %err, "Failed to emit peer discovery changed event");
}
```

The `if let Some(app) = app_handle.as_ref()` guard is eliminated — the emitter is always present (either Tauri or Logging), so no Option check is needed.

### Anti-Patterns to Avoid

- **Serde on HostEvent:** HostEvent must not have `#[derive(Serialize)]`. It is a pure semantic model. Only TauriEventEmitter's internal DTOs have serde.
- **Tauri import in uc-core:** `HostEventEmitterPort` must compile with `cargo check -p uc-core` without any tauri dependency.
- **Async trait on sync emit:** The `emit()` method is synchronous. Do not use `#[async_trait]` — it adds unnecessary boxing overhead for a fire-and-forget operation.
- **Removing out-of-scope app_handle usages:** Do not touch `TauriSessionReadyEmitter` or any out-of-scope emit sites in commit 4. wiring.rs still uses `app.clone()` for setup, pairing, etc.
- **Mixing uc-core + uc-tauri in one commit:** The Hexagonal Architecture Commit Boundary Rule from AGENTS.md forbids port + adapter in the same commit.

---

## Don't Hand-Roll

| Problem                            | Don't Build                        | Use Instead                                  | Why                                                         |
| ---------------------------------- | ---------------------------------- | -------------------------------------------- | ----------------------------------------------------------- |
| Event name dispatch table          | Custom HashMap<EventType, &str>    | Match arms in `map_event()`                  | Exhaustiveness checked at compile time; no runtime cost     |
| Async emit queuing                 | Custom channel + worker task       | Direct synchronous emit                      | Tauri's `app.emit()` is thread-safe and non-blocking        |
| Type-erased payload                | `Box<dyn erased_serde::Serialize>` | Per-variant DTO structs in TauriEventEmitter | No dependency on erased_serde; fully type-safe per event    |
| Test mock for HostEventEmitterPort | Complex mock framework             | Simple `Vec<HostEvent>` capturing struct     | Sufficient for unit tests; no mockall needed for this trait |

**Key insight:** The port trait is intentionally minimal (single sync method). Resist adding complexity like batching, priority, or async variants — these are Phase 37+ concerns if needed for daemon mode.

---

## Common Pitfalls

### Pitfall 1: Forgetting TauriSessionReadyEmitter Still Uses Old app_handle Pattern

**What goes wrong:** Developer sees `app_handle` field removed from AppRuntime and removes ALL usages, breaking `build_setup_orchestrator` which passes `app_handle.clone()` to `TauriSessionReadyEmitter::new()`.
**Why it happens:** The `app_handle` field in AppRuntime serves two purposes currently; only the clipboard-emit purpose is migrated.
**How to avoid:** Phase 36 migrates only the `app_handle` used for clipboard emit (lines 1189-1205). The `Arc<RwLock<Option<AppHandle>>>` for `TauriSessionReadyEmitter` must remain as a local in `with_setup()` or `build_setup_orchestrator()` — just no longer stored as a field on `AppRuntime`.
**Warning signs:** `cargo check -p uc-tauri` failure in `build_setup_orchestrator` after removing the field.

### Pitfall 2: ClipboardEvent / P2PPeerDiscoveryEvent Still Used by Out-of-Scope Sites

**What goes wrong:** Deleting `uc-tauri/events/p2p_peer.rs` (since it seems fully replaced) but forgetting that peer event types may still be referenced by pairing code in wiring.rs.
**Why it happens:** The in-scope peer discovery/connection events and the out-of-scope pairing events could use the same DTO structs.
**How to avoid:** Before deleting any type from `uc-tauri/events/`, grep for all usages. Only delete types with zero remaining references after the migration.
**Warning signs:** `cargo check -p uc-tauri` failure with "cannot find type" in pairing code.

### Pitfall 3: snake_case in TauriEventEmitter Payload DTOs

**What goes wrong:** Internal payload structs in `TauriEventEmitter` omit `#[serde(rename_all = "camelCase")]`, causing frontend to receive `transfer_id` instead of `transferId`.
**Why it happens:** Easy to miss when defining new internal structs.
**How to avoid:** Event contract tests (mandatory, commit 2) catch this immediately. Reference: `test_setting_changed_event_camelcase_serialization` in events/mod.rs.
**Warning signs:** Test asserting `json["transferId"]` fails; `json["transfer_id"]` is present instead.

### Pitfall 4: AppRuntime Constructor Callers Not Updated

**What goes wrong:** `AppRuntime::with_setup()` signature changes to require `Arc<dyn HostEventEmitterPort>`, breaking all callers (main.rs, tests).
**Why it happens:** Constructor injection replaces post-init setter, which is a signature change.
**How to avoid:** Commit 4 must update ALL callers of `AppRuntime::new()` and `AppRuntime::with_setup()`. Search: `AppRuntime::new(`, `AppRuntime::with_setup(`.
**Warning signs:** `cargo check` failure in `src-tauri/src/main.rs`.

### Pitfall 5: wiring.rs Function Signatures Carrying AppHandle

**What goes wrong:** Helper functions like `emit_pending_status<R: tauri::Runtime>(app: &AppHandle<R>, ...)` in `file_transfer_wiring.rs` still accept `AppHandle` directly. After migration, callers pass the emitter port but helpers still demand `AppHandle`.
**Why it happens:** Multiple layers of indirection; `file_transfer_wiring.rs` has its own functions that take `AppHandle`.
**How to avoid:** Migrate helper function signatures in the same commit 4 as call sites. `emit_pending_status` and `handle_transfer_progress` in `file_transfer_wiring.rs` must be updated to accept `&dyn HostEventEmitterPort` or the emitter Arc.
**Warning signs:** Compiler error at call sites even after updating wiring.rs closures.

---

## Code Examples

Verified patterns from official project sources:

### Existing Port Trait (SetupEventPort — structural template)

```rust
// Source: src-tauri/crates/uc-core/src/ports/setup_event_port.rs
use crate::setup::SetupState;

#[async_trait::async_trait]
pub trait SetupEventPort: Send + Sync {
    async fn emit_setup_state_changed(&self, state: SetupState, session_id: Option<String>);
}
```

### Existing Logging Adapter (LoggingLifecycleEventEmitter — LoggingEventEmitter template)

```rust
// Source: src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs
pub struct LoggingLifecycleEventEmitter;

#[async_trait]
impl LifecycleEventEmitter for LoggingLifecycleEventEmitter {
    async fn emit_lifecycle_event(&self, event: LifecycleEvent) -> Result<()> {
        tracing::info!(event = ?event, "Lifecycle event");
        Ok(())
    }
}
```

### Existing Tauri Adapter (TauriSessionReadyEmitter — TauriEventEmitter template)

```rust
// Source: src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs
pub struct TauriSessionReadyEmitter<R: Runtime> {
    app_handle: Arc<std::sync::RwLock<Option<AppHandle<R>>>>,
}

#[async_trait]
impl<R: Runtime> SessionReadyEmitter for TauriSessionReadyEmitter<R> {
    async fn emit_ready(&self) -> Result<()> {
        let guard = self.app_handle.read()...;
        if let Some(app) = guard.as_ref() {
            if let Err(err) = crate::events::forward_encryption_event(app, ...) {
                tracing::warn!(error = %err, "Failed to emit ...");
            }
        }
        Ok(())
    }
}
```

### Event Contract Test Template

```rust
// Source: src-tauri/crates/uc-tauri/src/events/transfer_progress.rs (tests)
#[tokio::test]
async fn forward_transfer_progress_event_emits_on_correct_channel() {
    let app = tauri::test::mock_app();
    let app_handle = app.handle();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

    app_handle.listen("file-transfer://progress", move |event: tauri::Event| {
        let _ = tx.try_send(event.payload().to_string());
    });

    forward_transfer_progress_event(&app_handle, progress).expect("emit");

    let payload = rx.recv().await.expect("event payload");
    // Assert camelCase keys present, snake_case absent
    let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(json["transferId"], "test-xfer");
    assert!(json.get("transfer_id").is_none());
}
```

### Existing wiring.rs emit pattern (to be replaced for in-scope sites)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:2329-2339
if let Some(app) = app_handle.as_ref() {
    let payload = P2PPeerDiscoveryEvent {
        peer_id: peer.peer_id.clone(),
        device_name: peer.device_name,
        addresses: peer.addresses,
        discovered: true,
    };
    if let Err(err) = app.emit("p2p-peer-discovery-changed", payload) {
        warn!(error = %err, "Failed to emit peer discovery changed event");
    }
}
```

---

## State of the Art

| Old Approach                                          | Current Approach                                                               | Impact                                                |
| ----------------------------------------------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------- |
| `Arc<RwLock<Option<AppHandle>>>` post-init setter     | `Arc<dyn HostEventEmitterPort>` constructor injection                          | Eliminates None-check guard, enables non-Tauri modes  |
| Direct `app.emit(...)` in background loops            | `emitter.emit(HostEvent::...)`                                                 | Background tasks have zero Tauri import dependency    |
| Payload DTOs in `uc-tauri/events/` as the event model | `HostEvent` in `uc-core` as semantic model, DTOs internal to TauriEventEmitter | Core stays clean, adapter owns serialization contract |

---

## Open Questions

1. **LoggingEventEmitter placement: uc-tauri/adapters vs uc-infra**
   - What we know: `LoggingLifecycleEventEmitter` lives in `uc-tauri/adapters/lifecycle.rs`. LoggingEventEmitter has no Tauri dependency.
   - What's unclear: Should it go in `uc-tauri/adapters/` (consistent with existing pattern) or `uc-infra/` (no Tauri dep)?
   - Recommendation: Place in `uc-tauri/adapters/host_event_emitter.rs` alongside TauriEventEmitter for cohesion. When daemon mode needs it, it can be moved to a shared crate. This matches the `LoggingLifecycleEventEmitter` precedent.

2. **Sync vs async emit method**
   - What we know: Tauri `app.emit()` is synchronous. `SetupEventPort` is async. `TransferProgressPort` is async.
   - What's unclear: Will future adapters need async emit (e.g., writing to async channel for daemon mode)?
   - Recommendation: Use synchronous `fn emit()` for Phase 36. Rationale: best-effort fire-and-forget semantics; no buffering needed; eliminates async overhead. If daemon mode needs async emit, the trait can be extended in a future phase.

3. **`file_transfer_wiring.rs` helper function migration scope**
   - What we know: `emit_pending_status` and `handle_transfer_progress` in `file_transfer_wiring.rs` take `AppHandle<R>` directly (lines 73-92, 101+).
   - What's unclear: Whether to refactor these helpers into the emitter port pattern or keep them as-is with an AppHandle passed from the emitter.
   - Recommendation: Refactor helper signatures to accept `&dyn HostEventEmitterPort` in commit 4. Passing AppHandle down would defeat the abstraction.

---

## Validation Architecture

### Test Framework

| Property           | Value                                    |
| ------------------ | ---------------------------------------- |
| Framework          | Rust built-in + cargo test               |
| Config file        | None (workspace tests)                   |
| Quick run command  | `cd src-tauri && cargo test -p uc-core`  |
| Full suite command | `cd src-tauri && cargo test --workspace` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                           | Test Type            | Automated Command                                              | File Exists? |
| ------- | ------------------------------------------------------------------ | -------------------- | -------------------------------------------------------------- | ------------ |
| EVNT-01 | `HostEventEmitterPort` in uc-core compiles without Tauri dep       | unit (compile check) | `cd src-tauri && cargo check -p uc-core`                       | ❌ Wave 0    |
| EVNT-02 | TauriEventEmitter emits correct event names and camelCase payloads | unit                 | `cd src-tauri && cargo test -p uc-tauri host_event_emitter`    | ❌ Wave 0    |
| EVNT-03 | LoggingEventEmitter always returns Ok, logs structured fields      | unit                 | `cd src-tauri && cargo test -p uc-tauri logging_event_emitter` | ❌ Wave 0    |
| EVNT-04 | Background tasks compile without AppHandle import                  | compile check        | `cd src-tauri && cargo check -p uc-tauri`                      | ❌ Wave 0    |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check -p uc-core` (commit 1), `cargo check -p uc-tauri` (commits 2-4)
- **Per wave merge:** `cd src-tauri && cargo test --workspace`
- **Phase gate:** `cd src-tauri && cargo test --workspace` green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` — covers EVNT-01 (created in commit 1)
- [ ] `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` — covers EVNT-02, EVNT-03 (created in commits 2-3)
- [ ] Event contract tests for all 9 in-scope event mappings — covers EVNT-02 (created in commit 2)

---

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-core/src/ports/setup_event_port.rs` — Port trait design pattern (single method, Send + Sync)
- `src-tauri/crates/uc-core/src/ports/transfer_progress.rs` — Async port with data payload; also shows NoopXxx pattern
- `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs` — LoggingLifecycleEventEmitter + TauriSessionReadyEmitter; exact template for both adapters
- `src-tauri/crates/uc-tauri/src/events/mod.rs` — Event contract test pattern (`test_setting_changed_event_camelcase_serialization`); existing ClipboardEvent / EncryptionEvent DTO shape
- `src-tauri/crates/uc-tauri/src/events/transfer_progress.rs` — TransferProgressEvent camelCase test template; `forward_*` function pattern
- `src-tauri/crates/uc-tauri/src/events/p2p_peer.rs` — P2PPeerDiscoveryEvent / P2PPeerConnectionEvent / P2PPeerNameUpdatedEvent DTO shapes (frontend contract source of truth)
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs:99-244` — AppRuntime struct, `app_handle` field, `set_app_handle()`, `app_handle()`, `with_setup()`, `build_setup_orchestrator()`
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs:1182-1209` — `on_clipboard_changed` emit path (primary EVNT-04 target)
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:2322-2421` — Peer discovery/connection emit call sites
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:1360-1400` — Inbound clipboard subscribe recovered emit
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:1854-1865` — Inbound clipboard error emit
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:1766-1800` — File transfer status-changed emit
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:2535-2553` — File transfer completed emit
- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs:70-92` — `emit_pending_status` helper (AppHandle signature to be migrated)
- `src-tauri/AGENTS.md` — Atomic Commit Rule, Hexagonal Architecture Commit Boundary Rule, Tauri Event Payload Serialization
- `src-tauri/crates/uc-core/AGENTS.md` — No Tauri/system imports in uc-core
- `src-tauri/crates/uc-tauri/AGENTS.md` — Event payload camelCase mandate, bootstrap editing rules
- `src-tauri/crates/uc-core/Cargo.toml` — Available dependencies (thiserror, async-trait, tracing optional)
- `src-tauri/crates/uc-tauri/Cargo.toml` — Available dependencies (tauri, serde, serde_json, async-trait)

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all libraries already present in Cargo.toml, no new dependencies
- Architecture: HIGH — directly derived from existing patterns in the codebase (SetupEventPort, TauriSessionReadyEmitter, LoggingLifecycleEventEmitter)
- Pitfalls: HIGH — identified from careful reading of AppRuntime struct, wiring.rs emit sites, and file_transfer_wiring.rs helper signatures
- Migration scope: HIGH — all 9 in-scope emit call sites catalogued with file + line references

**Research date:** 2026-03-17
**Valid until:** 2026-04-17 (stable architecture; no fast-moving libraries involved)
