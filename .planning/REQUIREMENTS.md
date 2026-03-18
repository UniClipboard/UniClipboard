# Requirements: UniClipboard Desktop

**Defined:** 2026-03-17
**Core Value:** Seamless clipboard synchronization across devices — copy on one, paste on another

## v0.4.0 Requirements

Requirements for runtime mode separation. Each maps to roadmap phases.

### Event Abstraction

- [x] **EVNT-01**: System can deliver host events through an abstract HostEventEmitterPort trait in uc-core/ports
- [x] **EVNT-02**: GUI mode can emit events to Tauri frontend via TauriEventEmitter adapter implementing HostEventEmitterPort
- [x] **EVNT-03**: Non-GUI modes can deliver events to logging output via LoggingEventEmitter adapter
- [x] **EVNT-04**: Background tasks (clipboard watcher, peer discovery, sync scheduler) accept HostEventEmitterPort instead of AppHandle<R: Runtime>

### Runtime Extraction

- [x] **RNTM-01**: CoreRuntime struct exists without Tauri dependency, holding AppDeps and shared orchestrators
- [x] **RNTM-02**: wiring.rs is decomposed into pure assembly module (Tauri-free) and Tauri-specific event loop module
- [x] **RNTM-03**: Configuration resolution functions (path resolve, profile suffix, keyslot dir) extracted from main.rs to reusable module
- [x] **RNTM-04**: UseCases accessor is shared across all entry points (not duplicated per runtime mode)
- [x] **RNTM-05**: SetupOrchestrator assembly unified into main composition root (no secondary wiring in runtime.rs)

### Bootstrap Crate

- [x] **BOOT-01**: uc-bootstrap crate exists as sole composition root, depending on uc-core + uc-app + uc-infra + uc-platform
- [x] **BOOT-02**: uc-bootstrap provides build_cli_context() returning CLI-ready dependencies
- [x] **BOOT-03**: uc-bootstrap provides build_daemon_app() returning daemon-ready dependencies with workers
- [ ] **BOOT-04**: uc-tauri depends on uc-bootstrap instead of directly on uc-infra + uc-platform
- [x] **BOOT-05**: Logging initialization unified in uc-bootstrap (not duplicated per entry point)

### Daemon Skeleton

- [ ] **DAEM-01**: uc-daemon crate exists with DaemonApp struct supporting startup and graceful shutdown
- [ ] **DAEM-02**: Daemon exposes local RPC server with ping and status commands
- [ ] **DAEM-03**: Daemon has DaemonWorker trait abstraction with placeholder workers (clipboard watcher, peer discovery)
- [ ] **DAEM-04**: Daemon maintains RuntimeState with uptime, worker health, and connected peers summary

### CLI Skeleton

- [ ] **CLI-01**: uc-cli crate exists with clap-based argument parsing and subcommand routing
- [ ] **CLI-02**: CLI supports daemon status command via RPC connection to daemon
- [ ] **CLI-03**: CLI supports direct app commands (space status, device list) via uc-bootstrap
- [ ] **CLI-04**: CLI supports --json output mode for machine-consumable output
- [ ] **CLI-05**: CLI uses stable exit codes (0=success, 1=error, 5=daemon unreachable)

## Future Requirements

### Daemon Full Implementation

- **DAEM-F01**: Daemon clipboard watcher worker with full event loop
- **DAEM-F02**: Daemon peer discovery worker with libp2p integration
- **DAEM-F03**: Daemon sync scheduler worker
- **DAEM-F04**: Daemon transfer manager worker
- **DAEM-F05**: Daemon config hot-reload

### CLI Full Implementation

- **CLI-F01**: CLI clipboard history list/show commands
- **CLI-F02**: CLI sync trigger command
- **CLI-F03**: CLI transfer list/show commands
- **CLI-F04**: CLI daemon start/stop commands with process management

## Out of Scope

| Feature                                | Reason                                                            |
| -------------------------------------- | ----------------------------------------------------------------- |
| Cross-machine remote daemon control    | v0.4.0 is local single-machine only                               |
| Stable public RPC API for third-party  | Internal protocol only, can change                                |
| Plugin system                          | Not needed for runtime mode separation                            |
| uc-tauri full decoupling from uc-infra | Incremental — uc-tauri keeps uc-infra as dev-dependency for tests |
| GUI feature changes                    | Pure architecture milestone, no UI changes                        |

## Traceability

| Requirement | Phase | Status   |
| ----------- | ----- | -------- |
| EVNT-01     | 36    | Complete |
| EVNT-02     | 36    | Complete |
| EVNT-03     | 36    | Complete |
| EVNT-04     | 36    | Complete |
| RNTM-02     | 37    | Complete |
| RNTM-01     | 38    | Complete |
| RNTM-05     | 38    | Complete |
| RNTM-03     | 39    | Complete |
| BOOT-01     | 40    | Complete |
| BOOT-02     | 40    | Complete |
| BOOT-03     | 40    | Complete |
| BOOT-04     | 40    | Pending  |
| BOOT-05     | 40    | Complete |
| RNTM-04     | 40    | Complete |
| DAEM-01     | 41    | Pending  |
| DAEM-02     | 41    | Pending  |
| DAEM-03     | 41    | Pending  |
| DAEM-04     | 41    | Pending  |
| CLI-01      | 41    | Pending  |
| CLI-02      | 41    | Pending  |
| CLI-03      | 41    | Pending  |
| CLI-04      | 41    | Pending  |
| CLI-05      | 41    | Pending  |

**Coverage:**

- v0.4.0 requirements: 23 total
- Mapped to phases: 23
- Unmapped: 0 ✓

---

_Requirements defined: 2026-03-17_
_Last updated: 2026-03-17 after roadmap creation_
