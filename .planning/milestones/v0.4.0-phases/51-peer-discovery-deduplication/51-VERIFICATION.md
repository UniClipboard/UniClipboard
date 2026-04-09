---
phase: 51-peer-discovery-deduplication
verified: 2026-03-23T12:00:00Z
status: passed
score: 3/3 must-haves verified
re_verification: false
---

# Phase 51: Peer Discovery Deduplication Verification Report

**Phase Goal:** 修复 mDNS peer 发现去重 bug: get_discovered_peers 过滤 local_peer_id、daemon peers.changed 改为全量快照语义
**Verified:** 2026-03-23T12:00:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1   | `get_discovered_peers()` never returns the local peer in its result set | ✓ VERIFIED | `libp2p_network.rs:641` `.filter(|p| p.peer_id != *local_id)` applied before `.cloned().collect()` |
| 2   | daemon `peers.changed` websocket event carries a full peer list, not a single-peer increment | ✓ VERIFIED | `host.rs:982,1006` both `PeerDiscovered` and `PeerLost` branches call `get_p2p_peers_snapshot().execute()` and emit `PeersChangedFullPayload { peers }` |
| 3   | frontend `peers.changed` handler receives a complete peer array that can be used as-is for `setPeers()` | ✓ VERIFIED | `daemon_ws_bridge.rs:633` deserializes `PeersChangedFullPayload` into `PeerChangedEvent { peers: full_list }` via `match` with `warn!()` on error |

**Score:** 3/3 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` | local_peer_id filter in `get_discovered_peers()` impl | ✓ VERIFIED | Line 641: `.filter(|p| p.peer_id != *local_id)` present; test `get_discovered_peers_excludes_local_peer_id` at line 3959 passes |
| `src-tauri/crates/uc-daemon/src/pairing/host.rs` | Full-snapshot `peers.changed` emission | ✓ VERIFIED | Lines 982 and 1006: both `PeerDiscovered` and `PeerLost` call `get_p2p_peers_snapshot().execute()` with match/warn pattern |
| `src-tauri/crates/uc-daemon/src/api/types.rs` | `PeersChangedFullPayload` with `Vec<PeerSnapshotDto>` | ✓ VERIFIED | Lines 169-175: struct defined with doc comment, camelCase serde, `pub peers: Vec<PeerSnapshotDto>` |
| `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` | Updated translation for full-snapshot `peers.changed` | ✓ VERIFIED | Line 633: `serde_json::from_value::<PeersChangedFullPayload>` with `Err(e) => warn!(error = %e, ...)` — no silent `.ok()` |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| `host.rs` PeerDiscovered/PeerLost | `GetP2pPeersSnapshot` | `CoreUseCases::new(runtime.as_ref()).get_p2p_peers_snapshot().execute()` | ✓ WIRED | Lines 981-1026: both branches call use case and emit `PeersChangedFullPayload` |
| `daemon_ws_bridge.rs` | `PeerChangedEvent { peers: full_list }` | `PeersChangedFullPayload -> peers.into_iter().map(RealtimePeerSummary)` | ✓ WIRED | Lines 633-650: full translation preserving all peers from payload |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
| -------- | ------------- | ------ | ------------------ | ------ |
| `daemon_ws_bridge.rs` peers.changed arm | `payload.peers` | `PeersChangedFullPayload` deserialized from daemon WS event | Yes — full list from `get_p2p_peers_snapshot()` use case which queries DB+network | ✓ FLOWING |
| `get_p2p_peers_snapshot.rs` | `discovered`, `connected`, `paired` | `PeerDirectoryPort::get_discovered_peers()` + `get_connected_peers()` + `PairedDeviceRepositoryPort::list_all()` | Yes — real network cache + DB queries | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| `get_discovered_peers()` excludes local_peer_id | `cargo test -p uc-platform -- get_discovered_peers_excludes_local_peer_id` | 1 passed | ✓ PASS |
| `GetP2pPeersSnapshot` defense-in-depth exclusion | `cargo test -p uc-app test_snapshot_excludes_local_peer` | 1 passed | ✓ PASS |
| DaemonWsBridge translates full payload with all peers | `cargo test -p uc-tauri -- peers_changed_full_payload_translates_all_peers` | ok | ✓ PASS |
| DaemonWsBridge handles empty peer list | `cargo test -p uc-tauri -- peers_changed_full_payload_empty_list_translates_to_empty_peers` | ok | ✓ PASS |
| host.rs PeerDiscovered emits PeersChangedFullPayload | `cargo test -p uc-daemon -- peer_discovered_emits_peers_changed_full_payload_with_peer_list` | ok | ✓ PASS |
| host.rs PeerLost emits PeersChangedFullPayload | `cargo test -p uc-daemon -- peer_lost_can_emit_peers_changed_with_empty_list` | ok | ✓ PASS |
| `cargo check` clean | `cargo check` | Finished dev profile, 2 pre-existing warnings unrelated to this phase | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| PH51-01 | 51-01-PLAN.md | `get_discovered_peers()` implementation filters out `local_peer_id` so the local device never appears in its own discovered peer list | ✓ SATISFIED | Filter at line 641 of `libp2p_network.rs`; unit test `get_discovered_peers_excludes_local_peer_id` at line 3959 |
| PH51-02 | 51-01-PLAN.md | daemon `peers.changed` websocket event carries a full peer snapshot list (not a single-peer increment), matching frontend full-replacement semantics | ✓ SATISFIED | `PeersChangedFullPayload` struct in `types.rs`; `host.rs` PeerDiscovered/PeerLost emit full snapshots; `pairing_ws.rs` integration test updated |
| PH51-03 | 51-01-PLAN.md | `GetP2pPeersSnapshot` use case has defense-in-depth `local_peer_id` exclusion independent of the adapter-level filter | ✓ SATISFIED | Lines 54-57 of `get_p2p_peers_snapshot.rs`; `test_snapshot_excludes_local_peer` unit test at line 292 |

All three requirements from REQUIREMENTS.md are now satisfied. The traceability table in REQUIREMENTS.md still shows them as `Pending` — this reflects pre-verification state and can be updated by the orchestrator.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| None found in phase 51 modified files | — | — | — | — |

No `TODO`, `FIXME`, empty stubs, `unwrap()`, or silent error suppressions were introduced by this phase. The `warn!()` pattern is correctly used in both `host.rs` and `daemon_ws_bridge.rs` error arms.

### Pre-Existing Test Failures (Not Introduced by Phase 51)

Two test failures exist in the workspace but are **not caused by phase 51**:

1. **`uc-app::usecases::pairing::transport_error_test::tests::transport_error_aborts_waiting_confirm`** — file `transport_error_test.rs` was last modified in commit `6873d914` / `ff8dfb91`, both predating phase 51. No diff between phase 51 doc commit (`5c1e4e0d`) and HEAD for this file.

2. **`uc-tauri::bootstrap::run::tests::startup_helper_rejects_healthy_but_incompatible_daemon`** — file `run.rs` was last modified in phase 46.3/46.6 commits (`e6421a62`, `bde15aa8`), well before phase 51.

These are pre-existing failures unrelated to the deduplication fix and should be addressed in separate phases.

### Human Verification Required

No human verification items. All acceptance criteria are testable programmatically and pass.

### Gaps Summary

No gaps found. All three must-have truths are verified, all four artifacts pass all four levels (exists, substantive, wired, data flowing), both key links are wired, all three requirements are satisfied, and the six behavioral spot-checks pass. The phase goal is fully achieved.

---

_Verified: 2026-03-23T12:00:00Z_
_Verifier: Claude (gsd-verifier)_
