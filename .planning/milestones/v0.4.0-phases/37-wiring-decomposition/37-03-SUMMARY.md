---
phase: 37-wiring-decomposition
plan: 03
subsystem: infra
tags: [rust, tauri, hexagonal-architecture, refactoring, wiring-decomposition]

requires:
  - phase: 37-wiring-decomposition
    provides: Plan 02 — all app.emit() calls migrated to HostEventEmitterPort; _app_handle params marked TODO(plan-03)

provides:
  - assembly.rs with zero tauri imports containing all pure dependency construction
  - wiring.rs retaining Tauri event loops and start_background_tasks
  - start_background_tasks with no AppHandle<R>/R:Runtime parameter
  - mod.rs re-exports from both modules preserving backward compat
  - lifecycle.rs updated to use bootstrap-level import path
  - ROADMAP.md Phase 37 SC#2/SC#4 updated to staged interpretation

affects: [37-phase-complete, 38-CoreRuntime-Extraction]

tech-stack:
  added: []
  patterns:
    - 'module split: assembly.rs (pure) + wiring.rs (tauri) with pub use re-exports for backward compat'
    - 'pub(crate) visibility for private assembly helpers needed by wiring.rs tests'
    - '#[cfg(test)] re-exports in wiring.rs bridge test access to moved private functions'

key-files:
  created:
    - src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/mod.rs
    - src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs
    - src-tauri/src/main.rs
    - .planning/ROADMAP.md

key-decisions:
  - 'assembly.rs uses super::wiring::BackgroundRuntimeDeps — cross-module type reference is fine in Rust same-crate modules'
  - 'Private assembly helpers made pub(crate) so wiring.rs test module can access via #[cfg(test)] re-exports'
  - 'PlatformLayer struct made pub(crate) with pub(crate) fields so wiring.rs tests can access layer fields directly'
  - 'invoke_handler stays in main.rs — generate_handler! macro requires all commands in single invocation; 3 macOS plugin commands in binary crate'

requirements-completed:
  - RNTM-02

duration: 24min
completed: 2026-03-17
---

# Phase 37 Plan 03: wiring.rs Split into assembly.rs + wiring.rs Summary

**assembly.rs created with zero tauri imports containing all pure dependency construction; start_background_tasks AppHandle parameter removed; all 211 tests pass**

## Performance

- **Duration:** ~24 min
- **Tasks:** 2
- **Files modified:** 5 (+ 1 created)

## Accomplishments

- Created `assembly.rs` with all pure dependency construction: `WiredDependencies`, `WiringError/WiringResult`, `HostEventSetupPort`, `wire_dependencies`, `wire_dependencies_with_identity_store`, `get_storage_paths`, `resolve_pairing_device_name`, `resolve_pairing_config`, `create_infra_layer`, `create_platform_layer`, `resolve_app_dirs`, `resolve_app_paths`, `apply_profile_suffix`, `get_default_app_dirs`
- `assembly.rs` has zero `use tauri` imports — verified by CI lint (only mention of `tauri::` is a doc comment)
- Removed `AppHandle<R>` / `R: Runtime` generics from `start_background_tasks`, `run_clipboard_receive_loop`, `run_pairing_event_loop`, `run_pairing_action_loop`, `handle_pairing_message`
- Updated `main.rs` to remove `Some(app.handle().clone())` argument from `start_background_tasks` call
- Updated `mod.rs` to declare `pub mod assembly` and re-export from both modules
- Updated `lifecycle.rs:18` from `crate::bootstrap::wiring::resolve_pairing_device_name` to `crate::bootstrap::resolve_pairing_device_name`
- Updated ROADMAP.md Phase 37 SC#2/SC#4 to reflect staged interpretation
- All 211 tests pass

## Task Commits

1. **Task 1: Split wiring.rs + remove AppHandle param** - `f45079e7` (refactor)
2. **Task 2: Update ROADMAP.md Phase 37 SC#2/SC#4 wording** - `d072db1a` (docs)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs` - **NEW**: Pure dependency construction module, zero tauri imports
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Removed pure assembly code; removed AppHandle from loop functions; added re-exports
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` - Added `pub mod assembly` + re-exports from assembly and wiring
- `src-tauri/crates/uc-tauri/src/adapters/lifecycle.rs` - Updated import from `::wiring::` to `::bootstrap::`
- `src-tauri/src/main.rs` - Removed `Some(app.handle().clone())` from `start_background_tasks` call
- `.planning/ROADMAP.md` - Phase 37 SC#2/SC#4 wording updated; plan checklist 3/3 complete

## Decisions Made

- `BackgroundRuntimeDeps` stays in `wiring.rs` (per plan locked decision); `WiredDependencies` in `assembly.rs` references it via `super::wiring::BackgroundRuntimeDeps` — valid cross-module reference within same crate
- Private assembly helpers (`create_db_pool`, `create_platform_layer`, `PlatformLayer`, `resolve_app_dirs`, `resolve_app_paths`, `apply_profile_suffix`, `get_default_app_dirs`) given `pub(crate)` visibility so wiring.rs test module can access them via `#[cfg(test)] pub(crate) use super::assembly::...`
- Command registration (`invoke_handler!`) stays in `main.rs` — `generate_handler!` macro requires all commands in single invocation; 3 macOS plugin commands (`enable_rounded_corners`, `enable_modern_window_style`, `reposition_traffic_lights`) are in binary crate and cannot be referenced from `uc_tauri`

## Deviations from Plan

None — plan executed exactly as written. All acceptance criteria met.

## Issues Encountered

- Tests in `wiring.rs` heavily used types (infra DB repos, platform types) that were previously in scope via wiring.rs imports but moved to `assembly.rs`. Fixed by adding missing imports to the test module and making private assembly helpers `pub(crate)`.
- Test calls to `run_clipboard_receive_loop`, `run_pairing_event_loop`, `run_pairing_action_loop`, `handle_pairing_message` used old generics (`::<tauri::test::MockRuntime>`) and `_app_handle` arguments. Removed type parameters and `None`/`Some(app_handle.clone())` arguments from all test call sites.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 37 is complete: all 3 plans executed
- assembly.rs is structurally ready for Phase 40 extraction to `uc-bootstrap` crate
- Phase 38 (CoreRuntime Extraction) can proceed

---

_Phase: 37-wiring-decomposition_
_Completed: 2026-03-17_
