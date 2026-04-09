---
phase: 43-unify-gui-and-cli-business-flows-to-eliminate-per-entrypoint-feature-adaptation
plan: '02'
subsystem: pairing
tags: [gui-cli-unification, pairing-aggregation, use-case-extraction]
dependency_graph:
  requires:
    - PH43-01
    - PH43-03
    - PH43-04
  provides:
    - GetP2pPeersSnapshot use case
  affects:
    - Tauri pairing commands
    - CLI devices command

tech_stack:
  added:
    - GetP2pPeersSnapshot use case (uc-app)
    - P2pPeerSnapshot DTO
  patterns:
    - Cross-port aggregation in app layer
    - Shared business logic between GUI and CLI

key_files:
  created:
    - src-tauri/crates/uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs
  modified:
    - src-tauri/crates/uc-app/src/usecases/pairing/mod.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-tauri/src/commands/pairing.rs
    - src-tauri/crates/uc-cli/src/commands/devices.rs

decisions:
  - Use both PeerDirectoryPort AND PairedDeviceRepositoryPort in GetP2pPeersSnapshot (FINDING-2 fix)
  - Preserve pairing_state and identity_fingerprint in P2pPeerSnapshot (FINDING-4 fix)
  - Extract aggregation logic from both Tauri commands into shared use case (FINDING-3 fix)

metrics:
  duration: ~5 min
  tasks_completed: 5
  files_modified: 4
  commits: 4
---

# Phase 43 Plan 02: Pairing Snapshot Unification Summary

## One-Liner

Created `GetP2pPeersSnapshot` use case that combines discovered, connected, and paired peers into unified snapshot for both GUI and CLI.

## Completed Tasks

| Task | Name                                                             | Commit   | Files                                                          |
| ---- | ---------------------------------------------------------------- | -------- | -------------------------------------------------------------- |
| 1    | Create GetP2pPeersSnapshot use case in uc-app                    | 3eecc83b | get_p2p_peers_snapshot.rs, mod.rs (pairing), mod.rs (usecases) |
| 2    | Update Tauri get_p2p_peers to use shared use case                | 239a2a8b | pairing.rs                                                     |
| 3    | Update Tauri get_paired_peers_with_status to use shared use case | 239a2a8b | pairing.rs                                                     |
| 4    | Update CLI devices command to use shared pairing snapshot        | e3c4ea4b | devices.rs                                                     |
| 5    | Add unit tests for GetP2pPeersSnapshot                           | 29987b63 | get_p2p_peers_snapshot.rs                                      |

## Key Changes

### 1. GetP2pPeersSnapshot Use Case (uc-app)

New use case combines three data sources:

- `PeerDirectoryPort::get_discovered_peers()` - discovered peers from mDNS
- `PeerDirectoryPort::get_connected_peers()` - currently connected peers
- `PairedDeviceRepositoryPort::list_all()` - persisted paired devices

Output: `P2pPeerSnapshot` containing:

- `peer_id`, `device_name`, `addresses`
- `is_paired`, `is_connected`
- `pairing_state`, `identity_fingerprint` (preserved per FINDING-4)

### 2. Tauri Commands Updated

Both `get_p2p_peers` and `get_paired_peers_with_status` now use the shared use case:

- Removed duplicate aggregation logic from command layer
- Commands are now thin - just map from use case to response model

### 3. CLI Command Updated

`devices` command now uses `GetP2pPeersSnapshot`:

- Preserves original output: `pairing_state` and `identity_fingerprint` from snapshot
- Same business logic as GUI (FINDING-4 fix)

## Acceptance Criteria Verified

- [x] GetP2pPeersSnapshot depends on BOTH PeerDirectoryPort AND PairedDeviceRepositoryPort (FINDING-2)
- [x] P2pPeerSnapshot contains pairing_state and identity_fingerprint fields (FINDING-4)
- [x] Both Tauri commands use shared use case (FINDING-3)
- [x] CLI uses same shared use case with preserved output (FINDING-4)
- [x] Unit tests exist and pass
- [x] cargo check passes for uc-app, uc-tauri, uc-cli

## Deviations from Plan

None - plan executed exactly as written.

## Commits

- 3eecc83b feat(43-02): add GetP2pPeersSnapshot use case in uc-app
- 239a2a8b refactor(43-02): update Tauri pairing commands to use shared use case
- e3c4ea4b refactor(43-02): update CLI devices command to use shared pairing snapshot
- 29987b63 test(43-02): add unit tests for GetP2pPeersSnapshot use case

## Self-Check

- [x] GetP2pPeersSnapshot.rs exists (329 lines)
- [x] Tests pass (4 tests)
- [x] cargo check passes
- [x] All commits verified

## Self-Check: PASSED
