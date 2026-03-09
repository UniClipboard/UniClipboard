---
phase: 10-boundary-repair-baseline
plan: 03
subsystem: infra
tags: [rust, hexagonal-architecture, port-eviction, uc-core, uc-platform, boundary-enforcement]
requires:
  - phase: 10-boundary-repair-baseline
    provides: phase context from 10-CONTEXT.md; command boundary fixes from 10-01
provides:
  - 6 non-domain ports evicted from uc-core to uc-platform
  - ClipboardIntegrationMode moved to uc-core as shared domain type
  - apply_autostart and start_clipboard_watcher use cases relocated to uc-platform
  - AppDeps cleaned of evicted port fields
  - Full workspace compiles with no stale uc_core::ports imports
affects: [uc-core, uc-platform, uc-app, uc-tauri, main.rs]
tech-stack:
  added: []
  patterns: [port-eviction, domain-type-promotion, wiring-facade]
key-files:
  created:
    - src-tauri/crates/uc-core/src/clipboard/integration_mode.rs
    - src-tauri/crates/uc-platform/src/ports/autostart.rs
    - src-tauri/crates/uc-platform/src/ports/ui_port.rs
    - src-tauri/crates/uc-platform/src/ports/app_dirs.rs
    - src-tauri/crates/uc-platform/src/ports/watcher_control.rs
    - src-tauri/crates/uc-platform/src/ports/identity_store.rs
    - src-tauri/crates/uc-platform/src/ports/observability.rs
    - src-tauri/crates/uc-platform/src/usecases/mod.rs
    - src-tauri/crates/uc-platform/src/usecases/apply_autostart.rs
    - src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs
  modified:
    - src-tauri/crates/uc-core/src/ports/mod.rs
    - src-tauri/crates/uc-core/src/clipboard/mod.rs
    - src-tauri/crates/uc-platform/src/ports/mod.rs
    - src-tauri/crates/uc-app/src/deps.rs
    - src-tauri/crates/uc-app/src/lib.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/integration_mode.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/src/main.rs
key-decisions:
  - Move ClipboardIntegrationMode to uc-core/clipboard/ as a domain concept (shared by uc-app and uc-platform without circular deps)
  - Add AppRuntime::wiring_deps() facade for bootstrap/wiring-level code (not command layer)
  - Add AppRuntime::new() NoopWatcherControl placeholder for test scenarios
  - StartClipboardWatcherPort trait stays in uc-core (it's a domain contract used by AppLifecycleCoordinator)
patterns-established:
  - Port eviction pattern: copy to uc-platform, delete from uc-core, update all imports
  - Domain types shared across layer boundaries belong in uc-core (not uc-platform or uc-app)
  - Bootstrap wiring code uses wiring_deps() facade; command handlers use usecases() only
requirements-completed: [BOUND-04]
duration: 35min
completed: 2026-03-06
---

# Phase 10 Plan 03: Port Layer Reorganization Summary

**Move 6 non-domain ports from uc-core to uc-platform, enforcing BOUND-04 so uc-core remains focused on domain contracts**

## Performance

- **Duration:** 35 min (including compilation error recovery)
- **Started:** 2026-03-06
- **Completed:** 2026-03-06
- **Tasks:** 2
- **Files modified:** 10 source files + 10 new files created

## Accomplishments

- Evicted 6 non-domain ports from `uc-core/src/ports/`: AutostartPort, UiPort, AppDirsPort, WatcherControlPort, IdentityStorePort, observability module.
- Created matching port files in `uc-platform/src/ports/` with proper re-exports.
- Updated `uc-platform/src/ports/mod.rs` to re-export all 6 evicted ports.
- Removed evicted port entries from `uc-core/src/ports/mod.rs` — uc-core now contains only domain-relevant contracts.
- Relocated `apply_autostart` and `start_clipboard_watcher` use cases from uc-app to uc-platform (they depend on evicted ports).
- Removed `watcher_control`, `ui_port`, `autostart` fields from `AppDeps` and `App` struct in uc-app.
- Promoted `ClipboardIntegrationMode` to `uc-core/src/clipboard/integration_mode.rs` as a shared domain type.
- Added `AppRuntime::wiring_deps()` facade for bootstrap code in main.rs.
- Fixed all stale `uc_core::ports::` imports across the workspace.
- Full workspace `cargo check` passes, all tests pass.

## Task Commits

1. **Task 1: Move port files from uc-core to uc-platform** - `ec26503` (refactor)
2. **Task 2: Update all consumers — deps, use cases, commands, bootstrap, adapters** - `e5a7631` (refactor)

## Files Created

- `src-tauri/crates/uc-core/src/clipboard/integration_mode.rs` — ClipboardIntegrationMode domain type
- `src-tauri/crates/uc-platform/src/ports/{autostart,ui_port,app_dirs,watcher_control,identity_store,observability}.rs` — Evicted ports
- `src-tauri/crates/uc-platform/src/usecases/mod.rs` — New usecases module for platform layer
- `src-tauri/crates/uc-platform/src/usecases/apply_autostart.rs` — Relocated from uc-app
- `src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs` — Relocated from uc-app

## Decisions Made

- `ClipboardIntegrationMode` promoted to uc-core so both uc-app and uc-platform can use it without circular dependencies. uc-app's `integration_mode.rs` now re-exports from uc-core.
- `StartClipboardWatcherPort` kept in uc-core because it's a domain contract consumed by `AppLifecycleCoordinator` in uc-app — evicting it would require uc-app to import from uc-platform (forbidden).
- `AppRuntime::wiring_deps()` added with explicit documentation warning against command-layer use — provides `&AppDeps` reference for bootstrap functions that need many port fields (e.g., `start_background_tasks`).

## Deviations from Plan

### Compilation Fixes (auto-resolved)

The executor agent's Task 1 commit left compilation errors in Task 2 scope:

1. `ClipboardIntegrationMode` was duplicated in uc-platform instead of being shared — fixed by promoting to uc-core.
2. `AppRuntime::new()` was missing the new `watcher_control` argument — fixed by adding `NoopWatcherControl` placeholder.
3. `AppDirsPort` trait not in scope in `wiring.rs` — fixed by adding import.
4. `uc-app/src/usecases/mod.rs` referenced deleted `start_clipboard_watcher_port.rs` — fixed by re-exporting from `uc_core::ports::`.
5. `main.rs` used `uc_core::ports::AppDirsPort` and `runtime.deps.*` directly — fixed with `uc_platform::ports::AppDirsPort` import and facade method calls.

## Verification Evidence

- `cargo check` — Finished `dev` profile with 1 warning (dead_code), 0 errors
- `cargo test` — 4 passed; 0 failed
- `grep -rn "uc_core::ports::AutostartPort|UiPort|AppDirsPort|WatcherControlPort|IdentityStorePort|observability" crates/` — NO STALE IMPORTS
- `ls uc-platform/src/ports/{autostart,ui_port,app_dirs,watcher_control,identity_store,observability}.rs` — ALL PORT FILES EXIST
- `grep -c "AutostartPort|UiPort|..." uc-core/src/ports/mod.rs` — returns 0 (REMOVED FROM CORE)

## Self-Check

PASSED

- FOUND: `ec26503` (Task 1 commit)
- FOUND: `e5a7631` (Task 2 commit)
- FOUND: All 6 port files in uc-platform/src/ports/
- FOUND: Zero stale uc_core::ports imports
- FOUND: Full workspace compiles and tests pass
