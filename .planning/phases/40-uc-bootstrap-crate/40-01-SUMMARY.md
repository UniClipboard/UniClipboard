---
phase: 40-uc-bootstrap-crate
plan: 01
subsystem: infra
tags: [rust, crate-extraction, composition-root, tracing, idempotent-init]

# Dependency graph
requires:
  - phase: 39-config-resolution-extraction
    provides: config_resolution.rs module with resolve_app_config and ConfigResolutionError
  - phase: 37-wiring-decomposition
    provides: assembly.rs with zero tauri imports, BackgroundRuntimeDeps in wiring.rs
provides:
  - uc-bootstrap crate as workspace member with all composition root modules
  - Idempotent tracing initialization via TRACING_INITIALIZED OnceLock
  - BackgroundRuntimeDeps struct in uc-bootstrap (single definition)
  - Re-export stubs in uc-tauri for backward compatibility
affects: [40-02-PLAN, 40-03-PLAN, 41-daemon-cli-skeletons]

# Tech tracking
tech-stack:
  added: [uc-bootstrap crate]
  patterns: [re-export-stub pattern for crate extraction, OnceLock idempotent init]

key-files:
  created:
    - src-tauri/crates/uc-bootstrap/Cargo.toml
    - src-tauri/crates/uc-bootstrap/src/lib.rs
    - src-tauri/crates/uc-bootstrap/src/assembly.rs
    - src-tauri/crates/uc-bootstrap/src/config.rs
    - src-tauri/crates/uc-bootstrap/src/config_resolution.rs
    - src-tauri/crates/uc-bootstrap/src/init.rs
    - src-tauri/crates/uc-bootstrap/src/tracing.rs
  modified:
    - src-tauri/Cargo.toml
    - src-tauri/crates/uc-tauri/Cargo.toml
    - src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/config.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/config_resolution.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/init.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs

key-decisions:
  - "Assembly helpers (create_db_pool, create_platform_layer, etc.) widened to pub for cross-crate test access"
  - "PlatformLayer struct widened to pub in uc-bootstrap for downstream crate testing"
  - "LoggingEventEmitter test dependency replaced with local NoopEventEmitter stub in uc-bootstrap tests"

patterns-established:
  - "Re-export stub pattern: uc-tauri bootstrap modules become thin `pub use uc_bootstrap::module::*` stubs"
  - "Idempotent init: TRACING_INITIALIZED OnceLock guard allows safe multiple init_tracing_subscriber calls"

requirements-completed: [BOOT-01, BOOT-05]

# Metrics
duration: 14min
completed: 2026-03-18
---

# Phase 40 Plan 01: uc-bootstrap Crate Creation Summary

**uc-bootstrap crate created as sole composition root with moved assembly/config/tracing modules and idempotent tracing init**

## Performance

- **Duration:** 14 min
- **Started:** 2026-03-18T10:49:55Z
- **Completed:** 2026-03-18T11:04:00Z
- **Tasks:** 2
- **Files modified:** 16

## Accomplishments
- Created uc-bootstrap crate with dependencies on uc-core, uc-app, uc-infra, uc-platform, uc-observability
- Moved assembly.rs, config.rs, config_resolution.rs, init.rs, tracing.rs into uc-bootstrap
- Moved BackgroundRuntimeDeps from wiring.rs into uc-bootstrap/assembly.rs (single definition)
- Made tracing init idempotent with TRACING_INITIALIZED OnceLock guard
- Replaced uc-tauri bootstrap modules with thin re-export stubs
- All 14 uc-bootstrap tests pass, full workspace compiles clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Create uc-bootstrap crate scaffold with Cargo.toml and move modules** - `03f11fa7` (feat)
2. **Task 2: Update uc-tauri to re-export from uc-bootstrap and verify full workspace builds** - `15f6fd7f` (refactor)

## Files Created/Modified
- `src-tauri/crates/uc-bootstrap/Cargo.toml` - Crate manifest with all composition root deps
- `src-tauri/crates/uc-bootstrap/src/lib.rs` - Public re-exports of all moved modules
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` - Dependency wiring with BackgroundRuntimeDeps
- `src-tauri/crates/uc-bootstrap/src/config.rs` - TOML config loader
- `src-tauri/crates/uc-bootstrap/src/config_resolution.rs` - Config path resolution with fixed imports
- `src-tauri/crates/uc-bootstrap/src/init.rs` - Device name initialization
- `src-tauri/crates/uc-bootstrap/src/tracing.rs` - Idempotent tracing subscriber initialization
- `src-tauri/crates/uc-tauri/src/bootstrap/*.rs` - Replaced with thin re-export stubs

## Decisions Made
- Widened PlatformLayer and assembly helpers to `pub` in uc-bootstrap so wiring.rs tests in uc-tauri can access them via re-exports. This is acceptable because uc-bootstrap is the composition root and downstream crates (daemon, CLI) will need these helpers.
- Replaced `crate::adapters::host_event_emitter::LoggingEventEmitter` in tests with a local `NoopEventEmitter` stub to avoid cross-crate dependency on uc-tauri internals.
- Config resolution import updated from `crate::bootstrap::config::load_config` to `crate::config::load_config` for uc-bootstrap module paths.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Cleaned up unused imports in wiring.rs after BackgroundRuntimeDeps removal**
- **Found during:** Task 2
- **Issue:** Removing BackgroundRuntimeDeps struct left orphan imports (RepresentationId, SpoolRequest, RepresentationCache, SpoolManager, Libp2pNetworkAdapter) causing compiler warnings
- **Fix:** Removed the unused imports from wiring.rs
- **Files modified:** src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
- **Committed in:** 15f6fd7f (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary cleanup for clean compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- uc-bootstrap crate exists and compiles as standalone crate
- uc-tauri depends on uc-bootstrap with backward-compatible re-exports
- Ready for Plan 02: scene-specific builders (build_gui_app, build_cli_context, build_daemon_app)

---
*Phase: 40-uc-bootstrap-crate*
*Completed: 2026-03-18*
