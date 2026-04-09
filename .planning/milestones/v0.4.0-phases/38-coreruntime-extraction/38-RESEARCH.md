# Phase 38: CoreRuntime Extraction - Research

**Researched:** 2026-03-18
**Domain:** Rust crate architecture / hexagonal architecture refactoring
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- CoreRuntime holds: AppDeps, event_emitter (RwLock<Arc<dyn HostEventEmitterPort>>), lifecycle_status, setup_orchestrator, clipboard_integration_mode, task_registry, storage_paths
- AppRuntime holds: CoreRuntime + app_handle (Arc<RwLock<Option<AppHandle>>>) + watcher_control (Arc<dyn WatcherControlPort>)
- CoreRuntime lives in uc-app crate (natural home alongside AppDeps and SetupOrchestrator)
- TaskRegistry must move to uc-app first — add tokio-util (with sync feature) to uc-app/Cargo.toml
- Stale emitter fix is MANDATORY — use Option (A): HostEventSetupPort holds Arc<RwLock<Arc<dyn HostEventEmitterPort>>> (shared cell read-through)
- SetupOrchestrator assembly moved from runtime.rs build_setup_orchestrator to assembly.rs as standalone function
- All adapters passed as Arc<dyn Port> parameters (parameterized injection); new SetupAssemblyPorts struct replaces SetupRuntimePorts
- Move to uc-app (NOT uc-infra): InMemoryLifecycleStatus, LoggingLifecycleEventEmitter, LoggingSessionReadyEmitter, DeviceNameAnnouncer
- DeviceNameAnnouncer must inline resolve_pairing_device_name logic when moving to uc-app
- Two-tier UseCases: CoreUseCases (uc-app, ~35 pure domain accessors) + AppUseCases (uc-tauri, wraps CoreUseCases)
- Three accessors stay in AppUseCases (uc-tauri): apply_autostart(), start_clipboard_watcher(), app_lifecycle_coordinator()
- runtime.usecases() returns AppUseCases which transparently exposes all CoreUseCases methods
- TauriSessionReadyEmitter stays in uc-tauri (depends on AppHandle)
- assembly.rs NOT moved out of uc-tauri (Phase 40)
- No new workspace crate (Phase 40)
- Command layer code unchanged (transparent proxy)
- SC#4 integration coverage: at least one Rust-side integration test verifying setup-state emission through mock emitter

### Claude's Discretion

- Exact CoreRuntime field ordering and constructor signature
- Whether SetupAssemblyPorts lives in assembly.rs or a dedicated module
- Whether AppUseCases uses Deref<Target=CoreUseCases> or explicit proxy methods
- Commit split granularity (minimum: uc-app CoreRuntime+CoreUseCases, uc-tauri AppRuntime wrapper+AppUseCases, assembly.rs setup migration, adapter moves)
- Internal implementation of AppRuntime proxy methods
- Exact module location for moved adapters within uc-app (e.g., usecases/app_lifecycle/adapters.rs or adapters/ top-level)

### Deferred Ideas (OUT OF SCOPE)

- Move assembly.rs to uc-bootstrap crate — Phase 40
- Create uc-bootstrap as sole composition root — Phase 40
- Daemon/CLI entry points using CoreRuntime directly — Phase 41
- Move remaining Tauri adapters out of uc-tauri — future phases
- Relocate lifecycle traits from uc-app to uc-core if uc-infra adapters are ever needed — future cleanup
- Split CoreUseCases into domain-specific accessor groups if it grows too large — future cleanup
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                       | Research Support                                                                                                                                                      |
| ------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RNTM-01 | CoreRuntime struct exists without Tauri dependency, holding AppDeps and shared orchestrators      | Fully supported: AppDeps is already in uc-app with zero Tauri deps; TaskRegistry is Tauri-free (only tokio/tokio_util/tracing); all lifecycle ports defined in uc-app |
| RNTM-05 | SetupOrchestrator assembly unified into main composition root (no secondary wiring in runtime.rs) | Fully supported: assembly.rs is already a pure Rust module with zero Tauri imports; build_setup_orchestrator in runtime.rs is a standalone-extractable function       |

</phase_requirements>

---

## Summary

Phase 38 extracts a `CoreRuntime` struct from `AppRuntime` in `uc-tauri`. The goal is to isolate all non-Tauri runtime state — `AppDeps`, `SetupOrchestrator`, `lifecycle_status`, `event_emitter`, `task_registry`, `clipboard_integration_mode`, and `storage_paths` — into a struct that lives in `uc-app` and passes `cargo check -p uc-app` independently. `AppRuntime` becomes a thin Tauri wrapper holding only `app_handle` and `watcher_control`.

The work decomposes into four tightly coupled moves: (1) migrate `TaskRegistry` to `uc-app` as a prerequisite, (2) move four lifecycle adapters from `uc-tauri/adapters/lifecycle.rs` to `uc-app`, (3) create `CoreRuntime` in `uc-app` and make `AppRuntime` wrap it, and (4) extract `build_setup_orchestrator` from `runtime.rs` to `assembly.rs`. A mandatory stale-emitter fix must accompany step 4: `HostEventSetupPort` must hold a shared `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` cell instead of a snapshot `Arc`.

The codebase is in a clean state for this extraction. `assembly.rs` already has zero Tauri imports. `AppDeps` is already in `uc-app`. All four lifecycle port traits are already defined in `uc-app/src/usecases/app_lifecycle/mod.rs`. The only cross-cutting dependency to cut is `DeviceNameAnnouncer`'s call to `resolve_pairing_device_name` (a 15-line function in `assembly.rs` depending only on `SettingsPort`).

**Primary recommendation:** Execute the migration as four sequential commits — TaskRegistry move, adapter moves, CoreRuntime creation + AppRuntime thinning, assembly.rs extraction + stale-emitter fix — verifying `cargo check -p uc-app` after each commit.

---

## Standard Stack

### Core

| Library     | Version  | Purpose                                         | Why Standard                                   |
| ----------- | -------- | ----------------------------------------------- | ---------------------------------------------- |
| tokio       | 1 (full) | Async runtime — already in uc-app               | tokio::sync::Mutex used by TaskRegistry        |
| tokio-util  | 0.7      | CancellationToken for TaskRegistry              | Currently in uc-tauri; must be added to uc-app |
| async-trait | 0.1      | Async trait implementations for lifecycle ports | Already in uc-app                              |
| tracing     | 0.1.44   | Logging in moved adapters                       | Already in uc-app                              |

### Supporting

| Library           | Version | Purpose                                                   | When to Use                                     |
| ----------------- | ------- | --------------------------------------------------------- | ----------------------------------------------- |
| std::sync::RwLock | stdlib  | Interior mutability for event_emitter swap in CoreRuntime | Sync RwLock matches existing AppRuntime pattern |
| std::sync::Arc    | stdlib  | Shared ownership of port implementations                  | All ports use Arc<dyn Port> pattern             |

**Installation (uc-app/Cargo.toml addition):**

```toml
tokio-util = { version = "0.7", features = ["sync"] }
```

**Version verification:** tokio-util 0.7 is the version already used in uc-tauri; no version bump needed.

---

## Architecture Patterns

### Recommended Project Structure (after Phase 38)

```
uc-app/src/
├── deps.rs                       # AppDeps (unchanged)
├── app_paths.rs                  # AppPaths (unchanged)
├── runtime.rs                    # NEW: CoreRuntime struct
├── task_registry.rs              # MOVED from uc-tauri/bootstrap/
├── usecases/
│   ├── mod.rs                    # CoreUseCases struct + accessors (~35 pure domain)
│   └── app_lifecycle/
│       ├── mod.rs                # Port traits (unchanged)
│       └── adapters.rs           # MOVED: InMemoryLifecycleStatus, LoggingLifecycleEventEmitter,
│                                 #        LoggingSessionReadyEmitter, DeviceNameAnnouncer

uc-tauri/src/bootstrap/
├── runtime.rs                    # THINNED: AppRuntime wraps Arc<CoreRuntime>; AppUseCases wrapper
├── assembly.rs                   # GAINS: build_setup_orchestrator() standalone fn; SetupAssemblyPorts
├── wiring.rs                     # SetupRuntimePorts deprecated/removed; HostEventSetupPort refactored
├── adapters/
│   └── lifecycle.rs              # REDUCED: Only TauriSessionReadyEmitter stays
```

### Pattern 1: CoreRuntime in uc-app

**What:** A Tauri-free struct holding all non-Tauri runtime state.
**When to use:** Any shared runtime state that does not depend on `tauri::AppHandle` or `uc-platform::ports::WatcherControlPort`.

```rust
// In uc-app/src/runtime.rs
use std::sync::Arc;
use crate::deps::AppDeps;
use crate::usecases::setup::SetupOrchestrator;
use crate::usecases::LifecycleStatusPort;
use crate::app_paths::AppPaths;
use crate::task_registry::TaskRegistry;
use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::ports::host_event_emitter::HostEventEmitterPort;

pub struct CoreRuntime {
    pub(crate) deps: AppDeps,
    pub(crate) event_emitter: std::sync::RwLock<Arc<dyn HostEventEmitterPort>>,
    pub(crate) lifecycle_status: Arc<dyn LifecycleStatusPort>,
    pub(crate) setup_orchestrator: Arc<SetupOrchestrator>,
    pub(crate) clipboard_integration_mode: ClipboardIntegrationMode,
    pub(crate) task_registry: Arc<TaskRegistry>,
    pub(crate) storage_paths: AppPaths,
}
```

### Pattern 2: AppRuntime as thin Tauri wrapper

**What:** AppRuntime holds `Arc<CoreRuntime>` plus only Tauri-specific fields.
**When to use:** Any field that genuinely requires `tauri::AppHandle` or platform-layer watcher control.

```rust
// In uc-tauri/src/bootstrap/runtime.rs
use uc_app::CoreRuntime;

pub struct AppRuntime {
    core: Arc<CoreRuntime>,
    app_handle: Arc<std::sync::RwLock<Option<tauri::AppHandle>>>,
    watcher_control: Arc<dyn uc_platform::ports::WatcherControlPort>,
}

impl AppRuntime {
    pub fn usecases(&self) -> AppUseCases<'_> {
        AppUseCases::new(self)
    }
    // All facade methods delegate to self.core
    pub fn device_id(&self) -> String { self.core.device_id() }
    pub fn is_encryption_ready(&self) -> impl Future<Output = bool> + '_ { self.core.is_encryption_ready() }
    pub fn settings_port(&self) -> Arc<dyn SettingsPort> { self.core.settings_port() }
}
```

### Pattern 3: Shared RwLock cell for stale emitter fix (MANDATORY)

**What:** `HostEventSetupPort` reads from the shared `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` cell on each emit call, instead of capturing a snapshot Arc at construction time.
**When to use:** Any adapter that wraps the event emitter and must survive the LoggingEmitter → TauriEmitter bootstrap swap.

```rust
// In uc-tauri/src/bootstrap/assembly.rs (HostEventSetupPort refactored)
#[derive(Clone)]
pub struct HostEventSetupPort {
    // CHANGED: holds shared cell, not snapshot Arc
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
}

impl HostEventSetupPort {
    pub fn new(emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>) -> Self {
        Self { emitter_cell }
    }
}

#[async_trait::async_trait]
impl SetupEventPort for HostEventSetupPort {
    async fn emit_setup_state_changed(&self, state: SetupState, session_id: Option<String>) {
        let emitter = self.emitter_cell
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Err(err) = emitter.emit(HostEvent::Setup(SetupHostEvent::StateChanged { state, session_id })) {
            warn!(error = %err, "Failed to emit setup-state-changed");
        }
    }
}
```

This means `CoreRuntime::event_emitter` field type becomes `Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>` (shared ownership of the lock cell), and `HostEventSetupPort` receives a clone of this `Arc<RwLock<...>>` at construction time.

### Pattern 4: Two-tier UseCases split

**What:** CoreUseCases lives in uc-app and exposes ~35 pure domain accessors. AppUseCases lives in uc-tauri, wraps CoreUseCases, and adds three Tauri-dependent accessors.
**When to use:** This split is mandatory because three accessors cannot move without dragging Tauri deps into uc-app.

```rust
// In uc-app/src/usecases/mod.rs
pub struct CoreUseCases<'a> {
    runtime: &'a CoreRuntime,
}

impl<'a> CoreUseCases<'a> {
    pub fn list_clipboard_entries(&self) -> ListClipboardEntries { ... }
    pub fn setup_orchestrator(&self) -> Arc<SetupOrchestrator> { ... }
    pub fn get_lifecycle_status(&self) -> Arc<dyn LifecycleStatusPort> { ... }
    // ... ~35 pure domain accessors
}

// In uc-tauri/src/bootstrap/runtime.rs
pub struct AppUseCases<'a> {
    app_runtime: &'a AppRuntime,
    core: CoreUseCases<'a>,  // or Deref, at discretion
}

impl<'a> AppUseCases<'a> {
    // Three Tauri-dependent accessors
    pub fn apply_autostart(&self) -> Option<ApplyAutostartSetting<TauriAutostart>> { ... }
    pub fn start_clipboard_watcher(&self) -> StartClipboardWatcher { ... }
    pub fn app_lifecycle_coordinator(&self) -> Arc<AppLifecycleCoordinator> { ... }

    // All CoreUseCases proxied (explicit or via Deref)
    pub fn list_clipboard_entries(&self) -> ListClipboardEntries {
        self.core.list_clipboard_entries()
    }
    // ...
}
```

### Pattern 5: SetupOrchestrator assembly as standalone function

**What:** `build_setup_orchestrator` extracted from `AppRuntime` impl block to a free function in `assembly.rs`.
**Parameters:** Takes `&AppDeps` and a `SetupAssemblyPorts` bundle containing all adapters that `runtime.rs` currently constructs inline.

```rust
// In uc-tauri/src/bootstrap/assembly.rs

pub struct SetupAssemblyPorts {
    pub pairing_orchestrator: Arc<PairingOrchestrator>,
    pub space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    pub discovery_port: Arc<dyn DiscoveryPort>,
    // TauriSessionReadyEmitter passed as trait object
    pub session_ready_emitter: Arc<dyn SessionReadyEmitter>,
    // DeviceNameAnnouncer passed as trait object (now in uc-app)
    pub device_announcer: Option<Arc<dyn DeviceAnnouncer>>,
    pub lifecycle_status: Arc<dyn LifecycleStatusPort>,
    pub lifecycle_emitter: Arc<dyn LifecycleEventEmitter>,
    pub watcher_control: Arc<dyn StartClipboardWatcherPort>,
    // Shared emitter cell for HostEventSetupPort (stale-emitter fix)
    pub emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    pub clipboard_integration_mode: ClipboardIntegrationMode,
}

pub fn build_setup_orchestrator(
    deps: &AppDeps,
    ports: SetupAssemblyPorts,
) -> Arc<SetupOrchestrator> {
    // ... extracted from runtime.rs AppRuntime::build_setup_orchestrator
}
```

### Anti-Patterns to Avoid

- **Constructing AppLifecycleCoordinator inside build_setup_orchestrator with inline Tauri adapters**: Move all Tauri-specific adapter construction (TauriSessionReadyEmitter, DeviceNameAnnouncer) outside the function. Pass them as `Arc<dyn Port>` parameters via `SetupAssemblyPorts`. The function in `assembly.rs` must remain zero-Tauri.
- **Snapshot Arc in HostEventSetupPort**: The old `HostEventSetupPort::new(event_emitter: Arc<dyn ...>)` pattern causes the stale emitter bug. Never clone the Arc value out of the RwLock at construction time.
- **Direct field access from command layer**: Commands must not access `runtime.core.deps.*` directly. All access goes through `runtime.usecases().xxx()` or the facade methods.
- **Moving InMemoryLifecycleStatus to uc-infra**: The four lifecycle traits are defined in uc-app; uc-infra cannot depend on uc-app (layering rule).

---

## Don't Hand-Roll

| Problem                      | Don't Build                           | Use Instead                                         | Why                                                                  |
| ---------------------------- | ------------------------------------- | --------------------------------------------------- | -------------------------------------------------------------------- |
| Async cancellation           | Custom boolean flags + Mutex          | `tokio_util::sync::CancellationToken`               | Already used by TaskRegistry; child tokens, cooperative cancellation |
| Interior mutability for swap | Unsafe / Cell / RefCell               | `std::sync::RwLock<Arc<dyn Port>>`                  | Existing project pattern; Send+Sync safe                             |
| Task join/abort              | Manual Vec<JoinHandle>                | `tokio::task::JoinSet`                              | Already used by TaskRegistry; bounded join + abort_all               |
| Transparent proxy            | Re-implementing every accessor method | `Deref<Target=CoreUseCases>` or explicit delegation | Compiler-enforced consistency                                        |

**Key insight:** The project already has the correct building blocks. TaskRegistry, the RwLock swap pattern, and the port injection pattern are all established — this phase reorganizes existing code, it does not invent new patterns.

---

## Common Pitfalls

### Pitfall 1: Forgetting tokio-util in uc-app/Cargo.toml

**What goes wrong:** `cargo check -p uc-app` fails with "can't find crate `tokio_util`" after moving TaskRegistry.
**Why it happens:** uc-app currently does not depend on `tokio-util`. uc-tauri has it at version `0.7`.
**How to avoid:** Add `tokio-util = { version = "0.7", features = ["sync"] }` to `uc-app/Cargo.toml` in the same commit that moves `task_registry.rs`.
**Warning signs:** Compile error mentioning `tokio_util::sync::CancellationToken`.

### Pitfall 2: DeviceNameAnnouncer import from crate::bootstrap

**What goes wrong:** Moving `DeviceNameAnnouncer` to uc-app while it still calls `crate::bootstrap::resolve_pairing_device_name` creates a circular/reverse dependency.
**Why it happens:** `resolve_pairing_device_name` is currently defined in `assembly.rs` (uc-tauri). Without inlining, uc-app would depend on uc-tauri.
**How to avoid:** Inline the 15-line function body (or a copy of it) into `DeviceNameAnnouncer::announce()`. Include the `DEFAULT_PAIRING_DEVICE_NAME` constant in the same file. The function only depends on `SettingsPort` (uc-core trait).
**Warning signs:** Compile error: "use of undeclared crate or module `crate::bootstrap`" when building uc-app.

### Pitfall 3: HostEventSetupPort still holds snapshot Arc (stale emitter bug survives)

**What goes wrong:** Setup state changes emitted from spawned listener tasks (e.g., `ProcessingJoinSpace → JoinSpaceConfirmPeer`) never reach the frontend even after the refactor.
**Why it happens:** If the signature `HostEventSetupPort::new(emitter: Arc<dyn HostEventEmitterPort>)` is preserved, the snapshot-capture bug is not fixed — only the assembly location changed.
**How to avoid:** Change the field type to `Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>`. Pass the shared cell (the same one `CoreRuntime::event_emitter` holds) to `HostEventSetupPort::new()`. Read from the cell on each `emit_setup_state_changed()` call.
**Warning signs:** UAT failure: PeerB setup UI does not advance from ProcessingJoinSpace to JoinSpaceConfirmPeer.

### Pitfall 4: AppRuntime::new() and AppRuntime::with_setup() call sites in main.rs

**What goes wrong:** After splitting into CoreRuntime + AppRuntime, the call sites in `main.rs` that construct `AppRuntime` need updating. Any call to `AppRuntime::with_setup()` must now construct `CoreRuntime` first.
**Why it happens:** The constructor signature changes fundamentally.
**How to avoid:** Audit `main.rs` for all `AppRuntime::new()` and `AppRuntime::with_setup()` calls before writing the new constructor. `main.rs:559-561` and the `.setup()` callback at `main.rs:673-677` are the primary call sites.
**Warning signs:** Compile errors in `main.rs` or `src-tauri/src/main.rs` referencing AppRuntime constructors.

### Pitfall 5: TaskRegistry .manage() call breaks after crate move

**What goes wrong:** `main.rs:600` calls `.manage(task_registry.clone())` on the Tauri builder. After TaskRegistry moves to uc-app, the import path changes.
**Why it happens:** Import was previously `use uc_tauri::bootstrap::task_registry::TaskRegistry`.
**How to avoid:** Update the import in `main.rs` to `use uc_app::task_registry::TaskRegistry` (or whatever module path is chosen). The `.manage()` call itself remains valid — Tauri accepts any `Send + Sync + 'static` type.
**Warning signs:** Compile error: "use of undeclared crate" for TaskRegistry in main.rs.

### Pitfall 6: CoreUseCases lifetime coupling

**What goes wrong:** `CoreUseCases<'a>` borrows `&'a CoreRuntime`, but `AppUseCases<'a>` borrows `&'a AppRuntime`. If AppUseCases stores a `CoreUseCases` field, the lifetimes must be compatible.
**Why it happens:** Rust lifetime inference doesn't always resolve nested borrows automatically.
**How to avoid:** Use explicit lifetime annotations. `AppUseCases<'a>` holds `&'a AppRuntime` and constructs `CoreUseCases { runtime: &self.app_runtime.core }` on demand (or stores it as `CoreUseCases<'a>` with the same `'a`).
**Warning signs:** Lifetime errors when implementing AppUseCases proxy methods.

---

## Code Examples

### TaskRegistry move (no logic change, only location)

```rust
// NEW LOCATION: uc-app/src/task_registry.rs
// Content identical to uc-tauri/src/bootstrap/task_registry.rs
// Only change: file path and crate::... import in main.rs

// uc-app/Cargo.toml add:
// tokio-util = { version = "0.7", features = ["sync"] }

// main.rs import update:
use uc_app::task_registry::TaskRegistry;  // was: uc_tauri::bootstrap::task_registry::TaskRegistry
```

### DeviceNameAnnouncer inlined resolve logic

```rust
// In uc-app/src/usecases/app_lifecycle/adapters.rs
const DEFAULT_PAIRING_DEVICE_NAME: &str = "Uniclipboard Device";

// Inlined from assembly.rs:resolve_pairing_device_name
async fn resolve_pairing_device_name(settings: Arc<dyn SettingsPort>) -> String {
    match settings.load().await {
        Ok(s) => {
            let name = s.general.device_name.unwrap_or_default();
            if name.trim().is_empty() {
                DEFAULT_PAIRING_DEVICE_NAME.to_string()
            } else {
                name
            }
        }
        Err(err) => {
            warn!(error = %err, "Failed to load settings for pairing device name");
            DEFAULT_PAIRING_DEVICE_NAME.to_string()
        }
    }
}

#[async_trait]
impl DeviceAnnouncer for DeviceNameAnnouncer {
    async fn announce(&self) -> Result<()> {
        let device_name = resolve_pairing_device_name(self.settings.clone()).await;
        self.network.announce_device_name(device_name).await
    }
}
```

### CoreRuntime event_emitter accessor (shared cell pattern)

```rust
// CoreRuntime exposes the shared cell for HostEventSetupPort construction
impl CoreRuntime {
    /// Returns a clone of the shared emitter cell.
    /// Used by assembly.rs to construct HostEventSetupPort with read-through.
    pub fn emitter_cell(&self) -> Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>> {
        self.event_emitter.clone()  // event_emitter IS the Arc<RwLock<...>> cell
    }

    /// Returns the current emitter value (clones the inner Arc).
    pub fn event_emitter(&self) -> Arc<dyn HostEventEmitterPort> {
        self.emitter_cell()
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Swap the emitter. Called from Tauri setup callback.
    pub fn set_event_emitter(&self, emitter: Arc<dyn HostEventEmitterPort>) {
        *self.emitter_cell()
            .write()
            .unwrap_or_else(|p| p.into_inner()) = emitter;
    }
}
```

Note: `CoreRuntime::event_emitter` field type changes to `Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>` (the RwLock is inside the Arc, making the cell itself cheaply cloneable/shareable). This differs slightly from the current `AppRuntime` pattern (`std::sync::RwLock<Arc<dyn ...>>` without outer Arc) — the outer `Arc` is needed so `HostEventSetupPort` can hold a reference to the same cell.

### Integration test for stale emitter fix (SC#4)

```rust
// In uc-app/src/runtime.rs or a tests/ module
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    struct RecordingEmitter {
        events: Arc<StdMutex<Vec<HostEvent>>>,
    }
    impl HostEventEmitterPort for RecordingEmitter {
        fn emit(&self, event: HostEvent) -> Result<(), anyhow::Error> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    #[tokio::test]
    async fn setup_state_emission_survives_emitter_swap() {
        let initial_emitter: Arc<dyn HostEventEmitterPort> =
            Arc::new(crate::adapters::host_event_emitter::LoggingEventEmitter);
        let cell = Arc::new(std::sync::RwLock::new(initial_emitter));

        let setup_port = HostEventSetupPort::new(cell.clone());

        let events = Arc::new(StdMutex::new(vec![]));
        let recording_emitter: Arc<dyn HostEventEmitterPort> =
            Arc::new(RecordingEmitter { events: events.clone() });

        // Simulate the Tauri emitter swap
        *cell.write().unwrap() = recording_emitter;

        // Emit after swap — should use new emitter
        setup_port.emit_setup_state_changed(
            uc_core::setup::SetupState::FirstRun,
            None,
        ).await;

        let recorded = events.lock().unwrap();
        assert_eq!(recorded.len(), 1, "Event should reach the new emitter");
    }
}
```

---

## State of the Art

| Old Approach                                 | Current Approach                                          | When Changed                 | Impact                                                         |
| -------------------------------------------- | --------------------------------------------------------- | ---------------------------- | -------------------------------------------------------------- |
| AppRuntime holds all fields (Tauri + domain) | CoreRuntime (uc-app) + AppRuntime thin wrapper (uc-tauri) | Phase 38                     | CoreRuntime independently testable without Tauri               |
| build_setup_orchestrator in AppRuntime impl  | Standalone function in assembly.rs                        | Phase 38                     | Single assembly point, no secondary wiring in runtime.rs       |
| HostEventSetupPort holds snapshot Arc        | HostEventSetupPort holds shared RwLock cell               | Phase 38 (stale emitter fix) | Setup state changes now reach frontend from all emission sites |
| UseCases as single struct in uc-tauri        | CoreUseCases (uc-app) + AppUseCases wrapper (uc-tauri)    | Phase 38                     | CoreUseCases independently testable without Tauri              |

**Deprecated/outdated after this phase:**

- `SetupRuntimePorts` struct in runtime.rs: replaced by `SetupAssemblyPorts` in assembly.rs
- `AppRuntime::build_setup_orchestrator`: replaced by free function `build_setup_orchestrator` in assembly.rs
- `HostEventSetupPort` snapshot constructor signature: replaced with shared-cell constructor

---

## Open Questions

1. **AppUseCases: Deref vs. explicit proxy**
   - What we know: Claude's discretion. Both work. Deref is more concise but can obscure the two-tier boundary at call sites.
   - What's unclear: Whether the project's AGENTS.md has a preference for explicitness.
   - Recommendation: Use explicit proxy methods (not Deref) for clarity. The ~35 methods can be generated with a macro or written out — explicit is better for reviewability given the architectural significance of this boundary.

2. **Where to put SetupAssemblyPorts in assembly.rs**
   - What we know: Claude's discretion. assembly.rs already has HostEventSetupPort (127-155) and WiredDependencies (119-124).
   - Recommendation: Define SetupAssemblyPorts inline at the top of assembly.rs alongside the `build_setup_orchestrator` function. No separate module needed at this scale.

3. **app_lifecycle_coordinator() in AppUseCases vs. CoreUseCases**
   - What we know: This accessor constructs TauriSessionReadyEmitter and DeviceNameAnnouncer inline, so it MUST stay in AppUseCases.
   - What's unclear: Whether there is a cached `Arc<AppLifecycleCoordinator>` that should stay on CoreRuntime (like setup_orchestrator) to share state across calls.
   - Recommendation: Examine how `app_lifecycle_coordinator()` is currently called. If it's called multiple times and sharing is required, keep a cached `Arc<AppLifecycleCoordinator>` on CoreRuntime (passed in at construction) — but leave construction of TauriSessionReadyEmitter to AppRuntime/AppUseCases.

---

## Validation Architecture

> workflow.nyquist_validation is not set in .planning/config.json — treating as enabled.

### Test Framework

| Property           | Value                                              |
| ------------------ | -------------------------------------------------- |
| Framework          | Cargo test (built-in)                              |
| Config file        | src-tauri/Cargo.toml workspace                     |
| Quick run command  | `cd src-tauri && cargo test -p uc-app`             |
| Full suite command | `cd src-tauri && cargo test -p uc-app -p uc-tauri` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                     | Test Type     | Automated Command                                                                 | File Exists?                              |
| ------- | ------------------------------------------------------------ | ------------- | --------------------------------------------------------------------------------- | ----------------------------------------- |
| RNTM-01 | CoreRuntime compiles in uc-app without Tauri                 | compile check | `cd src-tauri && cargo check -p uc-app`                                           | ❌ Wave 0 (CoreRuntime doesn't exist yet) |
| RNTM-01 | TaskRegistry works in uc-app (cancellation, spawn, shutdown) | unit          | `cd src-tauri && cargo test -p uc-app task_registry`                              | ❌ Wave 0 (move from uc-tauri)            |
| RNTM-01 | InMemoryLifecycleStatus defaults to Idle, set/get            | unit          | `cd src-tauri && cargo test -p uc-app in_memory_lifecycle`                        | ❌ Wave 0 (move from uc-tauri)            |
| RNTM-01 | LoggingLifecycleEventEmitter does not error                  | unit          | `cd src-tauri && cargo test -p uc-app logging_lifecycle`                          | ❌ Wave 0 (move from uc-tauri)            |
| RNTM-01 | LoggingSessionReadyEmitter does not error                    | unit          | `cd src-tauri && cargo test -p uc-app logging_session_ready`                      | ❌ Wave 0 (move from uc-tauri)            |
| RNTM-05 | SetupOrchestrator assembly via standalone function           | unit          | `cd src-tauri && cargo test -p uc-tauri build_setup_orchestrator`                 | ❌ Wave 0                                 |
| RNTM-05 | HostEventSetupPort reads from shared cell after swap         | unit (SC#4)   | `cd src-tauri && cargo test -p uc-app setup_state_emission_survives_emitter_swap` | ❌ Wave 0                                 |

**Note on existing tests:** `uc-tauri/src/adapters/lifecycle.rs` has 5 tests (lines 153-223) that will migrate with their types. `uc-tauri/src/bootstrap/task_registry.rs` has 4 tests (lines 104-189) that migrate with TaskRegistry. These are not new — they move with their code.

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check -p uc-app && cargo check -p uc-tauri`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-app -p uc-tauri`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-app/src/runtime.rs` — CoreRuntime struct (created in this phase)
- [ ] `src-tauri/crates/uc-app/src/task_registry.rs` — moved from uc-tauri; existing tests migrate with it
- [ ] `src-tauri/crates/uc-app/src/usecases/app_lifecycle/adapters.rs` — moved from uc-tauri; existing tests migrate with them
- [ ] SC#4 test `setup_state_emission_survives_emitter_swap` — new test for stale emitter fix, does not exist anywhere yet

---

## Sources

### Primary (HIGH confidence)

- Direct code inspection: `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — AppRuntime struct (lines 100-128), with_setup (lines 202-236), build_setup_orchestrator (lines 353-441), UseCases (lines 512-955)
- Direct code inspection: `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` — HostEventSetupPort (lines 126-155), resolve_pairing_device_name (lines 796-811)
- Direct code inspection: `src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs` — TaskRegistry (zero Tauri deps confirmed)
- Direct code inspection: `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs` — All 4 types confirmed; DeviceNameAnnouncer import of resolve_pairing_device_name at line 18
- Direct code inspection: `src-tauri/crates/uc-app/src/deps.rs` — AppDeps (no Tauri imports confirmed)
- Direct code inspection: `src-tauri/crates/uc-app/Cargo.toml` — tokio-util NOT present, must be added
- Direct code inspection: `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` — All 4 port traits confirmed in uc-app
- `.planning/phases/38-coreruntime-extraction/38-CONTEXT.md` — all implementation decisions, canonical refs, known pitfalls

### Secondary (MEDIUM confidence)

- `.planning/STATE.md` — Confirmed known bug description (stale emitter) matches code inspection findings

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — verified by direct Cargo.toml and source inspection; no external libraries involved
- Architecture: HIGH — all patterns are direct extractions from existing code; no new patterns invented
- Pitfalls: HIGH — stale emitter bug documented in STATE.md and verified by code trace through assembly.rs:128-135 and runtime.rs:420-422; other pitfalls verified by import analysis

**Research date:** 2026-03-18
**Valid until:** 2026-04-18 (stable Rust codebase; no fast-moving external dependencies)
