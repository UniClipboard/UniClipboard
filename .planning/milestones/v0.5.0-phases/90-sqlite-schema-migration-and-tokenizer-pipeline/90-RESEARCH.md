# Phase 90: SQLite Schema Migration and Tokenizer Pipeline - Research

**Researched:** 2026-04-11
**Domain:** Rust infra search pipeline (Diesel + SQLite + tokenizer/normalization + key derivation)
**Confidence:** HIGH

## Summary

Phase 90 should do two things and stop there:

1. Add the profile-scoped SQLite schema and migration artifacts needed for local encrypted search.
2. Build the pure infra pipeline that turns clipboard-relevant text into normalized tokens, derives a profile-scoped search key, and emits HMAC-tagged postings ready for storage.

The live `SearchIndexPort` adapter, query execution, rebuild orchestration, daemon routes, and UI all belong to later phases. This phase should leave Phase 91 with a stable schema, deterministic tokenizer behavior, and reusable helpers that can produce `SearchDocument` plus `Vec<SearchPosting>` without exposing plaintext tokens on disk.

The most important research outcome is the key-derivation decision: **Phase 90 should use HKDF-SHA256 to derive `SearchKey` from `MasterKey`, and HMAC-SHA256 for `term_tag` generation. Do not use `blake3::derive_key` or `blake3::keyed_hash` for this phase.** The authoritative sources are the current Phase 90 context, `docs/architecture/local-encrypted-search.md`, `.planning/STATE.md`, and the landed `SearchKeyDerivationPort` contract in `uc-core`, all of which now point to HKDF/HMAC. The older `blake3` recommendation in `.planning/research/STACK.md` is stale and should not drive Phase 90 planning.

Another key planning point is contract alignment: the schema must include `profile_id` on all three search tables, but that does **not** require adding `profile_id` to `uc_core::search::SearchDocument` or `SearchPosting`. Profile scoping is an infra persistence concern for this phase and should be carried by adapter-owned DB row structs plus `KeyScopePort`, not by widening already-landed Phase 88/89 domain contracts unless execution proves a hard blocker.

<user_constraints>

## User Constraints (from 90-CONTEXT.md)

### Locked Decisions

- Prefer `text/plain` over HTML when both are available.
- HTML indexing is visible-text only; no raw tags or noisy attributes.
- URL indexing includes host, path segments, and query parameter names, but not query values.
- File/path indexing includes file names, extensions, and directory segments; not one giant raw path token.
- Tokenization must do NFKC normalization, lowercase normalization, Latin word-boundary splitting, separator splitting (`_`, `-`, `.`, `/`), camelCase/PascalCase splitting, and CJK bigrams.
- Preserve both split identifier/path parts and the original whole segment.
- Drop single-character Latin noise; keep Latin tokens with length `>= 2`.
- Search must block on tokenizer/index version mismatch rather than serving stale results.
- Rebuild state must be explicit and truthful; no vague degraded mode.
- Persist both `active_time_ms` and `captured_at_ms`.
- Persist stable `file_type`, original `mime_type`, and unique file extensions.
- All search tables, including metadata, are profile-scoped from day one.
- The same plaintext token must not yield the same tag across different profiles.

### Planner's Discretion

- Exact SQLite column ordering, index names, and SQL types.
- Exact helper/module layout inside `uc-infra::search`.
- Exact HKDF byte layout as long as it stays purpose-separated and profile-scoped.
- Exact summary truncation heuristic, as long as persisted rows stay lean and user-facing summaries remain scannable.

### Deferred Ideas (OUT OF SCOPE)

- Query execution SQL and AND/OR posting resolution.
- Rebuild double-write behavior and version-flag swap implementation details.
- Daemon unlock guards, HTTP routes, and WS progress events.
- Frontend search UX and result rendering.

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description |
|----|-------------|
| SIDX-03 | Index terms are stored as `HMAC(search_key, normalized_token)` — no plaintext tokens written to disk |
| SIDX-04 | `search_key` is derived from `MasterKey` via HKDF, scoped per profile |
| SIDX-05 | Text is extracted from plain text, HTML, URL, file paths, and file names for indexing |
| SIDX-06 | Tokenization uses word-boundary splitting for Latin text and bigram generation for CJK text |
| SIDX-07 | Index schema includes `index_version` to enable safe full rebuild when normalization rules change |

</phase_requirements>

## Standard Stack

### Core

| Library / Module | Version / Origin | Purpose |
|------------------|------------------|---------|
| Diesel + embedded migrations | existing `uc-infra` stack | Schema creation and migration execution |
| SQLite WAL pool | `src-tauri/crates/uc-infra/src/db/pool.rs` | Runtime DB integration and migration smoke tests |
| `SearchDocument`, `SearchPosting`, `SearchIndexMeta` | `uc-core` Phase 88 output | Domain contract consumed by use cases |
| `SearchKey`, `SearchKeyDerivationPort` | `uc-core` Phase 88 output | Opaque derived key boundary |
| `KeyScopePort` | existing security port | Supplies active `profile_id` for persistence and key scoping |
| `EncryptionSession` / `MasterKey` access path | existing security runtime | Input key material for HKDF derivation |

### New Dependencies Required in `uc-infra`

| Crate | Why |
|-------|-----|
| `hkdf` | HKDF-SHA256 derivation required by current architecture and port contract |
| `hmac` | HMAC-SHA256 term tagging required by SIDX-03 and current comments/contracts |
| `sha2` | SHA-256 digest for both HKDF and HMAC |
| `unicode-normalization` | NFKC normalization before tokenization |
| `unicode-segmentation` | Unicode-aware Latin word boundary splitting |
| `url` | Direct host/path/query-key parsing in the infra extractor without pushing new helpers into `uc-core` |

### Supporting Existing Modules

| Module | Purpose | When to Use |
|--------|---------|-------------|
| `src-tauri/crates/uc-core/src/clipboard/link_utils.rs` | `text/uri-list` parsing and single/all-URL detection | Reuse for URI list classification before deeper segment extraction |
| `src-tauri/crates/uc-infra/src/clipboard/normalizer.rs` | Existing text MIME heuristics and safe preview truncation patterns | Reuse for text-type detection and friendly summary truncation |
| `src-tauri/crates/uc-infra/src/db/schema.rs` | Diesel schema source of truth | Extend with three search tables after migration |
| `src-tauri/crates/uc-infra/src/db/pool.rs` | Embedded migration runner and current busy timeout | Use for migration smoke tests and future adapter integration |

## Contract Alignment Notes

### 1. Follow the resolved design, not stale draft fields

`docs/architecture/local-encrypted-search.md` still contains early-draft fields such as `deleted_at_ms` and `rebuild_state`. Later sections in the same doc, the current Phase 90 context, and landed `uc-core` contracts have already resolved:

- hard-delete semantics
- no `deleted_at_ms` on the search document contract
- explicit `search_blocked` behavior
- profile-scoped key derivation

Phase 90 planning should follow the resolved design and current code contracts, not the stale draft column list.

### 2. Keep profile scoping in infra rows for this phase

Phase success criteria require `profile_id` on all search tables, but the current `SearchDocument` and `SearchPosting` domain structs do not contain that field. The clean Phase 90 approach is:

- keep `profile_id` in adapter-owned Diesel row structs and SQL schema
- obtain `profile_id` through `KeyScopePort`
- leave Phase 88 domain structs unchanged unless execution proves they block persistence or testing

This avoids turning a persistence concern into a cross-crate domain change.

### 3. `search_index_meta` should match current Phase 90 needs, not Phase 91 rebuild fullness

Phase 90 only needs enough metadata to:

- expose the active `index_version`
- expose `search_blocked`
- record rebuild timestamps
- scope metadata by `profile_id`

If Phase 91 needs richer rebuild-state details, extend `SearchIndexMeta` deliberately there instead of overloading Phase 90 with future adapter concerns.

### 4. Keep the live adapter out of this phase

This phase can define helper modules, persistence row types, and maybe a pipeline facade such as `SearchIndexMaterializer`, but it should **not** try to finish `SqliteSearchIndex`. Query SQL, version-flag swaps, rebuild windows, and double-write logic belong to Phase 91.

## Recommended Module Shape

The safest Phase 90 structure in `uc-infra` is:

```text
src-tauri/crates/uc-infra/src/search/
├── mod.rs
├── constants.rs
├── text_extractor.rs
├── tokenizer.rs
├── pipeline.rs
├── search_key_derivation.rs
└── rows.rs
```

- `constants.rs` owns `CURRENT_INDEX_VERSION`, field-mask bits, separator rules, and any stable purpose string.
- `text_extractor.rs` converts clipboard-relevant inputs into structured raw search text fields.
- `tokenizer.rs` performs normalization and token emission.
- `pipeline.rs` produces `SearchDocument` plus deduplicated `SearchPosting` values from extracted content.
- `search_key_derivation.rs` implements `SearchKeyDerivationPort`.
- `rows.rs` owns Diesel-facing row structs with `profile_id` and any meta-row shape.

The phase will also touch:

- `src-tauri/crates/uc-infra/Cargo.toml`
- `src-tauri/crates/uc-infra/src/lib.rs`
- `src-tauri/crates/uc-infra/src/db/schema.rs`
- `src-tauri/crates/uc-infra/migrations/<new-search-migration>/up.sql`
- `src-tauri/crates/uc-infra/migrations/<new-search-migration>/down.sql`

## Architecture Patterns

### Pattern 1: Extract once, tokenize once, tag once

Build a single infra pipeline that:

1. extracts raw text fields
2. normalizes and tokenizes them
3. derives the profile-scoped search key
4. emits deduplicated postings with `term_freq` and `field_mask`

Do not scatter extraction rules in one module, tokenizer rules in another crate, and HMAC tagging in call sites. Phase 90 should create one authoritative pipeline entry point.

### Pattern 2: Plain domain contracts, richer infra rows

The domain layer should stay focused on search behavior, not persistence scoping details. The SQLite layer can add:

- `profile_id`
- row primary/composite keys
- adapter-only metadata columns

without forcing those fields into `uc-core`.

### Pattern 3: Deterministic tokenizer version ownership

`CURRENT_INDEX_VERSION` should live next to the tokenizer rules and be bumped whenever:

- normalization changes
- separator splitting rules change
- camelCase/PascalCase logic changes
- CJK handling changes

This keeps version truth next to the behavior that can invalidate old postings.

### Pattern 4: Explicit field extraction with source masks

Treat each field as a first-class extraction source:

- body text
- HTML visible text
- URL text
- file path text
- file name text

Then convert those sources into the existing `field_mask` bit positions. This avoids losing explainability when Phase 91 later needs to rank or debug hits.

### Pattern 5: Stable search key derivation boundary

Implement `SearchKeyDerivationPort` in `uc-infra`, but keep `MasterKey` handling fully inside that adapter. The tokenizer/pipeline should accept `SearchKey`, never `MasterKey`.

Recommended derivation shape:

```text
HKDF-SHA256(
  ikm = master_key.as_bytes(),
  salt = profile_id.as_bytes(),
  info = b"uniclipboard-search-index/v1"
) -> 32-byte SearchKey
```

Then generate term tags as:

```text
HMAC-SHA256(search_key, normalized_token_bytes) -> 32-byte term_tag
```

This is deterministic, profile-scoped, purpose-separated, and matches the current port intent.

## Extraction and Tokenization Guidance

### Text Extraction Rules

- `text/plain`: authoritative source when present.
- `text/html`: use only when the entry is HTML-first or when no plain-text form exists.
- `text/uri-list`: parse each URI, index host, path segments, and query parameter names.
- file-like payloads: index directory segments, file stem, extension, and display file name.
- multi-file entries: deduplicate extensions across all files before writing `SearchDocument.file_extensions`.

### HTML Handling

- Strip tags and preserve visible text only.
- Decode the common entities needed for normal clipboard content (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`, `&nbsp;`).
- Do not add a heavy HTML parser for V1 unless execution proves the manual stripper breaks representative samples.

### Tokenization Rules

- Normalize with Unicode NFKC first.
- Lowercase after normalization.
- Split Latin text with Unicode word boundaries.
- Additionally split on `_`, `-`, `.`, `/`.
- Additionally split camelCase and PascalCase transitions.
- Preserve both the whole original segment and the split parts for identifiers and path-like text.
- Drop single-character Latin tokens.
- Keep numbers when they occur inside useful identifiers/path segments.
- For CJK, generate overlapping bigrams over contiguous CJK runs.
- Deduplicate identical `(term, field)` contributions before computing final `term_freq`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead |
|---------|-------------|-------------|
| Search key derivation | Direct `MasterKey` use or `blake3::derive_key` | `hkdf` + `sha2` in `uc-infra`, returning `SearchKey` |
| Term tag generation | Plain hashes or `blake3::keyed_hash` | `hmac` + `sha2` over normalized token bytes |
| SQLite integration | A second DB library or ad hoc file writes | Existing Diesel migration pipeline and pool |
| URL parsing | Regex-only parsing | `url` crate + existing `link_utils` helpers |
| HTML extraction | Browser-grade parser for V1 | Small visible-text stripper plus entity decode |
| Tokenization | Regex split on whitespace only | Unicode normalization + segmentation + explicit separator rules |
| Profile isolation | Global key or global search tables | `profile_id` on all tables + `KeyScopePort` |

## Common Pitfalls

### Pitfall 1: Treating `.planning/research/STACK.md` as authoritative for crypto

That document still recommends `blake3` for derivation and tagging. Current Phase 90 context and `uc-core` contracts do not. If Phase 90 follows the older stack note, the implementation will conflict with success criteria and the landed search comments.

### Pitfall 2: Forcing `profile_id` into `uc-core` too early

Adding `profile_id` to `SearchDocument`/`SearchPosting` just to satisfy the SQLite schema would create cross-crate churn before there is a real domain need. Keep it in infra rows for now.

### Pitfall 3: Re-introducing soft-delete semantics through the migration

`deleted_at_ms` was explicitly resolved away. Re-adding it to the search tables would create a schema/workflow contradiction with the hard-delete contract already landed.

### Pitfall 4: Splitting tokenizer truth across helpers

If NFKC/lowercase lives in one helper, separator splitting in another, and camelCase logic in a third, nobody owns `index_version`. Keep the full normalization grammar under one module.

### Pitfall 5: HMACing with raw `MasterKey`

Functional tests may still pass, but it breaks purpose separation and profile isolation. The only HMAC input key in Phase 90 should be `SearchKey`.

### Pitfall 6: Over-scoping into Phase 91

If this phase starts writing live query SQL or rebuild swap logic, it will mix persistence foundations with adapter behavior and produce awkward commits. Stop at schema + pipeline.

## Validation Architecture

### Test Framework

- Rust unit tests inside `uc-infra`.
- Migration smoke tests using the existing `init_db_pool` / embedded Diesel migrations path.
- No new test framework is required.

### Phase Requirements -> Test Map

| Requirement | Test Target | Test Type |
|-------------|-------------|-----------|
| SIDX-03 | `search_key_derivation` + posting builder emit 32-byte HMAC tags and never persist plaintext tokens | unit |
| SIDX-04 | HKDF derivation is deterministic per profile and differs across profiles | unit |
| SIDX-05 | extractor covers plain text, HTML fallback, URL segments/query keys, file paths, and file names | unit |
| SIDX-06 | tokenizer covers Latin word boundaries, separator splits, camelCase/PascalCase splits, and CJK bigrams | unit |
| SIDX-07 | migration creates all three tables with `profile_id`; meta table exposes `index_version` | migration smoke / static schema assertion |

### Sampling Rate

- After each task commit: `cd src-tauri && cargo test -p uc-infra search::`
- After each plan wave: `cd src-tauri && cargo test -p uc-infra`
- Before phase verification: `cd src-tauri && cargo test -p uc-infra && cargo check -p uc-infra`

### Likely Wave 0 Gaps

- No existing `uc-infra::search` module or test namespace yet.
- No current migration smoke test that asserts search table columns by name.
- No current shared helper for representative HTML/url/file extraction fixtures.

## Sources

### Primary (HIGH confidence)

- `.planning/phases/90-sqlite-schema-migration-and-tokenizer-pipeline/90-CONTEXT.md`
- `.planning/ROADMAP.md`
- `.planning/REQUIREMENTS.md`
- `.planning/STATE.md`
- `docs/architecture/local-encrypted-search.md`
- `src-tauri/crates/uc-core/src/search/document.rs`
- `src-tauri/crates/uc-core/src/search/key.rs`
- `src-tauri/crates/uc-core/src/search/result.rs`
- `src-tauri/crates/uc-core/src/ports/search/search_index.rs`
- `src-tauri/crates/uc-core/src/ports/search/search_key.rs`
- `src-tauri/crates/uc-core/src/clipboard/link_utils.rs`
- `src-tauri/crates/uc-infra/src/clipboard/normalizer.rs`
- `src-tauri/crates/uc-infra/src/db/pool.rs`
- `src-tauri/crates/uc-infra/src/db/schema.rs`
- `src-tauri/crates/uc-infra/Cargo.toml`
- `src-tauri/crates/uc-infra/src/security/encryption.rs`
- `src-tauri/crates/uc-platform/src/key_scope.rs`

### Secondary (MEDIUM confidence)

- `.planning/research/ARCHITECTURE.md`
- `.planning/research/PITFALLS.md`
- `.planning/research/STACK.md` (used only as a stale-reference counterexample for the crypto decision)
- `.planning/phases/88-core-domain-and-port-contracts/88-CONTEXT.md`
- `.planning/phases/89-use-cases-and-delete-integration/89-CONTEXT.md`

## Metadata

- Scope: infra foundation only; no daemon/UI work
- Risk level: medium-high (crypto choice, contract alignment, tokenizer determinism)
- Recommended plan count: 2
- Recommended execution split:
  - Plan 01: migration, schema, row models, constants, and migration tests
  - Plan 02: extractor, tokenizer, HKDF derivation, HMAC tagging, and pipeline tests
