# Phase 89: Use Cases and Delete Integration - Research

**Researched:** 2026-04-10
**Domain:** Rust use-case layer (uc-app), hexagonal architecture, async trait patterns
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** All four search use cases live in `uc-app/src/usecases/search/` as a dedicated subdirectory, parallel to `clipboard/`. Each use case gets its own file. A `mod.rs` re-exports the public API. Exports are added to `uc-app/src/usecases/mod.rs`.
- **D-02:** `IndexClipboardEntry::execute()` accepts pre-built `(SearchDocument, Vec<SearchPosting>)`. Thin orchestrator — delegates to `SearchIndexPort::index_entry()`. Caller constructs objects.
- **D-03:** No tokenizer port injected into IndexClipboardEntry.
- **D-04:** `RebuildSearchIndex::execute()` accepts caller-supplied `Vec<(SearchDocument, Vec<SearchPosting>)>` and a `tokio::sync::mpsc::Sender<RebuildProgress>`. Delegates to `SearchIndexPort::rebuild()`.
- **D-05:** Standalone `RemoveIndexedEntry` wraps `SearchIndexPort::remove_entry()`. Accepts `&EntryId`, returns `Result<(), SearchError>`.
- **D-06:** `SearchClipboardEntries::execute()` accepts `SearchQuery` and returns `Result<Vec<SearchResult>, SearchError>` via port. No extra filtering in use case.
- **D-07:** When `search_index.remove_entry()` fails in delete path, log `warn!` and continue. Delete always completes. Matches file cache pattern.
- **D-08:** `SearchIndexPort` injected via `.with_search_index(Arc<dyn SearchIndexPort>)` builder method. Field is `Option<Arc<dyn SearchIndexPort>>` — absence means no search cleanup (backwards-compatible).

### Claude's Discretion

- Exact tracing span naming within the new use cases
- Whether to add `#[tracing::instrument]` attribute to each execute() method (follow existing use case pattern)
- Whether RebuildSearchIndex also exposes a convenience method without the progress sender (can use a no-op channel internally)

### Deferred Ideas (OUT OF SCOPE)

- None — discussion stayed within phase scope.

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                      | Research Support                                                                                                     |
| ------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| SIDX-01 | User's clipboard entries are automatically indexed when captured in unlocked state | IndexClipboardEntry use case (Phase 89) is the indexing boundary; Phase 92 calls it from capture handler            |
| SIDX-02 | Deleting a clipboard entry synchronously removes its search_document and all search_posting rows | DeleteClipboardEntry extension with `.with_search_index()` builder and `remove_entry` call before returning         |

</phase_requirements>

## Summary

Phase 89 is a pure code addition phase in the `uc-app` crate. It creates five thin use case files in a new `usecases/search/` subdirectory, extends `delete_clipboard_entry.rs` with an optional search index port, and adds unit tests for all five success criteria. No new Cargo.toml dependencies are needed — uc-app already has uc-core (which exports `SearchIndexPort`), `async-trait`, `tokio`, `anyhow`, and `tracing`.

All four new search use cases are thin orchestrators: they receive pre-built domain objects, call one port method, and return. The interesting design work is in the delete integration, where the error policy (warn-and-continue) exactly mirrors the existing file cache deletion block.

**Primary recommendation:** Copy the file cache deletion block pattern verbatim for search cleanup. Use `Arc<AtomicBool>` + `Arc<Mutex<Option<EntryId>>>` in the spy mock to verify both that `remove_entry` was called and that it received the correct `EntryId`.

## Standard Stack

### Core

| Library     | Version | Purpose                             | Why Standard                                    |
| ----------- | ------- | ----------------------------------- | ----------------------------------------------- |
| uc-core     | ws      | SearchIndexPort, domain types       | Only source of port traits and search types     |
| async-trait | 0.1     | `#[async_trait]` on port impls      | Required for dyn-safe async traits in Rust      |
| tokio       | 1.x     | async runtime, `mpsc::Sender`       | Workspace-wide async executor                   |
| anyhow      | 1.0     | `Result<T>` for execute() returns   | Codebase standard for use case error handling   |
| tracing     | 0.1.44  | `#[tracing::instrument]`, `warn!`   | Codebase-wide observability                     |

### Supporting

| Library | Version | Purpose                          | When to Use                                   |
| ------- | ------- | -------------------------------- | --------------------------------------------- |
| std::sync::Arc | std | Port storage | All port fields stored as `Arc<dyn Port + Send + Sync>` |
| std::sync::atomic::AtomicBool | std | Test spy call tracking | Cheap "was this called?" check |
| std::sync::Mutex | std | Test spy argument capture | Capture `EntryId` argument in spy mock |

**No new dependencies needed.** All required crates are already in `uc-app/Cargo.toml`.

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-app/src/usecases/
├── search/                              # NEW directory (parallel to clipboard/)
│   ├── mod.rs                           # pub use re-exports for all 4 use cases
│   ├── index_clipboard_entry.rs         # IndexClipboardEntry use case
│   ├── remove_indexed_entry.rs          # RemoveIndexedEntry use case
│   ├── search_clipboard_entries.rs      # SearchClipboardEntries use case
│   └── rebuild_search_index.rs          # RebuildSearchIndex use case
├── delete_clipboard_entry.rs            # MODIFY: add field + builder + call + test
└── mod.rs                               # MODIFY: add `pub mod search;` + re-exports
```

### Files to Create vs. Modify (complete list)

**CREATE:**
- `usecases/search/mod.rs`
- `usecases/search/index_clipboard_entry.rs`
- `usecases/search/remove_indexed_entry.rs`
- `usecases/search/search_clipboard_entries.rs`
- `usecases/search/rebuild_search_index.rs`

**MODIFY:**
- `usecases/mod.rs` — add `pub mod search;` and `pub use search::{...}` re-exports
- `usecases/delete_clipboard_entry.rs` — add `search_index` field, `.with_search_index()` builder, search cleanup step, and new unit test

### Pattern 1: Thin Use Case with Single Port Call

**What:** Struct with `Arc<dyn Port>` field, one `execute()` method with `#[tracing::instrument]`, delegates entirely to the port.
**When to use:** All four new search use cases — IndexClipboardEntry, RemoveIndexedEntry, SearchClipboardEntries, RebuildSearchIndex.

```rust
// Source: src-tauri/crates/uc-app/src/usecases/list_clipboard_entries.rs (pattern)
use std::sync::Arc;
use uc_core::ids::EntryId;
use uc_core::ports::SearchIndexPort;
use uc_core::search::SearchError;

pub struct RemoveIndexedEntry {
    search_index: Arc<dyn SearchIndexPort>,
}

impl RemoveIndexedEntry {
    pub fn new(search_index: Arc<dyn SearchIndexPort>) -> Self {
        Self { search_index }
    }

    #[tracing::instrument(
        name = "usecase.remove_indexed_entry.execute",
        skip(self),
        fields(entry_id = %entry_id)
    )]
    pub async fn execute(&self, entry_id: &EntryId) -> Result<(), SearchError> {
        self.search_index.remove_entry(entry_id).await
    }
}
```

### Pattern 2: Optional Port Builder (Delete Integration)

**What:** `Option<Arc<dyn Port>>` field, separate builder method. If `None`, skip the optional step. On error in optional step, `warn!` and continue — do NOT propagate.
**When to use:** SearchIndexPort integration in DeleteClipboardEntry (D-08).

```rust
// Source: delete_clipboard_entry.rs (with_file_cache_dir pattern — replicate exactly)
pub struct DeleteClipboardEntry {
    // ... existing fields ...
    search_index: Option<Arc<dyn SearchIndexPort>>,
}

impl DeleteClipboardEntry {
    pub fn with_search_index(mut self, port: Arc<dyn SearchIndexPort>) -> Self {
        self.search_index = Some(port);
        self
    }
}
```

Search cleanup step (to be inserted before step 2 in execute(), since it's non-authoritative):

```rust
// Mirror the file cache cleanup block shape exactly
async {
    let Some(ref idx) = self.search_index else {
        return;
    };
    if let Err(e) = idx.remove_entry(entry_id).await {
        warn!(
            entry_id = %entry_id,
            error = %e,
            "Failed to remove search index entry during delete — orphan will self-heal via rebuild"
        );
    }
}
.instrument(info_span!("cleanup_search_index_entry", entry_id = %entry_id))
.await;
```

### Pattern 3: Spy Mock for Call Verification

**What:** Mock that tracks both whether a method was called and what argument it received. Uses `Arc<AtomicBool>` + `Arc<Mutex<Option<EntryId>>>`.
**When to use:** SC-5 unit test — verify `remove_entry` was called with the correct `EntryId`.

```rust
// Pattern: extend existing test mock approach in delete_clipboard_entry.rs
struct SpySearchIndex {
    remove_called: Arc<AtomicBool>,
    remove_entry_id: Arc<Mutex<Option<EntryId>>>,
}

#[async_trait]
impl SearchIndexPort for SpySearchIndex {
    async fn remove_entry(&self, entry_id: &EntryId) -> Result<(), SearchError> {
        self.remove_called.store(true, Ordering::SeqCst);
        *self.remove_entry_id.lock().unwrap() = Some(entry_id.clone());
        Ok(())
    }
    // ... stub other methods as Ok(()) / Ok(vec![]) / etc.
}
```

### Pattern 4: search/mod.rs Structure (parallel to clipboard/mod.rs)

```rust
// src-tauri/crates/uc-app/src/usecases/search/mod.rs
pub mod index_clipboard_entry;
pub mod rebuild_search_index;
pub mod remove_indexed_entry;
pub mod search_clipboard_entries;

pub use index_clipboard_entry::IndexClipboardEntry;
pub use rebuild_search_index::RebuildSearchIndex;
pub use remove_indexed_entry::RemoveIndexedEntry;
pub use search_clipboard_entries::SearchClipboardEntries;
```

### Pattern 5: SearchClipboardEntries typed error return

SearchClipboardEntries is the one use case that returns a typed error (not `anyhow::Result`) to preserve `SearchError` semantics through the port boundary for the daemon layer. This is the established pattern from the codebase (see error.rs doc comment on daemon HTTP status mapping).

```rust
pub async fn execute(&self, query: SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
    self.search_index.search(query).await
}
```

### Pattern 6: RebuildSearchIndex with progress sender

Per D-04, `execute()` receives the mpsc sender from the caller.

```rust
use tokio::sync::mpsc::Sender;
use uc_core::search::{RebuildProgress, SearchError};

pub async fn execute(
    &self,
    entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
    progress_tx: Sender<RebuildProgress>,
) -> Result<(), SearchError> {
    self.search_index.rebuild(entries, progress_tx).await
}
```

### Anti-Patterns to Avoid

- **Tokenizer injection into IndexClipboardEntry:** D-03 explicitly forbids this. HMAC computation is Phase 90 scope.
- **Propagating search_index errors from delete path:** D-07 requires warn-and-continue. Never `?` the `remove_entry` call in the delete path.
- **Returning `anyhow::Result` from SearchClipboardEntries:** The typed `SearchError` must be preserved for daemon HTTP status mapping. Use `Result<Vec<SearchResult>, SearchError>`.
- **Wiring real SearchIndexPort into CoreUseCases::delete_clipboard_entry():** D-08 says `Option` with `None` default. Phase 89 does NOT wire the real port into CoreUseCases — that is Phase 91/92 scope. The `with_search_index()` builder exists for future wiring and for unit tests only in this phase.
- **Adding new Cargo.toml dependencies:** All required crates already present. Adding unnecessary deps wastes review cycles.

## Don't Hand-Roll

| Problem                   | Don't Build           | Use Instead                            | Why                                                                  |
| ------------------------- | --------------------- | -------------------------------------- | -------------------------------------------------------------------- |
| Async trait object safety | Manual enum dispatch  | `async-trait` crate (already present)  | Rust async fn in trait not object-safe without it                   |
| Test mock structs          | Complex mock framework | Simple hand-rolled structs with `Arc<AtomicBool>` | Codebase pattern — mockall is available but not used here        |
| Error mapping              | Custom error wrapper  | `SearchError` variants from uc-core   | All error variants already defined and documented for daemon mapping |

**Key insight:** These use cases are intentionally thin. Any complexity added here is misplaced — business logic belongs in the port implementation (Phase 91) or in the Phase 90 tokenizer.

## Common Pitfalls

### Pitfall 1: `search_index` field not `Send + Sync`

**What goes wrong:** Compiler error — `Arc<dyn SearchIndexPort>` requires `+ Send + Sync` to be stored in structs used across `.await` boundaries.
**Why it happens:** Trait objects used in async contexts require `Send + Sync` bounds.
**How to avoid:** Declare as `Arc<dyn SearchIndexPort + Send + Sync>` (or verify `SearchIndexPort: Send + Sync` in the trait definition — it is, per the trait definition in search_index.rs line 20).
**Warning signs:** `dyn SearchIndexPort` without bounds compiles for non-async but fails in async context.

Note: `SearchIndexPort: Send + Sync` is already required by the trait definition (`pub trait SearchIndexPort: Send + Sync`), so `Arc<dyn SearchIndexPort>` is sufficient — the compiler infers the bounds from the supertrait constraints.

### Pitfall 2: search cleanup step ordering relative to delete steps

**What goes wrong:** Calling `remove_entry` after entry/event rows are deleted means the search record is orphaned for a window if the delete itself fails.
**Why it happens:** Wrong ordering in the delete chain.
**How to avoid:** Insert the search cleanup block before step 2 (delete_selection) — it's non-authoritative and should run first. If cleanup fails, delete proceeds. If cleanup succeeds but delete fails, a phantom search record exists until the next rebuild — acceptable per D-07.

### Pitfall 3: mod.rs re-export omissions

**What goes wrong:** Use case types not accessible from outside the crate, causing downstream compile errors in Phase 92.
**Why it happens:** Forgot to add `pub use` in `search/mod.rs` or `usecases/mod.rs`.
**How to avoid:** After creating files, verify: `pub mod search;` in usecases/mod.rs, and `pub use search::{...}` for all four types in usecases/mod.rs.

### Pitfall 4: Test spy does not capture EntryId argument

**What goes wrong:** Test asserts `remove_called == true` but SC-5 says "calls remove_entry" — implicitly the test should also verify the correct `EntryId` was passed, else a no-op mock passes the test incorrectly.
**Why it happens:** Using only `AtomicBool` without argument capture.
**How to avoid:** Include `Arc<Mutex<Option<EntryId>>>` in the spy struct and assert it equals the `entry_id` passed to `execute()`.

### Pitfall 5: RebuildSearchIndex channel lifetime

**What goes wrong:** Sender dropped before rebuild completes, causing rebuild to fail silently.
**Why it happens:** Caller creates channel but drops rx before awaiting execute().
**How to avoid:** This is the caller's responsibility — document in doc comment. The use case itself only forwards the sender to the port.

## Code Examples

Verified patterns from existing codebase files:

### Tracing instrument attribute (from delete_clipboard_entry.rs)

```rust
#[tracing::instrument(
    name = "usecase.delete_clipboard_entry.execute",
    skip(self),
    fields(entry_id = %entry_id)
)]
pub async fn execute(&self, entry_id: &EntryId) -> Result<()> {
```

Apply same pattern for all new use cases:
- `usecase.index_clipboard_entry.execute` with `fields(entry_id = %document.entry_id)`
- `usecase.remove_indexed_entry.execute` with `fields(entry_id = %entry_id)`
- `usecase.search_clipboard_entries.execute` with `fields(query = %query.query_string)`
- `usecase.rebuild_search_index.execute` with `fields(entry_count = entries.len())`

### Instrument span on async block (from delete_clipboard_entry.rs lines 82-157)

```rust
async {
    let Some(ref cache_dir) = self.file_cache_dir else {
        return;
    };
    // ... non-fatal cleanup ...
}
.instrument(info_span!("cleanup_cache_files", event_id = %event_id))
.await;
```

The search cleanup block follows this exact shape — note the `.instrument()` chained directly on the `async {}` block before `.await`.

### Import path for SearchIndexPort in uc-app

```rust
use uc_core::ports::SearchIndexPort;
// or
use uc_core::ports::search::SearchIndexPort;  // both work
```

All search domain types:
```rust
use uc_core::search::{
    RebuildProgress, SearchDocument, SearchError, SearchPosting, SearchQuery, SearchResult,
};
use uc_core::ids::EntryId;
```

### StubPort shape for tests (from search_index.rs cfg(test) block)

The existing StubPort in uc-core is the minimal valid implementation — copy its method stubs for test mocks in uc-app tests. All methods return `Ok(())` or `Ok(vec![])`.

## Validation Architecture

### Test Framework

| Property           | Value                                                     |
| ------------------ | --------------------------------------------------------- |
| Framework          | Rust built-in `#[tokio::test]` + `cargo test`            |
| Config file        | none — workspace Cargo.toml                               |
| Quick run command  | `cargo test -p uc-app`                                    |
| Full suite command | `cargo test --workspace`                                  |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                         | Test Type | Automated Command                                                         | File Exists? |
| ------- | ---------------------------------------------------------------- | --------- | ------------------------------------------------------------------------- | ------------ |
| SIDX-01 | IndexClipboardEntry delegates to mock SearchIndexPort            | unit      | `cargo test -p uc-app usecases::search::index_clipboard_entry`            | Wave 0       |
| SIDX-01 | RemoveIndexedEntry delegates to mock SearchIndexPort             | unit      | `cargo test -p uc-app usecases::search::remove_indexed_entry`             | Wave 0       |
| SIDX-01 | SearchClipboardEntries passes query and returns results          | unit      | `cargo test -p uc-app usecases::search::search_clipboard_entries`         | Wave 0       |
| SIDX-01 | RebuildSearchIndex delegates entries + progress_tx to port       | unit      | `cargo test -p uc-app usecases::search::rebuild_search_index`             | Wave 0       |
| SIDX-02 | DeleteClipboardEntry calls remove_entry before returning         | unit      | `cargo test -p uc-app usecases::delete_clipboard_entry::tests`            | Exists (modify) |

SC-5 test additionally verifies:
- `remove_entry` called with correct `EntryId`
- Delete still completes (returns `Ok(())`) when `remove_entry` fails (warn-and-continue)

### Sampling Rate

- **Per task commit:** `cargo test -p uc-app`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-app/src/usecases/search/mod.rs` — new module
- [ ] `src-tauri/crates/uc-app/src/usecases/search/index_clipboard_entry.rs` — covers SIDX-01 SC-1
- [ ] `src-tauri/crates/uc-app/src/usecases/search/remove_indexed_entry.rs` — covers SIDX-01 SC-2
- [ ] `src-tauri/crates/uc-app/src/usecases/search/search_clipboard_entries.rs` — covers SIDX-01 SC-3
- [ ] `src-tauri/crates/uc-app/src/usecases/search/rebuild_search_index.rs` — covers SIDX-01 SC-4

Tests for the delete integration (SC-5) go in the existing `#[cfg(test)]` block in `delete_clipboard_entry.rs` — no new file needed.

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs` — builder pattern, error policy, test mock structure, tracing instrument pattern
- `src-tauri/crates/uc-core/src/ports/search/search_index.rs` — SearchIndexPort trait, method signatures, StubPort shape
- `src-tauri/crates/uc-core/src/search/mod.rs` — all re-exported domain types
- `src-tauri/crates/uc-core/src/search/document.rs` — SearchDocument, SearchPosting, SearchIndexMeta
- `src-tauri/crates/uc-core/src/search/query.rs` — SearchQuery, QueryOperator, TimeRangeFilter
- `src-tauri/crates/uc-core/src/search/result.rs` — SearchResult, RebuildProgress, RebuildStage
- `src-tauri/crates/uc-core/src/search/error.rs` — SearchError variants
- `src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs` — clipboard subdirectory structure to replicate
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — existing mod layout, re-export conventions
- `src-tauri/crates/uc-app/Cargo.toml` — confirmed no new deps needed

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — confirmed from existing Cargo.toml and codebase
- Architecture: HIGH — all patterns verified from existing files, no inference
- Pitfalls: HIGH — derived from reading actual existing code, not speculation

**Research date:** 2026-04-10
**Valid until:** 2026-05-10 (stable Rust crate versions; patterns tied to this codebase)
