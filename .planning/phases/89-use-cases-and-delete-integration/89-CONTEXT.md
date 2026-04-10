# Phase 89: Use Cases and Delete Integration - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement four search use cases in uc-app (IndexClipboardEntry, RemoveIndexedEntry, SearchClipboardEntries, RebuildSearchIndex) and extend DeleteClipboardEntry with synchronous search index cleanup. All use cases delegate to SearchIndexPort — no tokenizer, no HMAC computation here (those are Phase 90 concerns). All use cases must be unit-testable with mock ports.

</domain>

<decisions>
## Implementation Decisions

### Module Location

- **D-01:** All four search use cases live in `uc-app/src/usecases/search/` as a dedicated subdirectory, parallel to `clipboard/`. Each use case gets its own file. A `mod.rs` re-exports the public API. Exports are added to `uc-app/src/usecases/mod.rs`.

### IndexClipboardEntry Input Contract

- **D-02:** `IndexClipboardEntry::execute()` accepts pre-built `(SearchDocument, Vec<SearchPosting>)` as parameters. The use case is a thin orchestrator — it delegates directly to `SearchIndexPort::index_entry()`. Caller (Phase 92 daemon capture handler) is responsible for constructing these objects using the Phase 90 tokenizer.
- **D-03:** No tokenizer port injected into IndexClipboardEntry — it is not this use case's responsibility to compute HMAC tags. This keeps the use case boundary clean and mockable.

### RebuildSearchIndex Input Contract

- **D-04:** `RebuildSearchIndex::execute()` accepts caller-supplied `Vec<(SearchDocument, Vec<SearchPosting>)>` and a `tokio::sync::mpsc::Sender<RebuildProgress>`. The use case delegates to `SearchIndexPort::rebuild()`. Same reasoning as D-02 — no tokenizer in Phase 89.

### RemoveIndexedEntry

- **D-05:** Standalone `RemoveIndexedEntry` use case wraps `SearchIndexPort::remove_entry()`. Accepts `&EntryId`, returns `Result<(), SearchError>`. Thin orchestrator. Separate from DeleteClipboardEntry's integration path.

### SearchClipboardEntries

- **D-06:** `SearchClipboardEntries::execute()` accepts `SearchQuery` and returns `Result<Vec<SearchResult>, SearchError>` via `SearchIndexPort::search()`. No additional filtering or sorting in the use case layer — port returns the final ordered result.

### DeleteClipboardEntry — Search Cleanup Error Policy

- **D-07:** When `search_index.remove_entry(entry_id)` fails, **log a warning and continue** — the delete always completes. Orphaned index entries are acceptable; a future rebuild will clean them up. This matches how file cache deletion is handled in the current codebase (warn but continue). An index bug must not prevent users from deleting clipboard entries.
- **D-08:** `SearchIndexPort` is injected via `.with_search_index(Arc<dyn SearchIndexPort>)` builder method on `DeleteClipboardEntry`, consistent with the existing `.with_file_cache_dir()` builder pattern. The field is `Option<Arc<dyn SearchIndexPort>>` — absence means no search cleanup (backwards-compatible).

### Claude's Discretion

- Exact tracing span naming within the new use cases
- Whether to add a `#[tracing::instrument]` attribute to each execute() method (follow existing use case pattern)
- Whether RebuildSearchIndex also exposes a convenience method without the progress sender (can use a no-op channel internally)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Port Contracts (Phase 88 output)

- `src-tauri/crates/uc-core/src/ports/search/search_index.rs` — SearchIndexPort trait: index_entry, remove_entry, search, rebuild, get_index_meta signatures
- `src-tauri/crates/uc-core/src/ports/search/search_key.rs` — SearchKeyDerivationPort trait
- `src-tauri/crates/uc-core/src/search/mod.rs` — Re-exports: SearchDocument, SearchPosting, SearchQuery, SearchResult, SearchError, RebuildProgress

### Domain Models

- `src-tauri/crates/uc-core/src/search/document.rs` — SearchDocument, SearchPosting, SearchIndexMeta, FileType
- `src-tauri/crates/uc-core/src/search/query.rs` — SearchQuery, QueryOperator, TimeRangeFilter
- `src-tauri/crates/uc-core/src/search/result.rs` — SearchResult, RebuildProgress, RebuildStage
- `src-tauri/crates/uc-core/src/search/error.rs` — SearchError variants

### Requirements

- `.planning/REQUIREMENTS.md` §SIDX-01 — Automatic indexing on capture
- `.planning/REQUIREMENTS.md` §SIDX-02 — Synchronous search cleanup on delete

### Existing Use Cases to Pattern Against

- `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs` — builder pattern (.with_file_cache_dir), Arc<dyn Port> injection, warn-and-continue for non-critical failures
- `src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs` — clipboard subdirectory module structure to replicate for search/

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `delete_clipboard_entry.rs::with_file_cache_dir()` — builder pattern to replicate for `with_search_index()`. `Option<Arc<dyn ...>>` field with `None` as safe default.
- `uc-core/src/ports/search/search_index.rs` — `StubPort` in #[cfg(test)] block shows the minimal mock shape needed for unit tests.

### Established Patterns

- All use cases use `#[async_trait]` indirectly (port calls are async); use case execute() methods are async.
- Tracing: `#[tracing::instrument(name = "usecase.X.execute", skip(self), fields(...))]` on execute().
- Arc injection: ports stored as `Arc<dyn Port + Send + Sync>` fields.
- Error handling: `anyhow::Result<T>` for use case execute() unless a typed error is needed by the caller (SearchClipboardEntries returns `Result<Vec<SearchResult>, SearchError>` to preserve error semantics through the port boundary).
- File cache deletion in DeleteClipboardEntry uses `warn!()` + early return from closure — same approach for search cleanup failure.

### Integration Points

- `uc-app/src/usecases/mod.rs` — add `pub mod search;` and re-export new use case types
- `uc-app/src/usecases/delete_clipboard_entry.rs` — add `search_index: Option<Arc<dyn SearchIndexPort>>` field and builder method; call `remove_entry` after step 4 (event/snapshot delete) — or before step 2 — ordering TBD by planner (search cleanup can be first since it's non-authoritative)
- Phase 92 daemon capture endpoint — will call `IndexClipboardEntry::execute(doc, postings)` after tokenizer builds the objects
- Phase 91 SqliteSearchIndex — will be the real SearchIndexPort implementation used in integration

</code_context>

<specifics>
## Specific Ideas

- The `.with_search_index()` builder follows the same ergonomic pattern as `.with_file_cache_dir()` — callers that don't need search cleanup just omit the builder call.
- Search error in delete path is logged at `warn!` level, not `error!`, since it is non-fatal and expected to self-heal via rebuild.

</specifics>

<deferred>
## Deferred Ideas

- None — discussion stayed within phase scope.

### Reviewed Todos (not folded)

- "修复 setup 配对确认提示缺失" (setup pairing confirmation toast) — unrelated to search use cases, not folded.

</deferred>

---

_Phase: 89-use-cases-and-delete-integration_
_Context gathered: 2026-04-10_
