# Phase 73: Refactor clipboard restore loop prevention - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning

<domain>
## Phase Boundary

Refactor the clipboard restore loop-prevention mechanism by introducing a single `ClipboardWriteCoordinator` service that owns all guard registration, key derivation, and clipboard write orchestration. Callers (restore, file copy, inbound sync) no longer manually manage `ClipboardChangeOriginPort` methods. The daemon entrypoint cannot accidentally construct a second origin store.

</domain>

<decisions>
## Implementation Decisions

### Coordinator API Design

- **D-01:** `ClipboardWriteCoordinator` provides a single `write(snapshot: SystemClipboardSnapshot, intent: ClipboardWriteIntent) -> Result<()>` method as the **only** entry point for all programmatic clipboard writes
- **D-02:** `ClipboardWriteIntent` enum has three variants: `LocalRestore`, `RemotePush`, `LocalCapture` (for test scenarios)
- **D-03:** Coordinator owns the entire write pipeline: compute `origin_guard_key()` → call `remember_local/remote_snapshot_hash()` → call `write_snapshot()` → consume guard on failure
- **D-04:** Coordinator TTL for guards: 2 seconds (matching the existing `Duration::from_secs(2)` in `RestoreClipboardSelectionUseCase`)

### Snapshot Building vs Write Pipeline

- **D-05:** `RestoreClipboardSelectionUseCase::build_snapshot()` **stays** in the use case with full business logic (query entry, select representation, read blob)
- **D-06:** `RestoreClipboardSelectionUseCase::execute()` delegates snapshot construction to `build_snapshot()`, then calls `coordinator.write(snapshot, LocalRestore)` instead of `restore_snapshot()` method
- **D-07:** `CopyFileToClipboardUseCase::execute()` delegates to `coordinator.write(snapshot, LocalRestore)` instead of `write_files_to_clipboard()` method
- **D-08:** `InboundClipboardSyncWorker` calls `coordinator.write(snapshot, RemotePush)` instead of manually calling `remember_remote_snapshot_hash` + `write_snapshot`

### Guard Key Single Source of Truth

- **D-09:** Coordinator is the **sole** caller of `snapshot.origin_guard_key()` — external callers never directly invoke `origin_guard_key()` anymore
- **D-10:** If future key derivation rules change, only `ClipboardWriteCoordinator::write()` needs updating (one location)

### Locking Down Composition

- **D-11:** `InMemoryClipboardChangeOrigin` is constructed exactly once in `uc-bootstrap/assembly.rs` (via `build_core()`) and exposed through `WiringDeps::clipboard.clipboard_change_origin`
- **D-12:** `ClipboardWriteCoordinator` is constructed in `uc-bootstrap/assembly.rs` and takes the shared `Arc<dyn ClipboardChangeOriginPort>` as a constructor dependency
- **D-13:** All clipboard **write-path** daemon workers (`InboundClipboardSyncWorker`, `FileSyncOrchestratorWorker`) receive `ClipboardWriteCoordinator` (not raw `ClipboardChangeOriginPort`); `DaemonClipboardChangeHandler` retains raw `Arc<dyn ClipboardChangeOriginPort>` as it is on the consume/read path, not the write path
- **D-14:** Direct construction of `InMemoryClipboardChangeOrigin` outside `uc-bootstrap/assembly.rs` is prevented — the struct remains `pub(crate)` in `uc-infra` and the only constructor is `pub fn new()` callable from bootstrap

### Coordinator Placement

- **D-15:** `ClipboardWriteCoordinator` lives in `uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs` following the `FileTransferOrchestrator` pattern
- **D-16:** Coordinator is accessible via `CoreUseCases::clipboard_write_coordinator()` accessor (alongside existing `restore_clipboard_selection()`, `copy_file_to_clipboard()`, etc.)

### Claude's Discretion

- Exact struct field layout of `ClipboardWriteCoordinator`
- Whether `restore_snapshot()` method on `RestoreClipboardSelectionUseCase` is deleted or kept as a delegating shim
- Whether `CopyFileToClipboardUseCase::write_files_to_clipboard()` is deleted or kept as a delegating shim
- Internal error types and logging details
- How `InboundClipboardSyncWorker` transitions from manual guard calls to coordinator calls (refactor in-place or rewrite)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Origin Guard Infrastructure

- `src-tauri/crates/uc-core/src/ports/clipboard/clipboard_change_origin.rs` — `ClipboardChangeOriginPort` trait (5 methods: `set_next_origin`, `consume_origin_or_default`, `has_pending_origin`, `remember_remote_snapshot_hash`, `remember_local_snapshot_hash`, `consume_origin_for_snapshot_or_default`)
- `src-tauri/crates/uc-infra/src/clipboard/change_origin.rs` — `InMemoryClipboardChangeOrigin` implementation (256-entry snapshot queue, TTL-based expiration)
- `src-tauri/crates/uc-core/src/clipboard/system.rs:230` — `SystemClipboardSnapshot::origin_guard_key()` method definition
- `src-tauri/crates/uc-core/src/clipboard/change.rs` — `ClipboardChangeOrigin` enum (LocalCapture, LocalRestore, RemotePush)

### Daemon Composition (Current Shared Arc Pattern)

- `src-tauri/crates/uc-daemon/src/entrypoint.rs:107-113` — `clipboard_change_origin` extracted from `runtime.wiring_deps().clipboard.clipboard_change_origin.clone()` and shared across all workers
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs:121-145` — `DaemonClipboardChangeHandler` receives `Arc<dyn ClipboardChangeOriginPort>` via constructor
- `src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs:61-69` — `InboundClipboardSyncWorker` receives `Arc<dyn ClipboardChangeOriginPort>` via constructor
- `src-tauri/crates/uc-daemon/src/workers/file_sync_orchestrator.rs:28-35` — `FileSyncOrchestratorWorker` receives `Arc<dyn ClipboardChangeOriginPort>` via constructor

### Current Guard-Managing Use Cases

- `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs:194-248` — `RestoreClipboardSelectionUseCase::restore_snapshot()` with `remember_local_snapshot_hash` + `write_snapshot` + error cleanup
- `src-tauri/crates/uc-app/src/usecases/file_sync/copy_file_to_clipboard.rs:162-190` — `CopyFileToClipboardUseCase::write_files_to_clipboard()` with identical pattern

### Daemon Restore Route (Phase 72)

- `src-tauri/crates/uc-daemon/src/api/routes.rs:104-157` — `restore_clipboard_entry_handler` calling `CoreUseCases::restore_clipboard_selection().execute()`

### Bootstrap Composition

- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — `build_non_gui_runtime()` and `build_non_gui_runtime_with_emitter()` showing `WiringDeps` construction
- `src-tauri/crates/uc-app/src/deps.rs` — `WiringDeps` struct showing `clipboard.clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>` field
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — `CoreUseCases` accessor pattern and existing use case builders

### Prior Phase Context (Already Applied)

- `.planning/phases/57-daemon-daemon-daemon-daemon/57-CONTEXT.md` — D-07/D-08: `spawn_blocking` + `WatcherShutdown` pattern, write-back loop prevention via `ClipboardChangeOriginPort`
- `.planning/phases/62-migrate-restore-clipboard-to-daemon-eliminate-cross-process-origin-desync/62-CONTEXT.md` — D-05: `InboundClipboardSyncWorker` accepts `Arc<dyn ClipboardChangeOriginPort>` via constructor, shared with `DaemonClipboardChangeHandler`
- `.planning/phases/72-migrate-restore-clipboard-to-daemon-eliminate-cross-process-origin-desync/72-CONTEXT.md` — Phase 72 moved restore to daemon, eliminated cross-process desync

</canonical_refs>

<codebase_context>

## Existing Code Insights

### Reusable Assets

- `InMemoryClipboardChangeOrigin` — already exists, well-tested with comprehensive unit tests
- `FileTransferOrchestrator` pattern — `uc-app` component with `Arc<Inner>` + constructor dependencies, accessible via `CoreUseCases` accessor, built in bootstrap. This is the direct template for `ClipboardWriteCoordinator`
- `CoreUseCases` accessor pattern — all use cases constructed in one place with `new(runtime.as_ref())`
- Existing unit tests in `change_origin.rs` — verify single-consumption, priority, TTL expiration

### Established Patterns

- Guard + write + cleanup: `remember_X_hash` → `write_snapshot` → `consume_on_error`
- 2-second TTL for snapshot guards (already standardized in `RestoreClipboardSelectionUseCase`)
- `Arc<dyn ClipboardChangeOriginPort>` shared across all daemon workers (Phase 62 established)
- Bootstrap composition: deps built in `assembly.rs`, passed to runtime, accessed via `wiring_deps()`
- Error cleanup: on `write_snapshot` failure, immediately `consume_origin_for_snapshot_or_default` to prevent stale guard

### Integration Points

- `CoreUseCases` needs a new `clipboard_write_coordinator()` accessor
- `uc-bootstrap/assembly.rs` needs `build_clipboard_write_coordinator()` function
- All 4 call sites need refactoring: `RestoreClipboardSelectionUseCase`, `CopyFileToClipboardUseCase`, `InboundClipboardSyncWorker`, `FileSyncOrchestratorWorker` (for file clipboard restore)
- `DaemonClipboardChangeHandler::on_clipboard_changed` — the READ side (consuming guards) stays unchanged; only the WRITE path through `InboundClipboardSyncWorker` changes

### Creative Options

- Coordinator could use `impl fmt::Display for SystemClipboardSnapshot` to derive the key, but `origin_guard_key()` already computes a hash-based key so that's not needed
- Coordinator could return metadata (entry_id written, guard TTL used) but return type should stay `Result<()>` for simplicity
- No changes needed to `SystemClipboardSnapshot::origin_guard_key()` — it already computes a content-meaningful key from representation data

</codebase_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches following established patterns from `FileTransferOrchestrator` and `ClipboardChangeOriginPort`.
</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

_Phase: 73-refactor-clipboard-restore-loop-prevention-introduce-clipboardwritecoordinator-as-single-write-boundary-owning-origin-guard-registration-derive-meaningful-content-key-and-remove-composition-time-re-creation-risk-of-origin-store_
_Context gathered: 2026-03-29_
