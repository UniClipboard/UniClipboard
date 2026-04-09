# Phase 41: Daemon and CLI Skeletons - Research

**Researched:** 2026-03-18
**Domain:** Rust binary crates, Unix socket JSON-RPC, Tokio async runtime, clap CLI
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **RPC transport**: Unix domain socket, JSON-RPC 2.0, newline-delimited JSON. No external RPC framework — use `tokio::net::UnixListener` + `serde_json` directly. Methods: `ping`, `status`, `device_list`. Socket path: `{app_data_dir}/uniclipboard-daemon.sock`.
- **Binary structure**: `src-tauri/crates/uc-daemon/` → `uniclipboard-daemon`; `src-tauri/crates/uc-cli/` → `uniclipboard-cli`. Both depend on `uc-bootstrap`. Shared RPC types live in `uc-daemon` lib section; `uc-cli` depends on `uc-daemon` as library.
- **DaemonWorker trait**: `name() -> &str`, `start(CancellationToken) -> Result<()>`, `stop() -> Result<()>`, `health_check() -> WorkerHealth`. `WorkerHealth` enum: `Healthy`, `Degraded(String)`, `Stopped`. Placeholder workers: `ClipboardWatcherWorker`, `PeerDiscoveryWorker`.
- **Graceful shutdown**: `tokio::signal::ctrl_c()` + SIGTERM → `CancellationToken` cascade. Sequence: stop accepting RPC → stop workers in reverse order → delete socket → exit 0.
- **Stale socket handling**: Ping existing socket on startup; if ping succeeds → error "already running"; if fails → delete stale socket and proceed.
- **CLI routing**: `status` → daemon RPC; `devices` and `space-status` → direct mode via `build_cli_context()` + `CoreRuntime`.
- **Output flag**: `--json` for machine-readable JSON; default human-readable key-value text.
- **Exit codes**: 0 = success, 1 = general error, 5 = daemon unreachable. Named constants in `uc-cli`.
- **Status response fields**: `uptime_seconds: u64`, `version: String`, `workers: Vec<WorkerStatus { name, health }>`, `connected_peers: Option<u32>`.
- **Non-GUI CoreRuntime assembly**: Logging-only `HostEventEmitterPort` must be created in `uc-app` or `uc-bootstrap` (current `LoggingEventEmitter` lives in `uc-tauri`, inaccessible). Planner decides exact assembly approach (helper fns vs extended context).
- **clap derive API**: Consistent with `uc-clipboard-probe`.

### Claude's Discretion

- Internal module organization within uc-daemon and uc-cli
- Exact JSON-RPC message framing (newline-delimited vs length-prefixed)
- Whether to use `async-trait` crate or Rust native async traits for DaemonWorker
- Error type design (anyhow vs custom error enums)
- Test structure and mock strategies
- Whether `DaemonApp::run()` starts workers concurrently or sequentially
- Exact CoreRuntime assembly approach for non-GUI modes

### Deferred Ideas (OUT OF SCOPE)

- Windows named pipe transport for daemon RPC
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                    | Research Support                                                                                     |
| ------- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| DAEM-01 | uc-daemon crate with DaemonApp struct supporting startup and graceful shutdown | Binary crate structure pattern from uc-clipboard-probe; CancellationToken from existing TaskRegistry |
| DAEM-02 | Daemon exposes local RPC server with ping and status commands                  | `tokio::net::UnixListener` + `serde_json`; newline-delimited JSON framing                            |
| DAEM-03 | DaemonWorker trait abstraction with placeholder workers                        | `async-trait` already in workspace; CancellationToken pattern already in uc-app                      |
| DAEM-04 | RuntimeState with uptime, worker health, connected peers summary               | `std::time::Instant` for uptime; `PeerDirectoryPort` for peer count                                  |
| CLI-01  | uc-cli crate with clap-based argument parsing and subcommand routing           | `clap = "4.5"` with derive feature, same as uc-clipboard-probe                                       |
| CLI-02  | CLI supports daemon status via RPC                                             | `tokio::net::UnixStream` connect + send JSON-RPC request                                             |
| CLI-03  | CLI supports direct app commands via uc-bootstrap                              | `build_cli_context()` returns `CliBootstrapContext`; construct `CoreRuntime` + `CoreUseCases`        |
| CLI-04  | CLI supports --json output mode                                                | Global clap flag, serialize with `serde_json::to_string_pretty`                                      |
| CLI-05  | Stable exit codes (0=success, 1=error, 5=daemon unreachable)                   | `std::process::exit()` with named constants                                                          |

</phase_requirements>

## Summary

Phase 41 creates two new binary crates (`uc-daemon` and `uc-cli`) that build on the `uc-bootstrap` composition root established in Phase 40. The key challenge is not the binary crate structure (that pattern is already established by `uc-clipboard-probe`) but the non-GUI `CoreRuntime` assembly path: the only `HostEventEmitterPort` implementation that does not require Tauri lives in `uc-tauri/src/adapters/host_event_emitter.rs` as `LoggingEventEmitter`. This type is inaccessible from `uc-daemon` and `uc-cli` because they must not depend on `uc-tauri`. A logging-only emitter must be created in `uc-app` or `uc-bootstrap` before `CoreRuntime` can be constructed in non-GUI mode.

The RPC layer is intentionally minimal: newline-delimited JSON over a Unix domain socket, three methods (`ping`, `status`, `device_list`), no external framework. All required Tokio primitives (`UnixListener`, `UnixStream`, `signal::ctrl_c`, `signal::unix::signal`) are available through `tokio = { features = ["full"] }` already present in the workspace.

The `DaemonWorker` trait requires an async `start()` method. Since Rust 1.75 supports async-in-traits natively (Return Position Impl Trait in traits), the `async-trait` crate is optional — but `async-trait` is already a workspace dependency and avoids RPITIT complexity. Planner should choose based on Rust edition constraints (edition = "2021" in all crates, which predates RPITIT stabilization — use `async-trait`).

**Primary recommendation:** Create `LoggingHostEventEmitter` in `uc-app` (or re-export from `uc-bootstrap`), add `build_non_gui_runtime()` helper to `uc-bootstrap` that returns a `CoreRuntime` ready for CLI/daemon use, then build the two binary crates using `uc-bootstrap` as sole entry point.

## Standard Stack

### Core

| Library     | Version    | Purpose                                      | Why Standard                                        |
| ----------- | ---------- | -------------------------------------------- | --------------------------------------------------- |
| tokio       | 1.x (full) | Async runtime, UnixListener, signal handling | Already workspace dep; required for all async ops   |
| serde_json  | 1.x        | JSON-RPC serialization/deserialization       | Already workspace dep; used throughout codebase     |
| clap        | 4.5        | CLI argument parsing with derive macros      | Already used in uc-clipboard-probe, same version    |
| anyhow      | 1.0        | Error handling/propagation                   | Already workspace dep; used in all crates           |
| tracing     | 0.1        | Structured logging                           | Already workspace dep; project standard             |
| async-trait | 0.1        | Async methods in traits (DaemonWorker)       | Already workspace dep; edition 2021 predates RPITIT |
| tokio-util  | 0.7        | CancellationToken for cooperative shutdown   | Already in uc-app and uc-bootstrap                  |

### Supporting

| Library   | Version      | Purpose                                    | When to Use                                |
| --------- | ------------ | ------------------------------------------ | ------------------------------------------ |
| serde     | 1.x (derive) | Derive Serialize/Deserialize for RPC types | Needed for JSON-RPC structs                |
| thiserror | 2.0          | Custom error types for RPC layer           | Optional — anyhow may suffice for skeleton |

**Installation (new crates, no new deps needed):**

```bash
# All required dependencies are already in the workspace
# New crates just reference workspace crates + existing deps
```

**Version verification:** All versions confirmed from existing `Cargo.toml` files in the codebase. No new external packages required.

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/
├── uc-daemon/
│   ├── Cargo.toml          # [[bin]] + [lib] sections
│   └── src/
│       ├── main.rs          # Entry point: call build_daemon_app(), run DaemonApp
│       ├── lib.rs           # Re-export rpc module for uc-cli dependency
│       ├── app.rs           # DaemonApp struct, run(), shutdown sequence
│       ├── worker.rs        # DaemonWorker trait, WorkerHealth enum
│       ├── workers/
│       │   ├── clipboard_watcher.rs   # Placeholder ClipboardWatcherWorker
│       │   └── peer_discovery.rs      # Placeholder PeerDiscoveryWorker
│       ├── rpc/
│       │   ├── mod.rs       # Re-export server + types
│       │   ├── server.rs    # UnixListener accept loop, dispatch
│       │   ├── types.rs     # RpcRequest, RpcResponse, StatusResponse, etc.
│       │   └── handler.rs   # Method handlers: ping, status, device_list
│       └── state.rs         # RuntimeState: start_time, workers, peer_count
└── uc-cli/
    ├── Cargo.toml          # [[bin]] only; depends on uc-daemon (lib)
    └── src/
        ├── main.rs          # Entry: parse args, dispatch, exit with code
        ├── commands/
        │   ├── status.rs    # Connect to daemon RPC, print status
        │   ├── devices.rs   # Direct mode: build_cli_context → CoreRuntime → list_paired_devices
        │   └── space_status.rs  # Direct mode: CoreRuntime → encryption_state
        ├── output.rs        # Format JSON vs human-readable, print helpers
        └── exit_codes.rs    # EXIT_SUCCESS=0, EXIT_ERROR=1, EXIT_DAEMON_UNREACHABLE=5
```

### Pattern 1: Dual-Section Cargo.toml (bin + lib)

**What:** `uc-daemon` exposes both a binary entry point and a library for shared RPC types.
**When to use:** When another crate (`uc-cli`) needs to import types from the same crate that produces a binary.
**Example:**

```toml
# src-tauri/crates/uc-daemon/Cargo.toml
[package]
name = "uc-daemon"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "uniclipboard-daemon"
path = "src/main.rs"

[lib]
name = "uc_daemon"
path = "src/lib.rs"

[dependencies]
uc-bootstrap = { path = "../uc-bootstrap" }
uc-app = { path = "../uc-app" }
uc-core = { path = "../uc-core" }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1.0"
async-trait = "0.1"
tokio-util = "0.7"
tracing = "0.1"
```

Reference: `uc-clipboard-probe/Cargo.toml` (existing binary crate pattern)

### Pattern 2: DaemonWorker Trait with async-trait

**What:** Trait that all background workers implement. `start()` receives a `CancellationToken` and runs until cancelled.
**When to use:** All daemon background workers.
**Example:**

```rust
// Source: uc-app/src/task_registry.rs (CancellationToken pattern)
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait DaemonWorker: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    fn health_check(&self) -> WorkerHealth;
}

#[derive(Debug, Clone)]
pub enum WorkerHealth {
    Healthy,
    Degraded(String),
    Stopped,
}
```

### Pattern 3: Unix Socket JSON-RPC Server

**What:** Accept connections on a Unix socket, read one JSON line per connection, dispatch to handler, write JSON response.
**When to use:** Daemon RPC server implementation.
**Example:**

```rust
// Source: tokio documentation pattern
use tokio::net::{UnixListener, UnixStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn run_rpc_server(socket_path: &Path, cancel: CancellationToken) {
    let listener = UnixListener::bind(socket_path).expect("bind");
    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = result.expect("accept");
                tokio::spawn(handle_connection(stream));
            }
            _ = cancel.cancelled() => break,
        }
    }
}

async fn handle_connection(stream: UnixStream) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.ok();
    // parse JSON-RPC request, dispatch, write response
}
```

### Pattern 4: Non-GUI CoreRuntime Assembly

**What:** Construct `CoreRuntime` without Tauri. Requires a `LoggingHostEventEmitter` in `uc-app` or `uc-bootstrap`.
**When to use:** Both `uc-daemon` and `uc-cli` direct-mode commands.
**Example:**

```rust
// Proposed addition to uc-bootstrap/src/builders.rs or uc-app
// LoggingHostEventEmitter: implement HostEventEmitterPort, log events via tracing
pub struct LoggingHostEventEmitter;

impl HostEventEmitterPort for LoggingHostEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        tracing::debug!(event = ?event, "host event (logging-only)");
        Ok(())
    }
}

// Assembly helper (in uc-bootstrap or inside uc-daemon/uc-cli)
pub fn build_non_gui_runtime(
    deps: AppDeps,
    storage_paths: AppPaths,
) -> anyhow::Result<CoreRuntime> {
    let emitter: Arc<dyn HostEventEmitterPort> = Arc::new(LoggingHostEventEmitter);
    let emitter_cell = Arc::new(std::sync::RwLock::new(emitter));
    let lifecycle_status = Arc::new(InMemoryLifecycleStatus::new());
    let task_registry = Arc::new(TaskRegistry::new());
    // SetupOrchestrator: needs a no-op or minimal instance for non-GUI
    // ...
    CoreRuntime::new(deps, emitter_cell, lifecycle_status, setup_orchestrator,
                     ClipboardIntegrationMode::Disabled, task_registry, storage_paths)
}
```

### Pattern 5: Stale Socket Check on Startup

**What:** Before binding, attempt a quick `ping` RPC to the socket. On success → daemon already running → exit error. On connection failure → stale socket → delete and proceed.
**When to use:** `DaemonApp` startup sequence.
**Example:**

```rust
async fn check_or_remove_stale_socket(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    // Try connecting with short timeout
    match tokio::time::timeout(Duration::from_millis(200), UnixStream::connect(path)).await {
        Ok(Ok(_stream)) => {
            anyhow::bail!("daemon already running at {:?}", path);
        }
        _ => {
            // Connection failed or timed out — stale socket
            tracing::warn!("removing stale socket at {:?}", path);
            std::fs::remove_file(path)?;
            Ok(())
        }
    }
}
```

### Pattern 6: Signal Handling for Graceful Shutdown

**What:** Listen for SIGTERM (Unix) and Ctrl-C. Cancel `CancellationToken` to trigger shutdown.
**When to use:** `DaemonApp::run()` main loop.
**Example:**

```rust
// Source: tokio::signal documentation
#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
}
```

### Anti-Patterns to Avoid

- **Depending on uc-tauri from uc-daemon/uc-cli:** Creates a circular dependency path; `uc-tauri` has Tauri as a dependency. Use `uc-bootstrap` which is already Tauri-free.
- **Constructing SetupOrchestrator from scratch in uc-daemon:** Too complex for a skeleton. Use a minimal no-op `SetupOrchestrator` or an empty one.
- **Opening multiple database connections in CLI direct mode:** `build_cli_context()` already handles database wiring via `wire_dependencies()`. Don't re-wire.
- **Holding the Unix socket file descriptor across process restart:** Always remove the socket in shutdown cleanup. Use `defer` pattern or Drop impl.
- **Blocking the RPC accept loop on slow handlers:** Each connection should be `tokio::spawn`-ed immediately.

## Don't Hand-Roll

| Problem                  | Don't Build                   | Use Instead                                                    | Why                                                                      |
| ------------------------ | ----------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Cancellation propagation | Custom shutdown flag          | `CancellationToken` (tokio-util, already in workspace)         | Already proven in TaskRegistry; handles cascade automatically            |
| Async task lifecycle     | Manual JoinHandle tracking    | `TaskRegistry` from uc-app                                     | Already implements join-with-timeout, abort on deadline                  |
| Dependency wiring        | Duplicate wire_dependencies() | `build_daemon_app()` / `build_cli_context()` from uc-bootstrap | Phase 40 built exactly this; using it directly is the point of the phase |
| Signal handling          | Manual signal pipe            | `tokio::signal::ctrl_c()` + `tokio::signal::unix::signal()`    | Tokio provides cross-runtime signal integration                          |
| JSON serialization       | Custom serializer             | `serde_json`                                                   | Already in workspace; handles edge cases                                 |

**Key insight:** The entire purpose of Phases 38-40 was to create reusable infrastructure for exactly this phase. Every architectural piece already exists — this phase is about connecting them correctly, not building new infrastructure.

## Common Pitfalls

### Pitfall 1: LoggingEventEmitter Accessibility

**What goes wrong:** `uc-daemon` and `uc-cli` try to use `LoggingEventEmitter` from `uc-tauri`, but `uc-tauri` depends on Tauri — adding it as a dependency pulls the entire Tauri ecosystem into daemon/CLI.
**Why it happens:** The existing `LoggingEventEmitter` was placed in `uc-tauri/src/adapters/host_event_emitter.rs` alongside `TauriEventEmitter` for convenience.
**How to avoid:** Create `LoggingHostEventEmitter` in `uc-app` (no external deps beyond `tracing`) or in `uc-bootstrap`. This is explicitly called out in CONTEXT.md as a prerequisite.
**Warning signs:** If `uc-daemon/Cargo.toml` lists `uc-tauri` as a dependency — stop and redesign.

### Pitfall 2: SetupOrchestrator Construction in Non-GUI Mode

**What goes wrong:** `CoreRuntime::new()` requires a `SetupOrchestrator`. `build_setup_orchestrator()` in `uc-bootstrap/assembly.rs` requires `SetupAssemblyPorts` which requires `PairingOrchestrator` and other GUI-heavy dependencies.
**Why it happens:** `SetupOrchestrator` was designed for the GUI setup wizard flow.
**How to avoid:** Construct a minimal `SetupOrchestrator` with no-op ports, or add a `SetupOrchestrator::new_noop()` constructor for non-GUI contexts. The daemon and CLI do not need the setup wizard.
**Warning signs:** Compilation error requiring `PairingOrchestrator` in daemon/CLI crates.

### Pitfall 3: Socket Path Resolution

**What goes wrong:** Daemon and CLI use different methods to resolve the socket path, resulting in CLI connecting to the wrong path.
**Why it happens:** `DaemonBootstrapContext` has `storage_paths` (an `AppPaths`) but `CliBootstrapContext` only has `deps` and `config`. The socket path must derive from the same source.
**How to avoid:** Both crates should compute socket path from `AppPaths::data_dir` (or equivalent). Add `storage_paths` to `CliBootstrapContext` if not already present, or compute it from `config` using `get_storage_paths()` (already pub in uc-bootstrap).
**Warning signs:** `uniclipboard-cli status` returns exit code 5 even when daemon is running.

### Pitfall 4: Workspace Member Registration

**What goes wrong:** New crates exist on disk but are not in `src-tauri/Cargo.toml` workspace `members` array, so `cargo build` in `src-tauri/` ignores them.
**Why it happens:** Workspace `[members]` must be updated manually.
**How to avoid:** Add `"crates/uc-daemon"` and `"crates/uc-cli"` to the `members` array in `src-tauri/Cargo.toml` as the first task.
**Warning signs:** `cargo build -p uc-daemon` gives "package not found" error.

### Pitfall 5: RPC Connection from CLI Not Timing Out

**What goes wrong:** `uniclipboard-cli status` hangs indefinitely when daemon is not running because `UnixStream::connect()` blocks until the socket file appears.
**Why it happens:** `UnixStream::connect()` returns `Err` immediately if socket doesn't exist, but calling code may retry or block.
**How to avoid:** Wrap `UnixStream::connect()` in `tokio::time::timeout()` with ~2 second limit. On `Err` or timeout → exit code 5.
**Warning signs:** CLI hangs when daemon is not running instead of printing "daemon unreachable".

### Pitfall 6: `CliBootstrapContext` Missing `storage_paths`

**What goes wrong:** CLI direct-mode commands need `AppPaths` to construct `CoreRuntime` (it's a required parameter), but `CliBootstrapContext` only exposes `deps` and `config`.
**Why it happens:** CONTEXT.md explicitly notes: "`CliBootstrapContext` needs it added or resolved inline".
**How to avoid:** Either (a) call `get_storage_paths(&config)` inline in CLI code (it's pub in uc-bootstrap), or (b) add `storage_paths: AppPaths` to `CliBootstrapContext` in a prerequisite task.
**Warning signs:** CLI direct-mode commands fail to compile without access to `AppPaths`.

## Code Examples

Verified patterns from existing codebase:

### TaskRegistry + CancellationToken (Reuse Pattern)

```rust
// Source: src-tauri/crates/uc-app/src/task_registry.rs
// Workers select on their child token:
async fn worker_run(cancel: CancellationToken) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("worker stopping");
                break;
            }
            _ = do_work() => {}
        }
    }
}
```

### CoreRuntime Construction (Full Signature)

```rust
// Source: src-tauri/crates/uc-app/src/runtime.rs
CoreRuntime::new(
    deps,                          // AppDeps from build_daemon_app().deps
    event_emitter,                 // Arc<RwLock<Arc<dyn HostEventEmitterPort>>>
    lifecycle_status,              // Arc<dyn LifecycleStatusPort>
    setup_orchestrator,            // Arc<SetupOrchestrator>
    clipboard_integration_mode,    // ClipboardIntegrationMode::Disabled for daemon/CLI
    task_registry,                 // Arc<TaskRegistry>
    storage_paths,                 // AppPaths
)
```

### InMemoryLifecycleStatus (Already in uc-app)

```rust
// Source: src-tauri/crates/uc-app/src/usecases/app_lifecycle/adapters.rs
// InMemoryLifecycleStatus is already available — use it for daemon/CLI lifecycle
let lifecycle_status = Arc::new(InMemoryLifecycleStatus::new());
```

### build_daemon_app() Return Type

```rust
// Source: src-tauri/crates/uc-bootstrap/src/builders.rs
// DaemonBootstrapContext has:
//   .deps: AppDeps
//   .background: BackgroundRuntimeDeps
//   .watcher_control: Arc<dyn WatcherControlPort>
//   .platform_cmd_tx / .platform_cmd_rx
//   .platform_event_tx / .platform_event_rx
//   .storage_paths: AppPaths   <-- available for socket path resolution
//   .config: AppConfig
let ctx = build_daemon_app()?;
// socket path: ctx.storage_paths.data_dir.join("uniclipboard-daemon.sock")
```

### CliBootstrapContext Return Type

```rust
// Source: src-tauri/crates/uc-bootstrap/src/builders.rs
// CliBootstrapContext has:
//   .deps: AppDeps
//   .config: AppConfig
// storage_paths NOT included — must call get_storage_paths(&ctx.config) separately
let ctx = build_cli_context()?;
let storage_paths = get_storage_paths(&ctx.config)?;
```

### clap Derive Pattern (from uc-clipboard-probe)

```rust
// Source: src-tauri/crates/uc-clipboard-probe/Cargo.toml — clap = { version = "4.5", features = ["derive"] }
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "uniclipboard-cli")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Status,
    Devices,
    SpaceStatus,
}
```

## State of the Art

| Old Approach                                     | Current Approach                      | When Changed | Impact                                                         |
| ------------------------------------------------ | ------------------------------------- | ------------ | -------------------------------------------------------------- |
| Direct uc-infra/uc-platform deps in entry points | uc-bootstrap as sole composition root | Phase 40     | uc-daemon/uc-cli depend only on uc-bootstrap, not infra layers |
| Manual task lifecycle in entry points            | TaskRegistry + CancellationToken      | Phase 38     | Daemon reuses proven shutdown pattern                          |
| app_handle-based event emission                  | HostEventEmitterPort trait            | Phase 36     | Daemon/CLI can use logging-only emitter without Tauri          |
| Flat wiring in main.rs                           | Decomposed assembly.rs                | Phase 37     | Tauri-free CoreRuntime constructable from non-GUI entry points |

**Deprecated/outdated:**

- Direct `uc-tauri` dependency from non-GUI crates: forbidden (pulls Tauri into binaries)
- `AppRuntime::with_setup()` in non-GUI contexts: this is Tauri-specific, use `CoreRuntime::new()` directly

## Open Questions

1. **SetupOrchestrator for non-GUI CoreRuntime**
   - What we know: `CoreRuntime::new()` requires `Arc<SetupOrchestrator>`. `build_setup_orchestrator()` requires full `SetupAssemblyPorts` (PairingOrchestrator etc.)
   - What's unclear: Does `SetupOrchestrator` have a cheaper no-op constructor? Or must we construct the full orchestrator with stub ports?
   - Recommendation: Add `SetupOrchestrator::new_noop()` or pass a stub `SetupAssemblyPorts` with no-op implementations. Planner should inspect `SetupOrchestrator::new()` signature in `uc-app/src/usecases/setup/orchestrator.rs` to determine the minimal viable construction path.

2. **ClipboardIntegrationMode for daemon**
   - What we know: `CoreRuntime::new()` requires `ClipboardIntegrationMode`. The daemon does run clipboard watching (via workers), but workers are placeholder in this phase.
   - What's unclear: Should daemon use `Disabled`, `Active`, or a new variant?
   - Recommendation: Use `ClipboardIntegrationMode::Disabled` for this skeleton phase since workers are placeholder and do nothing.

3. **`build_daemon_app()` async context requirement**
   - What we know: `build_daemon_app()` calls `build_core()` which initializes tracing and wires deps. CONTEXT.md notes `build_gui_app()` must be called outside a Tokio runtime. `build_daemon_app()` likely has the same constraint.
   - What's unclear: Whether `build_daemon_app()` can be called inside `#[tokio::main]` or must use a pre-runtime pattern.
   - Recommendation: Check if `build_daemon_app()` uses `block_on()` internally (it doesn't appear to in the code — only `build_gui_app()` does). If `build_daemon_app()` is sync-clean, it can be called at the start of `main()` before entering the Tokio runtime, or within `#[tokio::main]` directly.

## Validation Architecture

### Test Framework

| Property           | Value                                               |
| ------------------ | --------------------------------------------------- |
| Framework          | cargo test (built-in Rust testing)                  |
| Config file        | none — standard cargo test                          |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon`           |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon -p uc-cli` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                           | Test Type   | Automated Command                                                        | File Exists? |
| ------- | -------------------------------------------------- | ----------- | ------------------------------------------------------------------------ | ------------ |
| DAEM-01 | DaemonApp starts and shuts down without panic      | unit        | `cd src-tauri && cargo test -p uc-daemon test_daemon_app_lifecycle`      | ❌ Wave 0    |
| DAEM-02 | Ping RPC method returns pong response              | unit        | `cd src-tauri && cargo test -p uc-daemon test_rpc_ping`                  | ❌ Wave 0    |
| DAEM-03 | DaemonWorker placeholder workers return Healthy    | unit        | `cd src-tauri && cargo test -p uc-daemon test_worker_health`             | ❌ Wave 0    |
| DAEM-04 | RuntimeState uptime increases over time            | unit        | `cd src-tauri && cargo test -p uc-daemon test_runtime_state_uptime`      | ❌ Wave 0    |
| CLI-01  | CLI parses status/devices/space-status subcommands | unit        | `cd src-tauri && cargo test -p uc-cli test_cli_arg_parsing`              | ❌ Wave 0    |
| CLI-02  | CLI returns exit code 5 when daemon unreachable    | unit        | `cd src-tauri && cargo test -p uc-cli test_exit_code_daemon_unreachable` | ❌ Wave 0    |
| CLI-03  | Direct mode device list returns without daemon     | integration | `cd src-tauri && cargo test -p uc-cli test_devices_direct_mode`          | ❌ Wave 0    |
| CLI-04  | --json flag produces valid JSON output             | unit        | `cd src-tauri && cargo test -p uc-cli test_json_output_format`           | ❌ Wave 0    |
| CLI-05  | Exit codes are stable named constants              | unit        | `cd src-tauri && cargo test -p uc-cli test_exit_code_constants`          | ❌ Wave 0    |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon -p uc-cli 2>&1 | tail -20`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon -p uc-cli --all-features`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/` — entire crate (new)
- [ ] `src-tauri/crates/uc-cli/src/` — entire crate (new)
- [ ] `src-tauri/Cargo.toml` — add workspace members `crates/uc-daemon`, `crates/uc-cli`

_(Framework: cargo test already works for all existing crates — no new test infrastructure needed)_

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-bootstrap/src/builders.rs` — Builder functions, context structs, DaemonBootstrapContext/CliBootstrapContext
- `src-tauri/crates/uc-app/src/runtime.rs` — CoreRuntime::new() signature, all required parameters
- `src-tauri/crates/uc-app/src/task_registry.rs` — CancellationToken pattern, TaskRegistry API
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — CoreUseCases, list_paired_devices, encryption methods
- `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` — HostEventEmitterPort trait definition
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` — Existing LoggingEventEmitter impl (must be moved)
- `src-tauri/crates/uc-clipboard-probe/Cargo.toml` — Reference binary crate pattern
- `src-tauri/Cargo.toml` — Workspace members, existing dependency versions
- `src-tauri/crates/uc-bootstrap/Cargo.toml` — Available dependencies in composition root

### Secondary (MEDIUM confidence)

- CONTEXT.md decisions section — All architectural decisions locked by prior discussion
- STATE.md accumulated decisions — Phase 38-40 decisions affecting non-GUI CoreRuntime path

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all dependencies verified from existing Cargo.toml files
- Architecture: HIGH — patterns verified from existing code; binary crate structure from uc-clipboard-probe
- Pitfalls: HIGH — LoggingEventEmitter location verified by grep; socket path gap confirmed from CliBootstrapContext source
- Open questions: MEDIUM — SetupOrchestrator no-op path requires inspecting orchestrator.rs (not read in this research pass)

**Research date:** 2026-03-18
**Valid until:** 2026-04-18 (stable codebase, no fast-moving external deps)
