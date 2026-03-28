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

- [x] **DAEM-01**: uc-daemon crate exists with DaemonApp struct supporting startup and graceful shutdown
- [x] **DAEM-02**: Daemon exposes local RPC server with ping and status commands
- [x] **DAEM-03**: Daemon has DaemonWorker trait abstraction with placeholder workers (clipboard watcher, peer discovery)
- [x] **DAEM-04**: Daemon maintains RuntimeState with uptime, worker health, and connected peers summary

### CLI Skeleton

- [x] **CLI-01**: uc-cli crate exists with clap-based argument parsing and subcommand routing
- [x] **CLI-02**: CLI supports daemon status command via RPC connection to daemon
- [x] **CLI-03**: CLI supports direct app commands (space status, device list) via uc-bootstrap
- [x] **CLI-04**: CLI supports --json output mode for machine-consumable output
- [x] **CLI-05**: CLI uses stable exit codes (0=success, 1=error, 5=daemon unreachable)

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

### Daemon API Foundation

- [x] **PH45-01**: Daemon auth token and local connection metadata are persisted outside frontend storage and reusable by local daemon clients
- [x] **PH45-02**: Daemon exposes read-only transport DTOs and query helpers for status, peers, paired devices, and pairing-session summaries without leaking secrets
- [x] **PH45-05**: CLI status and paired-device reads use a shared daemon HTTP client and preserve stable exit semantics for unreachable daemon scenarios
- [x] **PH45-06**: Tauri shell can probe or start the daemon, keep connection info in memory, and emit runtime-only connection metadata without persisting the bearer token in frontend storage

### Daemon Pairing Host Migration

- [x] **PH46-01**: daemon owns pairing orchestrator/session lifecycle
- [x] **PH46-01A**: discoverability and participant readiness are separately controlled
- [x] **PH46-01B**: headless/CLI daemon remains non-discoverable until explicit opt-in
- [x] **PH46-02**: pairing action execution/event handling run daemon-side and survive Tauri/webview disconnects
- [x] **PH46-03**: daemon exposes pairing mutation surface (initiate/accept/reject/cancel/verify) with sessionId follow-up semantics
- [x] **PH46-03A**: discoverability/readiness mutation APIs support explicit lease semantics
- [x] **PH46-04**: read models remain metadata-only; verification secrets remain authenticated realtime-only
- [x] **PH46-05**: Tauri remains a compatibility bridge that forwards daemon pairing/peer updates into existing frontend event contract
- [x] **PH46-05A**: GUI-hosted daemon is discoverable by default while readiness stays flow-scoped
- [x] **PH46-06**: regression tests validate daemon continuity and setup compatibility

### Unified Realtime Subscriptions On Single DaemonWsBridge

- [ ] **PH461-01**: one authenticated daemon websocket exists per Tauri runtime and is owned only by DaemonWsBridge
- [x] **PH461-02**: uc-core exposes RealtimeTopicPort and typed RealtimeEvent/RealtimeFrontendEvent contracts without daemon transport DTO dependencies
- [ ] **PH461-03**: uc-app owns topic consumers for pairing, peers, and setup without consumer-managed websocket lifecycle
- [ ] **PH461-04**: frontend cuts over in one pass to a single daemon://realtime contract with camelCase payloads and stable ordering
- [ ] **PH461-05**: reconnect, resubscribe, and bounded backpressure behavior are centralized and regression-tested
- [ ] **PH461-06**: legacy PairingBridge, setup websocket subscription path, and p2p-\* realtime listeners/emissions are deleted without compatibility flags

### Daemon Pairing Hard Cutover

- [x] **R46.2-1**: all pairing business logic and session lifecycle execute on the daemon, not in `uc-tauri`
- [x] **R46.2-2**: any Tauri-owned pairing hosts or loops are removed or neutralized while GUI flow semantics stay unchanged
- [x] **R46.2-3**: the user-visible pairing flow remains behaviorally equivalent as `request -> verification -> verifying -> complete/failed`
- [x] **R46.2-4**: GUI pairing entrypoints (`PairingDialog`, passive accept/reject, setup flows) route through daemon APIs and realtime topics
- [x] **R46.2-5**: metadata and sensitivity boundaries from Phases 46/46.1 are preserved, with verification data only on authenticated realtime and not in generic read models
- [x] **R46.2-6**: admission, concurrency, and participant-readiness rules remain daemon-owned, including single-session and no headless auto-pairing behavior
- [x] **R46.2-7**: `DaemonWsBridge` remains the single realtime path for pairing topics
- [x] **R46.2-8**: regression coverage proves the daemon-based pairing flow is feature-complete versus the legacy Tauri host across API, websocket, setup, and GUI flows

### GUI Daemon Startup Compatibility And Recovery

- [ ] **GUI-DMN-01**: GUI startup must classify the expected local UniClipboard daemon endpoint as absent, compatible, or incompatible before deciding to continue or replace it
- [ ] **GUI-DMN-02**: when the expected local daemon is incompatible, GUI may terminate that local daemon once and replace it with the current bundled daemon, with bounded retry and deterministic failure
- [ ] **GUI-DMN-03**: GUI must not silently enter the main interface unless daemon bootstrap reaches compatible-ready; failed bootstrap must land in a dedicated startup failure state
- [ ] **GUI-DMN-04**: startup failure must expose both bounded automatic recovery polling and a user-triggered retry path, and automatically resume normal startup once daemon becomes compatible-ready
- [ ] **GUI-DMN-05**: daemon `/health` and `/status` must expose shared `packageVersion` and `apiRevision` metadata that come from the same release contract as the GUI bundle

### GUI-Owned Daemon Exit Lifecycle

- [x] **P46.6-01**: GUI bootstrap records a live daemon owner contract only when the current GUI process actually spawns the daemon; connecting to an already-compatible daemon must not register exit ownership
- [x] **P46.6-02**: on real application exit, GUI must boundedly terminate the GUI-owned daemon exactly once and allow process exit only after cleanup settles
- [x] **P46.6-03**: main-window `CloseRequested` must continue to hide to tray and must never trigger daemon cleanup
- [x] **P46.6-04**: a daemon spawned after incompatible replacement is treated as GUI-owned for later exit cleanup, while independently started daemons remain non-owned
- [x] **P46.6-05**: regression coverage must prove spawned-owned, replacement-owned, compatible-existing, and exit-idempotency paths using exact `cargo test --test ...` commands

### Daemon Encryption State Recovery

- [x] **PH50-01**: DaemonApp::run() calls AutoUnlockEncryptionSession before starting workers so encryption session is available immediately after daemon startup
- [x] **PH50-02**: When EncryptionState is Uninitialized (first run), daemon starts normally without attempting recovery
- [x] **PH50-03**: When EncryptionState is Initialized but recovery fails (keyslot corrupt, KEK missing, unwrap failure), daemon refuses to start with a descriptive error

### Peer Discovery Deduplication Fix

- [ ] **PH51-01**: `get_discovered_peers()` implementation filters out `local_peer_id` so the local device never appears in its own discovered peer list
- [ ] **PH51-02**: daemon `peers.changed` websocket event carries a full peer snapshot list (not a single-peer increment), matching frontend full-replacement semantics
- [ ] **PH51-03**: `GetP2pPeersSnapshot` use case has defense-in-depth `local_peer_id` exclusion independent of the adapter-level filter

### Daemon Space Access SSOT

- [x] **PH52-01**: daemon broadcasts `space_access.state_changed` WS event carrying full `SpaceAccessState` snapshot after every orchestrator dispatch
- [x] **PH52-02**: daemon exposes `GET /space-access/state` HTTP endpoint returning the current `SpaceAccessState`
- [x] **PH52-03**: WS subscribe to `space-access` topic delivers snapshot-first event followed by incremental `state_changed` events
- [x] **PH52-04**: GUI process no longer instantiates `SpaceAccessOrchestrator` — `GuiBootstrapContext` has no `space_access_orchestrator` field
- [x] **PH52-05**: `DaemonWsBridge` translates `space_access.state_changed` into `RealtimeEvent::SpaceAccessStateChanged` for frontend consumption
- [x] **PH52-06**: `wiring.rs` no longer spawns `space_access_completion` background task; space access events flow exclusively through daemon WS

### Daemon Host Architecture Refactor

- [ ] **PH56-01**: peer lifecycle websocket emission is owned by a dedicated `PeerMonitor`, not by `DaemonPairingHost`
- [ ] **PH56-02**: all daemon long-lived components implement one `DaemonService` lifecycle contract instead of `DaemonWorker`
- [ ] **PH56-03**: `DaemonApp` manages one `services: Vec<Arc<dyn DaemonService>>` list and removes pairing-host-specific spawn/shutdown boilerplate
- [ ] **PH56-04**: daemon HTTP routes keep typed access to `DaemonPairingHost` control methods after lifecycle unification and do not change the existing external pairing/setup contract

### Eliminate Hardcoded Strings In Pairing/Setup Flow

- [x] **PH561-01**: `daemon_api_strings.rs` module exists in `uc-core/src/network/` with five `pub mod` submodules (`ws_topic`, `ws_event`, `pairing_stage`, `pairing_busy_reason`, `pairing_error_code`)
- [x] **PH561-02**: value assertion unit tests in `uc-core` verify all constant values match expected wire-protocol strings
- [x] **PH561-03**: `uc-daemon` WS handler (`ws.rs`) uses shared constants from `uc-core` instead of module-level `const` definitions
- [x] **PH561-04**: `uc-daemon` pairing host (`host.rs`) and API routes (`routes.rs`, `server.rs`) use shared constants instead of inline string literals
- [x] **PH561-05**: `uc-daemon-client` WS bridge (`ws_bridge.rs`) uses shared constants for topic subscription and event type dispatch

### Daemon Clipboard Watcher Integration

- [x] **PH57-01**: `ClipboardWatcherWorker` uses real `clipboard_rs::ClipboardWatcherContext` with `spawn_blocking` and `WatcherShutdown`, not a placeholder
- [x] **PH57-02**: daemon constructs `DaemonClipboardChangeHandler` calling `CaptureClipboardUseCase` to persist clipboard entries with `ClipboardChangeOrigin::LocalCapture`
- [x] **PH57-03**: daemon broadcasts `clipboard.new_content` WS event carrying entry_id, preview, and origin after each successful clipboard capture
- [x] **PH57-04**: `DaemonWsBridge` translates `clipboard.new_content` into `RealtimeEvent::ClipboardNewContent` with `RealtimeTopic::Clipboard`
- [x] **PH57-05**: GUI `ClipboardIntegrationMode` is `Passive` so `StartClipboardWatcher` use case is a no-op and daemon is the sole clipboard observer
- [x] **PH57-06**: GUI receives daemon clipboard events via `DaemonWsBridge` and emits `clipboard://event` to frontend so `useClipboardEventStream` continues working unchanged
- [x] **PH57-07**: `DaemonClipboardChangeHandler` integrates `ClipboardChangeOriginPort` for write-back loop prevention, checking origin before capture and sharing the port instance with future inbound sync

### Extract DTO Models And Pairing Event Types

- [x] **PH58-01**: `EntryProjectionDto` in uc-app has `#[derive(Serialize, Deserialize)]` with `#[serde(skip)]` on `file_transfer_ids` and `#[serde(skip_serializing_if)]` on optional fields, matching the existing frontend wire contract
- [x] **PH58-02**: `ClipboardStats` in uc-app has `#[derive(Serialize, Deserialize)]` and the duplicate `ClipboardStats` definition in uc-tauri/models is deleted
- [x] **PH58-03**: `P2PPeerInfo` and `PairedPeer` structs live in `uc-app/src/usecases/pairing/dto.rs` with serde derives, exported via `uc-app::usecases::pairing`
- [x] **PH58-04**: `P2PPairingVerificationEvent` and `P2PPairingVerificationKind` in `uc-tauri/src/events/p2p_pairing.rs` are deleted (stale dead code with zero consumers)
- [x] **PH58-05**: all import paths updated directly (no re-export stubs in uc-tauri per D-05)

### Extract File Transfer Wiring Orchestration

- [x] **PH60-01**: `FileTransferOrchestrator` struct exists in `uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs` holding `Arc<TrackInboundTransfersUseCase>` + `Arc<dyn HostEventEmitterPort>` + `Arc<dyn ClockPort>` + `EarlyCompletionCache`
- [x] **PH60-02**: all 9 standalone functions from `file_transfer_wiring.rs` are methods on `FileTransferOrchestrator` with `now_ms` computed internally via `self.clock`
- [x] **PH60-03**: `uc-bootstrap/assembly.rs` provides `build_file_transfer_orchestrator()` builder function following the `build_setup_orchestrator` pattern
- [x] **PH60-04**: `wiring.rs` calls orchestrator methods at all integration points (clipboard receive loop, network event loop, startup reconciliation, timeout sweep) and does not create any standalone `TrackInboundTransfersUseCase` instances
- [x] **PH60-05**: `file_transfer_wiring.rs` is deleted from `uc-tauri/src/bootstrap/` with no re-export stubs, and all import paths updated directly

### Daemon Outbound Clipboard Sync

- [x] **PH61-01**: `DaemonClipboardChangeHandler::on_clipboard_changed` calls `OutboundSyncPlanner::plan()` after successful `LocalCapture` capture and dispatches `SyncOutboundClipboardUseCase::execute()` via `tokio::task::spawn_blocking`
- [x] **PH61-02**: `RemotePush` origin clipboard changes skip outbound sync entirely (no double-sync loop), guarded by `OutboundSyncPlanner` policy
- [x] **PH61-03**: `extract_file_paths_from_snapshot` function exists in `clipboard_watcher.rs` parsing `text/uri-list`, `file/uri-list`, `files`, `public.file-url` representations into `Vec<PathBuf>` with deduplication
- [x] **PH61-04**: File clipboard items produce `FileCandidate` vec with `extracted_paths_count` set before metadata filtering, and `SyncOutboundFileUseCase` dispatches for each file intent from the planner

### Daemon Inbound Clipboard Sync

- [x] **PH62-01**: `InboundClipboardSyncWorker` implements `DaemonService`, subscribes to `ClipboardTransportPort::subscribe_clipboard()`, and calls `SyncInboundClipboardUseCase::execute_with_outcome()` for each received message
- [x] **PH62-02**: Applied outcome with `entry_id: Some(id)` emits `clipboard.new_content` WS event with `origin="remote"` via `broadcast::Sender<DaemonWsEvent>`
- [x] **PH62-03**: Applied outcome with `entry_id: None` (Full mode non-file content) does NOT emit WS event — `ClipboardWatcherWorker` fires the event after OS clipboard write triggers capture
- [x] **PH62-04**: Skipped outcomes (echo prevention, dedup, encryption not ready) do not emit WS events
- [x] **PH62-05**: `InboundClipboardSyncWorker` accepts `clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>` via constructor and passes it to `SyncInboundClipboardUseCase::with_capture_dependencies()`, sharing the same Arc instance as `DaemonClipboardChangeHandler`

### Daemon File Transfer Orchestration

- [x] **PH63-01**: `DaemonApiEventEmitter` handles `HostEvent::Transfer(TransferHostEvent::StatusChanged)` by emitting a `DaemonWsEvent` on the `file-transfer` WS topic with event type `file-transfer.status_changed` instead of silently logging and dropping
- [x] **PH63-02**: `daemon_api_strings` module in uc-core has `ws_topic::FILE_TRANSFER = "file-transfer"` and `ws_event::FILE_TRANSFER_STATUS_CHANGED = "file-transfer.status_changed"` constants
- [x] **PH63-03**: `InboundClipboardSyncWorker` accepts `Option<Arc<FileTransferOrchestrator>>` via constructor and calls `tracker().record_pending_from_clipboard()` for `Applied { entry_id: Some(_), pending_transfers: non-empty }` outcomes, including early completion cache reconciliation
- [x] **PH63-04**: `FileSyncOrchestratorWorker` implements `DaemonService`, subscribes to `NetworkEventPort::subscribe_events()`, and handles `TransferProgress` (pending->transferring promotion), `FileTransferCompleted` (hash verification + clipboard restore), and `FileTransferFailed` (durable failure marking)
- [x] **PH63-05**: `FileSyncOrchestratorWorker::start()` calls `FileTransferOrchestrator::reconcile_on_startup()` before entering the network event loop, marking orphaned in-flight transfers as failed
- [x] **PH63-06**: `FileSyncOrchestratorWorker::start()` spawns `FileTransferOrchestrator::spawn_timeout_sweep()` with a `watch::channel` cancel signal that is sent `true` when the `CancellationToken` fires
- [x] **PH63-07**: daemon `main.rs` registers `FileSyncOrchestratorWorker` in the services vec with initial health status, and `cargo test -p uc-daemon` passes with all workers

### Tauri Sync Retirement

- [x] **PH64-01**: `wiring.rs` no longer spawns `clipboard_receive` or `pairing_events` background tasks — daemon `InboundClipboardSyncWorker` and `PeerDiscoveryWorker`/`PeerMonitor` own these responsibilities
- [x] **PH64-02**: `wiring.rs` no longer spawns `file_transfer_reconcile` or `file_transfer_timeout_sweep` tasks — daemon `FileSyncOrchestratorWorker` owns startup reconciliation and timeout sweeps
- [x] **PH64-03**: All dead helper functions (`register_pairing_background_tasks`, `run_clipboard_receive_loop`, `run_network_realtime_loop`, `new_sync_inbound_clipboard_usecase`, `restore_file_to_clipboard_after_transfer`, `resolve_device_name_for_peer`), backoff constants, and backoff utility functions are removed from `wiring.rs`
- [x] **PH64-04**: `restore_clipboard_entry` command gates outbound sync on `ClipboardIntegrationMode::Full` — in Passive mode (daemon running), daemon's `ClipboardWatcherWorker` handles outbound sync after OS clipboard write, preventing double-sync
- [x] **PH64-05**: `blake3` dependency removed from `uc-tauri/Cargo.toml` (only consumer was hash verification in deleted `run_network_realtime_loop`)
- [x] **PH64-06**: `sync_inbound_clipboard()` accessor removed from `AppUseCases` in `runtime.rs` (zero callers after clipboard_receive loop deletion)

### Daemon Dashboard Clipboard Refresh Fix

- [x] **PH66-01**: `is_supported_topic()` in `uc-daemon/src/api/ws.rs` includes `ws_topic::CLIPBOARD` so daemon WS server accepts clipboard topic subscriptions
- [x] **PH66-02**: `is_supported_topic()` includes `ws_topic::FILE_TRANSFER` so daemon WS server accepts file-transfer topic subscriptions
- [x] **PH66-03**: `build_snapshot_event()` returns `Ok(None)` for clipboard and file-transfer topics without bailing
- [x] **PH66-04**: `DaemonWsBridge` state monitor task detects `Degraded -> Ready` transitions and emits `HostEvent::Clipboard(ClipboardHostEvent::DaemonReconnected)` (not on initial startup)
- [x] **PH66-05**: Frontend `useClipboardEventStream` listens for `daemon://ws-reconnected` Tauri event and triggers `onRemoteInvalidate` to refetch clipboard list

### Setup Filter — Prevent Pre-Setup Device Discovery

- [x] **PH67-01**: `recover_encryption_session()` returns `anyhow::Result<bool>` distinguishing Initialized+unlocked (true) from Uninitialized (false)
- [x] **PH67-02**: `DaemonApp` accepts optional deferred `DaemonService` and `oneshot::Receiver<()>` for post-setup PeerDiscoveryWorker start
- [x] **PH67-03**: `SetupCompletionEmitter` implements `SessionReadyEmitter` by firing a oneshot channel sender on `emit_ready()`
- [x] **PH67-04**: `build_non_gui_runtime_with_emitter()` exists in uc-bootstrap accepting a custom `SessionReadyEmitter` for daemon injection
- [x] **PH67-05**: daemon `main.rs` conditionally excludes `PeerDiscoveryWorker` from initial services when encryption is `Uninitialized`
- [x] **PH67-06**: daemon `main.rs` wires `SetupCompletionEmitter` as the `SessionReadyEmitter` into `CoreRuntime` via `build_non_gui_runtime_with_emitter`
- [x] **PH67-07**: peer-discovery initial health status is `ServiceHealth::Stopped` when encryption is uninitialized, `Healthy` when initialized

### Tauri Sidecar For Daemon Binary Management

- [x] **PH68-01**: `tauri.conf.json` declares `"binaries/uniclipboard-daemon"` in `bundle.externalBin` array
- [x] **PH68-02**: `src-tauri/build.rs` contains `copy_daemon_binary_to_binaries()` that copies compiled daemon to `src-tauri/binaries/uniclipboard-daemon-{target-triple}` using `TAURI_ENV_TARGET_TRIPLE` or `CARGO_CFG_*` fallback
- [x] **PH68-03**: `spawn_daemon_process()` in `run.rs` uses `app.shell().sidecar("uniclipboard-daemon").args(["--gui-managed"]).spawn()` instead of `std::process::Command`
- [x] **PH68-04**: `bootstrap_daemon_connection()` and `supervise_daemon()` accept `AppHandle<R>` parameter for sidecar API access
- [x] **PH68-05**: `src-tauri/capabilities/default.json` contains `shell:allow-spawn` permission with `sidecar: true` and `args: ["--gui-managed"]`
- [x] **PH68-06**: `GuiOwnedDaemonState` holds `tauri_plugin_shell::process::CommandChild` instead of `std::process::Child`, and `shutdown_owned_daemon` uses PID-based termination

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
| DAEM-01     | 41    | Complete |
| DAEM-02     | 41    | Complete |
| DAEM-03     | 41    | Complete |
| DAEM-04     | 41    | Complete |
| CLI-01      | 41    | Complete |
| CLI-02      | 41    | Complete |
| CLI-03      | 41    | Complete |
| CLI-04      | 41    | Complete |
| CLI-05      | 41    | Complete |
| PH45-01     | 45    | Complete |
| PH45-02     | 45    | Complete |
| PH45-05     | 45    | Complete |
| PH45-06     | 45    | Complete |
| PH46-01     | 46    | Complete |
| PH46-01A    | 46    | Complete |
| PH46-01B    | 46    | Complete |
| PH46-02     | 46    | Complete |
| PH46-03     | 46    | Complete |
| PH46-03A    | 46    | Complete |
| PH46-04     | 46    | Complete |
| PH46-05     | 46    | Complete |
| PH46-05A    | 46    | Complete |
| PH46-06     | 46    | Complete |
| PH461-01    | 46.1  | Pending  |
| PH461-02    | 46.1  | Complete |
| PH461-03    | 46.1  | Pending  |
| PH461-04    | 46.1  | Pending  |
| PH461-05    | 46.1  | Pending  |
| PH461-06    | 46.1  | Pending  |
| R46.2-1     | 46.2  | Complete |
| R46.2-2     | 46.2  | Complete |
| R46.2-3     | 46.2  | Complete |
| R46.2-4     | 46.2  | Complete |
| R46.2-5     | 46.2  | Complete |
| R46.2-6     | 46.2  | Complete |
| R46.2-7     | 46.2  | Complete |
| R46.2-8     | 46.2  | Complete |
| GUI-DMN-01  | 46.3  | Pending  |
| GUI-DMN-02  | 46.3  | Pending  |
| GUI-DMN-03  | 46.3  | Pending  |
| GUI-DMN-04  | 46.3  | Pending  |
| GUI-DMN-05  | 46.3  | Pending  |
| P46.6-01    | 46.6  | Complete |
| P46.6-02    | 46.6  | Complete |
| P46.6-03    | 46.6  | Complete |
| P46.6-04    | 46.6  | Complete |
| P46.6-05    | 46.6  | Complete |
| PH50-01     | 50    | Complete |
| PH50-02     | 50    | Complete |
| PH50-03     | 50    | Complete |
| PH51-01     | 51    | Pending  |
| PH51-02     | 51    | Pending  |
| PH51-03     | 51    | Pending  |
| PH52-01     | 52    | Complete |
| PH52-02     | 52    | Complete |
| PH52-03     | 52    | Complete |
| PH52-04     | 52    | Complete |
| PH52-05     | 52    | Complete |
| PH52-06     | 52    | Complete |
| PH56-01     | 56    | Pending  |
| PH56-02     | 56    | Pending  |
| PH56-03     | 56    | Pending  |
| PH56-04     | 56    | Pending  |
| PH561-01    | 56.1  | Complete |
| PH561-02    | 56.1  | Complete |
| PH561-03    | 56.1  | Complete |
| PH561-04    | 56.1  | Complete |
| PH561-05    | 56.1  | Complete |
| PH57-01     | 57    | Complete |
| PH57-02     | 57    | Complete |
| PH57-03     | 57    | Complete |
| PH57-04     | 57    | Complete |
| PH57-05     | 57    | Complete |
| PH57-06     | 57    | Complete |
| PH57-07     | 57    | Complete |
| PH58-01     | 58    | Complete |
| PH58-02     | 58    | Complete |
| PH58-03     | 58    | Complete |
| PH58-04     | 58    | Complete |
| PH58-05     | 58    | Complete |
| PH60-01     | 60    | Complete |
| PH60-02     | 60    | Complete |
| PH60-03     | 60    | Complete |
| PH60-04     | 60    | Complete |
| PH60-05     | 60    | Complete |
| PH61-01     | 61    | Complete |
| PH61-02     | 61    | Complete |
| PH61-03     | 61    | Complete |
| PH61-04     | 61    | Complete |
| PH62-01     | 62    | Complete |
| PH62-02     | 62    | Complete |
| PH62-03     | 62    | Complete |
| PH62-04     | 62    | Complete |
| PH62-05     | 62    | Complete |
| PH63-01     | 63    | Complete |
| PH63-02     | 63    | Complete |
| PH63-03     | 63    | Complete |
| PH63-04     | 63    | Complete |
| PH63-05     | 63    | Complete |
| PH63-06     | 63    | Complete |
| PH63-07     | 63    | Complete |
| PH64-01     | 64    | Complete |
| PH64-02     | 64    | Complete |
| PH64-03     | 64    | Complete |
| PH64-04     | 64    | Complete |
| PH64-05     | 64    | Complete |
| PH64-06     | 64    | Complete |
| PH66-01     | 66    | Complete |
| PH66-02     | 66    | Complete |
| PH66-03     | 66    | Complete |
| PH66-04     | 66    | Complete |
| PH66-05     | 66    | Complete |
| PH67-01     | 67    | Complete |
| PH67-02     | 67    | Complete |
| PH67-03     | 67    | Complete |
| PH67-04     | 67    | Complete |
| PH67-05     | 67    | Complete |
| PH67-06     | 67    | Complete |
| PH67-07     | 67    | Complete |

| PH68-01 | 68 | Complete |
| PH68-02 | 68 | Complete |
| PH68-03 | 68 | Complete |
| PH68-04 | 68 | Complete |
| PH68-05 | 68 | Complete |
| PH68-06 | 68 | Complete |

**Coverage:**

- v0.4.0 requirements: 119 total
- Mapped to phases: 119
- Unmapped: 0

---

_Requirements defined: 2026-03-17_
_Last updated: 2026-03-28 after Phase 68 planning_
