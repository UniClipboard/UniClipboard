# Phase 60: Extract file transfer wiring orchestration from uc-tauri to uc-app - Context

**Gathered:** 2026-03-25
**Status:** Ready for planning

<domain>
## Phase Boundary

Extract `file_transfer_wiring.rs` (502 lines, zero Tauri dependencies) from `uc-tauri/bootstrap/` into a new `FileTransferOrchestrator` struct in `uc-app`, making file transfer lifecycle management (progress tracking, completion handling, timeout sweep, startup reconciliation) available to non-Tauri runtimes (daemon, CLI).

**In scope:**

- Create `FileTransferOrchestrator` struct in `uc-app/src/usecases/file_sync/`
- Move all 9 functions from `file_transfer_wiring.rs` as methods on the orchestrator
- Move `EarlyCompletionCache` and `FileTransferStatusPayload` as internal types
- Assemble `FileTransferOrchestrator` in `assembly.rs`, pass via `BackgroundRuntimeDeps`
- Update `wiring.rs` to call orchestrator methods instead of standalone functions
- Delete `file_transfer_wiring.rs` from uc-tauri
- Update all import paths, no re-export stubs

**Explicitly NOT in scope:**

- Changes to `TrackInboundTransfersUseCase` (already in uc-app, correct location)
- Changes to `HostEventEmitterPort` or `TransferHostEvent` (already in uc-core)
- Changes to `TauriEventEmitter` / `LoggingEventEmitter` adapters
- Frontend changes
- File transfer business logic changes
- Network event loop restructuring in wiring.rs (only update call sites)

</domain>

<decisions>
## Implementation Decisions

### Module organization

- **D-01:** Encapsulate as `FileTransferOrchestrator` struct holding `Arc<TrackInboundTransfersUseCase>` + `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` (shared emitter cell, same pattern as `HostEventSetupPort`) + `Arc<dyn ClockPort>`. All 9 functions become methods on this struct. The emitter_cell pattern allows construction at wire time — the cell starts with `LoggingEventEmitter` and is hot-swapped to `TauriEventEmitter` after Tauri setup.
- **D-02:** Place in `uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs`, exported via `uc-app::usecases::file_sync`.

### Data structure ownership

- **D-03:** `FileTransferStatusPayload` (serde DTO) stays in uc-app as an internal type of the orchestrator module. Not promoted to uc-core — only consumed by the orchestrator for event emission.
- **D-04:** `EarlyCompletionCache` stays in uc-app as an internal type of the orchestrator. Owned by `FileTransferOrchestrator` instance.

### Assembly and integration

- **D-05:** `assembly.rs` constructs `FileTransferOrchestrator` instance (consistent with CoreRuntime/SetupOrchestrator assembly pattern).
- **D-06:** `FileTransferOrchestrator` passed to `wiring.rs` via `BackgroundRuntimeDeps` struct as direct `Arc<FileTransferOrchestrator>` (non-Optional). Constructed at `wire_dependencies` time using emitter_cell — no deferred construction needed.
- **D-07:** `wiring.rs` calls orchestrator methods at existing integration points (clipboard receive loop, network event loop, startup reconciliation, timeout sweep spawn).

### Migration strategy

- **D-08:** Direct delete + update all imports. No re-export stubs (consistent with Phase 54 D-10, Phase 58 D-05).
- **D-09:** Delete `uc-tauri/src/bootstrap/file_transfer_wiring.rs` entirely after extraction.

### Claude's Discretion

- Exact `FileTransferOrchestrator` constructor signature and which deps it holds
- Whether `spawn_timeout_sweep` returns a `JoinHandle` or registers with `TaskRegistry`
- Internal module organization within the orchestrator file
- Test placement (move with source or keep in uc-tauri integration tests)
- Whether `BackgroundRuntimeDeps` needs a new field or reuses an existing pattern

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Source file (to be extracted)

- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` — All 9 functions + EarlyCompletionCache + FileTransferStatusPayload (502 lines, zero Tauri deps)

### Integration points in wiring.rs

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — Calls to emit_pending_status, handle_transfer_progress, handle_transfer_completed, handle_transfer_failed, spawn_timeout_sweep, reconcile_on_startup, cleanup_cached_path

### Assembly

- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` — Composition root, where FileTransferOrchestrator should be constructed
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` §BackgroundRuntimeDeps — Struct that carries shared deps into background tasks

### Target location (uc-app file sync)

- `src-tauri/crates/uc-app/src/usecases/file_sync/mod.rs` — Existing file sync module exports
- `src-tauri/crates/uc-app/src/usecases/file_sync/track_inbound_transfers.rs` — TrackInboundTransfersUseCase (orchestrator will hold reference to this)

### Port definitions (no changes needed)

- `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` — HostEventEmitterPort + TransferHostEvent::StatusChanged
- `src-tauri/crates/uc-core/src/ports/file_transfer_repository.rs` — FileTransferRepositoryPort + TrackedFileTransferStatus

### Prior phase patterns

- `.planning/phases/54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri/54-CONTEXT.md` — Extraction pattern precedent (D-10 no stubs)
- `.planning/phases/58-extract-dto-models-and-pairing-event-types-from-uc-tauri-to-uc-app-and-uc-core/58-CONTEXT.md` — DTO extraction pattern (D-05 no stubs)
- `.planning/phases/38-coreruntime-extraction/38-CONTEXT.md` — Orchestrator assembly pattern precedent

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `TrackInboundTransfersUseCase` — already in uc-app, provides all DB operations the orchestrator needs
- `HostEventEmitterPort` — abstract event emission, orchestrator uses `TransferHostEvent::StatusChanged`
- `assembly.rs` composition root — established pattern for orchestrator construction
- `BackgroundRuntimeDeps` — existing struct for passing shared deps to background tasks

### Established Patterns

- Orchestrator struct pattern: `SetupOrchestrator`, `PairingOrchestrator` hold `Arc` deps and expose methods
- Assembly constructs orchestrators once, wiring consumes them
- Phase 37: assembly.rs has zero Tauri imports — FileTransferOrchestrator construction fits naturally

### Integration Points

- `wiring.rs` clipboard receive loop — calls `emit_pending_status()` after seeding pending records
- `wiring.rs` network event loop — calls `handle_transfer_progress/completed/failed()` on transfer events
- `wiring.rs` startup — calls `reconcile_on_startup()` once
- `wiring.rs` background tasks — calls `spawn_timeout_sweep()` for periodic cleanup
- `assembly.rs` — construct `FileTransferOrchestrator` and pass to `BackgroundRuntimeDeps`

### Key Insight

`file_transfer_wiring.rs` has **zero Tauri imports** — it only depends on `std`, `serde`, `tracing`, `uc_core`, and `uc_app`. This is a clean extraction with no API changes needed.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — standard architectural extraction following established patterns.

</specifics>

<deferred>
## Deferred Ideas

### Reviewed Todos (not folded)

- "修复 setup 配对确认提示缺失" — UI bug, unrelated to file transfer orchestration extraction. Belongs in a separate UI fix phase.

### Future considerations

- Network event loop restructuring in wiring.rs — currently mixes file transfer dispatch with other event handling; could be further decomposed in a future phase
- Daemon file transfer integration — once orchestrator is in uc-app, daemon can use it directly for file transfer lifecycle management

</deferred>

---

_Phase: 60-extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app_
_Context gathered: 2026-03-25_
