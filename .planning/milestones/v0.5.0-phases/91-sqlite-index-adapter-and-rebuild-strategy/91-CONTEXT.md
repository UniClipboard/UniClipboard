# Phase 91: SQLite Index Adapter and Rebuild Strategy - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement the real `SqliteSearchIndex` adapter in `uc-infra` so the landed search
schema becomes a working local index: live index writes, removals, query execution,
index meta reads, and full rebuild with a version-flag atomic swap strategy.
This phase owns correctness and rebuild behavior inside the SQLite adapter.
It does not add daemon HTTP routes or WS events (Phase 92) and does not add UI (Phase 93).

</domain>

<decisions>
## Implementation Decisions

### Rebuild Read Availability
- **D-01:** Any full rebuild makes search unavailable for the full rebuild window, even when the rebuild is manually triggered and the on-disk index version matches the binary version.
- **D-02:** Do not continue serving pre-rebuild results during a manual rebuild. Truthful blocked state is preferred over stale availability.

### Rebuild-Window Consistency
- **D-03:** If a clipboard entry is deleted while a rebuild is in progress, the deletion must be applied to both the active index data and the rebuild temp data immediately.
- **D-04:** Rebuild completion must never resurrect an entry that was deleted during the rebuild window.

### First-Time Backfill
- **D-05:** A profile that already has clipboard history but no usable search index should auto-trigger a full rebuild on the first unlocked opportunity.
- **D-06:** First-run search should aim for complete history coverage without requiring the user to discover and trigger a manual rebuild.

### Carried-Forward Guardrails
- **D-07:** Version mismatch blocks search immediately; do not return best-effort stale results.
- **D-08:** Rebuild failure leaves search blocked until a successful rebuild completes.
- **D-09:** Rebuild uses the version-flag strategy in `search_index_meta`, not `RENAME TABLE` swaps that require an exclusive-lock rename path.
- **D-10:** New entries captured during a rebuild window are double-written so they survive the final swap.
- **D-11:** Search remains profile-scoped from day one, and query/filter time semantics continue to use `active_time_ms` as the primary time axis.

### the agent's Discretion
- Exact temp-table naming, SQL statement layout, and transaction boundaries, as long as they preserve the blocked-state and no-exclusive-lock intent above.
- Exact `UPSERT` / replace strategy for active and temp writes during rebuild, as long as duplicate concurrent writes stay idempotent.
- Exact progress emission cadence inside rebuild, as long as the port contract and future daemon forwarding remain compatible.
- Exact integration-test harness shape and concurrency setup, as long as it proves AND/OR search correctness, mid-rebuild double-write behavior, and no deleted-entry resurrection.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase Scope And Acceptance
- `.planning/ROADMAP.md` §Phase 91 — Phase goal and success criteria for live AND/OR search, version-flag rebuild swap, mid-rebuild double-write, and stale-result guard.
- `.planning/REQUIREMENTS.md` §REBLD-01, §REBLD-02, §REBLD-03 — Full rebuild availability, version-flag atomic swap requirement, and rebuild-window double-write requirement.
- `.planning/STATE.md` — Current precondition to confirm `busy_timeout` and pool behavior before finalizing swap details.

### Search Semantics And Rebuild Rules
- `docs/architecture/local-encrypted-search.md` §查询执行 — Query execution order, primary ordering, and `active_time_ms` time semantics.
- `docs/architecture/local-encrypted-search.md` §全量重建 — Temp-table rebuild plus atomic cutover intent for V1.
- `docs/architecture/local-encrypted-search.md` §架构评审清单 / 2 / 4 / 5 / 6 / 8 / 10 — `entry_id` identity, hard-delete semantics, profile isolation, primary time field, strict query parsing, and rebuild-window double-write.
- `.planning/research/PITFALLS.md` — Known failure modes around version mismatch, query guard, and rebuild-window correctness.

### Landed Search Contracts
- `.planning/phases/88-core-domain-and-port-contracts/88-CONTEXT.md` — Locked `SearchIndexPort`, `SearchError`, `SearchQuery`, and `RebuildProgress` contracts.
- `.planning/phases/89-use-cases-and-delete-integration/89-CONTEXT.md` — Thin use-case boundary and delete-path cleanup expectations that Phase 91 must satisfy.
- `.planning/phases/90-sqlite-schema-migration-and-tokenizer-pipeline/90-CONTEXT.md` — Blocked-on-mismatch policy, auto-rebuild-after-unlock preference, profile isolation, and Phase 90 schema/tokenizer expectations carried into Phase 91.
- `src-tauri/crates/uc-core/src/ports/search/search_index.rs` — Exact adapter contract to implement.
- `src-tauri/crates/uc-core/src/search/query.rs` — Structured query shape, operator model, and filter fields the adapter must honor.
- `src-tauri/crates/uc-core/src/search/document.rs` — `SearchDocument`, `SearchPosting`, and `SearchIndexMeta` shapes the adapter persists and returns.

### Existing Infra Foundation
- `src-tauri/crates/uc-infra/migrations/2026-04-11-000001_create_search_index/up.sql` — Landed search tables and indexes that Phase 91 must use directly.
- `src-tauri/crates/uc-infra/src/search/rows.rs` — Adapter-owned row mappings, profile-scoped meta seeding, and domain conversion helpers.
- `src-tauri/crates/uc-infra/src/search/pipeline.rs` — Prebuilt document/posting materialization that feeds live indexing and rebuild input.
- `src-tauri/crates/uc-infra/src/search/search_key_derivation.rs` — Profile-scoped key derivation used by query-time term tagging.
- `src-tauri/crates/uc-infra/src/db/pool.rs` — WAL setup, `busy_timeout = 5000`, and shared pool behavior that constrain rebuild strategy choices.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src-tauri/crates/uc-infra/src/search/rows.rs`: Already contains row converters for `search_document`, `search_posting`, and `search_index_meta`, plus a `seed(profile_id)` helper for meta rows.
- `src-tauri/crates/uc-infra/src/search/pipeline.rs`: Already builds `(SearchDocument, Vec<SearchPosting>)` for both live indexing and rebuild input; the adapter should consume those objects directly rather than re-tokenizing.
- `src-tauri/crates/uc-infra/src/search/search_key_derivation.rs`: Already provides the profile-scoped search key derivation and HMAC tagging boundary needed for query execution.
- `src-tauri/crates/uc-app/src/usecases/search/`: The use cases are already thin wrappers over `SearchIndexPort`, so Phase 91 only needs to satisfy the port contract cleanly.

### Established Patterns
- `src-tauri/crates/uc-infra/src/db/pool.rs` already uses WAL plus a 5-second `busy_timeout` on a shared r2d2 pool; rebuild design should work with that pool instead of creating a second SQLite runtime or relying on exclusive-lock rename behavior.
- `profile_id` remains an infra-only persistence concern; `uc-core` search models do not carry it and Phase 91 must keep that ownership boundary intact.
- Explicit blocked state is the established product preference. Query paths should return typed `SearchError::IndexNotReady` instead of silently degrading or guessing.
- Search results are expected to return render-ready metadata directly from the adapter, not just matching IDs.

### Integration Points
- Phase 92 will inject `SqliteSearchIndex` into AppDeps and daemon routes; Phase 91 must expose a clean constructor boundary for that wiring.
- Live capture indexing will call `IndexClipboardEntry` with Phase 90 pipeline output; `index_entry()` must be idempotent and compatible with rebuild-window double writes.
- Delete flow already calls `remove_entry()` through Phase 89's best-effort cleanup path; Phase 91 should make the delete path correct when it succeeds and self-healing via rebuild when it fails.
- Full rebuild callers already provide `Vec<(SearchDocument, Vec<SearchPosting>)>` plus a progress sender; Phase 91 owns only the adapter-side persistence, cutover, and correctness rules.

</code_context>

<specifics>
## Specific Ideas

- Manual rebuild should feel honest: if the index is rebuilding, search should clearly be unavailable instead of pretending old results are still current.
- First-time search should feel complete. Existing clipboard history should not appear "missing" just because the search index starts empty.
- A deleted item must never reappear after rebuild cutover.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

### Reviewed Todos (not folded)
- `修复 setup 配对确认提示缺失` — matched by the todo tool but kept out of Phase 91 because it is a setup/UI issue unrelated to SQLite search indexing or rebuild behavior.

</deferred>

---

*Phase: 91-sqlite-index-adapter-and-rebuild-strategy*
*Context gathered: 2026-04-11*
