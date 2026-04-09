# Phase 64: Tauri Sync Retirement - Research

**Researched:** 2026-03-26
**Domain:** Rust / uc-tauri sync code removal, daemon delegation
**Confidence:** HIGH

## Summary

Phase 64 removes sync logic from the Tauri layer that is now fully owned by the daemon (Phases 57-63). After these prior phases, the daemon runs `ClipboardWatcherWorker`, `InboundClipboardSyncWorker`, `FileSyncOrchestratorWorker`, and `PeerDiscoveryWorker` — all four functions that `wiring.rs` also runs in parallel inside the GUI process.

The central question is: which Tauri sync code paths are now dead weight vs. still necessary for standalone (no-daemon) GUI mode? The current architecture uses `UC_CLIPBOARD_MODE` env var (default `Full`) to gate standalone vs. daemon-delegated behavior. Phase 64 must remove or gate each sync path precisely.

**Primary recommendation:** Remove the three duplicate background loops from `wiring.rs` that mirror daemon workers (`clipboard_receive`, `pairing_events`/`run_network_realtime_loop`). Retain `on_clipboard_changed` in `AppRuntime` for standalone GUI mode. Gate or remove the two Tauri commands (`sync_clipboard_items`, `restore_clipboard_entry` sync branch) that call local sync use cases.

## Project Constraints (from CLAUDE.md)

- All Rust commands MUST run from `src-tauri/` directory — never from project root
- Use `tracing::*` (not `log::*`) for structured logging
- Never use `unwrap()` / `expect()` in production code — explicit error handling only
- Match arms must use explicit error paths, not silent `if let` for failure cases
- Port/Adapter pattern: all external dependencies accessed through trait ports
- Hexagonal architecture enforced via crate boundaries
- No business logic in command handlers — use `runtime.usecases().xxx()` pattern

## Standard Stack

No new library dependencies are expected for this phase. This is a code removal phase.

| Crate              | Role                    | Status After Phase 64                      |
| ------------------ | ----------------------- | ------------------------------------------ |
| `uc-tauri`         | Tauri adapter layer     | Thinner — sync loops removed               |
| `uc-daemon`        | Daemon runtime          | Unchanged — owns all sync workers          |
| `uc-daemon-client` | Daemon WS/HTTP client   | Unchanged                                  |
| `uc-app`           | Use cases               | Unchanged                                  |
| `uc-infra`         | Infrastructure adapters | Possibly removable from uc-tauri prod deps |

**Dependencies potentially removable from `uc-tauri/Cargo.toml` after cleanup:**

- `blake3 = "1"` — only used in `runtime.rs` `on_clipboard_changed` file hash computation. If that path is removed, this dep goes too. Verify before removing.
- `tokio-tungstenite`, `futures-util`, `reqwest` — noted in Phase 54 decisions as retained because `run.rs` uses `reqwest::Client` directly. Still needed; do NOT remove.

## Architecture Patterns

### What the Daemon Now Owns (Phases 57-63)

| Daemon Worker                | What It Does                                                                                                     | Tauri Equivalent                                                                            |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `ClipboardWatcherWorker`     | Captures OS clipboard changes via `CaptureClipboardUseCase`, emits `clipboard.new_content` WS                    | `AppRuntime::on_clipboard_changed` + outbound sync                                          |
| `InboundClipboardSyncWorker` | Subscribes `ClipboardTransportPort`, runs `SyncInboundClipboardUseCase`, seeds pending transfers                 | `wiring.rs::run_clipboard_receive_loop` + clipboard_receive task                            |
| `FileSyncOrchestratorWorker` | Handles `TransferProgress`, `FileTransferCompleted`, `FileTransferFailed`, startup reconciliation, timeout sweep | `wiring.rs::run_network_realtime_loop` (file transfer portion)                              |
| `PeerDiscoveryWorker`        | Subscribes network events, announces device name on `PeerDiscovered`                                             | `wiring.rs::run_network_realtime_loop` (peer portion + `register_pairing_background_tasks`) |
| `PeerMonitor`                | Emits `peers.changed` WS events for `PeerDiscovered/Lost/Connected/Disconnected/NameUpdated`                     | `wiring.rs::run_network_realtime_loop` (peer connection events)                             |

### Tauri Sync Code That Still Exists (Target of Phase 64)

#### 1. `wiring.rs::start_background_tasks` — Duplicate Sync Loops

```
start_background_tasks()
├── spool scanner + spooler + blob worker + spool janitor     ← KEEP (GUI-owned storage workers)
├── register_pairing_background_tasks()                        ← REMOVE (daemon owns this)
│   └── "pairing_events" task → run_network_realtime_loop()
│       ├── PeerDiscovered → announce device name              ← daemon PeerDiscoveryWorker
│       ├── PeerReady/NotReady → emit PeerConnection events    ← daemon PeerMonitor
│       ├── PeerConnected/Disconnected → emit events           ← daemon PeerMonitor
│       ├── PeerNameUpdated → emit event                       ← daemon PeerMonitor
│       ├── TransferProgress → orchestrator + emit             ← daemon FileSyncOrchestratorWorker
│       ├── FileTransferCompleted → inbound file sync          ← daemon FileSyncOrchestratorWorker
│       └── FileTransferFailed → mark failed                   ← daemon FileSyncOrchestratorWorker
├── "clipboard_receive" task → run_clipboard_receive_loop()   ← REMOVE (daemon owns this)
│   └── SyncInboundClipboardUseCase                           ← daemon InboundClipboardSyncWorker
├── start_realtime_runtime()                                   ← KEEP (DaemonWsBridge)
├── file_cache_cleanup                                         ← KEEP (GUI cleanup)
├── "file_transfer_reconcile" fire-and-forget                  ← REMOVE (daemon owns this)
└── "file_transfer timeout sweep"                              ← REMOVE (daemon owns this)
```

**IMPORTANT CAVEAT:** The `clipboard_receive` loop and `run_network_realtime_loop` are only active when GUI is in `Full` mode (i.e., no daemon running). When GUI is in `Passive` mode (daemon running), the daemon's workers own these responsibilities. Phase 64 must handle this correctly — either:

- Option A: Remove these loops entirely (assume daemon always present)
- Option B: Gate them on `ClipboardIntegrationMode::Full` (keep for standalone mode)

Research finding: The `UC_CLIPBOARD_MODE` env var controls mode. Daemon-paired GUI sets this to `passive`. The `clipboard_receive` loop uses `ClipboardTransportPort` — the same network transport the daemon uses. Running BOTH would cause double-processing. This confirms: **these loops must be removed entirely when daemon is running**, not just gated.

Given the project's architectural direction (daemon is the sole clipboard observer), Phase 64 should remove the duplicate loops from wiring.rs. The standalone GUI mode (without daemon) loses these capabilities — but since the project is moving toward daemon-mandatory, that's acceptable.

#### 2. `runtime.rs::on_clipboard_changed` — Duplicate Outbound Sync

`AppRuntime::on_clipboard_changed` runs when the platform clipboard watcher fires. It:

- Calls `CaptureClipboardUseCase`
- Calls `OutboundSyncPlanner` + `SyncOutboundClipboardUseCase`
- Calls `SyncOutboundFileUseCase`

The daemon's `DaemonClipboardChangeHandler` does the exact same thing. In `Passive` mode, `StartClipboardWatcher` is a no-op so `on_clipboard_changed` never fires. In `Full` mode, both the GUI and daemon would compete.

**Resolution:** Since GUI is `Passive` when daemon runs (via `UC_CLIPBOARD_MODE=passive`), `on_clipboard_changed` only fires in standalone mode. Phase 64 should likely NOT remove this function — it's the standalone GUI clipboard path. But the outbound sync portion can be reviewed.

#### 3. Tauri Commands — Direct Sync Use Case Calls

**`sync_clipboard_items` command (clipboard.rs:477-527):**

- Already has a guard: returns `ValidationError` if mode is `Passive`
- In `Passive` mode it errors out — so in daemon-paired mode it already does nothing harmful
- In `Full` mode it directly calls `SyncOutboundClipboardUseCase::execute_current_snapshot`
- **Decision needed:** Should this command delegate to daemon HTTP endpoint instead? Or remain as local use case for standalone mode?

**`restore_clipboard_entry` command (clipboard.rs:529-620):**

- After restoring to OS clipboard, calls `SyncOutboundClipboardUseCase::execute()` to sync the restored entry
- No mode guard — runs in both Full and Passive modes
- In `Passive` mode, the clipboard write triggers `DaemonClipboardChangeHandler` which then does outbound sync — so the Tauri-level sync is redundant and may cause double-sync
- **Should be guarded on `Full` mode or removed**

#### 4. `AppUseCases::sync_inbound_clipboard` and `sync_outbound_clipboard` accessors

These exist in `runtime.rs` as AppUseCases methods. They are only called by:

- `sync_clipboard_items` command → `sync_outbound_clipboard`
- `restore_clipboard_entry` command → `sync_outbound_clipboard`
- `new_sync_inbound_clipboard_usecase` in wiring.rs → used by `clipboard_receive` loop

If the clipboard_receive loop is removed, `sync_inbound_clipboard` becomes unused. If both command sync calls are removed/gated, `sync_outbound_clipboard` can be removed from `AppUseCases`.

#### 5. `new_sync_inbound_clipboard_usecase` function in wiring.rs

Private helper only called by `start_background_tasks` to build the `SyncInboundClipboardUseCase` for `clipboard_receive`. If clipboard_receive is removed, this function is dead code.

#### 6. Constants and backoff helpers in wiring.rs

```rust
CLIPBOARD_SUBSCRIBE_BACKOFF_INITIAL_MS
CLIPBOARD_SUBSCRIBE_BACKOFF_MAX_MS
NETWORK_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS
NETWORK_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS
fn subscribe_backoff_ms()
fn network_events_subscribe_backoff_ms()
```

These are only used by `clipboard_receive` and `register_pairing_background_tasks` tasks. If both are removed, all these constants and helpers are dead code.

#### 7. Imports in wiring.rs

```rust
use uc_app::usecases::clipboard::sync_inbound::{InboundApplyOutcome, SyncInboundClipboardUseCase};
use uc_core::network::ClipboardMessage;
use uc_core::network::NetworkEvent;
use uc_core::ports::clipboard::ClipboardChangeOriginPort;
```

These imports become unused after removals.

### Architecture Pattern: Removal Strategy

```
Phase 64 scope:
  wiring.rs:
    REMOVE: register_pairing_background_tasks() function and call
    REMOVE: run_clipboard_receive_loop() function and "clipboard_receive" spawn
    REMOVE: run_network_realtime_loop() function
    REMOVE: new_sync_inbound_clipboard_usecase() function
    REMOVE: CLIPBOARD_SUBSCRIBE_BACKOFF_*, NETWORK_EVENTS_SUBSCRIBE_BACKOFF_*
    REMOVE: subscribe_backoff_ms(), network_events_subscribe_backoff_ms()
    REMOVE: "file_transfer_reconcile" task (daemon owns startup reconciliation)
    REMOVE: "file_transfer_timeout_sweep" task (daemon owns timeout sweeps)
    REMOVE: file_transfer_orchestrator from start_background_tasks params
             (only needed for clipboard_receive and pairing_events tasks)
    KEEP:   spool scanner, spooler, blob worker, spool janitor
    KEEP:   start_realtime_runtime (DaemonWsBridge)
    KEEP:   file_cache_cleanup task
    POSSIBLY SIMPLIFY: BackgroundRuntimeDeps.file_transfer_orchestrator field removal
                       if it's only used by the removed tasks

  runtime.rs:
    KEEP:   on_clipboard_changed (standalone GUI mode path)
    GATE:   sync calls in restore_clipboard_entry command (only in Full mode)
    REVIEW: sync_clipboard_items command — may need to delegate to daemon

  AppUseCases:
    REMOVE: sync_inbound_clipboard() if clipboard_receive loop is gone
    EVALUATE: sync_outbound_clipboard() — keep if restore_clipboard_entry still needs it

  BackgroundRuntimeDeps (uc-bootstrap):
    EVALUATE: Remove file_transfer_orchestrator field if unused after wiring.rs cleanup
```

### Anti-Patterns to Avoid

- **Removing standalone-mode paths prematurely:** The `on_clipboard_changed` in AppRuntime handles the case when GUI runs without a daemon. Don't remove it.
- **Double-sync in Passive mode:** `restore_clipboard_entry` currently calls outbound sync unconditionally. This causes double-sync when daemon is running (daemon's ClipboardWatcher fires after OS write). Must gate on Full mode.
- **Silent failures:** When removing sync code, ensure test coverage verifies the code paths are actually dead, not just seemingly unused.

## Don't Hand-Roll

| Problem                 | Don't Build                  | Use Instead                                            |
| ----------------------- | ---------------------------- | ------------------------------------------------------ |
| Inbound clipboard sync  | Custom receive loop in Tauri | Daemon `InboundClipboardSyncWorker` (already exists)   |
| Outbound clipboard sync | Custom sync trigger in Tauri | Daemon `DaemonClipboardChangeHandler` (already exists) |
| File transfer lifecycle | Custom network event handler | Daemon `FileSyncOrchestratorWorker` (already exists)   |
| Peer discovery          | Custom event subscription    | Daemon `PeerDiscoveryWorker` (already exists)          |

## Common Pitfalls

### Pitfall 1: Removing wiring.rs Tasks While Daemon Is Not Mandatory

**What goes wrong:** After removing clipboard_receive loop, standalone GUI (no daemon) loses inbound clipboard sync entirely.
**Why it happens:** The removal assumes daemon is always present.
**How to avoid:** Document the standalone-mode impact explicitly. The project is moving toward daemon-mandatory architecture, so this regression in standalone mode is acceptable as a deliberate trade-off.
**Warning signs:** Tests that spin up a standalone AppRuntime and test inbound sync will fail — update those tests.

### Pitfall 2: Double Outbound Sync from restore_clipboard_entry

**What goes wrong:** In Passive mode, `restore_clipboard_entry` writes to OS clipboard, which triggers daemon's `ClipboardWatcherWorker` to capture and run outbound sync. The command ALSO calls `sync_outbound_clipboard` explicitly — double sync.
**Why it happens:** The guard on `sync_clipboard_items` was added but not on `restore_clipboard_entry`.
**How to avoid:** Gate the `sync_outbound_clipboard` call in `restore_clipboard_entry` on `Full` mode check.
**Warning signs:** On a paired setup, restoring a clipboard entry sends the content twice to peers.

### Pitfall 3: file_transfer_orchestrator Still Needed in wiring.rs

**What goes wrong:** Removing file_transfer_orchestrator from `start_background_tasks` params breaks callers in `main.rs`.
**Why it happens:** The orchestrator is in `BackgroundRuntimeDeps` and wired at the composition root.
**How to avoid:** Only remove from `start_background_tasks` local bindings, not from `BackgroundRuntimeDeps` itself (other places may still use it). Check all callers before removing from the struct.
**Warning signs:** Compile error in `main.rs` or `uc-bootstrap`.

### Pitfall 4: Test Doubles Still Reference Removed Types

**What goes wrong:** Integration tests in `tests/bootstrap_integration_test.rs` or `tests/integration_clipboard_capture.rs` may reference `new_sync_inbound_clipboard_usecase` or expect clipboard_receive behavior.
**Why it happens:** Tests were written when wiring.rs owned these loops.
**How to avoid:** Grep all test files for references to removed functions before deleting.
**Warning signs:** Compile errors in test files.

### Pitfall 5: restore_file_to_clipboard_after_transfer Becomes Dead Code

**What goes wrong:** `restore_file_to_clipboard_after_transfer` in wiring.rs is only called from `run_network_realtime_loop`. Removing that loop makes this function dead code.
**Why it happens:** It's a helper for the network event loop.
**How to avoid:** Delete along with its caller.
**Warning signs:** `#[allow(dead_code)]` needed or compiler warning.

## Code Examples

### Pattern: Checking ClipboardIntegrationMode in Commands

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/clipboard.rs (existing pattern)
if matches!(
    runtime.clipboard_integration_mode(),
    ClipboardIntegrationMode::Passive
) {
    return Err(CommandError::ValidationError("...".to_string()));
}
```

Apply the same guard to `restore_clipboard_entry`'s outbound sync call.

### Pattern: Removing wiring.rs Tasks

```rust
// BEFORE — in start_background_tasks
register_pairing_background_tasks(
    &registry,
    pairing_events,
    peer_directory,
    event_emitter.clone(),
    ...
).await;

let clipboard_receive_orchestrator = file_transfer_orchestrator.clone();
registry.spawn("clipboard_receive", |token| { ... }).await;

// AFTER — both blocks deleted
// No clipboard_receive task; daemon InboundClipboardSyncWorker owns this.
// No pairing_events task; daemon PeerDiscoveryWorker + PeerMonitor own this.
```

### Pattern: Verifying daemon owns a task

```rust
// Confirm the daemon registers the equivalent worker in main.rs:
// src-tauri/crates/uc-daemon/src/main.rs
let inbound_clipboard_sync = Arc::new(InboundClipboardSyncWorker::new(...));
let file_sync_orchestrator_worker = Arc::new(FileSyncOrchestratorWorker::new(...));
let services = vec![
    Arc::clone(&clipboard_watcher) as Arc<dyn DaemonService>,
    Arc::clone(&inbound_clipboard_sync) as Arc<dyn DaemonService>,
    Arc::clone(&file_sync_orchestrator_worker) as Arc<dyn DaemonService>,
    ...
];
```

## State of the Art

| Old Approach                                  | Current Approach                                 | When Changed | Impact                                                                |
| --------------------------------------------- | ------------------------------------------------ | ------------ | --------------------------------------------------------------------- |
| GUI owns clipboard watcher                    | Daemon owns clipboard watcher (Phase 57)         | 2026-03-25   | GUI in Passive mode, no clipboard capture                             |
| GUI runs inbound sync loop                    | Daemon `InboundClipboardSyncWorker` (Phase 62)   | 2026-03-25   | Tauri clipboard_receive loop is redundant                             |
| GUI runs file transfer lifecycle              | Daemon `FileSyncOrchestratorWorker` (Phase 63)   | 2026-03-26   | Tauri pairing_events/network loop is redundant                        |
| GUI runs outbound sync on capture             | Daemon `DaemonClipboardChangeHandler` (Phase 61) | 2026-03-25   | Tauri on_clipboard_changed outbound sync is redundant in Passive mode |
| GUI had `UC_CLIPBOARD_MODE` hardcoded Passive | Reverted to env-var (fix 91e2e49b)               | 2026-03-25   | Standalone GUI still works without daemon                             |

**Deprecated/outdated in uc-tauri after Phase 63:**

- `run_clipboard_receive_loop()`: superseded by `InboundClipboardSyncWorker`
- `run_network_realtime_loop()` (file transfer portion): superseded by `FileSyncOrchestratorWorker`
- `run_network_realtime_loop()` (peer portion): superseded by `PeerMonitor` + `PeerDiscoveryWorker`
- `register_pairing_background_tasks()`: superseded by `PeerDiscoveryWorker`
- `new_sync_inbound_clipboard_usecase()`: private helper for removed loops

## Environment Availability

Step 2.6: SKIPPED (no new external dependencies — this is a code removal phase)

## Validation Architecture

### Test Framework

| Property           | Value                                    |
| ------------------ | ---------------------------------------- |
| Framework          | cargo test (Rust built-in)               |
| Config file        | src-tauri/Cargo.toml workspace           |
| Quick run command  | `cd src-tauri && cargo test -p uc-tauri` |
| Full suite command | `cd src-tauri && cargo test`             |

### Phase Requirements → Test Map

| ID      | Behavior                                                       | Test Type   | Automated Command                                                                         | File Exists? |
| ------- | -------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------- | ------------ |
| PH64-01 | clipboard_receive task removed from wiring.rs                  | unit        | `cd src-tauri && cargo test -p uc-tauri`                                                  | ✅ existing  |
| PH64-02 | pairing_events/network_realtime task removed from wiring.rs    | unit        | `cd src-tauri && cargo test -p uc-tauri`                                                  | ✅ existing  |
| PH64-03 | file_transfer_reconcile + timeout sweep removed from wiring.rs | unit        | `cd src-tauri && cargo test -p uc-tauri`                                                  | ✅ existing  |
| PH64-04 | restore_clipboard_entry outbound sync gated on Full mode       | unit        | `cd src-tauri && cargo test -p uc-tauri tests/clipboard_commands_stats_favorites_test.rs` | ✅ existing  |
| PH64-05 | Dead code helpers removed from wiring.rs                       | compile     | `cd src-tauri && cargo check -p uc-tauri`                                                 | ✅ inherent  |
| PH64-06 | Full uc-tauri test suite passes                                | integration | `cd src-tauri && cargo test -p uc-tauri`                                                  | ✅ existing  |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-tauri`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

None — existing test infrastructure covers all phase requirements. No new test files required for a removal phase; existing tests will catch any regressions via compile errors or behavior changes.

## Open Questions

1. **Should `BackgroundRuntimeDeps.file_transfer_orchestrator` be removed?**
   - What we know: It's used in wiring.rs only for the tasks being removed (clipboard_receive and file_transfer_reconcile/sweep)
   - What's unclear: Whether any other consumers in uc-tauri or uc-bootstrap reference this field
   - Recommendation: Grep for all usages before deciding; if uc-tauri is the only consumer and all uses are removed, drop the field. If it remains in BackgroundRuntimeDeps, keep it but unused in uc-tauri (scope creep risk).

2. **Should `sync_clipboard_items` Tauri command delegate to daemon HTTP?**
   - What we know: It directly calls `SyncOutboundClipboardUseCase` and is guarded on Passive mode
   - What's unclear: Whether the frontend ever calls this command and whether daemon has an equivalent endpoint
   - Recommendation: Scope this to a separate phase (47/48 daemon cutover). For Phase 64, the existing Passive-mode guard is sufficient.

3. **Are the `SpaceAccessBusyPayload` test helpers in wiring.rs still needed?**
   - What we know: They are `#[cfg(test)]` only and reference `SpaceAccessOrchestrator` which is now daemon-owned
   - What's unclear: Whether any test still uses them
   - Recommendation: Grep for callers. If no callers, remove as part of Phase 64 cleanup.

## Sources

### Primary (HIGH confidence)

- Direct code reading: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — full 1378-line file reviewed
- Direct code reading: `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — AppRuntime::on_clipboard_changed reviewed
- Direct code reading: `src-tauri/crates/uc-daemon/src/main.rs` — daemon service composition root reviewed
- Direct code reading: `src-tauri/crates/uc-daemon/src/workers/` — all 5 worker files reviewed
- Git commit history: Phase 57/61/62/63 commit messages confirm what daemon owns
- `.planning/STATE.md` — decision log reviewed

### Secondary (MEDIUM confidence)

- `.planning/REQUIREMENTS.md` — PH57-PH63 requirements confirm daemon ownership
- `.planning/ROADMAP.md` — Phase 64 goal description

## Metadata

**Confidence breakdown:**

- What to remove: HIGH — code reading confirms exact functions and their daemon equivalents
- Side effects: HIGH — env-var-based mode gating is clear
- Dependency cleanup: MEDIUM — blake3 usage needs verification before removal

**Research date:** 2026-03-26
**Valid until:** 2026-04-09 (stable codebase, 14-day window)
