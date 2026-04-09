# Phase 56: Refactor daemon host architecture - Research

**Researched:** 2026-03-24
**Domain:** Rust daemon lifecycle orchestration, pairing host decomposition, peer event projection
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Responsibility split strategy

- **D-01:** Extract peer lifecycle event handling into a new `PeerMonitor` struct, separate from `PairingHost`. `PeerMonitor` handles: `PeerDiscovered`, `PeerLost`, `PeerConnected`, `PeerDisconnected`, `PeerNameUpdated` → emits WebSocket events. `PairingHost` keeps only: `PairingMessageReceived`, `PairingFailed` → delegates to orchestrator/transport.
- **D-02:** `PeerMonitor` and `PairingHost` each independently call `network_events.subscribe_events()` and filter for their own event types. No central router or dispatcher needed — the existing broadcast channel supports multiple subscribers.
- **D-03:** `PeerMonitor` lives as a peer-level struct alongside `PairingHost` (e.g., new module `peers/monitor.rs` or `peers/peer_monitor.rs`), not nested inside PairingHost.

#### Unified service lifecycle

- **D-04:** Merge `DaemonWorker` trait into a new `DaemonService` trait. The interface stays the same: `name()`, `start(CancellationToken)`, `stop()`, `health_check()`. All daemon components — `PairingHost`, `PeerMonitor`, `ClipboardWatcherWorker`, `PeerDiscoveryWorker` — implement `DaemonService`.
- **D-05:** `DaemonApp` holds a single `services: Vec<Arc<dyn DaemonService>>` instead of separate `workers` vec + hardcoded `PairingHost`. The `run()` method spawns all services uniformly and handles shutdown uniformly — no more per-component `completed_xxx_handle` boilerplate.
- **D-06:** Remove `DaemonWorker` trait entirely after migration. Rename the file from `worker.rs` to `service.rs`.
- **D-07:** `DaemonApp` struct fields for pairing-specific state (`pairing_orchestrator`, `pairing_action_rx`, `space_access_orchestrator`, `key_slot_store`) move into `PairingHost` construction before `DaemonApp::new()`. `DaemonApp` only receives `Vec<Arc<dyn DaemonService>>` + `CoreRuntime` + socket path.

#### Naming conventions

- **D-08:** Event loop functions are named by responsibility, prefixed by their owning struct's domain:
  - PeerMonitor: `run_peer_event_loop` (or similar, matching peer lifecycle)
  - PairingHost: `run_pairing_protocol_loop` (was `run_pairing_network_event_loop`), `run_pairing_action_loop` (unchanged), `run_pairing_domain_event_loop` (unchanged), `run_pairing_session_sweep_loop` (unchanged)
- **D-09:** The current `PeerDiscoveryWorker` (in `workers/peer_discovery.rs`) may overlap with `PeerMonitor` since both consume NetworkEvent for peer-related concerns. Evaluate whether to merge `PeerDiscoveryWorker`'s announce logic into `PeerMonitor` or keep them separate.

### Claude's Discretion

- Exact module/file organization for `PeerMonitor` (e.g., `peers/` module vs. flat file)
- Whether `PeerDiscoveryWorker`'s device name announcement logic merges into `PeerMonitor` or stays separate
- Internal implementation details of the `DaemonService` trait (e.g., whether `run()` is `self: Arc<Self>` or `&self`)
- Order of service startup/shutdown in `DaemonApp`

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                                                                                            | Research Support                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| PH56-01 | peer lifecycle websocket emission is owned by a dedicated `PeerMonitor`, not by `DaemonPairingHost`                                                                    | Confirmed: `run_pairing_network_event_loop` in `host.rs` currently handles PeerDiscovered/PeerLost/PeerConnected/PeerDisconnected/PeerNameUpdated in the same function as PairingMessageReceived/PairingFailed. The peer arms (lines 993-1075) can be cleanly extracted into `PeerMonitor` because they only need `runtime`, `event_tx`, and network subscription — no pairing orchestrator state.                                                                                                                                      |
| PH56-02 | all daemon long-lived components implement one `DaemonService` lifecycle contract instead of `DaemonWorker`                                                            | Confirmed: `DaemonWorker` trait in `worker.rs` has the correct interface — `name()`, `start(CancellationToken)`, `stop()`, `health_check()`. Rename to `DaemonService` + `ServiceHealth` propagates to `state.rs` (`DaemonWorkerSnapshot`), `app.rs`, `main.rs`, and the two workers crates. `DaemonPairingHost` currently has a `run(self: Arc<Self>, cancel: CancellationToken)` and will need `DaemonService` adaption.                                                                                                              |
| PH56-03 | `DaemonApp` manages one `services: Vec<Arc<dyn DaemonService>>` list and removes pairing-host-specific spawn/shutdown boilerplate                                      | Confirmed: `app.rs` currently has `workers: Vec<Arc<dyn DaemonWorker>>` + hardcoded `pairing_host` field + three separate `completed_xxx_handle` booleans for RPC/HTTP/pairing. After this change, all five components (RPC, HTTP, ClipboardWatcher, PeerDiscovery, PairingHost, PeerMonitor) become uniform services. Note: RPC/HTTP servers are infrastructure, not services in the `DaemonService` sense — they stay as separate spawned tasks or also migrate; planner must decide scope boundary.                                  |
| PH56-04 | daemon HTTP routes keep typed access to `DaemonPairingHost` control methods after lifecycle unification and do not change the existing external pairing/setup contract | Confirmed: `api/routes.rs` calls 10+ typed methods on `Arc<DaemonPairingHost>` (initiate_pairing, accept_pairing, reject_pairing, cancel_pairing, verify_pairing, set_discoverability, set_participant_ready, register_gui_participant, reset_setup_state, active_session_id). `DaemonApiState` already has `pairing_host: Option<Arc<DaemonPairingHost>>`. The solution is to build typed `Arc<DaemonPairingHost>` first in `main.rs`, clone it into `DaemonApiState`, and also wrap as `Arc<dyn DaemonService>` for the services vec. |

</phase_requirements>

## Summary

This phase is a structural refactor inside `uc-daemon`, not a behavior rewrite. The current hotspot is clear from code inspection: `DaemonPairingHost` owns pairing lifecycle, peer websocket projection, and its own service-style supervision, while `DaemonApp` supervises placeholder workers separately and hardcodes pairing-host startup/shutdown in parallel. That leaves the daemon with two lifecycle systems and one misleading event loop that mixes unrelated concerns.

Direct code analysis reveals the exact split point in `run_pairing_network_event_loop` (lines 992–1104 of `host.rs`): the five peer lifecycle event arms (`PeerDiscovered`, `PeerLost`, `PeerConnected`, `PeerDisconnected`, `PeerNameUpdated`) need only `runtime` and `event_tx` — no pairing orchestrator state. The two pairing arms (`PairingMessageReceived`, `PairingFailed`) need the full pairing stack. This is a clean mechanical extraction.

The main planning risk is typed access. `DaemonApp` can supervise `Vec<Arc<dyn DaemonService>>`, but API routes and setup-state queries still need a typed pairing-host handle — `api/routes.rs` has 10+ typed calls to `DaemonPairingHost` methods that must not go through trait objects. The composition root must build typed services first, clone the typed `Arc<DaemonPairingHost>` into `DaemonApiState`, and only then erase services into trait objects for generic supervision.

**Primary recommendation:** Build `DaemonPairingHost` and `PeerMonitor` as typed services in the composition root (`main.rs`), pass clones into both `DaemonApiState` and `Vec<Arc<dyn DaemonService>>`, and make `DaemonApp` supervise only generic services.

## Standard Stack

### Core

| Library      | Version         | Purpose                                                | Why Standard                                                                                 |
| ------------ | --------------- | ------------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| `uc-daemon`  | `0.4.0-alpha.1` | Daemon lifecycle, HTTP/WS/RPC host crate               | This phase refactors existing daemon structure, not crate selection                          |
| `uc-app`     | `0.1.0`         | Pairing/setup/space-access orchestrators and use cases | Existing business logic already lives here and should remain unchanged                       |
| `tokio`      | `1.x`           | Async runtime, `JoinSet`, channels, task spawning      | Already used across daemon lifecycle and officially supports the needed supervision patterns |
| `tokio-util` | `0.7`           | `CancellationToken` for coordinated shutdown           | Already established in daemon and officially supports parent/child cancellation propagation  |
| `axum`       | `0.7`           | Daemon HTTP + websocket server                         | Existing daemon API already depends on it and this phase should preserve behavior            |

### Supporting

| Library                  | Version                | Purpose                                                     | When to Use                                                                         |
| ------------------------ | ---------------------- | ----------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `async-trait`            | `0.1`                  | Async trait methods on `DaemonService`                      | Keep for `start()` / `stop()` on trait objects                                      |
| `tracing`                | `0.1`                  | Structured lifecycle logs for startup/shutdown/failures     | Use at service boundaries and unexpected exits                                      |
| `tokio::sync::broadcast` | bundled in `tokio 1.x` | Independent `NetworkEvent` subscribers and websocket fanout | Use for `PeerMonitor` + `PairingHost` concurrent subscriptions and daemon WS topics |

### Alternatives Considered

| Instead of                                              | Could Use                        | Tradeoff                                                                                                  |
| ------------------------------------------------------- | -------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Independent `subscribe_events()` per service            | Central event router/dispatcher  | More plumbing, more shared state, and unnecessary because broadcast already supports multiple subscribers |
| `Vec<Arc<dyn DaemonService>>` supervision               | Hardcoded fields per component   | Easier typed access, but every new daemon service reintroduces app boilerplate                            |
| Separate `PeerMonitor` + existing `PeerDiscoveryWorker` | Merge both into one peer service | Fewer files, but mixes websocket projection with network-start/announce side effects in the same phase    |

**Installation:**

```bash
# No new dependencies recommended for this phase.
```

**Version verification:** Verified from `src-tauri/crates/uc-daemon/Cargo.toml`, `src-tauri/Cargo.toml`, and direct code analysis on 2026-03-24. No new libraries required.

## Architecture Patterns

### Recommended Project Structure

```text
src-tauri/crates/uc-daemon/src/
├── app.rs                 # Generic daemon supervisor only — no pairing-specific fields
├── service.rs             # DaemonService trait + ServiceHealth (replaces worker.rs)
├── state.rs               # RuntimeState + DaemonServiceSnapshot (renamed from DaemonWorkerSnapshot)
├── pairing/
│   └── host.rs            # Pairing-only lifecycle and protocol handling; remove peer event arms
├── peers/
│   └── monitor.rs         # NEW: Peer lifecycle -> websocket projection (PeerMonitor)
└── workers/
    ├── clipboard_watcher.rs  # Renamed to implement DaemonService
    ├── mod.rs
    └── peer_discovery.rs     # Renamed to implement DaemonService
```

Note: `worker.rs` is deleted entirely. `lib.rs` pub exports updated accordingly.

### Pattern 1: Compose Typed Services Before Trait-Object Erasure

**What:** Construct `Arc<DaemonPairingHost>` and `Arc<PeerMonitor>` as typed values first, then clone them into `DaemonApiState` and into `Vec<Arc<dyn DaemonService>>`.
**When to use:** Always for services that need typed external access. This satisfies D-05/D-07 while preserving typed pairing APIs for PH56-04.
**Example:**

```rust
// Source: src-tauri/crates/uc-daemon/src/main.rs (recommended adaptation)
let pairing_host = Arc::new(DaemonPairingHost::new(
    runtime.clone(),
    ctx.pairing_orchestrator,
    ctx.pairing_action_rx,
    state.clone(),
    ctx.space_access_orchestrator,
    ctx.key_slot_store,
    api_state.event_tx.clone(),
));
let peer_monitor = Arc::new(PeerMonitor::new(
    runtime.clone(),
    api_state.event_tx.clone(),
));

let api_state = api_state.with_pairing_host(Arc::clone(&pairing_host));

let services: Vec<Arc<dyn DaemonService>> = vec![
    Arc::clone(&pairing_host) as Arc<dyn DaemonService>,
    Arc::clone(&peer_monitor) as Arc<dyn DaemonService>,
    Arc::new(ClipboardWatcherWorker),
    Arc::new(PeerDiscoveryWorker::new(...)),
];

let daemon = DaemonApp::new(services, runtime, socket_path);
```

### Pattern 2: One Event Subscription Per Responsibility

**What:** `PeerMonitor` and `PairingHost` each call `network_events.subscribe_events()` and filter for their own event variants. The existing `subscribe_events()` returns a new `mpsc::Receiver` per call, so both services receive all events independently.
**When to use:** For any long-lived daemon service that reacts to a subset of `NetworkEvent`.
**Example:**

```rust
// Source: existing worker.rs pattern + tokio broadcast docs
// PeerMonitor.start():
let mut event_rx = self.network_events.subscribe_events().await?;
loop {
    tokio::select! {
        _ = cancel.cancelled() => return Ok(()),
        maybe_event = event_rx.recv() => {
            let Some(event) = maybe_event else { break; };
            match event {
                NetworkEvent::PeerDiscovered(_) | NetworkEvent::PeerLost(_) => { ... }
                NetworkEvent::PeerConnected(_) | NetworkEvent::PeerDisconnected(_) => { ... }
                NetworkEvent::PeerNameUpdated { .. } => { ... }
                _ => {}  // Ignore pairing events
            }
        }
    }
}
```

### Pattern 3: DaemonService Trait (Rename of DaemonWorker)

**What:** Rename `DaemonWorker` to `DaemonService` and `WorkerHealth` to `ServiceHealth`. Interface is unchanged.
**When to use:** All daemon long-lived components.
**Example:**

```rust
// Source: src-tauri/crates/uc-daemon/src/worker.rs (rename only, no interface change)
#[async_trait]
pub trait DaemonService: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    fn health_check(&self) -> ServiceHealth;
}
```

### Pattern 4: DaemonPairingHost DaemonService Adapter

**What:** `DaemonPairingHost` currently has `run(self: Arc<Self>, cancel: CancellationToken)` but not `DaemonService`. Add an impl that delegates `start()` to `run()`.
**When to use:** When adapting a service with Arc-receiver `run()` to the uniform trait.
**Example:**

```rust
// Source: src-tauri/crates/uc-daemon/src/pairing/host.rs (new impl)
#[async_trait]
impl DaemonService for DaemonPairingHost {
    fn name(&self) -> &str { "pairing-host" }

    async fn start(self: Arc<Self>, cancel: CancellationToken) -> anyhow::Result<()> {
        Arc::clone(&self).run(cancel).await
    }

    async fn stop(&self) -> anyhow::Result<()> { Ok(()) }

    fn health_check(&self) -> ServiceHealth { ServiceHealth::Healthy }
}
```

Note: The `DaemonService::start` signature decision — whether it takes `self: Arc<Self>` or `&self` — is discretionary. The existing `DaemonPairingHost::run` uses `Arc<Self>`, so if `start` takes `Arc<Self>`, no wrapper is needed. If `start` takes `&self`, a clone is needed inside.

### Pattern 5: Uniform DaemonApp Supervision Without Per-Component Booleans

**What:** Replace five-way `tokio::select!` with `completed_rpc_handle` / `completed_http_handle` / `completed_pairing_handle` booleans. Use one `JoinSet` for all services.
**When to use:** `DaemonApp::run()` post-refactor.
**Example:**

```rust
// Source: src-tauri/crates/uc-daemon/src/app.rs (recommended adaptation)
let mut tasks = JoinSet::new();
for service in &self.services {
    let svc = Arc::clone(service);
    let token = self.cancel.child_token();
    tasks.spawn(async move { svc.start(token).await });
}

tokio::select! {
    _ = wait_for_shutdown_signal() => { info!("shutdown signal received"); }
    Some(result) = tasks.join_next() => {
        warn!("service exited unexpectedly: {:?}", result);
    }
}

self.cancel.cancel();
tokio::time::timeout(Duration::from_secs(5), async {
    while tasks.join_next().await.is_some() {}
}).await.ok();

for service in self.services.iter().rev() {
    if let Err(e) = service.stop().await {
        warn!(service = service.name(), "error stopping service: {}", e);
    }
}
```

Note: RPC and HTTP servers may remain as separate spawned tasks outside the services vec if they have a different startup contract. Planner decides.

### Anti-Patterns to Avoid

- **Erasing `PairingHost` too early:** if only `Arc<dyn DaemonService>` remains, API routes cannot call pairing mutations without downcasting or hidden globals.
- **Moving peer websocket emission into `PeerDiscoveryWorker`:** that couples frontend projection to network-start side effects and weakens the responsibility split.
- **Keeping peer events inside `run_pairing_protocol_loop`:** that preserves the main architecture smell this phase exists to remove.
- **Replacing full peer snapshot emission with ad-hoc deltas:** existing websocket tests and pending PH51-02 direction both expect full snapshot semantics on `peers.changed`.
- **Dropping subscribe retry/backoff during extraction:** current pairing network loop already retries failed subscriptions; `PeerMonitor` should preserve that resilience.
- **Merging `DaemonPairingHost` struct and `PeerMonitor` into one `DaemonService` file:** This defeats the responsibility split. Keep them in separate modules.

## Don't Hand-Roll

| Problem                            | Don't Build                                                | Use Instead                                                                       | Why                                                             |
| ---------------------------------- | ---------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| Multi-service network event fanout | Custom dispatcher/router layer                             | Existing `NetworkEventPort::subscribe_events()` — returns fresh receiver per call | Already supports multiple independent subscribers by design     |
| Graceful shutdown graph            | Custom bool flags / bespoke stop channels                  | `CancellationToken` root + `child_token()`                                        | Official semantics match parent-child service shutdown exactly  |
| Parallel service tracking          | Manual `JoinHandle` fields like `completed_pairing_handle` | One `JoinSet` for all services                                                    | Avoids scaling boilerplate and keeps exit behavior uniform      |
| Peer list reconciliation           | Hand-maintained incremental peer cache in the daemon       | Reuse `get_p2p_peers_snapshot()` and emit full `peers.changed` payloads           | Preserves current websocket contract and matches existing tests |

**Key insight:** This phase does not need new infrastructure. The daemon already has the right runtime primitives; the issue is inconsistent application.

## Common Pitfalls

### Pitfall 1: Losing Typed Pairing Access After the Service Refactor

**What goes wrong:** `DaemonApp` becomes generic, but API routes and query code still need `active_session_id()`, `accept_pairing()`, `reject_pairing()`, and setup-state synthesis.
**Why it happens:** Trait-object erasure happens before `DaemonApiState` is assembled. `DaemonPairingHost` type-erased away too early in the composition flow.
**How to avoid:** Build `Arc<DaemonPairingHost>` in `main.rs`, clone into `DaemonApiState` (before it is consumed by the HTTP server), then also push `as Arc<dyn DaemonService>` into the services vec.
**Warning signs:** Plan talks about downcasting `dyn DaemonService`, storing pairing host globally, or making routes generic over service lookup.

### Pitfall 2: Re-splitting By File Instead of By Responsibility

**What goes wrong:** `PeerMonitor` exists, but pairing-only code and peer-only code are still mixed through shared helpers.
**Why it happens:** The old `run_pairing_network_event_loop` handled both concerns. Copy/paste extraction picks up wrong helpers.
**How to avoid:** Split by network event family first: peer arms (`PeerDiscovered`, `PeerLost`, `PeerConnected`, `PeerDisconnected`, `PeerNameUpdated`) go to `PeerMonitor`; pairing arms (`PairingMessageReceived`, `PairingFailed`) stay in `PairingHost`. Move only peer-only helpers (`emit_connection_changed`, peer snapshot fetch) into the new module.
**Warning signs:** `PeerMonitor` imports pairing orchestrator, or `PairingHost` still imports `PeersChangedFullPayload` or peer connection DTOs.

### Pitfall 3: Hidden Lifecycle Still Lives In `DaemonApp`

**What goes wrong:** Services move into `Vec<Arc<dyn DaemonService>>`, but `DaemonApp::run()` still has special cases for pairing-host startup/shutdown.
**Why it happens:** Refactor stops at trait renaming but leaves the three `completed_xxx_handle` booleans and the three separate `tokio::spawn` blocks.
**How to avoid:** Remove `pairing_host` field from `DaemonApp` struct entirely. Delete `completed_pairing_handle`, `completed_rpc_handle`, `completed_http_handle` booleans. Move HTTP/RPC into the services vec or keep them as separate tasks, but handle all via uniform `JoinSet`.
**Warning signs:** New code still has `completed_pairing_handle` or one-off `tokio::spawn` blocks for a specific service.

### Pitfall 4: Breaking Subscription Resilience During Extraction

**What goes wrong:** `PeerMonitor` exits permanently after one failed `subscribe_events()` call.
**Why it happens:** The existing backoff/retry logic in `run_pairing_network_event_loop` is tied to the old mixed loop and gets lost during extraction.
**How to avoid:** Preserve the subscribe-backoff outer loop (currently using `PAIRING_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS` / `PAIRING_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS`) in `PeerMonitor`. The retry pattern is separate from the event handling code and can be extracted as a shared helper or duplicated.
**Warning signs:** `PeerMonitor` calls `subscribe_events()` once outside any retry loop.

### Pitfall 5: Rename Drift Leaves Status/RPC Terms Half-Updated

**What goes wrong:** `service.rs` exists, but `DaemonWorkerSnapshot`, `WorkerHealth`, docs, and RPC mapping still use worker terms inconsistently.
**Why it happens:** Renames span `app.rs`, `state.rs`, `rpc/handler.rs`, `api/query.rs`, `main.rs`, `lib.rs`, and `workers/*.rs`.
**How to avoid:** Plan one explicit rename sweep as a distinct task. It touches: `DaemonWorker` → `DaemonService`, `WorkerHealth` → `ServiceHealth`, `DaemonWorkerSnapshot` → `DaemonServiceSnapshot`, import paths, and doc comments.
**Warning signs:** New trait is `DaemonService`, but `cargo check` shows `DaemonWorker` still referenced anywhere.

### Pitfall 6: DaemonPairingHost::run() Arc<Self> vs DaemonService::start() &self

**What goes wrong:** `DaemonPairingHost::run()` takes `self: Arc<Self>`. If `DaemonService::start()` is defined as `async fn start(&self, ...)`, adapting requires an extra `Arc::clone()`.
**Why it happens:** Rust async trait methods with `Arc<Self>` receivers are idiomatic for services that need to be cloned across spawned tasks, but the `DaemonService` trait must have a uniform signature for all components.
**How to avoid:** Two clean options: (a) define `DaemonService::start` as `async fn start(self: Arc<Self>, cancel: CancellationToken)` uniformly, or (b) keep `&self` and inside `PairingHost`'s impl do `Arc::new(self.clone())` (only works if Clone is derived). Option (a) is simpler.
**Warning signs:** Compiler error about mismatched receiver types when implementing the trait for `DaemonPairingHost`.

## Code Examples

Verified patterns from official sources and current codebase:

### Peer Event Arms Currently In run_pairing_network_event_loop (extraction target)

```rust
// Source: src-tauri/crates/uc-daemon/src/pairing/host.rs lines 993–1075
// These five arms should move to PeerMonitor::run_peer_event_loop():
NetworkEvent::PeerDiscovered(_peer) => {
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.get_p2p_peers_snapshot().execute().await {
        Ok(snapshots) => {
            let peers: Vec<PeerSnapshotDto> = snapshots
                .into_iter()
                .map(PeerSnapshotDto::from)
                .collect();
            emit_ws_event(&event_tx, "peers", "peers.changed", None,
                PeersChangedFullPayload { peers });
        }
        Err(e) => warn!(error = %e, "Failed to fetch peer snapshot on PeerDiscovered"),
    }
}
NetworkEvent::PeerLost(_peer_id) => { /* identical snapshot emit */ }
NetworkEvent::PeerNameUpdated { peer_id, device_name } => {
    emit_ws_event(&event_tx, "peers", "peers.name_updated", None,
        PeerNameUpdatedPayload { peer_id, device_name });
}
NetworkEvent::PeerConnected(peer) => {
    emit_ws_event(&event_tx, "peers", "peers.connection_changed", None,
        PeerConnectionChangedPayload { peer_id: peer.peer_id, device_name: Some(peer.device_name), connected: true });
}
NetworkEvent::PeerDisconnected(peer_id) => {
    emit_ws_event(&event_tx, "peers", "peers.connection_changed", None,
        PeerConnectionChangedPayload { peer_id, device_name: None, connected: false });
}
```

### DaemonApp Struct After Refactor

```rust
// Source: recommended adaptation of src-tauri/crates/uc-daemon/src/app.rs
pub struct DaemonApp {
    services: Vec<Arc<dyn DaemonService>>,  // replaces workers + pairing_host
    runtime: Arc<CoreRuntime>,
    state: Arc<RwLock<RuntimeState>>,
    socket_path: PathBuf,
    cancel: CancellationToken,
}

impl DaemonApp {
    pub fn new(
        services: Vec<Arc<dyn DaemonService>>,
        runtime: Arc<CoreRuntime>,
        socket_path: PathBuf,
    ) -> Self { ... }
}
```

### State Rename

```rust
// Source: src-tauri/crates/uc-daemon/src/state.rs (rename targets)
// DaemonWorkerSnapshot → DaemonServiceSnapshot
// WorkerHealth → ServiceHealth (in service.rs, imported here)
#[derive(Debug, Clone, PartialEq)]
pub struct DaemonServiceSnapshot {
    pub name: String,
    pub health: ServiceHealth,
}
```

### Typed Pairing Access in API State (unchanged shape)

```rust
// Source: src-tauri/crates/uc-daemon/src/api/server.rs (unchanged)
// DaemonApiState already holds: pairing_host: Option<Arc<DaemonPairingHost>>
// Routes call: state.pairing_host().unwrap().accept_pairing(...)
// This must continue working unchanged after lifecycle unification.
```

## State of the Art

| Old Approach                                                          | Current Approach                                                                  | When Changed       | Impact                                                   |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------ | -------------------------------------------------------- |
| `run_pairing_network_event_loop` handles both peer and pairing events | `PeerMonitor` handles peer lifecycle; `PairingHost` handles pairing protocol only | Phase 56 (planned) | Clear ownership and easier tests                         |
| `DaemonApp` supervises workers generically but pairing host specially | `DaemonApp` supervises one `Vec<Arc<dyn DaemonService>>`                          | Phase 56 (planned) | New services do not require app boilerplate changes      |
| Pairing construction state stored in `DaemonApp`                      | Pairing construction happens before `DaemonApp::new()`                            | Phase 56 (planned) | Cleaner app boundary and better composition-root control |
| `DaemonWorker` naming in trait/file/state                             | `DaemonService` naming across daemon lifecycle                                    | Phase 56 (planned) | Internal terminology matches real responsibility         |

**Deprecated/outdated:**

- `src-tauri/crates/uc-daemon/src/worker.rs`: deleted and replaced with `service.rs` after service unification
- `run_pairing_network_event_loop`: outdated name and outdated responsibility mix — becomes `run_pairing_protocol_loop` in `PairingHost`
- Pairing-specific constructor fields on `DaemonApp` (`pairing_orchestrator`, `pairing_action_rx`, `space_access_orchestrator`, `key_slot_store`): moved into `PairingHost` construction before `DaemonApp::new()`

## Open Questions

1. **Should RPC and HTTP servers also become `DaemonService` implementations?**
   - What we know: Current `DaemonApp::run()` spawns `run_rpc_accept_loop` and `run_http_server` as separate `JoinHandle`s alongside the workers `JoinSet`. D-05 says `DaemonApp` removes per-component boilerplate.
   - What's unclear: Whether the intent is to put RPC/HTTP into the services vec too, or just unify the formerly-separate `DaemonPairingHost` with the workers.
   - Recommendation: Keep RPC and HTTP as separate infrastructure tasks outside the services vec. They have a different nature (infrastructure bindings vs. business logic workers) and their state (`DaemonApiState`, `RpcState`) is separate from `DaemonService`. D-05 is satisfied by moving `PairingHost` into the services vec.

2. **Should `PeerDiscoveryWorker` merge into `PeerMonitor` in this phase?**
   - What we know: `PeerDiscoveryWorker` currently starts the network (`network_control.start_network()`) and announces device name on `PeerDiscovered`. `PeerMonitor` will also subscribe to `PeerDiscovered`.
   - What's unclear: Whether having two services both subscribe to `PeerDiscovered` will cause ordering or redundancy issues.
   - Recommendation: Keep them separate in Phase 56. Both subscribing to `PeerDiscovered` is safe (broadcast semantics). `PeerDiscoveryWorker` does network setup; `PeerMonitor` does websocket projection. Different responsibilities.

3. **Should `DaemonService::start()` take `self: Arc<Self>` or `&self`?**
   - What we know: `DaemonPairingHost::run()` takes `self: Arc<Self>` (required because internal tasks capture `Arc<Self>` references). `DaemonWorker::start()` takes `&self`.
   - What's unclear: Whether changing to `Arc<Self>` receiver on the trait is acceptable given the existing worker implementations.
   - Recommendation: Keep `&self` on the `DaemonService` trait for simplicity. In `DaemonPairingHost`'s impl, the `start(&self)` body creates a new `Arc::clone()` internally. This is a minor inefficiency but keeps all existing `DaemonWorker` impls compatible with just a rename.

## Environment Availability

Step 2.6: SKIPPED (no external dependencies identified — this is a pure Rust code/architecture refactor within the existing crate, no external tools, services, or new binaries required)

## Validation Architecture

### Test Framework

| Property           | Value                                                                                                     |
| ------------------ | --------------------------------------------------------------------------------------------------------- |
| Framework          | Rust `cargo test` (`tokio::test`, crate integration tests, axum/tower HTTP/WS tests)                      |
| Config file        | `src-tauri/Cargo.toml` and `src-tauri/crates/uc-daemon/Cargo.toml`                                        |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon peer_discovered_emits_peers_changed_full_payload_with_peer_list` |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon -- --test-threads=1`                                             |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                                                 | Test Type        | Automated Command                                                                                                                         | File Exists?        |
| ------- | ---------------------------------------------------------------------------------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| PH56-01 | Peer lifecycle websocket projection is owned by `PeerMonitor`, not `PairingHost`         | unit             | `cd src-tauri && cargo test -p uc-daemon peer_discovered_emits_peers_changed_full_payload_with_peer_list`                                 | ✅ (move to peers/) |
| PH56-02 | Generic service trait covers all daemon long-lived components                            | unit             | `cd src-tauri && cargo test -p uc-daemon -- --lib`                                                                                        | ✅ (trait rename)   |
| PH56-03 | DaemonApp manages uniform services vec without per-component boilerplate                 | unit/integration | `cd src-tauri && cargo test -p uc-daemon --lib app::tests`                                                                                | ✅ partial          |
| PH56-04 | Pairing mutations still work through typed pairing-host access after service unification | integration      | `cd src-tauri && cargo test -p uc-daemon --test pairing_host -- --test-threads=1`                                                         | ✅ baseline         |
| PH56-04 | Peer websocket bridge contract remains unchanged after extraction                        | integration      | `cd src-tauri && cargo test -p uc-daemon --test pairing_ws peers_and_paired_devices_incremental_events_preserve_bridge_fields -- --exact` | ✅                  |
| PH56-01 | New peer subscriber keeps retry/backoff and cancellation behavior                        | unit             | `cd src-tauri && cargo test -p uc-daemon peer_monitor_`                                                                                   | ❌ Wave 0           |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon -- --lib`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon -- --test-threads=1`
- **Phase gate:** Full `uc-daemon` suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/peers/monitor.rs` tests for peer-event filtering, snapshot emission, retry/backoff, and cancellation
- [ ] `src-tauri/crates/uc-daemon/src/app.rs` or `src-tauri/crates/uc-daemon/tests/service_lifecycle.rs` coverage for uniform service startup/shutdown and unexpected service exit
- [ ] Rename-consistency test confirming `DaemonServiceSnapshot` / `ServiceHealth` naming in status/logging
- [ ] Note: `cd src-tauri && cargo test -p uc-daemon --test pairing_host -- --test-threads=1` should be confirmed green as a baseline before Phase 56 starts; previous research noted `daemon_pairing_host_accept_pairing_projects_verifying_stage` may be flaky

_(If no gaps: "None — existing test infrastructure covers all phase requirements")_

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-daemon/src/app.rs` — current daemon supervision and hardcoded pairing-host lifecycle (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — current responsibility mix; exact extraction lines 993–1104 identified (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/worker.rs` — current trait definition (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` — overlap boundary with PeerMonitor (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — placeholder service (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/state.rs` — DaemonWorkerSnapshot rename target (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/api/server.rs` — DaemonApiState.pairing_host typed field (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — 10+ typed pairing_host method calls requiring PH56-04 preservation (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/main.rs` — composition root; DaemonApp construction site (read 2026-03-24)

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon/src/lib.rs` — pub module exports (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/api/pairing.rs` — pairing DTO types (read 2026-03-24)
- `src-tauri/crates/uc-daemon/src/workers/mod.rs` — module structure (read 2026-03-24)
- `.planning/phases/56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management/56-CONTEXT.md` — locked decisions and canonical refs (read 2026-03-24)

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — existing workspace stack was verified from source files and no new libraries are required
- Architecture: HIGH — exact extraction lines identified in `host.rs`, typed access preservation path is clear
- Pitfalls: HIGH — risks are directly visible in current code structure; typed-access pitfall is the main implementation risk

**Research date:** 2026-03-24
**Valid until:** 2026-04-23
