# Phase 37: Wiring Decomposition - Research

**Researched:** 2026-03-17
**Domain:** Rust module decomposition, Tauri event loop isolation, HostEventEmitterPort migration
**Confidence:** HIGH

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Split boundary:**

- Pure assembly module contains: `wire_dependencies`, `wire_dependencies_with_identity_store`, `get_storage_paths`, `create_infra_layer`, `resolve_pairing_device_name`, `resolve_pairing_config`, and related helper functions
- `resolve_pairing_device_name` and `resolve_pairing_config` belong in assembly.rs (pure helpers, called from commands/settings.rs, adapters/lifecycle.rs, AND wiring.rs event loops)
- `start_background_tasks` and all event loop code stay in wiring.rs (Tauri module) — after all app.emit() calls are migrated, AppHandle<R> is removed from its signature
- `WiredDependencies` struct definition lives in assembly.rs (return type of wire_dependencies)
- `BackgroundRuntimeDeps` struct definition stays in wiring.rs (only used by start_background_tasks)

**app.emit() migration — complete:**

- ALL 14 remaining app.emit() calls in wiring.rs are migrated to HostEventEmitterPort in this phase
- Events covered: setup-state-changed (1), space-access-completed + p2p-space-access-completed (2), pairing-verification (7), pairing-events-subscribe-failure/recovered (2), inbound-clipboard-subscribe-error/retry (2)
- New HostEvent sub-enums: `HostEvent::Pairing(PairingHostEvent)`, `HostEvent::Setup(SetupHostEvent)`, `HostEvent::SpaceAccess(SpaceAccessHostEvent)`
- TauriEventEmitter extended with event name mapping and camelCase payload DTOs for each new domain
- Frontend contract tests mandatory for every new event mapping

**file_transfer_wiring.rs migration — complete:**

- All 5 functions migrated from AppHandle<R> to HostEventEmitterPort: handle_transfer_progress, handle_transfer_completed, handle_transfer_failed, spawn_timeout_sweep, reconcile_on_startup
- File stays in uc-tauri/bootstrap/ — no location change
- FileTransferStatusPayload remains as-is (already has serde annotations)
- After migration, file_transfer_wiring.rs has zero Tauri imports

**Module naming:**

- Pure assembly: `assembly.rs` in uc-tauri/src/bootstrap/
- Tauri event loops: retains `wiring.rs` name
- mod.rs uses `pub use assembly::*` and `pub use wiring::*` — external import paths unchanged

**Command registration ownership:**

- invoke_handler![...] macro (main.rs:852-927, ~60 commands) moves into Tauri module (wiring.rs or dedicated helper)
- main.rs becomes thin entry point

**SC#2/SC#4 staged interpretation:**

- SC#2 "only place that imports tauri types" is scoped to the wiring split pair (assembly.rs vs wiring.rs) — other bootstrap/ modules unchanged
- SC#4 is satisfied by structural proof: assembly.rs has zero tauri imports (grep + CI lint), API surface uses only Tauri-free crate types; full cargo check independence is Phase 40

**Commit split strategy (MANDATORY):**

1. `arch:` New HostEvent sub-enums (PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent) in uc-core
2. `impl:` TauriEventEmitter + LoggingEventEmitter extended + contract tests
3. `refactor:` Migrate app.emit() calls + file_transfer_wiring.rs to HostEventEmitterPort, remove AppHandle<R> from start_background_tasks
4. `refactor:` Split wiring.rs → assembly.rs + wiring.rs, move command registration
5. `docs:` Update ROADMAP.md Phase 37 SC#2/SC#4 wording

### Claude's Discretion

- Exact PairingHostEvent / SetupHostEvent / SpaceAccessHostEvent variant names and field structures
- Internal refactoring of wiring.rs closure patterns to accommodate emitter injection
- Order of migration (which event domain first)
- Whether command registration moves into wiring.rs or a separate helper function

### Deferred Ideas (OUT OF SCOPE)

- Moving assembly.rs to independent crate (uc-bootstrap) — Phase 40
- Making tauri an optional dependency in uc-tauri + feature-gate all Tauri-heavy modules — Phase 40
- Enforcing SC#2 "only place" constraint across ALL of uc-tauri — Phase 40
- Moving file_transfer_wiring.rs out of uc-tauri — Phase 38+
- Migrate commands-layer emits (pairing.rs, clipboard.rs, encryption.rs, tray.rs) — future phase
- Migrate emit_to for quick-panel/preview-panel — future phase
- Split wiring.rs further by domain — optional future cleanup
- clipboard monitor heartbeat (clipboard_monitor.rs:43)
  </user_constraints>

---

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                         | Research Support                                                                                                                                                                                                                                                                                                                     |
| ------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| RNTM-02 | wiring.rs is decomposed into pure assembly module (Tauri-free) and Tauri-specific event loop module | Confirmed feasible: all dependency construction functions use only uc-core/uc-app/uc-infra/uc-platform types; the 14 remaining app.emit() calls are clearly enumerated; file_transfer_wiring.rs has 5 AppHandle<R> functions ready for migration; start_background_tasks already accepts HostEventEmitterPort alongside AppHandle<R> |

</phase_requirements>

---

## Summary

Phase 37 decomposes `wiring.rs` (6328 lines, 246 KB) into two modules: `assembly.rs` (pure dependency construction, Tauri-free) and the retained `wiring.rs` (Tauri-specific event loops). This is a structural refactor — no behavioral changes.

Three prerequisite tasks run before the file split: (1) add new HostEvent domain sub-enums to uc-core for Pairing, Setup, and SpaceAccess events; (2) extend TauriEventEmitter and LoggingEventEmitter with new match arms and contract tests; (3) migrate all remaining `app.emit()` call sites to use the port instead of AppHandle<R>, and do the same for the 5 AppHandle<R> functions in file_transfer_wiring.rs.

After all emits are migrated, `start_background_tasks` drops its `app_handle: Option<AppHandle<R>>` parameter and the `R: Runtime` generic entirely. The file split then separates the pure-construction code into `assembly.rs` and moves the `invoke_handler![...]` block from main.rs into the Tauri module. A grep lint rule is added to CI to guarantee assembly.rs contains zero tauri imports.

**Primary recommendation:** Execute in strict commit order (arch → impl → refactor migrate → refactor split → docs). Do not merge uc-core changes with uc-tauri changes into a single commit.

---

## Standard Stack

### Core (already present — no new dependencies needed)

| Library                                   | Version  | Purpose                                           | Note                                |
| ----------------------------------------- | -------- | ------------------------------------------------- | ----------------------------------- |
| `uc-core/ports/host_event_emitter.rs`     | Phase 36 | HostEvent enum + HostEventEmitterPort trait       | Extend with new sub-enums           |
| `uc-tauri/adapters/host_event_emitter.rs` | Phase 36 | TauriEventEmitter + LoggingEventEmitter           | Extend with new match arms          |
| `tauri::Emitter`                          | Tauri 2  | app.emit() on AppHandle                           | Stays in wiring.rs only after split |
| `serde_json::Value`                       | current  | erased payload serialization in TauriEventEmitter | Existing pattern                    |
| `Arc<dyn HostEventEmitterPort>`           | —        | Port injection via closure capture                | Established pattern                 |

No new crate dependencies are introduced in this phase.

---

## Architecture Patterns

### Recommended Module Structure After Split

```
src-tauri/crates/uc-tauri/src/bootstrap/
├── assembly.rs           # NEW: pure dependency construction (no tauri imports)
│   ├── WiredDependencies struct
│   ├── wire_dependencies()
│   ├── wire_dependencies_with_identity_store()
│   ├── get_storage_paths()
│   ├── create_infra_layer()
│   ├── resolve_pairing_device_name()
│   └── resolve_pairing_config()
├── wiring.rs             # RETAINED: Tauri event loops + command registration
│   ├── BackgroundRuntimeDeps struct
│   ├── start_background_tasks()  (AppHandle<R> param removed)
│   ├── run_clipboard_receive_loop()
│   ├── run_pairing_event_loop()
│   ├── run_pairing_action_loop()
│   ├── run_space_access_completion_loop()
│   └── create_command_handler() / register_commands()
├── file_transfer_wiring.rs  # Zero Tauri imports after migration
├── mod.rs                # pub use assembly::*; pub use wiring::*
└── ... (other files unchanged)
```

### Pattern 1: HostEvent Sub-Enum Addition (uc-core)

New sub-enums follow the exact same pattern as Phase 36's ClipboardHostEvent, PeerDiscoveryHostEvent, etc. The HostEvent top-level enum gains three new arms:

```rust
// Source: src-tauri/crates/uc-core/src/ports/host_event_emitter.rs
// (Phase 36 established pattern — extend with new arms)

pub enum HostEvent {
    Clipboard(ClipboardHostEvent),
    PeerDiscovery(PeerDiscoveryHostEvent),
    PeerConnection(PeerConnectionHostEvent),
    Transfer(TransferHostEvent),
    // NEW in Phase 37:
    Pairing(PairingHostEvent),
    Setup(SetupHostEvent),
    SpaceAccess(SpaceAccessHostEvent),
}
```

**PairingHostEvent** must cover all 7 `app.emit("p2p-pairing-verification", ...)` call sites. The existing `P2PPairingVerificationEvent` struct in events/p2p_pairing.rs defines the wire contract: `kind` (Request/Verification/Verifying/Complete/Failed), `session_id`, `peer_id`, `device_name`, `code`, `local_fingerprint`, `peer_fingerprint`, `error`. PairingHostEvent variants should carry the same semantic fields — TauriEventEmitter maps them to the existing JSON wire format.

**SetupHostEvent** must cover the 1 `app.emit("setup-state-changed", ...)` call site in `TauriSetupEventPort::emit_setup_state_changed()`. Fields: `state: SetupState`, `session_id: Option<String>`. Note that `SetupState` is a uc-core type (no Tauri dependency), so it can be used directly as a variant field.

**SpaceAccessHostEvent** must cover the 2 `app.emit("space-access-completed", ...)` + `app.emit("p2p-space-access-completed", ...)` calls. Both use the same `SpaceAccessCompletedPayload` struct (fields: session_id, peer_id, success, reason, ts). A single `Completed` variant covers both; TauriEventEmitter emits the event twice with different event name strings, OR the variant can carry a flag indicating which Tauri event name to use. The simplest approach: a single `SpaceAccessHostEvent::Completed { ... }` and TauriEventEmitter emits BOTH event names from one variant.

**PairingSubscriptionHostEvent** (alternatively fold into PairingHostEvent): covers pairing-events-subscribe-failure (attempt, retry_in_ms, error) and pairing-events-subscribe-recovered (recovered_after_attempts).

### Pattern 2: TauriEventEmitter Extension

```rust
// Source: src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs
// (Established pattern: add new private payload DTOs + new match arms in map_event_to_json)

fn map_event_to_json(event: HostEvent) -> (&'static str, serde_json::Value) {
    match event {
        // ... existing arms ...
        HostEvent::Pairing(PairingHostEvent::Request { session_id, peer_id, device_name }) => {
            let payload = PairingVerificationPayload { session_id, kind: "request", peer_id: Some(peer_id), device_name, .. };
            ("p2p-pairing-verification", serde_json::to_value(payload).unwrap_or_default())
        }
        // ... all PairingHostEvent variants ...
        HostEvent::Setup(SetupHostEvent::StateChanged { state, session_id }) => {
            let payload = SetupStateChangedDto { state, session_id };
            ("setup-state-changed", serde_json::to_value(payload).unwrap_or_default())
        }
        HostEvent::SpaceAccess(SpaceAccessHostEvent::Completed { .. }) => {
            // emit twice — but TauriEventEmitter only supports one return value
            // Resolution: emit both in the emit() method body (not map_event_to_json),
            // or emit "space-access-completed" and let wiring also listen for an alias
            // Simplest: extend emit() to handle this special case directly
        }
    }
}
```

**SpaceAccess dual-emit challenge:** `run_space_access_completion_loop` emits both "space-access-completed" AND "p2p-space-access-completed" with identical payloads. The `map_event_to_json` pattern returns a single `(&'static str, Value)` pair. Resolution options:

- Option A: TauriEventEmitter overrides `emit()` for this case to call `self.app.emit()` twice
- Option B: Define two separate SpaceAccessHostEvent variants (SpaceAccessCompleted and P2PSpaceAccessCompleted) emitted separately from the loop — cleaner but requires calling emitter.emit() twice
- Option C: Return `Vec<(&'static str, Value)>` from map_event_to_json — breaks the existing pattern

**Recommended: Option B** — emit two separate events from the loop, matching the existing two-emit pattern. Both share the same payload fields. This keeps `map_event_to_json` returning a single tuple.

### Pattern 3: start_background_tasks Signature After Migration

Current signature (line 1139-1149):

```rust
pub fn start_background_tasks<R: Runtime>(
    background: BackgroundRuntimeDeps,
    deps: &AppDeps,
    app_handle: Option<AppHandle<R>>,   // REMOVED after migration
    event_emitter: Arc<dyn HostEventEmitterPort>,
    pairing_orchestrator: Arc<PairingOrchestrator>,
    pairing_action_rx: mpsc::Receiver<PairingAction>,
    staged_store: Arc<StagedPairedDeviceStore>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    key_slot_store: Arc<dyn KeySlotStore>,
    task_registry: &Arc<TaskRegistry>,
)
```

After migration: `app_handle: Option<AppHandle<R>>` is removed; `<R: Runtime>` generic is removed; all internal functions that currently take `Option<AppHandle<R>>` are updated to take `Arc<dyn HostEventEmitterPort>` (or a clone thereof).

### Pattern 4: file_transfer_wiring.rs Migration

`emit_pending_status` (already migrated in Phase 36) is the reference. All 5 remaining functions follow this pattern:

```rust
// BEFORE (current)
pub async fn handle_transfer_progress<R: tauri::Runtime>(
    tracker: &TrackInboundTransfersUseCase,
    app: Option<&AppHandle<R>>,
    ...
) { /* app.emit("file-transfer://status-changed", payload) */ }

// AFTER
pub async fn handle_transfer_progress(
    tracker: &TrackInboundTransfersUseCase,
    emitter: &dyn HostEventEmitterPort,   // replaces Option<&AppHandle<R>>
    ...
) {
    if let Err(err) = emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged { ... })) {
        warn!(error = %err, "Failed to emit ...");
    }
}
```

`spawn_timeout_sweep` currently takes `app_handle: Option<AppHandle<R>>` and is spawned as a task. After migration it takes `emitter: Arc<dyn HostEventEmitterPort>` — no generic parameter needed.

### Pattern 5: Command Registration Move

Currently in main.rs lines 852-927:

```rust
.invoke_handler(tauri::generate_handler![
    uc_tauri::commands::clipboard::get_clipboard_entries,
    // ... ~60 commands ...
])
```

Move to wiring.rs (or a dedicated `commands_registration.rs` in bootstrap/) as a function:

```rust
// In wiring.rs or bootstrap/commands_registration.rs
pub fn build_invoke_handler() -> impl Fn(tauri::Invoke<tauri::Wry>) + Send + Sync + 'static {
    tauri::generate_handler![
        uc_tauri::commands::clipboard::get_clipboard_entries,
        // ... all commands ...
    ]
}
```

Then in main.rs:

```rust
.invoke_handler(uc_tauri::bootstrap::build_invoke_handler())
```

`tauri::generate_handler!` is a macro that expands to a closure compatible with `.invoke_handler()`. The exact return type is opaque — `impl Fn(tauri::Invoke<Wry>) + Send + Sync` works because `Wry` is the default runtime in the binary. The macro and its output type do NOT need to be in the same file, only the command identifiers must be in scope when the macro expands.

### Pattern 6: mod.rs Re-export After Split

```rust
// bootstrap/mod.rs — add assembly module, keep existing re-exports unchanged
pub mod assembly;
pub mod wiring;
// ... other mods unchanged ...

pub use assembly::{
    get_storage_paths, resolve_pairing_config, resolve_pairing_device_name,
    wire_dependencies, WiredDependencies,
};
pub use wiring::{start_background_tasks, BackgroundRuntimeDeps};
// wire_dependencies_with_identity_store if it exists in assembly.rs
```

External callers (commands/settings.rs, adapters/lifecycle.rs) that currently import from `bootstrap::wiring` or `bootstrap::*` will continue to work unchanged because mod.rs re-exports from both modules under the same `bootstrap::` namespace.

### Pattern 7: TauriSetupEventPort Elimination

Currently in wiring.rs lines 177-209: `TauriSetupEventPort` is a struct that holds `Arc<std::sync::RwLock<Option<AppHandle>>>` and implements `SetupEventPort` by calling `app.emit("setup-state-changed", ...)`.

After migration, this adapts to inject `HostEventEmitterPort` instead:

```rust
struct HostEventSetupPort {
    emitter: Arc<dyn HostEventEmitterPort>,
}

#[async_trait]
impl SetupEventPort for HostEventSetupPort {
    async fn emit_setup_state_changed(&self, state: SetupState, session_id: Option<String>) {
        if let Err(err) = self.emitter.emit(HostEvent::Setup(SetupHostEvent::StateChanged {
            state, session_id
        })) {
            warn!(error = %err, "Failed to emit setup-state-changed");
        }
    }
}
```

This struct can live in wiring.rs (it will be on the Tauri side by usage, but contains no tauri imports itself) OR in assembly.rs since it has no Tauri types. Given that `SetupEventPort` has async semantics and the emitter is injected, it belongs in assembly.rs — the constructor is called during dependency construction.

### Anti-Patterns to Avoid

- **Merge uc-core and uc-tauri changes in one commit:** Violates atomic commit rule + hex boundary
- **Leave `AppHandle<R>` in assembly.rs:** The entire point of the split fails; grep lint catches this
- **Create assembly.rs with Tauri imports for "convenience":** Defeats the structural guarantee for Phase 40
- **Put `WiredDependencies` in wiring.rs:** It is the return type of wire_dependencies, which lives in assembly.rs
- **Put `BackgroundRuntimeDeps` in assembly.rs:** It is only consumed by start_background_tasks, which stays in wiring.rs
- **Dual-emit via a single HostEvent variant that TauriEventEmitter emits twice by calling app.emit twice internally:** This is acceptable but must be explicitly documented. The cleaner approach is two variants.

---

## Don't Hand-Roll

| Problem                               | Don't Build           | Use Instead                                                                        | Why                                               |
| ------------------------------------- | --------------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------- |
| Event name mapping                    | Custom dispatch table | extend `map_event_to_json` in TauriEventEmitter                                    | Established pattern; all event names in one place |
| Dual-event emission for SpaceAccess   | Complex dispatcher    | Two separate variants OR explicit double-emit in TauriEventEmitter.emit()          | Simple, auditable                                 |
| Grep lint for assembly.rs             | Custom build script   | CI `grep -c 'tauri::' assembly.rs` assertion                                       | Sufficient for Phase 37; ratcheted in Phase 40    |
| Test doubles for HostEventEmitterPort | New test framework    | Existing `RecordingEmitter` pattern (already in wiring.rs tests and uc-core tests) | Already established                               |

---

## Common Pitfalls

### Pitfall 1: SpaceAccessCompletedPayload Has serde — Can It Be a HostEvent Field?

**What goes wrong:** `SpaceAccessCompletedPayload` in wiring.rs currently has `#[serde(...)]` annotations. HostEvent fields must be pure Rust types (no serde). If the payload struct is copied directly into the HostEvent variant, it imports serde into uc-core.

**Why it happens:** HostEvent is intentionally serde-free; TauriEventEmitter owns serialization.

**How to avoid:** Define `SpaceAccessHostEvent::Completed { session_id: String, peer_id: String, success: bool, reason: Option<String>, ts: i64 }` with plain Rust types. TauriEventEmitter defines a separate `SpaceAccessCompletedDto` payload struct with `#[serde(rename_all = "camelCase")]`.

**Warning signs:** `use serde` appears in host_event_emitter.rs.

### Pitfall 2: PairingHostEvent Variant Proliferation vs. Flat Struct

**What goes wrong:** The 7 pairing-verification emit sites all use `P2PPairingVerificationEvent` with a `kind` field (Request/Verification/Verifying/Complete/Failed). Modeling each as a separate PairingHostEvent variant leads to 5 variants with overlapping optional fields. Modeling as a single flat variant reduces clarity.

**How to avoid:** Mirror the existing kind-based structure: `PairingHostEvent::Verification { session_id, kind: PairingVerificationKind, peer_id: Option<String>, device_name: Option<String>, code: Option<String>, local_fingerprint: Option<String>, peer_fingerprint: Option<String>, error: Option<String> }`. This maps 1:1 to `P2PPairingVerificationEvent` and TauriEventEmitter simply converts the PairingVerificationKind enum to its serde representation.

Alternatively, define distinct variants per kind — cleaner semantically, more verbose. Both are valid; pick one and be consistent.

### Pitfall 3: resolve_pairing_device_name Import Breakage

**What goes wrong:** After the split, callers of `resolve_pairing_device_name` import from `bootstrap::wiring`. When the function moves to `assembly.rs`, those callers break unless mod.rs re-exports it.

**Why it happens:** Rust module system — moving a function between modules changes its path.

**How to avoid:** Update mod.rs re-exports FIRST before or in the same commit as the file split. Verify: commands/settings.rs and adapters/lifecycle.rs must continue to compile without import changes.

**Known callers (from CONTEXT.md):**

- `src-tauri/crates/uc-tauri/src/commands/settings.rs` lines 4, 140
- `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs` lines 18, 148

### Pitfall 4: `TauriSetupEventPort` Stranded in assembly.rs

**What goes wrong:** `TauriSetupEventPort` (lines 177-209 in wiring.rs) uses `Arc<std::sync::RwLock<Option<AppHandle>>>` — this is a Tauri type. If the split naively moves all "non-event-loop" code to assembly.rs, this struct ends up there with a tauri import.

**How to avoid:** `TauriSetupEventPort` is replaced by `HostEventSetupPort` (uses emitter, no tauri import) as part of the app.emit() migration task. After migration the RwLock-based struct is deleted. assembly.rs gets the new HostEventSetupPort, which has zero tauri imports.

### Pitfall 5: BackgroundRuntimeDeps Misplaced

**What goes wrong:** If `BackgroundRuntimeDeps` ends up in assembly.rs (because the wiring.rs "dependency construction" section currently defines it), then start_background_tasks (which stays in wiring.rs) fails to compile without importing from assembly.

**How to avoid:** CONTEXT.md is explicit: `BackgroundRuntimeDeps` stays in wiring.rs because it is only consumed by `start_background_tasks`. Double-check: `BackgroundRuntimeDeps` fields are all Tauri-free types (Libp2pNetworkAdapter, RepresentationCache, mpsc::Receiver, PathBuf, u64, u32) — it could live in assembly.rs. But since it's only used by start_background_tasks (wiring side), keeping it in wiring.rs avoids an assembly.rs import in wiring.rs for just one struct.

### Pitfall 6: generate_handler! Macro Scope

**What goes wrong:** `tauri::generate_handler![uc_tauri::commands::clipboard::get_clipboard_entries, ...]` requires the command functions to be accessible from the macro expansion site. Moving the macro invocation to wiring.rs requires all `uc_tauri::commands::*` modules to be accessible from there — which they are, since wiring.rs is inside uc-tauri.

**How to avoid:** Ensure the new function that wraps generate_handler! is in a module that can see `crate::commands::*`. Since wiring.rs is at `uc_tauri::bootstrap::wiring`, it can use `crate::commands::clipboard::*` etc. The macro also needs the `tauri` crate in scope. All of this is true for wiring.rs.

### Pitfall 7: RwLock AppHandle in WiredDependencies

**What goes wrong:** `WiredDependencies` includes an `app_handle: Arc<std::sync::RwLock<Option<AppHandle>>>` field currently — if it does, moving the struct to assembly.rs breaks the tauri-free guarantee.

**How to avoid:** Verify WiredDependencies fields before splitting. Current definition (lines 154-158 in wiring.rs):

```rust
pub struct WiredDependencies {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    pub watcher_control: Arc<dyn WatcherControlPort>,
}
```

No AppHandle field — safe to move to assembly.rs as-is. The `Arc<RwLock<Option<AppHandle>>>` is used by `TauriSetupEventPort` separately, not as a field of WiredDependencies.

---

## Code Examples

### Existing Phase 36 Emit Pattern (reference for all new migrations)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs:84-102
pub fn emit_pending_status(
    emitter: &dyn HostEventEmitterPort,
    entry_id: &str,
    pending_transfers: &[PendingTransferLinkage],
) {
    for t in pending_transfers {
        if let Err(err) = emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
            transfer_id: t.transfer_id.clone(),
            entry_id: entry_id.to_string(),
            status: "pending".to_string(),
            reason: None,
        })) {
            warn!(
                error = %err,
                transfer_id = %t.transfer_id,
                "Failed to emit pending file-transfer status"
            );
        }
    }
}
```

### Current app.emit() Pattern (to be replaced)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:3281
fn emit_pairing_events_subscribe_failure<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    attempt: u32,
    retry_in_ms: u64,
    error: String,
) {
    if let Some(app) = app_handle {
        let payload = PairingEventsSubscribeFailurePayload { attempt, retry_in_ms, error };
        if let Err(emit_err) = app.emit("pairing-events-subscribe-failure", payload) {
            warn!(error = %emit_err, "Failed to emit pairing events subscribe failure event");
        }
    }
}
```

After migration:

```rust
fn emit_pairing_events_subscribe_failure(
    emitter: &dyn HostEventEmitterPort,
    attempt: u32,
    retry_in_ms: u64,
    error: String,
) {
    if let Err(err) = emitter.emit(HostEvent::Pairing(PairingHostEvent::SubscribeFailure {
        attempt, retry_in_ms, error
    })) {
        warn!(error = %err, "Failed to emit pairing events subscribe failure event");
    }
}
```

### TauriEventEmitter map_event_to_json Extension Pattern

```rust
// Source: src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs:171-366
// Add new private DTO structs at module top, then add match arms:

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingVerificationPayload {
    session_id: String,
    kind: String,           // "request" | "verification" | "verifying" | "complete" | "failed"
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_name: Option<String>,
    // ... other optional fields ...
}

// In map_event_to_json match:
HostEvent::Pairing(PairingHostEvent::SubscribeFailure { attempt, retry_in_ms, error }) => {
    // private DTO struct with camelCase
    ("pairing-events-subscribe-failure", serde_json::to_value(dto).unwrap_or_default())
}
```

### Contract Test Pattern for New Events

```rust
// Pattern from src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs:575-605
#[tokio::test]
async fn test_pairing_subscribe_failure_event_contract() {
    let app = tauri::test::mock_app();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);
    app.handle().listen("pairing-events-subscribe-failure", move |event: tauri::Event| {
        let _ = tx.try_send(event.payload().to_string());
    });
    let emitter = TauriEventEmitter::new(app.handle().clone());
    emitter.emit(HostEvent::Pairing(PairingHostEvent::SubscribeFailure {
        attempt: 3,
        retry_in_ms: 2000,
        error: "subscribe failed".to_string(),
    })).expect("emit");
    let payload = rx.recv().await.expect("event payload");
    let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(json["attempt"], 3);
    assert_eq!(json["retryInMs"], 2000);  // camelCase
    assert_eq!(json["error"], "subscribe failed");
    assert!(json.get("retry_in_ms").is_none());  // no snake_case
}
```

---

## State of the Art

| Old Approach                                      | Current Approach                   | When Changed          | Impact                                       |
| ------------------------------------------------- | ---------------------------------- | --------------------- | -------------------------------------------- |
| app.emit() directly in event loops                | HostEventEmitterPort::emit()       | Phase 36 (partial)    | Decouples event loops from Tauri runtime     |
| All wiring in one 6328-line file                  | assembly.rs + wiring.rs split      | Phase 37 (this phase) | Enables future uc-bootstrap crate extraction |
| AppHandle<R> as R: Runtime generic in event loops | Pure function / Arc<dyn Port>      | Phase 37 (this phase) | Enables non-Tauri runtime modes              |
| Command registration in main.rs                   | In Tauri-specific bootstrap module | Phase 37 (this phase) | main.rs becomes thin entry point             |

---

## Open Questions

1. **SpaceAccess dual-emit: one variant or two?**
   - What we know: `run_space_access_completion_loop` calls `app.emit("space-access-completed", ...)` and `app.emit("p2p-space-access-completed", ...)` with identical payloads
   - What's unclear: Whether the frontend listens on both event names independently (legacy compatibility) or only one
   - Recommendation: Define two separate HostEvent variants (`SpaceAccessHostEvent::Completed` and `SpaceAccessHostEvent::P2PCompleted`) for semantic clarity. Alternatively, a single variant with TauriEventEmitter making two app.emit() calls — acceptable since this is adapter-layer logic, but unconventional relative to the 1:1 variant-to-event-name mapping everywhere else.

2. **PairingVerificationKind enum in PairingHostEvent — in uc-core or uc-tauri?**
   - What we know: The kind enum (Request/Verification/Verifying/Complete/Failed) is pure semantic; `P2PPairingVerificationKind` already exists in uc-tauri/events/p2p_pairing.rs with serde annotations
   - What's unclear: Should a serde-free version of PairingVerificationKind be defined in uc-core as part of PairingHostEvent, or should PairingHostEvent carry a plain string kind field?
   - Recommendation: Define `PairingVerificationKind` as a Rust enum (no serde) in uc-core alongside PairingHostEvent. TauriEventEmitter maps it to the string representation.

3. **Where does `build_invoke_handler` live?**
   - Options: (a) directly in wiring.rs as an exported function, (b) new `commands_registration.rs` in bootstrap/
   - Recommendation: If the function is small (just the macro call), inline in wiring.rs. If it grows (conditional compilation for platform-specific commands), extract to `commands_registration.rs`. The macro call is ~76 lines; wiring.rs is already large. A dedicated file improves discoverability and keeps wiring.rs focused on event loops.

---

## Validation Architecture

### Test Framework

| Property           | Value                                                             |
| ------------------ | ----------------------------------------------------------------- |
| Framework          | Rust built-in test + tokio::test                                  |
| Config file        | src-tauri/Cargo.toml (test profiles)                              |
| Quick run command  | `cd src-tauri && cargo test -p uc-core`                           |
| Full suite command | `cd src-tauri && cargo test -p uc-core && cargo test -p uc-tauri` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                             | Test Type            | Automated Command                                                                       | File Exists?                              |
| ------- | ---------------------------------------------------- | -------------------- | --------------------------------------------------------------------------------------- | ----------------------------------------- |
| RNTM-02 | assembly.rs has zero tauri imports                   | lint/grep            | `grep -c 'tauri::' src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` should output 0 | ❌ Wave 0 (assembly.rs doesn't exist yet) |
| RNTM-02 | New HostEvent sub-enums compile in uc-core           | cargo check          | `cd src-tauri && cargo check -p uc-core`                                                | ✅ (uc-core exists)                       |
| RNTM-02 | TauriEventEmitter maps new events correctly          | contract test        | `cd src-tauri && cargo test -p uc-tauri test_pairing_`                                  | ❌ Wave 0 (tests don't exist yet)         |
| RNTM-02 | file_transfer_wiring.rs has zero tauri imports       | lint/grep            | `grep -c 'tauri::' src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs`     | ✅ (file exists, will become 0)           |
| RNTM-02 | start_background_tasks compiles without AppHandle<R> | cargo check          | `cd src-tauri && cargo check -p uc-tauri`                                               | ✅ (will compile after refactor)          |
| RNTM-02 | Existing GUI behavior unchanged                      | integration / manual | `bun tauri dev` + clipboard + pairing manual test                                       | manual-only                               |
| RNTM-02 | LoggingEventEmitter handles new variants             | unit test            | `cd src-tauri && cargo test -p uc-tauri test_logging_emitter`                           | ❌ Wave 0 (new variants not yet tested)   |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check -p uc-core` (after arch commit) or `cargo check -p uc-tauri` (after impl/refactor commits)
- **Per wave merge:** `cd src-tauri && cargo test -p uc-core && cargo test -p uc-tauri`
- **Phase gate:** Full suite green + grep lint passes before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` — doesn't exist yet; created in commit 4
- [ ] New contract tests for PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent in `adapters/host_event_emitter.rs` — covers RNTM-02 event contract verification
- [ ] New unit tests for LoggingEventEmitter with new HostEvent variants in `adapters/host_event_emitter.rs`
- [ ] CI grep lint rule for assembly.rs — verify zero tauri imports

---

## Detailed Emit Site Inventory

All 14 `app.emit()` calls in wiring.rs that must be migrated:

| Line                       | Current Event Name                   | Target HostEvent Variant                                | Notes                                              |
| -------------------------- | ------------------------------------ | ------------------------------------------------------- | -------------------------------------------------- |
| 204                        | `setup-state-changed`                | `SetupHostEvent::StateChanged`                          | Inside TauriSetupEventPort (struct to be replaced) |
| 2098                       | `space-access-completed`             | `SpaceAccessHostEvent::Completed`                       | In run_space_access_completion_loop                |
| 2102                       | `p2p-space-access-completed`         | `SpaceAccessHostEvent::P2PCompleted`                    | Same loop, identical payload                       |
| 2652                       | `p2p-pairing-verification`           | `PairingHostEvent::Verification { kind: Request }`      | In handle_pairing_message                          |
| 3091                       | `p2p-pairing-verification`           | `PairingHostEvent::Verification { kind: Verification }` | ShowVerification action                            |
| 3105                       | `p2p-pairing-verification`           | `PairingHostEvent::Verification { kind: Verifying }`    | ShowVerifying action                               |
| 3222                       | `p2p-pairing-verification`           | `PairingHostEvent::Verification { kind: Complete }`     | EmitResult success                                 |
| 3230                       | `p2p-pairing-verification`           | `PairingHostEvent::Verification { kind: Failed }`       | EmitResult failure                                 |
| 3253                       | `p2p-pairing-verification`           | `PairingHostEvent::Verification { kind: Failed }`       | signal_pairing_transport_failure                   |
| 3281                       | `pairing-events-subscribe-failure`   | `PairingHostEvent::SubscribeFailure`                    | emit_pairing_events_subscribe_failure fn           |
| 3295                       | `pairing-events-subscribe-recovered` | `PairingHostEvent::SubscribeRecovered`                  | emit_pairing_events_subscribe_recovered fn         |
| +(inbound clipboard error) | `inbound-clipboard-subscribe-error`  | already handled by ClipboardHostEvent?                  | Verify — may already be migrated                   |
| +(inbound clipboard retry) | `inbound-clipboard-subscribe-retry`  | already handled?                                        | Verify — may already be migrated                   |

**Note:** CONTEXT.md says 14 remaining calls including "inbound-clipboard-subscribe-error/retry (2)". The grep result shows 11 explicit `app.emit()` calls. The remaining 3 may be in helper functions or use a different call pattern. Implementer should re-run the grep and audit all `app.emit` / `app_handle.emit` occurrences before starting migration.

5 `AppHandle<R>` functions in file_transfer_wiring.rs to migrate:

- `handle_transfer_progress` (lines 111-163)
- `handle_transfer_completed` (lines 170-224)
- `handle_transfer_failed` (lines 230-257)
- `spawn_timeout_sweep` (lines 263-...) — returns JoinHandle, captures app_handle
- `reconcile_on_startup` (function below spawn_timeout_sweep)

---

## Sources

### Primary (HIGH confidence)

- Direct code read: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — structure, AppHandle usage, emit sites, struct definitions
- Direct code read: `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` — 5 AppHandle<R> functions
- Direct code read: `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` — HostEvent enum design, HostEventEmitterPort trait
- Direct code read: `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` — TauriEventEmitter pattern, map_event_to_json, LoggingEventEmitter, contract tests
- Direct code read: `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — current re-exports
- Direct code read: `src-tauri/src/main.rs:852-927` — invoke_handler! block
- Direct code read: `src-tauri/crates/uc-tauri/src/events/p2p_pairing.rs` — P2PPairingVerificationEvent wire contract
- CONTEXT.md Phase 37 — locked decisions, emit site inventory, commit strategy

### Secondary (MEDIUM confidence)

- AGENTS.md (root + uc-tauri + uc-core) — atomic commit rule, hex boundary, anti-patterns
- STATE.md + REQUIREMENTS.md — RNTM-02 definition, predecessor phase context

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all code read directly, no external libraries needed
- Architecture: HIGH — split boundary defined in CONTEXT.md, patterns verified in existing code
- Pitfalls: HIGH — identified from actual code structure, not speculation
- Emit inventory: MEDIUM — grep found 11 explicit calls vs. CONTEXT.md claim of 14; implementer must verify the remaining 3

**Research date:** 2026-03-17
**Valid until:** 2026-04-17 (stable — pure refactor, no external dependencies)
