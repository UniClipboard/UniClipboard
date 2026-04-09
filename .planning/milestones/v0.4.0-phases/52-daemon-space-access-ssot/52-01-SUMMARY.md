---
phase: 52-daemon-space-access-ssot
plan: 01
subsystem: api
tags: [websocket, realtime, space-access, daemon, axum, broadcast]

requires:
  - phase: 50-daemon-encryption-state-recovery
    provides: DaemonPairingHost and SpaceAccessOrchestrator wired into daemon runtime

provides:
  - SpaceAccess topic and SpaceAccessStateChanged event in uc-core RealtimeTopic/RealtimeEvent
  - SpaceAccessStateChangedPayload and SpaceAccessStateResponse DTOs in uc-daemon
  - GET /space-access/state HTTP endpoint returning current SpaceAccessState
  - WS space-access topic with snapshot-first semantics (space_access.snapshot on subscribe)
  - DaemonPairingHost broadcasts space_access.state_changed on every relevant state transition
  - DaemonApiState carries space_access_orchestrator injected from DaemonApp

affects:
  - 52-02 (uc-tauri side consuming these daemon WS/HTTP endpoints)
  - any future phase adding more space access states or transitions

tech-stack:
  added: []
  patterns:
    - "broadcast_space_access_state_changed free function reused by both method and free fn callers"
    - "DaemonApiState builder pattern (with_space_access chained after with_setup)"
    - "WS snapshot-first: build_snapshot_event returns space_access.snapshot on subscribe"

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/ports/realtime.rs
    - src-tauri/crates/uc-daemon/src/api/types.rs
    - src-tauri/crates/uc-daemon/src/api/ws.rs
    - src-tauri/crates/uc-daemon/src/api/routes.rs
    - src-tauri/crates/uc-daemon/src/api/query.rs
    - src-tauri/crates/uc-daemon/src/api/server.rs
    - src-tauri/crates/uc-daemon/src/api/event_emitter.rs
    - src-tauri/crates/uc-daemon/src/pairing/host.rs
    - src-tauri/crates/uc-daemon/src/app.rs

key-decisions:
  - "broadcast_space_access_state_changed is a free function called by DaemonPairingHost method and handle_pairing_message, avoiding duplication"
  - "Space access broadcasts placed at three transition sites in DaemonPairingHost: reset_setup_state, start_completed_host_sponsor_authorization, resolve_host_space_access_proof, apply_joiner_space_access_result"
  - "TOPIC_SPACE_ACCESS and SPACE_ACCESS_SNAPSHOT_EVENT constants exported as pub for ws.rs internal usage"
  - "space_access_orchestrator passed to run_pairing_network_event_loop and handle_pairing_message as new parameters for broadcast access"

requirements-completed:
  - PH52-01
  - PH52-02
  - PH52-03

duration: 15min
completed: 2026-03-23
---

# Phase 52 Plan 01: Daemon Space Access SSOT — Realtime + HTTP Endpoint Summary

**Daemon broadcasts space_access.state_changed WS events on every SpaceAccess transition and serves GET /space-access/state, establishing the daemon as single source of truth for space access state**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-03-23T13:00:00Z
- **Completed:** 2026-03-23T13:15:00Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments

- Added `SpaceAccess` to `RealtimeTopic` and `SpaceAccessStateChanged` to `RealtimeEvent` in uc-core
- Added `TOPIC_SPACE_ACCESS`, snapshot event constants, and snapshot-first WS topic handling in ws.rs
- Added `GET /space-access/state` HTTP endpoint returning current `SpaceAccessState` from `SpaceAccessOrchestrator`
- DaemonApiState now carries `space_access_orchestrator` via `with_space_access()` builder method
- DaemonPairingHost broadcasts `space_access.state_changed` after every state-changing dispatch: reset, sponsor authorization start, proof resolution, and joiner result application
- Unit test verifies the `DaemonWsEvent` structure for `space_access.state_changed` events

## Task Commits

1. **Task 1: SpaceAccess realtime types, daemon DTOs, HTTP endpoint, and WS topic** - `8e3da34c` (feat)
2. **Task 2: Wire DaemonPairingHost broadcast and DaemonApp state injection** - `b474ad92` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/ports/realtime.rs` - Added SpaceAccess topic, SpaceAccessStateChangedEvent, SpaceAccessStateChanged variant
- `src-tauri/crates/uc-daemon/src/api/types.rs` - Added SpaceAccessStateChangedPayload and SpaceAccessStateResponse DTOs
- `src-tauri/crates/uc-daemon/src/api/ws.rs` - Added TOPIC_SPACE_ACCESS, SPACE_ACCESS_SNAPSHOT_EVENT, topic registration and snapshot support
- `src-tauri/crates/uc-daemon/src/api/routes.rs` - Added GET /space-access/state endpoint
- `src-tauri/crates/uc-daemon/src/api/query.rs` - Added space_access_state() query method
- `src-tauri/crates/uc-daemon/src/api/server.rs` - Added space_access_orchestrator field and with_space_access() builder
- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` - Added unit test for space_access.state_changed event structure
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` - Added broadcast_space_access_state_changed, broadcast method, and three dispatch-site broadcasts
- `src-tauri/crates/uc-daemon/src/app.rs` - Added .with_space_access() chaining in DaemonApp::run()

## Decisions Made

- `broadcast_space_access_state_changed` implemented as a free function to avoid code duplication between the DaemonPairingHost method and the free function `handle_pairing_message`
- `space_access_orchestrator` parameter added to `run_pairing_network_event_loop` and `handle_pairing_message` signatures to provide access for broadcasting after state-changing operations
- Broadcast sites cover all paths where SpaceAccess state changes occur: joiner result, host proof resolution, host sponsor authorization start, and setup reset

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed start_completed_host_sponsor_authorization return type mismatch**
- **Found during:** Task 2 (DaemonPairingHost broadcast wiring)
- **Issue:** Plan showed `Ok(())` match arm but the method returns `Result<SpaceAccessState, SetupError>`, not `Result<(), SetupError>`. Pattern `Ok(())` caused type mismatch compile error.
- **Fix:** Changed `Ok(())` to `Ok(_)` to correctly ignore the returned SpaceAccessState
- **Files modified:** src-tauri/crates/uc-daemon/src/pairing/host.rs
- **Verification:** cargo check passes
- **Committed in:** b474ad92 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug - return type mismatch)
**Impact on plan:** Minor correction; no scope change. Plan architecture delivered as designed.

## Issues Encountered

None beyond the auto-fixed return type issue.

## Next Phase Readiness

- Daemon now broadcasts `space_access.state_changed` and serves `GET /space-access/state`
- Ready for Phase 52-02: uc-tauri side consuming these daemon WS/HTTP endpoints to eliminate duplicated space access state in the Tauri layer

---
*Phase: 52-daemon-space-access-ssot*
*Completed: 2026-03-23*

## Self-Check: PASSED

All 14 acceptance criteria verified:
- SpaceAccess in RealtimeTopic: PASS
- SpaceAccessStateChanged event: PASS
- SpaceAccessStateChangedPayload DTO: PASS
- SpaceAccessStateResponse DTO: PASS
- TOPIC_SPACE_ACCESS in ws.rs: PASS
- SPACE_ACCESS_SNAPSHOT_EVENT in ws.rs: PASS
- /space-access/state route: PASS
- space_access_orchestrator field in DaemonApiState: PASS
- with_space_access builder: PASS
- space_access_state query method: PASS
- broadcast_space_access_state method in host.rs: PASS
- space_access.state_changed event in host.rs: PASS
- with_space_access in DaemonApp: PASS
- unit test in event_emitter: PASS

Commits verified: 8e3da34c, b474ad92
