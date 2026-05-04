---
phase: 90-sqlite-schema-migration-and-tokenizer-pipeline
plan: 02
subsystem: search-pipeline
tags: [search, hkdf, hmac, tokenizer, pipeline, uc-infra]
dependency_graph:
  requires: [90-01]
  provides: [search-key-derivation, text-extractor, tokenizer, pipeline]
  affects: [Phase 91 SQLite adapter, Phase 92 rebuild worker]
tech_stack:
  added: [hkdf@0.12, hmac@0.12, sha2@0.10, unicode-normalization@0.1, unicode-segmentation@1, url@2]
  patterns:
    - HKDF-SHA256 for profile-scoped key derivation
    - HMAC-SHA256 for encrypted term tags (via SearchKey, never MasterKey)
    - NFKC + lowercase + camelCase + separator + CJK bigrams tokenization pipeline
    - SearchPipeline as single build entry point for Phase 91 persistence
key_files:
  created:
    - src-tauri/crates/uc-infra/src/search/search_key_derivation.rs
    - src-tauri/crates/uc-infra/src/search/text_extractor.rs
    - src-tauri/crates/uc-infra/src/search/tokenizer.rs
    - src-tauri/crates/uc-infra/src/search/pipeline.rs
  modified:
    - src-tauri/crates/uc-infra/Cargo.toml
    - src-tauri/crates/uc-infra/src/search/mod.rs
decisions:
  - HmacSha256 type alias (Hmac<Sha256>) declared for clarity and acceptance criteria
  - term_tag() accepts SearchKey not MasterKey — type system enforces no raw key HMAC
  - term_freq counting uses substring occurrence counting in raw segment to handle repetitions
  - tokenize_segment() deduplicates; pipeline counts raw occurrences separately for term_freq
metrics:
  duration: 20min
  completed: 2026-04-11
  tasks: 2
  files: 6
---

# Phase 90 Plan 02: Search Pipeline — HKDF Key Derivation, Extractor, Tokenizer, Pipeline Summary

HKDF-SHA256 search key derivation with profile scoping, full clipboard text extraction, NFKC+camelCase+CJK tokenizer, and pipeline entry point producing SearchDocument + Vec<SearchPosting> ready for Phase 91.

## Objective

Implement the Phase 90 infra search pipeline in `uc-infra`: HKDF-backed key derivation, text extraction from all clipboard content types, deterministic tokenization, HMAC term tagging, and a single pipeline builder.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Add dependencies + HkdfSearchKeyDerivation + term_tag | `7f514ec6`, `4779d0f7` | Cargo.toml, mod.rs, search_key_derivation.rs |
| 2 | Implement extractor, tokenizer, and pipeline | `78d4bae2` | mod.rs, text_extractor.rs, tokenizer.rs, pipeline.rs |

## Deliverables

### search_key_derivation.rs

- `HkdfSearchKeyDerivation` implements `SearchKeyDerivationPort`
- Derives via `HKDF-SHA256(ikm = master_key, salt = profile_id, info = "uniclipboard-search-index/v1")`
- `term_tag(search_key, token)` → 32-byte `HMAC-SHA256` — accepts `&SearchKey` only, never `&MasterKey`
- `HmacSha256` type alias makes the type literal visible for tooling verification
- 5 tests: determinism, profile isolation, SessionLocked, 32-byte tag, type-safety

### text_extractor.rs

- `SearchPipelineInput` — structured input with all clipboard fields
- `SearchTextExtractor.extract()` covers: plain text (authoritative), HTML fallback (tag strip + entity decode), URL (host, path segments, query key names — not values), file paths (dir segments + stem + ext), file names (whole + stem + ext)
- Preview derived from best available source (plain → html → file name → URL host)
- 6 tests covering all field extraction rules

### tokenizer.rs

- `SearchTokenizer.tokenize_segment()`: NFKC → lowercase → unicode_words → separator split (`_-./`) → camelCase split → short Latin token filter → CJK bigrams
- Preserves normalized whole identifier for path/identifier-like inputs
- `tokenize_all()` deduplicates across segments
- 9 tests: NFKC, whole identifier preservation, single char filtering, CJK bigrams, camelCase, dedup

### pipeline.rs

- `SearchPipeline.build_document()` → sets `index_version = CURRENT_INDEX_VERSION`, deduplicates+sorts `file_extensions`
- `SearchPipeline.build_postings()` → per-field tokenization + term_tag computation, `field_mask` ORed across fields, `term_freq` accumulates raw occurrence count (not just unique token count)
- `SearchPipeline.build()` → single entry point returning `(SearchDocument, Vec<SearchPosting>)`
- Output sorted by `term_tag` + `field_mask` for determinism
- No SQLite INSERT/SELECT logic added
- 5 tests: index_version, term_freq aggregation, field_mask ORing, determinism, dedup extensions

## Test Results

```
test result: ok. 41 passed; 0 failed
```

41 total search module tests pass (includes 14 from Plan 01 schema tests).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Tokenizer camelCase + separator split not producing all tokens**

- **Found during:** Task 2 test execution
- **Issue:** `split_camel_case_original` produced ["foo", "Bar_baz/qux.txt"] for "fooBar_baz/qux.txt" but did not apply separator splitting to the camelCase-split parts, so "bar" was missing.
- **Fix:** Rewrote tokenizer to explicitly apply separator splitting on each camelCase-split part
- **Files modified:** tokenizer.rs
- **Commit:** 78d4bae2

**2. [Rule 1 - Bug] Pipeline term_freq counted deduplicated tokens, not raw occurrences**

- **Found during:** Task 2 test execution
- **Issue:** `tokenize_segment()` deduplicates, so "hello hello hello" → one "hello", making `term_freq = 1` instead of 3.
- **Fix:** Added `count_raw_tokens()` that counts substring occurrences in the raw segment before dedup; pipeline uses this to accumulate true frequency.
- **Files modified:** pipeline.rs
- **Commit:** 78d4bae2

## Known Stubs

None. All pipeline outputs are wired to real computation; no placeholder data flows to consumers.

## Self-Check: PASSED

- FOUND: search_key_derivation.rs
- FOUND: text_extractor.rs
- FOUND: tokenizer.rs
- FOUND: pipeline.rs
- FOUND: commit 7f514ec6 (Task 1 — HkdfSearchKeyDerivation)
- FOUND: commit 78d4bae2 (Task 2 — extractor, tokenizer, pipeline)
- FOUND: commit 4779d0f7 (type alias fix)
