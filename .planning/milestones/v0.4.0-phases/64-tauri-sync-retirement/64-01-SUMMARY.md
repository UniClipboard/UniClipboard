---
phase: 64
plan: 01
subsystem: uc-tauri/bootstrap
tags: [cleanup, wiring, daemon-retirement, dependency-removal]
dependency_graph:
  requires: [63-02]
  provides: [leaner-wiring-rs]
  affects: [uc-tauri/bootstrap/wiring.rs, uc-tauri/Cargo.toml]
tech_stack:
  added: []
  patterns: [daemon-owns-sync-loops, gui-owns-storage-tasks]
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/Cargo.toml
decisions:
  - "file_transfer_orchestrator field preserved in BackgroundRuntimeDeps struct (uc-bootstrap) with _ prefix in wiring.rs destructure; daemon still references it via uc-bootstrap"
  - "uc_core::ports::* import moved to #[cfg(test)] since only test code (TimerPort) uses it after removal"
  - "Pre-existing test failure (startup_helper_rejects_healthy_but_incompatible_daemon) confirmed unrelated to these changes"
metrics:
  duration: 7min
  completed: "2026-03-26"
  tasks: 2
  files: 2
---

# Phase 64 Plan 01: Remove Daemon-Duplicated Sync Loops from wiring.rs Summary

Removed all daemon-duplicated background sync loops from uc-tauri wiring.rs and deleted the now-unused blake3 dependency. wiring.rs reduced from 1378 to 482 lines (-896 lines), retaining only GUI-owned storage tasks and the DaemonWsBridge realtime runtime.

## What Was Built

**Task 1 — Remove daemon-duplicated sync loops, helpers, and constants (commit 0e72a9c5)**

Deleted from wiring.rs:
- `register_pairing_background_tasks()` — daemon PeerDiscoveryWorker + PeerMonitor own this
- `run_clipboard_receive_loop()` — daemon InboundClipboardSyncWorker owns this
- `run_network_realtime_loop()` — daemon FileSyncOrchestratorWorker + PeerMonitor own this
- `new_sync_inbound_clipboard_usecase()` — private helper for removed clipboard receive loop
- `restore_file_to_clipboard_after_transfer()` — only called from removed network loop
- `resolve_device_name_for_peer()` — only called from removed network loop
- `CLIPBOARD_SUBSCRIBE_BACKOFF_*` and `NETWORK_EVENTS_SUBSCRIBE_BACKOFF_*` constants
- `subscribe_backoff_ms()` and `network_events_subscribe_backoff_ms()` helpers
- `clipboard_receive`, `file_transfer_reconcile`, `file_transfer_timeout_sweep` task spawns
- All associated unused variable bindings in `start_background_tasks()`
- Unused imports: `ClipboardMessage`, `NetworkEvent`, `ClipboardChangeOriginPort`, `InboundApplyOutcome`, `SyncInboundClipboardUseCase`, `FileTransferOrchestrator`, `ClipboardHostEvent`, `ClipboardOriginKind`, `PeerConnectionHostEvent`, `TransferHostEvent`, `mpsc`, `PathBuf`

Retained in wiring.rs:
- spool scanner, SpoolerTask, BackgroundBlobWorker, SpoolJanitor spawns
- `start_realtime_runtime()` (DaemonWsBridge)
- `file_cache_cleanup` spawn
- All `#[cfg(test)]` blocks (SpaceAccessBusy* helpers, NoopSpaceAccessCrypto, etc.)

**Task 2 — Remove blake3 dependency from uc-tauri Cargo.toml (commit 9347d5b6)**

- Removed `blake3 = "1"` from `[dependencies]` in uc-tauri/Cargo.toml
- Confirmed no other non-test file in uc-tauri references `blake3::`
- 70 uc-tauri tests pass; 1 pre-existing failure confirmed unrelated

## Deviations from Plan

### Auto-fixed Issues

None.

### Additional Changes

**[Rule 2 - Missing] Moved `uc_core::ports::*` import to `#[cfg(test)]`**
- Found during: Task 1
- Issue: After removing all non-test callers of the ports glob import, Rust flagged `use uc_core::ports::*` as unused in non-test code (only `TimerPort` in `#[cfg(test)]` struct still used it)
- Fix: Added `#[cfg(test)]` attribute to the glob import
- Files modified: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
- This is correctness cleanup, not a deviation from plan intent

## Known Stubs

None — all removed code paths are fully owned by the daemon. The GUI retains only the DaemonWsBridge bridge for realtime event delivery.

## Self-Check

- [x] `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` exists and is 482 lines
- [x] `src-tauri/crates/uc-tauri/Cargo.toml` exists with blake3 removed
- [x] Commit `0e72a9c5` exists
- [x] Commit `9347d5b6` exists
- [x] `cargo check -p uc-tauri` exits 0
- [x] `cargo test -p uc-tauri` — 70 passed, 1 pre-existing failure unrelated to this plan

## Self-Check: PASSED
