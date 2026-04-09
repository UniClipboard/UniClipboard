# Phase 40: uc-bootstrap Crate - Context

**Gathered:** 2026-03-18
**Status:** Ready for planning
**Mode:** Auto-resolved (all recommended defaults)

<domain>
## Phase Boundary

Create `uc-bootstrap` as the sole composition root crate. Move Tauri-free assembly, config resolution, and logging init from `uc-tauri/bootstrap/` into `uc-bootstrap`. Provide scene-specific builders (`build_gui_app`, `build_cli_context`, `build_daemon_app`) so all entry points depend on uc-bootstrap instead of wiring uc-infra and uc-platform directly. After this phase, uc-tauri's composition dependency on uc-infra and uc-platform is substantially reduced — bulk composition moves to uc-bootstrap. uc-tauri retains direct deps on uc-platform (for Tauri adapters) and may retain residual uc-infra deps for types used by AppUseCases and start_background_tasks.

</domain>

<decisions>
## Implementation Decisions

### Crate boundary — what moves to uc-bootstrap

- **Moves:** `assembly.rs` (42K, zero tauri imports), `config_resolution.rs` (7K, pure), `config.rs` (6K, config loading), `init.rs` (4.8K, ensure_default_device_name), tracing init delegation
- **Stays in uc-tauri:** `runtime.rs` (AppRuntime — wraps CoreRuntime + tauri::AppHandle, Tauri-specific), `wiring.rs` (Tauri event loops, `start_background_tasks()`), `file_transfer_wiring.rs` (Tauri event loop), `run.rs` (Tauri builder), `logging.rs` (Tauri-specific log plugin), `clipboard_integration_mode.rs` (used by runtime.rs). Note: `AppRuntime` construction stays in uc-tauri because it holds `tauri::AppHandle`; uc-bootstrap provides all non-Tauri ingredients via `GuiBootstrapContext`
- `task_registry.rs` is a 128B re-export stub (already moved to uc-app in Phase 38) — stays or is deleted
- `WiredDependencies` struct moves with assembly.rs — BUT requires type boundary refactoring (see below)
- `BackgroundRuntimeDeps` type boundary: currently embedded in `WiredDependencies` as `super::wiring::BackgroundRuntimeDeps`. Since `WiredDependencies` moves to `uc-bootstrap`, this dependency must be resolved. Options: (a) split `WiredDependencies` so the bootstrap-owned portion is Tauri-free and a separate Tauri-specific struct wraps background deps, or (b) move `BackgroundRuntimeDeps` into `uc-bootstrap` as a generic background-worker payload (it contains channel receivers, not Tauri types), keeping only `start_background_tasks()` in uc-tauri. Evaluate during planning which option fits best.

### Builder API surface — scene-specific constructors

- `build_gui_app()` — Returns a Tauri-free `GuiBootstrapContext` containing: resolved config, initialized logging, wired CoreRuntime (with all domain deps), background runtime deps, watcher control, setup assembly ports (PairingOrchestrator, SpaceAccessOrchestrator, KeySlotStore, DeviceAnnouncer). This context also creates and owns platform event/command channels (returning both tx and rx sides), so that `PlatformRuntime` can be wired in uc-tauri without main.rs creating channels itself. This context contains everything needed to construct `AppRuntime` EXCEPT the Tauri `AppHandle`. The `uc-tauri` layer then performs the final `AppRuntime::with_setup(context)` call, adding only the Tauri-specific `app_handle`. This preserves the dependency direction: `uc-tauri -> uc-bootstrap`, with NO reverse dependency. Main.rs responsibility is narrowed to: (1) calling `build_gui_app()` via uc-bootstrap, (2) Tauri Builder setup with `AppRuntime` construction, (3) `.manage()` registration, (4) plugin/command registration. This satisfies BOOT-01 (sole composition root for non-Tauri assembly) and RNTM-04 (shared runtime). NOTE: some current main.rs assembly uses `tauri::async_runtime::block_on` — evaluate during planning whether to use a standalone tokio runtime in uc-bootstrap or restructure to avoid blocking
- `build_cli_context()` — returns CLI-ready dependency set (CoreRuntime + UseCases) WITHOUT starting background workers. No event emitter swap needed (uses LoggingEventEmitter permanently)
- `build_daemon_app()` — returns daemon-ready dependencies WITH worker handles. Uses LoggingEventEmitter (no Tauri). Workers are registered but not started (caller starts them)
- All three builders share the core assembly path internally (wire_dependencies or a shared `build_core(config) -> CoreRuntime` helper)
- UseCases accessor ownership model (RNTM-04): bootstrap context returns the owning runtime (e.g. `CoreRuntime` or a higher-level struct containing it). Callers obtain the borrow-based `CoreUseCases<'a>` via a `.usecases()` factory method on the returned runtime — NOT by storing a pre-constructed accessor inside the context. This avoids self-referential lifetime issues. "No entry point constructs its own" means no entry point manually wires a `CoreRuntime` — they all receive it from uc-bootstrap

### Logging unification (BOOT-05)

- **Two-tier tracing architecture**: `uc-observability` owns low-level layer builders (`build_console_layer`, `build_json_layer`, `build_seq_layer`) and a basic `init_tracing_subscriber(logs_dir, profile)` that does NOT include Sentry or app-dir resolution
- **App-level tracing wrapper moves to uc-bootstrap**: The current `uc-tauri/bootstrap/tracing.rs` contains the process-level initializer that resolves app dirs via `DirsAppDirsAdapter`, reads device_id, composes Sentry layer, wires Seq, and registers the composed subscriber. This wrapper (NOT the uc-observability low-level function) must move to `uc-bootstrap` since it is Tauri-free (only depends on `uc-platform::app_dirs`, `uc-app::app_paths`, `uc-observability`)
- uc-bootstrap calls this wrapper exactly once inside each builder function
- main.rs removes its direct call to init_tracing_subscriber — uc-bootstrap handles it
- Each builder can pass a logging profile (Dev/Prod/DebugClipboard) to customize output
- Sentry/Seq static guards (`OnceLock`) move with the wrapper into uc-bootstrap
- Idempotency requirement: the current tracing wrapper uses `OnceLock::set()` which explicitly bails on a second call — it is NOT idempotent. When tracing init moves inside each builder, the wrapper MUST be made idempotent: treat "already initialized" as `Ok(())` rather than `Err`. This is needed because (a) tests may construct multiple builder contexts in one process, (b) the builder API should be safe to call without precondition checks by the caller. Implementation: change `OnceLock::set().is_err()` → log a debug message and return `Ok(())`, and change `try_init()` error to non-fatal when the global subscriber is already set. Alternatively, expose a builder option to skip logging init (e.g. `BuilderOptions { skip_tracing: bool }`) for test scenarios

### Dependency graph restructuring (BOOT-04)

- uc-bootstrap depends on: uc-core, uc-app, uc-infra, uc-platform, uc-observability
- uc-tauri depends on: uc-bootstrap (for composition), uc-core (for types), uc-app (for CoreRuntime/UseCases types)
- uc-tauri REDUCES direct dependency on uc-infra and uc-platform for composition purposes (bulk composition moves to uc-bootstrap)
- uc-tauri RETAINS uc-platform dependency for Tauri-specific adapters (TauriEventEmitter, PlatformRuntime)
- uc-tauri MAY retain residual uc-infra dependency for types consumed by `AppUseCases` (e.g. `TransferPayloadEncryptorAdapter`) and `start_background_tasks()` (e.g. `KeySlotStore`). Full uc-infra decoupling from uc-tauri is explicitly deferred — the phase goal is to centralize the bulk composition root, not achieve zero-infra-coupling in uc-tauri
- main.rs (uniclipboard binary) depends on uc-tauri (which transitively gets uc-bootstrap)
- Future uc-daemon and uc-cli (Phase 41) depend on uc-bootstrap directly, NOT uc-tauri

### uc-tauri bootstrap/ module after extraction

- `mod.rs` re-exports from uc-bootstrap for backward compatibility during transition (main.rs import paths may change)
- Remaining bootstrap/ files: runtime.rs, wiring.rs, file_transfer_wiring.rs, run.rs, logging.rs, clipboard_integration_mode.rs
- assembly.rs replaced by re-export: `pub use uc_bootstrap::assembly;` (or imports updated directly)

### Claude's Discretion

- Internal module organization within uc-bootstrap (single lib.rs vs sub-modules)
- Import migration strategy: preferred approach is to add `uc-bootstrap` as a direct dependency in `src-tauri/Cargo.toml` (the root `uniclipboard` package) alongside `uc-tauri`, and update import paths directly. Alternative: use `pub use` re-exports in uc-tauri/bootstrap/mod.rs for backward compatibility. Either way, the root package Cargo.toml must be updated
- Exact Cargo.toml feature flags (if any) for uc-bootstrap
- Whether build_gui_app internally calls wire_dependencies or replaces it with a new implementation (but it MUST return more than WiredDependencies — see builder API decision above)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements

- `.planning/REQUIREMENTS.md` — BOOT-01 through BOOT-05 and RNTM-04 define success criteria

### Phase context (prior decisions)

- `.planning/phases/37-wiring-decomposition/37-CONTEXT.md` — assembly.rs creation, zero-tauri-import guarantee, SC#4 staged approach
- `.planning/phases/38-coreruntime-extraction/38-CONTEXT.md` — CoreRuntime in uc-app, UseCases split (CoreUseCases/AppUseCases), TaskRegistry migration
- `.planning/phases/39-config-resolution-extraction/39-CONTEXT.md` — config_resolution.rs location rationale, resolve_app_config() API, SC#1 partial satisfaction note

### Roadmap

- `.planning/ROADMAP.md` — Phase 40 success criteria (6 items), Phase 41 dependency on Phase 40

### Codebase entry points

- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — Current re-exports that define the public API surface being extracted
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` — Primary extraction target (42K, zero tauri imports)
- `src-tauri/crates/uc-tauri/src/bootstrap/config_resolution.rs` — Config resolution extraction target
- `src-tauri/crates/uc-tauri/Cargo.toml` — Current dependency graph to restructure
- `src-tauri/src/main.rs` — Entry point that imports from uc-tauri::bootstrap (import paths will change). Lines 320-397 contain substantial non-Tauri assembly (PairingOrchestrator, SpaceAccessOrchestrator, AppRuntime) that should move to uc-bootstrap
- `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` — App-level tracing wrapper (Sentry/Seq composition) that moves to uc-bootstrap

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `assembly.rs` — Already Tauri-free composition root with `wire_dependencies()`, `build_setup_orchestrator()`, `get_storage_paths()`, etc. This is the primary extraction target.
- `config_resolution.rs` — Pure `resolve_app_config()` with no Tauri deps. Ready to move as-is.
- `uc-observability` crate — Owns low-level layer builders and a basic `init_tracing_subscriber(logs_dir, profile)`. The app-level wrapper (Sentry, Seq, app-dir resolution) currently in `uc-tauri/bootstrap/tracing.rs` must move to uc-bootstrap.
- `CoreRuntime` (in uc-app) — Tauri-free runtime core created in Phase 38. uc-bootstrap builders will construct this.
- `CoreUseCases` (in uc-app) — Pure domain use case accessors. Shared across all entry points.

### Established Patterns

- Crate-based modularization: each concern in its own crate under `src-tauri/crates/`
- Port/adapter pattern: composition happens in assembly, traits in uc-core/ports
- Re-export pattern: `pub use` in mod.rs for backward compatibility (used in Phase 38 for TaskRegistry)
- Workspace Cargo.toml at `src-tauri/Cargo.toml` defines workspace members

### Integration Points

- `src-tauri/Cargo.toml` workspace members array — must add `crates/uc-bootstrap`
- `src-tauri/crates/uc-tauri/Cargo.toml` — must add uc-bootstrap dep, evaluate removing uc-infra/uc-platform
- `src-tauri/src/main.rs` — import paths change from `uc_tauri::bootstrap::*` to either `uc_bootstrap::*` or uc-tauri re-exports
- `uc-tauri/bootstrap/mod.rs` — re-exports must be updated to delegate to uc-bootstrap

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. The phase is a straightforward crate extraction following the patterns established in Phases 37-39.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

_Phase: 40-uc-bootstrap-crate_
_Context gathered: 2026-03-18_
