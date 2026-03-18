# Roadmap: UniClipboard Desktop

## Milestones

- ✅ **v0.1.0 Daily Driver** — Phases 1-9 (shipped 2026-03-06)
- ✅ **v0.2.0 Architecture Remediation** — Phases 10-18 (shipped 2026-03-09)
- ✅ **v0.3.0 Log Observability & Feature Expansion** — Phases 19-35 (shipped 2026-03-17)
- 🚧 **v0.4.0 Runtime Mode Separation** — Phases 36-41 (in progress)

## Phases

<details>
<summary>✅ v0.1.0 Daily Driver (Phases 1-9) — SHIPPED 2026-03-06</summary>

See: `.planning/milestones/v0.1.0-ROADMAP.md`

</details>

<details>
<summary>✅ v0.2.0 Architecture Remediation (Phases 10-18) — SHIPPED 2026-03-09</summary>

See: `.planning/milestones/v0.2.0-ROADMAP.md`

</details>

<details>
<summary>✅ v0.3.0 Log Observability & Feature Expansion (Phases 19-35) — SHIPPED 2026-03-17</summary>

See: `.planning/milestones/v0.3.0-ROADMAP.md`

- [x] Phase 19: Dual Output Logging Foundation (2/2 plans)
- [x] Phase 20: Clipboard Capture Flow Correlation (3/3 plans)
- [x] Phase 21: Sync Flow Correlation (2/2 plans)
- [x] Phase 22: Seq Local Visualization (2/2 plans)
- [x] Phase 23: Distributed Tracing (2/2 plans)
- [x] Phase 24: Per-Device Sync Settings (3/3 plans)
- [x] Phase 25: Content Type Toggles (2/2 plans)
- [x] Phase 26: Global Sync Master Toggle (2/2 plans)
- [x] Phase 27: Keyboard Shortcuts Settings (1/2 plans — code complete, doc gap)
- [x] Phase 28: File Sync Foundation + Link Content Type (5/5 plans)
- [x] Phase 29: macOS Keychain Modal (2/2 plans)
- [x] Phase 30: File Transfer Service (4/4 plans)
- [x] Phase 31: File Sync UI (3/3 plans)
- [x] Phase 32: File Sync Settings (3/3 plans)
- [x] Phase 32.1: File Clipboard Integration (3/3 plans)
- [x] Phase 33: File Sync Eventual Consistency (6/6 plans)
- [x] Phase 34: Event-Driven Device Discovery (2/2 plans)
- [x] Phase 35: OutboundSyncPlanner Extraction (2/2 plans)

</details>

### 🚧 v0.4.0 Runtime Mode Separation (In Progress)

**Milestone Goal:** Extract non-Tauri logic from uc-tauri into shared crates, enabling GUI, CLI, and daemon as independent runtime modes with a single composition root.

#### Phases

- [x] **Phase 36: Event Emitter Abstraction** - Replace hardcoded AppHandle::emit() with HostEventEmitterPort trait and adapters (completed 2026-03-17)
- [x] **Phase 37: Wiring Decomposition** - Split wiring.rs into pure assembly module and Tauri-specific event loop module (completed 2026-03-17)
- [x] **Phase 38: CoreRuntime Extraction** - Extract Tauri-free CoreRuntime and unify SetupOrchestrator into single composition point (completed 2026-03-18)
- [x] **Phase 39: Config Resolution Extraction** - Move path/profile/keyslot resolution from main.rs into reusable bootstrap module (completed 2026-03-18)
- [ ] **Phase 40: uc-bootstrap Crate** - Create sole composition root crate with scene-specific builders and unified logging init
- [x] **Phase 41: Daemon and CLI Skeletons** - Create uc-daemon and uc-cli crates with end-to-end path validation (completed 2026-03-18)

## Phase Details

### Phase 36: Event Emitter Abstraction

**Goal**: Background tasks deliver host events through an abstract port, eliminating direct AppHandle coupling
**Depends on**: Nothing (first phase of v0.4.0)
**Requirements**: EVNT-01, EVNT-02, EVNT-03, EVNT-04
**Success Criteria** (what must be TRUE):

1. HostEventEmitterPort trait exists in uc-core/ports and compiles without any Tauri dependency
2. TauriEventEmitter adapter wraps AppHandle and implements HostEventEmitterPort; GUI app continues to emit clipboard and sync events to the frontend as before
3. LoggingEventEmitter adapter exists and implements HostEventEmitterPort by writing events to tracing output
4. Clipboard watcher, peer discovery, and sync scheduler accept HostEventEmitterPort instead of AppHandle<R>; the compiler rejects any direct AppHandle use in these components

**Plans:** 2/2 plans complete

Plans:

- [ ] 36-01-PLAN.md — Define HostEventEmitterPort trait, HostEvent type system, TauriEventEmitter and LoggingEventEmitter adapters with contract tests
- [ ] 36-02-PLAN.md — Wire emitter port into AppRuntime, wiring.rs, and file_transfer_wiring.rs; delete obsolete event types

### Phase 37: Wiring Decomposition

**Goal**: wiring.rs is split into a pure Rust assembly module (no Tauri types) and a thin Tauri-specific event loop layer
**Depends on**: Phase 36
**Requirements**: RNTM-02
**Success Criteria** (what must be TRUE):

1. A new pure-assembly module exists that constructs application dependencies without importing any tauri crate; it compiles as a library with no Tauri feature flags
2. A separate Tauri-specific module (wiring.rs) owns the Tauri event loop setup, app handle wiring, and command registration; within the wiring split pair, it is the only module that imports tauri types (assembly.rs has zero tauri imports)
3. Existing GUI behavior is unchanged: clipboard sync, pairing, and settings all continue to function after the split
4. assembly.rs contains zero tauri imports (verified by CI lint) and its public API is Tauri-type-free, preparing it for independent `cargo check` in Phase 40 when uc-bootstrap crate is created

**Plans:** 5/5 plans complete

Plans:

- [x] 37-01-PLAN.md — Define PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent in uc-core; extend TauriEventEmitter + LoggingEventEmitter with contract tests
- [x] 37-02-PLAN.md — Migrate all app.emit() calls in wiring.rs and file_transfer_wiring.rs to HostEventEmitterPort
- [x] 37-03-PLAN.md — Split wiring.rs into assembly.rs + wiring.rs, remove AppHandle from start_background_tasks, update ROADMAP

### Phase 38: CoreRuntime Extraction

**Goal**: A Tauri-free CoreRuntime struct holds AppDeps and orchestrators; SetupOrchestrator assembly lives in one composition point
**Depends on**: Phase 37
**Requirements**: RNTM-01, RNTM-05
**Success Criteria** (what must be TRUE):

1. CoreRuntime struct exists in a crate with no Tauri dependency, holds AppDeps and shared orchestrators, and passes `cargo check` independently
2. AppRuntime wraps CoreRuntime and adds only Tauri-specific handles; no orchestration logic lives in AppRuntime itself
3. SetupOrchestrator is assembled exactly once in the main composition root; runtime.rs contains no secondary wiring or orchestrator construction
4. The existing GUI setup flow (first-run setup, encrypted space unlock) continues to work end-to-end

**Plans:** 4 plans (3 complete + 1 gap closure)

Plans:

- [ ] 38-01-PLAN.md — Move TaskRegistry and lifecycle adapters from uc-tauri to uc-app (prerequisites)
- [ ] 38-02-PLAN.md — Create CoreRuntime in uc-app, refactor AppRuntime to wrap it, fix stale emitter bug
- [ ] 38-03-PLAN.md — Split UseCases into CoreUseCases + AppUseCases, extract SetupOrchestrator to assembly.rs

### Phase 39: Config Resolution Extraction

**Goal**: Path resolution, profile suffix derivation, and keyslot directory logic are extracted from main.rs into a reusable, testable module
**Depends on**: Phase 38
**Requirements**: RNTM-03
**Success Criteria** (what must be TRUE):

1. A dedicated config resolution module (not main.rs) owns all path/profile/keyslot resolution functions and is accessible to non-Tauri entry points
2. main.rs delegates to the module rather than containing inline resolution logic; main.rs shrinks accordingly
3. The resolution functions are unit-testable without a running Tauri app
4. GUI app launches and resolves config paths correctly after the extraction

**Plans:** 2/2 plans complete

Plans:

- [ ] 39-01-PLAN.md — Create config_resolution.rs module with resolve_config_path, resolve_app_config, ConfigResolutionError, and migrated tests
- [ ] 39-02-PLAN.md — Wire main.rs to use config_resolution module, delete duplicate functions, consolidate key_slot_store path resolution

### Phase 40: uc-bootstrap Crate

**Goal**: uc-bootstrap exists as the sole composition root; all entry points depend on it instead of wiring uc-infra and uc-platform directly
**Depends on**: Phase 39
**Requirements**: BOOT-01, BOOT-02, BOOT-03, BOOT-04, BOOT-05, RNTM-04
**Success Criteria** (what must be TRUE):

1. uc-bootstrap crate exists with declared dependencies on uc-core, uc-app, uc-infra, and uc-platform; `cargo check` passes
2. build_cli_context() function exists in uc-bootstrap and returns a CLI-ready dependency set without starting background workers
3. build_daemon_app() function exists in uc-bootstrap and returns daemon-ready dependencies with worker handles
4. uc-tauri's Cargo.toml lists uc-bootstrap as a dependency and no longer directly depends on uc-infra or uc-platform for composition
5. UseCases accessor is instantiated inside uc-bootstrap and shared to all entry points; no entry point constructs its own UseCases
6. Logging initialization (init_tracing_subscriber) is called exactly once inside uc-bootstrap; duplicate calls in main.rs or other entry points are removed

**Plans:** 2/3 plans executed

Plans:

- [ ] 40-01-PLAN.md — Create uc-bootstrap crate, move assembly/config/tracing/init modules, make tracing idempotent, wire uc-tauri re-exports
- [ ] 40-02-PLAN.md — Create scene-specific builders (build_gui_app, build_cli_context, build_daemon_app) with shared build_core helper
- [ ] 40-03-PLAN.md — Migrate main.rs to use build_gui_app(), update root Cargo.toml, verify GUI app functions correctly

### Phase 41: Daemon and CLI Skeletons

**Goal**: uc-daemon and uc-cli crates exist with working startup paths, end-to-end RPC connectivity, and direct command execution validated
**Depends on**: Phase 40
**Requirements**: DAEM-01, DAEM-02, DAEM-03, DAEM-04, CLI-01, CLI-02, CLI-03, CLI-04, CLI-05
**Success Criteria** (what must be TRUE):

1. uc-daemon binary starts, initializes via uc-bootstrap, logs startup, and shuts down gracefully on SIGTERM/Ctrl-C without panicking
2. Running `uniclipboard-daemon` then `uniclipboard-cli status` returns a JSON response containing uptime and worker health fields
3. Running `uniclipboard-cli devices` (direct mode via uc-bootstrap) returns a device list without the daemon running
4. `uniclipboard-cli --json status` outputs valid JSON; `uniclipboard-cli status` (no flag) outputs human-readable text
5. Exit codes are stable: 0 on success, 1 on error, 5 when daemon is unreachable
6. DaemonWorker trait exists; placeholder clipboard watcher and peer discovery workers implement it and are registered with DaemonApp

**Plans:** 4/4 plans complete

Plans:

- [ ] 41-01-PLAN.md — Create LoggingHostEventEmitter + build_non_gui_runtime() in uc-bootstrap; create uc-daemon crate with DaemonWorker trait, placeholder workers, RPC types, RuntimeState
- [ ] 41-02-PLAN.md — Create RPC server, handler, DaemonApp lifecycle, and daemon main.rs entry point
- [ ] 41-03-PLAN.md — Create uc-cli crate with clap parsing, status command (RPC), devices and space-status commands (direct mode), --json flag, exit codes
- [ ] 41-04-PLAN.md — Fix socket path SUN_LEN overflow: extract shared resolve_daemon_socket_path to uc-daemon, wire daemon and CLI (gap closure)

## Progress

| Phase                            | Milestone | Plans Complete | Status     | Completed  |
| -------------------------------- | --------- | -------------- | ---------- | ---------- |
| 1-9                              | v0.1.0    | 17/17          | Complete   | 2026-03-06 |
| 10-18                            | v0.2.0    | 22/22          | Complete   | 2026-03-09 |
| 19-35                            | v0.3.0    | 51/51          | Complete   | 2026-03-17 |
| 36. Event Emitter Abstraction    | 2/2       | Complete       | 2026-03-17 | -          |
| 37. Wiring Decomposition         | 5/5       | Complete       | 2026-03-18 | 2026-03-17 |
| 38. CoreRuntime Extraction       | 3/3       | Complete       | 2026-03-18 | -          |
| 39. Config Resolution Extraction | 2/2       | Complete       | 2026-03-18 | -          |
| 40. uc-bootstrap Crate           | 2/3       | In Progress    |            | -          |
| 41. Daemon and CLI Skeletons     | 4/4       | Complete       | 2026-03-18 | -          |
