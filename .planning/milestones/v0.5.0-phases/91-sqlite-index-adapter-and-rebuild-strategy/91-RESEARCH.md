# Phase 91: SQLite Index Adapter and Rebuild Strategy - Research

**Researched:** 2026-04-11
**Domain:** Rust infra search adapter (`uc-infra` SQLite + Diesel + rebuild coordination)
**Confidence:** HIGH

## Summary

Phase 91 should do two things and stop there:

1. Implement the real `SqliteSearchIndex` adapter in `uc-infra` so `SearchIndexPort` works against the Phase 90 schema for live writes, deletes, meta reads, and structured search.
2. Implement a truthful full-rebuild flow that blocks search for the entire rebuild window, mirrors concurrent live mutations into the rebuild workspace, and finishes without `RENAME TABLE` or any exclusive-lock rename path.

The key planning conclusion is that the current Phase 90 schema supports a no-rename rebuild, but **not** by storing two live index versions side by side in the permanent tables. `search_document` has `index_version`, but `search_posting` does not. That means a row-version multiplexing design cannot work without widening the schema again. The safe V1 approach is:

1. keep `search_document` and `search_posting` as the single serving tables
2. create per-rebuild temp tables with the same shape
3. set `search_index_meta.search_blocked = true` for the full rebuild window
4. mirror `index_entry()` and `remove_entry()` into both active tables and temp tables while rebuild is running
5. finalize by copying temp rows into the active tables inside one write transaction, then clear `search_blocked`

This matches the Phase 91 context decisions:

- no stale results during rebuild
- no deleted-entry resurrection
- blocked-on-version-mismatch
- no `RENAME TABLE` swap under WAL + `busy_timeout = 5000`

Search behavior should already be fully implemented in Phase 91 even though HTTP and UI requirements land later. `SearchIndexPort::search()` already owns query tokenization, AND/OR term matching, filter application, ordering, pagination, and blocked-state enforcement. Phase 92 should only wire that finished adapter into daemon routes and end-to-end lifecycle flows.

<user_constraints>

## User Constraints (from 91-CONTEXT.md)

### Locked Decisions

- Any manual or automatic rebuild blocks search for the full rebuild window. Returning stale pre-rebuild results is not allowed.
- A delete that happens during rebuild must be mirrored immediately into rebuild temp data. Rebuild completion must never resurrect deleted content.
- If clipboard history exists but the search index is missing or unusable, the product should auto-trigger a full rebuild on the first unlocked opportunity.
- Version mismatch blocks search immediately. No best-effort stale queries.
- Rebuild failure leaves search blocked until a later successful rebuild completes.
- Rebuild strategy must use `search_index_meta` state plus temp-table copy-in, not `RENAME TABLE`.
- New entries captured during rebuild must be double-written.
- `active_time_ms` remains the primary time axis for filtering and ordering.
- Profile isolation remains mandatory from day one.

### Planner's Discretion

- Exact temp-table naming pattern and cleanup policy
- Exact SQL shape for AND/OR posting aggregation
- Exact batching/progress cadence during rebuild
- Exact integration-test harness layout and synchronization primitives

### Deferred Ideas (OUT OF SCOPE)

- Daemon HTTP routes, lock-to-423 mapping, and WebSocket event forwarding
- Frontend rebuild/status UI
- Query grammar parsing beyond the already-landed `SearchQuery` contract

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description |
|----|-------------|
| REBLD-01 | User can trigger a full index rebuild when the encryption session is unlocked |
| REBLD-02 | Full rebuild uses version-flag atomic swap strategy (not `RENAME TABLE`) to avoid exclusive lock contention |
| REBLD-03 | New entries captured during a rebuild window are double-written to both active and temp tables |

</phase_requirements>

## What Already Exists

### Landed Search Foundation

- `src-tauri/crates/uc-core/src/ports/search/search_index.rs` already locks the adapter contract: `index_entry`, `remove_entry`, `search`, `rebuild`, and `get_index_meta`.
- `src-tauri/crates/uc-core/src/search/query.rs` already defines the full query shape, including AND/OR, time range, file types, file extensions, limit, and offset.
- `src-tauri/crates/uc-core/src/search/result.rs` already defines `SearchResult`, `RebuildStage`, and `RebuildProgress`.
- `src-tauri/crates/uc-infra/src/search/pipeline.rs` already builds `SearchDocument` plus `Vec<SearchPosting>` ready for storage.
- `src-tauri/crates/uc-infra/src/search/search_key_derivation.rs` already implements HKDF-based `SearchKeyDerivationPort` and a `term_tag()` helper over `SearchKey`.
- `src-tauri/crates/uc-infra/src/search/rows.rs` already owns Diesel row types plus `NewSearchIndexMetaRow::seed(profile_id)`.

### Pool and Locking Reality

- `src-tauri/crates/uc-infra/src/db/pool.rs` enables WAL once before pool creation.
- Every pool connection gets `PRAGMA busy_timeout = 5000` and `PRAGMA foreign_keys = ON`.
- The pool builder does not override max size. Inference: the default r2d2 pool sizing applies, so Phase 91 should not depend on a custom single-connection or high-concurrency pool setup.

### Practical Consequence

Because readers can stay open under WAL, `RENAME TABLE` is the dangerous step, not ordinary insert/update/delete traffic. A write-transaction copy-in strategy is compatible with the current pool setup; an exclusive-lock rename strategy is not.

## Recommended Adapter Shape

Phase 91 should add one new adapter module:

```text
src-tauri/crates/uc-infra/src/search/
├── mod.rs
├── constants.rs
├── pipeline.rs
├── rows.rs
├── search_key_derivation.rs
├── text_extractor.rs
├── tokenizer.rs
└── sqlite_index.rs
```

Recommended concrete boundary in `sqlite_index.rs`:

```text
pub struct SqliteSearchIndex {
    pool: DbPool,
    key_scope: Arc<dyn KeyScopePort>,
    search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    rebuild_state: Arc<RwLock<Option<ActiveRebuild>>>,
}
```

Recommended internal helper ownership:

- `ActiveRebuild` owns the current profile-scoped temp-table names and target version.
- profile lookup lives in one helper (`current_profile_id()`), not scattered across methods.
- meta-row seeding/loading lives in one helper (`ensure_meta_row()` / `load_meta()`).
- active-table upsert/delete helpers are the single source of truth for both normal writes and rebuild mirroring.

This keeps the adapter's mutable coordination state in one place instead of spreading `if rebuilding` branches across every SQL callsite.

## Search Execution Guidance

### 1. Meta and blocked-state gate

Before any query:

1. resolve current `profile_id`
2. ensure a meta row exists for that profile
3. load `SearchIndexMeta`
4. if `search_blocked` is already `true`, return `SearchError::IndexNotReady`
5. if `meta.index_version != CURRENT_INDEX_VERSION`, update `search_blocked = true` for that profile and return `SearchError::IndexNotReady`

That gives Phase 91 a truthful stale-index guard even before Phase 92 adds auto-rebuild routing.

### 2. Query token preparation

- reject an empty or all-noise query with `SearchError::InvalidQuery("query produced no searchable terms")`
- tokenize with the existing `SearchTokenizer`
- de-duplicate normalized tokens for AND/OR matching
- derive `SearchKey` through `SearchKeyDerivationPort`
- compute query term tags with `term_tag(&search_key, token)`

### 3. Candidate resolution in SQLite

The cleanest V1 approach is:

- scope every query by `profile_id`
- query `search_posting` first to get candidate `entry_id` values plus hit counts
- for AND mode: `GROUP BY entry_id HAVING COUNT(DISTINCT term_tag) = {term_count}`
- for OR mode: `GROUP BY entry_id HAVING COUNT(DISTINCT term_tag) >= 1`

Then load matching `search_document` rows and apply filters in the order already specified by `docs/architecture/local-encrypted-search.md`:

1. time range via `active_time_ms`
2. `file_type`
3. file extensions
4. ordering
5. pagination

Extension filtering can be applied after loading candidate documents if that keeps the code simpler and deterministic. The architecture spec only locks the evaluation order, not an SQL-only implementation.

### 4. Result ordering

Phase 91 should already use the architecture order:

1. `active_time_ms DESC`
2. hit count DESC
3. `captured_at_ms DESC`

`captured_at_ms` does not need to appear in `SearchResult`; it only needs to stay available inside the internal query projection used for sorting.

## Rebuild Strategy Guidance

### Why temp tables are required

The current schema cannot keep active and staged posting rows in the same permanent table because `search_posting` has no `index_version` column. Therefore the rebuild workspace has to live in temp tables or uniquely named transient tables.

### Recommended flow

1. Resolve `profile_id` and ensure meta row exists.
2. Update `search_index_meta` for that profile:
   - `search_blocked = true`
   - `last_rebuild_started_at_ms = now`
   - keep `last_rebuild_completed_at_ms` unchanged
3. Emit `RebuildProgress { stage: Started, indexed: 0, total: entries.len() }`.
4. Create profile-scoped temp tables that mirror:
   - `search_document`
   - `search_posting`
5. Register `ActiveRebuild` in memory so live `index_entry()` and `remove_entry()` calls know where to mirror.
6. Write the supplied rebuild entries into the temp tables in batches and emit `Indexing` progress.
7. Finalize in a single write transaction:
   - delete active `search_posting` rows for the profile
   - delete active `search_document` rows for the profile
   - insert all temp posting rows into active `search_posting`
   - insert all temp document rows into active `search_document`
   - update `search_index_meta.index_version = CURRENT_INDEX_VERSION`
   - update `search_index_meta.search_blocked = false`
   - update `last_rebuild_completed_at_ms = now`
8. Emit `RebuildProgress { stage: Complete, ... }`.
9. Drop temp tables and clear `ActiveRebuild`.

### Failure policy

If any rebuild step fails:

- emit `RebuildProgress { stage: Failed, ... }`
- clear in-memory `ActiveRebuild`
- drop temp tables best-effort
- leave `search_blocked = true`
- return `SearchError::Internal(...)`

This directly honors the locked Phase 91 decision that rebuild failure leaves search blocked until a later successful rebuild.

### Live mutation mirroring during rebuild

`index_entry()` and `remove_entry()` must use one authoritative helper each:

- active document/posting upsert/delete
- optional temp-table mirror when the current profile matches `ActiveRebuild.profile_id`

Required mirroring behavior:

- `index_entry()` replaces any prior active rows for the same `(profile_id, entry_id)` and, when rebuilding, also replaces the staged temp rows for that entry
- `remove_entry()` deletes active rows and, when rebuilding, also deletes staged temp rows for that entry

This is what prevents new entries from disappearing after cutover and deleted entries from reappearing after cutover.

## Test Strategy Recommendations

Phase 91 needs real SQLite integration tests, not only unit tests with mocks.

### Must-have harness pieces

- temp-file database fixture using `init_db_pool(path)` so WAL and multi-connection behavior match production
- fixed-scope `KeyScopePort` stub for deterministic `profile_id`
- fixed `SearchKeyDerivationPort` stub so search-query term tags are reproducible in tests
- helper that inserts representative `SearchDocument` + `Vec<SearchPosting>` rows through the real adapter
- helper that opens a second SQLite connection and holds a read transaction open across rebuild finalization

### Must-have integration coverage

1. AND query over real postings returns only entries matching all terms.
2. OR query over real postings returns entries matching any term.
3. version mismatch flips or respects `search_blocked` and returns `SearchError::IndexNotReady`.
4. rebuild creates blocked state immediately and clears it only after successful cutover.
5. entry inserted during rebuild is visible after cutover.
6. entry deleted during rebuild stays absent after cutover.
7. rebuild completion succeeds while another connection holds a read transaction; no `RENAME TABLE`, no `SQLITE_BUSY` timeout caused by rename.

### Nice-to-have

- deterministic test hook or barrier inside rebuild batch processing so mid-rebuild insert/delete tests do not depend on sleeps
- explicit assertion that `sqlite_index.rs` does not contain `RENAME TABLE`

## Don't Hand-Roll

| Problem | Don't Build | Use Instead |
|---------|-------------|-------------|
| Query tokenization | ad hoc whitespace split | existing `SearchTokenizer` |
| Query HMAC tags | direct `MasterKey` use or duplicate HMAC helper | existing `SearchKeyDerivationPort` + `term_tag()` |
| Meta seeding | duplicated INSERT logic in every method | `NewSearchIndexMetaRow::seed(profile_id)` |
| Active-row mapping | hand-built tuples scattered across methods | `rows.rs` conversion helpers |
| Rebuild synchronization | sleeps or global static flags | adapter-owned `RwLock<Option<ActiveRebuild>>` + test-only hooks if needed |
| Swap strategy | `RENAME TABLE` or permanent shadow tables | temp tables + transactional copy-in |

## Common Pitfalls

### Pitfall 1: Treating `search_document.index_version` as enough for a true dual-version live index

It is not enough because `search_posting` has no version column. Any plan that tries to keep two live permanent posting sets in place will either overwrite active postings or require an unplanned schema change.

### Pitfall 2: Checking `search_blocked` only in the daemon later

The port contract already exposes `SearchError::IndexNotReady`. If Phase 91 leaves the guard to Phase 92, internal callers can still query stale rows through the port.

### Pitfall 3: Rebuild-only temp writes with no mirrored delete path

Mid-rebuild insert mirroring is only half the story. If delete mirroring is missing, rebuild cutover will resurrect content the user already deleted.

### Pitfall 4: Using `:memory:` for the lock-contention test

That bypasses the real WAL file behavior and cannot prove the no-rename claim. The concurrent-read test has to use a temp file.

### Pitfall 5: Applying `limit/offset` before time/type/extension filters

That will drop valid results and make pagination unstable. The architecture order is candidate terms -> metadata filters -> ordering -> pagination.

## Validation Architecture

### Test Framework

- Rust integration and module tests inside `uc-infra`
- temp-file SQLite fixtures via the existing Diesel pool
- no new framework required

### Phase Requirements -> Test Map

| Requirement | Test Target | Test Type |
|-------------|-------------|-----------|
| REBLD-01 | `SqliteSearchIndex::rebuild()` transitions meta row into blocked mode, persists rebuilt rows, and clears blocked on success | integration |
| REBLD-02 | rebuild completion under concurrent read load succeeds without `RENAME TABLE` and without rename-triggered busy timeout | integration |
| REBLD-03 | `index_entry()` and `remove_entry()` mirror to rebuild temp tables, so mid-rebuild insert/delete state survives cutover correctly | integration |

### Sampling Rate

- After each task commit: `cd src-tauri && cargo test -p uc-infra search::sqlite_index`
- After each plan wave: `cd src-tauri && cargo test -p uc-infra search:: && cargo check -p uc-infra`
- Before phase verification: `cd src-tauri && cargo test -p uc-infra search:: && cargo check -p uc-infra`

### Likely Wave 0 Gaps

- no `sqlite_index.rs` module exists yet
- no temp-file fixture currently holds a second live read connection during a rebuild
- no shared search-adapter test support exists for fixed profile scope + derived search key stubs

## Sources

### Primary (HIGH confidence)

- `.planning/phases/91-sqlite-index-adapter-and-rebuild-strategy/91-CONTEXT.md`
- `.planning/ROADMAP.md`
- `.planning/REQUIREMENTS.md`
- `.planning/STATE.md`
- `docs/architecture/local-encrypted-search.md`
- `.planning/research/PITFALLS.md`
- `src-tauri/crates/uc-core/src/ports/search/search_index.rs`
- `src-tauri/crates/uc-core/src/search/query.rs`
- `src-tauri/crates/uc-core/src/search/document.rs`
- `src-tauri/crates/uc-core/src/search/result.rs`
- `src-tauri/crates/uc-core/src/search/key.rs`
- `src-tauri/crates/uc-core/src/ports/security/key_scope.rs`
- `src-tauri/crates/uc-core/src/ports/security/encryption_session.rs`
- `src-tauri/crates/uc-infra/src/db/pool.rs`
- `src-tauri/crates/uc-infra/src/db/schema.rs`
- `src-tauri/crates/uc-infra/src/search/mod.rs`
- `src-tauri/crates/uc-infra/src/search/rows.rs`
- `src-tauri/crates/uc-infra/src/search/pipeline.rs`
- `src-tauri/crates/uc-infra/src/search/search_key_derivation.rs`
- `src-tauri/crates/uc-infra/migrations/2026-04-11-000001_create_search_index/up.sql`

### Secondary (MEDIUM confidence)

- `.planning/phases/90-sqlite-schema-migration-and-tokenizer-pipeline/90-RESEARCH.md`
- `.planning/phases/88-core-domain-and-port-contracts/88-CONTEXT.md`
- `.planning/phases/89-use-cases-and-delete-integration/89-CONTEXT.md`
- `src-tauri/crates/uc-infra/src/db/repositories/clipboard_entry_repo.rs`
- `src-tauri/crates/uc-infra/src/db/repositories/file_transfer_repo.rs`

## Metadata

- Scope: `uc-infra` adapter only; no daemon or UI wiring
- Risk level: high (rebuild correctness under concurrency)
- Recommended plan count: 2
- Recommended execution split:
  - Plan 01: adapter core (`index_entry`, `remove_entry`, `get_index_meta`, `search`) + real SQLite query tests
  - Plan 02: rebuild temp-table flow, live mirroring, failure/blocked semantics, and concurrency tests
