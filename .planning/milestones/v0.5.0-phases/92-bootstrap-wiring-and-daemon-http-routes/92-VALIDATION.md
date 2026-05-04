---
phase: 92
slug: bootstrap-wiring-and-daemon-http-routes
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-11
---

# Phase 92 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust unit tests + daemon HTTP/WS integration tests |
| **Config file** | none — existing Cargo workspace + daemon test fixtures |
| **Quick run command** | `cd src-tauri && cargo test -p uc-core search:: && cargo test -p uc-app usecases::search && cargo test -p uc-daemon search_` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-core search:: && cargo test -p uc-app usecases::search && cargo test -p uc-infra search::sqlite_index && cargo test -p uc-daemon search_ && cargo check -p uc-daemon` |
| **Estimated runtime** | ~210 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-core search:: && cargo test -p uc-app usecases::search` for contract work, or `cd src-tauri && cargo test -p uc-daemon search_` for daemon work
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-core search:: && cargo test -p uc-app usecases::search && cargo test -p uc-infra search::sqlite_index && cargo test -p uc-daemon search_ && cargo check -p uc-daemon`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 210 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 92-01-01 | 01 | 1 | SQRY-01, SQRY-02, SQRY-03, SQRY-04 | unit | `cd src-tauri && cargo test -p uc-core search::result && cargo test -p uc-app usecases::search::search_clipboard_entries` | ❌ W0 | ⬜ pending |
| 92-01-02 | 01 | 1 | SQRY-01, SQRY-02, SQRY-03, SQRY-04 | integration | `cd src-tauri && cargo test -p uc-infra search::sqlite_index::tests::search_query` | ✅ existing | ⬜ pending |
| 92-02-01 | 02 | 2 | SQRY-05 | integration | `cd src-tauri && cargo test -p uc-daemon search_capture_indexes_entries_and_delete_keeps_postings_clean` | ❌ W0 | ⬜ pending |
| 92-02-02 | 02 | 2 | REBLD-04 | integration | `cd src-tauri && cargo test -p uc-daemon search_coordinator_auto_backfill_and_manual_rebuild_serialization` | ❌ W0 | ⬜ pending |
| 92-03-01 | 03 | 3 | SQRY-01, SQRY-02, SQRY-03, SQRY-04, SQRY-06 | integration | `cd src-tauri && cargo test -p uc-daemon search_query_route_parses_filters_and_rejects_mixed_operators` | ❌ W0 | ⬜ pending |
| 92-03-02 | 03 | 3 | SQRY-05, REBLD-04 | integration | `cd src-tauri && cargo test -p uc-daemon search_status_and_rebuild_routes_enforce_lock_and_emit_progress` | ❌ W0 | ⬜ pending |
| 92-04-01 | 04 | 4 | SQRY-01, SQRY-02, SQRY-03, SQRY-04, SQRY-05, SQRY-06 | integration | `cd src-tauri && cargo test -p uc-daemon search_api_end_to_end_capture_query_and_locking` | ❌ W0 | ⬜ pending |
| 92-04-02 | 04 | 4 | REBLD-04 | websocket integration | `cd src-tauri && cargo test -p uc-daemon search_rebuild_websocket_events_include_started_and_complete` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-core/src/search/result.rs` — add `SearchResultsPage` so later tests have a stable page contract to target
- [ ] `src-tauri/crates/uc-daemon/src/search/` — create the coordinator/projection module namespace used by daemon search tests
- [ ] `src-tauri/crates/uc-daemon/tests/search_api.rs` and/or `src-tauri/crates/uc-daemon/tests/search_ws.rs` — dedicated fixtures for search routes and raw WebSocket assertions

---

## Manual-Only Verifications

All Phase 92 behaviors should be automatable with daemon HTTP fixtures, raw WebSocket clients, and direct SQLite inspection. No manual-only verification is expected.

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 210s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-04-11
