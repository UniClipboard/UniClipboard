# Phase 62: Daemon Inbound Clipboard Sync - Research

**Researched:** 2026-03-25
**Domain:** Daemon inbound clipboard sync — receive peer clipboard, write to local OS clipboard, persist entry, broadcast WS event
**Confidence:** HIGH

## Summary

Phase 62 adds inbound clipboard sync to the daemon. When a remote peer sends clipboard content over the libp2p network, the daemon must:

1. Subscribe to incoming `ClipboardMessage` messages via `ClipboardTransportPort::subscribe_clipboard()`
2. Apply the message through the existing `SyncInboundClipboardUseCase` (already battle-tested in uc-tauri wiring)
3. Broadcast a `clipboard.new_content` WS event for GUI clients
4. Mark the snapshot hash in `ClipboardChangeOriginPort` as `RemotePush` BEFORE writing to the OS clipboard, so `DaemonClipboardChangeHandler` skips re-syncing the inbound content

The existing `SyncInboundClipboardUseCase::with_capture_dependencies()` covers both "Full" mode (write to OS clipboard) and "Passive" mode (persist only). For the daemon, the mode must be **Full** — the daemon is the OS clipboard owner, so it must write received content to the system clipboard directly.

The `run_clipboard_receive_loop` in `wiring.rs` is the exact pattern to replicate in the daemon. The daemon variant can be implemented as a new `InboundClipboardSyncWorker` implementing `DaemonService`, following the `ClipboardWatcherWorker` pattern.

**Primary recommendation:** Create `InboundClipboardSyncWorker` in `uc-daemon/src/workers/inbound_clipboard_sync.rs` that subscribes to `ClipboardTransportPort`, calls `SyncInboundClipboardUseCase`, and broadcasts the WS event — mirroring `run_clipboard_receive_loop` in wiring.rs.

## Standard Stack

### Core

| Library                            | Version              | Purpose                                                                       | Why Standard                                                                                        |
| ---------------------------------- | -------------------- | ----------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `SyncInboundClipboardUseCase`      | in-crate (uc-app)    | Apply inbound ClipboardMessage to local clipboard and persist                 | Already implements all logic: dedup, echo prevention, encryption, OS write, passive capture         |
| `ClipboardTransportPort`           | in-crate (uc-core)   | Subscribe to incoming clipboard messages from peers                           | Canonical port for network clipboard; returns `mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>` |
| `ClipboardChangeOriginPort`        | in-crate (uc-core)   | Mark snapshot as `RemotePush` before OS write to prevent loopback             | Shared `Arc` already created in `main.rs` and injected into `DaemonClipboardChangeHandler`          |
| `InMemoryClipboardChangeOrigin`    | in-crate (uc-infra)  | Concrete implementation of `ClipboardChangeOriginPort`                        | Already used in daemon main.rs                                                                      |
| `DaemonService`                    | in-crate (uc-daemon) | Lifecycle trait with `start(CancellationToken)` + `stop()` + `health_check()` | All daemon workers implement this                                                                   |
| `broadcast::Sender<DaemonWsEvent>` | tokio                | Broadcast WS events to GUI clients                                            | Already injected into `ClipboardWatcherWorker` and `DaemonPairingHost`                              |

### Supporting

| Library                               | Version             | Purpose                                                                          | When to Use                                     |
| ------------------------------------- | ------------------- | -------------------------------------------------------------------------------- | ----------------------------------------------- |
| `TransferPayloadDecryptorAdapter`     | in-crate (uc-infra) | Implements `TransferPayloadDecryptorPort` for `SyncInboundClipboardUseCase`      | Required as dependency in use case construction |
| `FileTransferOrchestrator`            | in-crate (uc-app)   | Handle pending file transfer linkage after inbound clipboard with file_transfers | Pass `None` in Phase 62; Phase 63 adds this     |
| `tokio_util::sync::CancellationToken` | 0.7                 | Cooperative cancellation in service start loop                                   | Standard across all daemon services             |

### Alternatives Considered

| Instead of                                  | Could Use                       | Tradeoff                                                                              |
| ------------------------------------------- | ------------------------------- | ------------------------------------------------------------------------------------- |
| Full mode SyncInboundClipboardUseCase       | Passive mode                    | Passive doesn't write OS clipboard; daemon must write since it IS the clipboard owner |
| InboundClipboardSyncWorker as DaemonService | Inline into PeerDiscoveryWorker | Separation of concerns; each service has one responsibility                           |
| Pass `file_transfer_orchestrator: None`     | Full file orchestration         | Phase 63 handles file orchestration; Phase 62 scope is clipboard content only         |

## Architecture Patterns

### Recommended Project Structure

The new file goes in the existing workers directory:

```
src-tauri/crates/uc-daemon/src/workers/
├── clipboard_watcher.rs         # Existing: outbound capture + sync
├── inbound_clipboard_sync.rs    # NEW: inbound receive + write + WS event
├── mod.rs                       # Add pub mod inbound_clipboard_sync
└── peer_discovery.rs            # Existing
```

### Pattern 1: DaemonService with subscribe-then-loop

**What:** The worker calls `subscribe_clipboard()` once, then runs a receive loop. If the channel closes, it re-subscribes with backoff. Mirrors the `"clipboard_receive"` task in `wiring.rs`.

**When to use:** Any daemon worker that consumes a streaming port subscription.

**Example (from wiring.rs lines 248-290):**

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
loop {
    let subscribe_result = tokio::select! {
        _ = token.cancelled() => { return; }
        result = clipboard_network.subscribe_clipboard() => result,
    };
    match subscribe_result {
        Ok(clipboard_rx) => {
            run_clipboard_receive_loop(clipboard_rx, &sync_inbound_usecase, ...).await;
        }
        Err(err) => {
            // exponential backoff, then retry
        }
    }
}
```

### Pattern 2: SyncInboundClipboardUseCase construction (Full mode with capture deps)

**What:** Use `with_capture_dependencies()` to construct the use case in Full mode. This enables both OS clipboard write (for non-file content) AND DB persistence (for file content or passive capture). The daemon is always in Full mode.

**Example (from wiring.rs new_sync_inbound_clipboard_usecase, line 412-434):**

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
SyncInboundClipboardUseCase::with_capture_dependencies(
    ClipboardIntegrationMode::Full,          // daemon owns OS clipboard
    deps.clipboard.system_clipboard.clone(),
    deps.clipboard.clipboard_change_origin.clone(), // SHARED Arc
    deps.security.encryption_session.clone(),
    deps.security.encryption.clone(),
    deps.device.device_identity.clone(),
    Arc::new(TransferPayloadDecryptorAdapter),
    deps.clipboard.clipboard_entry_repo.clone(),
    deps.clipboard.clipboard_event_repo.clone(),
    deps.clipboard.representation_policy.clone(),
    deps.clipboard.representation_normalizer.clone(),
    deps.clipboard.representation_cache.clone(),
    deps.clipboard.spool_queue.clone(),
    Some(file_cache_dir),                    // runtime.storage_paths.file_cache_dir
    deps.settings.clone(),
)
```

### Pattern 3: ClipboardNewContent WS event after successful apply

**What:** After `InboundApplyOutcome::Applied { entry_id: Some(id), .. }`, broadcast a `clipboard.new_content` WS event with `origin = "remote"`. This is what `DaemonClipboardChangeHandler` does for local captures.

**Example (from clipboard_watcher.rs lines 200-226):**

```rust
// Source: src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs
let payload = ClipboardNewContentPayload {
    entry_id: entry_id.to_string(),
    preview: "Remote clipboard content".to_string(),
    origin: "remote".to_string(),
};
let event = DaemonWsEvent {
    topic: ws_topic::CLIPBOARD.to_string(),
    event_type: ws_event::CLIPBOARD_NEW_CONTENT.to_string(),
    session_id: None,
    ts: chrono::Utc::now().timestamp_millis(),
    payload: serde_json::to_value(payload)?,
};
let _ = event_tx.send(event); // ignore no-receivers error
```

### Pattern 4: Write-back loop prevention

**What:** `SyncInboundClipboardUseCase` already handles this internally — it calls `clipboard_change_origin.remember_remote_snapshot_hash()` before writing to OS clipboard and `set_next_origin(RemotePush)` after writing. The SHARED `clipboard_change_origin` Arc between `InboundClipboardSyncWorker` and `DaemonClipboardChangeHandler` is what makes this work.

**Key constraint:** The `clipboard_change_origin` passed to `SyncInboundClipboardUseCase` MUST be the SAME Arc instance used by `DaemonClipboardChangeHandler`. In `main.rs`, this is the `clipboard_change_origin` variable (line 104-105). The new worker must receive this same Arc.

### Pattern 5: InboundApplyOutcome::Applied with entry_id None (Full mode)

**What:** In Full mode (non-file content), `SyncInboundClipboardUseCase` writes to OS clipboard directly and returns `Applied { entry_id: None }`. The `ClipboardWatcher` will then capture the change and persist it. This is correct behavior — the daemon must NOT double-emit a WS event in this case (the ClipboardWatcher's capture will emit its own `clipboard.new_content`).

**When to emit WS event from inbound:**

- Passive mode: `Applied { entry_id: Some(id) }` — no watcher fires, must emit
- Full mode with file_transfers: `Applied { entry_id: Some(id) }` — watcher skipped, must emit
- Full mode without file_transfers: `Applied { entry_id: None }` — watcher will fire, do NOT emit

This logic is in `wiring.rs` lines 644-681 and must be replicated in the daemon worker.

### Anti-Patterns to Avoid

- **Constructing SyncInboundClipboardUseCase with `new()` instead of `with_capture_dependencies()`:** `new()` rejects Passive mode and doesn't set up the capture use case internally. Always use `with_capture_dependencies()` for daemon.
- **Creating a separate `clipboard_change_origin` for inbound:** Both `DaemonClipboardChangeHandler` and the inbound worker MUST share the same Arc. Separate instances break write-back loop prevention.
- **Calling `SyncInboundClipboardUseCase` directly from main.rs:** Must be in a DaemonService that can be cancelled.
- **Double-emitting WS event for Full mode non-file content:** Only emit from inbound worker when `entry_id: Some(_)` (file transfers or passive mode); for `entry_id: None` in Full mode, the ClipboardWatcher emits it.

## Don't Hand-Roll

| Problem                                  | Don't Build                  | Use Instead                                                                                                | Why                                                |
| ---------------------------------------- | ---------------------------- | ---------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Inbound dedup by message_id              | Custom message ID tracker    | `SyncInboundClipboardUseCase` internal `recent_ids` VecDeque (TTL=600s, max=1024)                          | Already battle-tested with TTL pruning             |
| Echo prevention (ignore self-originated) | Check device_id in worker    | `SyncInboundClipboardUseCase.execute_with_outcome()` checks `origin_device_id == local_device_id`          | Built into use case                                |
| Encryption session check                 | Guard in worker              | `SyncInboundClipboardUseCase` checks `encryption_session.is_ready()` and returns `Skipped`                 | Built into use case                                |
| V3 payload decoding and repr selection   | Custom decode                | `SyncInboundClipboardUseCase.apply_v3_inbound()` handles decode, priority selection, file path rewriting   | 700+ lines of tested logic                         |
| OS clipboard write with loopback guard   | Direct `write_snapshot` call | `SyncInboundClipboardUseCase` calls `remember_remote_snapshot_hash` + `set_next_origin` before/after write | Correct ordering is critical and easy to get wrong |
| File transfer pending linkage tracking   | Custom DB writes             | `FileTransferOrchestrator` (Phase 63 scope)                                                                | Out of scope for Phase 62                          |

**Key insight:** `SyncInboundClipboardUseCase` encapsulates a decade of clipboard sync edge cases. The daemon's inbound worker is a thin adapter that feeds messages into this use case, not a reimplementation.

## Common Pitfalls

### Pitfall 1: Separate clipboard_change_origin instances break loopback prevention

**What goes wrong:** If `InboundClipboardSyncWorker` creates its own `InMemoryClipboardChangeOrigin` instead of sharing the one from `DaemonClipboardChangeHandler`, the write-back loop prevention fails. The inbound worker registers the snapshot hash on its origin instance, but the clipboard watcher checks the change handler's origin instance — they never see each other's state.

**Why it happens:** Easy to construct fresh Arc<InMemoryClipboardChangeOrigin> without realizing it must be shared.

**How to avoid:** Pass `clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>` into `InboundClipboardSyncWorker::new()` as a constructor parameter. In `main.rs`, pass the SAME `clipboard_change_origin` that was given to `DaemonClipboardChangeHandler`.

**Warning signs:** Integration test where inbound clipboard triggers a second outbound sync (double-sync).

### Pitfall 2: Emitting WS event in Full mode for non-file content (double event)

**What goes wrong:** In Full mode, `SyncInboundClipboardUseCase` writes to OS clipboard and returns `Applied { entry_id: None }`. The `ClipboardWatcherWorker` then captures this change and emits its own `clipboard.new_content` WS event. If the inbound worker ALSO emits an event for `entry_id: None`, the GUI receives two events for the same clipboard change.

**Why it happens:** Naively emitting on every `Applied` outcome without checking whether capture happened.

**How to avoid:** Only emit WS event from inbound worker when `entry_id: Some(_)` (passive mode or file transfers). Mirror the condition in `wiring.rs` lines 656-667.

### Pitfall 3: Using ClipboardIntegrationMode::Passive for daemon

**What goes wrong:** Passive mode does NOT write to the OS clipboard. The daemon is the OS clipboard owner — it must write received content to the system clipboard so the user can paste it locally.

**Why it happens:** "Passive" sounds appropriate for "receiving" content. But Passive means "don't interact with OS clipboard at all."

**How to avoid:** Use `ClipboardIntegrationMode::Full` for the daemon's inbound use case. Passive is reserved for the GUI process (which no longer watches the clipboard since Phase 57).

### Pitfall 4: subscribe_clipboard() channel is single-consumer

**What goes wrong:** If two code paths call `subscribe_clipboard()`, only one gets the messages (the port contract says "adapters may expose this as a single-consumer stream").

**Why it happens:** Not obvious from the trait signature.

**How to avoid:** Only the `InboundClipboardSyncWorker` should call `subscribe_clipboard()`. No other daemon code path should call it.

### Pitfall 5: file_cache_dir not available without extracting from storage_paths

**What goes wrong:** `SyncInboundClipboardUseCase::with_capture_dependencies()` takes `file_cache_dir: Option<PathBuf>`. In the daemon, this comes from `ctx.storage_paths.file_cache_dir` (an `AppPaths` field). The builder context must be passed through to the worker construction.

**Why it happens:** `storage_paths` is passed to `build_non_gui_runtime_with_setup` but the file_cache_dir must also be captured separately for the inbound use case.

**How to avoid:** In `main.rs`, extract `ctx.storage_paths.file_cache_dir.clone()` before moving `ctx.storage_paths` into `build_non_gui_runtime_with_setup`. Pass it to `InboundClipboardSyncWorker::new()`.

## Code Examples

### InboundClipboardSyncWorker struct skeleton

```rust
// Source: pattern from clipboard_watcher.rs
pub struct InboundClipboardSyncWorker {
    runtime: Arc<CoreRuntime>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    file_cache_dir: Option<PathBuf>,
}

impl InboundClipboardSyncWorker {
    pub fn new(
        runtime: Arc<CoreRuntime>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
        file_cache_dir: Option<PathBuf>,
    ) -> Self { ... }

    fn build_sync_inbound_usecase(&self) -> SyncInboundClipboardUseCase {
        let deps = self.runtime.wiring_deps();
        SyncInboundClipboardUseCase::with_capture_dependencies(
            ClipboardIntegrationMode::Full,
            deps.clipboard.system_clipboard.clone(),
            self.clipboard_change_origin.clone(),
            deps.security.encryption_session.clone(),
            deps.security.encryption.clone(),
            deps.device.device_identity.clone(),
            Arc::new(TransferPayloadDecryptorAdapter),
            deps.clipboard.clipboard_entry_repo.clone(),
            deps.clipboard.clipboard_event_repo.clone(),
            deps.clipboard.representation_policy.clone(),
            deps.clipboard.representation_normalizer.clone(),
            deps.clipboard.representation_cache.clone(),
            deps.clipboard.spool_queue.clone(),
            self.file_cache_dir.clone(),
            deps.settings.clone(),
        )
    }
}
```

### Start loop (subscribe + receive)

```rust
// Source: pattern from wiring.rs run_clipboard_receive_loop + clipboard_receive task
#[async_trait]
impl DaemonService for InboundClipboardSyncWorker {
    fn name(&self) -> &str { "inbound-clipboard-sync" }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("inbound clipboard sync starting");
        let usecase = self.build_sync_inbound_usecase();
        let clipboard_network = self.runtime.wiring_deps().network_ports.clipboard.clone();

        loop {
            let subscribe_result = tokio::select! {
                _ = cancel.cancelled() => { return Ok(()); }
                result = clipboard_network.subscribe_clipboard() => result,
            };

            match subscribe_result {
                Ok(mut rx) => {
                    self.run_receive_loop(&mut rx, &usecase, &cancel).await;
                }
                Err(e) => {
                    warn!(error = %e, "inbound clipboard subscribe failed; retrying");
                    tokio::select! {
                        _ = cancel.cancelled() => return Ok(()),
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                }
            }
        }
    }
}
```

### WS event broadcast condition

```rust
// Source: wiring.rs lines 644-681 pattern
// Only emit WS event when entry_id is Some (passive mode or file transfers)
// In Full mode without file_transfers: entry_id is None; ClipboardWatcher fires instead
if let InboundApplyOutcome::Applied {
    entry_id: Some(ref entry_id),
    ref pending_transfers,
} = outcome
{
    let payload = ClipboardNewContentPayload {
        entry_id: entry_id.to_string(),
        preview: "Remote clipboard content".to_string(),
        origin: "remote".to_string(),
    };
    // broadcast to WS clients
    let _ = self.event_tx.send(build_ws_event(payload));
}
```

### Registration in main.rs

```rust
// In uc-daemon/src/main.rs — after existing workers
let inbound_clipboard_sync = Arc::new(InboundClipboardSyncWorker::new(
    runtime.clone(),
    event_tx.clone(),
    clipboard_change_origin.clone(), // SAME Arc as DaemonClipboardChangeHandler
    Some(ctx.storage_paths.file_cache_dir.clone()),
));

let services: Vec<Arc<dyn DaemonService>> = vec![
    Arc::clone(&clipboard_watcher) as Arc<dyn DaemonService>,
    Arc::clone(&inbound_clipboard_sync) as Arc<dyn DaemonService>,
    Arc::new(PeerDiscoveryWorker::new(...)) as Arc<dyn DaemonService>,
    Arc::clone(&pairing_host) as Arc<dyn DaemonService>,
    Arc::clone(&peer_monitor) as Arc<dyn DaemonService>,
];
```

## State of the Art

| Old Approach                                      | Current Approach                                        | When Changed          | Impact                                                                 |
| ------------------------------------------------- | ------------------------------------------------------- | --------------------- | ---------------------------------------------------------------------- |
| AppRuntime clipboard receive in uc-tauri          | Daemon-owned inbound sync worker                        | Phase 62 (this phase) | Daemon now handles both outbound (Phase 61) and inbound clipboard sync |
| wiring.rs has clipboard_receive TaskRegistry task | `InboundClipboardSyncWorker` DaemonService in uc-daemon | Phase 62              | Mirrors the uc-tauri pattern in the daemon                             |

**Deprecated/outdated:**

- None for Phase 62 scope — uc-tauri inbound sync stays until Phase 64 (Tauri sync retirement)

## Open Questions

1. **FileTransferOrchestrator for inbound file transfers (Phase 62 vs 63)**
   - What we know: `run_clipboard_receive_loop` in wiring.rs takes `file_transfer_orchestrator: Option<Arc<FileTransferOrchestrator>>` and uses it to persist pending transfer records and reconcile early completions
   - What's unclear: Phase 62 description says "receive peer clipboard and write to local system" — should file transfer linkage tracking be in scope?
   - Recommendation: Pass `file_transfer_orchestrator: None` in Phase 62 to keep scope minimal. Phase 63 adds file orchestration. The inbound worker API should accept `Option<Arc<FileTransferOrchestrator>>` to allow Phase 63 to inject it without restructuring.

2. **ServiceSnapshot registration for inbound-clipboard-sync**
   - What we know: `main.rs` creates `initial_statuses` Vec with hard-coded service names. Adding a new service requires adding its snapshot entry.
   - What's unclear: Should it match the service's `name()` exactly or use a different display name?
   - Recommendation: Add `DaemonServiceSnapshot { name: "inbound-clipboard-sync".to_string(), health: ServiceHealth::Healthy }` to `initial_statuses` in main.rs.

3. **Backoff strategy for subscribe failures**
   - What we know: wiring.rs uses exponential backoff (250ms initial, 30s max). PeerDiscoveryWorker uses no backoff (subscribe_events rarely fails).
   - What's unclear: How frequently does `subscribe_clipboard()` fail in practice?
   - Recommendation: Use a simple fixed backoff (1-2 seconds) for Phase 62 simplicity. The wiring.rs exponential pattern can be adopted if needed.

## Environment Availability

Step 2.6: SKIPPED (no external dependencies — Phase 62 adds Rust code only, no new CLI tools or services needed)

## Validation Architecture

`workflow.nyquist_validation` key is absent from `.planning/config.json` — treated as enabled.

### Test Framework

| Property           | Value                                                                     |
| ------------------ | ------------------------------------------------------------------------- |
| Framework          | cargo test (Rust unit tests in-module)                                    |
| Config file        | src-tauri/Cargo.toml workspace                                            |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon workers::inbound_clipboard_sync` |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon`                                 |

### Phase Requirements → Test Map

Phase 62 has no pre-mapped requirement IDs (TBD). Proposed requirements based on the phase goal:

| Req ID  | Behavior                                                                                              | Test Type | Automated Command                                                                | File Exists? |
| ------- | ----------------------------------------------------------------------------------------------------- | --------- | -------------------------------------------------------------------------------- | ------------ |
| PH62-01 | InboundClipboardSyncWorker subscribes to ClipboardTransportPort and calls SyncInboundClipboardUseCase | unit      | `cd src-tauri && cargo test -p uc-daemon workers::inbound_clipboard_sync::tests` | ❌ Wave 0    |
| PH62-02 | Applied outcome with entry_id emits clipboard.new_content WS event with origin="remote"               | unit      | same                                                                             | ❌ Wave 0    |
| PH62-03 | Applied outcome without entry_id (Full mode non-file) does NOT emit WS event                          | unit      | same                                                                             | ❌ Wave 0    |
| PH62-04 | Skipped outcome (echo prevention, dedup, encryption not ready) does not emit WS event                 | unit      | same                                                                             | ❌ Wave 0    |
| PH62-05 | shared clipboard_change_origin prevents write-back loop (structural test)                             | unit      | same                                                                             | ❌ Wave 0    |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon workers::inbound_clipboard_sync`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon`
- **Phase gate:** Full uc-daemon suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs` — covers PH62-01 through PH62-05
- [ ] Update `src-tauri/crates/uc-daemon/src/workers/mod.rs` — add `pub mod inbound_clipboard_sync`

_(No new test framework setup needed — cargo test already works for uc-daemon)_

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — DaemonClipboardChangeHandler pattern, ClipboardNewContentPayload, shared clipboard_change_origin Arc
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — run_clipboard_receive_loop, new_sync_inbound_clipboard_usecase, InboundApplyOutcome handling, WS event emission conditions
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` — SyncInboundClipboardUseCase full implementation, all modes, with_capture_dependencies API
- `src-tauri/crates/uc-daemon/src/main.rs` — composition root, existing service registration, clipboard_change_origin Arc creation
- `src-tauri/crates/uc-core/src/ports/clipboard_transport.rs` — ClipboardTransportPort::subscribe_clipboard() return type
- `src-tauri/crates/uc-core/src/ports/clipboard/clipboard_change_origin.rs` — ClipboardChangeOriginPort trait methods

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` — DaemonService trait implementation pattern
- `src-tauri/crates/uc-app/src/deps.rs` — NetworkPorts.clipboard field (ClipboardTransportPort access)
- `src-tauri/crates/uc-app/src/app_paths.rs` — AppPaths.file_cache_dir field

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all components are existing in-codebase, fully verified by reading source files
- Architecture: HIGH — direct mirroring of run_clipboard_receive_loop pattern in wiring.rs
- Pitfalls: HIGH — write-back loop prevention and double-event prevention are documented in source code comments and test assertions

**Research date:** 2026-03-25
**Valid until:** 2026-04-25 (stable internal codebase, no external dependencies)
