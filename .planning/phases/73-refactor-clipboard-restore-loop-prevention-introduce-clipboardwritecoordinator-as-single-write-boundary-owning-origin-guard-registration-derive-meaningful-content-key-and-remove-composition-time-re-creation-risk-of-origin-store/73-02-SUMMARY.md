---
phase: 73-refactor-clipboard-restore-loop-prevention
plan: 02
subsystem: clipboard
tags: [clipboard, coordinator, refactor, origin-guard, daemon, workers]
dependency_graph:
  requires: [ClipboardWriteCoordinator, ClipboardWriteIntent, new_clipboard_change_origin]
  provides: [all-callsites-migrated, dead-guard-code-removed]
  affects: [uc-app, uc-daemon]
tech_stack:
  added: []
  patterns: [coordinator-pattern, builder-pattern]
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs
    - src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs
    - src-tauri/crates/uc-daemon/src/workers/file_sync_orchestrator.rs
    - src-tauri/crates/uc-daemon/src/entrypoint.rs
    - src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs
decisions:
  - 'SyncInboundClipboardUseCase keeps legacy local_clipboard/clipboard_change_origin constructor params (with #[allow(dead_code)]) for e2e test compatibility; coordinator wired via with_clipboard_write_coordinator() builder'
  - 'REMOTE_SNAPSHOT_HASH_TTL_MS constant removed from sync_inbound.rs — TTL semantics now owned exclusively by ClipboardWriteCoordinator'
  - 'InboundClipboardSyncWorker swaps clipboard_change_origin field for clipboard_write_coordinator to align constructor with coordinator-first design'
  - 'FileSyncOrchestratorWorker consolidates system_clipboard + clipboard_change_origin into single clipboard_write_coordinator field'
  - 'has_pending_origin() added to ClipboardWriteCoordinator as a non-destructive peek for concurrent-write detection in FileSyncOrchestratorWorker'
metrics:
  duration_minutes: 90
  completed_date: '2026-03-29'
  tasks_completed: 2
  files_modified: 6
---

# Phase 73 Plan 02: Migrate All Clipboard Write Callsites to Coordinator Summary

**One-liner:** Routed all four programmatic OS clipboard write paths through ClipboardWriteCoordinator, removing duplicated guard-registration logic from SyncInboundClipboardUseCase, InboundClipboardSyncWorker, and FileSyncOrchestratorWorker.

## What Was Built

### Task 1: RestoreClipboardSelectionUseCase and CopyFileToClipboardUseCase (committed a3d11526)

Both use cases refactored to use `ClipboardWriteCoordinator`:

- `RestoreClipboardSelectionUseCase`: removed `local_clipboard` and `clipboard_change_origin` fields; added `coordinator: Arc<ClipboardWriteCoordinator>`; deleted `restore_snapshot()` helper; `execute()` now calls `coordinator.write(snapshot, ClipboardWriteIntent::LocalRestore)`
- `CopyFileToClipboardUseCase`: same pattern — removed two fields, added coordinator; `write_files_to_clipboard()` calls `coordinator.write(snapshot, ClipboardWriteIntent::LocalRestore)`
- `CoreUseCases::restore_clipboard_selection()` and `copy_file_to_clipboard()` return `anyhow::Result<T>` via `ok_or_else` when coordinator is absent
- Updated callsites: `uc-daemon/src/api/routes.rs` (handles `Err` from restore), `uc-tauri/src/commands/clipboard.rs` (`.map_err(|e| e.to_string())`)

### Task 2: SyncInboundClipboardUseCase, Daemon Workers, and E2E Tests (committed 069831e5)

**SyncInboundClipboardUseCase (`sync_inbound.rs`):**

- Removed `REMOTE_SNAPSHOT_HASH_TTL_MS` constant
- Added `clipboard_write_coordinator: Option<Arc<ClipboardWriteCoordinator>>` field
- Added `with_clipboard_write_coordinator(coordinator)` builder method
- Replaced manual guard-registration + write + rollback logic with `coordinator.write(snapshot_for_os, ClipboardWriteIntent::RemotePush).await`
- Legacy `local_clipboard`/`clipboard_change_origin` constructor params retained with `#[allow(dead_code)]`

**InboundClipboardSyncWorker (`inbound_clipboard_sync.rs`):**

- Replaced `clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>` field with `clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>`
- `build_sync_inbound_usecase()` now chains `.with_clipboard_write_coordinator(self.clipboard_write_coordinator.clone())`
- Tests updated: `build_full_usecase()` constructs coordinator from same mock ports; constructor test renamed and updated

**FileSyncOrchestratorWorker (`file_sync_orchestrator.rs`):**

- Replaced `system_clipboard: Arc<dyn SystemClipboardPort>` + `clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>` with `clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>`
- `restore_file_to_clipboard_after_transfer()` uses `coordinator.has_pending_origin()` for concurrent-write guard then `coordinator.write(snapshot, ClipboardWriteIntent::LocalRestore)`
- Tests updated with `build_test_coordinator()` helper

**ClipboardWriteCoordinator (`clipboard_write_coordinator.rs`):**

- Added `has_pending_origin()` delegation method for non-destructive peek

**Daemon entrypoint (`entrypoint.rs`):**

- `InboundClipboardSyncWorker::new()` receives `clipboard_write_coordinator.clone()`
- `FileSyncOrchestratorWorker::new()` receives `clipboard_write_coordinator` (last use, no clone needed)
- First usage at line 96 clones to avoid move before second use

**E2E tests (`clipboard_sync_e2e_test.rs`):**

- All 3 `SyncInboundClipboardUseCase::new()` Full-mode constructions updated to build `ClipboardWriteCoordinator` from same mock ports and chain `.with_clipboard_write_coordinator(coordinator)`
- Added `use uc_app::usecases::clipboard::clipboard_write_coordinator::ClipboardWriteCoordinator`

## Verification

- `cargo test -p uc-app --test clipboard_sync_e2e_test`: 5/5 pass
- `cargo test -p uc-app -p uc-daemon -p uc-bootstrap -p uc-infra`: 271 pass, 1 pre-existing unrelated failure (`transport_error_aborts_waiting_confirm` — confirmed pre-existing before changes)
- `cargo build -p uc-tauri`: compiles without error
- No remaining `remember_local/remote_snapshot_hash` or `origin_guard_key()` calls in production code outside coordinator and read-side watcher

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] has_pending_origin() missing from ClipboardWriteCoordinator**

- **Found during:** Task 2, FileSyncOrchestratorWorker refactor
- **Issue:** `FileSyncOrchestratorWorker::restore_file_to_clipboard_after_transfer()` called `clipboard_change_origin.has_pending_origin()` to guard against concurrent writes. After removing the raw port field, this delegation method was not yet on the coordinator.
- **Fix:** Added `pub async fn has_pending_origin(&self) -> bool` to `ClipboardWriteCoordinator` delegating to `self.clipboard_change_origin.has_pending_origin()`
- **Files modified:** clipboard_write_coordinator.rs

**2. [Rule 3 - Blocking] E2E tests missing coordinator wiring for Full-mode inbound**

- **Found during:** Task 2 final test run
- **Issue:** `clipboard_sync_e2e_test.rs` tests `clipboard_sync_e2e_dual_peer_in_process`, `clipboard_sync_e2e_image_single_rep`, and `clipboard_sync_e2e_windows_image_multi_rep` all construct `SyncInboundClipboardUseCase::new()` in Full mode but did not chain `.with_clipboard_write_coordinator()`, causing "clipboard_write_coordinator required for Full-mode OS write" runtime error
- **Fix:** For each test, construct `ClipboardWriteCoordinator::new(clipboard_b.clone(), origin_b.clone())` from already-available mocks and chain `.with_clipboard_write_coordinator(coordinator_b)` on the builder
- **Files modified:** clipboard_sync_e2e_test.rs

## Known Stubs

None. All four clipboard write paths are fully migrated to ClipboardWriteCoordinator.

## Self-Check: PASSED

Files exist:

- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` (contains `with_clipboard_write_coordinator`) ✓
- `src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs` (contains `clipboard_write_coordinator` field) ✓
- `src-tauri/crates/uc-daemon/src/workers/file_sync_orchestrator.rs` (contains `clipboard_write_coordinator` field) ✓
- `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs` (contains `ClipboardWriteCoordinator` import) ✓

Commits exist:

- `a3d11526` feat(73-02): refactor RestoreClipboardSelectionUseCase and CopyFileToClipboardUseCase to use coordinator ✓
- `069831e5` refactor(73-02): route all OS clipboard writes through ClipboardWriteCoordinator ✓
