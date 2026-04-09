---
phase: 51-peer-discovery-deduplication
plan: 01
subsystem: network
tags: [libp2p, mdns, peer-discovery, websocket, deduplication, rust]

# Dependency graph
requires:
  - phase: 34-optimize-joinpickdevice-page-event-driven-discovery-with-scanning-ux
    provides: useDeviceDiscovery hook that consumes peers.changed events

provides:
  - local_peer_id filter in Libp2pNetworkAdapter.get_discovered_peers()
  - defense-in-depth local_peer_id filter in GetP2pPeersSnapshot.execute()
  - PeersChangedFullPayload struct with Vec<PeerSnapshotDto>
  - full-snapshot peers.changed emission on PeerDiscovered/PeerLost
  - DaemonWsBridge translation of PeersChangedFullPayload to PeerChangedEvent

affects:
  - setup/join-flow peer discovery page
  - frontend useDeviceDiscovery hook
  - daemon-to-tauri realtime event bridge

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Full-snapshot event emission replaces single-peer incremental events for peer discovery
    - Defense-in-depth local_peer_id filtering at both adapter and use-case layers

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs
    - src-tauri/crates/uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs
    - src-tauri/crates/uc-daemon/src/api/types.rs
    - src-tauri/crates/uc-daemon/src/pairing/host.rs
    - src-tauri/crates/uc-daemon/src/api/ws.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs
    - src-tauri/crates/uc-daemon/tests/pairing_ws.rs

key-decisions:
  - "PeersChangedFullPayload replaces PeerChangedPayload for peers.changed events (full-snapshot pattern)"
  - "PeerChangedPayload struct kept in types.rs for backward compatibility (still used by other code)"
  - "local_peer_id filter applied at both adapter layer (root fix) and use-case layer (defense-in-depth)"
  - "host.rs uses CoreUseCases::new(runtime.as_ref()).get_p2p_peers_snapshot() for snapshot emission"
  - "DaemonWsBridge uses match with warn!() on error - no silent .ok() for peers.changed deserialization"

patterns-established:
  - "Full-snapshot WebSocket event emission: emit complete list on change rather than single-peer increments"
  - "Defense-in-depth filtering: apply correctness filters at both infrastructure and domain layers"

requirements-completed:
  - PH51-01
  - PH51-02
  - PH51-03

# Metrics
duration: 21min
completed: 2026-03-23
---

# Phase 51 Plan 01: Peer Discovery Deduplication Fix Summary

**Fixed mDNS peer discovery duplication via local_peer_id exclusion at adapter level and full-snapshot peers.changed emission replacing single-peer incremental events**

## Performance

- **Duration:** 21 min
- **Started:** 2026-03-23T~08:44:18Z
- **Completed:** 2026-03-23T~09:05:18Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- Added `local_peer_id` filter to `Libp2pNetworkAdapter::get_discovered_peers()` preventing self-discovery in edge cases
- Added defense-in-depth local peer exclusion in `GetP2pPeersSnapshot::execute()` after fetching discovered peers
- Introduced `PeersChangedFullPayload { peers: Vec<PeerSnapshotDto> }` type for atomic full-list updates
- Modified `host.rs` `PeerDiscovered`/`PeerLost` branches to query full snapshot via `CoreUseCases::get_p2p_peers_snapshot()` and emit `PeersChangedFullPayload`
- Updated `DaemonWsBridge` to deserialize `PeersChangedFullPayload` with proper `match`/`warn!()` error handling
- Updated all tests and references to use the new full-snapshot payload format

## Task Commits

Each task was committed atomically:

1. **Task 1: Filter local_peer_id + full-snapshot peers.changed** - `8e7df7ce` (feat)
2. **Task 2: Update old payload references in tests** - `a7dcfce3` (chore)

## Files Created/Modified

- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` - Added local_peer_id filter in get_discovered_peers(), added test
- `src-tauri/crates/uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs` - Added defense-in-depth local_id filter, added test_snapshot_excludes_local_peer
- `src-tauri/crates/uc-daemon/src/api/types.rs` - Added PeersChangedFullPayload struct
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` - Updated PeerDiscovered/PeerLost to emit full snapshot, added unit tests
- `src-tauri/crates/uc-daemon/src/api/ws.rs` - Updated _event_type_markers to reference PeersChangedFullPayload
- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` - Updated peers.changed to deserialize PeersChangedFullPayload, added tests
- `src-tauri/crates/uc-daemon/tests/pairing_ws.rs` - Updated integration test to use PeersChangedFullPayload format

## Decisions Made

- **Full-snapshot pattern**: `peers.changed` now carries the complete peer list so frontend can replace state atomically, eliminating append/remove logic that caused duplicates
- **PeerChangedPayload retained**: Kept in `types.rs` since it's still used by `ws.rs` marker function and potential future use; only removed from active `peers.changed` path
- **CoreUseCases pattern in host.rs**: Used `CoreUseCases::new(runtime.as_ref()).get_p2p_peers_snapshot()` for snapshot queries, consistent with existing command pattern
- **match over .ok() in bridge**: Used `match serde_json::from_value::<PeersChangedFullPayload>` with `Err(e) => warn!()` to avoid silent failures per CLAUDE.md conventions

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Updated ws.rs _event_type_markers and pairing_ws.rs integration test**
- **Found during:** Task 2 (full compilation verification)
- **Issue:** `_event_type_markers` function in `ws.rs` and integration test in `pairing_ws.rs` still referenced old `PeerChangedPayload` for peers.changed context, causing unused import warnings and test failures
- **Fix:** Updated `ws.rs` marker function to use `PeersChangedFullPayload`, updated `pairing_ws.rs` test fixture and assertions to match new `peers.peers[N]` structure
- **Files modified:** `uc-daemon/src/api/ws.rs`, `uc-daemon/tests/pairing_ws.rs`
- **Verification:** `cargo check` clean, `cargo test` workspace passes
- **Committed in:** `a7dcfce3` (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (Rule 2 - missing update to downstream references)
**Impact on plan:** Required for correctness - old references would cause compilation warnings and integration test failures.

## Issues Encountered

- **Pre-existing pairing_api test flakiness**: `pairing_api.rs` integration tests fail when run with `--test-threads=1` due to shared state race conditions. This is a pre-existing issue unrelated to this plan's changes, confirmed by running the same tests on the unmodified phase46 branch.
- **Worktree was on `main` branch**: The agent worktree was initially on `main` (87677b87) and lacked `uc-daemon` crate. Required creating `phase46-work` branch tracking `phase46` before execution.

## Next Phase Readiness

- Peer discovery deduplication root causes fixed at backend layer per D-04 decision
- Frontend `useDeviceDiscovery` hook receives complete peer list per `peers.changed` event and can use it directly for `setPeers()` without additional deduplication logic
- No frontend changes needed (D-05: trust backend data)

---
*Phase: 51-peer-discovery-deduplication*
*Completed: 2026-03-23*
