---
phase: 67-setup-filter
plan: 01
subsystem: daemon-bootstrap
tags: [rust, daemon, encryption, deferred-startup, peer-discovery]
one_liner: "Deferred PeerDiscoveryWorker infrastructure: SetupCompletionEmitter + DaemonApp oneshot-gated worker + parameterized runtime builder"
dependency_graph:
  requires: []
  provides:
    - SetupCompletionEmitter (uc-daemon/src/app.rs)
    - DaemonApp::new_with_deferred() (uc-daemon/src/app.rs)
    - build_non_gui_runtime_with_emitter (uc-bootstrap/src/non_gui_runtime.rs)
  affects:
    - uc-daemon startup sequence
    - uc-bootstrap public API
tech_stack:
  added:
    - tokio::sync::oneshot channel for setup completion signaling
  patterns:
    - Deferred service start via oneshot receiver in event loop
    - SessionReadyEmitter injection for daemon-specific signaling
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-daemon/src/app.rs
    - src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs
    - src-tauri/crates/uc-bootstrap/src/lib.rs
decisions:
  - "SetupCompletionEmitter uses tokio::sync::Mutex<Option<Sender>> so double-call is a no-op not a panic"
  - "recover_encryption_session returns bool to distinguish initialized+unlocked from uninitialized"
  - "DaemonApp::run() restructured to loop so deferred worker arm fires once then daemon continues"
  - "Existing new() constructor unchanged; new_with_deferred() is additive for Plan 02 integration"
  - "build_non_gui_runtime_with_setup now delegates to build_non_gui_runtime_with_emitter"
metrics:
  duration: "6min"
  completed_date: "2026-03-27"
  tasks_completed: 2
  files_modified: 3
requirements:
  - PH67-01
  - PH67-02
  - PH67-03
  - PH67-04
  - PH67-05
---

# Phase 67 Plan 01: Deferred PeerDiscoveryWorker Infrastructure Summary

Adds foundational plumbing for conditional `PeerDiscoveryWorker` startup in the daemon.
Uninitialized devices (no encryption setup) must not advertise on the network before the setup flow completes. This plan creates the signaling mechanism and startup hook.

## Tasks Completed

| # | Task | Commit | Files |
|---|------|--------|-------|
| 1 | Modify recover_encryption_session, create SetupCompletionEmitter, add deferred worker fields | f846c308 | src-tauri/crates/uc-daemon/src/app.rs |
| 2 | Add build_non_gui_runtime_with_emitter parameter | da800ab6 | src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs, src-tauri/crates/uc-bootstrap/src/lib.rs |

## What Was Built

### recover_encryption_session returns bool

The function signature changed from `anyhow::Result<()>` to `anyhow::Result<bool>`:
- `Ok(true)` — encryption Initialized and session recovered successfully
- `Ok(false)` — encryption Uninitialized (first run, no recovery needed)
- `Err(_)` — recovery failed, daemon must not start

### SetupCompletionEmitter

A new struct implementing `SessionReadyEmitter` via a oneshot channel. When `AppLifecycleCoordinator::ensure_ready()` calls `emit_ready()`, it fires the channel, signaling `DaemonApp` to start the deferred `PeerDiscoveryWorker`. Double-call is a no-op (uses `Option<Sender>` behind a `Mutex`).

### DaemonApp deferred worker support

Two new fields added to `DaemonApp`:
- `deferred_peer_discovery: Option<Arc<dyn DaemonService>>` — the worker to start after setup
- `setup_complete_rx: Option<tokio::sync::oneshot::Receiver<()>>` — the signal channel

New `new_with_deferred()` constructor for Phase 67 callers. Existing `new()` is unchanged and initializes both fields to `None`.

`run()` changed to `mut self` and restructured with a `loop` around the `select!` block. A new arm handles the oneshot receiver — when it fires, the deferred worker is spawned and registered in `self.services` for managed shutdown.

### build_non_gui_runtime_with_emitter

New function accepting a custom `SessionReadyEmitter`. The existing `build_non_gui_runtime_with_setup` now delegates to it, passing `LoggingSessionReadyEmitter`. This allows the daemon to inject `SetupCompletionEmitter` in Plan 02.

## Test Results

- `cargo test -p uc-daemon --lib -- app::tests`: 7 passed (includes 2 new SetupCompletionEmitter tests)
- `cargo test -p uc-bootstrap --lib`: 20 passed
- Pre-existing `process_metadata::tests::remove_pid_file_deletes_existing_pid_metadata` failure is unrelated to this plan's changes

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None - all connections are properly wired. Plan 02 will call `new_with_deferred()` and `build_non_gui_runtime_with_emitter` from the daemon's `main.rs` to complete the full integration.

## Self-Check: PASSED

- [x] `src-tauri/crates/uc-daemon/src/app.rs` modified — SetupCompletionEmitter, new fields, new_with_deferred()
- [x] `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` modified — build_non_gui_runtime_with_emitter
- [x] `src-tauri/crates/uc-bootstrap/src/lib.rs` modified — re-export added
- [x] Commit f846c308 exists (Task 1)
- [x] Commit da800ab6 exists (Task 2)
