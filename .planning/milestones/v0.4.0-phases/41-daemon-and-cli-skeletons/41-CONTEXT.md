# Phase 41: Daemon and CLI Skeletons - Context

**Gathered:** 2026-03-18
**Status:** Ready for planning
**Mode:** Auto-resolved (all recommended defaults)

<domain>
## Phase Boundary

Create `uc-daemon` and `uc-cli` as independent binary crates that validate the end-to-end path from entry point through `uc-bootstrap` to working functionality. The daemon runs background workers with a local RPC server; the CLI dispatches commands either directly (via uc-bootstrap) or through the daemon's RPC interface. This phase produces working skeletons — placeholder workers, minimal RPC surface, and stable exit codes — NOT full-featured implementations.

**In scope:**

- `uc-daemon` crate: DaemonApp struct, DaemonWorker trait, placeholder workers, Unix socket JSON-RPC server, graceful shutdown, RuntimeState with uptime/health/peers
- `uc-cli` crate: clap-based argument parsing, `status` subcommand (via daemon RPC), `devices` and `space-status` subcommands (direct mode via uc-bootstrap), `--json` output flag, stable exit codes
- Shared RPC types (request/response structs) accessible to both crates
- Workspace Cargo.toml updates (new members)
- End-to-end validation: daemon start → CLI status → JSON response with uptime and worker health

**Out of scope:**

- Full worker implementations (clipboard watcher event loop, peer discovery with libp2p)
- Daemon process management (daemonize, PID files, launchd/systemd integration)
- Config hot-reload
- Remote daemon control
- GUI changes
- Stable public RPC API (internal protocol, can change)

</domain>

<decisions>
## Implementation Decisions

### RPC transport — Unix domain socket with JSON-RPC 2.0

- Unix domain socket on macOS/Linux (Windows named pipe support deferred to a future phase)
- Socket path: `{app_data_dir}/uniclipboard-daemon.sock` (resolved via uc-bootstrap config)
- Protocol: minimal JSON-RPC 2.0 over newline-delimited JSON (one request per line)
- No external RPC framework — use `tokio::net::UnixListener` + `serde_json` directly
- Methods for skeleton: `ping`, `status`, `device_list`
- Can upgrade to tonic/tarpc later if command surface grows significantly

### Binary structure — separate workspace crates

- `src-tauri/crates/uc-daemon/` → produces `uniclipboard-daemon` binary
- `src-tauri/crates/uc-cli/` → produces `uniclipboard-cli` binary
- Both depend on `uc-bootstrap` for dependency wiring and `uc-core` for domain types
- Shared RPC types (request/response enums, socket path resolution) live in a `rpc` module within `uc-daemon`, re-exported as library. `uc-cli` depends on `uc-daemon` as a library dependency (not binary) for these shared types
- `uc-daemon` has both `[[bin]]` and `[lib]` sections in Cargo.toml
- clap derive API for CLI argument parsing (consistent with existing uc-clipboard-probe)

### DaemonWorker trait — async start/stop/health_check

- Trait: `name() -> &str`, `start() -> Result<()>`, `stop() -> Result<()>`, `health_check() -> WorkerHealth`
- Workers receive a `CancellationToken` (from `tokio_util`) at start time — they must select on the token for cooperative shutdown. This integrates with the existing TaskRegistry pattern from Phase 38
- `WorkerHealth` enum: `Healthy`, `Degraded(String)`, `Stopped`
- `DaemonApp` struct owns `Vec<Box<dyn DaemonWorker>>`, manages lifecycle
- Placeholder workers: `ClipboardWatcherWorker` and `PeerDiscoveryWorker`
  - `start()` logs "started" and returns Ok
  - `stop()` logs "stopped" and returns Ok
  - `health_check()` always returns `Healthy`
- Workers are registered at construction, started sequentially on `DaemonApp::run()`

### Graceful shutdown — CancellationToken cascade

- `tokio::signal::ctrl_c()` + Unix SIGTERM handler
- Triggers `CancellationToken` shared with all workers and RPC server
- Shutdown sequence: stop accepting new RPC connections → stop workers in reverse order → clean up socket file → exit 0
- Matches existing TaskRegistry pattern from Phase 38

### Stale socket handling on daemon startup

- Before binding the Unix socket, check if a daemon is already running by attempting a `ping` RPC to the existing socket
- If ping succeeds → another daemon is running, exit with error "daemon already running"
- If ping fails (connection refused / timeout) → stale socket from a crash, delete it and proceed with bind
- If socket file doesn't exist → proceed normally
- This prevents `bind()` failures after unclean shutdowns (kill -9, crash)

### CLI command routing — dual dispatch

- `uniclipboard-cli status` → connects to daemon via Unix socket, sends JSON-RPC `status` request
- `uniclipboard-cli devices` → uses `build_cli_context()` directly, constructs CoreRuntime, queries device list via `CoreUseCases::list_paired_devices()` without daemon
- `uniclipboard-cli space-status` → uses `build_cli_context()` directly, constructs CoreRuntime, queries encryption/space status via CoreRuntime runtime-level methods (e.g., `encryption_state()`, `is_encryption_ready()`) without daemon
- `--json` global flag: outputs JSON on stdout; default: human-readable key-value text
- Human-readable format: simple key-value lines (no table/color libraries for skeleton)

### Exit codes — stable contract

- 0: success
- 1: general error (invalid args, runtime failure)
- 5: daemon unreachable (connection refused / socket not found)
- Exit code constants defined in `uc-cli` as named constants

### Status response fields

- `uptime_seconds: u64` — seconds since daemon start
- `version: String` — from Cargo package version
- `workers: Vec<WorkerStatus>` where `WorkerStatus { name, health }`
- `connected_peers: Option<u32>` — count of currently connected peers (sourced from PeerDirectoryPort; `null` if query failed, with error logged)
- Human-readable example:
  ```
  Status: running
  Uptime: 2h 15m
  Workers: 2/2 healthy
    clipboard-watcher: healthy
    peer-discovery: healthy
  Connected peers: 0
  ```

### Non-GUI CoreRuntime assembly path

- `CoreRuntime::new()` requires: `AppDeps`, emitter cell (`Arc<RwLock<Arc<dyn HostEventEmitterPort>>>`), `LifecycleStatusPort`, `SetupOrchestrator`, `ClipboardIntegrationMode`, `TaskRegistry`, `AppPaths`
- Bootstrap contexts currently return `AppDeps` + `config` (CLI) or `AppDeps` + `BackgroundRuntimeDeps` + channels (daemon), but NOT a ready-to-use CoreRuntime
- Non-GUI modes need a lightweight assembly path: create a logging-only `HostEventEmitterPort` implementation as permanent emitter (no swap), `InMemoryLifecycleStatus`, fresh `TaskRegistry`, and a minimal or no-op `SetupOrchestrator`
- **IMPORTANT**: `LoggingEventEmitter` currently lives in `uc-tauri` (not accessible from uc-bootstrap/uc-daemon/uc-cli). A non-GUI emitter implementation must be created in `uc-app` or `uc-bootstrap` — or the existing one moved out of uc-tauri. This is a prerequisite for non-GUI CoreRuntime construction
- Options: (a) add `build_cli_runtime()` / `build_daemon_runtime()` helpers to `uc-bootstrap` that return `CoreRuntime` directly, or (b) extend existing bootstrap contexts to expose enough for callers to construct CoreRuntime. Planner decides which approach fits best
- `DaemonBootstrapContext` already has `storage_paths`; `CliBootstrapContext` needs it added or resolved inline

### Claude's Discretion

- Internal module organization within uc-daemon and uc-cli
- Exact JSON-RPC message framing (newline-delimited vs length-prefixed)
- Whether to use `async-trait` crate or Rust native async traits for DaemonWorker
- Error type design (anyhow vs custom error enums)
- Test structure and mock strategies
- Whether `DaemonApp::run()` starts workers concurrently or sequentially
- Exact CoreRuntime assembly approach for non-GUI modes (see above)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements

- `.planning/REQUIREMENTS.md` — DAEM-01 through DAEM-04, CLI-01 through CLI-05 define success criteria

### Phase context (prior decisions)

- `.planning/phases/40-uc-bootstrap-crate/40-CONTEXT.md` — uc-bootstrap as sole composition root, builder API surface, GuiBootstrapContext/CliBootstrapContext/DaemonBootstrapContext
- `.planning/phases/38-coreruntime-extraction/38-CONTEXT.md` — CoreRuntime in uc-app, CoreUseCases accessors, TaskRegistry with CancellationToken

### Roadmap

- `.planning/ROADMAP.md` — Phase 41 success criteria (6 items), requirements mapping

### Bootstrap entry points (primary integration targets)

- `src-tauri/crates/uc-bootstrap/src/builders.rs` — `build_cli_context()`, `build_daemon_app()`, context structs
- `src-tauri/crates/uc-bootstrap/src/lib.rs` — Public API re-exports

### Existing patterns to follow

- `src-tauri/crates/uc-clipboard-probe/Cargo.toml` — Existing binary crate with clap derive (reference for Cargo.toml structure)
- `src-tauri/crates/uc-app/src/runtime.rs` — CoreRuntime struct (daemon will construct this)
- `src-tauri/crates/uc-app/src/task_registry.rs` — TaskRegistry + CancellationToken pattern (reuse for daemon shutdown)

### Workspace configuration

- `src-tauri/Cargo.toml` — Workspace members array (add uc-daemon, uc-cli)

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `uc-bootstrap::build_daemon_app()` — Returns `DaemonBootstrapContext` with AppDeps, BackgroundRuntimeDeps, platform channels, watcher control. Daemon binary calls this directly.
- `uc-bootstrap::build_cli_context()` — Returns `CliBootstrapContext` with AppDeps + config. CLI uses for direct-mode commands.
- `TaskRegistry` (uc-app) — CancellationToken-based task lifecycle. Daemon can reuse the cancel-on-shutdown pattern.
- `uc-clipboard-probe` — Existing binary crate with `clap = "4.5"` derive. Reference for Cargo.toml structure and workspace binary patterns.
- `CoreRuntime` (uc-app) — Tauri-free runtime core. Daemon constructs this from `DaemonBootstrapContext.deps`.

### Established Patterns

- Crate-based modularization: each crate in `src-tauri/crates/`
- Port/adapter pattern: domain logic via uc-core traits, adapters in uc-infra/uc-platform
- Non-GUI modes use a logging-only `HostEventEmitterPort` implementation permanently (no emitter swap). Note: current `LoggingEventEmitter` is in `uc-tauri` — must be moved or recreated in uc-app/uc-bootstrap for daemon/CLI use
- Idempotent tracing init in uc-bootstrap (OnceLock guard)
- `AppDeps` as the dependency bundle from `wire_dependencies()`

### Integration Points

- `src-tauri/Cargo.toml` workspace members — add `crates/uc-daemon` and `crates/uc-cli`
- `DaemonBootstrapContext.deps` → construct `CoreRuntime` → access domain use cases
- `CliBootstrapContext.deps` → construct `CoreRuntime` → access `CoreUseCases` → execute direct commands (e.g., device list, space status)
- Socket path resolution — use config from `resolve_app_config()` or platform app data dir

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. The phase is a straightforward skeleton creation leveraging the bootstrap infrastructure built in Phases 38-40.

</specifics>

<deferred>
## Deferred Ideas

- Windows named pipe transport for daemon RPC — Unix socket only in skeleton phase, Windows support in a future phase

</deferred>

---

_Phase: 41-daemon-and-cli-skeletons_
_Context gathered: 2026-03-18_
