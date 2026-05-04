# Phase 60: Extract File Transfer Wiring Orchestration from uc-tauri to uc-app - Research

**Researched:** 2026-03-25
**Domain:** Rust crate extraction / hexagonal architecture refactor
**Confidence:** HIGH

## Summary

Phase 60 is a mechanical extraction: move `file_transfer_wiring.rs` (502 lines, zero Tauri dependencies) from `uc-tauri/src/bootstrap/` into a new `FileTransferOrchestrator` struct in `uc-app/src/usecases/file_sync/`. All 9 functions become methods on the orchestrator. Two internal types (`EarlyCompletionCache`, `FileTransferStatusPayload`) move with the functions as private module state. The orchestrator is assembled in `uc-bootstrap/src/assembly.rs` and passed to `wiring.rs` via `BackgroundRuntimeDeps`.

The key insight is that `file_transfer_wiring.rs` already has zero Tauri imports — it depends only on `std`, `serde`, `tracing`, `uc_core`, and `uc_app`. All necessary crates (`serde`, `tracing`, `tokio`) are already listed in `uc-app/Cargo.toml`. No new dependencies are required.

The established pattern for this type of extraction is: (1) create new struct file in uc-app, (2) update uc-bootstrap assembly.rs to construct it, (3) update BackgroundRuntimeDeps in assembly.rs, (4) update wiring.rs call sites to call orchestrator methods, (5) delete the source file in uc-tauri, (6) update all imports (no re-export stubs per D-08/D-09). This is identical to the patterns used in Phase 54 (D-10), Phase 58 (D-05), and the SetupOrchestrator/PairingOrchestrator assembly pattern from Phase 38.

**Primary recommendation:** Create `FileTransferOrchestrator` holding `Arc<TrackInboundTransfersUseCase>` + `Arc<dyn HostEventEmitterPort>` + `Arc<dyn ClockPort>`, converting all 9 standalone functions to methods, assembled in `uc-bootstrap/assembly.rs` once and passed via `BackgroundRuntimeDeps`.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Encapsulate as `FileTransferOrchestrator` struct holding `Arc<TrackInboundTransfersUseCase>` + `Arc<dyn HostEventEmitterPort>` (+ any other shared deps). All 9 functions become methods on this struct.
- **D-02:** Place in `uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs`, exported via `uc-app::usecases::file_sync`.
- **D-03:** `FileTransferStatusPayload` (serde DTO) stays in uc-app as an internal type of the orchestrator module. Not promoted to uc-core.
- **D-04:** `EarlyCompletionCache` stays in uc-app as an internal type of the orchestrator. Owned by `FileTransferOrchestrator` instance.
- **D-05:** `assembly.rs` constructs `FileTransferOrchestrator` instance (consistent with CoreRuntime/SetupOrchestrator assembly pattern).
- **D-06:** `FileTransferOrchestrator` passed to `wiring.rs` via `BackgroundRuntimeDeps` struct.
- **D-07:** `wiring.rs` calls orchestrator methods at existing integration points.
- **D-08:** Direct delete + update all imports. No re-export stubs.
- **D-09:** Delete `uc-tauri/src/bootstrap/file_transfer_wiring.rs` entirely after extraction.

### Claude's Discretion

- Exact `FileTransferOrchestrator` constructor signature and which deps it holds
- Whether `spawn_timeout_sweep` returns a `JoinHandle` or registers with `TaskRegistry`
- Internal module organization within the orchestrator file
- Test placement (move with source or keep in uc-tauri integration tests)
- Whether `BackgroundRuntimeDeps` needs a new field or reuses an existing pattern

### Deferred Ideas (OUT OF SCOPE)

- "修复 setup 配对确认提示缺失" — UI bug, unrelated. Belongs in a separate UI fix phase.
- Network event loop restructuring in wiring.rs
- Daemon file transfer integration
  </user_constraints>

## Standard Stack

### Core (all already available in uc-app/Cargo.toml)

| Library          | Purpose                                             | Status            |
| ---------------- | --------------------------------------------------- | ----------------- |
| `tokio` (full)   | Async runtime, spawn, watch channel, time::interval | Already in uc-app |
| `serde` (derive) | `FileTransferStatusPayload` serialization           | Already in uc-app |
| `tracing`        | Spans and structured logging                        | Already in uc-app |
| `std`            | Mutex, HashMap, Arc, Duration                       | Standard library  |

**No new dependencies required** — `uc-app/Cargo.toml` already has all needed crates.

### Key Types the Orchestrator Holds

| Type                                | From                                 | Role                                   |
| ----------------------------------- | ------------------------------------ | -------------------------------------- |
| `Arc<TrackInboundTransfersUseCase>` | `uc-app::usecases::file_sync`        | DB operations for status transitions   |
| `Arc<dyn HostEventEmitterPort>`     | `uc-core::ports::host_event_emitter` | Frontend event emission                |
| `Arc<dyn ClockPort>`                | `uc-core::ports`                     | Timestamp generation for timeout sweep |

The `ClockPort` is needed because `spawn_timeout_sweep` takes `Arc<dyn ClockPort>` and all time-dependent methods need `now_ms`. Holding it on the struct eliminates the need to pass it at each call site.

## Architecture Patterns

### Established Orchestrator Pattern

Every existing orchestrator in this codebase follows this shape:

```rust
// Source: uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs (target)
// Pattern reference: SetupOrchestrator, PairingOrchestrator in uc-app
pub struct FileTransferOrchestrator {
    tracker: Arc<TrackInboundTransfersUseCase>,
    emitter: Arc<dyn HostEventEmitterPort>,
    clock: Arc<dyn ClockPort>,
}

impl FileTransferOrchestrator {
    pub fn new(
        tracker: Arc<TrackInboundTransfersUseCase>,
        emitter: Arc<dyn HostEventEmitterPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self { tracker, emitter, clock }
    }
}
```

### Assembly Pattern (uc-bootstrap/assembly.rs)

`assembly.rs` has zero Tauri imports (verified). All orchestrators are constructed here once:

```rust
// Pattern from build_setup_orchestrator() in assembly.rs
pub fn build_file_transfer_orchestrator(
    deps: &AppDeps,
    emitter: Arc<dyn HostEventEmitterPort>,
) -> Arc<FileTransferOrchestrator> {
    let tracker = Arc::new(TrackInboundTransfersUseCase::new(
        deps.storage.file_transfer_repo.clone(),
    ));
    Arc::new(FileTransferOrchestrator::new(
        tracker,
        emitter,
        deps.system.clock.clone(),
    ))
}
```

### BackgroundRuntimeDeps Extension

`BackgroundRuntimeDeps` is defined in `uc-bootstrap/src/assembly.rs` (line 111-122). Add one field:

```rust
pub struct BackgroundRuntimeDeps {
    pub libp2p_network: Arc<Libp2pNetworkAdapter>,
    pub representation_cache: Arc<RepresentationCache>,
    // ... existing fields ...
    pub file_transfer_orchestrator: Arc<FileTransferOrchestrator>,  // NEW
}
```

### Method Migration Map

All 9 standalone functions from `file_transfer_wiring.rs` become methods on `FileTransferOrchestrator`:

| Standalone function                                                                  | Method signature                                                 | Notes                           |
| ------------------------------------------------------------------------------------ | ---------------------------------------------------------------- | ------------------------------- |
| `emit_pending_status(emitter, entry_id, transfers)`                                  | `&self.emit_pending_status(entry_id, transfers)`                 | emitter from self               |
| `handle_transfer_progress(tracker, emitter, transfer_id, direction, chunks, now_ms)` | `&self.handle_transfer_progress(transfer_id, direction, chunks)` | tracker/emitter/clock from self |
| `handle_transfer_completed(tracker, emitter, transfer_id, hash, now_ms, cache)`      | `&self.handle_transfer_completed(transfer_id, hash, cache)`      | tracker/emitter/clock from self |
| `handle_transfer_failed(tracker, emitter, transfer_id, reason, now_ms)`              | `&self.handle_transfer_failed(transfer_id, reason)`              | tracker/emitter/clock from self |
| `spawn_timeout_sweep(tracker, emitter, clock, cancel)`                               | `self.spawn_timeout_sweep(cancel)`                               | tracker/emitter/clock from self |
| `reconcile_on_startup(tracker, emitter, now_ms)`                                     | `&self.reconcile_on_startup()`                                   | tracker/emitter/clock from self |
| `cleanup_cached_path(path)`                                                          | keep as free `async fn` (private, no self deps)                  | no state needed                 |
| `EarlyCompletionCache`                                                               | internal type in orchestrator module                             | D-04                            |
| `FileTransferStatusPayload`                                                          | internal type in orchestrator module                             | D-03                            |

### Module Export Pattern

```rust
// uc-app/src/usecases/file_sync/mod.rs — add one line
pub mod file_transfer_orchestrator;
pub use file_transfer_orchestrator::FileTransferOrchestrator;
```

No other exports needed — `EarlyCompletionCache` is used by `wiring.rs`, so it must be `pub` from the orchestrator module but does NOT need to be exported from the top-level `uc-app` crate.

### wiring.rs Call Site Updates

Current pattern uses module-path calls like `super::file_transfer_wiring::handle_transfer_progress(tracker, emitter, ...)`. After extraction, these become orchestrator method calls:

```rust
// Before (wiring.rs)
super::file_transfer_wiring::handle_transfer_progress(
    transfer_tracker.as_ref(),
    event_emitter.as_ref(),
    &progress.transfer_id,
    progress.direction.clone(),
    progress.chunks_completed,
    now_ms,
).await;

// After (wiring.rs)
background.file_transfer_orchestrator
    .handle_transfer_progress(&progress.transfer_id, progress.direction.clone(), progress.chunks_completed)
    .await;
```

The `EarlyCompletionCache` is currently created in `start_background_tasks` inside wiring.rs. After the extraction, `FileTransferOrchestrator` OWNS the `EarlyCompletionCache` (D-04), so it is created once in the constructor, not passed as a parameter from wiring.rs.

## Don't Hand-Roll

| Problem                       | Don't Build                  | Use Instead                                                                     |
| ----------------------------- | ---------------------------- | ------------------------------------------------------------------------------- |
| Cancellation in timeout sweep | Custom boolean flag + unsafe | `tokio::sync::watch::Receiver<bool>` (already used)                             |
| Test mocks for ports          | Production test doubles      | `MockFileTransferRepo` already in `track_inbound_transfers.rs` (full mock impl) |
| TaskRegistry integration      | Manual JoinHandle tracking   | Use `registry.spawn()` OR keep the existing `watch::channel` pattern            |

## Runtime State Inventory

This is a pure code refactor with no rename. No runtime state is affected.

- **Stored data:** None — no database schema changes, no key changes
- **Live service config:** None — no service configuration changes
- **OS-registered state:** None
- **Secrets/env vars:** None
- **Build artifacts:** None — the crate name does not change

## Common Pitfalls

### Pitfall 1: EarlyCompletionCache Ownership Confusion

**What goes wrong:** Leaving `EarlyCompletionCache` as a separate `Arc` created in `wiring.rs`, requiring it to be passed to orchestrator methods.
**Why it happens:** The current code creates the cache in `start_background_tasks` and passes `Arc<EarlyCompletionCache>` to both the clipboard receive loop and the network event loop.
**How to avoid:** Per D-04, `FileTransferOrchestrator` OWNS `EarlyCompletionCache`. The orchestrator creates it internally (`EarlyCompletionCache::default()`) in `new()`. Methods that currently take `cache: Option<&EarlyCompletionCache>` become `&self` methods and access `self.early_completion_cache` directly.
**Warning signs:** If `EarlyCompletionCache` still appears in `wiring.rs` as a standalone variable, the ownership migration is incomplete.

### Pitfall 2: spawn_timeout_sweep Returns JoinHandle that Gets Dropped

**What goes wrong:** `spawn_timeout_sweep` currently returns `tokio::task::JoinHandle<()>`, but the call site in `wiring.rs` uses `std::mem::forget(cancel_tx)` to prevent the cancel sender from being dropped. If the method signature changes and the handle is dropped or the cancel logic is changed, the sweep stops silently.
**Why it happens:** The `forget` pattern is fragile — it prevents cancel_tx from sending the signal but is easily misread.
**How to avoid:** The planner must decide: keep the `watch::channel` cancel pattern (simplest, minimal change to existing behavior), OR integrate with `TaskRegistry` using `CancellationToken`. Either works; the `watch` pattern is lower risk. Document the choice in the plan.
**Warning signs:** Tests that verify timeout sweep termination would catch this.

### Pitfall 3: `now_ms` Passed at Call Sites vs. Computed in Method

**What goes wrong:** Some callers currently compute `let now_ms = clock.now_ms()` before calling wiring functions. If the method signature drops `now_ms` as a parameter (using `self.clock.now_ms()` internally), call sites must be updated to NOT compute `now_ms` and pass it.
**Why it happens:** The standalone function design required the caller to provide the clock value; the orchestrator pattern centralizes it.
**How to avoid:** Remove `now_ms` parameter from all method signatures that have `clock` on self. Verify all call sites in `wiring.rs` that previously computed `now_ms`.

### Pitfall 4: Missed Import Reference in wiring.rs Function Signatures

**What goes wrong:** `run_clipboard_receive_loop` and `register_pairing_background_tasks` in `wiring.rs` have `EarlyCompletionCache` in their function signatures (as `Arc<super::file_transfer_wiring::EarlyCompletionCache>`). These must be updated to `Arc<uc_app::usecases::file_sync::EarlyCompletionCache>` or removed entirely if the orchestrator owns the cache.
**Why it happens:** The cache is currently shared between two independent loops through an `Arc` in wiring.rs. After the orchestrator owns it, wiring.rs should pass `orchestrator.clone()` instead of the raw cache Arc.
**How to avoid:** Search for all occurrences of `file_transfer_wiring` and `EarlyCompletionCache` in `wiring.rs` — there are at least 4 usage sites.

### Pitfall 5: Test Module Migration

**What goes wrong:** The existing unit tests in `file_transfer_wiring.rs` (lines 396-501) test `FileTransferStatusPayload` serialization and `emit_pending_status`. These must be preserved but their location must be decided.
**Why it happens:** Tests reference `RecordingEmitter` and `PendingTransferLinkage` which are all local to the test module.
**How to avoid:** Move the tests into the new `file_transfer_orchestrator.rs` file. The `RecordingEmitter` in the existing tests uses only `HostEvent` and `HostEventEmitterPort` — both are already available to `uc-app`. The `PendingTransferLinkage` is from `uc_app::usecases::clipboard::sync_inbound` which is within the same crate.

## Code Examples

### Verified: Current Integration Points in wiring.rs

Integration point 1 — clipboard receive loop (`run_clipboard_receive_loop`, line 678):

```rust
// Before:
super::file_transfer_wiring::emit_pending_status(
    event_emitter.as_ref(),
    &entry_id.to_string(),
    pending_transfers,
);

// After:
background.file_transfer_orchestrator
    .emit_pending_status(&entry_id.to_string(), pending_transfers);
```

Integration point 2 — network event loop, TransferProgress (line 1213):

```rust
// Before:
super::file_transfer_wiring::handle_transfer_progress(
    transfer_tracker.as_ref(),
    event_emitter.as_ref(),
    &progress.transfer_id,
    progress.direction.clone(),
    progress.chunks_completed,
    now_ms,
).await;

// After (now_ms computed internally via self.clock):
background.file_transfer_orchestrator
    .handle_transfer_progress(&progress.transfer_id, progress.direction.clone(), progress.chunks_completed)
    .await;
```

Integration point 3 — network event loop, FileTransferCompleted (line 1280, 1312, 1359):

```rust
// These are inside tokio::spawn, so orchestrator must be cloned:
let orchestrator = background.file_transfer_orchestrator.clone();
// ...
orchestrator.handle_transfer_failed(...).await;
orchestrator.handle_transfer_completed(...).await;
```

Integration point 4 — startup reconciliation (line 417):

```rust
// Before (creates separate TrackInboundTransfersUseCase):
let tracker = uc_app::usecases::file_sync::TrackInboundTransfersUseCase::new(reconcile_repo);
super::file_transfer_wiring::reconcile_on_startup(&tracker, &*reconcile_emit, now_ms).await;

// After (orchestrator owns tracker):
background.file_transfer_orchestrator.reconcile_on_startup().await;
```

Integration point 5 — timeout sweep (line 436):

```rust
// Before:
let _sweep_handle = super::file_transfer_wiring::spawn_timeout_sweep(
    sweep_tracker, sweep_emitter, sweep_clock, cancel_rx,
);
std::mem::forget(cancel_tx);

// After:
let _sweep_handle = background.file_transfer_orchestrator
    .spawn_timeout_sweep(cancel_rx);
std::mem::forget(cancel_tx);
// OR: use TaskRegistry pattern
```

### Verified: assembly.rs is Already Zero-Tauri

```rust
// uc-tauri/src/bootstrap/assembly.rs (lines 1-3)
//! Re-exports from uc-bootstrap for backward compatibility.
pub use uc_bootstrap::assembly::*;
```

The real `assembly.rs` is in `uc-bootstrap/src/assembly.rs`. `FileTransferOrchestrator` construction goes there. `BackgroundRuntimeDeps` struct definition is also in `uc-bootstrap/src/assembly.rs` (line 111-122).

## State of the Art

| Old Approach                                         | Current Approach            | Impact for This Phase                                                                                                 |
| ---------------------------------------------------- | --------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Standalone free functions in bootstrap               | Orchestrator struct pattern | FileTransferOrchestrator follows orchestrator pattern                                                                 |
| Caller owns tracker creation                         | Orchestrator owns tracker   | Eliminate duplicate `TrackInboundTransfersUseCase::new()` calls in wiring.rs (currently 3 separate instances created) |
| `EarlyCompletionCache` passed as `Arc` between loops | Owned by orchestrator       | Single instance, no `Arc` threading between loops needed                                                              |

**Note on duplicate tracker construction:** Currently `wiring.rs` creates `TrackInboundTransfersUseCase` in 3 separate places:

1. Line 234: `clipboard_transfer_tracker` for clipboard receive loop
2. Line 413: inside `file_transfer_reconcile` spawn for startup reconciliation
3. Line 433: `sweep_tracker` for timeout sweep

After the extraction, the orchestrator holds ONE tracker instance. The `run_network_realtime_loop` in wiring.rs also creates its OWN tracker (line 1124). This fourth tracker should ALSO use the orchestrator's tracker, meaning the orchestrator should be passed into `register_pairing_background_tasks` and `run_network_realtime_loop` instead of the raw `file_transfer_repo`.

## Open Questions

1. **Should `spawn_timeout_sweep` use `TaskRegistry` or keep `watch::channel`?**
   - What we know: existing code uses `watch::channel(false)` + `std::mem::forget(cancel_tx)`; `TaskRegistry` pattern uses `CancellationToken` from `tokio_util`
   - What's unclear: whether the `forget` pattern was an intentional choice or a placeholder
   - Recommendation: Keep `watch::channel` for minimal diff; the `forget(cancel_tx)` means the sweep runs forever until process exit, which is the intended behavior. Alternatively, use `registry.spawn()` with `CancellationToken` for consistency with other tasks — this is the "Claude's Discretion" area.

2. **Should `run_network_realtime_loop` receive the orchestrator directly?**
   - What we know: this function creates its own `TrackInboundTransfersUseCase::new(file_transfer_repo)` at line 1124
   - What's unclear: whether the planner intends this fourth tracker to also be consolidated
   - Recommendation: Yes — pass `Arc<FileTransferOrchestrator>` to `run_network_realtime_loop` and remove the standalone tracker construction there. This eliminates all 4 duplicate tracker instances and makes the function signature simpler.

3. **EarlyCompletionCache visibility from wiring.rs after extraction?**
   - What we know: `wiring.rs` currently references `super::file_transfer_wiring::EarlyCompletionCache` in function signatures (e.g., `register_pairing_background_tasks`)
   - Recommendation: If orchestrator owns the cache internally, `wiring.rs` only needs to pass `Arc<FileTransferOrchestrator>` to the loops. The `EarlyCompletionCache` type name would not appear in `wiring.rs` at all after the refactor.

## Environment Availability

Step 2.6: SKIPPED — this phase is a pure code/module refactor with no external tool dependencies beyond the existing Rust/Cargo toolchain.

## Validation Architecture

### Test Framework

| Property           | Value                                                            |
| ------------------ | ---------------------------------------------------------------- |
| Framework          | Rust `cargo test` (built-in)                                     |
| Config file        | `src-tauri/Cargo.toml` workspace                                 |
| Quick run command  | `cd src-tauri && cargo test -p uc-app file_transfer`             |
| Full suite command | `cd src-tauri && cargo test -p uc-app && cargo test -p uc-tauri` |

### Phase Requirements → Test Map

| Behavior                                                           | Test Type | Automated Command                                                              | Notes                                            |
| ------------------------------------------------------------------ | --------- | ------------------------------------------------------------------------------ | ------------------------------------------------ |
| `FileTransferOrchestrator` struct created and exported from uc-app | unit      | `cd src-tauri && cargo test -p uc-app file_transfer`                           | Move existing tests from file_transfer_wiring.rs |
| `FileTransferStatusPayload` serializes camelCase                   | unit      | `cd src-tauri && cargo test -p uc-app file_transfer_status_payload_serializes` | Already exists, move location                    |
| `emit_pending_status` emits correct events                         | unit      | `cd src-tauri && cargo test -p uc-app emit_pending_status`                     | Already exists, move location                    |
| uc-tauri compiles after deletion of file_transfer_wiring.rs        | compile   | `cd src-tauri && cargo check -p uc-tauri`                                      | Phase gate                                       |
| uc-app compiles with new module                                    | compile   | `cd src-tauri && cargo check -p uc-app`                                        | Per-task                                         |

### Wave 0 Gaps

None — existing test infrastructure in `file_transfer_wiring.rs` covers the key behaviors. Tests move with the source code to the new location.

## Project Constraints (from CLAUDE.md)

- **Rust error handling:** Never use `unwrap()` or `expect()` in production code — use `?`, `match`, or `unwrap_or_else`. The existing `file_transfer_wiring.rs` code uses `unwrap_or_else(|e| e.into_inner())` in `EarlyCompletionCache` (mutex poison recovery) — this is acceptable.
- **Logging:** Use `tracing` crate spans and events, not `log::*`. All existing code uses `tracing::warn!`, `tracing::info!` — maintain this.
- **Architecture:** Port/Adapter pattern must be preserved. `FileTransferOrchestrator` must depend only on trait ports (`HostEventEmitterPort`, `ClockPort`), not concrete implementations.
- **Cargo commands:** MUST run from `src-tauri/`. Never from project root.
- **Error handling in event-driven code:** Use `match` not `if let` when errors should be reported (all existing `emit()` calls use `if let Err(err)` + `warn!` — maintain this pattern).
- **No re-export stubs:** Consistent with D-08/D-09 and prior phase decisions (Phase 54 D-10, Phase 58 D-05).

## Sources

### Primary (HIGH confidence)

- Direct code inspection of `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` (502 lines)
- Direct code inspection of `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` (all integration points)
- Direct code inspection of `src-tauri/crates/uc-bootstrap/src/assembly.rs` (BackgroundRuntimeDeps struct, build_setup_orchestrator pattern)
- Direct code inspection of `src-tauri/crates/uc-app/src/usecases/file_sync/mod.rs` and `track_inbound_transfers.rs`
- Direct code inspection of `src-tauri/crates/uc-app/Cargo.toml` (dependency verification)

### Secondary (MEDIUM confidence)

- Phase 38/54/58 CONTEXT.md decisions — confirmed "no re-export stubs" pattern via STATE.md accumulated context
- CLAUDE.md project constraints — confirmed tracing/error handling conventions

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all deps already present in uc-app/Cargo.toml, verified by direct file inspection
- Architecture: HIGH — orchestrator pattern is established and used in multiple places; direct code inspection of all integration points
- Pitfalls: HIGH — all pitfalls identified from direct inspection of actual call sites in wiring.rs

**Research date:** 2026-03-25
**Valid until:** 2026-04-25 (stable codebase, no external dependencies)
