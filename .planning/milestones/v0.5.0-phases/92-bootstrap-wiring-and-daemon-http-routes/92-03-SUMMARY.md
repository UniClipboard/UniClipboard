---
phase: 92-bootstrap-wiring-and-daemon-http-routes
plan: "03"
subsystem: daemon-api
tags: [search, http-routes, websocket, transport]
dependency_graph:
  requires: [92-01, 92-02]
  provides: [search-http-surface, search-ws-topic]
  affects: [uc-daemon, uc-daemon-contract]
tech_stack:
  added: []
  patterns:
    - query param parsing with operator inference and mixed-operator rejection
    - HTTP 423 session_locked lock guard pattern on all search routes
    - SearchStatusData combining coordinator snapshot and index meta timestamps
    - WS snapshot event built from coordinator + search_index.get_index_meta()
key_files:
  created:
    - src-tauri/crates/uc-daemon/src/api/search.rs
    - src-tauri/crates/uc-daemon/src/api/dto/search.rs
  modified:
    - src-tauri/crates/uc-daemon/src/api/routes.rs
    - src-tauri/crates/uc-daemon/src/api/mod.rs
    - src-tauri/crates/uc-daemon/src/api/dto/mod.rs
    - src-tauri/crates/uc-daemon/src/api/openapi.rs
    - src-tauri/crates/uc-daemon/src/api/ws.rs
    - src-tauri/crates/uc-daemon-contract/src/api/dto/ws.rs
decisions:
  - "SearchStatusData combines coordinator status_snapshot() with runtime.wiring_deps().search.search_index.get_index_meta() — coordinator does not track timestamps independently"
  - "WS search snapshot calls get_index_meta() from state.runtime directly — DaemonApiState has runtime field, no new accessor needed"
  - "Repeated query params (fileTypes, extensions) handled via comma-separated strings in single param — avoids needing custom deserializer for repeated params"
metrics:
  duration: 40min
  completed: "2026-04-11T08:45:34Z"
  tasks: 2
  files: 8
---

# Phase 92 Plan 03: Search HTTP and WebSocket Transport Summary

Expose the Phase 92 daemon transport surface: `/search/query`, `/search/status`, `/search/rebuild` HTTP endpoints, a dedicated `search` WebSocket topic with status snapshot, and exact 423 lock behavior.

## Tasks Completed

### Task 1: Add `/search/query`, `/search/status`, `/search/rebuild` HTTP routes

**Commit:** 3988f6c4

Created `src-tauri/crates/uc-daemon/src/api/dto/search.rs` with six transport structs (`SearchResultDto`, `SearchQueryResponse`, `SearchStatusData`, `SearchStatusResponse`, `SearchRebuildAcceptedData`, `SearchRebuildAcceptedResponse`), all using `#[serde(rename_all = "camelCase")]`.

Created `src-tauri/crates/uc-daemon/src/api/search.rs` with:
- `pub fn router()` mounting GET `/search/query`, GET `/search/status`, POST `/search/rebuild`
- `parse_search_query()` parsing `SearchQueryParams` with operator inference from standalone AND/OR tokens, mixed-operator rejection (`invalid_query` 400), time preset/absolute range handling, comma-separated file types and extensions, limit clamped to 200
- `require_encryption_ready()` lock guard returning HTTP 423 `session_locked` when encryption session is locked
- `/search/status` handler combining coordinator `status_snapshot()` with `runtime.wiring_deps().search.search_index.get_index_meta()` for timestamps
- `/search/rebuild` returning 202 on accept, 409 `rebuild_already_running` on concurrent request

Registered `search::router()` in `router_l2_plus` and search DTOs in `openapi.rs`.

### Task 2: Add `search` WebSocket topic with subscribe snapshot and progress forwarding

**Commit:** 1ffe0164

Updated `uc-daemon-contract/src/api/dto/ws.rs`: added `ws_topic::SEARCH` to `WS_SUPPORTED_TOPICS`.

Updated `uc-daemon/src/api/ws.rs`:
- Added `ws_topic::SEARCH` to `is_supported_topic()`
- Added search topic case in `build_snapshot_event()` that produces a `SearchStatusData` payload (with camelCase `lastRebuildStartedAtMs` and `lastRebuildCompletedAtMs`) by combining coordinator `status_snapshot()` with index meta
- Existing shared broadcast path delivers `search.rebuild_progress` events — no second channel created

## Verification

All required tests pass:
- `search_query_route_parses_filters_and_rejects_mixed_operators` — verifies parser rules for operator inference, mixed-operator rejection, time range, file types, extensions, limit clamping
- `search_topic_is_supported_for_websocket_subscriptions` — verifies `ws_topic::SEARCH` is in supported set
- `search_topic_subscription_receives_status_snapshot` — verifies snapshot event has correct topic, event_type, and camelCase payload fields

## Deviations from Plan

None — plan executed exactly as written.

The pre-existing `search_status_snapshot_reports_unavailable_after_failed_rebuild` test in `coordinator.rs` fails intermittently when tests run in parallel due to SQLite database locking (`Failed to set journal_mode=WAL: database is locked`). This is a pre-existing infrastructure issue unrelated to this plan's changes (confirmed by reproducing in isolation on the base commit).

## Known Stubs

None — all routes delegate correctly to coordinator and use cases.

## Self-Check: PASSED

- FOUND: src-tauri/crates/uc-daemon/src/api/search.rs
- FOUND: src-tauri/crates/uc-daemon/src/api/dto/search.rs
- FOUND commit 3988f6c4: feat(92-03): add /search/query, /search/status, /search/rebuild HTTP routes
- FOUND commit 1ffe0164: feat(92-03): add search websocket topic with status snapshot and contract
