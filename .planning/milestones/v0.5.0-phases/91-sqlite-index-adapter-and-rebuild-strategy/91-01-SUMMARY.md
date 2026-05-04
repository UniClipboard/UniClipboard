---
phase: 91-sqlite-index-adapter-and-rebuild-strategy
plan: 01
subsystem: infra
tags: [sqlite, diesel, search, hmac, inverted-index, uc-infra]

requires:
  - phase: 90-sqlite-schema-migration-and-tokenizer-pipeline
    provides: search_document/search_posting/search_index_meta schema, row structs, HKDF key derivation, HMAC term_tag helper, NFKC tokenizer

provides:
  - SqliteSearchIndex struct implementing SearchIndexPort in uc-infra::search::sqlite_index
  - index_entry() — atomic upsert with transaction: delete postings, replace document, insert postings
  - remove_entry() — atomic hard-delete for search_document and search_posting
  - get_index_meta() — lazy seeds search_index_meta via NewSearchIndexMetaRow::seed
  - search() — real SQLite posting-based AND/OR resolution, blocked/version guards, filter pipeline, deterministic ordering
  - normalize_query_terms() — whitespace-split tokenization preventing spurious whole-segment tokens
  - resolve_time_range() — preset TimeRangeFilter variants resolved to (from_ms, to_ms)

affects:
  - 91-02 (Plan 02 adds rebuild temp-table flow, wires into the same adapter)
  - 92-daemon-search-routes (wires SqliteSearchIndex into daemon HTTP and WS)

tech-stack:
  added: []
  patterns:
    - spawn_blocking pattern for Diesel/r2d2 operations from async trait methods
    - Rust-side HashSet aggregation for COUNT(DISTINCT term_tag) AND/OR semantics
    - normalize_query_terms splits on whitespace before tokenizing each word (prevents spurious combined tokens)
    - TimeRangeFilter preset resolution in adapter layer (resolve_time_range helper)
    - ensure_meta_row + load_meta two-step pattern for per-profile lazy seeding

key-files:
  created:
    - src-tauri/crates/uc-infra/src/search/sqlite_index.rs
  modified:
    - src-tauri/crates/uc-infra/src/search/mod.rs

key-decisions:
  - 'normalize_query_terms splits query string on whitespace before tokenizing each word to prevent the SearchTokenizer from generating spurious whole-segment tokens (e.g., "alpha beta" would produce ["alpha", "beta", "alpha beta"] as a 3-term AND query if the full string were passed to tokenize_segment)'
  - 'AND/OR posting aggregation done in Rust using HashSet<Vec<u8>> per entry_id rather than SQL HAVING COUNT(DISTINCT term_tag) — avoids Diesel typed query builder limitations with dynamic-length IN parameter lists'
  - 'rebuild() returns SearchError::Internal("rebuild not yet implemented (Plan 02)") — stub satisfies trait compilation while keeping Plan 01 scope clean'
  - 'spawn_blocking wraps all Diesel calls per async-safe pool usage pattern consistent with rest of uc-infra'

patterns-established:
  - 'search adapter pattern: current_profile_id() -> ensure_meta_row() -> operation helper; all methods follow this three-step entry protocol'
  - 'upsert_active_entry / delete_active_entry as single-authority write helpers; Plan 02 rebuild mirroring will call these same helpers for temp-table double-writes'

requirements-completed: [REBLD-01]

duration: 45min
completed: 2026-04-11
---

# Phase 91 Plan 01: SQLite Index Adapter Core Summary

**SqliteSearchIndex adapter implementing SearchIndexPort with live upsert/delete, lazy meta seeding, AND/OR SQLite search with blocked/version guards, and 6 real SQLite integration tests**

## Performance

- **Duration:** ~45 min
- **Started:** 2026-04-11T03:30:00Z
- **Completed:** 2026-04-11T04:16:57Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Created `SqliteSearchIndex` in `uc-infra::search::sqlite_index` as the single authoritative adapter for `SearchIndexPort`
- Implemented atomic live write path: `upsert_active_entry` deletes old postings, upserts document row, inserts new postings in one transaction
- Implemented `search()` with blocked/version guards, AND/OR term matching via Rust HashSet aggregation, time/file-type/extension filter pipeline, and deterministic ordering
- Added `resolve_time_range()` covering all 7 `TimeRangeFilter` variants including `ThisMonth` via chrono calendar
- Discovered and fixed tokenizer behavior: `tokenize_segment("alpha beta")` produces 3 tokens including a spurious whole-segment "alpha beta" compound token; fixed by splitting query string on whitespace before tokenizing each word individually

## Task Commits

1. **Task 1: Adapter module, meta-row seeding, live write/delete** - `061bec46` (feat)
2. **Task 2: Real SQLite search with blocked/version guard** - `061bec46` (feat — combined in same commit as tasks share the single new file)

**Plan metadata:** (docs commit to follow)

## Files Created/Modified

- `src-tauri/crates/uc-infra/src/search/sqlite_index.rs` — Full `SqliteSearchIndex` implementation with 6 integration tests
- `src-tauri/crates/uc-infra/src/search/mod.rs` — Added `pub mod sqlite_index;` and `pub use sqlite_index::*;`

## Decisions Made

- Rust-side HashSet aggregation for AND/OR matching instead of SQL `HAVING COUNT(DISTINCT term_tag)` — avoids Diesel typed query builder limitations with variable-length IN parameter binding
- `normalize_query_terms` splits on whitespace before tokenizing each word to prevent 3-term AND queries from a 2-word query string
- `rebuild()` stub returns `Internal` error — Plan 02 scope boundary

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added rebuild() stub to satisfy SearchIndexPort trait**

- **Found during:** Task 1 (creating SqliteSearchIndex)
- **Issue:** `SearchIndexPort` requires `rebuild()` but Plan 01 explicitly defers rebuild to Plan 02; the trait must be fully implemented for the adapter to compile
- **Fix:** Added `rebuild()` returning `SearchError::Internal("rebuild not yet implemented (Plan 02)")` — satisfies the trait boundary without implementing rebuild logic
- **Files modified:** src-tauri/crates/uc-infra/src/search/sqlite_index.rs
- **Verification:** cargo check passes; rebuild() stub does not affect any Plan 01 tests
- **Committed in:** 061bec46

---

**Total deviations:** 1 auto-fixed (Rule 2 — missing critical trait implementation)
**Impact on plan:** Necessary to compile. No scope creep; explicit stub documents Plan 02 boundary.

## Issues Encountered

- `normalize_query_terms("alpha beta")` produced 3 tokens `["alpha", "beta", "alpha beta"]` because `tokenize_segment` treats camelCase/separator-split after word-boundary split, and `split_camel_case_original` on "alpha beta" returns the whole string which `split_on_separators` does not break further. Fixed by using `split_whitespace` + `tokenize_all` per word.
- Diesel's `sql_query().bind()` returns a new generic type on each chain call, making a loop over dynamic parameter lists impossible without a concrete type. Solved by moving to Diesel's `eq_any` for BLOB term_tag matching and aggregating in Rust.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `SqliteSearchIndex` is fully wired for Phase 91 Plan 02: the `upsert_active_entry` and `delete_active_entry` helpers are the single-authority write path that Plan 02 will extend with rebuild temp-table mirroring
- `get_index_meta()` truthfully signals `search_blocked` and `index_version` so Phase 92 can auto-trigger first-unlock backfill without changing adapter semantics
- Phase 92 daemon wiring can inject `SqliteSearchIndex` as `Arc<dyn SearchIndexPort>` without any adapter changes

---

_Phase: 91-sqlite-index-adapter-and-rebuild-strategy_
_Completed: 2026-04-11_

## Self-Check: PASSED

- FOUND: src-tauri/crates/uc-infra/src/search/sqlite_index.rs
- FOUND: src-tauri/crates/uc-infra/src/search/mod.rs
- FOUND: .planning/phases/91-sqlite-index-adapter-and-rebuild-strategy/91-01-SUMMARY.md
- FOUND: commit 061bec46
- No untracked files
