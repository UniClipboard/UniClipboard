---
phase: 46-daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri
plan: 02
subsystem: daemon-pairing-api
tags: [daemon, pairing, http-api, websocket, contracts]
requires:
  - phase: 46-01
    provides: daemon-owned pairing host and RuntimeState pairing session projection
provides:
  - daemon pairing mutation routes and lease-based host controls
  - pairing/discovery websocket incremental payload contracts
  - regression coverage for pairing API and websocket contract boundaries
affects: [phase-46-daemon-pairing-host-migration, daemon-api, websocket, tauri-bridge]
tech-stack:
  added: []
  patterns:
    [
      shared daemon pairing host facade in API state,
      lease-based discoverability/readiness controls,
      metadata-only pairing snapshots with realtime secret-bearing deltas,
    ]
key-files:
  created:
    [
      src-tauri/crates/uc-daemon/src/api/pairing.rs,
      src-tauri/crates/uc-daemon/tests/pairing_api.rs,
      src-tauri/crates/uc-daemon/tests/pairing_ws.rs,
    ]
  modified:
    [
      src-tauri/crates/uc-daemon/src/api/mod.rs,
      src-tauri/crates/uc-daemon/src/api/routes.rs,
      src-tauri/crates/uc-daemon/src/api/server.rs,
      src-tauri/crates/uc-daemon/src/api/types.rs,
      src-tauri/crates/uc-daemon/src/api/ws.rs,
      src-tauri/crates/uc-daemon/src/app.rs,
      src-tauri/crates/uc-daemon/src/pairing/host.rs,
      src-tauri/crates/uc-daemon/tests/pairing_host.rs,
    ]
key-decisions:
  - 'DaemonApiState now exposes one shared pairing_host handle so HTTP routes and websocket fanout can share the same daemon-owned control surface instead of constructing route-local state.'
  - 'Discoverability and participant readiness are modeled as lease registries keyed by client kind, allowing explicit opt-in and automatic expiry without keeping the daemon permanently discoverable.'
  - 'pairing.snapshot and pairing session HTTP reads remain metadata-only, while verification code and fingerprints are reserved for authenticated realtime websocket events.'
patterns-established:
  - 'Daemon mutation routes acknowledge commands immediately with 202 and stable error codes, leaving final session outcomes to realtime updates.'
  - 'Realtime websocket DTOs use camelCase payloads and the top-level `type` key, so the Tauri bridge can forward events without frontend contract translation drift.'
requirements-completed: [PH46-03, PH46-03A, PH46-04]
duration: 4min
completed: 2026-03-20
---

# Phase 46 Plan 02: Daemon Pairing Control Surface And Realtime Contract Summary

**Daemon pairing mutation API plus metadata-safe websocket contract for pairing and discovery events**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-20T00:15:54+08:00
- **Completed:** 2026-03-20T00:19:22+08:00
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments

- Added authenticated daemon pairing mutation routes for discoverability, participant readiness, initiate, accept, reject, cancel, and verify, all backed by one shared daemon pairing host handle.
- Extended the daemon pairing host with lease-based discoverability/readiness control and follow-up command helpers, so CLI/GUI clients can explicitly opt in and leases can expire automatically.
- Added websocket payload contracts and regression tests for pairing/discovery incremental events, while keeping snapshot and HTTP session reads metadata-only and free of verification secrets.

## Task Commits

Each task was committed atomically:

1. **Task 1: Add daemon pairing mutation routes plus explicit discoverability/readiness registration with lease semantics** - `b1398361` (impl)
2. **Task 2: Extend daemon websocket pairing/discovery topics for incremental updates without leaking verification secrets into snapshots** - `979df782` (impl)

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/api/pairing.rs` - mutation DTOs for pairing initiation, verification, discoverability, readiness, and acknowledged responses.
- `src-tauri/crates/uc-daemon/src/api/routes.rs` - authenticated pairing mutation routes with explicit 202/409/412/404/400 semantics.
- `src-tauri/crates/uc-daemon/src/api/server.rs` - shared `pairing_host` handle attached to `DaemonApiState`.
- `src-tauri/crates/uc-daemon/src/api/types.rs` - websocket incremental payload DTOs for pairing, peers, and paired-devices topics.
- `src-tauri/crates/uc-daemon/src/api/ws.rs` - websocket topic markers and snapshot/incremental contract wiring around the daemon broadcast path.
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` - lease registries plus accept/reject/cancel/verify helpers for daemon pairing follow-up commands.
- `src-tauri/crates/uc-daemon/tests/pairing_api.rs` - API regression coverage for conflict, readiness, discoverability, 404, malformed body, and lease expiry.
- `src-tauri/crates/uc-daemon/tests/pairing_ws.rs` - websocket regression coverage for secret-free snapshots, incremental verification payloads, bridge-facing peer fields, and `type` serialization.

## Decisions Made

- Kept the broadcast path generic: websocket fanout listens to one daemon `broadcast::Sender<DaemonWsEvent>`, and tests inject incremental events through that path instead of creating a second websocket-only contract.
- Treated follow-up pairing commands as explicit daemon host methods (`accept_pairing`, `reject_pairing`, `cancel_pairing`, `verify_pairing`) so HTTP route semantics are stable before Tauri bridge work begins.
- Preserved the Phase 45 sensitivity boundary by keeping verification secrets out of `RuntimeState`, `pairing.snapshot`, and `/pairing/sessions/:session_id`, even when realtime verification events are available.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Pairing API tests reused the same nested-runtime fixture problem as Wave 1**

- **Found during:** Task 1 (Add daemon pairing mutation routes plus explicit discoverability/readiness registration with lease semantics)
- **Issue:** `pairing_api` tests called daemon bootstrap inside `#[tokio::test]`, which panicked with `Cannot start a runtime from within a runtime` and then poisoned the shared fixture lock.
- **Fix:** Moved the API router fixture behind `spawn_blocking(build_api_router)` and made the global mutex recover from poison by taking `into_inner()`.
- **Files modified:** `src-tauri/crates/uc-daemon/tests/pairing_api.rs`
- **Verification:** `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- --test-threads=1`
- **Committed in:** `b1398361`

---

**2. [Rule 3 - Blocking] Generic follow-up route helper created avoidable lifetime coupling**

- **Found during:** Task 1 compile verification
- **Issue:** A generic `follow_up_pairing_command(...)` helper forced borrowed host/session lifetimes into boxed futures and blocked compilation for accept/reject/cancel routes.
- **Fix:** Replaced the helper with explicit route handlers for the three follow-up commands, preserving the same HTTP behavior while removing the lifetime complexity.
- **Files modified:** `src-tauri/crates/uc-daemon/src/api/routes.rs`
- **Verification:** `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- --test-threads=1`
- **Committed in:** `b1398361`

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocker)
**Impact on plan:** Both fixes stayed within the planned route/test scope and were necessary to reach a compilable daemon control surface.

## Issues Encountered

- The new API tests initially failed for the same fixture reason as Wave 1; this was corrected at the test boundary without changing production bootstrap semantics.
- The first route-helper abstraction introduced unnecessary lifetime complexity, so the final implementation uses explicit follow-up handlers for clarity and compiler stability.

## User Setup Required

None.

## Next Phase Readiness

- Phase 46-03 can now treat daemon HTTP mutations and websocket topics as the stable backing contract for the Tauri compatibility bridge.
- The Tauri bridge can rely on explicit `pairing.verification_required`, `pairing.failed`, `peers.changed`, `peers.name_updated`, `peers.connection_changed`, and `paired-devices.changed` event types without inventing new daemon-side payload shapes.

## Self-Check

PASSED
