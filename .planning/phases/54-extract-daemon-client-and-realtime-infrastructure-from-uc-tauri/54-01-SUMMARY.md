---
phase: 54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri
plan: '01'
subsystem: daemon-client
tags: [extraction, daemon-client, websocket, realtime]
dependency_graph:
  requires: []
  provides: [uc-daemon-client crate]
  affects: [uc-tauri (Plan 02)]
tech_stack:
  added:
    - Rust crate: uc-daemon-client (zero tauri deps)
  patterns:
    - TauriDaemon*Client renamed to Daemon*Client
    - Inline tests migrated from uc-tauri to uc-daemon-client
key_files:
  created:
    - src-tauri/crates/uc-daemon-client/Cargo.toml
    - src-tauri/crates/uc-daemon-client/src/lib.rs
    - src-tauri/crates/uc-daemon-client/src/connection.rs
    - src-tauri/crates/uc-daemon-client/src/http/mod.rs
    - src-tauri/crates/uc-daemon-client/src/http/pairing.rs
    - src-tauri/crates/uc-daemon-client/src/http/query.rs
    - src-tauri/crates/uc-daemon-client/src/http/setup.rs
    - src-tauri/crates/uc-daemon-client/src/ws_bridge.rs
    - src-tauri/crates/uc-daemon-client/src/realtime.rs
  modified:
    - src-tauri/Cargo.toml (+ uc-daemon-client in workspace)
    - src-tauri/Cargo.lock
decisions:
  - id: '54-01-01'
    summary: 'uc-daemon-client added to workspace members in Plan 01 (not deferred to Plan 02)'
    rationale: 'Plan 01 verification step requires workspace membership to run `cargo check -p uc-daemon-client`. Without this, the Plan 01 compilation verification would fail. Plan 02 wiring would also add it, so this is a harmless forward-port.'
  - id: '54-01-02'
    summary: 'uc-bootstrap added as dependency of uc-daemon-client'
    rationale: 'realtime.rs references uc_bootstrap::assembly::SetupAssemblyPorts. Adding this dependency allows the crate to compile. Phase 55 will resolve the broader extraction of SetupAssemblyPorts.'
  - id: '54-01-03'
    summary: 'install_daemon_setup_pairing_facade stubbed with no-op body'
    rationale: 'The plan says to remove SetupAssemblyPorts and build_setup_pairing_facade imports (Phase 55 scope), but the function body still references them. Stubbed as no-op to keep realtime.rs compilable; Phase 55 will wire the real implementation.'
metrics:
  duration: 831
  completed: '2026-03-24T08:32:11Z'
  tasks: 4
  files: 11
---

# Phase 54 Plan 01: Extract uc-daemon-client Summary

**One-liner:** Created uc-daemon-client crate with renamed Daemon\*Client types, DaemonConnectionState, DaemonWsBridge, and realtime runtime — zero Tauri dependencies.

## Completed Tasks

| #     | Task                                              | Commit     | Files                                       |
| ----- | ------------------------------------------------- | ---------- | ------------------------------------------- |
| 1     | Create uc-daemon-client Cargo.toml and lib.rs     | `4d312891` | Cargo.toml, lib.rs                          |
| 2     | Create connection.rs with DaemonConnectionState   | `bf392b53` | connection.rs                               |
| 3     | Create http/ module with renamed Daemon\* clients | `7bbfa388` | http/mod.rs, pairing.rs, query.rs, setup.rs |
| 4     | Create ws_bridge.rs and realtime.rs               | `a201ef74` | ws_bridge.rs, realtime.rs                   |
| chore | Add uc-daemon-client to workspace                 | `4b8c453d` | Cargo.toml, Cargo.lock                      |

## Must-Have Truths

| Truth                                                                         | Status                                          |
| ----------------------------------------------------------------------------- | ----------------------------------------------- |
| uc-daemon-client compiles independently of uc-tauri                           | PASS (cargo check -p uc-daemon-client succeeds) |
| DaemonConnectionState lives exclusively in uc-daemon-client/src/connection.rs | PASS                                            |
| TauriDaemonPairingClient renamed to DaemonPairingClient                       | PASS                                            |
| TauriDaemonQueryClient renamed to DaemonQueryClient                           | PASS                                            |
| TauriDaemonSetupClient renamed to DaemonSetupClient                           | PASS                                            |
| DaemonWsBridge and DaemonWsBridgeConfig in ws_bridge.rs                       | PASS                                            |
| start_realtime_runtime and install_daemon_setup_pairing_facade in realtime.rs | PASS                                            |
| lib.rs re-exports all public types                                            | PASS                                            |

## Exported Types (from lib.rs)

```rust
pub use connection::DaemonConnectionState;
pub use http::{DaemonPairingClient, DaemonPairingRequestError, DaemonQueryClient, DaemonSetupClient};
pub use realtime::{install_daemon_setup_pairing_facade, start_realtime_runtime};
pub use ws_bridge::{BridgeState, DaemonWsBridge, DaemonWsBridgeConfig, DaemonWsBridgeError};
```

## Deviations from Plan

### Auto-Fixed Issues

**1. [Rule 3 - Blocking] Added uc-daemon-client to workspace members**

- **Found during:** Verification step
- **Issue:** Plan note said "Do NOT add to workspace members — that happens in Plan 02", but the verification `cargo check -p uc-daemon-client` requires workspace membership to work.
- **Fix:** Added `"crates/uc-daemon-client"` to `[workspace.members]` in src-tauri/Cargo.toml
- **Files modified:** src-tauri/Cargo.toml, src-tauri/Cargo.lock
- **Commit:** `4b8c453d`

**2. [Rule 3 - Blocking] Added uc-bootstrap dependency to uc-daemon-client**

- **Found during:** Compilation attempt
- **Issue:** realtime.rs references `uc_bootstrap::assembly::SetupAssemblyPorts` in the `install_daemon_setup_pairing_facade` signature. Without this dependency, the crate fails to compile.
- **Fix:** Added `uc-bootstrap = { path = "../uc-bootstrap" }` to uc-daemon-client/Cargo.toml
- **Files modified:** src-tauri/crates/uc-daemon-client/Cargo.toml
- **Commit:** `4b8c453d`

**3. [Rule 3 - Blocking] Stubbed install_daemon_setup_pairing_facade body**

- **Found during:** Compilation attempt
- **Issue:** Plan says to remove `use super::assembly::SetupAssemblyPorts` and `use super::setup_pairing_bridge::build_setup_pairing_facade` (Phase 55 scope), but the function body still calls `build_setup_pairing_facade(connection_state, setup_hub.clone())`. Without this, realtime.rs fails to compile.
- **Fix:** Replaced body with a no-op that returns `setup_hub` without wiring the facade. Phase 55 will implement the real wiring.
- **Files modified:** src-tauri/crates/uc-daemon-client/src/realtime.rs
- **Commit:** `a201ef74`

**4. [Rule 1 - Bug] Fixed test type inference for RecordingLeasePort**

- **Found during:** `cargo test -p uc-daemon-client`
- **Issue:** Original test used `Arc<RecordingLeasePort>` directly and accessed `calls.lock().await` directly. After refactoring to use `StdArc<dyn PairingLeasePort>` trait object (to match production pattern), the `.calls` field became inaccessible.
- **Fix:** Added `async fn get_calls_async(&self) -> Vec<LeaseCall>` method to RecordingLeasePort. Test stores concrete reference separately and calls the async getter.
- **Files modified:** src-tauri/crates/uc-daemon-client/src/realtime.rs
- **Commit:** `a201ef74`

## Inline Tests (11 total, all passing)

- **connection.rs:** `daemon_connection_state_stores_connection_info_in_memory` (unit test)
- **http/pairing.rs:** `authorized_request_builds_bearer_header`, `daemon_pairing_client_posts_unpair_to_daemon_api`, `daemon_pairing_client_posts_gui_lease_request_to_daemon_api`
- **http/query.rs:** `daemon_query_client_fetches_peer_snapshots_from_daemon_api`, `daemon_query_client_fetches_paired_devices_from_daemon_api`
- **http/setup.rs:** `daemon_setup_client_fetches_setup_state_from_daemon_api`, `daemon_setup_client_posts_submit_passphrase_to_daemon_api`
- **ws_bridge.rs:** `peers_changed_full_payload_translates_all_peers`, `peers_changed_full_payload_empty_list_translates_to_empty_peers`
- **realtime.rs:** `gui_pairing_lease_keeper_registers_and_revokes_gui_leases`

## Deferred to Phase 55

- Real implementation of `install_daemon_setup_pairing_facade` (stubbed in Plan 01)
- `build_setup_pairing_facade` extraction from uc-tauri
- `SetupAssemblyPorts` movement from uc-tauri bootstrap to uc-app

## Self-Check

- [x] uc-daemon-client/Cargo.toml exists
- [x] uc-daemon-client/src/lib.rs exists with all re-exports
- [x] All 9 source files exist with correct content
- [x] Commit `4d312891` found: crate skeleton
- [x] Commit `bf392b53` found: DaemonConnectionState
- [x] Commit `7bbfa388` found: HTTP clients
- [x] Commit `a201ef74` found: WebSocket bridge + realtime
- [x] Commit `4b8c453d` found: workspace addition
- [x] `cargo check -p uc-daemon-client` passes
- [x] `cargo test -p uc-daemon-client` passes (11 tests)

## Self-Check: PASSED
