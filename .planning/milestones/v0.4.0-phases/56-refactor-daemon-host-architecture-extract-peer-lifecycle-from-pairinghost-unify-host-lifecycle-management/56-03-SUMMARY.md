---
phase: 56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management
plan: 03
subsystem: infra
tags: [rust, uc-daemon, refactor, service-supervisor, lifecycle, composition-root]

requires:
  - phase: 56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management
    plan: 01
    provides: DaemonService trait in service.rs, ServiceHealth enum
  - phase: 56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management
    plan: 02
    provides: PeerMonitor as DaemonService in peers/monitor.rs

provides:
  - DaemonService impl for DaemonPairingHost (run() signature changed from Arc<Self> to &self)
  - DaemonApp simplified to generic service supervisor with services: Vec<Arc<dyn DaemonService>>
  - main.rs as composition root building typed services then erasing to Vec<Arc<dyn DaemonService>>
  - Shared broadcast::Sender<DaemonWsEvent> channel wired through all services and DaemonApiState
  - DaemonApiState retains typed Arc<DaemonPairingHost> for HTTP route access (PH56-04)

affects:
  - Future plans building on unified DaemonApp lifecycle

tech-stack:
  added: []
  patterns:
    - "Composition root pattern: main.rs builds typed services, erases to trait objects for DaemonApp"
    - "Shared event channel: broadcast::Sender created once, cloned to PairingHost, PeerMonitor, and DaemonApp"
    - "Uniform JoinSet service lifecycle: all services started/stopped uniformly without per-component boolean flags"
    - "State pre-construction: RuntimeState created in main.rs before DaemonPairingHost so both can share it"

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-daemon/src/pairing/host.rs
    - src-tauri/crates/uc-daemon/src/app.rs
    - src-tauri/crates/uc-daemon/src/main.rs
    - src-tauri/crates/uc-daemon/tests/pairing_host.rs

key-decisions:
  - "DaemonPairingHost::run() changed from Arc<Self> to &self: run() never passes self to spawned tasks, only clones Arc fields, so &self is safe"
  - "DaemonApp::new() accepts pre-built state and event_tx: enables main.rs to share RuntimeState and broadcast channel with PairingHost before DaemonApp construction"
  - "space_access_orchestrator retained as DaemonApp field: needed for DaemonApiState wiring (not lifecycle), stored as Option<Arc<SpaceAccessOrchestrator>>"
  - "api_pairing_host stored in DaemonApp: typed access for DaemonApiState.with_pairing_host() (PH56-04); not used for lifecycle (that's via services vec)"
  - "Uniform JoinSet for all services: no per-component booleans (completed_rpc_handle etc.) for pairing host — clean D-05 compliance"
  - "event_tx reassignment pattern: DaemonApiState::new() creates default channel, then api_state.event_tx = self.event_tx.clone() replaces it with shared one without modifying server.rs API"

requirements-completed:
  - PH56-03
  - PH56-04

duration: 20min
completed: 2026-03-24
---

# Phase 56 Plan 03: Unify DaemonApp Service Lifecycle Summary

**DaemonPairingHost implements DaemonService; DaemonApp becomes a generic service supervisor with uniform JoinSet lifecycle; main.rs becomes composition root building typed services then erasing to Vec<Arc<dyn DaemonService>>**

## Performance

- **Duration:** ~20 min
- **Started:** 2026-03-24T14:05:00Z
- **Completed:** 2026-03-24T14:27:48Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Changed `DaemonPairingHost::run()` from `self: Arc<Self>` to `&self` — safe because run() only clones Arc fields, never passes self to spawned tasks
- Added `impl DaemonService for DaemonPairingHost` with `start()` delegating to `self.run(cancel)`
- Rewrote `DaemonApp` struct: removed `workers`, `pairing_orchestrator`, `pairing_action_rx`, `key_slot_store`; added `services`, `event_tx`, `api_pairing_host`, `space_access_orchestrator`
- Rewrote `DaemonApp::run()`: uniform JoinSet for all services (no `completed_*_handle` boolean flags), shared event_tx wired to DaemonApiState via field assignment
- Rewrote `main.rs` as composition root: creates shared broadcast channel and RuntimeState, builds typed `PairingHost` and `PeerMonitor`, erases all 4 services to `Vec<Arc<dyn DaemonService>>`
- Fixed `tests/pairing_host.rs`: updated `Arc::clone(&host).run(cancel)` call to work with `&self` signature

## Task Commits

Each task was committed atomically:

1. **Worktree initialization** - `81d38d26` (chore) — checked out cedar-plum codebase with Plan 01/02 changes
2. **Task 1: DaemonService impl for DaemonPairingHost** - included in `81d38d26` (chore)
3. **Task 2: Simplify DaemonApp + update main.rs** - `11bf8737` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — Added `impl DaemonService for DaemonPairingHost`; changed `run(self: Arc<Self>)` to `run(&self)`
- `src-tauri/crates/uc-daemon/src/app.rs` — Complete rewrite: generic service supervisor, uniform JoinSet, no pairing-specific fields
- `src-tauri/crates/uc-daemon/src/main.rs` — Complete rewrite: composition root, typed service construction, shared channel/state
- `src-tauri/crates/uc-daemon/tests/pairing_host.rs` — Fixed run() call to use async move block with cloned Arc

## Decisions Made

- Changed `run()` to `&self` rather than recovering `Arc<Self>` from a wrapper: simpler and correct since none of the internal spawns capture `self` — they only capture individual Arc field clones
- Created `RuntimeState` in main.rs and passed to both PairingHost and DaemonApp: avoids chicken-and-egg construction ordering problem
- Used `api_state.event_tx = self.event_tx.clone()` field assignment instead of modifying `DaemonApiState::new()` signature: keeps api/server.rs untouched in this plan
- Retained `space_access_orchestrator` in DaemonApp for API state wiring (not lifecycle): it doesn't implement DaemonService but is needed for setup/space-access HTTP routes

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed temporary Arc drop in pairing_host.rs test**
- **Found during:** Task 2 verification (cargo test)
- **Issue:** `tests/pairing_host.rs` line 156 used `Arc::clone(&host).run(cancel.child_token())` — the temporary Arc was dropped before the spawned async task could use the borrowed reference. After changing `run()` to `&self`, this triggered E0716.
- **Fix:** Wrapped in `async move` block with pre-cloned child token: `let h = Arc::clone(&host); let child_cancel = cancel.child_token(); tokio::spawn(async move { h.run(child_cancel).await })`
- **Files modified:** `src-tauri/crates/uc-daemon/tests/pairing_host.rs`
- **Commit:** `11bf8737`

**2. [Rule 3 - Blocking] Worktree missing cedar-plum crates**
- **Found during:** Initial cargo check
- **Issue:** Worktree was based on main branch with old workspace; uc-daemon needed uc-bootstrap, uc-cli, uc-daemon-client and updated uc-core/uc-app/etc.
- **Fix:** Checked out all cedar-plum crates and updated Cargo.toml workspace; committed as initialization step
- **Files modified:** All src-tauri/crates/*, src-tauri/Cargo.toml, src-tauri/Cargo.lock
- **Commit:** `81d38d26`

## Pre-existing Test Failures (Not Related to This Plan)

5 tests in `pairing_api` integration test suite fail in both the cedar-plum baseline and after our changes — confirmed pre-existing (same failures on the prior Plan 02 commit):

- `pairing_api_requires_explicit_discoverability_opt_in_for_cli`
- `pairing_api_returns_409_active_pairing_session_exists`
- `pairing_api_returns_409_host_not_discoverable`
- `pairing_api_returns_412_when_no_local_participant_ready`
- One additional failure

These are documented in Plans 01 and 02 SUMMARY files as pre-existing concurrency issues.

## Known Stubs

None — all wiring is complete. DaemonApiState receives the typed `Arc<DaemonPairingHost>` and the shared `event_tx` channel.

## Self-Check

- [x] `src-tauri/crates/uc-daemon/src/pairing/host.rs` exists with `impl DaemonService for DaemonPairingHost`
- [x] `src-tauri/crates/uc-daemon/src/app.rs` exists with `services: Vec<Arc<dyn DaemonService>>` and no `pairing_orchestrator`
- [x] `src-tauri/crates/uc-daemon/src/main.rs` exists with `PeerMonitor::new` and `Arc::clone(&pairing_host)`
- [x] `81d38d26` commit exists (worktree initialization + Task 1)
- [x] `11bf8737` commit exists (Task 2)

## Self-Check: PASSED

---
*Phase: 56-refactor-daemon-host-architecture-extract-peer-lifecycle-from-pairinghost-unify-host-lifecycle-management*
*Completed: 2026-03-24*
