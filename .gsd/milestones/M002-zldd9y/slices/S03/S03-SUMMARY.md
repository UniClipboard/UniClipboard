---
id: S03
parent: M002-zldd9y
milestone: M002-zldd9y
provides:
  - GET /storage/stats handler returning 5 storage metrics: totalSizeBytes, blobCount, databaseSizeBytes, cacheSizeBytes, spoolSizeBytes
  - POST /storage/clear-cache handler requiring confirmed:true or returning 400 confirmation_required
requires:
  - slice: S01
    provides: CoreUseCases, L2+ router infrastructure, permission level constants
affects:
  - Frontend direct-connection consumers of daemon HTTP API
key_files:
  - src-tauri/crates/uc-daemon/src/api/storage.rs
  - src-tauri/crates/uc-daemon/src/api/mod.rs
  - src-tauri/crates/uc-daemon/src/api/routes.rs
key_decisions:
  - Inline compute_dir_size in uc-daemon because uc-app dir_size is pub(crate) — Rust visibility applies at crate level, not module level
  - GET /storage/stats runs get_storage_stats() and list_clipboard_entries() sequentially with per-branch error handling
  - POST /storage/clear-cache uses L4 confirmation pattern: JsonRejection catches missing body, explicit false check catches confirmed:false
patterns_established:
  - L4 confirmation-required pattern for destructive operations (confirmed field with 400 on absent/false)
observability_surfaces:
  - tracing::info on successful cache clear with freed_bytes field
  - tracing::error on storage stats or clipboard list failures with %e error context
drill_down_paths:
  - tasks/T01-SUMMARY.md
  - tasks/T02-SUMMARY.md
duration: ''
verification_result: passed
completed_at: 2026-03-30T02:16:01.707Z
blocker_discovered: false
---

# S03: Storage Stats &amp; Clear Cache HTTP Handlers

**GET /storage/stats with 5 metrics fields and POST /storage/clear-cache with L4 confirmation pattern shipped**

## What Happened

Created src-tauri/crates/uc-daemon/src/api/storage.rs implementing two HTTP handlers. GET /storage/stats calls get_storage_stats().execute() for db/cache/vault/logs sizes, list_clipboard_entries() for blob_count, and an inline compute_dir_size() for spool_size_bytes, returning a StorageStatsResponse with 5 camelCase fields. POST /storage/clear-cache implements the L4 confirmation pattern: returns HTTP 400 with confirmation_required error if the request body is absent (JsonRejection) or if confirmed is false; executes clear_cache().execute() and returns freed_bytes only when confirmed: true. The storage module was declared in api/mod.rs and merged into router_l2_plus in routes.rs during T01. T02 confirmed both registrations were already correct — no delta needed. A notable implementation detail: compute_dir_size() is inlined in storage.rs using tokio::fs because uc-app/usecases/storage::dir_size is pub(crate) and not visible across the uc-daemon crate boundary.

## Verification

cargo check -p uc-daemon: 0 errors, 1 pre-existing warning (unused unauthorized fn in routes.rs). cargo test -p uc-daemon --lib: 113 passed, 0 failed. pairing_api integration test failures are pre-existing (DB-locking + assertion mismatches), completely unrelated to storage handlers.

## Requirements Advanced

None.

## Requirements Validated

None.

## New Requirements Surfaced

None.

## Requirements Invalidated or Re-scoped

None.

## Deviations

None. Minor implementation adaptation: inlined compute_dir_size() instead of importing from uc-app (pub(crate) boundary).

## Known Limitations

spool_size_bytes uses an inline recursive directory-size walk with no caching or debouncing — could be slow for large directories. Storage stats endpoint has no caching; every request hits disk and DB.

## Follow-ups

Consider adding a TTL cache or snapshot for spool_size_bytes if directories grow large. GET /storage/stats could use tokio::join! to parallelize storage stats + clipboard list calls instead of sequential execution.

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/api/storage.rs` — New file: GET /storage/stats and POST /storage/clear-cache HTTP handlers
- `src-tauri/crates/uc-daemon/src/api/mod.rs` — Added pub mod storage;
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — Merged storage::router() into router_l2_plus
