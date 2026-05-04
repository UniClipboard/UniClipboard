---
phase: 92-bootstrap-wiring-and-daemon-http-routes
plan: 02
subsystem: daemon-search-integration
tags: [search, coordinator, projection, clipboard-watcher, daemon-service, tdd]
dependency_graph:
  requires: [92-01]
  provides: [search-coordinator-service, search-projection-builder, live-capture-indexing]
  affects: [clipboard-watcher, daemon-api-state, entrypoint]
tech_stack:
  added: []
  patterns:
    - SearchProjectionBuilder static methods (no instance state)
    - Single-flight Mutex for rebuild serialization
    - tokio::sync::Mutex<()> as owned guard for background spawn
    - Deferred service pattern for search-coordinator (mirrors clipboard-watcher deferral)
key_files:
  created:
    - src-tauri/crates/uc-daemon/src/search/mod.rs
    - src-tauri/crates/uc-daemon/src/search/projection.rs
    - src-tauri/crates/uc-daemon/src/search/coordinator.rs
    - src-tauri/crates/uc-daemon/tests/search_integration.rs
  modified:
    - src-tauri/crates/uc-daemon/src/lib.rs
    - src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs
    - src-tauri/crates/uc-daemon/src/api/server.rs
    - src-tauri/crates/uc-daemon/src/app.rs
    - src-tauri/crates/uc-daemon/src/entrypoint.rs
decisions:
  - SearchProjectionBuilder uses static methods not instance methods — no state needed
  - build_from_capture uses live snapshot bytes directly; build_from_persisted uses inline_data only
  - Rebuild serialization uses try_lock_owned() — non-blocking check avoids async deadlock
  - SearchCoordinator deferred alongside clipboard services when locked/GUI-managed
  - Key derivation failure during rebuild logs at debug (session locked is expected in test mode)
metrics:
  duration: 90min
  completed_date: 2026-04-11
  tasks_completed: 2
  files_changed: 9
---

# Phase 92 Plan 02: Daemon Search Integration Layer Summary

SearchProjectionBuilder as single projection authority + SearchCoordinator as rebuild lifecycle owner with WS progress forwarding and single-flight serialization.

## Tasks Completed

### Task 1: SearchProjectionBuilder and live capture indexing (TDD)

Created `src-tauri/crates/uc-daemon/src/search/projection.rs` with two static methods:

- `build_from_capture(entry, snapshot, selection) -> Option<SearchPipelineInput>` — builds from live OS clipboard snapshot
- `build_from_persisted(entry, selection, reps) -> Option<SearchPipelineInput>` — builds from stored representations

Updated `clipboard_watcher.rs` to call `index_clipboard_entry()` after successful capture:
1. Fetch saved `ClipboardEntry` from `clipboard_entry_repo`
2. Compute `ClipboardSelection` via `representation_policy.select()`
3. Derive search key via `search_key_derivation.derive_search_key()`
4. Build `(document, postings)` via `search_pipeline.build()`
5. Skip if session locked or postings empty
6. Call `CoreUseCases::new(runtime).index_clipboard_entry().execute(doc, postings)`

**Commit:** `3a3575ae`

### Task 2: SearchCoordinator and daemon plumbing (TDD)

Created `src-tauri/crates/uc-daemon/src/search/coordinator.rs` with `SearchCoordinator`:

- Single-flight rebuild guard: `tokio::sync::Mutex<()>` with `try_lock_owned()`
- Status/reason state: separate `Mutex<CoordinatorState>` for lock-free observable reads
- Startup evaluation: version_mismatch → initial_backfill → unavailable fallback
- Manual rebuild: `request_manual_rebuild()` returns `ManualRebuildResult::{Accepted, AlreadyInProgress}`
- Progress forwarding: spawns task to forward `RebuildProgress` events to broadcast channel
- Paginated rebuild in batches of 200 entries
- WS events: `ws_topic::SEARCH` + `ws_event::SEARCH_REBUILD_PROGRESS` / `SEARCH_STATUS_SNAPSHOT`

Exact reason codes: `initial_backfill`, `version_mismatch`, `manual_rebuild`, `rebuild_failed_waiting_for_retry`
Exact status values: `ready`, `rebuilding`, `unavailable`

Daemon plumbing:
- `DaemonApiState.search_coordinator: Option<Arc<SearchCoordinator>>` + `with_search_coordinator()` + `search_coordinator()`
- `DaemonApp.new_with_deferred()` accepts `search_coordinator: Option<Arc<SearchCoordinator>>`
- `DaemonApp.run()` wires coordinator into api_state via `.with_search_coordinator()`
- `entrypoint.rs` constructs coordinator, registers as `"search-coordinator"` service
- Deferred alongside clipboard-watcher + inbound-clipboard-sync when `should_defer_clipboard`

**Commit:** `581d313e`

## Acceptance Criteria Verification

All 4 acceptance tests pass:
- `search_projection_builder_builds_from_capture_and_persisted_sources` — PASS
- `search_capture_indexes_entries_and_delete_keeps_postings_clean` — PASS
- `search_coordinator_auto_backfill_and_manual_rebuild_serialization` — PASS
- `search_status_snapshot_reports_unavailable_after_failed_rebuild` — PASS

## Deviations from Plan

### Auto-fixed Issues

None — plan executed as specified.

### Design Notes

1. The `search_capture_indexes_entries_and_delete_keeps_postings_clean` integration test uses a fixed `SearchKey([0xABu8; 32])` instead of deriving from the locked encryption session. The test validates the `index_entry → remove_entry` round-trip, not key derivation. The plan's intent (prove delete cleanup works) is fulfilled.

2. `DaemonApp::new_with_deferred` signature extended with `search_coordinator` parameter. All callers (only `entrypoint.rs`) updated accordingly.

3. The `main_calls_recovery_before_daemon_construction` structural test in `app.rs` continues to pass — `recover_encryption_session` appears before `new_with_deferred` in `entrypoint.rs`.

## Known Stubs

None — all implemented code is production-ready.

## Self-Check: PASSED
