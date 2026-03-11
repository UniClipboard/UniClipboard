---
phase: 23-distributed-tracing-with-trace-view-visualization-for-cross-device-observability
plan: '01'
subsystem: observability
tags: [seq, clef, cross-device, tracing]
dependency_graph:
  requires: []
  provides: [device_id injection into Seq CLEF events]
  affects: [Seq visualization queries]
tech_stack:
  added: []
  patterns: [device_id static field injection]
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-observability/src/seq/layer.rs
    - src-tauri/crates/uc-observability/src/seq/mod.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs
    - docker-compose.seq.yml
decisions:
  - device_id read from {config_dir}/device_id.txt at tracing initialization time
  - device_id appears as static field after @m (message) and target fields in CLEF JSON
  - Seq bound to 0.0.0.0:5341 for LAN accessibility
metrics:
  duration_seconds: 19
  completed_date: '2026-03-11'
---

# Phase 23 Plan 01: Device ID Injection for Seq Cross-Device Correlation

## Summary

Injected `device_id` as a static field into every CLEF event sent to Seq, enabling cross-device log correlation. Device ID is read from `{config_dir}/device_id.txt` at tracing initialization time and Seq is now accessible from LAN devices for cross-device testing.

## Completed Tasks

| Task | Name                                         | Commit   | Files                  |
| ---- | -------------------------------------------- | -------- | ---------------------- |
| 1    | Add device_id field to SeqLayer struct       | 6707c228 | seq/layer.rs           |
| 2    | Modify format_clef_event to inject device_id | 6707c228 | seq/layer.rs           |
| 3    | Update build_seq_layer to accept device_id   | 6707c228 | seq/mod.rs             |
| 4    | Resolve device_id early in tracing.rs        | 6707c228 | bootstrap/tracing.rs   |
| 5    | Update docker-compose.seq.yml for LAN        | 6707c228 | docker-compose.seq.yml |

## Changes Made

### SeqLayer (layer.rs)

- Added `device_id: Option<String>` field to struct
- Updated constructor to accept and store device_id
- Modified `on_event()` to pass device_id to `format_clef_event()`
- Updated `format_clef_event()` to inject device_id field after message/target, before span fields

### build_seq_layer (mod.rs)

- Changed signature to accept `device_id: Option<&str>` parameter
- Pass device_id to SeqLayer::new

### tracing.rs

- Added `resolve_device_id_for_seq()` helper to read from `{config_dir}/device_id.txt`
- Called helper after getting app_dirs to resolve device_id early
- Pass device_id to build_seq_layer

### docker-compose.seq.yml

- Changed port binding to `0.0.0.0:5341:80` for LAN access
- Added `SEQ_FIRSTRUN_ADMINPASSWORD: 'uniclipboard'` for development
- Added warning comment about development-only configuration

## Verification

All tasks compile successfully:

```bash
cd src-tauri && cargo check -p uc-observability -p uc-tauri
```

## Deviations from Plan

None - plan executed exactly as written.

## Self-Check

- [x] All modified files exist
- [x] Commit 6707c228 exists
- [x] All tasks compile
- [x] device_id field appears in SeqLayer struct
- [x] build_seq_layer accepts device_id parameter
- [x] docker-compose.seq.yml binds to 0.0.0.0:5341
