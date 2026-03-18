---
phase: 41-daemon-and-cli-skeletons
plan: 02
subsystem: daemon
tags: [unix-socket, json-rpc, tokio, daemon, graceful-shutdown]

requires:
  - phase: 41-01
    provides: DaemonWorker trait, RuntimeState, RPC types, placeholder workers, uc-bootstrap builders

provides:
  - Unix socket JSON-RPC server with ping/status/device_list dispatch
  - DaemonApp lifecycle (bind, start workers, accept loop, graceful shutdown)
  - Stale socket detection with full ping RPC verification
  - uniclipboard-daemon binary entry point

affects: [41-03, daemon-workers, cli-rpc-client]

tech-stack:
  added: []
  patterns:
    [JoinSet-based connection tracking, fail-fast socket bind, reverse-order worker shutdown]

key-files:
  created:
    - src-tauri/crates/uc-daemon/src/rpc/handler.rs
    - src-tauri/crates/uc-daemon/src/rpc/server.rs
    - src-tauri/crates/uc-daemon/src/app.rs
  modified:
    - src-tauri/crates/uc-daemon/src/rpc/mod.rs
    - src-tauri/crates/uc-daemon/src/rpc/types.rs
    - src-tauri/crates/uc-daemon/src/lib.rs
    - src-tauri/crates/uc-daemon/src/main.rs

key-decisions:
  - "Explicit tokio runtime construction (not #[tokio::main]) to avoid potential conflicts with tracing init's internal Seq runtime"
  - 'DaemonApp binds RPC socket before starting workers for fail-fast on already-running daemon'
  - "Workers stored as Vec<Arc<dyn DaemonWorker>> for tokio::spawn 'static compatibility"

patterns-established:
  - 'JoinSet connection tracking: RPC accept loop tracks spawned handlers in JoinSet, drained with 5s timeout on shutdown'
  - 'Stale socket verification: full ping RPC check (not just TCP connect) per CONTEXT.md locked decision'
  - 'Reverse-order worker shutdown: workers stopped in reverse start order for dependency-safe teardown'

requirements-completed: [DAEM-01, DAEM-02]

duration: 4min
completed: 2026-03-18
---

# Phase 41 Plan 02: RPC Server and DaemonApp Lifecycle Summary

**Unix socket JSON-RPC server with ping/status dispatch, DaemonApp lifecycle orchestration, and graceful shutdown with stale socket detection**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-18T13:57:29Z
- **Completed:** 2026-03-18T14:01:10Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- RPC server accepts connections on Unix socket, dispatches ping/status/device_list methods
- DaemonApp orchestrates full startup/shutdown lifecycle with fail-fast socket bind
- Stale socket detection sends full ping RPC for liveness verification
- Binary entry point bootstraps via build_daemon_app() and runs in explicit tokio runtime

## Task Commits

Each task was committed atomically:

1. **Task 1: Create RPC server and handler modules** - `253fe45d` (feat)
2. **Task 2: Create DaemonApp struct and main.rs entry point** - `3d810a1f` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/rpc/handler.rs` - JSON-RPC method dispatch (ping, status, device_list)
- `src-tauri/crates/uc-daemon/src/rpc/server.rs` - Unix socket accept loop, stale socket detection, connection handling
- `src-tauri/crates/uc-daemon/src/app.rs` - DaemonApp struct with run() and shutdown sequence
- `src-tauri/crates/uc-daemon/src/main.rs` - Binary entry point calling build_daemon_app() and DaemonApp::run()
- `src-tauri/crates/uc-daemon/src/rpc/mod.rs` - Added handler and server module exports
- `src-tauri/crates/uc-daemon/src/rpc/types.rs` - Added Clone derive to WorkerStatus
- `src-tauri/crates/uc-daemon/src/lib.rs` - Added app module export

## Decisions Made

- Used explicit `tokio::runtime::Builder::new_multi_thread()` instead of `#[tokio::main]` to avoid potential conflicts with tracing init's internal Seq runtime
- DaemonApp binds RPC socket BEFORE starting workers for fail-fast on already-running daemon detection
- Workers stored as `Vec<Arc<dyn DaemonWorker>>` (not Box) for tokio::spawn 'static compatibility

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added Clone derive to WorkerStatus**

- **Found during:** Task 1 (RPC server and handler)
- **Issue:** `WorkerStatus` lacked `Clone` derive, required by `to_vec()` in handler's status snapshot
- **Fix:** Added `#[derive(Clone)]` to `WorkerStatus` in types.rs
- **Files modified:** `src-tauri/crates/uc-daemon/src/rpc/types.rs`
- **Verification:** `cargo check -p uc-daemon` exits 0
- **Committed in:** `253fe45d` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minimal — single derive addition required for correctness. No scope creep.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Daemon binary compiles and links successfully
- RPC server ready for CLI client connections (Plan 03)
- All 10 unit tests pass (handler dispatch + types + state)
- Integration test (start daemon, send ping, verify pong, SIGTERM) can be added in Plan 03

---

_Phase: 41-daemon-and-cli-skeletons_
_Completed: 2026-03-18_
