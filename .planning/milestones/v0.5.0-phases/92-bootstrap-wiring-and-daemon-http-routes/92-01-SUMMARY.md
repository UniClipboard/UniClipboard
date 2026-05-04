---
phase: 92-bootstrap-wiring-and-daemon-http-routes
plan: 01
subsystem: search-foundation
tags: [search, pagination, deps-wiring, tdd]
one_liner: "SearchResultsPage paged contract + AppDeps search bundle wired with HKDF adapter and CoreUseCases accessors"

dependency_graph:
  requires:
    - "Phase 91: SqliteSearchIndex with rebuild mechanics"
    - "Phase 89: SearchIndexPort, search use cases"
  provides:
    - "SearchResultsPage: authoritative pagination metadata at port/adapter/use-case boundary"
    - "AppDeps.search: single owned search bundle (SearchIndexPort + SearchKeyDerivationPort + SearchPipeline)"
    - "CoreUseCases search accessors: index_clipboard_entry, remove_indexed_entry, search_clipboard_entries, rebuild_search_index"
    - "Daemon API string constants for search HTTP routes and WS topics"
  affects:
    - "uc-core/ports/search: SearchIndexPort.search() now returns SearchResultsPage"
    - "uc-app/usecases/search: all mock impls updated to return SearchResultsPage"
    - "uc-app/deps: SearchPorts bundle added to AppDeps"
    - "uc-bootstrap/assembly: search bundle wired with concrete infra types"
    - "uc-tauri/bootstrap/runtime: test NoopPort updated with search port impls"

tech_stack:
  added:
    - "uc-infra as runtime dependency of uc-app (pragmatic exception for SearchPipeline grouping)"
  patterns:
    - "TDD RED/GREEN for type contract evolution"
    - "Port evolution: adding SearchResultsPage without breaking existing Vec<SearchResult> consumers"
    - "Dependency bundle pattern: SearchPorts groups 3 co-owned search pieces"

key_files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/search/result.rs
    - src-tauri/crates/uc-core/src/search/mod.rs
    - src-tauri/crates/uc-core/src/ports/search/search_index.rs
    - src-tauri/crates/uc-core/src/network/daemon_api_strings.rs
    - src-tauri/crates/uc-app/Cargo.toml
    - src-tauri/crates/uc-app/src/deps.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-app/src/usecases/search/search_clipboard_entries.rs
    - src-tauri/crates/uc-app/src/usecases/search/index_clipboard_entry.rs
    - src-tauri/crates/uc-app/src/usecases/search/rebuild_search_index.rs
    - src-tauri/crates/uc-app/src/usecases/search/remove_indexed_entry.rs
    - src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs
    - src-tauri/crates/uc-infra/src/search/sqlite_index.rs
    - src-tauri/crates/uc-bootstrap/src/assembly.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs

decisions:
  - "SearchResultsPage is computed in SqliteSearchIndex (infra layer) — total is counted before pagination, has_more derived from total > offset + paged_len. Route layer gets authoritative pagination truth with no double-query."
  - "uc-app formally depends on uc-infra (runtime [dependencies]) to allow SearchPipeline type in SearchPorts bundle. This is the pragmatic exception documented in Phase 92 research and accepted by plan authors."
  - "SearchPorts bundle groups search_index + search_key_derivation + search_pipeline in AppDeps — prevents daemon code from constructing search pieces ad hoc."
  - "CoreUseCases.delete_clipboard_entry() now injects search_index via with_search_index() — closes Phase 89 wiring gap for delete cleanup."
  - "Early-return path in SqliteSearchIndex.search() (empty hit_map) updated to return SearchResultsPage with total=0/has_more=false instead of Ok(vec![])."

metrics:
  duration: "~60min"
  completed_date: "2026-04-11"
  tasks_completed: 2
  files_modified: 15
---

# Phase 92 Plan 01: Search Foundation — SearchResultsPage + AppDeps Wiring Summary

SearchResultsPage paged contract introduced at the uc-core port/use-case/adapter boundary, plus a single owned search dependency bundle wired into AppDeps and CoreUseCases with daemon API string constants ready for Phase 92 transport plans.

## Tasks Completed

### Task 1: Evolve search query outputs to SearchResultsPage and compute pagination metadata

**TDD RED:** Added `SearchResultsPage` struct skeleton in `result.rs`, re-exported from `search/mod.rs`, then wrote three behavioral tests that failed because port/adapter still returned `Vec<SearchResult>`:
- `search::result::tests::search_results_page_serde_round_trip`
- `usecases::search::search_clipboard_entries::tests::execute_forwards_query_and_returns_page_metadata`
- `search::sqlite_index::tests::search_query_returns_total_and_has_more_metadata`

Commit: `832caf28`

**TDD GREEN:** Full implementation across the stack:
- `SearchIndexPort::search()` returns `Result<SearchResultsPage, SearchError>`
- `SqliteSearchIndex.search()` computes `total = sorted.len() as u32` (before pagination), `has_more = total > offset + paginated.len()`, returns `SearchResultsPage { items, total, has_more }`
- Early-return path for empty hit_map fixed to return `SearchResultsPage { items: vec![], total: 0, has_more: false }`
- All four mock `SearchIndexPort` impls in uc-app test modules updated
- Existing sqlite_index integration tests updated from `results.len()` to `page.items.len()`

Commit: `62331e7c`

### Task 2: Wire one owned search bundle into runtime/bootstrap and expose search use-case accessors

**TDD RED:** Added `search_transport_constants_match` test asserting all six new constants exist. Test failed with `cannot find value` errors.

Commit: `0faff899`

**TDD GREEN:**
- Added `ws_topic::SEARCH`, `ws_event::SEARCH_STATUS_SNAPSHOT`, `ws_event::SEARCH_REBUILD_PROGRESS`, `http_route::SEARCH_QUERY`, `http_route::SEARCH_STATUS`, `http_route::SEARCH_REBUILD` to `daemon_api_strings.rs`
- Added `SearchPorts` struct to `uc-app/src/deps.rs` with `search_index: Arc<dyn SearchIndexPort>`, `search_key_derivation: Arc<dyn SearchKeyDerivationPort>`, `search_pipeline: Arc<SearchPipeline>`
- Added `pub search: SearchPorts` field to `AppDeps`
- Added `uc-infra` to `uc-app/Cargo.toml` `[dependencies]` (pragmatic exception for SearchPipeline)
- Wired search bundle in `assembly.rs`: `HkdfSearchKeyDerivation::new(...)`, `SqliteSearchIndex::new(db_pool_for_search, ...)`, `SearchPipeline::new()` — `db_pool` cloned before `create_infra_layer` consumes it
- Added four search accessors to `CoreUseCases`: `index_clipboard_entry`, `remove_indexed_entry`, `search_clipboard_entries`, `rebuild_search_index` — each calls `.from_port(self.runtime.deps.search.search_index.clone())`
- Updated `CoreUseCases::delete_clipboard_entry()` to end with `.with_search_index(self.runtime.deps.search.search_index.clone())`
- Added `SearchIndexPort` and `SearchKeyDerivationPort` impls for `NoopPort` in uc-tauri tests, plus `search: SearchPorts` in the test `AppDeps` construction

Commit: `b991cb1d`

## Verification Results

All acceptance criteria passed:

```
cargo test -p uc-core search::result::tests::search_results_page_serde_round_trip       → ok
cargo test -p uc-app  usecases::search::...::execute_forwards_query_and_returns_page_metadata  → ok
cargo test -p uc-infra search::sqlite_index::tests::search_query_returns_total_and_has_more_metadata  → ok
cargo check -p uc-app -p uc-bootstrap -p uc-daemon                                       → ok (0 errors)
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed early-return path in SqliteSearchIndex.search() returning wrong type**

- **Found during:** Task 1 GREEN implementation
- **Issue:** The `hit_map.is_empty()` early-return at line ~806 returned `Ok(vec![])` which is `Vec<SearchResult>`, not `SearchResultsPage`. This caused a type mismatch in the `spawn_blocking` closure.
- **Fix:** Changed early return to `Ok(SearchResultsPage { items: vec![], total: 0, has_more: false })`
- **Files modified:** `src-tauri/crates/uc-infra/src/search/sqlite_index.rs`
- **Commit:** `62331e7c`

**2. [Rule 1 - Bug] Updated four additional mock SearchIndexPort impls in uc-app**

- **Found during:** Task 1 GREEN implementation
- **Issue:** `delete_clipboard_entry.rs`, `index_clipboard_entry.rs`, `rebuild_search_index.rs`, and `remove_indexed_entry.rs` all had `MockSearchIndex::search()` returning `Vec<SearchResult>` — compile error after port change.
- **Fix:** Updated all four mock `search()` methods to return `SearchResultsPage`. Removed redundant `SearchResult` imports.
- **Files modified:** `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs`, `...search/index_clipboard_entry.rs`, `...search/rebuild_search_index.rs`, `...search/remove_indexed_entry.rs`
- **Commit:** `62331e7c`

**3. [Rule 2 - Missing] Added uc-infra as runtime dependency of uc-app**

- **Found during:** Task 2 planning
- **Issue:** Plan requires `Arc<SearchPipeline>` (concrete infra type) in `AppDeps.search`. `uc-app/Cargo.toml` only had `uc-infra` in `[dev-dependencies]`.
- **Fix:** Added `uc-infra = { path = "../uc-infra" }` to `[dependencies]`. This was pre-approved by plan authors as a "pragmatic exception" (Phase 92 Research §2 inference note).
- **Files modified:** `src-tauri/crates/uc-app/Cargo.toml`
- **Commit:** `b991cb1d`

**4. [Rule 1 - Bug] Updated NoopPort in uc-tauri tests to implement search ports**

- **Found during:** Task 2 GREEN implementation
- **Issue:** Test `AppDeps` construction in `uc-tauri/src/bootstrap/runtime.rs` required `search: SearchPorts` field after struct was widened. `NoopPort` did not implement `SearchIndexPort` or `SearchKeyDerivationPort`.
- **Fix:** Added `impl SearchIndexPort for NoopPort` and `impl SearchKeyDerivationPort for NoopPort` in test module, plus the `search: SearchPorts` field in the test `AppDeps`.
- **Files modified:** `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`
- **Commit:** `b991cb1d`

## Known Stubs

None — all search accessors in `CoreUseCases` are fully wired to real ports. The `SearchPorts` bundle is real infrastructure constructed in `assembly.rs`.

## Self-Check: PASSED
