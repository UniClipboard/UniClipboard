---
phase: 56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management
plan: 02
subsystem: infra
tags: [rust, uc-daemon, refactor, peer-lifecycle, daemon-service, network-events]

requires:
  - phase: 56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management
    plan: 01
    provides: DaemonService trait in service.rs, ServiceHealth enum

provides:
  - PeerMonitor struct in peers/monitor.rs implementing DaemonService
  - Dedicated peer lifecycle event subscription loop with retry/backoff
  - DaemonPairingHost stripped of all 5 peer lifecycle event arms
  - run_pairing_protocol_loop (renamed from run_pairing_network_event_loop)

affects:
  - 56-03-PLAN.md (next plan building on PeerMonitor as DaemonService)

tech-stack:
  added: []
  patterns:
    - "PeerMonitor as DaemonService: dedicated service for peer lifecycle WS emission, separate from pairing protocol"
    - "Peer event loop extraction: same retry/backoff pattern (250ms initial, 30s max, exponential) replicated in dedicated module"
    - "emit_ws_event helper duplicated per module rather than shared to keep modules self-contained"

key-files:
  created:
    - src-tauri/crates/uc-daemon/src/peers/mod.rs
    - src-tauri/crates/uc-daemon/src/peers/monitor.rs
  modified:
    - src-tauri/crates/uc-daemon/src/lib.rs
    - src-tauri/crates/uc-daemon/src/pairing/host.rs

key-decisions:
  - "PeerMonitor duplicates emit_ws_event and now_ms helpers rather than moving them to a shared module — keeping modules self-contained avoids circular dependencies and the helpers are tiny (~15 lines each)"
  - "run_pairing_network_event_loop renamed to run_pairing_protocol_loop per D-08 — the new name reflects that it handles only the pairing protocol state machine, not all network events"
  - "PeerMonitor unit tests use only the emit_ws_event helper and backoff function directly (no CoreRuntime mock needed) — this avoids heavy test setup while still verifying all observable contracts"

requirements-completed:
  - PH56-01

duration: 7min
completed: 2026-03-24
---

# Phase 56 Plan 02: Extract PeerMonitor from DaemonPairingHost Summary

**PeerMonitor DaemonService extracted to peers/monitor.rs handling 5 peer lifecycle events; DaemonPairingHost stripped to pairing protocol only with run_pairing_protocol_loop rename**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-24T14:04:18Z
- **Completed:** 2026-03-24T14:11:19Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Created `src/peers/mod.rs` and `src/peers/monitor.rs` with `PeerMonitor` struct implementing `DaemonService`
- PeerMonitor handles all 5 peer lifecycle network events: `PeerDiscovered`, `PeerLost`, `PeerNameUpdated`, `PeerConnected`, `PeerDisconnected` — with exact replica of the retry/backoff pattern (250ms initial, 30s max)
- Removed 5 peer lifecycle event arms from `DaemonPairingHost::run_pairing_network_event_loop`
- Renamed `run_pairing_network_event_loop` → `run_pairing_protocol_loop` per D-08
- Added `pub mod peers` to `lib.rs`; removed unused imports from `host.rs` (`PeerConnectionChangedPayload`, `PeerNameUpdatedPayload`, `PeerSnapshotDto`, `PeersChangedFullPayload`, `CoreUseCases`)
- Unit tests: backoff cap verification, cancel-aware resubscribe loop, peer event emission round-trips

## Task Commits

Each task was committed atomically:

1. **Task 1: Create PeerMonitor with extracted peer event handling** - `f214ce8a` (feat)
2. **Task 2: Remove peer event arms from DaemonPairingHost and rename network event loop** - `4356289c` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/peers/mod.rs` - New file: peers module declaration (`pub mod monitor`)
- `src-tauri/crates/uc-daemon/src/peers/monitor.rs` - New file: PeerMonitor struct implementing DaemonService with peer lifecycle event loop, backoff/retry, unit tests
- `src-tauri/crates/uc-daemon/src/lib.rs` - Added `pub mod peers`
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` - Removed 5 peer event arms, renamed function, removed unused imports

## Decisions Made

- Duplicated `emit_ws_event` and `now_ms` helpers into `peers/monitor.rs` rather than moving to a shared module — keeps modules self-contained and avoids circular dependency concerns; both helpers are ~15 lines each
- `run_pairing_network_event_loop` → `run_pairing_protocol_loop` per D-08: the old name implied "all network events" but it now processes only pairing protocol messages
- Unit tests do not require a real `CoreRuntime` — tests directly exercise the backoff function and the `emit_ws_event` helper in isolation, providing full observable contract coverage without infrastructure overhead

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Pre-existing integration test failures in `pairing_api` (5 failures) and `pairing_host` (1 failure) and `setup_api` (1 failure) are unrelated to this plan's changes — confirmed by stashing our changes and seeing the same failures on the prior commit. These are documented in the 56-01 SUMMARY as known pre-existing concurrency issues.
- All lib tests (55 total) pass; all pairing_ws, api_query, websocket_api, http_api integration tests pass.

## Next Phase Readiness

- `PeerMonitor` is a complete `DaemonService` implementor ready to be wired into `app.rs` alongside `ClipboardWatcherWorker` and `PeerDiscoveryWorker`
- `DaemonPairingHost` handles only pairing protocol events — clean separation achieved
- `cargo check -p uc-daemon` passes; lib tests all pass

## Self-Check

- [x] `src-tauri/crates/uc-daemon/src/peers/mod.rs` exists
- [x] `src-tauri/crates/uc-daemon/src/peers/monitor.rs` exists
- [x] `f214ce8a` commit exists (Task 1)
- [x] `4356289c` commit exists (Task 2)

---
*Phase: 56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management*
*Completed: 2026-03-24*
