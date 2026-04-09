---
phase: 41-daemon-and-cli-skeletons
plan: 01
subsystem: infra
tags: [daemon, cli, runtime, rpc, worker-trait, tokio]

# Dependency graph
requires:
  - phase: 40-uc-bootstrap-crate
    provides: uc-bootstrap composition root with wire_dependencies, build_setup_orchestrator, SetupAssemblyPorts
provides:
  - LoggingHostEventEmitter in uc-bootstrap for non-GUI event emission
  - build_non_gui_runtime() for constructing CoreRuntime without Tauri
  - DaemonWorker trait with async start/stop and sync health_check
  - Placeholder ClipboardWatcherWorker and PeerDiscoveryWorker
  - Shared RPC types (RpcRequest, RpcResponse, StatusResponse, WorkerStatus)
  - RuntimeState snapshot struct tracking uptime and worker health
  - uc-daemon crate in workspace with lib + bin targets
affects: [41-02-daemon-binary, 41-03-cli-skeleton]

# Tech tracking
tech-stack:
  added: [uc-daemon crate]
  patterns:
    [DaemonWorker trait with CancellationToken, snapshot-only RuntimeState, JSON-RPC 2.0 types]

key-files:
  created:
    - src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs
    - src-tauri/crates/uc-daemon/Cargo.toml
    - src-tauri/crates/uc-daemon/src/lib.rs
    - src-tauri/crates/uc-daemon/src/worker.rs
    - src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs
    - src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs
    - src-tauri/crates/uc-daemon/src/rpc/types.rs
    - src-tauri/crates/uc-daemon/src/state.rs
    - src-tauri/crates/uc-daemon/src/main.rs
  modified:
    - src-tauri/crates/uc-bootstrap/src/lib.rs
    - src-tauri/Cargo.toml

key-decisions:
  - 'ClipboardIntegrationMode::Passive used instead of Disabled (Disabled variant does not exist) -- Passive disables OS clipboard observation which is the correct non-GUI behavior'
  - 'Tests embedded inline with implementation files (Tasks 1 and 2) rather than separate Task 3 commit -- reduces redundant commit overhead'

patterns-established:
  - 'DaemonWorker trait: async start(CancellationToken), async stop(), sync health_check() -> WorkerHealth'
  - 'RuntimeState is snapshot-only -- DaemonApp owns workers and updates snapshot, no trait objects in state'
  - 'LoggingHostEventEmitter logs event_type string only, never inner payload (security)'

requirements-completed: [DAEM-03, DAEM-04]

# Metrics
duration: 7min
completed: 2026-03-18
---

# Phase 41 Plan 01: Non-GUI Runtime and Daemon Foundation Summary

**LoggingHostEventEmitter + build_non_gui_runtime() in uc-bootstrap; uc-daemon crate with DaemonWorker trait, placeholder workers, JSON-RPC types, and RuntimeState**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-18T13:47:21Z
- **Completed:** 2026-03-18T13:54:45Z
- **Tasks:** 3
- **Files modified:** 13

## Accomplishments

- LoggingHostEventEmitter implements HostEventEmitterPort without any Tauri dependency, logging only event type names (no sensitive payloads)
- build_non_gui_runtime() constructs CoreRuntime for daemon/CLI using SetupAssemblyPorts::placeholder() and existing build_setup_orchestrator() -- no CoreRuntime signature changes
- uc-daemon crate created with DaemonWorker trait, two placeholder workers (ClipboardWatcherWorker, PeerDiscoveryWorker), shared RPC types, and RuntimeState
- 7 unit tests passing: LoggingHostEventEmitter emit, RuntimeState uptime/update, RPC type serde roundtrips

## Task Commits

Each task was committed atomically:

1. **Task 1: Create LoggingHostEventEmitter and build_non_gui_runtime()** - `e88f99a5` (feat)
2. **Task 2: Create uc-daemon crate with DaemonWorker trait, workers, RPC types, RuntimeState** - `ffa0769c` (feat)
3. **Task 3: Tests** - included in Tasks 1 and 2 commits (inline tests)
4. **Cargo.lock + fmt** - `0c516e3a` (chore)

## Files Created/Modified

- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` - LoggingHostEventEmitter + build_non_gui_runtime()
- `src-tauri/crates/uc-bootstrap/src/lib.rs` - Added non_gui_runtime module and re-exports
- `src-tauri/crates/uc-daemon/Cargo.toml` - New crate manifest with lib + bin targets
- `src-tauri/crates/uc-daemon/src/lib.rs` - Module re-exports
- `src-tauri/crates/uc-daemon/src/worker.rs` - DaemonWorker trait and WorkerHealth enum
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` - Placeholder clipboard watcher
- `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` - Placeholder peer discovery
- `src-tauri/crates/uc-daemon/src/rpc/types.rs` - RpcRequest, RpcResponse, StatusResponse, WorkerStatus
- `src-tauri/crates/uc-daemon/src/state.rs` - RuntimeState with uptime tracking
- `src-tauri/crates/uc-daemon/src/main.rs` - Placeholder binary (Plan 02 fills in)
- `src-tauri/Cargo.toml` - Added uc-daemon to workspace members

## Decisions Made

- Used ClipboardIntegrationMode::Passive instead of Disabled (the Disabled variant does not exist in the codebase; Passive correctly disables OS clipboard observation for non-GUI modes)
- Embedded unit tests inline with implementation files rather than creating a separate Task 3 commit, since the plan's test content maps directly to the files created in Tasks 1 and 2
- NoopWatcherControl created locally in non_gui_runtime.rs using async_trait (matching the existing pattern in uc-tauri/bootstrap/runtime.rs)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] ClipboardIntegrationMode::Disabled does not exist**

- **Found during:** Task 1
- **Issue:** Plan specified ClipboardIntegrationMode::Disabled but the enum only has Full and Passive variants
- **Fix:** Used ClipboardIntegrationMode::Passive which correctly disables OS clipboard observation
- **Files modified:** src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs
- **Verification:** cargo check -p uc-bootstrap passes
- **Committed in:** e88f99a5

**2. [Rule 1 - Bug] WatcherControlPort uses async methods, not sync**

- **Found during:** Task 1
- **Issue:** Plan implied sync start_watching/stop_watching but actual trait has async start_watcher/stop_watcher
- **Fix:** Used #[async_trait] impl matching the actual trait signature
- **Files modified:** src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs
- **Verification:** cargo check -p uc-bootstrap passes
- **Committed in:** e88f99a5

---

**Total deviations:** 2 auto-fixed (2 bugs -- plan used outdated type information)
**Impact on plan:** Minor corrections to match actual codebase types. No scope creep.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- uc-daemon library surface ready for Plan 02 (daemon binary with DaemonApp, signal handling, RPC listener)
- build_non_gui_runtime() ready for Plan 03 (CLI skeleton using CliBootstrapContext)
- All 7 unit tests passing, full workspace compiles

---

_Phase: 41-daemon-and-cli-skeletons_
_Completed: 2026-03-18_
