---
phase: 90-sqlite-schema-migration-and-tokenizer-pipeline
plan: 01
subsystem: database
tags: [diesel, sqlite, search, migration, inverted-index, hmac, profile-scoped]

# Dependency graph
requires:
  - phase: 89-use-cases-and-delete-integration
    provides: SearchDocument, SearchPosting, SearchIndexMeta domain contracts in uc-core
  - phase: 88-core-domain-and-port-contracts
    provides: SearchKey, FileType, EntryId, EventId domain types

provides:
  - Diesel migration 2026-04-11-000001_create_search_index with profile-scoped search tables
  - uc-infra::search::constants (CURRENT_INDEX_VERSION, SEARCH_FIELD_* masks)
  - uc-infra::search::rows (SearchDocumentRow, SearchPostingRow, SearchIndexMetaRow + New* + helpers)
  - Migration smoke tests asserting all three tables with profile_id, no deleted_at_ms

affects:
  - 90-02 (tokenizer pipeline needs these row types and constants)
  - 91 (SqliteSearchIndex adapter needs this schema and rows)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Profile scoping carried by adapter-owned Diesel row structs, not uc-core domain types
    - FileType stored as serde snake_case TEXT; serde_json for file_extensions JSON array
    - field_mask/term_freq stored as i32 in Diesel rows, cast from/to u8/u32 in helpers
    - Migration smoke tests use tempfile::NamedTempFile with init_db_pool (WAL requires real file)

key-files:
  created:
    - src-tauri/crates/uc-infra/migrations/2026-04-11-000001_create_search_index/up.sql
    - src-tauri/crates/uc-infra/migrations/2026-04-11-000001_create_search_index/down.sql
    - src-tauri/crates/uc-infra/src/search/mod.rs
    - src-tauri/crates/uc-infra/src/search/constants.rs
    - src-tauri/crates/uc-infra/src/search/rows.rs
  modified:
    - src-tauri/crates/uc-infra/src/db/schema.rs
    - src-tauri/crates/uc-infra/src/lib.rs

key-decisions:
  - 'Profile scoping (profile_id) is a persistence concern owned by uc-infra row structs only; uc-core SearchDocument/SearchPosting are not widened'
  - 'Hard-delete semantic enforced: no deleted_at_ms on search_document — matches resolved Phase 88 contract'
  - 'FileType serialized to TEXT via serde snake_case; file_extensions serialized as JSON array TEXT'
  - 'field_mask and term_freq stored as INTEGER (i32) with cast helpers due to Diesel SQLite type mapping'
  - 'Migration smoke tests use NamedTempFile (not :memory:) because enable_wal_mode requires a real file path'

patterns-established:
  - 'Pattern: from_domain(profile_id, &DomainType) -> infra row; to_domain() -> DomainType without profile_id'
  - 'Pattern: seed(profile_id) on NewSearchIndexMetaRow for fresh profile initialization'

requirements-completed: [SIDX-07]

# Metrics
duration: 40min
completed: 2026-04-11
---

# Phase 90 Plan 01: SQLite Schema Migration and Row Foundation Summary

**Profile-scoped SQLite search schema via Diesel migration with three tables (search_document, search_posting, search_index_meta), adapter-owned row types with from_domain/to_domain helpers, CURRENT_INDEX_VERSION constant, and 14 passing tests**

## Performance

- **Duration:** ~40 min
- **Started:** 2026-04-11T01:00:00Z
- **Completed:** 2026-04-11T01:36:11Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- Created Diesel migration with three profile-scoped search tables; enforces hard-delete (no deleted_at_ms), 32-byte term_tag BLOB CHECK constraint, and 4 query-pattern indexes
- Added uc-infra::search::constants with CURRENT_INDEX_VERSION="search-v1" and 5 distinct field-mask bits
- Added uc-infra::search::rows with 6 row types (3 queryable + 3 insertable) and domain conversion helpers that keep profile_id out of uc-core
- 4 migration smoke tests verify all three tables exist with profile_id and correct columns after fresh DB init; 8 row unit tests cover round-trips, cast safety, and seed behavior

## Task Commits

Each task was committed atomically:

1. **Task 1: Add profile-scoped search migration and Diesel schema declarations** - `5ac774b9` (feat)
2. **Task 2: Add search constants, row helpers, and migration smoke tests** - `bfc2336b` (feat)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `src-tauri/crates/uc-infra/migrations/2026-04-11-000001_create_search_index/up.sql` - Creates search_document, search_posting, search_index_meta tables and 4 indexes
- `src-tauri/crates/uc-infra/migrations/2026-04-11-000001_create_search_index/down.sql` - Drops indexes then tables in reverse order
- `src-tauri/crates/uc-infra/src/db/schema.rs` - Added three Diesel table! blocks and updated allow_tables_to_appear_in_same_query!
- `src-tauri/crates/uc-infra/src/lib.rs` - Added pub mod search
- `src-tauri/crates/uc-infra/src/search/mod.rs` - Module re-exports and 4 migration smoke tests
- `src-tauri/crates/uc-infra/src/search/constants.rs` - CURRENT_INDEX_VERSION and 5 SEARCH_FIELD_* constants with tests
- `src-tauri/crates/uc-infra/src/search/rows.rs` - 6 row types with from_domain/to_domain/seed helpers and 8 unit tests

## Decisions Made

- **Profile scoping stays in infra rows**: Adding profile_id to uc-core SearchDocument/SearchPosting was considered but rejected — it's a persistence concern. The row helpers accept profile_id as a separate parameter.
- **FileType serialization**: Used serde_json to serialize to snake_case string, then trim surrounding quotes for TEXT storage. This avoids a Display impl and stays consistent with the existing serde contract.
- **field_mask/term_freq as i32**: Diesel SQLite's Integer maps to i32. The domain uses u8/u32 respectively. The CHECK (term_freq > 0) constraint guarantees safe casting. Cast explicitly in from_domain/to_domain.
- **NamedTempFile for smoke tests**: init_db_pool calls enable_wal_mode which requires a real file path; :memory: would fail WAL setup.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## Known Stubs

None — all row helpers are fully implemented and tested.

## Next Phase Readiness

- Phase 90 Plan 02 (tokenizer pipeline) can import `uc_infra::search::constants::*` and `uc_infra::search::rows::*` immediately
- Phase 91 (SqliteSearchIndex adapter) has stable schema and row types to build against
- `CURRENT_INDEX_VERSION` is defined and ready for version-check logic in Phase 91

---

_Phase: 90-sqlite-schema-migration-and-tokenizer-pipeline_
_Completed: 2026-04-11_

## Self-Check: PASSED
