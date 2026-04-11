# Phase 92: Bootstrap Wiring and Daemon HTTP Routes - Research

**Researched:** 2026-04-11
**Domain:** Cross-crate search wiring (`uc-core` + `uc-app` + `uc-bootstrap` + `uc-daemon`)
**Confidence:** HIGH

## Summary

Phase 92 should be planned as a backend integration phase with four concrete concerns:

1. Evolve the search query result contract so the backend can truthfully return `total` and `hasMore` without double-query hacks.
2. Wire search dependencies into `AppDeps` / `CoreUseCases` so daemon code stops constructing search pieces ad hoc.
3. Add one daemon-owned search coordinator that owns first-unlock auto-backfill, manual rebuild serialization, rebuild reason tracking, and WebSocket progress forwarding.
4. Expose the transport surface in `uc-daemon` with readable query params, 423 lock semantics, and dedicated search WebSocket events.

The most important finding is that the current `SearchIndexPort::search()` contract is too weak for Phase 92. It returns only `Vec<SearchResult>`, but the phase context explicitly requires query responses to include `total` and `hasMore`. The correct fix is to evolve the port/use-case/infra return type to a paged domain object instead of doing shallow route-level workarounds.

## Current Gap Scan

### What already exists

- `uc-infra` already has the real `SqliteSearchIndex` adapter, rebuild mechanics, tokenizer, text extractor, and search-key derivation.
- `uc-app` already has the four thin search use cases from Phase 89.
- `DeleteClipboardEntry` already supports `.with_search_index(...)`, but `CoreUseCases::delete_clipboard_entry()` does not inject it.
- `uc-daemon` already has the shared broadcast channel, route composition pattern, and WebSocket fanout path needed for search events.

### What is still missing

- `AppDeps` has no search bundle.
- `CoreUseCases` exposes no search accessors.
- The daemon has no search coordinator, no search projection builder, no search routes, and no search WebSocket topic.
- The current search query return contract cannot produce `total` / `hasMore`.
- There is no daemon-owned status reason authority for `initial_backfill`, `version_mismatch`, `manual_rebuild`, and `rebuild_failed_waiting_for_retry`.

## Recommended Architecture

### 1. Evolve search results from list-only to page metadata

Recommended new domain output:

```text
SearchResultsPage {
  items: Vec<SearchResult>,
  total: u32,
  has_more: bool,
}
```

Required changes:

- `uc-core/src/search/result.rs` adds `SearchResultsPage`
- `uc-core/src/search/mod.rs` re-exports it
- `uc-core/src/ports/search/search_index.rs` changes `search()` to return `Result<SearchResultsPage, SearchError>`
- `uc-app/src/usecases/search/search_clipboard_entries.rs` forwards the new type unchanged
- `uc-infra/src/search/sqlite_index.rs` computes `total` before pagination and `has_more = total > offset + items.len() as u32`

Why this is the right fix:

- It satisfies Phase 92 D-02 directly.
- It keeps pagination truth inside the single authority that already owns filtering and ordering.
- It avoids route-level double queries or fake `hasMore` inference.

### 2. Add one search bundle to `AppDeps`

Recommended new grouping in `uc-app/src/deps.rs`:

```text
SearchPorts {
  search_index: Arc<dyn SearchIndexPort>,
  search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
  search_pipeline: Arc<SearchPipeline>,
}
```

Reasoning:

- Phase 92 context explicitly calls out that `AppDeps` currently has no owned search path.
- The daemon needs all three pieces repeatedly: live indexing, rebuild materialization, and truthful query execution.
- Keeping them grouped prevents `uc-daemon` code from constructing infra search objects in multiple places.

Inference from the current codebase:

- `SearchPipeline` is a concrete infra helper, not a core port. Grouping it beside the two search ports is a pragmatic exception that still preserves single ownership better than re-instantiating it inside handlers.

### 3. Add `CoreUseCases` search accessors and complete delete integration

Recommended accessors:

- `index_clipboard_entry()`
- `remove_indexed_entry()`
- `search_clipboard_entries()`
- `rebuild_search_index()`

Also update:

- `CoreUseCases::delete_clipboard_entry()` to call `.with_search_index(self.runtime.deps.search.search_index.clone())`

Why:

- The daemon already consistently uses `CoreUseCases::new(runtime.as_ref())`.
- Search should follow the same pattern instead of reaching into `runtime.wiring_deps()` for business operations.
- This closes the Phase 89 wiring gap for delete cleanup.

### 4. Keep projection building daemon-owned, but make it a single module

Recommended daemon module:

```text
src-tauri/crates/uc-daemon/src/search/
├── mod.rs
├── projection.rs
└── coordinator.rs
```

`projection.rs` should own two entrypoints:

- `build_from_capture(entry, snapshot, selection) -> Option<SearchPipelineInput>`
- `build_from_persisted(entry, selection, reps) -> Option<SearchPipelineInput>`

Concrete rules:

- Reuse clipboard MIME / URI / file-path interpretation already established in clipboard code.
- Prefer live snapshot bytes for immediate capture indexing so staged blob payloads do not block indexing.
- Use persisted entry + persisted representations for rebuild.
- Skip entries whose pipeline output produces zero postings; do not write empty search documents.

Why not put this inside `uc-app`:

- Phase 89 explicitly locked search use cases as thin orchestrators over prebuilt documents/postings.
- The assembly of `SearchPipelineInput` is daemon integration work, not a new domain use case.

### 5. Add a daemon-owned `SearchCoordinator`

Recommended ownership:

- one `SearchCoordinator` service or worker in `uc-daemon`
- one rebuild mutex / single-flight guard
- one in-memory reason snapshot for status + WS payloads

The coordinator should own:

- first-unlock auto-backfill
- version-mismatch auto-rebuild on unlocked startup
- manual rebuild requests
- rebuild progress forwarding into the existing `broadcast::Sender<DaemonWsEvent>`
- current search availability snapshot for `/search/status`

Recommended reason codes:

- `initial_backfill`
- `version_mismatch`
- `manual_rebuild`
- `rebuild_failed_waiting_for_retry`

Recommended unlocked status states:

- `ready`
- `rebuilding`
- `unavailable`

Inference to reconcile D-04 and D-10:

- Keep HTTP `423 Locked` as the locked-state truth for all three routes.
- Reserve the normal `/search/status` payload `state` field for unlocked sessions only.
- This is the cleanest way to satisfy both “product-oriented states” and the locked-route requirement.

### 6. Use readable HTTP params with explicit operator support

Recommended query route shape:

```text
GET /search/query
  ?query=foo%20bar
  &operator=and
  &timePreset=last_7d
  &fileTypes=text
  &fileTypes=file
  &extensions=md
  &extensions=txt
  &limit=50
  &offset=0
```

Supported params:

- `query` required
- `operator` optional, values `and|or`
- `timePreset` optional, values `today|yesterday|last_24h|last_7d|last_30d|this_week|this_month`
- `fromMs` and `toMs` optional absolute range pair
- repeated `fileTypes`
- repeated `extensions`
- `limit`, default `50`, max `200`
- `offset`, default `0`

Recommended parser behavior:

- If `operator` is present, build `SearchQuery.operator` from it.
- If `operator` is absent, infer it from the raw query text:
  - only `OR` keywords present => `or`
  - only `AND` keywords present => `and`
  - neither present => `and`
  - both present => `SearchError::InvalidQuery("mixed AND/OR operators are not supported")`
- Strip standalone `AND` / `OR` tokens before sending `query_string` into the search port.

This preserves:

- the explicit query-param decision from Phase 92 context
- the mixed-operator rejection requirement from SQRY-06
- the current `SearchQuery` model in `uc-core`

### 7. Use one dedicated search topic with one progress event type

Recommended WebSocket constants:

- topic: `search`
- snapshot event: `search.status_snapshot`
- progress event: `search.rebuild_progress`

Recommended progress payload:

```text
{
  stage: "started" | "indexing" | "complete" | "failed",
  indexed: u32,
  total: u32,
  reason: "initial_backfill" | "version_mismatch" | "manual_rebuild" | "rebuild_failed_waiting_for_retry" | null
}
```

Why use one progress event type:

- It matches the existing file-transfer pattern of a stable event type plus payload discriminator/state.
- It maps directly from `RebuildProgress`.
- It gives later UI work one stream to subscribe to.

### 8. Keep `uc-daemon-client` / realtime bridge work out of this phase

Recommended scope line:

- Phase 92 should establish the daemon-side HTTP and WebSocket wire contract.
- Phase 93 can add `uc-daemon-client` search helpers and bridge-to-frontend subscription wiring when the UI starts consuming them.

Why this is a reasonable boundary:

- The roadmap says Phase 92 is backend-only and explicitly “without UI work”.
- The daemon already exposes raw HTTP + raw WS surfaces directly.
- Search UI is the first consumer that will force bridge/client ergonomics.

This is an inference from the roadmap and current crate layout, not a previously locked decision.

## Concrete Transport Contracts

### Query response

Recommended JSON shape:

```text
{
  data: [SearchResultDto...],
  total: 123,
  hasMore: true,
  ts: 1760000000000
}
```

### Status response

Recommended `200 OK` JSON shape for unlocked sessions:

```text
{
  data: {
    state: "ready" | "rebuilding" | "unavailable",
    reason: null | "initial_backfill" | "version_mismatch" | "manual_rebuild" | "rebuild_failed_waiting_for_retry",
    lastRebuildStartedAtMs: 1760000000000 | null,
    lastRebuildCompletedAtMs: 1760000005000 | null
  },
  ts: 1760000005000
}
```

Locked-session behavior:

- `423 Locked`
- `{ code: "session_locked", message: "encryption session is locked" }`

### Rebuild response

Recommended response on accepted manual rebuild:

```text
HTTP 202
{
  data: {
    accepted: true
  },
  ts: 1760000000000
}
```

Recommended response when a rebuild is already running:

- `409 Conflict`
- `{ code: "rebuild_already_running", message: "search rebuild is already running" }`

## Test Strategy Recommendations

### Required automated coverage

1. `uc-core` / `uc-app` tests proving the new `SearchResultsPage` contract compiles and propagates.
2. `uc-infra` tests proving `SqliteSearchIndex.search()` returns `total` and `has_more` correctly after filters and pagination.
3. Daemon integration tests for:
   - capture -> index -> `/search/query`
   - delete -> direct DB check for missing `search_posting`
   - `/search/query`, `/search/status`, `/search/rebuild` all return `423` when locked
   - mixed `AND` + `OR` query returns `invalid_query`
   - manual rebuild emits `search.rebuild_progress` with `started` and `complete`
   - first unlocked startup auto-triggers rebuild when legacy history exists

### Recommended command split

- Quick search foundation loop:
  - `cd src-tauri && cargo test -p uc-core search:: && cargo test -p uc-app usecases::search`
- Quick daemon loop:
  - `cd src-tauri && cargo test -p uc-daemon search_`
- Full phase loop:
  - `cd src-tauri && cargo test -p uc-core search:: && cargo test -p uc-app usecases::search && cargo test -p uc-infra search::sqlite_index && cargo test -p uc-daemon search_ && cargo check -p uc-daemon`

## Main Pitfalls

1. Returning `total` / `hasMore` from the route without evolving the port contract.
   This would duplicate query execution or force fake pagination metadata.

2. Building search dependencies inside daemon handlers.
   This would recreate the exact ad hoc ownership gap the phase context calls out.

3. Treating rebuild orchestration as a route-local spawned task.
   That scatters rebuild state, progress forwarding, and reason codes across multiple handlers.

4. Reusing only `search_index_meta.search_blocked` for status.
   That loses the distinction between `initial_backfill`, `version_mismatch`, and `manual_rebuild`.

5. Re-reading only persisted representations during live capture indexing.
   That can miss staged payload content that still exists in the current live snapshot.

## Validation Architecture

Phase 92 should validate in four layers:

1. Contract layer
   Verify `SearchResultsPage` exists and every search caller compiles against it.

2. Infra layer
   Verify pagination metadata, filter correctness, and `SearchError` behavior in `uc-infra`.

3. Daemon transport layer
   Verify query parsing, lock/error mapping, status payloads, and rebuild acceptance/conflict behavior.

4. End-to-end daemon layer
   Verify real capture, delete, query, rebuild, and WebSocket progress against a built runtime fixture.

The minimum acceptable implementation is not “routes compile”. It is “capture writes become queryable, delete removes postings, rebuild progress is observable, and lock semantics are correct on all three endpoints.”
