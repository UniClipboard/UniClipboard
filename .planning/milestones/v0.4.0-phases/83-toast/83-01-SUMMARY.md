---
plan: 83-01
phase: 83-toast
status: complete
completed_at: 2026-04-02
---

# Plan 83-01: Shared Utility Module and Redux discoveredPeers Foundation

## Summary

Built the shared event utility module and Redux state foundation that all subsequent plans in Phase 83 depend on.

## What Was Built

- **`src/api/daemon/events.ts`** — New shared utility module exporting:
  - `diffPeerSnapshots(nextPeers, knownPeers, callback)` — converts full peer snapshots into discovered/lost diff events (extracted from p2p.ts)
  - `classifyPairingError(rawError)` — maps error strings to typed `PairingErrorKind` (moved from p2p.ts)
  - `PeerSnapshotPeer`, `PeerDiffEvent` interfaces
  - Re-exports `PairingErrorKind` from daemon/pairing

- **`src/store/slices/devicesSlice.ts`** — Added:
  - `DiscoveredPeer` interface (`id`, `deviceName`, `device_type`)
  - `discoveredPeers: DiscoveredPeer[]` and `discoveredPeersLoading: boolean` state fields
  - `setDiscoveredPeers` action with functional updater support (fixes stale closure in useDeviceDiscovery)
  - `clearDiscoveredPeers` action
  - `updateDiscoveredPeerDeviceName` action

- **Test files created and passing:**
  - `src/api/daemon/__tests__/events.test.ts` — 9 tests for classifyPairingError and diffPeerSnapshots
  - `src/store/slices/__tests__/devicesSlice.test.ts` — 6 tests for discoveredPeers reducers (including functional updater)

## Commits

1. `arch: add daemon/events.ts shared utility module (diffPeerSnapshots, classifyPairingError)`
2. `feat: add discoveredPeers state to devicesSlice with functional updater support`

## Key Files

### key-files

- created:
  - src/api/daemon/events.ts
  - src/api/daemon/**tests**/events.test.ts
  - src/store/slices/**tests**/devicesSlice.test.ts
- modified:
  - src/store/slices/devicesSlice.ts

## Test Results

All 15 tests passing across 2 files.

## Self-Check: PASSED

- [x] `src/api/daemon/events.ts` exports `diffPeerSnapshots`, `classifyPairingError`, `PeerSnapshotPeer`, `PeerDiffEvent`, `PairingErrorKind`
- [x] `devicesSlice` has `discoveredPeers`, `discoveredPeersLoading`, functional updater support
- [x] All test scaffolds created and passing
- [x] No p2p.ts callers migrated yet (Wave 3 responsibility)
