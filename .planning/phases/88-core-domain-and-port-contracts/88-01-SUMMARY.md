---
phase: 88-core-domain-and-port-contracts
plan: 01
subsystem: search
tags: [rust, uc-core, async-trait, thiserror, serde, tokio, search-domain, ports, hexagonal-architecture]

# Dependency graph
requires: []
provides:
  - SearchError typed enum (thiserror): InvalidQuery, SessionLocked, IndexNotReady, IndexUnavailable, Internal
  - SearchKey opaque [u8;32] newtype with redacted Debug, no Serialize/Deserialize
  - SearchDocument (hard-delete, no soft-delete timestamp), SearchPosting, SearchIndexMeta, FileType
  - SearchQuery with QueryOperator (And/Or), TimeRangeFilter (preset + Absolute { from_ms, to_ms })
  - SearchResult with D-01 exact fields; RebuildStage enum; RebuildProgress struct
  - SearchIndexPort async trait: index_entry, remove_entry, search, rebuild, get_index_meta
  - SearchKeyDerivationPort async trait: derive_search_key
  - All types exported from uc_core::search and uc_core::ports
affects: [89-search-use-cases, 90-search-key-derivation, 91-search-index-infra, 92-search-daemon-routes, 93-search-ui]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - 'Opaque newtype for key material: [u8;32] + custom Debug (REDACTED) + no Serialize/Deserialize'
    - 'Port trait shape: #[async_trait] pub trait FooPort: Send + Sync { async fn method() -> Result<T, TypedError> }'
    - 'Hard-delete domain constraint: struct has no soft-delete timestamp field by design'
    - 'Channel-based progress: rebuild method takes tokio::sync::mpsc::Sender<RebuildProgress>'
    - 'Include_str! compile-time invariant tests for struct shape enforcement'

key-files:
  created:
    - src-tauri/crates/uc-core/src/search/mod.rs
    - src-tauri/crates/uc-core/src/search/error.rs
    - src-tauri/crates/uc-core/src/search/key.rs
    - src-tauri/crates/uc-core/src/search/query.rs
    - src-tauri/crates/uc-core/src/search/document.rs
    - src-tauri/crates/uc-core/src/search/result.rs
    - src-tauri/crates/uc-core/src/ports/search/mod.rs
    - src-tauri/crates/uc-core/src/ports/search/search_index.rs
    - src-tauri/crates/uc-core/src/ports/search/search_key.rs
  modified:
    - src-tauri/crates/uc-core/src/lib.rs
    - src-tauri/crates/uc-core/src/ports/mod.rs

key-decisions:
  - 'SearchKey uses MasterKey pattern (pub as_bytes()) — HMAC computation is Phase 90 infra concern, not in uc-core'
  - 'SearchDocument has no deleted_at_ms field — hard-delete is the resolved semantic (arch spec Q4)'
  - 'TimeRangeFilter uses #[serde(tag = "kind")] for tagged enum serialization of preset vs Absolute variants'
  - 'search_document_has_no_deleted_at_field test uses split-string concat trick to avoid include_str! self-detection'
  - 'RebuildProgress channel mirrors TransferProgress pattern already in codebase'

patterns-established:
  - 'Port trait: #[async_trait] pub trait FooPort: Send + Sync with Result<T, TypedError>'
  - 'SearchKey: opaque [u8;32] newtype, no serde derives, custom redacted Debug'
  - 'Domain types in uc-core/src/search/; ports in uc-core/src/ports/search/'

requirements-completed: [FOUNDATION-v0.5.0]

# Metrics
duration: 30min
completed: 2026-04-10
---

# Phase 88 Plan 01: Core Domain and Port Contracts Summary

**Compiler-enforced search contract in uc-core: 12 domain types, 2 async port traits, 19 unit tests — zero implementations, cargo check --workspace green**

## Performance

- **Duration:** ~30 min
- **Started:** 2026-04-10T00:00:00Z
- **Completed:** 2026-04-10T00:30:00Z
- **Tasks:** 2
- **Files modified:** 11 (9 created, 2 modified)

## Accomplishments

- Defined the complete search type system in uc-core with no downstream breakage (cargo check --workspace passes)
- SearchIndexPort and SearchKeyDerivationPort are object-safe Arc<dyn Trait + Send + Sync> async traits returning Result<T, SearchError>
- 19 inline unit tests verify SearchKey Debug redaction, serde round-trips for all public DTOs, hard-delete invariant on SearchDocument, and port object-safety

## Task Commits

1. **Task 1: Create uc-core search domain module** - `119597c9` (feat)
2. **Task 2: Create uc-core search ports module and wire into workspace** - `1bf3c716` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/search/error.rs` - SearchError thiserror enum: InvalidQuery, SessionLocked, IndexNotReady, IndexUnavailable, Internal
- `src-tauri/crates/uc-core/src/search/key.rs` - SearchKey opaque [u8;32] with redacted Debug, as_bytes(), from_bytes(); no Serialize/Deserialize
- `src-tauri/crates/uc-core/src/search/document.rs` - FileType enum, SearchDocument (no soft-delete), SearchPosting, SearchIndexMeta
- `src-tauri/crates/uc-core/src/search/query.rs` - SearchQuery, QueryOperator (And/Or), TimeRangeFilter (7 presets + Absolute)
- `src-tauri/crates/uc-core/src/search/result.rs` - SearchResult (D-01 fields), RebuildStage, RebuildProgress
- `src-tauri/crates/uc-core/src/search/mod.rs` - Re-exports all public search domain types
- `src-tauri/crates/uc-core/src/ports/search/search_index.rs` - SearchIndexPort: index_entry, remove_entry, search, rebuild (mpsc Sender), get_index_meta
- `src-tauri/crates/uc-core/src/ports/search/search_key.rs` - SearchKeyDerivationPort: derive_search_key
- `src-tauri/crates/uc-core/src/ports/search/mod.rs` - Re-exports SearchIndexPort + SearchKeyDerivationPort
- `src-tauri/crates/uc-core/src/lib.rs` - Added `pub mod search;`
- `src-tauri/crates/uc-core/src/ports/mod.rs` - Added `pub mod search;` and re-exports for both port traits

## Decisions Made

- **SearchKey follows MasterKey pattern:** `pub as_bytes()` is the sole raw bytes accessor. No `compute_term_tag` method in uc-core — HMAC is Phase 90 infra. Consistent with how MasterKey is used.
- **Hard-delete enforced at struct level:** SearchDocument has no `deleted_at_ms` field. The `include_str!` compile-time test uses split-string concat (`["deleted", "_at_ms"].concat()`) to avoid self-detecting the test string.
- **TimeRangeFilter tagged enum:** `#[serde(tag = "kind", rename_all = "snake_case")]` gives clean JSON like `{"kind":"last_7d"}` for presets and `{"kind":"absolute","from_ms":...,"to_ms":...}` for ranges.
- **No Cargo.toml changes needed:** async-trait, thiserror, serde, tokio/sync were already in uc-core dependencies.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed include_str! self-detection in hard-delete test**

- **Found during:** Task 1 (search domain module creation)
- **Issue:** The plan's `search_document_has_no_deleted_at_field` test used `src.contains("deleted_at_ms")` with `include_str!("document.rs")`. The doc comment "no `deleted_at_ms` field" and the test assertion message both contain the forbidden string, causing the test to fail on the first run.
- **Fix:** Rewrote doc comments to avoid the literal string; used `["deleted", "_at_ms"].concat()` in the test to build the forbidden pattern at runtime, preventing self-detection.
- **Files modified:** src-tauri/crates/uc-core/src/search/document.rs
- **Verification:** Test passes (`search_document_has_no_deleted_at_field` ok)
- **Committed in:** 119597c9 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 - bug in test design)
**Impact on plan:** Minor test implementation fix. No scope creep. All plan contracts honored exactly.

## Issues Encountered

- `include_str!` approach for compile-time invariant tests reads the entire source file — doc comments mentioning the forbidden field name can cause false failures. Fixed via string split-concat.

## Exported Types (for downstream phases)

All resolvable from `uc_core::search::` or `uc_core::ports::`:

| Type | Module | Purpose |
|------|--------|---------|
| `SearchError` | `uc_core::search` | Port boundary error enum |
| `SearchKey` | `uc_core::search` | Opaque 32-byte HMAC key |
| `SearchQuery` | `uc_core::search` | Structured query input |
| `QueryOperator` | `uc_core::search` | And/Or enum |
| `TimeRangeFilter` | `uc_core::search` | Preset + Absolute range |
| `FileType` | `uc_core::search` | Content type classification |
| `SearchDocument` | `uc_core::search` | Inverted index document row |
| `SearchPosting` | `uc_core::search` | (term_tag, entry_id) pair |
| `SearchIndexMeta` | `uc_core::search` | Index metadata projection |
| `SearchResult` | `uc_core::search` | Query result with render metadata |
| `RebuildStage` | `uc_core::search` | Started/Indexing/Complete/Failed |
| `RebuildProgress` | `uc_core::search` | Progress channel payload |
| `SearchIndexPort` | `uc_core::ports` | Index CRUD + query + rebuild |
| `SearchKeyDerivationPort` | `uc_core::ports` | Derive search key from session |

## Verification Results

- `cargo check -p uc-core`: PASS
- `cargo test -p uc-core --lib -- search`: PASS (19/19 tests)
- `cargo check --workspace`: PASS (uc-app, uc-infra, uc-daemon, uc-tauri all compile)
- Grep audit: key.rs has no Serialize/Deserialize derive macros on SearchKey
- Grep audit: document.rs has no `deleted_at_ms` string
- Grep audit: search_index.rs returns `Vec<SearchResult>` (not Vec<EntryId>)
- Grep audit: ports/search/ has no `anyhow::Result` on trait method signatures

## Next Phase Readiness

- Phase 89 (search use cases): can import `SearchIndexPort`, `SearchKeyDerivationPort`, `SearchQuery`, `SearchResult`, `SearchError` from uc-core
- Phase 90 (key derivation infra): can receive `MasterKey`, return `SearchKey` via `SearchKeyDerivationPort`
- Phase 91 (index infra): can implement `SearchIndexPort`, write `SearchDocument`/`SearchPosting` to SQLite
- Phase 92 (daemon routes): can map `SearchError::InvalidQuery` → HTTP 400, `SessionLocked` → HTTP 423
- Phase 93 (UI): SearchQuery shape matches what frontend SearchContext will expand to

---

_Phase: 88-core-domain-and-port-contracts_
_Completed: 2026-04-10_
