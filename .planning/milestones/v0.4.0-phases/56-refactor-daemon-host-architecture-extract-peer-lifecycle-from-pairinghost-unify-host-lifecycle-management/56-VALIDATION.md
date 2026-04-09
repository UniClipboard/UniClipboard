---
phase: 56
slug: refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-03-23
---

# Phase 56 - Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Framework**          | Rust `cargo test`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| **Config file**        | `src-tauri/Cargo.toml`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon peer_discovered_emits_peers_changed_full_payload_with_peer_list`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| **Full suite command** | `cd src-tauri && cargo check -p uc-daemon && cargo test -p uc-daemon peer_discovered_emits_peers_changed_full_payload_with_peer_list && cargo test -p uc-daemon peer_lost_can_emit_peers_changed_with_empty_list && cargo test -p uc-daemon peer_discovery_worker_starts_network_and_announces_device_name && cargo test -p uc-daemon daemon_pairing_host_starts_non_discoverable_in_headless_mode -- --test-threads=1 && cargo test -p uc-daemon daemon_pairing_host_survives_client_disconnect -- --test-threads=1 && cargo test -p uc-daemon --test pairing_ws peers_and_paired_devices_incremental_events_preserve_bridge_fields -- --exact` |
| **Estimated runtime**  | ~75 seconds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

---

## Sampling Rate

- **After every task commit:** Run a targeted passing `uc-daemon` test or `cargo check -p uc-daemon`
- **After every plan wave:** Run the targeted full-suite command above, not the red baseline `pairing_host` suite
- **Before `$gsd-verify-work`:** All targeted phase checks must be green; do not use `cargo test -p uc-daemon --test pairing_host -- --test-threads=1` as a gate until its baseline failures are fixed
- **Max feedback latency:** 75 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement                                                                                     | Test Type    | Automated Command                                                                                                                                                                                                        | File Exists | Status     |
| -------- | ---- | ---- | ----------------------------------------------------------------------------------------------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------- | ---------- |
| 56-01-01 | 01   | 1    | Rename lifecycle contract from worker to service without daemon behavior change                 | compile      | `cd src-tauri && cargo check -p uc-daemon`                                                                                                                                                                               | ✅          | ⬜ pending |
| 56-01-02 | 01   | 1    | Runtime status/query code still compiles after snapshot rename                                  | unit/compile | `cd src-tauri && cargo test -p uc-daemon rpc::handler -- --nocapture`                                                                                                                                                    | ✅          | ⬜ pending |
| 56-02-01 | 02   | 2    | `PeerDiscovered` still emits `peers.changed` full payloads after extraction                     | unit         | `cd src-tauri && cargo test -p uc-daemon peer_discovered_emits_peers_changed_full_payload_with_peer_list`                                                                                                                | ✅          | ⬜ pending |
| 56-02-02 | 02   | 2    | `PeerLost` still emits empty/full peer snapshot payloads after extraction                       | unit         | `cd src-tauri && cargo test -p uc-daemon peer_lost_can_emit_peers_changed_with_empty_list`                                                                                                                               | ✅          | ⬜ pending |
| 56-02-03 | 02   | 2    | Peer discovery still starts network and announces device name                                   | unit         | `cd src-tauri && cargo test -p uc-daemon peer_discovery_worker_starts_network_and_announces_device_name`                                                                                                                 | ✅          | ⬜ pending |
| 56-02-04 | 02   | 2    | `PeerMonitor` preserves subscribe retry/backoff and cancellation behavior                       | unit         | `cd src-tauri && cargo test -p uc-daemon peer_monitor_backoff_grows_and_caps_at_30000ms && cargo test -p uc-daemon peer_monitor_resubscribe_loop_stops_when_cancelled`                                                   | ❌ W0       | ⬜ pending |
| 56-02-05 | 02   | 2    | Bridge payload fields remain unchanged after peer extraction                                    | integration  | `cd src-tauri && cargo test -p uc-daemon --test pairing_ws peers_and_paired_devices_incremental_events_preserve_bridge_fields -- --exact`                                                                                | ✅          | ⬜ pending |
| 56-03-01 | 03   | 3    | `DaemonApp` lifecycle compiles with unified `services` list and no dedicated pairing spawn path | compile      | `cd src-tauri && cargo check -p uc-daemon`                                                                                                                                                                               | ✅          | ⬜ pending |
| 56-03-02 | 03   | 3    | Pairing host still starts and survives connection lifecycle after service unification           | integration  | `cd src-tauri && cargo test -p uc-daemon daemon_pairing_host_starts_non_discoverable_in_headless_mode -- --test-threads=1 && cargo test -p uc-daemon daemon_pairing_host_survives_client_disconnect -- --test-threads=1` | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [x] Existing daemon unit tests already cover peer websocket payload behavior that can be migrated into `PeerMonitor`
- [x] Existing websocket bridge regression `peers_and_paired_devices_incremental_events_preserve_bridge_fields` is green and available as a Phase 56 guard
- [x] `cargo check -p uc-daemon` is available as a low-latency safety gate for lifecycle wiring refactors
- [ ] Add `peer_monitor_backoff_grows_and_caps_at_30000ms` and `peer_monitor_resubscribe_loop_stops_when_cancelled` before finishing Plan 02
- [ ] Do not use `cargo test -p uc-daemon --test pairing_host -- --test-threads=1` as a phase gate until its baseline failures are fixed

---

## Manual-Only Verifications

| Behavior                                                                                                      | Requirement                    | Why Manual                                                                            | Test Instructions                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Daemon shutdown still stops all long-lived services in the expected order                                     | Lifecycle regression safety    | real async shutdown timing is awkward to assert end-to-end with current unit coverage | 1. Start daemon locally. 2. Trigger shutdown. 3. Confirm logs show service shutdown without hanging or duplicate pairing-host start errors.                                          |
| Setup/pairing HTTP flows still work after keeping typed pairing-host access outside the generic service trait | API/control boundary preserved | requires live HTTP route interaction                                                  | 1. Start daemon API. 2. Hit pairing/setup endpoints that call `accept_pairing`, `reject_pairing`, or `register_gui_participant`. 3. Confirm no `pairing_host_unavailable` responses. |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or existing coverage noted above
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all referenced regression targets
- [x] No watch-mode flags
- [x] Feedback latency < 75s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-03-23
