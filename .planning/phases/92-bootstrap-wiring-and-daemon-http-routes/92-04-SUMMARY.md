---
phase: 92-bootstrap-wiring-and-daemon-http-routes
plan: "04"
subsystem: daemon-search-integration-tests
tags: [search, integration-test, websocket, http, tdd]
dependency_graph:
  requires: [92-01, 92-02, 92-03]
  provides: [search-http-integration-tests, search-ws-integration-tests]
  affects: [uc-daemon tests]
tech_stack:
  added: [diesel dev-dependency for direct DB inspection in uc-daemon tests]
  patterns:
    - TDD: RED (compiling, failing) → GREEN (passing) commit sequence
    - SearchCoordinator event_tx must share the DaemonApiState broadcast channel
    - build_cli_runtime for test fixture construction (avoids block_on conflicts)
    - Direct SQLite inspection via diesel for search_posting row counting
key_files:
  created:
    - src-tauri/crates/uc-daemon/tests/search_api.rs
    - src-tauri/crates/uc-daemon/tests/search_ws.rs
  modified:
    - src-tauri/crates/uc-daemon/Cargo.toml
decisions:
  - id: D-01
    summary: "SearchCoordinator must use DaemonApiState.event_tx, not its own channel"
    rationale: "WS fanout subscribes to DaemonApiState.event_tx; coordinator emitting to a separate channel means WS clients never receive progress events"
  - id: D-02
    summary: "build_cli_runtime instead of build_non_gui_runtime_with_setup for test fixture"
    rationale: "build_non_gui_runtime_with_setup calls block_on internally, which panics when called within a tokio::test runtime"
  - id: D-03
    summary: "Search coordinator emits status_snapshot(rebuilding) before first rebuild_progress event"
    rationale: "Test must skip the status_snapshot and look for rebuild_progress events, not assert the first event is rebuild_progress"
metrics:
  duration: "~45min"
  completed: "2026-04-11"
  tasks_completed: 2
  files_created: 2
  files_modified: 1
---

# Phase 92 Plan 04: Search Integration Tests Summary

**One-liner:** Daemon integration tests proving search HTTP routes return correct status codes and WS transport delivers rebuild progress events with started/complete stages.

## What Was Built

Two new integration test files completing Phase 92's end-to-end verification layer:

### Task 1: `src-tauri/crates/uc-daemon/tests/search_api.rs`

HTTP integration tests proving:
- `search_api_end_to_end_capture_query_and_locking`: index entry → `/search/query` returns 200 → `remove_entry` deletes all `search_posting` rows (direct DB inspection) → lock encryption → all three search routes return HTTP 423 with `code=session_locked`
- `search_query_route_parses_filters_and_rejects_mixed_operators`: mixed AND+OR returns 400 with `invalid_query`; comma-separated `fileTypes`, `extensions`, `timePreset`, and `fromMs/toMs` range all parsed correctly via real axum transport

### Task 2: `src-tauri/crates/uc-daemon/tests/search_ws.rs`

WebSocket integration tests proving:
- `search_status_and_rebuild_routes_enforce_lock_and_emit_progress`: subscribing to `search` topic yields `search.status_snapshot`; rebuild emits `search.rebuild_progress`; locked routes return 423
- `search_rebuild_websocket_events_include_started_and_complete`: event stream contains at least one `stage=started` and one `stage=complete` payload, both with `topic=search` and `type=search.rebuild_progress`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed event_tx channel isolation**

- **Found during:** Task 1 (first GREEN attempt)
- **Issue:** Initial fixture created SearchCoordinator with its own `broadcast::channel`. The WS fanout subscribes to `DaemonApiState.event_tx`, a different channel. Result: all coordinator events showed "no WS subscribers" and were silently dropped.
- **Fix:** Build `DaemonApiState` first, extract `api_state.event_tx.clone()`, pass that to `SearchCoordinator::new`. Now coordinator and WS use the same broadcast channel.
- **Files modified:** `search_api.rs`, `search_ws.rs`
- **Commit:** a344e864

**2. [Rule 3 - Blocking] Switched from build_non_gui_runtime_with_setup to build_cli_runtime**

- **Found during:** Task 1 RED run
- **Issue:** `build_non_gui_runtime_with_setup` internally calls `block_on`, which panics when called within a `#[tokio::test]` runtime context ("Cannot start a runtime from within a runtime").
- **Fix:** Use `uc_bootstrap::build_cli_runtime(None)` (same pattern as `clipboard_api.rs`, `websocket_api.rs`).
- **Files modified:** `search_api.rs`, `search_ws.rs`
- **Commit:** a344e864

**3. [Rule 1 - Bug] Fixed progress event ordering assertion**

- **Found during:** Task 2 first GREEN attempt
- **Issue:** Test asserted the FIRST event after `POST /search/rebuild` is `search.rebuild_progress`. The coordinator correctly emits `search.status_snapshot(rebuilding)` first.
- **Fix:** Loop over events until `search.rebuild_progress` is found, skipping any `search.status_snapshot` interim events.
- **Files modified:** `search_ws.rs`
- **Commit:** a344e864

### Additions (Rule 2)

**Added diesel as dev-dependency to uc-daemon/Cargo.toml**

- **Reason:** The plan requires "direct DB inspection" for the `search_posting` count after `remove_entry`. The `SearchIndexPort` trait doesn't expose a count query. Using diesel directly on the runtime's db_path provides an authoritative row count without widening the public API.
- **Impact:** `diesel = "2.3.5"` added to `[dev-dependencies]` in `Cargo.toml`. Test-only dependency — no production surface change.

## Known Stubs

None. All 4 integration tests exercise real code paths with no stubs or placeholder data.

## Self-Check: PASSED

- FOUND: src-tauri/crates/uc-daemon/tests/search_api.rs
- FOUND: src-tauri/crates/uc-daemon/tests/search_ws.rs
- FOUND: commit 6b01f638 (RED - failing tests)
- FOUND: commit a344e864 (GREEN - all tests passing)
