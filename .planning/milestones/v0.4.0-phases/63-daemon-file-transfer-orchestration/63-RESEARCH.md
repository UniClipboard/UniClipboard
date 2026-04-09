# Phase 63: Daemon File Transfer Orchestration - Research

**Researched:** 2026-03-26
**Domain:** Rust daemon architecture — file transfer lifecycle in daemon (mirroring Phases 61/62 pattern)
**Confidence:** HIGH

## Summary

Phase 63 mirrors the progression of Phases 61 and 62: clipboard outbound and inbound sync were moved into daemon workers, now file transfer orchestration must follow. Phase 60 already extracted `FileTransferOrchestrator` from `file_transfer_wiring.rs` into `uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs` and exposed `build_file_transfer_orchestrator()` in `uc-bootstrap/assembly.rs`. The daemon already receives `file_cache_dir` from `DaemonBootstrapContext` and passes it to `InboundClipboardSyncWorker` for file metadata storage. What is missing: the daemon does not yet subscribe to `NetworkEventPort`, run a network event loop, or instantiate `FileTransferOrchestrator` for durable lifecycle tracking (pending → transferring → completed/failed), timeout sweep, or startup reconciliation.

The daemon's `main.rs` already has `daemon_network_events` (`ctx.deps.network_ports.events.clone()`) available but currently passes it only to `PeerDiscoveryWorker`. A new `FileSyncOrchestratorWorker` (or similar) must subscribe to network events and call `FileTransferOrchestrator` methods for `TransferProgress`, `FileTransferCompleted`, and `FileTransferFailed` events — exactly what `run_network_realtime_loop` does in `wiring.rs`. Additionally, the daemon needs to run the reconcile-on-startup sweep and the periodic timeout sweep, which in Tauri are handled by `start_background_tasks`.

The `InboundClipboardSyncWorker` currently discards `pending_transfers` from `InboundApplyOutcome::Applied`, which means file transfer pending records are never seeded in the DB. Phase 63 must also fix this gap — either by extending `InboundClipboardSyncWorker` to call `orchestrator.tracker().record_pending_from_clipboard()`, or by creating a combined service.

**Primary recommendation:** Create a `FileSyncOrchestratorWorker` implementing `DaemonService` that subscribes to `NetworkEventPort`, handles transfer lifecycle events via `FileTransferOrchestrator`, and extend `InboundClipboardSyncWorker` to receive and use the orchestrator for pending record seeding. Reuse the existing `build_file_transfer_orchestrator()` builder already in `uc-bootstrap`.

## Standard Stack

### Core

| Library                                                     | Version   | Purpose                          | Why Standard                                                            |
| ----------------------------------------------------------- | --------- | -------------------------------- | ----------------------------------------------------------------------- |
| `uc-app::usecases::file_sync::FileTransferOrchestrator`     | workspace | File transfer lifecycle          | Already extracted in Phase 60, all 9 wiring.rs functions are methods    |
| `uc-app::usecases::file_sync::TrackInboundTransfersUseCase` | workspace | DB CRUD for transfer records     | Wrapped by orchestrator                                                 |
| `uc-bootstrap::assembly::build_file_transfer_orchestrator`  | workspace | Constructor for orchestrator     | Follows `build_setup_orchestrator` pattern                              |
| `tokio_util::sync::CancellationToken`                       | 0.7       | Cooperative shutdown             | Used by all DaemonService implementations                               |
| `async_trait`                                               | 0.1       | Trait for async in DaemonService | Used by every DaemonService                                             |
| `tokio::sync::broadcast`                                    | 1.x       | WS event fan-out                 | `event_tx: broadcast::Sender<DaemonWsEvent>` shared across all services |

### Supporting

| Library                                               | Version   | Purpose                                        | When to Use                                        |
| ----------------------------------------------------- | --------- | ---------------------------------------------- | -------------------------------------------------- |
| `uc-app::usecases::file_sync::SyncInboundFileUseCase` | workspace | Final file processing after network transfer   | Called on `FileTransferCompleted` event            |
| `uc-core::ports::SystemClipboardPort`                 | workspace | Restore file to OS clipboard after transfer    | Used in `restore_file_to_clipboard_after_transfer` |
| `uc-core::ports::ClipboardChangeOriginPort`           | workspace | Write-back loop prevention during file restore | Shared Arc with ClipboardWatcherWorker             |
| `blake3`                                              | 0.3.x     | File content hash verification                 | Used in wiring.rs to verify transferred file hash  |
| `tracing`                                             | 0.1       | Structured logging with spans                  | All daemon code uses tracing                       |

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-daemon/src/workers/
├── clipboard_watcher.rs       # Phase 57 - clipboard capture
├── inbound_clipboard_sync.rs  # Phase 62 - receives peer clipboard
├── peer_discovery.rs          # network discovery
└── file_sync_orchestrator.rs  # NEW Phase 63 - file transfer lifecycle
```

### Pattern 1: DaemonService for Network Event Loop

The `FileSyncOrchestratorWorker` follows the `PeerDiscoveryWorker` pattern: subscribes to `NetworkEventPort::subscribe_events()`, loops over events, calls orchestrator methods for file transfer events, ignores non-file events.

**What:** `FileSyncOrchestratorWorker` wraps `FileTransferOrchestrator` and `NetworkEventPort`.
**When to use:** Any time the daemon needs to react to network events for lifecycle management.

```rust
// Source: src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs (pattern)
// and wiring.rs run_network_realtime_loop for file transfer event handling

pub struct FileSyncOrchestratorWorker {
    orchestrator: Arc<FileTransferOrchestrator>,
    network_events: Arc<dyn NetworkEventPort>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    system_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    file_cache_dir: PathBuf,
    settings: Arc<dyn SettingsPort>,
}

#[async_trait]
impl DaemonService for FileSyncOrchestratorWorker {
    fn name(&self) -> &str { "file-sync-orchestrator" }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        // 1. Run startup reconciliation (orphaned in-flight transfers → failed)
        self.orchestrator.reconcile_on_startup().await;

        // 2. Start timeout sweep (15s interval, cancellable)
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let _sweep = self.orchestrator.spawn_timeout_sweep(cancel_rx);

        // 3. Subscribe to network events with retry backoff
        loop {
            let event_rx = tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                result = self.network_events.subscribe_events() => result?,
            };
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = cancel_tx.send(true); // stop sweep
                    return Ok(());
                }
                _ = self.run_event_loop(event_rx) => {}
            }
        }
    }
}
```

### Pattern 2: Pending Record Seeding in InboundClipboardSyncWorker

The existing `InboundClipboardSyncWorker::run_receive_loop` ignores `pending_transfers` from `InboundApplyOutcome::Applied`. This must be fixed by passing `Arc<FileTransferOrchestrator>` into the worker and calling `tracker().record_pending_from_clipboard()` — mirroring wiring.rs lines 559-636.

```rust
// Source: wiring.rs run_clipboard_receive_loop lines 552-637
// The orchestrator call sequence for file transfers in inbound clipboard:

if let InboundApplyOutcome::Applied {
    entry_id: Some(ref entry_id),
    ref pending_transfers,
} = outcome {
    if !pending_transfers.is_empty() {
        let now_ms = orchestrator.now_ms();
        let db_transfers: Vec<PendingInboundTransfer> = pending_transfers
            .iter()
            .map(|t| PendingInboundTransfer {
                transfer_id: t.transfer_id.clone(),
                entry_id: entry_id.to_string(),
                origin_device_id: origin_device_id.clone(),
                filename: t.filename.clone(),
                cached_path: t.cached_path.clone(),
                created_at_ms: now_ms,
            })
            .collect();
        orchestrator.tracker().record_pending_from_clipboard(db_transfers).await?;
        // Reconcile early completions + drain cache
        let seeded_ids: Vec<String> = pending_transfers.iter().map(|t| t.transfer_id.clone()).collect();
        let early = orchestrator.early_completion_cache().drain_matching(&seeded_ids);
        // ... for each early completion: mark_completed + emit status
        orchestrator.emit_pending_status(&entry_id.to_string(), pending_transfers);
    }
}
```

### Pattern 3: FileTransferCompleted Network Event Handling

The network event `NetworkEvent::FileTransferCompleted` triggers `SyncInboundFileUseCase::handle_transfer_complete()`, hash verification, and clipboard restore — all matching the wiring.rs pattern at lines 1175-1344.

```rust
// Source: wiring.rs run_network_realtime_loop lines 1175-1344
NetworkEvent::FileTransferCompleted {
    transfer_id, peer_id, filename, file_path, batch_id, batch_total,
} => {
    let orch_for_spawn = Arc::clone(&self.orchestrator);
    let inbound_uc = SyncInboundFileUseCase::new(
        self.settings.clone(),
        self.file_cache_dir.clone(),
    );
    tokio::spawn(async move {
        let file_bytes = tokio::fs::read(&file_path).await?;
        let expected_hash = blake3::hash(&file_bytes).to_hex().to_string();
        match inbound_uc.handle_transfer_complete(&transfer_id, &file_path, &expected_hash).await {
            Ok(result) => {
                orch_for_spawn.handle_transfer_completed(&result.transfer_id, Some(&expected_hash)).await;
                // emit Transfer::Completed host event via DaemonApiEventEmitter (currently logged only)
                // restore file to OS clipboard if not batch
            }
            Err(err) => {
                orch_for_spawn.handle_transfer_failed(&transfer_id, &err.to_string()).await;
            }
        }
    }.instrument(info_span!("inbound_file_sync", transfer_id = %transfer_id)));
    // Handle batch accumulation outside spawn
}
```

### Pattern 4: WS Event Emission for File Transfer Status

The daemon currently logs `HostEvent::Transfer(_)` in `DaemonApiEventEmitter` (line 100: `HostEvent::Transfer(_) => Self::log_non_setup_event("transfer")`). For the GUI to receive file transfer status updates, the daemon needs to emit `file-transfer://status-changed` events over the WS broadcast channel.

Two options:

1. **Option A (recommended):** Extend `DaemonApiEventEmitter::emit()` to handle `HostEvent::Transfer(TransferHostEvent::StatusChanged)` and emit a `DaemonWsEvent` on a `"file-transfer"` topic.
2. **Option B:** Emit `DaemonWsEvent` directly in `FileSyncOrchestratorWorker`, bypassing the emitter cell pattern.

Option A is preferred because it follows the established emitter cell pattern — the orchestrator already uses the emitter cell, and this emitter is swapped to `DaemonApiEventEmitter` when daemon starts (app.rs line 149-151).

### Anti-Patterns to Avoid

- **Duplicating network event subscription:** Do not also subscribe in `InboundClipboardSyncWorker` — one worker owns `NetworkEventPort`, one owns `ClipboardTransportPort`. Keep responsibilities separate.
- **Building FileTransferOrchestrator without shared emitter_cell:** Must use `build_file_transfer_orchestrator()` from uc-bootstrap with the same `emitter_cell` that gets swapped to `DaemonApiEventEmitter` in `DaemonApp::run()`. Do NOT create a standalone orchestrator.
- **Forgetting the early completion cache:** Race condition exists where `FileTransferCompleted` arrives before pending records are seeded. The wiring.rs `EarlyCompletionCache` pattern MUST be replicated in the daemon.
- **Blocking batch clipboard restore:** `restore_file_to_clipboard_after_transfer` must run inside a `tokio::spawn` to avoid blocking the event loop, per wiring.rs line 1335-1343.

## Don't Hand-Roll

| Problem                            | Don't Build  | Use Instead                                        | Why                                                                        |
| ---------------------------------- | ------------ | -------------------------------------------------- | -------------------------------------------------------------------------- |
| Transfer lifecycle state machine   | Custom FSM   | `FileTransferOrchestrator` methods                 | Already has pending/transferring/completed/failed + early completion cache |
| Timeout sweep                      | Custom timer | `FileTransferOrchestrator::spawn_timeout_sweep()`  | Handles 60s pending, 5min transferring cutoffs                             |
| Startup orphan reconciliation      | Custom query | `FileTransferOrchestrator::reconcile_on_startup()` | Handles bulk_fail_inflight + cleanup                                       |
| File hash verification             | Custom hash  | `blake3::hash()` (already in deps)                 | Used identically in wiring.rs                                              |
| Network event subscription backoff | Custom retry | Pattern from `register_pairing_background_tasks`   | Already tested, handles cancellation                                       |

**Key insight:** `FileTransferOrchestrator` already encapsulates all complexity. Phase 63 is about wiring it into the daemon, not rebuilding it.

## Common Pitfalls

### Pitfall 1: InboundClipboardSyncWorker ignores pending_transfers

**What goes wrong:** File clipboard entries are persisted but `pending_transfers` are discarded — no DB records for the transfer tracker, so `mark_completed`/`mark_failed` find no rows to update. File transfer status is never tracked.
**Why it happens:** Phase 62 implemented only the clipboard message path; file transfer lifecycle was deferred to Phase 63.
**How to avoid:** Pass `Arc<FileTransferOrchestrator>` into `InboundClipboardSyncWorker`, call `record_pending_from_clipboard()` after a successful `Applied { entry_id: Some(_), pending_transfers: ... }`.
**Warning signs:** `mark_completed` returning `Ok(false)` ("no row found") for every file transfer.

### Pitfall 2: Emitter cell not shared with file transfer orchestrator

**What goes wrong:** File transfer status-changed events are emitted to a `LoggingEventEmitter` that was captured at construction time, never reaching the WS broadcast channel.
**Why it happens:** Forgetting to pass the shared `emitter_cell` Arc from `WiredDependencies` to `build_file_transfer_orchestrator()`.
**How to avoid:** Use `build_file_transfer_orchestrator(deps.storage.file_transfer_repo.clone(), emitter_cell.clone(), deps.system.clock.clone())` — same pattern as in `wire_dependencies()` for the Tauri wiring.
**Warning signs:** No `file-transfer://status-changed` events received by the frontend even though transfers complete.

### Pitfall 3: DaemonApiEventEmitter silently drops Transfer events

**What goes wrong:** Even if orchestrator emits correctly, `DaemonApiEventEmitter` logs transfer events as `log_non_setup_event("transfer")` and drops them (line 100 in `event_emitter.rs`).
**Why it happens:** Transfer events weren't needed in previous daemon phases — setup was the only concern.
**How to avoid:** Extend `DaemonApiEventEmitter::emit()` to handle `HostEvent::Transfer(TransferHostEvent::StatusChanged)` and emit to a `"file-transfer"` WS topic. Must also define the topic string (following `daemon_api_strings` pattern from Phase 56.1).
**Warning signs:** Orchestrator emits correctly but no WS events appear in the broadcast channel.

### Pitfall 4: Network event loop not separate from clipboard receive loop

**What goes wrong:** File transfer network events (`TransferProgress`, `FileTransferCompleted`, `FileTransferFailed`) arrive on `NetworkEventPort` while clipboard content arrives on `ClipboardTransportPort`. These are different subscriptions.
**Why it happens:** Confusing the two transport channels.
**How to avoid:** `FileSyncOrchestratorWorker` subscribes to `deps.network_ports.events` (same as `PeerDiscoveryWorker`). `InboundClipboardSyncWorker` subscribes to `deps.network_ports.clipboard`. Keep them separate.

### Pitfall 5: Blocking the event loop with file I/O

**What goes wrong:** `tokio::fs::read()` (for hash verification) blocks the event loop if called without `spawn`.
**Why it happens:** Forgetting to wrap the `FileTransferCompleted` handler in `tokio::spawn`.
**How to avoid:** Every `FileTransferCompleted` handler must `tokio::spawn` the file processing — same as wiring.rs lines 1211-1307.

### Pitfall 6: spawn_timeout_sweep cancellation

**What goes wrong:** `spawn_timeout_sweep` takes a `watch::Receiver<bool>`. The `watch::Sender` must be kept alive or sent `true` to cancel — simply dropping the sender terminates the sweep task.
**Why it happens:** The sweep uses `cancel.changed()` to detect shutdown.
**How to avoid:** Store the `cancel_tx` for the sweep. In `FileSyncOrchestratorWorker::start()`, when the CancellationToken fires, send `true` to `cancel_tx` before returning. The current wiring.rs uses `std::mem::forget(cancel_tx)` which is a workaround — the daemon should use the cancel token approach instead.

## Code Examples

Verified patterns from the codebase:

### FileTransferOrchestrator construction (uc-bootstrap/assembly.rs lines 1044-1057)

```rust
pub fn build_file_transfer_orchestrator(
    file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort>,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    clock: Arc<dyn ClockPort>,
) -> Arc<uc_app::usecases::file_sync::FileTransferOrchestrator> {
    let tracker = Arc::new(uc_app::usecases::file_sync::TrackInboundTransfersUseCase::new(
        file_transfer_repo,
    ));
    Arc::new(uc_app::usecases::file_sync::FileTransferOrchestrator::new(
        tracker,
        emitter_cell,
        clock,
    ))
}
```

### DaemonApp::run() swaps emitter into the orchestrator's cell (app.rs lines 148-151)

```rust
// 3. Wire the event emitter into the runtime so use cases can emit WS events
self.runtime
    .set_event_emitter(Arc::new(DaemonApiEventEmitter::new(
        self.event_tx.clone(),
    )));
```

Note: This swaps the emitter in the runtime's `emitter_cell`, which must be the same cell used by `FileTransferOrchestrator`. The `DaemonApiEventEmitter` currently only handles setup events — Phase 63 must extend it to handle `HostEvent::Transfer(TransferHostEvent::StatusChanged)`.

### DaemonBootstrapContext already provides background deps

```rust
// daemon/main.rs line 47
let file_cache_dir = ctx.storage_paths.file_cache_dir.clone();
// ctx.background.file_transfer_orchestrator is already built by wire_dependencies()
// No need to call build_file_transfer_orchestrator again — just use ctx.background.file_transfer_orchestrator
```

### PeerDiscoveryWorker subscription pattern (reference for FileSyncOrchestratorWorker)

```rust
// Source: src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs
pub struct PeerDiscoveryWorker {
    network_control: Arc<dyn NetworkControlPort>,
    network_events: Arc<dyn NetworkEventPort>,
    peer_directory: Arc<dyn PeerDirectoryPort>,
    settings: Arc<dyn SettingsPort>,
}
```

## State of the Art

| Old Approach                                                               | Current Approach                                          | When Changed | Impact                                                                                 |
| -------------------------------------------------------------------------- | --------------------------------------------------------- | ------------ | -------------------------------------------------------------------------------------- |
| File transfer wiring in `file_transfer_wiring.rs` (502 lines, 9 functions) | `FileTransferOrchestrator` in uc-app with method dispatch | Phase 60     | Daemon can now reuse without Tauri coupling                                            |
| `BackgroundRuntimeDeps.file_transfer_orchestrator` unused by daemon        | Ready to be consumed by daemon composition root           | Phase 60     | `ctx.background.file_transfer_orchestrator` already exists in `DaemonBootstrapContext` |
| `DaemonApiEventEmitter` drops all `HostEvent::Transfer` events             | Transfer events must be forwarded to WS                   | Phase 63     | Extend emitter to handle `StatusChanged` variant                                       |
| `InboundClipboardSyncWorker` ignores `pending_transfers`                   | Must seed pending records into DB                         | Phase 63     | File transfer tracking only works after this fix                                       |

**Deprecated/outdated:**

- The plan to pass `file_transfer_orchestrator: Option<Arc<...>>` (Optional) from wiring.rs — daemon uses non-Optional, since orchestrator is always present.

## Open Questions

1. **Should the FileSyncOrchestratorWorker also handle PeerReady/PeerConnected events?**
   - What we know: wiring.rs `run_network_realtime_loop` handles both transfer events AND peer connection events (lines 1102-1144). The daemon's `PeerMonitor` already handles peer lifecycle separately.
   - What's unclear: Does the file transfer path need peer connection events (e.g., to clean up in-flight transfers when a peer disconnects)?
   - Recommendation: Focus only on transfer events (`TransferProgress`, `FileTransferCompleted`, `FileTransferFailed`) in Phase 63. Peer connection events are already handled by `PeerMonitor`.

2. **Does DaemonWsBridge in uc-daemon-client need to translate file-transfer WS events?**
   - What we know: `DaemonWsBridge` currently handles clipboard, pairing, peers, setup, space-access topics. File transfer events are needed by the GUI.
   - What's unclear: Whether uc-daemon-client's `DaemonWsBridge` needs updating or whether the Tauri event bridge for file transfers should be handled in Phase 64.
   - Recommendation: Define the WS topic/event strings in Phase 63 but defer the DaemonWsBridge translation to Phase 64 (Tauri sync retirement). The daemon should emit correctly even if the Tauri bridge doesn't translate yet.

3. **Should `origin_device_id` be tracked per-message in InboundClipboardSyncWorker?**
   - What we know: wiring.rs extracts `message.origin_device_id` before the receive loop (line 526). `PendingInboundTransfer` requires `origin_device_id`.
   - What's unclear: `InboundClipboardSyncWorker::run_receive_loop` doesn't currently extract this.
   - Recommendation: `ClipboardMessage` has `origin_device_id` field — extract it alongside `pending_transfers` inside the `Applied` match arm.

## Environment Availability

Step 2.6: SKIPPED (no external dependencies — this is a pure Rust code/wiring phase).

## Validation Architecture

### Test Framework

| Property           | Value                                     |
| ------------------ | ----------------------------------------- |
| Framework          | cargo test (Rust unit tests)              |
| Config file        | src-tauri/Cargo.toml workspace            |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon` |
| Full suite command | `cd src-tauri && cargo test`              |

### Phase Requirements → Test Map

Phase 63 requirements are TBD, but based on the pattern from Phases 61/62:

| Req ID  | Behavior                                                                                      | Test Type   | Automated Command                                                | File Exists?         |
| ------- | --------------------------------------------------------------------------------------------- | ----------- | ---------------------------------------------------------------- | -------------------- |
| PH63-01 | `FileSyncOrchestratorWorker::start()` calls `reconcile_on_startup` before entering event loop | unit        | `cd src-tauri && cargo test -p uc-daemon file_sync_orchestrator` | ❌ Wave 0            |
| PH63-02 | `TransferProgress` events call `handle_transfer_progress` (pending→transferring)              | unit        | `cd src-tauri && cargo test -p uc-daemon file_sync_orchestrator` | ❌ Wave 0            |
| PH63-03 | `FileTransferCompleted` events call `handle_transfer_completed` with hash                     | unit        | `cd src-tauri && cargo test -p uc-daemon file_sync_orchestrator` | ❌ Wave 0            |
| PH63-04 | `FileTransferFailed` events call `handle_transfer_failed`                                     | unit        | `cd src-tauri && cargo test -p uc-daemon file_sync_orchestrator` | ❌ Wave 0            |
| PH63-05 | `InboundClipboardSyncWorker` seeds pending records for file clipboard messages                | unit        | `cd src-tauri && cargo test -p uc-daemon inbound_clipboard_sync` | ✅ (extend existing) |
| PH63-06 | `DaemonApiEventEmitter` emits `file-transfer://status-changed` WS event for `StatusChanged`   | unit        | `cd src-tauri && cargo test -p uc-daemon event_emitter`          | ✅ (extend existing) |
| PH63-07 | Full daemon test suite passes                                                                 | integration | `cd src-tauri && cargo test -p uc-daemon`                        | ✅                   |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/workers/file_sync_orchestrator.rs` — covers PH63-01 through PH63-04
- [ ] Extend `src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs` — covers PH63-05
- [ ] Extend `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` — covers PH63-06

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs` — Complete FileTransferOrchestrator implementation with all 9 methods
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — `run_network_realtime_loop` (lines 1062-1365), `run_clipboard_receive_loop` (lines 516-705), `start_background_tasks` (lines 103-409)
- `src-tauri/crates/uc-daemon/src/main.rs` — Current daemon composition root showing available context
- `src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs` — Phase 62 pattern to follow/extend
- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` — DaemonApiEventEmitter with Transfer event currently dropped
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — `build_file_transfer_orchestrator()` builder, `BackgroundRuntimeDeps` struct

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` — DaemonService subscription pattern reference
- `.planning/REQUIREMENTS.md` — Phase 60-62 requirements for pattern continuity (DAEM-F03, DAEM-F04)

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all libraries already used in codebase, no new dependencies needed
- Architecture: HIGH — wiring.rs is the ground truth; Phase 63 ports it to daemon
- Pitfalls: HIGH — discovered by reading actual code gaps (InboundClipboardSyncWorker ignores pending_transfers, DaemonApiEventEmitter drops Transfer events)

**Research date:** 2026-03-26
**Valid until:** 2026-04-25 (stable codebase, no external library changes)
