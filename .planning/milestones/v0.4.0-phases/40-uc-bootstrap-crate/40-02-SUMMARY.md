---
phase: 40-uc-bootstrap-crate
plan: 02
subsystem: infra
tags: [rust, composition-root, builders, dependency-injection, tokio]

# Dependency graph
requires:
  - phase: 40-uc-bootstrap-crate-01
    provides: uc-bootstrap crate with assembly, config_resolution, tracing, init modules
provides:
  - build_gui_app() returning GuiBootstrapContext with AppDeps, background deps, channels, orchestrators
  - build_cli_context() returning CliBootstrapContext with AppDeps only (no background workers)
  - build_daemon_app() returning DaemonBootstrapContext with AppDeps, background deps, live platform channels
  - Shared build_core() helper for tracing/config/wiring
affects: [40-uc-bootstrap-crate-03, 41-daemon-cli-skeletons]

# Tech tracking
tech-stack:
  added: []
  patterns: [scene-specific-builders, shared-build-core-helper, AppDeps-not-CoreRuntime-return]

key-files:
  created:
    - src-tauri/crates/uc-bootstrap/src/builders.rs
  modified:
    - src-tauri/crates/uc-bootstrap/src/lib.rs

key-decisions:
  - "Builders return AppDeps (not CoreRuntime) per Codex Review R1 -- callers construct CoreRuntime with appropriate emitter/lifecycle"
  - "GUI builder uses standalone tokio::runtime::Builder (not tauri::async_runtime) to keep uc-bootstrap tauri-free"
  - "PeerDirectoryPort trait import needed for local_peer_id() method resolution on Arc<Libp2pNetworkAdapter>"

patterns-established:
  - "Scene builder pattern: build_gui_app/build_cli_context/build_daemon_app share build_core() then diverge"
  - "Context structs bundle AppDeps + scene-specific deps without constructing CoreRuntime"

requirements-completed: [BOOT-02, BOOT-03, RNTM-04]

# Metrics
duration: 2min
completed: 2026-03-18
---

# Phase 40 Plan 02: Scene-Specific Builders Summary

**Three entry-point builders (GUI/CLI/daemon) sharing build_core() helper, returning AppDeps-based contexts without tauri dependency**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-18T11:09:09Z
- **Completed:** 2026-03-18T11:11:30Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- Created builders.rs with build_gui_app, build_cli_context, build_daemon_app functions
- All three builders share private build_core() for tracing init, config resolution, and dependency wiring
- GuiBootstrapContext includes full orchestrator setup (pairing, space access, key slot store)
- Zero tauri imports in builders.rs -- uses standalone tokio runtime for async blocking calls
- Full workspace compiles cleanly

## Task Commits

Each task was committed atomically:

1. **Task 1: Create builders.rs with build_gui_app, build_cli_context, and build_daemon_app** - `52d303d8` (feat)

**Plan metadata:** (pending)

## Files Created/Modified
- `src-tauri/crates/uc-bootstrap/src/builders.rs` - Scene-specific builder functions and context structs
- `src-tauri/crates/uc-bootstrap/src/lib.rs` - Added builders module declaration and re-exports

## Decisions Made
- Builders return AppDeps (not CoreRuntime) per Codex Review R1 -- preserves compatibility with AppRuntime::with_setup() which builds CoreRuntime internally
- Used standalone tokio::runtime::Builder::new_current_thread() instead of tauri::async_runtime::block_on to keep uc-bootstrap tauri-free
- Added PeerDirectoryPort trait import to resolve local_peer_id() method on Arc<Libp2pNetworkAdapter>

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added PeerDirectoryPort trait import**
- **Found during:** Task 1 (initial compilation)
- **Issue:** `local_peer_id()` is a trait method on PeerDirectoryPort, not a direct method on Libp2pNetworkAdapter -- compiler error E0599
- **Fix:** Added `use uc_core::ports::PeerDirectoryPort;` to imports
- **Files modified:** src-tauri/crates/uc-bootstrap/src/builders.rs
- **Verification:** `cargo check -p uc-bootstrap` passes
- **Committed in:** 52d303d8

**2. [Rule 1 - Bug] Removed unused PairingConfig import**
- **Found during:** Task 1 (initial compilation)
- **Issue:** Plan's import block included PairingConfig but it's only used internally by PairingOrchestrator::new()
- **Fix:** Removed from import list
- **Files modified:** src-tauri/crates/uc-bootstrap/src/builders.rs
- **Verification:** No warnings from cargo check
- **Committed in:** 52d303d8

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes necessary for clean compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- build_gui_app() ready for Plan 03 to migrate main.rs to use it
- CLI and daemon builders ready for Phase 41 skeleton crates
- uc-bootstrap has zero tauri dependency, validated by full workspace check

---
*Phase: 40-uc-bootstrap-crate*
*Completed: 2026-03-18*
