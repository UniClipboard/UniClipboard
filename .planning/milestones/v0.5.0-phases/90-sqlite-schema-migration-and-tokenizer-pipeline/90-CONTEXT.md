# Phase 90: SQLite Schema Migration and Tokenizer Pipeline - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Create the SQLite search schema in `uc-infra` and implement the extraction, normalization,
tokenization, and search-key-derivation pipeline that turns clipboard-relevant text into
profile-scoped HMAC-tagged terms ready for storage. This phase prepares data for indexing only.
It does not add query execution, daemon routes, rebuild orchestration, or UI.

</domain>

<decisions>
## Implementation Decisions

### Text Extraction Boundary
- **D-01:** When the same entry exposes both plain text and HTML, plain text is the authoritative searchable source. HTML is only used when the indexed representation itself is HTML or when no plain-text representation is available.
- **D-02:** HTML extraction is visible-text only. Do not index raw tags or generic attribute noise.
- **D-03:** URL extraction includes host, path segments, and query parameter names. Do not index query parameter values in V1.
- **D-04:** File/path extraction includes file names, file extensions, and directory segments. Do not index the full raw path as one long literal.

### Tokenization Behavior
- **D-05:** Carry forward the tokenizer baseline from the architecture spec: Unicode NFKC normalization, lowercase normalization, Latin word-boundary tokenization, and CJK bigram generation.
- **D-06:** Additionally split common code/path separators such as `_`, `-`, `.`, and `/` so identifiers, file names, and paths are searchable by parts.
- **D-07:** Additionally split camelCase and PascalCase identifiers into constituent words.
- **D-08:** Preserve both the split parts and the original whole segment for identifiers/path-like text.
- **D-09:** Keep short Latin tokens with length >= 2; drop single-character Latin tokens as noise.

### Version Mismatch and Rebuild UX
- **D-10:** If the binary's tokenizer/index version does not match the on-disk active index version, search must be blocked rather than serving best-effort stale results.
- **D-11:** Version-mismatch rebuild should start automatically on the first unlocked opportunity.
- **D-12:** Rebuild state should be communicated explicitly to the user; do not hide it behind a vague busy state.
- **D-13:** If rebuild fails, keep search blocked until rebuild succeeds. Do not fall back to the old index.

### Search Result Summary Style
- **D-14:** Search result summaries should be short and highly scannable rather than raw payload dumps.
- **D-15:** Link-like results should prefer human-readable text first, with the URL only as a fallback.
- **D-16:** File-like results should prefer file-name-oriented summaries. Multi-file summaries should show representative file names plus a count, not full paths.

### Search Document Metadata Shape
- **D-17:** The persisted SQLite search document row should stay lean. It must carry the metadata needed for filtering, ordering, and isolation, but it should not become a fat render-ready object.
- **D-18:** Human-readable result summaries are not required to live in the persisted SQLite search document row. If summaries are needed later, they should be enriched outside the persisted index row path.
- **D-19:** Persist both `active_time_ms` and `captured_at_ms`.
- **D-20:** Persist both the stable search `file_type` enum and the original `mime_type`.
- **D-21:** Persist the full set of unique file extensions for multi-file entries so extension filtering can work without lossy heuristics.

### Profile Isolation Boundary
- **D-22:** Profile isolation applies to every search table, including index metadata / rebuild state, from day one.
- **D-23:** Rebuild operations are scoped to the current profile only.
- **D-24:** Every search table should include an explicit `profile_id` column even if the runtime commonly operates on the default profile today.
- **D-25:** The same plaintext token must not produce the same index tag across different profiles.

### the agent's Discretion
- Exact SQL column types, index names, and column ordering, as long as the schema preserves the decisions above and matches the current Rust contract or explicitly plans any required follow-up changes.
- Exact HKDF info-string byte layout and helper structure, as long as derivation remains purpose-separated and profile-scoped per the canonical references.
- Exact separator list beyond the explicitly approved `_`, `-`, `.`, `/`, and the precise camelCase/PascalCase splitting heuristics.
- Exact summary truncation length and where summary enrichment is performed, as long as summaries stay short/scannable and the persisted SQLite row remains lean.
- Exact rebuild-status wording and event names, as long as the product state is explicit rather than vague.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Primary Spec
- `docs/architecture/local-encrypted-search.md` — V1 local encrypted search design: extraction targets, tokenizer baseline, key derivation intent, schema concepts, and resolved architecture review items.

### Milestone Planning
- `.planning/ROADMAP.md` §Phase 90 — Phase goal, success criteria, and research flag for key-derivation resolution.
- `.planning/REQUIREMENTS.md` — Phase 90 requirements `SIDX-03` through `SIDX-07`.
- `.planning/STATE.md` — Current phase preconditions and warnings that affect Phase 90 planning.

### Prior Phase Context
- `.planning/phases/88-core-domain-and-port-contracts/88-CONTEXT.md` — Search domain contracts, hard-delete semantic, `SearchKey` boundary, and search-model ownership.
- `.planning/phases/89-use-cases-and-delete-integration/89-CONTEXT.md` — Use-case boundary that keeps tokenization/HMAC work out of `uc-app` and squarely inside Phase 90 infra.

### Research Notes
- `.planning/research/ARCHITECTURE.md` — Layer placement, migration pipeline, key-scope usage, and infra integration guidance for the search subsystem.
- `.planning/research/PITFALLS.md` — High-risk failure modes for key separation, profile isolation, version mismatch handling, and rebuild behavior.

### Implemented Contracts
- `src-tauri/crates/uc-core/src/search/document.rs` — Current `SearchDocument`, `SearchPosting`, and `SearchIndexMeta` shapes that the Phase 90 schema must honor or deliberately evolve.
- `src-tauri/crates/uc-core/src/search/key.rs` — Opaque `SearchKey` contract; no serialization and redacted debug output.
- `src-tauri/crates/uc-core/src/ports/search/search_key.rs` — `SearchKeyDerivationPort` contract and current HKDF-scoped expectation.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src-tauri/crates/uc-infra/src/db/pool.rs` — Existing embedded Diesel migration pipeline (`embed_migrations!` + `run_pending_migrations`) means Phase 90 only needs new migration files plus schema refresh.
- `src-tauri/crates/uc-core/src/search/key.rs` — SearchKey newtype already exists and gives Phase 90 a safe boundary for HMAC inputs.
- `src-tauri/crates/uc-core/src/clipboard/link_utils.rs` — Existing helpers already parse `text/uri-list`, detect URLs, and extract domains; reuse them for link/file-path extraction instead of inventing new parsing rules.
- `src-tauri/crates/uc-infra/src/clipboard/normalizer.rs` — Existing text MIME detection and safe preview truncation patterns can inform extractor behavior and summary shaping.
- `src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` — Current human-readable preview behavior for links/files is a good reference if later phases enrich search summaries outside the persisted index row.

### Established Patterns
- Search key derivation is already expected to be HKDF-scoped behind `SearchKeyDerivationPort`; raw `MasterKey` bytes must not cross the search boundary.
- Clipboard-domain code already distinguishes `text/plain`, `text/html`, and `text/uri-list`; the search extractor should reuse these classifications instead of creating a second interpretation layer.
- The current runtime already has `KeyScopePort` with a default `profile_id = "default"`; Phase 90 should still persist explicit `profile_id` columns from day one.
- Existing planning docs and pitfall analysis strongly favor explicit, truthful blocked states over silent degraded behavior.

### Integration Points
- `src-tauri/crates/uc-infra/migrations/` and `src-tauri/crates/uc-infra/src/db/schema.rs` — Search tables enter through the existing migration/schema toolchain.
- `src-tauri/crates/uc-core/src/search/document.rs` and `src-tauri/crates/uc-core/src/ports/search/search_key.rs` — Phase 90 must align SQLite schema and key-derivation behavior with the already-landed domain contracts.
- Future `uc-infra::search` modules will feed the Phase 89 use cases that already accept prebuilt `SearchDocument` / `SearchPosting` values; keep tokenization and HMAC work out of `uc-app`.
- Any later summary enrichment outside the index row should align with existing clipboard projection logic to avoid a second, conflicting summary rule set.

</code_context>

<specifics>
## Specific Ideas

- Search should feel friendly to code snippets, commands, identifiers, paths, and mixed-language clipboard content — not just plain natural-language prose.
- If the indexed representation is HTML, treat it as visible text, not raw markup.
- Result lists should be fast to scan: short summaries, human-readable text for links when available, and file names over raw paths.
- Different profiles must not share term tags or rebuild state.

</specifics>

<deferred>
## Deferred Ideas

- Richer HTML attribute indexing (for example `href`, `title`, or `alt` beyond visible text) — defer unless real recall gaps justify the extra noise.
- URL query-value indexing and full raw-path indexing — defer to later tuning if the balanced extraction strategy proves insufficient.
- Turning the persisted SQLite search document row into a fully render-ready object — defer unless later performance measurements show that external summary enrichment is too expensive.

### Reviewed Todos (not folded)
- `修复 setup 配对确认提示缺失` — reviewed during discuss-phase; not folded because it is a UI/setup concern unrelated to Phase 90 search schema and tokenizer infrastructure.

</deferred>

---

*Phase: 90-sqlite-schema-migration-and-tokenizer-pipeline*
*Context gathered: 2026-04-10*
