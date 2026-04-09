---
phase: 52-daemon-space-access-ssot
plan: 02
subsystem: bootstrap
tags: [websocket, realtime, space-access, daemon, bootstrap, wiring]

requires:
  - phase: 52-01
    provides: SpaceAccess topic and SpaceAccessStateChangedEvent in uc-core, daemon broadcasts space_access.state_changed

provides:
  - DaemonWsBridge translates space_access.state_changed and space_access.snapshot into RealtimeEvent::SpaceAccessStateChanged
  - GuiBootstrapContext no longer carries space_access_orchestrator field
  - wiring.rs no longer spawns space_access_completion background task
  - main.rs no longer destructures or passes space_access_orchestrator
  - Full workspace compiles cleanly

affects:
  - any future phase consuming RealtimeEvent::SpaceAccessStateChanged from the bridge

tech-stack:
  added: []
  patterns:
    - "space_access.state_changed and space_access.snapshot map to the same RealtimeEvent variant (snapshot-first subscribe pattern)"
    - "GUI process holds zero SpaceAccessOrchestrator ownership — state flows daemon -> WS -> bridge -> frontend"

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs
    - src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs
    - src-tauri/crates/uc-bootstrap/src/builders.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/src/main.rs

key-decisions:
  - "space_access.snapshot and space_access.state_changed both deserialize to SpaceAccessStateChangedPayload for uniform frontend event type"
  - "SpaceAccessOrchestrator creation kept in build_gui_app — SetupAssemblyPorts still requires it for internal wiring (not in returned GuiBootstrapContext)"
  - "run_space_access_completion_loop removed from wiring.rs — space access completion state now flows exclusively via daemon WS"

requirements-completed:
  - PH52-04
  - PH52-05
  - PH52-06

duration: 9min
completed: 2026-03-23
---

# Phase 52 Plan 02: Remove GUI-Owned SpaceAccessOrchestrator and Wire DaemonWsBridge Summary

**DaemonWsBridge now translates space_access.state_changed and space_access.snapshot WS events into RealtimeEvent::SpaceAccessStateChanged; GUI process no longer instantiates SpaceAccessOrchestrator for completion loop**

## Performance

- **Duration:** ~9 min
- **Started:** 2026-03-23T13:12:17Z
- **Completed:** 2026-03-23T13:21:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Added `SpaceAccessStateChangedEvent` and `SpaceAccessStateChangedPayload` imports to `daemon_ws_bridge.rs`
- Added `space_access.state_changed` and `space_access.snapshot` match arms to `map_daemon_ws_event`
- Added `RealtimeTopic::SpaceAccess` routing in `event_topic` function
- Added `RealtimeTopic::SpaceAccess => "space-access"` in `topic_name` function
- Removed `space_access_orchestrator` field from `GuiBootstrapContext` struct in `builders.rs`
- Removed `space_access_completion` background task spawn from `start_background_tasks` in `wiring.rs`
- Removed `run_space_access_completion_loop` function definition from `wiring.rs`
- Removed `space_access_orchestrator` param from `start_background_tasks` function signature
- Removed `space_access_orchestrator` destructuring and call-site argument from `main.rs`

## Task Commits

1. **Task 1: Wire DaemonWsBridge space_access event translation** - `4992c062` (feat)
2. **Task 2: Remove GUI-owned SpaceAccessOrchestrator from bootstrap** - `40097f46` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` - Added SpaceAccess event translation arms and topic routing
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` - Added SpaceAccess arm to realtime_topic_to_str
- `src-tauri/crates/uc-bootstrap/src/builders.rs` - Removed space_access_orchestrator from GuiBootstrapContext struct and return value
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Removed space_access_completion task, run_space_access_completion_loop, and function param
- `src-tauri/src/main.rs` - Removed space_access_orchestrator destructuring and argument

## Decisions Made

- `space_access.snapshot` and `space_access.state_changed` both map to the same `RealtimeEvent::SpaceAccessStateChanged` variant — the payload shape is identical (`{ state: SpaceAccessState }`) so uniform deserialization is correct
- `SpaceAccessOrchestrator::new()` creation is kept inside `build_gui_app()` because `SetupAssemblyPorts::from_network()` still requires it for internal setup wiring; it is simply not returned in `GuiBootstrapContext`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing functionality] Added SpaceAccess arm to realtime_topic_to_str in host_event_emitter.rs**
- **Found during:** Task 1 (cargo check after adding SpaceAccess to event_topic)
- **Issue:** `realtime_topic_to_str` in `host_event_emitter.rs` had a non-exhaustive match after `RealtimeTopic::SpaceAccess` was added in Plan 01 — compiler error E0004
- **Fix:** Added `RealtimeTopic::SpaceAccess => "spaceAccess"` arm
- **Files modified:** src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs
- **Committed in:** 4992c062 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 missing exhaustive match arm)
**Impact on plan:** Minor additive fix; no scope change. Plan architecture delivered as designed.

## Issues Encountered

One pre-existing test failure found: `bootstrap::run::tests::startup_helper_rejects_healthy_but_incompatible_daemon` fails with "internal error: entered unreachable code". Verified this failure exists before our changes (tested with `git stash`). Out of scope for this plan.

## Next Phase Readiness

- GUI process now has zero direct SpaceAccessOrchestrator ownership
- Space access state flows exclusively: daemon -> WS broadcast -> DaemonWsBridge -> RealtimeEvent::SpaceAccessStateChanged -> frontend subscribers
- Phase 52 SSOT migration complete for space access state

---
*Phase: 52-daemon-space-access-ssot*
*Completed: 2026-03-23*

## Self-Check: PASSED

Files verified:
- FOUND: src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs (contains space_access.state_changed, space_access.snapshot, SpaceAccess topic routing)
- FOUND: src-tauri/crates/uc-bootstrap/src/builders.rs (GuiBootstrapContext without space_access_orchestrator)
- FOUND: src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs (no space_access_completion, no run_space_access_completion_loop)
- FOUND: src-tauri/src/main.rs (no space_access_orchestrator)

Commits verified: 4992c062, 40097f46
Cargo check: PASSED (workspace compiles cleanly)
