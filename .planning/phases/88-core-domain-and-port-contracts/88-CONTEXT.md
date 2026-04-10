# Phase 88: Core Domain and Port Contracts - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Define SearchIndexPort, SearchKeyDerivationPort, domain models (SearchQuery, SearchResult, SearchFilter, SearchDocument, SearchPosting), and SearchKey newtype in uc-core. Pure contracts — no implementations. All downstream crates (uc-app, uc-infra, uc-daemon) will reference these types and traits after this phase. No business logic, no database access, no HTTP routes.

</domain>

<decisions>
## Implementation Decisions

### SearchResult Shape

- **D-01:** SearchResult carries the full metadata set needed for ClipboardItemRow rendering without a second API call: `entry_id`, `file_type` (text/html/link/file/image/other enum), `active_time_ms`, `text_preview: Option<String>` (truncated to ~80 chars), `mime_type: String`, `file_extensions: Vec<String>`
- **D-02:** No match position information in V1 — HMAC architecture does not return token offsets, so no highlight fields on SearchResult

### Error Model

- **D-03:** Define `SearchError` as a typed domain enum in uc-core. Port traits return `Result<T, SearchError>`.
- **D-04:** SearchError variants must include at minimum: `InvalidQuery(String)` (mixed AND/OR — maps to HTTP 400 in daemon), `SessionLocked` (encryption session not ready — maps to HTTP 423), `IndexNotReady` (version mismatch during rebuild window). Additional variants at planner's discretion.
- **D-05:** Use `anyhow::Error` for internal infrastructure errors within implementations — SearchError is the port boundary type only, not used inside uc-infra adapters for internal failures.

### Rebuild Progress Model

- **D-06:** `RebuildProgress` is defined as a struct in uc-core (not an enum) with fields for stage (e.g., started/indexing/complete) and progress counters (indexed, total). Port trait's rebuild method accepts a `tokio::sync::mpsc::Sender<RebuildProgress>` parameter.
- **D-07:** This matches the existing file transfer progress pattern — daemon subscribes to the channel and forwards events via DaemonApiEventEmitter. uc-core does not know about WS serialization.

### Port Trait Structure

- **D-08:** Two separate port traits following the pattern in `uc-core/src/ports/`: `SearchIndexPort` (index_entry, remove_entry, search, rebuild) and `SearchKeyDerivationPort` (derive_key from master key + profile context). Separate traits allow independent mock implementations in uc-app unit tests.
- **D-09:** Both traits use `#[async_trait]` and are injected as `Arc<dyn Port + Send + Sync>` — consistent with existing `ClipboardEntryRepositoryPort`, `EncryptionPort` patterns.

### SearchQuery Structure

- **D-10:** SearchQuery encodes: `query_string: String`, `operator: QueryOperator` (And/Or enum), `time_range: Option<TimeRangeFilter>`, `file_types: Vec<FileType>`, `extensions: Vec<String>`, `limit: u32`, `offset: u32`. The AND/OR distinction is at the top level — no mixed operators (enforcement at parse time in use case, results in `SearchError::InvalidQuery`).
- **D-11:** TimeRangeFilter is either a preset variant (Today/Yesterday/Last7d/Last30d/ThisWeek/ThisMonth/Last24h) or an absolute range `{ from_ms: u64, to_ms: u64 }`. Both are variants of a `TimeRangeFilter` enum.

### Module Location

- **D-12:** All search domain types live in `uc-core/src/search/` with a `mod.rs` that re-exports the public API. Port traits live in `uc-core/src/ports/search/`. Consistent with existing `uc-core/src/clipboard/`, `uc-core/src/ports/clipboard/` structure.

### Claude's Discretion

- Exact field ordering and derive macros on structs (`Debug`, `Clone`, `PartialEq`, `serde::Serialize/Deserialize`)
- Whether SearchError implements `std::error::Error` directly or via `thiserror`
- Exact SearchDocument and SearchPosting field sets (follow arch spec in `docs/architecture/local-encrypted-search.md` exactly)
- Whether FileType enum is defined in the search module or promoted to a shared domain type

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Primary Architecture Spec

- `docs/architecture/local-encrypted-search.md` — Complete V1 design: data model (search_document, search_posting, search_index_meta), query semantics, key derivation, API shape, tokenizer strategy, all 10 resolved architectural decisions

### Requirements

- `.planning/REQUIREMENTS.md` — 22 requirements (SIDX-01–07, SQRY-01–06, REBLD-01–04, SUI-01–07); Phase 88 enables all of them

### Research Artifacts

- `.planning/research/ARCHITECTURE.md` — Hexagonal integration points: which crate owns each type, where SearchIndexPort hooks into CaptureClipboardUseCase, delete cascade approach, daemon route structure
- `.planning/research/STACK.md` — Only 2 new crates needed (unicode-normalization, unicode-segmentation); blake3 already in workspace; Diesel (not rusqlite) for persistence

### Existing Port Patterns (read before defining new ports)

- `src-tauri/crates/uc-core/src/ports/` — Existing port trait structure to replicate
- `src-tauri/crates/uc-core/src/ids/` — Typed ID newtype pattern (SearchKey should follow this)

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `uc-core/src/ids/` — Typed ID newtype pattern: `pub struct EntryId(Uuid)` with opaque wrapper. SearchKey newtype should follow the same shape but expose no raw bytes — only an interface for computing term tags.
- `uc-core/src/ports/security/` — EncryptionPort/EncryptionSessionPort pattern for session-gated operations — SearchKeyDerivationPort should follow this style for get_master_key() hook.
- Existing `FileType`-adjacent enums in clipboard domain — check if a content type enum already exists before creating a new FileType enum from scratch.

### Established Patterns

- Port traits: `#[async_trait] pub trait FooPort: Send + Sync { async fn method(&self, ...) -> Result<T, E>; }` — use this shape for both SearchIndexPort and SearchKeyDerivationPort.
- `AppDeps` sub-structs (ClipboardPorts, SecurityPorts, etc.) — new SearchPorts sub-struct will be added in Phase 92; the types defined here must be Arc-compatible.
- `anyhow::Result<T>` as default for infrastructure errors; typed enums at domain boundaries — apply this to SearchError at port boundary.

### Integration Points

- `uc-app/src/usecases/delete_clipboard_entry.rs` — will gain `.with_search_index(Arc<dyn SearchIndexPort>)` builder method in Phase 89. Domain types defined here must be compatible.
- `uc-daemon` DaemonApiState — will hold `Arc<dyn SearchIndexPort>` in Phase 92. Types must be `Send + Sync`.
- Frontend `SearchContext` — currently `string` query only. Phase 93 will expand to structured state matching `SearchQuery` shape. The field names here will influence TypeScript DTO naming.

</code_context>

<specifics>
## Specific Ideas

- No specific UI references for this phase — it's pure Rust domain contracts.
- The `RebuildProgress` channel approach mirrors the file transfer progress pattern already in the codebase. Planner should read how `FileTransferProgressPort` or similar is implemented before designing RebuildProgress.

</specifics>

<deferred>
## Deferred Ideas

- Highlight/match position fields on SearchResult — blocked by HMAC architecture (no token positions returned). V2 concern.
- Fuzzy match or partial token variants on SearchQuery — explicitly out of scope for V1.
- Multi-vault / multi-space key derivation beyond profile scoping — V2 concern.

</deferred>

---

_Phase: 88-core-domain-and-port-contracts_
_Context gathered: 2026-04-10_
