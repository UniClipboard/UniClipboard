# Phase 40: uc-bootstrap Crate - Research

**Researched:** 2026-03-18
**Domain:** Rust crate extraction, composition root pattern, Cargo workspace management
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Crate boundary — what moves to uc-bootstrap**

- Moves: `assembly.rs` (42K, zero tauri imports), `config_resolution.rs` (7K, pure), `config.rs` (6K, config loading), `init.rs` (4.8K, ensure_default_device_name), tracing init delegation
- Stays in uc-tauri: `runtime.rs`, `wiring.rs`, `file_transfer_wiring.rs`, `run.rs`, `logging.rs`, `clipboard_integration_mode.rs`
- `task_registry.rs` is a 128B re-export stub — stays or is deleted
- `WiredDependencies` struct moves with assembly.rs
- `BackgroundRuntimeDeps` type boundary: evaluate during planning — either (a) split WiredDependencies so bootstrap portion is Tauri-free and a separate Tauri-specific struct wraps background deps, or (b) move BackgroundRuntimeDeps into uc-bootstrap as a generic background-worker payload (contains channel receivers, not Tauri types)

**Builder API surface — scene-specific constructors**

- `build_gui_app()` — Returns Tauri-free `GuiBootstrapContext` containing: resolved config, initialized logging, wired CoreRuntime, background runtime deps, watcher control, setup assembly ports, platform event/command channels. Contains everything to construct `AppRuntime` EXCEPT `tauri::AppHandle`. uc-tauri performs `AppRuntime::with_setup(context)` adding only Tauri-specific `app_handle`
- `build_cli_context()` — returns CLI-ready dependency set (CoreRuntime + UseCases) WITHOUT starting background workers. Uses LoggingEventEmitter permanently
- `build_daemon_app()` — returns daemon-ready dependencies WITH worker handles. Uses LoggingEventEmitter. Workers registered but not started (caller starts them)
- All three builders share `build_core(config) -> CoreRuntime` helper internally
- UseCases accessor: callers obtain borrow-based `CoreUseCases<'a>` via `.usecases()` factory method on returned runtime — NOT pre-constructed inside context (avoids self-referential lifetime issues)

**Logging unification (BOOT-05)**

- Two-tier architecture: uc-observability owns low-level layer builders; app-level wrapper moves to uc-bootstrap
- App-level tracing wrapper (current `uc-tauri/bootstrap/tracing.rs`) moves to uc-bootstrap
- uc-bootstrap calls wrapper exactly once inside each builder function
- main.rs removes its direct call to `init_tracing_subscriber`
- Each builder can pass a logging profile (Dev/Prod/DebugClipboard)
- Sentry/Seq static guards (`OnceLock`) move with the wrapper into uc-bootstrap
- Idempotency requirement: change `OnceLock::set().is_err()` to log debug + return Ok(()), change `try_init()` error to non-fatal when subscriber already set. OR expose `BuilderOptions { skip_tracing: bool }` for test scenarios

**Dependency graph restructuring (BOOT-04)**

- uc-bootstrap depends on: uc-core, uc-app, uc-infra, uc-platform, uc-observability
- uc-tauri depends on: uc-bootstrap (for composition), uc-core (for types), uc-app (for CoreRuntime/UseCases types)
- uc-tauri REDUCES direct dependency on uc-infra and uc-platform for composition purposes
- uc-tauri RETAINS uc-platform for Tauri-specific adapters; MAY retain residual uc-infra for types consumed by AppUseCases and start_background_tasks
- Future uc-daemon and uc-cli depend on uc-bootstrap directly, NOT uc-tauri

**uc-tauri bootstrap/ module after extraction**

- `mod.rs` re-exports from uc-bootstrap for backward compatibility
- Remaining bootstrap/ files: runtime.rs, wiring.rs, file_transfer_wiring.rs, run.rs, logging.rs, clipboard_integration_mode.rs

### Claude's Discretion

- Internal module organization within uc-bootstrap (single lib.rs vs sub-modules)
- Import migration strategy: preferred is to add `uc-bootstrap` as direct dependency in `src-tauri/Cargo.toml` and update import paths directly. Alternative: `pub use` re-exports in uc-tauri/bootstrap/mod.rs for backward compatibility. Either way, root package Cargo.toml must be updated
- Exact Cargo.toml feature flags (if any) for uc-bootstrap
- Whether build_gui_app internally calls wire_dependencies or replaces it with a new implementation

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                                | Research Support                                                                     |
| ------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| BOOT-01 | uc-bootstrap crate exists as sole composition root, depending on uc-core + uc-app + uc-infra + uc-platform | Crate creation pattern, Cargo workspace membership, dependency declarations          |
| BOOT-02 | uc-bootstrap provides build_cli_context() returning CLI-ready dependencies                                 | Builder function design, return type with CoreRuntime + no background workers        |
| BOOT-03 | uc-bootstrap provides build_daemon_app() returning daemon-ready dependencies with workers                  | Builder function design, BackgroundRuntimeDeps handling, worker registration pattern |
| BOOT-04 | uc-tauri depends on uc-bootstrap instead of directly on uc-infra + uc-platform                             | Cargo.toml restructuring, identifying which uc-infra/uc-platform deps remain vs move |
| BOOT-05 | Logging initialization unified in uc-bootstrap (not duplicated per entry point)                            | OnceLock idempotency pattern, tracing.rs migration, builder integration              |
| RNTM-04 | UseCases accessor is shared across all entry points (not duplicated per runtime mode)                      | CoreRuntime::usecases() borrow pattern, no self-referential Arc in context struct    |

</phase_requirements>

## Summary

Phase 40 is a crate extraction refactor: the existing Tauri-free assembly logic in `uc-tauri/bootstrap/` moves to a new `uc-bootstrap` crate that becomes the sole composition root. The extraction is largely mechanical because the primary source (`assembly.rs`) was specifically designed in Phase 37 with "zero tauri imports — enforced by CI lint" and a comment stating it is "structurally ready for extraction to a standalone `uc-bootstrap` crate in Phase 40."

The main technical challenges are: (1) resolving the `BackgroundRuntimeDeps` type boundary — it currently lives in `wiring.rs` but `WiredDependencies` moving to uc-bootstrap creates a compile-time dependency issue; (2) designing three scene-specific builder functions with a shared internal core path; (3) making `init_tracing_subscriber` idempotent so test scenarios can construct multiple bootstrap contexts in one process; (4) restructuring the Cargo dependency graph so uc-tauri no longer depends on uc-infra/uc-platform for composition purposes.

The `tauri::async_runtime::block_on` calls in `main.rs` (lines 351-355: resolving pairing device name and config) need to move into uc-bootstrap's `build_gui_app()`. Since uc-bootstrap has no Tauri dependency, this requires either using a standalone `tokio::runtime::Builder` (the same pattern already used in `tracing.rs` for Seq) or restructuring `build_gui_app()` as an async function that callers await via `tauri::async_runtime::block_on`.

**Primary recommendation:** Implement uc-bootstrap as a flat crate with sub-modules (builders.rs, tracing.rs, assembly.rs re-export), resolving BackgroundRuntimeDeps by moving it into uc-bootstrap (option b from CONTEXT — it contains only channel receivers and `Arc<Libp2pNetworkAdapter>`, zero Tauri types).

## Standard Stack

### Core

| Library            | Version         | Purpose                                               | Why Standard                                                                     |
| ------------------ | --------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------- |
| tokio              | 1.x (workspace) | Async runtime for builder functions needing block_on  | Already in workspace; uc-bootstrap builders need async resolution of device name |
| tokio-util         | 0.7             | CancellationToken for task registry                   | Already used in uc-app                                                           |
| anyhow             | 1.0             | Error propagation in builders                         | Already used across all crates                                                   |
| tracing            | 0.1             | Logging inside bootstrap                              | Already used across all crates                                                   |
| tracing-subscriber | 0.3             | Subscriber composition for init_tracing_subscriber    | Already in uc-tauri                                                              |
| sentry             | 0.46.1          | Error tracking layer (moved from uc-tauri/tracing.rs) | Already in uc-tauri deps                                                         |
| sentry-tracing     | 0.46.1          | Sentry tracing layer integration                      | Already in uc-tauri deps                                                         |
| gethostname        | 1.1             | Used by ensure_default_device_name (init.rs)          | Already in uc-tauri                                                              |

### Supporting

| Library            | Version | Purpose                                                                          | When to Use                   |
| ------------------ | ------- | -------------------------------------------------------------------------------- | ----------------------------- |
| dirs               | 6.0.0   | Fallback platform dirs (used in config_resolution.rs indirectly via uc-platform) | Already present via uc-tauri  |
| chrono             | 0.4     | Timestamps (used in assembly.rs)                                                 | Verify if assembly.rs uses it |
| toml               | 0.8     | Config file parsing (used in config.rs)                                          | Required for load_config      |
| serde / serde_json | 1.x     | Serialization in config types                                                    | Required for AppConfig        |

### Alternatives Considered

| Instead of                                     | Could Use                      | Tradeoff                                                                                                                                                                |
| ---------------------------------------------- | ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| standalone tokio runtime for async in builders | tauri::async_runtime::block_on | uc-bootstrap must NOT depend on tauri; standalone tokio rt already used in tracing.rs for Seq, so the pattern is established                                            |
| async build_gui_app()                          | sync wrapper with block_on     | Making builders async would require callers to have an async context; Tauri's setup closure is sync-ish (uses block_on). Sync builder with internal block_on is simpler |

**Installation:**

```bash
# No new external dependencies — all packages already in workspace
# Just add uc-bootstrap to workspace members and declare path deps
```

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-bootstrap/
├── Cargo.toml           # deps: uc-core, uc-app, uc-infra, uc-platform, uc-observability + tokio, anyhow, tracing, etc.
├── src/
│   ├── lib.rs           # pub use re-exports of all public items
│   ├── assembly.rs      # Moved from uc-tauri/bootstrap/assembly.rs (wire_dependencies, WiredDependencies, etc.)
│   ├── config.rs        # Moved from uc-tauri/bootstrap/config.rs (load_config)
│   ├── config_resolution.rs  # Moved from uc-tauri/bootstrap/config_resolution.rs
│   ├── init.rs          # Moved from uc-tauri/bootstrap/init.rs (ensure_default_device_name)
│   ├── tracing.rs       # Moved from uc-tauri/bootstrap/tracing.rs (init_tracing_subscriber + OnceLocks)
│   └── builders.rs      # NEW: build_gui_app(), build_cli_context(), build_daemon_app()
```

### Pattern 1: Cargo Workspace Crate Addition

**What:** Add new crate to workspace by declaring it in `src-tauri/Cargo.toml` members array and creating `src-tauri/crates/uc-bootstrap/Cargo.toml`.

**When to use:** Every new crate in this workspace follows this pattern (established in Phases 37-39).

**Example:**

```toml
# src-tauri/Cargo.toml [workspace] members
members = [
  "crates/uc-core",
  "crates/uc-app",
  "crates/uc-platform",
  "crates/uc-infra",
  "crates/uc-clipboard-probe",
  "crates/uc-tauri",
  "crates/uc-observability",
  "crates/uc-bootstrap",   # ADD THIS
]

# src-tauri/crates/uc-bootstrap/Cargo.toml
[package]
name = "uc-bootstrap"
version = "0.1.0"
edition = "2021"
description = "Composition root — sole crate allowed to depend on uc-core + uc-app + uc-infra + uc-platform simultaneously"

[dependencies]
uc-core     = { path = "../uc-core" }
uc-app      = { path = "../uc-app" }
uc-infra    = { path = "../uc-infra" }
uc-platform = { path = "../uc-platform" }
uc-observability = { path = "../uc-observability" }

tokio    = { version = "1", features = ["full"] }
tokio-util = "0.7"
anyhow   = "1.0"
thiserror = "2.0"
tracing  = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "registry"] }
sentry   = { version = "0.46.1", features = ["tracing"] }
sentry-tracing = "0.46.1"
gethostname = "1.1"
toml     = "0.8"
serde    = { version = "1", features = ["derive"] }
serde_json = "1"
chrono   = { version = "0.4", features = ["serde"] }
log      = "0.4"
dirs     = "6.0.0"
async-trait = "0.1"
uuid     = { version = "1", features = ["v4", "fast-rng"] }
blake3   = "1"
base64   = "0.22"
```

### Pattern 2: BackgroundRuntimeDeps — Move to uc-bootstrap (Option B)

**What:** `BackgroundRuntimeDeps` currently lives in `wiring.rs` (uc-tauri). It contains only: `Arc<Libp2pNetworkAdapter>`, `Arc<RepresentationCache>`, `Arc<SpoolManager>`, `mpsc::Receiver<SpoolRequest>`, `mpsc::Receiver<RepresentationId>`, and a few primitive config values. No Tauri types. Moving it to uc-bootstrap is clean.

**When to use:** The struct is the natural "background payload" returned from `wire_dependencies()`. Since that function moves to uc-bootstrap, `BackgroundRuntimeDeps` should move with it.

**Example:**

```rust
// src-tauri/crates/uc-bootstrap/src/assembly.rs
// (Moved from uc-tauri/bootstrap/assembly.rs — already zero tauri imports)

pub struct WiredDependencies {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    pub watcher_control: Arc<dyn uc_platform::ports::WatcherControlPort>,
}

// BackgroundRuntimeDeps moved here FROM wiring.rs (zero Tauri types confirmed)
pub struct BackgroundRuntimeDeps {
    pub libp2p_network: Arc<Libp2pNetworkAdapter>,
    pub representation_cache: Arc<RepresentationCache>,
    pub spool_manager: Arc<SpoolManager>,
    pub spool_rx: mpsc::Receiver<SpoolRequest>,
    pub worker_rx: mpsc::Receiver<RepresentationId>,
    pub spool_dir: PathBuf,
    pub file_cache_dir: PathBuf,
    pub spool_ttl_days: u64,
    pub worker_retry_max_attempts: u32,
    pub worker_retry_backoff_ms: u64,
}
```

**wiring.rs update:**

```rust
// src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
// BackgroundRuntimeDeps is now in uc-bootstrap — re-export for backward compatibility
pub use uc_bootstrap::BackgroundRuntimeDeps;
```

### Pattern 3: Scene-Specific Builder Functions

**What:** Three public builder functions in `builders.rs` sharing a private `build_core()` helper.

**When to use:** Each entry point (GUI, CLI, daemon) calls its specific builder to get a fully-wired context without touching uc-infra or uc-platform directly.

**Example:**

```rust
// src-tauri/crates/uc-bootstrap/src/builders.rs

/// Context returned from build_gui_app(). Contains everything to construct
/// AppRuntime EXCEPT tauri::AppHandle. uc-tauri calls AppRuntime::with_setup(context).
pub struct GuiBootstrapContext {
    pub core_runtime: Arc<CoreRuntime>,
    pub background: BackgroundRuntimeDeps,
    pub watcher_control: Arc<dyn WatcherControlPort>,
    pub setup_ports: SetupAssemblyPorts,
    pub storage_paths: AppPaths,
    pub platform_event_tx: PlatformEventSender,
    pub platform_event_rx: PlatformEventReceiver,
    pub platform_cmd_tx: tokio::sync::mpsc::Sender<PlatformCommand>,
    pub platform_cmd_rx: PlatformCommandReceiver,
    pub pairing_orchestrator: Arc<PairingOrchestrator>,
    pub pairing_action_rx: tokio::sync::mpsc::Receiver<PairingAction>,
    pub staged_store: Arc<StagedPairedDeviceStore>,
    pub space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    pub key_slot_store: Arc<dyn KeySlotStore>,
    pub config: AppConfig,
}

pub fn build_gui_app() -> anyhow::Result<GuiBootstrapContext> {
    init_tracing_subscriber()?;   // idempotent — safe to call multiple times
    let config = resolve_app_config()?;
    // ... wire_dependencies, build orchestrators, create channels
    // uses block_on for async resolution of pairing device name (same as main.rs lines 351-355)
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let (pairing_device_name, pairing_config) = rt.block_on(async {
        let device_name = resolve_pairing_device_name(settings.clone()).await;
        let config = resolve_pairing_config(settings).await;
        (device_name, config)
    });
    Ok(GuiBootstrapContext { ... })
}

pub struct CliBootstrapContext {
    pub core_runtime: Arc<CoreRuntime>,
    pub config: AppConfig,
}

pub fn build_cli_context() -> anyhow::Result<CliBootstrapContext> {
    init_tracing_subscriber()?;
    let config = resolve_app_config()?;
    // wire_dependencies but NO background workers registered
    // Uses LoggingEventEmitter permanently
    Ok(CliBootstrapContext { ... })
}

pub struct DaemonBootstrapContext {
    pub core_runtime: Arc<CoreRuntime>,
    pub background: BackgroundRuntimeDeps,
    pub config: AppConfig,
}

pub fn build_daemon_app() -> anyhow::Result<DaemonBootstrapContext> {
    init_tracing_subscriber()?;
    let config = resolve_app_config()?;
    // wire_dependencies — includes background deps; workers registered but NOT started
    // Uses LoggingEventEmitter permanently
    Ok(DaemonBootstrapContext { ... })
}
```

### Pattern 4: Idempotent init_tracing_subscriber

**What:** Current implementation uses `OnceLock::set().is_err()` which returns `Err` on second call and `try_init()` which fails if subscriber already set. Must be made idempotent for test safety and multi-builder scenarios.

**When to use:** Always — the builder API must be safe without preconditions.

**Example:**

```rust
// src-tauri/crates/uc-bootstrap/src/tracing.rs

static TRACING_INITIALIZED: OnceLock<()> = OnceLock::new();

pub fn init_tracing_subscriber() -> anyhow::Result<()> {
    // Idempotent: second call is a no-op
    if TRACING_INITIALIZED.get().is_some() {
        tracing::debug!("Tracing already initialized, skipping");
        return Ok(());
    }

    // ... existing setup logic ...

    // Replace try_init()? with explicit check
    match tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
        .with(seq_layer)
        .try_init()
    {
        Ok(()) => {}
        Err(_) => {
            // Subscriber already set (e.g. in tests) — treat as success
            tracing::debug!("Global subscriber already set, skipping registration");
            return Ok(());
        }
    }

    let _ = TRACING_INITIALIZED.set(());
    Ok(())
}
```

**Alternative:** Expose `BuilderOptions { skip_tracing: bool }` for test scenarios. Either approach is acceptable.

### Pattern 5: uc-tauri bootstrap/mod.rs Re-exports

**What:** After extraction, uc-tauri's `bootstrap/mod.rs` re-exports from uc-bootstrap to avoid breaking internal callers (commands/\*.rs, runtime.rs, etc.) that use `use uc_tauri::bootstrap::...`.

**When to use:** For backward compatibility during extraction and for items that commands still reference.

**Example:**

```rust
// src-tauri/crates/uc-tauri/src/bootstrap/mod.rs (after extraction)

// Re-exports from uc-bootstrap for backward compatibility
pub use uc_bootstrap::{
    resolve_app_config, resolve_config_path, ConfigResolutionError,
    wire_dependencies, WiredDependencies, BackgroundRuntimeDeps,
    get_storage_paths, resolve_pairing_config, resolve_pairing_device_name,
    SetupAssemblyPorts, build_setup_orchestrator,
    ensure_default_device_name,
    init_tracing_subscriber,
    build_gui_app, GuiBootstrapContext,
    build_cli_context, CliBootstrapContext,
    build_daemon_app, DaemonBootstrapContext,
};

// Items that remain in uc-tauri
pub mod runtime;
pub mod wiring;
pub mod file_transfer_wiring;
pub mod run;
pub mod logging;
pub mod clipboard_integration_mode;
pub mod task_registry;

pub use clipboard_integration_mode::resolve_clipboard_integration_mode;
pub use runtime::{create_app, create_runtime, AppRuntime, AppUseCases};
pub use wiring::{start_background_tasks};
```

### Pattern 6: main.rs Simplification

**What:** After phase, main.rs imports from uc-bootstrap (or uc-tauri re-exports) and calls `build_gui_app()` instead of manually wiring everything. Lines 320-397 of main.rs move into uc-bootstrap.

**Example:**

```rust
// main() after phase
fn main() {
    let config = match uc_bootstrap::resolve_app_config() { ... };
    run_app(config);
}

fn run_app(config: AppConfig) {
    let ctx = match uc_bootstrap::build_gui_app() {
        Ok(ctx) => ctx,
        Err(e) => { error!(...); panic!(...); }
    };

    // Remaining Tauri-specific code: Builder setup, .manage(), plugin registration
    let builder = Builder::default()
        .manage(Arc::new(AppRuntime::with_setup(
            ctx.core_runtime,
            ctx.setup_ports,
            ctx.watcher_control,
            ctx.storage_paths,
            LoggingEventEmitter,
        )))
        // ...
```

**NOTE:** Tracing is now called inside `build_gui_app()`, so `main()` does NOT call `bootstrap_tracing::init_tracing_subscriber()` anymore.

### Anti-Patterns to Avoid

- **Circular dependency:** uc-bootstrap must NEVER depend on uc-tauri. Dependency direction: uc-tauri → uc-bootstrap. Check this explicitly before writing any import.
- **Tauri types leaking into uc-bootstrap:** GuiBootstrapContext must NOT contain `tauri::AppHandle` or any `tauri::*` type. Only channel senders/receivers (platform types), Arc'd domain objects, and config are allowed.
- **Self-referential Arc lifetime:** Do NOT store a pre-constructed `CoreUseCases<'a>` inside any bootstrap context struct. The `'a` lifetime borrows from `CoreRuntime` — you cannot store both in the same struct without unsafe code. Callers call `.usecases()` on the runtime they receive.
- **Duplicating tokio runtimes:** The Seq SEQ_RUNTIME OnceLock pattern from tracing.rs is correct for pre-Tauri async work. Don't create additional runtimes unnecessarily.
- **Double-moving files:** assembly.rs is 42K. It moves as-is. Its import paths need updating (from `use uc_infra::...` etc. — these remain valid since uc-bootstrap depends on all of them).

## Don't Hand-Roll

| Problem                 | Don't Build         | Use Instead                                                                       | Why                                                                         |
| ----------------------- | ------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Idempotent global init  | Custom mutex+bool   | OnceLock<()>                                                                      | Standard library, zero-cost after first call                                |
| Async in sync context   | Custom executor     | tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(...) | Pattern already established in tracing.rs lines 115-121 for Seq; consistent |
| Crate path dependencies | Relative path hacks | path = "../uc-xxx" in Cargo.toml                                                  | Standard Cargo workspace pattern used by all 7 existing crates              |

**Key insight:** The assembly is already written and tested in assembly.rs. Phase 40 is a crate move, not a rewrite. Resist the urge to refactor assembly logic during extraction.

## Common Pitfalls

### Pitfall 1: config_resolution.rs Imports `crate::bootstrap::config::load_config`

**What goes wrong:** `config_resolution.rs` currently has `use crate::bootstrap::config::load_config;` (relative import within uc-tauri). After moving to uc-bootstrap, this becomes `use crate::config::load_config;` (within uc-bootstrap). If the planner creates a task to copy the file verbatim without updating this import, it will fail to compile.

**Why it happens:** The file uses a crate-relative import path.

**How to avoid:** In the task that moves `config_resolution.rs`, explicitly list the import to update: `crate::bootstrap::config` → `crate::config`.

**Warning signs:** `cargo check` error "failed to find module `bootstrap`" in uc-bootstrap.

### Pitfall 2: BackgroundRuntimeDeps Re-export in wiring.rs

**What goes wrong:** `wiring.rs` currently defines `BackgroundRuntimeDeps`. After moving it to uc-bootstrap, `wiring.rs` must re-export it to avoid breaking callers. The re-export in wiring.rs is:

```rust
pub use super::assembly::{..., BackgroundRuntimeDeps, ...};
```

After the move this becomes:

```rust
pub use uc_bootstrap::BackgroundRuntimeDeps;
```

Forgetting this re-export causes compile errors in `start_background_tasks` signature and callers.

**Warning signs:** `cargo check` error "cannot find type BackgroundRuntimeDeps in module wiring".

### Pitfall 3: tauri::async_runtime::block_on in main.rs (lines 351-355)

**What goes wrong:** Lines 351-355 in main.rs use `tauri::async_runtime::block_on` to resolve pairing device name and config. When this code moves into `build_gui_app()`, uc-bootstrap cannot use `tauri::async_runtime` (it has no tauri dep). Must use `tokio::runtime::Builder` instead.

**Why it happens:** Tauri's async_runtime is a thin wrapper around tokio. The code works identically with a standalone tokio runtime.

**How to avoid:** Use the already-established pattern from tracing.rs lines 115-121:

```rust
let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
let result = rt.block_on(async { ... });
```

**Warning signs:** "use of undeclared crate or module `tauri`" in uc-bootstrap build.

### Pitfall 4: OnceLock JSON_GUARD double-set panic

**What goes wrong:** The current tracing.rs uses `if JSON_GUARD.set(guard).is_err() { anyhow::bail!("JSON log guard already initialized"); }` — this is a hard error on second call. In tests or multi-builder scenarios, this causes the second call to fail.

**Why it happens:** The original code was written for single-call-per-process semantics.

**How to avoid:** Implement the idempotency pattern: check `TRACING_INITIALIZED` before doing anything. If already initialized, return `Ok(())` immediately, before any OnceLock operations. The single TRACING_INITIALIZED guard prevents any OnceLock from being hit twice.

### Pitfall 5: uc-tauri Cargo.toml — which deps to keep vs remove

**What goes wrong:** Removing uc-infra and uc-platform from uc-tauri's Cargo.toml when they are still needed (wiring.rs and file_transfer_wiring.rs use uc-infra types; runtime.rs uses uc-platform types for TauriEventEmitter, PlatformRuntime).

**Why it happens:** The goal is to "reduce" direct composition deps, but not eliminate all deps.

**How to avoid:** The CONTEXT.md is explicit: "uc-tauri RETAINS uc-platform dependency for Tauri-specific adapters" and "MAY retain residual uc-infra dependency." Do NOT remove these from uc-tauri's Cargo.toml in this phase. The goal is that uc-tauri no longer uses uc-infra/uc-platform FOR COMPOSITION (assembly) — they remain for their other uses.

### Pitfall 6: Root uniclipboard Cargo.toml also needs updating

**What goes wrong:** The root `src-tauri/Cargo.toml` (the `uniclipboard` binary package) currently lists `uc-infra`, `uc-platform` as direct deps. After this phase, if main.rs calls `uc_bootstrap::build_gui_app()` instead of manually wiring, the root may no longer need its own uc-infra/uc-platform deps (it gets them transitively via uc-tauri → uc-bootstrap). However, main.rs still imports specific types from uc-infra and uc-platform directly (see `use uc_infra::fs::key_slot_store::...` and `use uc_platform::...` at the top of main.rs). Plan for updating root Cargo.toml based on what remains in main.rs after simplification.

**Warning signs:** "unused dependency" warnings, or conversely "unresolved import" if direct dep is removed prematurely.

## Code Examples

Verified patterns from existing codebase:

### Cargo Workspace Crate Declaration Pattern (from src-tauri/Cargo.toml)

```toml
# Existing pattern — every crate follows this
[workspace]
members = [
  "crates/uc-core",
  "crates/uc-app",
  ...
]

# Path dependencies within workspace (from any crate's Cargo.toml)
uc-app = { path = "../uc-app" }
```

### Established OnceLock Guard Pattern (from tracing.rs lines 28-34)

```rust
static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static SEQ_GUARD: OnceLock<SeqGuard> = OnceLock::new();
static SEQ_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
```

### Pre-Tauri Async Block Pattern (from tracing.rs lines 115-121)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs
let rt = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(1)
    .enable_all()
    .build()?;
let layer_result = rt.block_on(async { ... });
```

### Phase 38 Re-export Pattern for Backward Compatibility (from task_registry.rs)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs (128 bytes)
// Entire file is just:
pub use uc_app::task_registry::TaskRegistry;
```

This is the exact pattern uc-tauri/bootstrap/mod.rs should use after extraction.

### CoreRuntime::usecases() Access Pattern (from uc-app/runtime.rs)

```rust
// Source: uc-app/src/runtime.rs
// Callers do:
let uc = runtime.usecases().some_use_case();
// NOT:
// let uc = context.usecases; // pre-stored accessor
```

### wire_dependencies Signature (from assembly.rs — the primary extraction target)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs
// Zero tauri imports, already documented as "ready for extraction to uc-bootstrap in Phase 40"
pub fn wire_dependencies(
    config: &AppConfig,
    platform_cmd_tx: tokio::sync::mpsc::Sender<PlatformCommand>,
) -> WiringResult<WiredDependencies>
```

## State of the Art

| Old Approach                                       | Current Approach                                                | When Changed          | Impact                                       |
| -------------------------------------------------- | --------------------------------------------------------------- | --------------------- | -------------------------------------------- |
| All wiring in wiring.rs (Tauri imports throughout) | Pure assembly in assembly.rs, Tauri event loops in wiring.rs    | Phase 37              | assembly.rs ready to move as-is              |
| CoreRuntime embedded in AppRuntime                 | CoreRuntime extracted to uc-app, AppRuntime wraps it            | Phase 38              | uc-bootstrap only needs to build CoreRuntime |
| config_resolution inline in main.rs                | resolve_app_config() in uc-tauri/bootstrap/config_resolution.rs | Phase 39              | Moves to uc-bootstrap verbatim               |
| Single monolithic init                             | Three scene-specific builders                                   | Phase 40 (this phase) | Enables CLI/daemon without Tauri             |

**Deprecated/outdated:**

- Direct `uc_infra`/`uc_platform` imports in main.rs for composition: These move to uc-bootstrap builders, leaving main.rs with only Tauri-specific imports.

## Open Questions

1. **Whether build_gui_app() should be async or sync**
   - What we know: All callers (main.rs, future uc-daemon) need the result before starting their event loops. Tauri's setup closure can use `block_on`. uc-bootstrap has no tauri async runtime.
   - What's unclear: Whether a single `new_current_thread().block_on()` inside `build_gui_app()` is the right pattern, or if it would conflict with later tokio runtime creation.
   - Recommendation: Keep `build_gui_app()` synchronous; use `tokio::runtime::Builder::new_current_thread()` inside it for async resolution (device name, pairing config). The Tauri async runtime starts later and is independent.

2. **Whether `config.rs` import paths need updating in uc-bootstrap**
   - What we know: `config.rs` uses `use uc_core::config::AppConfig` (external crate, fine) and `toml` (external dep). No `crate::` relative imports visible.
   - Recommendation: Verify `config.rs` imports during implementation — it's small (6K) and likely needs no changes beyond moving.

3. **task_registry.rs (128B re-export stub) fate**
   - What we know: It's `pub use uc_app::task_registry::TaskRegistry`. It's in uc-tauri/bootstrap/ not uc-bootstrap.
   - Recommendation: Leave it in uc-tauri (it's already a thin wrapper). It does not need to move to uc-bootstrap. TaskRegistry is in uc-app.

## Validation Architecture

### Test Framework

| Property           | Value                                                                   |
| ------------------ | ----------------------------------------------------------------------- |
| Framework          | cargo test (no separate test framework config)                          |
| Config file        | none — standard cargo test                                              |
| Quick run command  | `cd src-tauri && cargo test -p uc-bootstrap`                            |
| Full suite command | `cd src-tauri && cargo check && cargo test -p uc-bootstrap -p uc-tauri` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                   | Test Type   | Automated Command                                                        | File Exists? |
| ------- | ---------------------------------------------------------- | ----------- | ------------------------------------------------------------------------ | ------------ |
| BOOT-01 | uc-bootstrap crate compiles with declared deps             | build/check | `cd src-tauri && cargo check -p uc-bootstrap`                            | ❌ Wave 0    |
| BOOT-02 | build_cli_context() returns without starting workers       | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_build_cli_context`   | ❌ Wave 0    |
| BOOT-03 | build_daemon_app() returns with background deps populated  | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_build_daemon_app`    | ❌ Wave 0    |
| BOOT-04 | uc-tauri no longer uses uc-infra for composition           | build/check | `cd src-tauri && cargo check -p uc-tauri`                                | ❌ implied   |
| BOOT-05 | init_tracing_subscriber called once; second call no-op     | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_tracing_idempotent`  | ❌ Wave 0    |
| RNTM-04 | UseCases accessible via runtime.usecases() in all contexts | unit        | `cd src-tauri && cargo test -p uc-bootstrap -- test_usecases_accessible` | ❌ Wave 0    |

**Note:** BOOT-01 and BOOT-04 are verified purely by `cargo check` succeeding. BOOT-02/03 require test helpers since `wire_dependencies` needs a real database path — use `tempfile::TempDir` pattern already established in config_resolution.rs tests.

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check -p uc-bootstrap`
- **Per wave merge:** `cd src-tauri && cargo check && cargo test -p uc-bootstrap`
- **Phase gate:** Full `cargo check` green across all workspace crates before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-bootstrap/src/lib.rs` — crate must exist before any tests can run
- [ ] `src-tauri/crates/uc-bootstrap/Cargo.toml` — must be created in Wave 0 (crate scaffold)
- [ ] `src-tauri/crates/uc-bootstrap/src/builders.rs` — test functions for build_cli_context, build_daemon_app require this module

## Sources

### Primary (HIGH confidence)

- Direct code inspection: `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` — confirmed zero tauri imports, wire_dependencies signature, WiredDependencies struct
- Direct code inspection: `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` — confirmed OnceLock pattern, tokio runtime for async, all static guards
- Direct code inspection: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` (lines 1-104) — confirmed BackgroundRuntimeDeps fields (no Tauri types)
- Direct code inspection: `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` (lines 80-174) — confirmed AppRuntime::with_setup() signature and CoreRuntime construction
- Direct code inspection: `src-tauri/src/main.rs` — confirmed assembly lines 320-397, tauri::async_runtime::block_on usage at lines 351-355
- Direct code inspection: `src-tauri/Cargo.toml` — confirmed workspace members, existing 7 crates pattern
- Direct code inspection: `src-tauri/crates/uc-tauri/Cargo.toml` — confirmed all uc-infra/uc-platform/uc-observability deps present

### Secondary (MEDIUM confidence)

- `.planning/phases/40-uc-bootstrap-crate/40-CONTEXT.md` — locked decisions on crate boundary, builder API, logging strategy
- `.planning/REQUIREMENTS.md` — formal requirement definitions for BOOT-01 through BOOT-05 and RNTM-04
- `.planning/STATE.md` — accumulated decisions from Phases 37-39 explaining why current structure is ready for extraction

### Tertiary (LOW confidence)

- None — all findings verified from codebase directly.

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all packages are already in workspace; no new external deps needed
- Architecture: HIGH — extraction targets confirmed by code inspection; patterns established in prior phases
- Pitfalls: HIGH — specific compile-error scenarios identified from actual import paths in source files
- Builder API design: MEDIUM — GuiBootstrapContext field list is confirmed from main.rs lines 320-397 but exact struct layout is discretionary

**Research date:** 2026-03-18
**Valid until:** 2026-04-18 (stable Rust/Cargo workspace patterns; no fast-moving dependencies)
