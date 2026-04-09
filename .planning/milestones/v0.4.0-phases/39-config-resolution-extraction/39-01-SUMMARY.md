---
phase: 39-config-resolution-extraction
plan: 01
subsystem: infra
tags: [rust, config, uc-tauri, bootstrap, error-handling]

requires:
  - phase: 38-coreruntime-extraction
    provides: assembly.rs with get_storage_paths, bootstrap/mod.rs re-export pattern

provides:
  - config_resolution.rs module with resolve_config_path, resolve_app_config, ConfigResolutionError
  - 3 tests covering path resolution and system-default fallback

affects:
  - 39-config-resolution-extraction (plan 02 — main.rs wiring uses resolve_app_config)
  - 40-uc-bootstrap (will migrate config_resolution.rs into new crate)

tech-stack:
  added: []
  patterns:
    - 'ConfigResolutionError enum with structured variants for InvalidConfig and PlatformDirsFailed'
    - 'bootstrap/mod.rs re-exports for public module API'

key-files:
  created:
    - src-tauri/crates/uc-tauri/src/bootstrap/config_resolution.rs
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/mod.rs
    - src-tauri/src/main.rs

key-decisions:
  - 'config_resolution.rs placed in uc-tauri/src/bootstrap/ alongside config.rs and assembly.rs — uc-app cannot own it because DirsAppDirsAdapter (uc-platform) cannot be a dependency of uc-app'
  - 'resolve_app_config() returns Result<AppConfig, ConfigResolutionError>, not bare AppConfig — entry points decide how to handle fatal vs recoverable errors'
  - 'ConfigResolutionError distinguishes InvalidConfig (file exists but malformed) from PlatformDirsFailed (platform dirs unavailable)'
  - 'Tests migrated to new module with CWD_TEST_LOCK static Mutex pattern preserved; main.rs retains only CORS tests'

patterns-established:
  - 'ConfigResolutionError: structured enum error type instead of anyhow — allows entry points to pattern-match on error variants'
  - 'resolve_app_config fallback chain: env var -> ancestor search -> system defaults with DirsAppDirsAdapter'

requirements-completed: [RNTM-03]

duration: 4min
completed: 2026-03-18
---

# Phase 39 Plan 01: Config Resolution Extraction Summary

**ConfigResolutionError enum + resolve_config_path/resolve_app_config extracted into testable uc-tauri bootstrap module, with 3 tests migrated from main.rs**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-18T09:11:39Z
- **Completed:** 2026-03-18T09:15:59Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments

- Created `config_resolution.rs` with `ConfigResolutionError` enum, `resolve_config_path()`, and `resolve_app_config()` — all pure functions with no Tauri API dependencies
- Added `pub mod config_resolution` and re-exports to `bootstrap/mod.rs`
- Migrated 2 existing `resolve_config_path` tests from `main.rs` and added new `test_resolve_app_config_returns_system_defaults_when_no_config_file` test — 3 tests pass
- Removed migrated tests and unused imports (`std::env`, `std::fs`, `std::sync::Mutex`, `tempfile::TempDir`) from `main.rs`; only CORS tests remain

## Task Commits

1. **Task 1: Create config_resolution.rs module** - `43dd8dea` (feat)
2. **Task 2: Migrate tests and remove from main.rs** - `234cf697` (refactor)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/config_resolution.rs` — New module: ConfigResolutionError, resolve_config_path, resolve_app_config, 3 tests
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — Added `pub mod config_resolution` and re-export line
- `src-tauri/src/main.rs` — Removed 2 migrated config path tests + CWD_TEST_LOCK + unused imports from test block

## Decisions Made

- `config_resolution.rs` stays in `uc-tauri/src/bootstrap/` (not uc-app) because `DirsAppDirsAdapter` is in `uc-platform`, which cannot be a production dependency of `uc-app`.
- `resolve_app_config()` returns `Result` so callers can decide how to handle `InvalidConfig` (halt) vs other errors.
- `ConfigResolutionError` is a typed enum (not `anyhow::Error`) to give entry points structured branching.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 02 (main.rs wiring) can now call `resolve_app_config()` instead of inline config loading logic
- `apply_profile_suffix` deduplication and `resolve_keyslot_store_vault_dir` deletion are Plan 02 scope
- `config_resolution.rs` module is ready to migrate to `uc-bootstrap` crate in Phase 40

---

_Phase: 39-config-resolution-extraction_
_Completed: 2026-03-18_

## Self-Check: PASSED

- config_resolution.rs: FOUND
- 39-01-SUMMARY.md: FOUND
- Commit 43dd8dea: FOUND
- Commit 234cf697: FOUND
