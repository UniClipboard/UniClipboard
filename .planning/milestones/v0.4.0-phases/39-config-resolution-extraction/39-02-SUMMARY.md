---
phase: 39-config-resolution-extraction
plan: 02
subsystem: infra
tags: [rust, config, uc-tauri, bootstrap, main]

requires:
  - phase: 39-config-resolution-extraction
    provides: config_resolution.rs module with resolve_app_config, resolve_config_path, ConfigResolutionError

provides:
  - Simplified main() delegating to resolve_app_config() — 10 lines, no inline logic
  - run_app() key_slot_store constructed from storage_paths.vault_dir — no duplicate path resolution
  - Deleted resolve_config_path, apply_profile_suffix, resolve_keyslot_store_vault_dir from main.rs

affects:
  - 40-uc-bootstrap (will migrate config_resolution.rs and assembly.rs into new crate)

tech-stack:
  added: []
  patterns:
    - 'main() as thin delegator: tracing init + resolve_app_config + run_app — no inline logic'
    - 'key_slot_store constructed from storage_paths.vault_dir to avoid duplicate path resolution'

key-files:
  created: []
  modified:
    - src-tauri/src/main.rs

key-decisions:
  - 'main.rs imports uc_tauri::bootstrap::resolve_app_config via the existing bootstrap/mod.rs re-export'
  - 'storage_paths moved before key_slot_store construction so vault_dir is available at that point'
  - 'DirsAppDirsAdapter, load_config, PathBuf, AppConfig imports cleaned up — removed unused, kept AppConfig for run_app signature'

patterns-established:
  - 'Entry point (main.rs) as zero-logic delegator — all resolution pushed to bootstrap modules'

requirements-completed: [RNTM-03]

duration: 3min
completed: 2026-03-18
---

# Phase 39 Plan 02: Wire main.rs to Config Resolution Module Summary

**main.rs reduced from 903 to 764 lines by deleting 3 duplicate functions and replacing inline config loading with resolve_app_config() delegation**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-18T09:18:05Z
- **Completed:** 2026-03-18T09:21:10Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Replaced inline config loading in `main()` (resolve path → load → fallback to system defaults block) with a single `resolve_app_config()` call — main() is now 10 lines
- Deleted `resolve_config_path()`, `apply_profile_suffix()`, and `resolve_keyslot_store_vault_dir()` from `main.rs` — all three were either extracted (Plan 01) or subsumed by existing assembly.rs functions
- Replaced inline `key_slot_store` path resolution block (lines 486-508) with `storage_paths.vault_dir.clone()` — eliminates the secondary path derivation that duplicated `get_storage_paths()` logic
- Cleaned up now-unused imports: `DirsAppDirsAdapter`, `load_config`, `AppDirsPort`, `PathBuf`, `debug` macro

## Task Commits

1. **Task 1: Replace main() config loading with resolve_app_config()** - `dcfe6099` (refactor)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `src-tauri/src/main.rs` — Deleted 3 functions + inline blocks; replaced with delegating calls; -139 lines

## Decisions Made

- Kept `use uc_core::config::AppConfig` because `run_app(config: AppConfig)` signature requires it — even though the config value now comes from `resolve_app_config()`, the type is still referenced in the function parameter.
- Moved `get_storage_paths(&config)` call before `key_slot_store` construction so `vault_dir` is available at point of use — previously `storage_paths` was computed after the now-deleted inline block.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

Minor: After removing `use uc_core::config::AppConfig` (which was in the imports I replaced), the compiler reported a missing type for `run_app(config: AppConfig)`. Fixed by adding back the `AppConfig` import. Also removed unused `PathBuf` import that the plan's import cleanup didn't explicitly mention (auto-fix Rule 3 - blocking warning turned into a deviate-free fix since it was a direct consequence of the planned changes).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 39 is now complete: config_resolution.rs module exists (Plan 01) and main.rs delegates to it (Plan 02)
- Phase 40 (uc-bootstrap) can now migrate `config_resolution.rs` and `assembly.rs` functions into a new standalone crate without Tauri dependencies
- All existing tests pass: 199 uc-tauri tests + 2 binary CORS tests

---

_Phase: 39-config-resolution-extraction_
_Completed: 2026-03-18_

## Self-Check: PASSED

- src-tauri/src/main.rs: FOUND
- 39-02-SUMMARY.md: FOUND
- Commit dcfe6099: FOUND
