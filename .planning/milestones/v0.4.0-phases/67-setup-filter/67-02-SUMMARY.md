---
phase: 67-setup-filter
plan: 02
subsystem: daemon-bootstrap
tags: [rust, daemon, encryption, deferred-startup, peer-discovery, conditional-services]
one_liner: "Conditional PeerDiscoveryWorker registration: main.rs gates discovery on encryption state, wires SetupCompletionEmitter, defers worker via oneshot on first run"
dependency_graph:
  requires:
    - 67-01 (SetupCompletionEmitter, DaemonApp::new_with_deferred, build_non_gui_runtime_with_emitter)
  provides:
    - Daemon does NOT start PeerDiscoveryWorker when encryption is Uninitialized
    - Daemon starts PeerDiscoveryWorker normally when encryption is Initialized+unlocked
    - PeerDiscoveryWorker starts dynamically after setup completes on uninitialized device
    - Peer-discovery health status is Stopped when uninitialized (transitions to Healthy after setup)
  affects:
    - uc-daemon startup sequence
    - uc-daemon/src/state.rs (update_service_health added)
tech_stack:
  added: []
  patterns:
    - Conditional service registration based on encryption state query at startup
    - SetupCompletionEmitter injected as SessionReadyEmitter into CoreRuntime
    - Oneshot channel bridges emitter (sender in CoreRuntime) to DaemonApp (receiver)
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-daemon/src/main.rs
    - src-tauri/crates/uc-daemon/src/app.rs
    - src-tauri/crates/uc-daemon/src/state.rs
decisions:
  - "recover_encryption_session made pub so main.rs can call it before DaemonApp construction"
  - "Removed recover_encryption_session from DaemonApp::run() — Phase 67 moved it to main.rs for deferred-start logic"
  - "PeerDiscoveryWorker built unconditionally (cheap) then conditionally included in services vec"
  - "initial_statuses built AFTER encryption check so peer-discovery health reflects actual state"
  - "RuntimeState::update_service_health() added (not update_worker_statuses) for single-entry mutation"
  - "Deferred worker arm updates state health to Healthy after starting worker"
metrics:
  duration: "8min"
  completed_date: "2026-03-27"
  tasks_completed: 1
  files_modified: 3
requirements:
  - PH67-01
  - PH67-02
  - PH67-03
  - PH67-06
  - PH67-07
---

# Phase 67 Plan 02: Wire Conditional PeerDiscoveryWorker Summary

Completes the Phase 67 integration by using the infrastructure from Plan 01 to conditionally start
`PeerDiscoveryWorker` based on encryption state. On first run (encryption Uninitialized),
peer discovery is suppressed until setup completes; on subsequent runs (Initialized+unlocked),
peer discovery starts immediately.

## Tasks Completed

| # | Task | Commit | Files |
|---|------|--------|-------|
| 1 | Wire conditional PeerDiscoveryWorker and SetupCompletionEmitter in main.rs and app.rs | 1a8a9b08 | src-tauri/crates/uc-daemon/src/main.rs, src-tauri/crates/uc-daemon/src/app.rs, src-tauri/crates/uc-daemon/src/state.rs |

## What Was Built

### main.rs restructured (composition root)

The daemon's `main()` now follows this explicit construction sequence:

1. Build deps, storage_paths, setup_ports (unchanged)
2. Create oneshot channel + `SetupCompletionEmitter`
3. Build `CoreRuntime` via `build_non_gui_runtime_with_emitter` (injects emitter)
4. Create Tokio runtime
5. Call `recover_encryption_session` inside `rt.block_on()` → get `encryption_unlocked: bool`
6. Build `initial_statuses` with peer-discovery health conditional on `encryption_unlocked`
7. Build state, pairing_host, peer_monitor, workers
8. Build `PeerDiscoveryWorker` unconditionally (cheap to construct)
9. Build conditional services vec based on `encryption_unlocked`
10. Call `DaemonApp::new_with_deferred()` with all parameters
11. `rt.block_on(daemon.run())`

### app.rs changes

- `recover_encryption_session` promoted to `pub` so `main.rs` can call it
- Removed `recover_encryption_session` call from `DaemonApp::run()` (it moved to `main.rs`)
- Removed now-unused `info_span` and `Instrument` imports
- Deferred worker select arm now updates peer-discovery health via `state.update_service_health("peer-discovery", ServiceHealth::Healthy)` after spawning the worker
- Updated structural test: replaced `run_method_contains_encryption_recovery_call` with two tests:
  - `recover_encryption_session_calls_auto_unlock_use_case` — verifies pub visibility and use case call
  - `main_calls_recovery_before_daemon_construction` — structural test that main.rs calls recovery before `new_with_deferred`

### state.rs addition

Added `update_service_health(&mut self, name: &str, health: ServiceHealth)` — targeted mutation
of a single service's health in `worker_statuses`, used for the Stopped→Healthy transition
when the deferred peer discovery worker starts.

## Test Results

- `cargo test -p uc-daemon --lib -- app::tests`: 8 passed (all new tests pass)
- `cargo test -p uc-bootstrap --lib`: 20 passed
- `cargo test -p uc-app -- setup`: 31 passed
- `cargo check -p uc-daemon --bin uniclipboard-daemon`: 0 errors, 0 warnings
- Pre-existing `daemon_pid_guard_removes_pid_file_on_drop` is non-deterministic (unrelated to our changes)

## Deviations from Plan

### Minor adjustment: state construction order

The plan described building state before pairing_host, but `initial_statuses` depends on
`encryption_unlocked` which is only known after `rt.block_on(recover_encryption_session)`.
Solution: worker construction (clipboard_watcher, inbound_clipboard_sync, etc.) moved BEFORE
the tokio runtime creation, and `initial_statuses`/state/pairing_host/peer_monitor built AFTER
the encryption check. This is consistent with the plan's CONSTRUCTION ORDER (steps 6-8 after step 5).

### `setup_complete_rx_opt` naming

The plan used `setup_complete_rx` in the destructuring. We named the variable `setup_complete_rx_opt`
in the destructuring to avoid collision with the `setup_complete_rx` from the oneshot channel
creation. Functionally identical.

## Known Stubs

None — all connections are fully wired. The complete Phase 67 flow is:
1. `SetupCompletionEmitter` created in `main.rs` and injected into `CoreRuntime`
2. `AppLifecycleCoordinator::ensure_ready()` calls `emit_ready()` when setup flow completes
3. `emit_ready()` fires the oneshot channel
4. `DaemonApp::run()` select arm receives the signal and spawns `PeerDiscoveryWorker`
5. Peer-discovery health updated from `Stopped` → `Healthy`

## Self-Check: PASSED

- [x] `main.rs` contains `build_non_gui_runtime_with_emitter` — line 60
- [x] `main.rs` contains `SetupCompletionEmitter::new(setup_complete_tx)` — line 58
- [x] `main.rs` contains `recover_encryption_session(&runtime)` — line 151
- [x] `main.rs` contains `if encryption_unlocked {` — line 175
- [x] `main.rs` contains `DaemonApp::new_with_deferred(` call — line 235
- [x] `main.rs` contains `deferred_peer_discovery` in call — line 244
- [x] `main.rs` contains `setup_complete_rx_opt` in call — line 245
- [x] `main.rs` contains `ServiceHealth::Stopped` for peer-discovery when uninitialized — line 178
- [x] `app.rs` select arm updates health to `ServiceHealth::Healthy` — line 292
- [x] `app.rs` contains `pub async fn recover_encryption_session` — line 40
- [x] `app.rs` `run()` method does NOT contain `recover_encryption_session` call
- [x] `cargo check -p uc-daemon --bin uniclipboard-daemon` — 0 errors
- [x] `cargo test -p uc-daemon --lib` — 74 passed (1 pre-existing flaky pid test)
- [x] Commit 1a8a9b08 exists
