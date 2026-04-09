---
phase: 38-coreruntime-extraction
plan: '03'
subsystem: infra
tags: [rust, architecture, use-cases, assembly, setup-orchestrator, hexagonal]

# Dependency graph
requires:
  - phase: 38-coreruntime-extraction
    provides: CoreRuntime struct in uc-app wrapping AppDeps; AppRuntime wraps CoreRuntime
provides:
  - CoreUseCases struct in uc-app with ~35 pure domain accessors (no Tauri dependency)
  - AppUseCases struct in uc-tauri wrapping CoreUseCases via Deref + 5 Tauri-specific accessors
  - SetupAssemblyPorts struct in assembly.rs with 5 network/external adapter ports only
  - build_setup_orchestrator standalone pub fn in assembly.rs as single composition point (RNTM-05)
  - SC#4 integration test for HostEventSetupPort emitter read-through behavior
affects:
  - 38-coreruntime-extraction
  - 39-platform-split
  - 40-uc-bootstrap

# Tech tracking
tech-stack:
  added: []
  patterns:
    - 'CoreUseCases/AppUseCases split: pure domain accessors in uc-app, Tauri-specific in uc-tauri wrapper'
    - 'Deref<Target=CoreUseCases> on AppUseCases for transparent accessor delegation'
    - 'Standalone build_setup_orchestrator fn as single composition point — no secondary wiring in runtime.rs'
    - 'SetupAssemblyPorts excludes shared-cell state — those are passed as separate params to build_setup_orchestrator'
    - 'SC#4 shared-cell pattern: Arc<RwLock<Arc<dyn Port>>> shared between CoreRuntime and HostEventSetupPort for live emitter swap'

key-files:
  created:
    - src-tauri/crates/uc-tauri/tests/usecases_accessor_test.rs
  modified:
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-app/src/lib.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/mod.rs
    - src-tauri/src/main.rs

key-decisions:
  - 'AppUseCases wraps CoreUseCases via Deref<Target=CoreUseCases> — all ~35 pure domain accessors are transparent without duplication'
  - 'SetupAssemblyPorts contains only 5 network/external adapter ports; shared-cell params (emitter_cell, lifecycle_status, watcher_control, session_ready_emitter, clipboard_integration_mode) are separate build_setup_orchestrator params'
  - 'build_setup_orchestrator extracted to assembly.rs as standalone pub fn — satisfies RNTM-05 single composition point'
  - 'NetworkDiscoveryPort and EmptyDiscoveryPort structs moved inline into assembly.rs (from_network and placeholder constructors)'
  - 'SetupRuntimePorts removed; main.rs migrated to SetupAssemblyPorts::from_network with 5 params (added device_announcer and lifecycle_emitter that were previously created inside build_setup_orchestrator)'

patterns-established:
  - 'Pure domain use cases live in uc-app (CoreUseCases); Tauri-platform use cases live in uc-tauri (AppUseCases)'
  - 'Assembly functions in assembly.rs have zero tauri imports — pure dependency construction'
  - 'Shared-cell test pattern: create Arc<RwLock<Arc<dyn Port>>>, build the port, swap cell contents, call port method, assert new emitter was reached'

requirements-completed: [RNTM-01, RNTM-05]

# Metrics
duration: 60min
completed: 2026-03-18
---

# Phase 38 Plan 03: CoreUseCases/AppUseCases Split and SetupOrchestrator Single Composition Point Summary

**CoreUseCases struct extracted to uc-app (Tauri-free), AppUseCases wraps it via Deref in uc-tauri, and build_setup_orchestrator moved to assembly.rs as the single composition point with SC#4 emitter-swap test**

## Performance

- **Duration:** ~60 min
- **Started:** 2026-03-18T06:08:12Z
- **Completed:** 2026-03-18T07:10:00Z
- **Tasks:** 2 (committed atomically in 1 commit due to interdependency)
- **Files modified:** 6

## Accomplishments

- `CoreUseCases<'a>` struct with ~35 pure domain accessors added to uc-app — zero Tauri dependency, usable from daemon/CLI
- `AppUseCases<'a>` struct in runtime.rs wraps CoreUseCases via `Deref<Target=CoreUseCases>` and adds 5 Tauri-specific accessors (`apply_autostart`, `start_clipboard_watcher`, `app_lifecycle_coordinator`, `sync_inbound_clipboard`, `sync_outbound_clipboard`)
- `SetupAssemblyPorts` struct in assembly.rs replaces `SetupRuntimePorts` — contains only 5 external adapter ports, shared-cell state passed as separate `build_setup_orchestrator` params
- `pub fn build_setup_orchestrator` extracted to assembly.rs as standalone single composition point (RNTM-05), eliminating secondary wiring in runtime.rs
- SC#4 integration test `setup_state_emission_survives_emitter_swap` verifies HostEventSetupPort sees swapped emitter via shared cell
- Known bug "setup_event_port holds stale LoggingEventEmitter" (Phase 38 deferred item) is now structurally fixed by the unified assembly point

## Task Commits

Both tasks were committed atomically due to tight interdependency (Task 1's `AppRuntime::new()` required `SetupAssemblyPorts::placeholder()` from Task 2):

1. **Tasks 1+2: Split UseCases + extract build_setup_orchestrator** - `8bd7f27b` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/mod.rs` - Added `CoreUseCases<'a>` struct with ~35 pure domain accessors
- `src-tauri/crates/uc-app/src/lib.rs` - Added `pub use usecases::CoreUseCases` re-export
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Removed `UseCases<'a>` (~700 lines), `SetupRuntimePorts`, private `build_setup_orchestrator`; added `AppUseCases<'a>` with Deref; updated `with_setup()` to use `SetupAssemblyPorts`
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` - Added `SetupAssemblyPorts`, `pub fn build_setup_orchestrator`, SC#4 test module
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` - Re-exports updated: `AppUseCases` and `SetupAssemblyPorts` in place of `UseCases` and `SetupRuntimePorts`
- `src-tauri/src/main.rs` - Migrated from `SetupRuntimePorts::from_network` to `SetupAssemblyPorts::from_network` with 5 params
- `src-tauri/crates/uc-tauri/tests/usecases_accessor_test.rs` - Updated all references from `UseCases` to `AppUseCases`

## Decisions Made

- Committed Tasks 1 and 2 atomically — `AppRuntime::new()` calls `SetupAssemblyPorts::placeholder()` which only exists after Task 2's assembly.rs changes; impossible to split into independent commits
- Used `Deref<Target=CoreUseCases>` on AppUseCases rather than delegating all 35+ methods — eliminates duplication, preserves transparent access from all call sites
- `SetupAssemblyPorts` excludes shared-cell state (`emitter_cell`, `lifecycle_status`, `watcher_control`, `clipboard_integration_mode`, `session_ready_emitter`) — these are constructed in `with_setup()` and passed as separate params to `build_setup_orchestrator`
- `NetworkDiscoveryPort` and `EmptyDiscoveryPort` structs moved inline into assembly.rs constructors, removing the need for them in runtime.rs

## Deviations from Plan

None — plan executed exactly as written. Import cleanup (removing unused `async_trait`, `Mutex`, `DiscoveredPeer` from runtime.rs) was part of the planned removal of `SetupRuntimePorts` and `UseCases`.

## Issues Encountered

- **Duplicate import**: `HostEventEmitterPort` was imported twice in assembly.rs (line 40 already imported it; SC#4 test code added it again). Fixed by removing the duplicate.
- **Unresolved space_access imports**: Inner `use` block inside `build_setup_orchestrator` tried to import types via `uc_app::usecases::DefaultSpaceAccessCryptoFactory` but they live under `uc_app::usecases::space_access::*`. Removed the inner use block entirely (function body used fully-qualified paths already).
- **Tasks 1 and 2 interdependent**: Committed atomically as one commit. Both tasks change the same files and neither compiles without the other.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- CoreUseCases available from uc-app with no Tauri dependency — ready for Phase 39 platform split and Phase 40 daemon/CLI
- Known bug "setup_event_port holds stale emitter" structurally resolved by unified assembly point
- Phase 40 (uc-bootstrap) remains high risk — verify cargo workspace configuration before planning

---

_Phase: 38-coreruntime-extraction_
_Completed: 2026-03-18_
