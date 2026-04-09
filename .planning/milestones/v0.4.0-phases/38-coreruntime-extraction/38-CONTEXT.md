# Phase 38: CoreRuntime Extraction - Context

**Gathered:** 2026-03-18
**Status:** Ready for planning
**Revised:** 2026-03-18 (7 context review corrections applied)

<domain>
## Phase Boundary

Extract a Tauri-free `CoreRuntime` struct from `AppRuntime`. CoreRuntime holds AppDeps, shared orchestrators (SetupOrchestrator), event emitter, task registry, and other non-Tauri state. AppRuntime becomes a thin wrapper adding only Tauri-specific handles (app_handle, watcher_control). SetupOrchestrator assembly is unified into the main composition root (assembly.rs). UseCases are split into two tiers: CoreUseCases (pure domain, in uc-app) and platform/Tauri accessors (stay in uc-tauri).

**In scope:**

- Define CoreRuntime struct in uc-app crate (Tauri-free, passes `cargo check -p uc-app`)
- Move fields from AppRuntime to CoreRuntime: deps (AppDeps), event_emitter, lifecycle_status, setup_orchestrator, clipboard_integration_mode, task_registry, storage_paths
- Move `TaskRegistry` type from uc-tauri to uc-app (prerequisite for CoreRuntime to compile in uc-app — see TaskRegistry migration below)
- AppRuntime wraps CoreRuntime + adds: app_handle, watcher_control (platform-layer)
- Move SetupOrchestrator assembly from runtime.rs `build_setup_orchestrator` to assembly.rs as standalone function
- Move non-Tauri adapters (DeviceNameAnnouncer, LoggingLifecycleEventEmitter, LoggingSessionReadyEmitter, InMemoryLifecycleStatus) from uc-tauri/adapters to **uc-app only** (see adapter disposition below)
- Inline `resolve_pairing_device_name` logic into DeviceNameAnnouncer when moving to uc-app (eliminates uc-tauri dependency)
- Split UseCases into CoreUseCases (uc-app, ~35 pure domain accessors) and platform accessors (stay in uc-tauri)
- Move facade accessors (device_id, is_encryption_ready, settings_port, etc.) to CoreRuntime
- AppRuntime proxies usecases() and facade calls to inner CoreRuntime (transparent to command layer)
- Existing GUI setup flow continues to work end-to-end (SC#4)

**Out of scope:**

- Creating new workspace crate (no uc-runtime or uc-bootstrap yet — Phase 40)
- Moving assembly.rs out of uc-tauri (Phase 40)
- Modifying command layer code (transparent proxy keeps `runtime.usecases().xxx()` unchanged)
- Daemon/CLI entry points (Phase 41)
- Creating new port traits for SessionReadyEmitter — it already exists in uc-app

</domain>

<decisions>
## Implementation Decisions

### CoreRuntime struct boundary (maximum extraction)

- CoreRuntime holds: AppDeps, event_emitter (RwLock<Arc<dyn HostEventEmitterPort>>), lifecycle_status, setup_orchestrator, clipboard_integration_mode, task_registry, storage_paths
- AppRuntime holds: CoreRuntime + app_handle (Arc<RwLock<Option<AppHandle>>>) + watcher_control (Arc<dyn WatcherControlPort>)
- CoreRuntime lives in uc-app crate — natural home alongside AppDeps and SetupOrchestrator
- Dependency is clean: uc-app already depends on uc-core; CoreRuntime uses only uc-core traits and uc-app types
- **TaskRegistry must move to uc-app first** — currently defined in `uc-tauri/src/bootstrap/task_registry.rs:20`. The type has zero Tauri dependencies (only `tokio`, `tokio_util`, `tracing`), so it can move cleanly. uc-app already depends on tokio. **Note**: uc-app does NOT currently depend on `tokio-util` — the migration must add `tokio-util` (with `sync` feature for `CancellationToken`) to `uc-app/Cargo.toml` and update the lockfile. After the move, `main.rs:600` `.manage(task_registry.clone())` continues to work since Tauri `.manage()` accepts any `Send + Sync + 'static` type regardless of source crate.

### Mutability and event emitter

- Arc<CoreRuntime> shared, interior mutability via RwLock (same pattern as current AppRuntime)
- RwLock swap for event_emitter preserved — Tauri mode swaps LoggingEmitter to TauriEmitter at setup time
- daemon/CLI construct CoreRuntime with final emitter, never swap

### Stale emitter fix (MANDATORY — fixes known bug in STATE.md)

**Problem**: The current `HostEventSetupPort` (assembly.rs:128-135) stores a cloned `Arc<dyn HostEventEmitterPort>` at construction time. The startup sequence is:

1. `main.rs:559-561`: Create `LoggingEventEmitter`, pass to `AppRuntime::with_setup()`
2. `runtime.rs:420-422`: `build_setup_orchestrator` captures this Arc into `HostEventSetupPort::new(event_emitter)` — **a snapshot, not the live cell**
3. `main.rs:673-677`: `.setup()` callback swaps emitter to `TauriEventEmitter` via `set_event_emitter()`
4. **Result**: The RwLock cell now holds TauriEventEmitter, but `SetupOrchestrator`'s `HostEventSetupPort` still holds the old LoggingEventEmitter. Setup state changes emitted from spawned listener tasks never reach the frontend.

**Required fix mechanism** (choose one, do not leave as "should fix itself"):

- **(A) Shared cell read-through** (recommended): Change `HostEventSetupPort` to hold `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` (the same shared cell that CoreRuntime owns). On each `emit_setup_state_changed()` call, read the current emitter from the cell. This way the swap automatically propagates — zero reconstruction needed.
- **(B) Post-swap reconstruction**: After `set_event_emitter()`, rebuild the `SetupOrchestrator` (or at least its `HostEventSetupPort`) with the new emitter. Requires exposing a rebuild/replace method on SetupOrchestrator.
- **(C) Deferred construction**: Don't construct SetupOrchestrator until after `.setup()` when AppHandle is available and the final emitter is set. Requires restructuring the startup sequence since `AppRuntime::with_setup()` currently builds the orchestrator eagerly.

Option (A) is the cleanest — it matches the existing RwLock swap pattern used for `event_emitter` on CoreRuntime itself, requires no startup sequence changes, and eliminates the entire class of stale-capture bugs for any future emitter consumers.

### SetupOrchestrator assembly location

- Moved from AppRuntime::build_setup_orchestrator (runtime.rs) to assembly.rs as standalone function
- assembly.rs only orchestrates — does NOT create adapters
- All adapters passed in as Arc<dyn Port> parameters (parameterized injection)
- New SetupAssemblyPorts struct bundles all required ports (replaces SetupRuntimePorts which is deprecated/merged)

### Adapter disposition

**Already ports (no change needed):**

- InitializeEncryption, MarkSetupComplete, StartNetworkAfterUnlock, DefaultSpaceAccessCryptoFactory, SpaceAccessNetworkAdapter, HmacProofAdapter, SpaceAccessPersistenceAdapter (all uc-app)
- StartClipboardWatcher (uc-platform, implements StartClipboardWatcherPort)
- Timer (uc-infra, implements TimerPort)
- HostEventSetupPort (assembly.rs:128, wraps HostEventEmitterPort; re-exported via wiring.rs:80)

**Port already exists (no new abstraction needed):**

- `SessionReadyEmitter` trait already defined in `uc-app/src/usecases/app_lifecycle/mod.rs:65`
- `TauriSessionReadyEmitter` already implements it in `uc-tauri/src/adapters/lifecycle.rs:90`
- TauriSessionReadyEmitter stays in uc-tauri (depends on AppHandle), passed as `Arc<dyn SessionReadyEmitter>`

**Move to uc-app (NOT uc-infra — all implement traits defined in uc-app):**

- `InMemoryLifecycleStatus` → uc-app (implements `LifecycleStatusPort`, zero external deps)
- `LoggingLifecycleEventEmitter` → uc-app (implements `LifecycleEventEmitter`, only uses tracing)
- `LoggingSessionReadyEmitter` → uc-app (implements `SessionReadyEmitter`, only uses tracing)
- `DeviceNameAnnouncer` → uc-app (implements `DeviceAnnouncer`, uses `PeerDirectoryPort` + `SettingsPort` from uc-core)
  - **IMPORTANT**: Must inline the `resolve_pairing_device_name` logic (currently at `assembly.rs:796-811`) into DeviceNameAnnouncer. This function only depends on `SettingsPort` (uc-core trait) and `DEFAULT_PAIRING_DEVICE_NAME` constant. Without inlining, moving DeviceNameAnnouncer to uc-app would create an uc-app → uc-tauri reverse dependency.

**Why NOT uc-infra:** All 4 traits (`LifecycleStatusPort`, `LifecycleEventEmitter`, `SessionReadyEmitter`, `DeviceAnnouncer`) are defined in `uc-app/src/usecases/app_lifecycle/mod.rs`. The project's fixed layering forbids uc-infra from depending on uc-app. Moving implementations to uc-infra would require first relocating traits to uc-core — unnecessary churn for this phase.

### UseCases two-tier split

**CRITICAL CORRECTION**: UseCases CANNOT move wholesale to uc-app. Three accessors have Tauri/platform dependencies that prevent migration:

| Accessor                      | Dependency                                        | Reason                                           |
| ----------------------------- | ------------------------------------------------- | ------------------------------------------------ |
| `apply_autostart()`           | `AppHandle`, `TauriAutostart`                     | Constructs Tauri-specific adapter inline         |
| `start_clipboard_watcher()`   | `WatcherControlPort` (uc-platform)                | Uses platform-layer port not in CoreRuntime      |
| `app_lifecycle_coordinator()` | `TauriSessionReadyEmitter`, `DeviceNameAnnouncer` | Constructs Tauri adapter inline using app_handle |

**Solution — two-tier UseCases:**

```
CoreUseCases (in uc-app, bound to CoreRuntime)
├── ~35 pure domain accessors (get_settings, initialize_encryption, etc.)
│   All depend only on AppDeps fields (uc-core ports)
├── setup_orchestrator(), get_lifecycle_status() — pure state accessors
└── facade accessors (device_id, is_encryption_ready, settings_port, etc.)

AppUseCases (stays in uc-tauri, wraps CoreUseCases)
├── apply_autostart()           → needs AppHandle
├── start_clipboard_watcher()   → needs WatcherControlPort
├── app_lifecycle_coordinator() → constructs Tauri adapters
└── Deref or explicit proxy to CoreUseCases for all other accessors
```

- `runtime.usecases()` returns `AppUseCases` which transparently exposes all `CoreUseCases` methods
- Command layer code unchanged: `runtime.usecases().get_settings()` still works
- CoreRuntime can independently compile and test via `cargo check -p uc-app`

### Testing strategy

- CoreRuntime unit tests in uc-app using existing noop port implementations (from v0.2.0 test infrastructure)
- Add noop/logging event emitter for tests as needed
- Existing runtime.rs tests split: pure domain tests migrate to uc-app, Tauri-dependent tests stay in uc-tauri
- Both sides must compile and pass independently
- **SC#4 integration coverage**: At least one integration test verifying setup-state emission reaches the frontend event stream (e.g., lifecycle state transition emits correct event through HostEventEmitterPort). Compile-only checks are insufficient given the prior stale-emitter bug (STATE.md known bug). This can be a Rust-side test using mock emitter + assertion on emitted events — does not require a running GUI.

### Claude's Discretion

- Exact CoreRuntime field ordering and constructor signature
- Whether SetupAssemblyPorts lives in assembly.rs or a dedicated module
- Whether AppUseCases uses Deref<Target=CoreUseCases> or explicit proxy methods
- Commit split granularity (minimum: uc-app CoreRuntime+CoreUseCases, uc-tauri AppRuntime wrapper+AppUseCases, assembly.rs setup migration, adapter moves)
- Internal implementation of AppRuntime proxy methods
- Exact module location for moved adapters within uc-app (e.g., `usecases/app_lifecycle/adapters.rs` or `adapters/` top-level)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements and phase definition

- `.planning/REQUIREMENTS.md` — RNTM-01 (CoreRuntime without Tauri dependency) and RNTM-05 (SetupOrchestrator unified assembly)
- `.planning/ROADMAP.md` — Phase 38 success criteria (4 items): CoreRuntime in Tauri-free crate, AppRuntime as thin wrapper, SetupOrchestrator single assembly point, GUI flow unchanged

### Phase 36 and 37 context (predecessor decisions)

- `.planning/phases/36-event-emitter-abstraction/36-CONTEXT.md` — HostEventEmitterPort design, RwLock swap pattern, app_handle coexistence
- `.planning/phases/37-wiring-decomposition/37-CONTEXT.md` — assembly.rs creation, wiring.rs split, AppHandle removal from start_background_tasks

### Primary code targets

- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — AppRuntime struct (line 100-128), build_setup_orchestrator (lines 353-441), UseCases struct and accessors, facade methods
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` — Pure Rust assembly module (wire_dependencies, resolve_pairing_device_name at line 796)
- `src-tauri/crates/uc-app/src/deps.rs` — AppDeps struct definition (line 108-127)
- `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs` — SetupOrchestrator constructor signature

### Types and adapters to move

- `src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs` — TaskRegistry (move to uc-app, prerequisite for CoreRuntime)
- `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs` — TauriSessionReadyEmitter (stays in uc-tauri), DeviceNameAnnouncer (move to uc-app), LoggingLifecycleEventEmitter (move to uc-app), LoggingSessionReadyEmitter (move to uc-app), InMemoryLifecycleStatus (move to uc-app)
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs:128-135` — HostEventSetupPort definition (wraps HostEventEmitterPort for setup events — must be refactored to read from shared RwLock cell, see stale emitter fix). Note: `wiring.rs:80` only re-exports this type from assembly.rs.

### Existing port traits (already in uc-app, NO new traits needed)

- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs:47-76` — `LifecycleStatusPort`, `LifecycleEventEmitter`, `SessionReadyEmitter`, `DeviceAnnouncer` (all 4 traits already defined here)

### Crate-level rules

- `src-tauri/crates/uc-core/AGENTS.md` — No Tauri/system imports, port conventions
- `src-tauri/crates/uc-tauri/AGENTS.md` — Bootstrap editing rules
- `AGENTS.md` — Atomic commit rules, hex boundary

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `assembly.rs`: Already a pure Rust module with zero tauri imports — natural home for SetupOrchestrator assembly function
- `AppDeps` (uc-app/src/deps.rs): Clean dependency bundle with 6 port groups, already in uc-app
- Existing noop port implementations in uc-app test modules — reusable for CoreRuntime unit tests
- `SetupRuntimePorts` struct (runtime.rs:131-135) — to be merged into new SetupAssemblyPorts
- `resolve_pairing_device_name` (assembly.rs:796-811) — pure function depending only on `SettingsPort`, ready to inline into DeviceNameAnnouncer
- `TaskRegistry` (task_registry.rs:20-23) — Tauri-free type using only `CancellationToken` + `Mutex<JoinSet<()>>`, ready to move to uc-app

### Established Patterns

- Port injection via `Arc<dyn XxxPort>` through constructors — CoreRuntime follows same pattern
- Interior mutability via RwLock for late-binding (event_emitter swap) — CoreRuntime preserves this
- UseCases factory accessor pattern (`runtime.usecases().xxx()`) — preserved via transparent proxy
- Facade accessors for simple read-only state — preserved via proxy

### Integration Points

- AppRuntime wraps CoreRuntime: `core: Arc<CoreRuntime>` field, all proxy methods delegate
- assembly.rs gains `build_setup_orchestrator(deps: &AppDeps, ports: SetupAssemblyPorts, ...) -> Arc<SetupOrchestrator>`
- Command layer unchanged: `runtime.usecases().xxx()` still works (AppUseCases proxies CoreUseCases)
- wiring.rs: Creates Tauri-specific adapters (TauriSessionReadyEmitter) and passes to assembly.rs

### Known Pitfalls

- **Stale emitter bug (STATE.md)**: See "Stale emitter fix (MANDATORY)" in decisions section. This is an explicit in-scope deliverable, not a side-effect assumption. The fix mechanism must be chosen and implemented — merely "unifying assembly location" does NOT fix the bug because the root cause is `HostEventSetupPort` capturing a snapshot Arc instead of reading from the shared RwLock cell.
- **TaskRegistry crate move**: `TaskRegistry` is currently in `uc-tauri/src/bootstrap/task_registry.rs`. It must move to uc-app before CoreRuntime can compile there. The type itself is Tauri-free (only tokio/tokio_util/tracing), but `main.rs:592-600` retrieves it via `runtime_for_handler.task_registry().clone()` and registers it with `.manage()`. After the move, the accessor method and `.manage()` call remain valid — only the import path changes.
- **DeviceNameAnnouncer → resolve_pairing_device_name**: Currently imports from `crate::bootstrap::resolve_pairing_device_name` (uc-tauri). When moving to uc-app, must inline the 15-line function body or move it alongside. The function only needs `SettingsPort` + a `DEFAULT_PAIRING_DEVICE_NAME` constant.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

- Move assembly.rs to uc-bootstrap crate with real `cargo check -p` independence — Phase 40
- Create uc-bootstrap as sole composition root — Phase 40
- Daemon/CLI entry points using CoreRuntime directly — Phase 41
- Move remaining Tauri adapters out of uc-tauri — future phases
- Relocate lifecycle traits from uc-app to uc-core if uc-infra adapters are ever needed — future cleanup
- Split CoreUseCases into domain-specific accessor groups if it grows too large — future cleanup

</deferred>

---

_Phase: 38-coreruntime-extraction_
_Context gathered: 2026-03-18_
_Revised: 2026-03-18 (7 context review corrections applied)_
