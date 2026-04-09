# Phase 56: Refactor daemon host architecture - Context

**Gathered:** 2026-03-23
**Status:** Ready for planning

<domain>
## Phase Boundary

Refactor `DaemonApp`'s component lifecycle to fix three problems:

1. **Mixed responsibilities**: `run_pairing_network_event_loop` handles both peer lifecycle events (PeerDiscovered/PeerLost/PeerConnected/PeerDisconnected/PeerNameUpdated) and pairing protocol messages (PairingMessageReceived/PairingFailed). These are unrelated concerns sharing one function.

2. **Misleading names**: All event loop functions use `run_pairing_*` prefix even when they have nothing to do with pairing.

3. **No unified lifecycle**: `PairingHost` is hardcoded in `DaemonApp::run()` with manual spawn/shutdown boilerplate, while `DaemonWorker` trait exists for generic components. Adding new hosts requires modifying `DaemonApp` struct and `run()` method.

**In scope:**

- Extract peer lifecycle events into a new `PeerMonitor` struct
- Unify `DaemonWorker` and `PairingHost` under a single `DaemonService` trait
- Rename event loop functions to match their actual responsibilities
- Refactor `DaemonApp` to manage all services uniformly via `Vec<Arc<dyn DaemonService>>`

**Out of scope:**

- New functionality (clipboard sync host, file sync host, etc.)
- Changing the NetworkEvent subscription mechanism (broadcast channel)
- Frontend-facing API changes
- Modifying pairing business logic

</domain>

<decisions>
## Implementation Decisions

### Responsibility split strategy

- **D-01:** Extract peer lifecycle event handling into a new `PeerMonitor` struct, separate from `PairingHost`. `PeerMonitor` handles: `PeerDiscovered`, `PeerLost`, `PeerConnected`, `PeerDisconnected`, `PeerNameUpdated` → emits WebSocket events. `PairingHost` keeps only: `PairingMessageReceived`, `PairingFailed` → delegates to orchestrator/transport.
- **D-02:** `PeerMonitor` and `PairingHost` each independently call `network_events.subscribe_events()` and filter for their own event types. No central router or dispatcher needed — the existing broadcast channel supports multiple subscribers.
- **D-03:** `PeerMonitor` lives as a peer-level struct alongside `PairingHost` (e.g., new module `peers/monitor.rs` or `peers/peer_monitor.rs`), not nested inside PairingHost.

### Unified service lifecycle

- **D-04:** Merge `DaemonWorker` trait into a new `DaemonService` trait. The interface stays the same: `name()`, `start(CancellationToken)`, `stop()`, `health_check()`. All daemon components — `PairingHost`, `PeerMonitor`, `ClipboardWatcherWorker`, `PeerDiscoveryWorker` — implement `DaemonService`.
- **D-05:** `DaemonApp` holds a single `services: Vec<Arc<dyn DaemonService>>` instead of separate `workers` vec + hardcoded `PairingHost`. The `run()` method spawns all services uniformly and handles shutdown uniformly — no more per-component `completed_xxx_handle` boilerplate.
- **D-06:** Remove `DaemonWorker` trait entirely after migration. Rename the file from `worker.rs` to `service.rs`.
- **D-07:** `DaemonApp` struct fields for pairing-specific state (`pairing_orchestrator`, `pairing_action_rx`, `space_access_orchestrator`, `key_slot_store`) move into `PairingHost` construction before `DaemonApp::new()`. `DaemonApp` only receives `Vec<Arc<dyn DaemonService>>` + `CoreRuntime` + socket path.

### Naming conventions

- **D-08:** Event loop functions are named by responsibility, prefixed by their owning struct's domain:
  - PeerMonitor: `run_peer_event_loop` (or similar, matching peer lifecycle)
  - PairingHost: `run_pairing_protocol_loop` (was `run_pairing_network_event_loop`), `run_pairing_action_loop` (unchanged), `run_pairing_domain_event_loop` (unchanged), `run_pairing_session_sweep_loop` (unchanged)
- **D-09:** The current `PeerDiscoveryWorker` (in `workers/peer_discovery.rs`) may overlap with `PeerMonitor` since both consume NetworkEvent for peer-related concerns. Evaluate whether to merge `PeerDiscoveryWorker`'s announce logic into `PeerMonitor` or keep them separate.

### Claude's Discretion

- Exact module/file organization for `PeerMonitor` (e.g., `peers/` module vs. flat file)
- Whether `PeerDiscoveryWorker`'s device name announcement logic merges into `PeerMonitor` or stays separate
- Internal implementation details of the `DaemonService` trait (e.g., whether `run()` is `self: Arc<Self>` or `&self`)
- Order of service startup/shutdown in `DaemonApp`

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Daemon architecture

- `src-tauri/crates/uc-daemon/src/app.rs` — DaemonApp struct and run() method, the main refactoring target
- `src-tauri/crates/uc-daemon/src/worker.rs` — Current DaemonWorker trait definition, to be replaced by DaemonService
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — DaemonPairingHost and all run*pairing*\* event loops

### Existing workers

- `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` — PeerDiscoveryWorker, potential merge candidate with PeerMonitor
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — ClipboardWatcherWorker placeholder

### State and types

- `src-tauri/crates/uc-daemon/src/state.rs` — RuntimeState with DaemonWorkerSnapshot, needs renaming to DaemonServiceSnapshot

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `DaemonWorker` trait (`worker.rs`): Interface is correct (start/stop/health_check), just needs renaming to `DaemonService`
- `broadcast::Sender<DaemonWsEvent>` (`event_tx`): Already used for WebSocket event emission, PeerMonitor will use the same pattern
- `emit_ws_event()` helper in `pairing/host.rs`: Already handles peer event emission, can be extracted or shared

### Established Patterns

- CancellationToken-based cooperative shutdown across all loops
- `tokio::select!` for cancellation-aware event processing
- `JoinSet` for managing concurrent tasks within a service
- Subscribe-with-backoff retry loop in `run_pairing_network_event_loop`

### Integration Points

- `DaemonApp::run()` — primary refactoring site for unified service management
- `DaemonApp::new()` constructor — simplify parameter list after extracting pairing-specific state
- `src-tauri/crates/uc-daemon/src/main.rs` — daemon construction site, where services are assembled
- `DaemonApiState` — currently receives `pairing_host` directly, may need adjustment
- `RuntimeState` / `DaemonWorkerSnapshot` — rename to match DaemonService terminology

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 56-refactor-daemon-host-architecture_
_Context gathered: 2026-03-23_
