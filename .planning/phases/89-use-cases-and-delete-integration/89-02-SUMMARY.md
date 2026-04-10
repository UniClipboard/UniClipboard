---
phase: 89-use-cases-and-delete-integration
plan: 02
subsystem: search
tags: [rust, search-index, delete, use-case, hexagonal-architecture, tdd]

# Dependency graph
requires:
  - phase: 88-search-domain-types
    provides: SearchIndexPort trait and SearchError types used here
provides:
  - DeleteClipboardEntry use case with optional SearchIndexPort cleanup (SIDX-02)
  - with_search_index() builder method for optional dependency injection (D-08)
  - Warn-and-continue error policy for search cleanup failures (D-07)
affects:
  - phase-92-wiring (will inject SearchIndexPort via AppDeps into DeleteClipboardEntry)
  - phase-91-search-infra (implements SearchIndexPort that will be injected here)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - 'Optional port injection via builder method (.with_search_index()) — mirrors existing .with_file_cache_dir() pattern (D-08)'
    - 'Warn-and-continue for non-authoritative cleanup — search failures logged at warn level, delete never blocked (D-07)'
    - 'SpySearchIndex test double with tokio::sync::Mutex for async mutation capture in unit tests'

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs

key-decisions:
  - 'Search cleanup placed after file cache cleanup and before selection delete — consistent with non-authoritative cleanup ordering'
  - 'Used uc_core::ports::SearchIndexPort import (re-exported from ports mod.rs) rather than full path'
  - 'SpySearchIndex uses tokio::sync::Mutex (not std::sync::Mutex) to safely capture async call args'

patterns-established:
  - 'Builder method for optional port injection: pub fn with_X(mut self, x: Arc<dyn XPort>) -> Self'
  - 'Non-authoritative cleanup: if let Some(port) = self.port.as_ref() { async { if let Err(e) = ... { warn!(...); } }.instrument(...).await; }'

requirements-completed: [SIDX-02]

# Metrics
duration: 15min
completed: 2026-04-10
---

# Phase 89 Plan 02: DeleteClipboardEntry with Optional Search Index Cleanup Summary

**DeleteClipboardEntry extended with optional SearchIndexPort via builder, synchronously removing search documents on delete with warn-and-continue error policy (SIDX-02)**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-10T14:21:00Z
- **Completed:** 2026-04-10T14:36:40Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Added `search_index: Option<Arc<dyn SearchIndexPort>>` field to `DeleteClipboardEntry` struct
- Implemented `with_search_index()` builder method mirroring the existing `with_file_cache_dir()` pattern
- Added synchronous `remove_entry(entry_id)` call inside `execute()` with instrumented span `cleanup_search_index`
- Failures from search cleanup logged at `warn!` level and do not block the delete chain (D-07)
- Three new unit tests: spy captures correct EntryId, backwards compat without port, warn-and-continue on error

## Task Commits

Each task was committed atomically:

1. **Task 1: Add optional SearchIndexPort field, builder, and synchronous cleanup call** - `7e3f7e8d` (feat)

**Plan metadata:** (docs commit follows)

_Note: TDD approach — tests written first (RED), then implementation (GREEN). Both in single commit as they compiled together._

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs` - Added SearchIndexPort field, builder, execute() cleanup call, SpySearchIndex mock, and 3 new unit tests

## Decisions Made

- Search cleanup placed after file cache cleanup (step 1b) and before authoritative deletes (step 2 onward) — consistent with non-authoritative cleanup positioning
- `uc_core::ports::SearchIndexPort` used via the re-exported path in ports mod.rs, keeping imports consistent with other port imports in the file
- `tokio::sync::Mutex` used in `SpySearchIndex` rather than `std::sync::Mutex` because `remove_entry` is an async method and must not hold a sync lock across an await point

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `DeleteClipboardEntry` now exposes `with_search_index()` — ready for Phase 92 wiring where AppDeps will inject the search port
- `CoreUseCases::delete_clipboard_entry()` in `usecases/mod.rs` was NOT modified (correctly deferred to Phase 92)
- All 10 tests in delete_clipboard_entry pass; `cargo check --workspace` is clean

---

_Phase: 89-use-cases-and-delete-integration_
_Completed: 2026-04-10_
