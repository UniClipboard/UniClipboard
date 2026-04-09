---
phase: 46-daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri
plan: 01
subsystem: daemon-pairing-host
tags: [daemon, pairing, runtime-state, projection]
requires:
  - phase: 45-03
    provides: daemon bootstrap connection state and read-only daemon transport foundation
provides:
  - daemon-owned pairing host task and runtime projection
  - pairing session snapshots in RuntimeState
  - uc-daemon regression coverage for host ownership and headless gating
affects: [phase-46-daemon-pairing-host-migration, daemon, pairing, runtime-state]
tech-stack:
  added: []
  patterns:
    [
      daemon-owned pairing host,
      runtime-state pairing projection,
      spawn_blocking bootstrap fixture for daemon tests,
    ]
key-files:
  created: [src-tauri/crates/uc-daemon/tests/pairing_host.rs]
  modified:
    [
      src-tauri/crates/uc-daemon/src/app.rs,
      src-tauri/crates/uc-daemon/src/main.rs,
      src-tauri/crates/uc-daemon/src/pairing/host.rs,
      src-tauri/crates/uc-daemon/src/pairing/session_projection.rs,
      src-tauri/crates/uc-daemon/src/state.rs,
    ]
key-decisions:
  - 'DaemonApp now starts one daemon-owned PairingHost alongside RPC and HTTP so pairing session lifetime is no longer tied to Tauri/webview lifetime.'
  - 'RuntimeState stores metadata-only DaemonPairingSessionSnapshot records and never stores verification secrets, keyslot files, or raw challenge bytes.'
  - 'pairing_host tests build daemon bootstrap fixtures off the async runtime via spawn_blocking instead of changing production bootstrap runtime semantics.'
patterns-established:
  - 'Long-lived daemon business loops live in uc-daemon and project summaries into RuntimeState for later API/WebSocket fanout.'
  - 'Headless pairing admission separates discoverability from participant readiness and rejects inbound work when no local participant is ready.'
requirements-completed: [PH46-01, PH46-01A, PH46-01B, PH46-02]
duration: 10min
completed: 2026-03-19
---

# Phase 46 Plan 01: Daemon Pairing Host Ownership And Runtime Projection Summary

**Daemon now owns pairing host lifetime, action/event loops, and runtime session projection**

## Performance

- **Duration:** 10 min
- **Started:** 2026-03-19T23:46:06+08:00
- **Completed:** 2026-03-19T23:56:29+08:00
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Moved live pairing host ownership into `uc-daemon`, including host startup from `DaemonApp::run()` and pairing session projection into daemon `RuntimeState`.
- Added `DaemonPairingHost` runtime behavior for single active-session gating, non-discoverable headless defaults, ready-participant admission checks, network-event retry handling, and terminal-state projection.
- Added `uc-daemon` regression tests covering single-session enforcement, non-discoverable headless startup, inbound rejection without a ready participant, disconnect continuity, and secret-free snapshot projection.

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend daemon bootstrap context so the daemon, not Tauri, owns pairing runtime dependencies** - `8d3c63ee` (impl)
2. **Task 2: Start a daemon-owned pairing host and project daemon session state into RuntimeState** - `385f534e` (impl)

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/app.rs` - starts `DaemonPairingHost` alongside RPC/HTTP workers and keeps host lifetime daemon-owned.
- `src-tauri/crates/uc-daemon/src/main.rs` - removes the unused staged-store hop from daemon app construction after host ownership moved into daemon runtime.
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` - implements daemon pairing host orchestration, gating, event loops, backoff, and session lifecycle projection.
- `src-tauri/crates/uc-daemon/src/pairing/session_projection.rs` - adds helpers for metadata-only pairing snapshot upsert/terminal/remove flows.
- `src-tauri/crates/uc-daemon/src/state.rs` - extends `RuntimeState` with `DaemonPairingSessionSnapshot` storage and lookup helpers.
- `src-tauri/crates/uc-daemon/tests/pairing_host.rs` - adds daemon-host regression coverage and async-safe bootstrap fixture creation.

## Decisions Made

- Kept daemon bootstrap runtime behavior unchanged in production and fixed the failing tests at the fixture boundary using `spawn_blocking`, because the root cause was nested runtime construction inside the test harness, not a production bug.
- Projected only metadata (`session_id`, `peer_id`, `device_name`, `state`, `updated_at_ms`) into daemon snapshots so later HTTP/WebSocket surfaces can stay within the Phase 45 sensitivity boundary.
- Started the pairing host from `DaemonApp::run()` instead of a route-local or CLI-local facade so host ownership stays singular and compatible with later API/bridge waves.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Async test fixture triggered nested Tokio runtime construction**

- **Found during:** Task 2 (Start a daemon-owned pairing host and project daemon session state into RuntimeState)
- **Issue:** `pairing_host` tests called `build_daemon_app()` inside `#[tokio::test]`, which panicked with `Cannot start a runtime from within a runtime` because daemon bootstrap uses its own current-thread runtime for async config resolution.
- **Fix:** Moved fixture construction behind `tokio::task::spawn_blocking(build_host)` so bootstrap happens outside the async test runtime while keeping production builder semantics unchanged.
- **Files modified:** `src-tauri/crates/uc-daemon/tests/pairing_host.rs`
- **Verification:** `cd src-tauri && cargo test -p uc-daemon --test pairing_host -- --test-threads=1`
- **Committed in:** `385f534e`

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** The fix stayed inside test infrastructure and did not expand production scope.

## Issues Encountered

- The first `DaemonPairingHost` test pass failed because the fixture entered daemon bootstrap from inside the Tokio test runtime. This was resolved without altering production bootstrap behavior.

## User Setup Required

None.

## Next Phase Readiness

- Phase 46-02 can attach HTTP mutation routes and websocket fanout to daemon-owned pairing state instead of continuing to hang those concerns off Tauri startup.
- Phase 46-03 can bridge GUI pairing/discovery behavior onto daemon-owned session and event streams while keeping the current frontend contract stable.

## Self-Check

PASSED
