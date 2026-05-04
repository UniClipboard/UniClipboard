# Phase 88: Core Domain and Port Contracts - Research

**Researched:** 2026-04-10
**Domain:** Rust hexagonal architecture — uc-core domain models and port trait definitions
**Confidence:** HIGH (based on direct source code reading + pre-existing architecture research)

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** SearchResult carries: `entry_id`, `file_type` (text/html/link/file/image/other enum), `active_time_ms`, `text_preview: Option<String>` (truncated ~80 chars), `mime_type: String`, `file_extensions: Vec<String>`
- **D-02:** No match position information in V1
- **D-03:** `SearchError` is a typed domain enum in uc-core. Port traits return `Result<T, SearchError>`
- **D-04:** SearchError variants: `InvalidQuery(String)`, `SessionLocked`, `IndexNotReady`. Additional variants at planner's discretion
- **D-05:** `anyhow::Error` for internal infra failures; `SearchError` is port boundary type only
- **D-06:** `RebuildProgress` is a struct in uc-core with stage field and progress counters; port trait's rebuild method accepts `tokio::sync::mpsc::Sender<RebuildProgress>`
- **D-07:** Matches file transfer progress pattern; uc-core does not know about WS serialization
- **D-08:** Two separate port traits: `SearchIndexPort` (index_entry, remove_entry, search, rebuild) and `SearchKeyDerivationPort` (derive_key from master key + profile context)
- **D-09:** Both traits use `#[async_trait]` and are injected as `Arc<dyn Port + Send + Sync>`
- **D-10:** SearchQuery encodes: `query_string: String`, `operator: QueryOperator` (And/Or enum), `time_range: Option<TimeRangeFilter>`, `file_types: Vec<FileType>`, `extensions: Vec<String>`, `limit: u32`, `offset: u32`
- **D-11:** `TimeRangeFilter` is an enum with preset variants (Today/Yesterday/Last7d/Last30d/ThisWeek/ThisMonth/Last24h) or absolute range `{ from_ms: u64, to_ms: u64 }`
- **D-12:** All search domain types live in `uc-core/src/search/` with `mod.rs` re-exporting the public API. Port traits live in `uc-core/src/ports/search/`

### Claude's Discretion

- Exact field ordering and derive macros on structs (`Debug`, `Clone`, `PartialEq`, `serde::Serialize/Deserialize`)
- Whether SearchError implements `std::error::Error` directly or via `thiserror`
- Exact SearchDocument and SearchPosting field sets (follow arch spec in `docs/architecture/local-encrypted-search.md` exactly)
- Whether FileType enum is defined in the search module or promoted to a shared domain type

### Deferred Ideas (OUT OF SCOPE)

- Highlight/match position fields on SearchResult (blocked by HMAC architecture)
- Fuzzy match or partial token variants on SearchQuery
- Multi-vault / multi-space key derivation beyond profile scoping

</user_constraints>

---

## Summary

Phase 88 is a pure Rust contract-definition phase: no implementations, no databases, no HTTP routes. The deliverable is a set of types and trait definitions in `uc-core` that all downstream crates (uc-app, uc-infra, uc-daemon) will compile against. Success is measured by `cargo check --workspace` passing with no broken imports.

The existing codebase provides clear patterns to follow. The security port pattern (`EncryptionPort`, `EncryptionSessionPort`) is the correct template for the new search ports — not the clipboard port pattern. The most important design constraint is that `SearchKey` must be opaque: it follows the `MasterKey` model (`[u8; 32]` with no `pub` inner field and no `as_bytes()`), not the `EntryId` newtype model which exposes raw string access via `inner()`, `Deref`, and `into_inner()`.

The architecture research document (`.planning/research/ARCHITECTURE.md`) describes `search()` returning `Vec<EntryId>`, but decision D-01 locks `SearchResult` as carrying full render metadata. The port method should return `Vec<SearchResult>` directly. The `text_preview` truncation logic (~80 chars) is a Phase 89 concern — Phase 88 only defines the field.

**Primary recommendation:** Replicate the security port pattern (typed error enum, Arc-injected async traits) for all search ports. Define `SearchKey` as a zero-raw-access newtype analogous to `MasterKey`.

---

## Standard Stack

### Core (no new dependencies for this phase)

| Library | Version | Purpose | Why Used |
| --- | --- | --- | --- |
| async-trait | 0.1 | `#[async_trait]` macro for port traits | Required by every port in codebase |
| thiserror | 2.0.17 | Derive `std::error::Error` on `SearchError` | Used throughout security domain for typed errors |
| serde | 1 | Derive `Serialize`/`Deserialize` on domain models | Required for daemon DTO serialization |
| tokio | 1 (features: sync) | `mpsc::Sender<RebuildProgress>` in port signature | Already in uc-core Cargo.toml with `sync` feature |
| anyhow | 1.0 | Error handling inside implementations (not at port boundary) | Workspace standard |

**No Cargo.toml changes required for uc-core.** All required crates are already present in `src-tauri/crates/uc-core/Cargo.toml`. Verified: `tokio = { version = "1", features = ["sync"] }` is already declared.

---

## Architecture Patterns

### Recommended Module Structure

```
src-tauri/crates/uc-core/src/
├── search/
│   ├── mod.rs               # pub use of all public types
│   ├── query.rs             # SearchQuery, QueryOperator, TimeRangeFilter
│   ├── document.rs          # SearchDocument, SearchPosting, FileType, SearchIndexMeta
│   ├── result.rs            # SearchResult, RebuildProgress
│   └── error.rs             # SearchError
└── ports/
    ├── search/
    │   ├── mod.rs           # pub use SearchIndexPort, SearchKeyDerivationPort
    │   ├── search_index.rs  # SearchIndexPort trait
    │   └── search_key.rs    # SearchKeyDerivationPort trait
    └── mod.rs               # add: pub mod search; pub use search::*;
```

This follows the existing parallel structure of `uc-core/src/clipboard/` + `uc-core/src/ports/clipboard/`.

### Pattern 1: Typed Error Enum at Port Boundary

The security ports use typed error enums, not `anyhow::Result`. This is the correct template for SearchError.

```rust
// Source: src-tauri/crates/uc-core/src/ports/security/transfer_crypto.rs
#[derive(Debug, thiserror::Error)]
pub enum TransferCryptoError {
    #[error("transfer payload encryption failed: {0}")]
    EncryptionFailed(String),
    ...
}

pub trait TransferPayloadEncryptorPort: Send + Sync {
    fn encrypt(&self, ...) -> Result<Vec<u8>, TransferCryptoError>;
}
```

Apply the same shape for SearchError and both search ports:

```rust
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("encryption session is locked")]
    SessionLocked,
    #[error("search index not ready")]
    IndexNotReady,
    // Additional variants at planner's discretion
}

#[async_trait]
pub trait SearchIndexPort: Send + Sync {
    async fn index_entry(&self, doc: SearchDocument, postings: Vec<SearchPosting>) -> Result<(), SearchError>;
    async fn remove_entry(&self, entry_id: &EntryId) -> Result<(), SearchError>;
    async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>, SearchError>;
    async fn rebuild(
        &self,
        entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
        progress_tx: tokio::sync::mpsc::Sender<RebuildProgress>,
    ) -> Result<(), SearchError>;
    async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError>;
}

#[async_trait]
pub trait SearchKeyDerivationPort: Send + Sync {
    async fn derive_search_key(&self) -> Result<SearchKey, SearchError>;
}
```

### Pattern 2: Opaque Newtype for SearchKey

**Critical:** SearchKey MUST NOT follow the `impl_id!` macro pattern. That macro exposes `inner()`, `into_inner()`, `Deref<Target=String>`, `AsRef<str>` — all leak raw bytes. The `MasterKey` model is the correct template.

```rust
// Source: src-tauri/crates/uc-core/src/security/model.rs
// MasterKey is a correct model — pub inner field but no Serialize/Deserialize,
// and importantly: no impl_id! macro applied

// For SearchKey, use a stricter pattern — no pub field, no as_bytes():
pub struct SearchKey([u8; 32]);

impl SearchKey {
    /// Only infra layer calls this — through the port, not directly
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for SearchKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SearchKey([REDACTED])")
    }
}
```

The `SearchKeyDerivationPort` returns `SearchKey`. The infra-layer HMAC computation in `uc-infra` uses `as_bytes()` via `pub(crate)` or a targeted `pub` depending on crate visibility. No external consumer ever sees the raw bytes — the only way to produce a term tag is through the port interface.

### Pattern 3: RebuildProgress Struct (not enum)

Decision D-06 specifies a struct (not enum) with a stage field. Observe `TransferProgress` in `uc-core/src/ports/transfer_progress.rs` as the structural model:

```rust
// Source: src-tauri/crates/uc-core/src/ports/transfer_progress.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub direction: TransferDirection,   // an enum used as stage indicator
    pub chunks_completed: u32,
    pub total_chunks: u32,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
}
```

SearchRebuildProgress should follow this shape:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RebuildStage {
    Started,
    Indexing,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildProgress {
    pub stage: RebuildStage,
    pub indexed: u32,
    pub total: u32,
}
```

### Pattern 4: SearchDocument and SearchPosting Field Sets

Follow `docs/architecture/local-encrypted-search.md` exactly. Hard-delete decision (D-04 equivalent in arch spec) means **no `deleted_at_ms`** on SearchDocument.

```rust
// SearchDocument — one row per indexable clipboard entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDocument {
    pub entry_id: EntryId,
    pub event_id: EventId,
    pub active_time_ms: i64,
    pub captured_at_ms: i64,
    pub file_type: FileType,
    pub file_extensions: Vec<String>,
    pub mime_type: String,
    pub indexed_at_ms: i64,
    pub index_version: String,
    // NO deleted_at_ms — hard-delete semantic
}

// SearchPosting — one row per (term_tag, entry_id) pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPosting {
    pub term_tag: Vec<u8>,   // HMAC output (32 bytes)
    pub entry_id: EntryId,
    pub field_mask: u8,      // bitmask: body=1, html=2, url=4, file_path=8, file_name=16
    pub term_freq: u32,
}
```

### Pattern 5: FileType Enum in Search Domain

No existing FileType enum exists anywhere in the clipboard domain (confirmed by grep). The search module is the correct owner per ARCHITECTURE.md. Since D-12 states all search domain types live in `uc-core/src/search/`, define it in `document.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    Text,
    Html,
    Link,
    File,
    Image,
    Other,
}
```

### Pattern 6: SearchResult Correction

The pre-existing ARCHITECTURE.md research shows `search() -> Result<Vec<EntryId>>`, but Decision D-01 locks `SearchResult` as the return type carrying full render metadata. The port method must return `Vec<SearchResult>`, not `Vec<EntryId>`. The `search_document` table in infra will join to populate this in later phases.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub entry_id: EntryId,
    pub file_type: FileType,
    pub active_time_ms: i64,
    pub text_preview: Option<String>,  // truncation (~80 chars) is Phase 89 work, not here
    pub mime_type: String,
    pub file_extensions: Vec<String>,
}
```

### Anti-Patterns to Avoid

- **Using `impl_id!` macro for SearchKey:** The macro exposes raw string access through multiple trait implementations. SearchKey must be more opaque than clipboard IDs.
- **Using `anyhow::Result` for port return types:** Clipboard ports use `anyhow::Result` — search ports must use `Result<T, SearchError>`. Follow the security port pattern.
- **Returning `Vec<EntryId>` from search():** The ARCHITECTURE.md research pre-dates the CONTEXT.md decisions. Return `Vec<SearchResult>` per D-01.
- **Defining `deleted_at_ms` on SearchDocument:** The arch spec data model section mentions it, but the decisions section resolves hard-delete. Do not include it.
- **Putting truncation logic in `text_preview` on SearchResult in uc-core:** The field is a plain `Option<String>` here; truncation is infra/use-case concern (Phase 89+).
- **Defining a `KeyNotFound` variant in SearchError:** Key errors at session layer are `EncryptionError`. SearchError maps session-not-ready to `SessionLocked` at the port boundary per D-04.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
| --- | --- | --- | --- |
| Async trait support | Manual associated type futures | `async-trait = "0.1"` | Already in uc-core Cargo.toml; every port uses it |
| Typed error boilerplate | Manual `Display` + `Error` impls | `thiserror = "2.0.17"` | Already in uc-core Cargo.toml; used by EncryptionError |
| Key-safe memory clearing | Manual `Drop` with zeroing | Consider `zeroize` on SearchKey | Already in uc-core Cargo.toml (`zeroize = "1.8.2"`) |

**Key insight:** This phase adds zero new crate dependencies. Everything needed for the domain types and port traits is already declared in `uc-core/Cargo.toml`.

---

## Common Pitfalls

### Pitfall 1: Copying impl_id! for SearchKey

**What goes wrong:** Planner sees EntryId and assumes SearchKey should use the same `impl_id!` macro. The macro adds `inner()`, `Deref<Target=String>`, `into_inner()`, `AsRef<str>` — all expose raw bytes.
**Why it happens:** The macro is the obvious reuse pattern for newtypes in this codebase.
**How to avoid:** SearchKey is `[u8; 32]`, not `String`. Model it after `MasterKey` in `security/model.rs`. `pub(crate)` gate the byte accessor. Do not use `impl_id!`.
**Warning signs:** Any method on SearchKey returning `&str`, `String`, or `&[u8]` through a public interface.

### Pitfall 2: Wrong return type on search()

**What goes wrong:** Using `Vec<EntryId>` as the return of `SearchIndexPort::search()` following the ARCHITECTURE.md research document. The infra adapter would then require a separate `get_document()` call in the use case to build SearchResult — adding an unnecessary round-trip.
**Why it happens:** ARCHITECTURE.md was written before CONTEXT.md decisions locked D-01.
**How to avoid:** Return `Vec<SearchResult>` from `search()`. The SQLite infra adapter fetches from `search_document` directly in the same query.

### Pitfall 3: anyhow::Result on search port methods

**What goes wrong:** Planner copies the clipboard port pattern (`anyhow::Result`) for search ports. This loses the typed error information that Phase 92 needs to map `SearchError::SessionLocked` → HTTP 423 and `SearchError::InvalidQuery` → HTTP 400.
**Why it happens:** Clipboard ports (`ClipboardEntryRepositoryPort`) use `anyhow::Result` and are more prominent examples.
**How to avoid:** Use the security port pattern. `EncryptionPort` and `EncryptionSessionPort` both return `Result<T, EncryptionError>`. Search ports return `Result<T, SearchError>`.

### Pitfall 4: Including deleted_at_ms in SearchDocument

**What goes wrong:** The data model section of `docs/architecture/local-encrypted-search.md` lists `deleted_at_ms nullable`. A planner reading only that section would include it.
**Why it happens:** The field appears in the architecture spec before the decisions section resolves it.
**How to avoid:** Architecture spec question 4 ("删除语义是硬删还是软删") resolves hard-delete: rows are physically removed, no soft-delete column needed. SearchDocument has no `deleted_at_ms`.

### Pitfall 5: Defining ports/mod.rs without exporting new search module

**What goes wrong:** New `search/` module is added to `uc-core/src/ports/` but not added to `ports/mod.rs`. The downstream crates can't import the traits.
**Why it happens:** Easy to miss the `mod.rs` update step.
**How to avoid:** The plan must explicitly add `pub mod search;` and the corresponding `pub use` lines to `ports/mod.rs`.

---

## Code Examples

### Existing Port Pattern (template to follow)

```rust
// Source: src-tauri/crates/uc-core/src/ports/security/encryption_session.rs
use crate::security::model::{EncryptionError, MasterKey};
use async_trait::async_trait;

#[async_trait]
pub trait EncryptionSessionPort: Send + Sync {
    async fn is_ready(&self) -> bool;
    async fn get_master_key(&self) -> Result<MasterKey, EncryptionError>;
    async fn set_master_key(&self, master_key: MasterKey) -> Result<(), EncryptionError>;
    async fn clear(&self) -> Result<(), EncryptionError>;
}
```

### Existing Typed Error Pattern (template to follow)

```rust
// Source: src-tauri/crates/uc-core/src/security/model.rs
#[derive(Debug, thiserror::Error)]
pub enum EncryptionError {
    #[error("encryption is locked")]
    Locked,
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
    // ... more variants
}
```

### Existing Opaque Newtype Pattern (template for SearchKey)

```rust
// Source: src-tauri/crates/uc-core/src/security/model.rs
#[derive(Clone, PartialEq, Eq)]
pub struct MasterKey(pub [u8; 32]);

impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MasterKey([REDACTED])")
    }
}

impl MasterKey {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}
```

For SearchKey, make `as_bytes()` more restrictive — `pub(crate)` or expose only through the port interface.

### Existing Builder/Optional Port Pattern (reference for Phase 89)

```rust
// Source: src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs
pub struct DeleteClipboardEntry {
    // ... existing fields ...
    file_cache_dir: Option<PathBuf>,  // optional cross-cutting dep
}

impl DeleteClipboardEntry {
    pub fn with_file_cache_dir(mut self, dir: PathBuf) -> Self {
        self.file_cache_dir = Some(dir);
        self
    }
}
```

Phase 89 adds `search_index: Option<Arc<dyn SearchIndexPort>>` to `DeleteClipboardEntry` using this identical pattern.

### RebuildProgress Channel Pattern (existing analog)

```rust
// Source: src-tauri/crates/uc-core/src/ports/transfer_progress.rs
// TransferProgressPort uses a report_progress(&self, progress: TransferProgress) method.
// Phase 88 opts for a channel sender instead (D-06), which is compatible with
// tokio::sync::mpsc already declared in uc-core's Cargo.toml.
```

---

## Validation Architecture

### Test Framework

| Property | Value |
| --- | --- |
| Framework | cargo test (built-in, no config file) |
| Config file | none (workspace-level Cargo.toml) |
| Quick run command | `cargo check --workspace` |
| Full suite command | `cargo test -p uc-core` |

### Phase Requirements → Test Map

Phase 88 has no individual requirement IDs — it is foundation for all v0.5.0 requirements. The success criteria map to compilation checks:

| Success Criterion | Test Type | Automated Command |
| --- | --- | --- |
| SearchIndexPort + SearchKeyDerivationPort traits compile | compilation | `cargo check -p uc-core` |
| SearchKey exposes no raw bytes (public API audit) | code review | manual — verify no `pub` byte accessor |
| SearchQuery encodes all filter fields | compilation | `cargo check -p uc-core` |
| SearchResult carries full metadata | compilation | `cargo check -p uc-core` |
| uc-app, uc-infra, uc-daemon compile after additions | compilation | `cargo check --workspace` |

### Wave 0 Gaps

No new test files required. Success criteria are satisfied by `cargo check --workspace` passing. Inline unit tests (serde round-trips, Debug redaction verification) may be added to `uc-core/src/search/` module files following the pattern in `transfer_progress.rs`.

---

## Environment Availability

Step 2.6: SKIPPED — this phase is purely Rust source code additions with no external service dependencies. All required crates (`async-trait`, `thiserror`, `serde`, `tokio`) are already in `uc-core/Cargo.toml`.

---

## State of the Art

| Old Approach | Current Approach | Impact |
| --- | --- | --- |
| ARCHITECTURE.md: `search() -> Vec<EntryId>` | CONTEXT.md D-01 locks: `search() -> Vec<SearchResult>` | Port returns full render metadata, avoids second query in use case |
| Arch spec data model: `deleted_at_ms nullable` on search_document | Decisions section: hard-delete, no soft-delete column | Simpler schema, cleaner deletion flow |

**Deprecated/outdated in pre-existing research:**

- `ARCHITECTURE.md` SearchIndexPort signature returning `Vec<EntryId>` — superseded by D-01
- `ARCHITECTURE.md` AppDeps adding `search: Option<SearchPorts>` — Phase 88 only defines the port traits; the `SearchPorts` struct in AppDeps is Phase 92 work

---

## Open Questions

1. **SearchKey byte visibility across crate boundary**
   - What we know: `SearchKey` must not expose raw bytes externally. The infra adapter (`uc-infra`) needs bytes for HMAC computation.
   - What's unclear: `pub(crate)` scopes to `uc-core` only. Since `uc-infra` is a separate crate, it cannot use `pub(crate)`. Options: (a) expose `as_bytes()` as `pub` but with a doc comment warning, (b) add a `fn compute_term_tag(&self, token: &str) -> [u8; 32]` method directly on `SearchKey` that wraps the HMAC call, keeping bytes opaque to all callers.
   - Recommendation: Option (b) — add `compute_term_tag` as the only public method on `SearchKey`. This is more opaque but requires `blake3` in `uc-core` (already present at 1.8.2). The planner should decide and document the choice.

2. **SearchIndexMeta in uc-core vs uc-infra**
   - What we know: ARCHITECTURE.md places `SearchIndexMeta` in `uc-core/src/search/document.rs`. It contains runtime state fields (`last_rebuild_started_at_ms`, `rebuild_state`).
   - What's unclear: Whether a pure domain model crate should hold infra-runtime state.
   - Recommendation: Include it in `uc-core/src/search/` as a read-only projection struct. The port `get_index_meta()` returns it; infra populates it from `search_index_meta` table. Consistent with how `EncryptionState` is a domain projection.

---

## Sources

### Primary (HIGH confidence)

- Direct source reading: `src-tauri/crates/uc-core/src/ports/security/` (encryption.rs, encryption_session.rs, transfer_crypto.rs, key_scope.rs)
- Direct source reading: `src-tauri/crates/uc-core/src/security/model.rs` — MasterKey opaque newtype pattern
- Direct source reading: `src-tauri/crates/uc-core/src/ids/` — impl_id! macro (confirms this must NOT be used for SearchKey)
- Direct source reading: `src-tauri/crates/uc-core/src/ports/transfer_progress.rs` — RebuildProgress structural template
- Direct source reading: `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs` — optional builder port pattern
- Direct source reading: `src-tauri/crates/uc-app/src/deps.rs` — AppDeps structure (5 existing groups)
- Direct source reading: `src-tauri/crates/uc-core/Cargo.toml` — verified no new dependencies needed

### Secondary (MEDIUM confidence)

- `.planning/research/ARCHITECTURE.md` — integration point research (note: search() return type superseded by D-01)
- `.planning/research/STACK.md` — dependency research confirming no new crates for uc-core
- `docs/architecture/local-encrypted-search.md` — primary architecture spec with all 10 resolved decisions

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — verified directly from Cargo.toml lockfile
- Architecture patterns: HIGH — verified directly from source code
- Port signatures: HIGH — locked by CONTEXT.md decisions
- Pitfalls: HIGH — derived from direct source reading of existing patterns

**Research date:** 2026-04-10
**Valid until:** 2026-05-10 (stable crate versions; architecture decisions locked)
