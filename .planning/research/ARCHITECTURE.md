# Architecture Research: Local Encrypted Search Integration

**Domain:** Hexagonal architecture integration — local encrypted inverted index into existing Rust codebase
**Researched:** 2026-04-10
**Confidence:** HIGH (based on primary source code reading, confirmed against architecture spec)

---

## System Overview

The existing hexagonal stack (compiler-enforced private deps):

```
uc-core          (domain models + port traits — zero deps on other crates)
    ↓
uc-app           (use cases — depends only on uc-core)
    ↓
uc-infra         (SQLite adapters, crypto — depends on uc-core + uc-app)
    ↓
uc-bootstrap     (wiring: constructs AppDeps, passes Arc<dyn Port> into use cases)
    ↓
uc-daemon        (Axum HTTP routes + background workers — depends on uc-app + uc-core)
    ↓
uc-tauri         (Tauri shell — thin proxy to daemon)
    ↑
frontend React   (TypeScript — calls daemon HTTP/WS)
```

The search subsystem touches all five Rust layers in a specific, constrained way. Each layer integration is described below.

---

## Integration Point 1: uc-core — SearchIndexPort

**Status: NEW**

All port traits live in `uc-core/src/ports/`. A new `search/` submodule must be added there, following the existing pattern (`ports/security/`, `ports/clipboard/`).

```
uc-core/src/ports/
├── search/
│   ├── mod.rs            (pub use)
│   ├── search_index.rs   (SearchIndexPort trait)
│   └── search_key.rs     (SearchKeyDerivationPort trait)
└── mod.rs                (pub use search::*)
```

**SearchIndexPort** owns all index read/write operations:

```rust
#[async_trait]
pub trait SearchIndexPort: Send + Sync {
    async fn index_entry(&self, doc: SearchDocument, postings: Vec<SearchPosting>) -> Result<()>;
    async fn remove_entry(&self, entry_id: &EntryId) -> Result<()>;
    async fn search(&self, query: SearchQuery) -> Result<Vec<EntryId>>;
    async fn rebuild(&self, entries: Vec<(SearchDocument, Vec<SearchPosting>)>) -> Result<()>;
    async fn get_index_meta(&self) -> Result<SearchIndexMeta>;
}
```

**SearchKeyDerivationPort** isolates the HKDF derivation behind a port boundary so `uc-app` use cases can request a search key without knowing about HMAC internals:

```rust
#[async_trait]
pub trait SearchKeyDerivationPort: Send + Sync {
    async fn derive_search_key(&self) -> Result<SearchKey, EncryptionError>;
}
```

**Domain models** also belong in `uc-core/src/`:

```
uc-core/src/
└── search/
    ├── mod.rs
    ├── query.rs        (SearchQuery, BoolOp, TimeRange, FileTypeFilter)
    ├── document.rs     (SearchDocument, SearchPosting, SearchIndexMeta)
    └── projection.rs   (SearchResult — entry_id + matched fields)
```

The file type enum (`text | html | link | file | image | other`) is defined in `uc-core::search::document` as a search-layer classification derived from clipboard domain projections. It does NOT belong in the clipboard domain — the clipboard domain produces a `SearchableProjection` struct; the search subsystem classifies it.

---

## Integration Point 2: uc-app — Use Cases

**Status: FOUR NEW use cases + ONE modified**

New use cases in `uc-app/src/usecases/`:

```
uc-app/src/usecases/
├── search_clipboard_entries.rs   (NEW — query execution)
├── index_clipboard_entry.rs      (NEW — incremental index add)
├── remove_indexed_entry.rs       (NEW — incremental delete from index)
└── rebuild_search_index.rs       (NEW — full rebuild with atomic swap)
```

**Modified: `DeleteClipboardEntry`** (in `uc-app/src/usecases/delete_clipboard_entry.rs`)

The existing use case uses an optional `.with_file_cache_dir()` builder already. Follow the identical pattern:

```rust
pub struct DeleteClipboardEntry {
    // ... existing fields ...
    search_index: Option<Arc<dyn SearchIndexPort>>,  // ADD
}

impl DeleteClipboardEntry {
    pub fn with_search_index(mut self, port: Arc<dyn SearchIndexPort>) -> Self {
        self.search_index = Some(port);
        self
    }
}
```

In `execute()`, after step 3 (entry deleted), call `self.search_index.remove_entry(entry_id)` if present. This is synchronous within the deletion workflow — the design doc is explicit that search index removal must be part of the normal deletion flow, not a best-effort async side-effect.

**Indexing hook location:** The `IndexClipboardEntry` use case is NOT called inside `CaptureClipboardUseCase`. The capture use case returns `EntryId` and stops. The indexing hook belongs in `uc-daemon`'s `DaemonClipboardChangeHandler` — which already orchestrates post-capture actions (outbound sync trigger per Phase 61). This is the correct decoupling boundary: `uc-app` stays free of search concerns at the capture layer.

**AppDeps (`uc-app/src/deps.rs`):** Add a new `SearchPorts` group, following the five existing groups (`ClipboardPorts`, `SecurityPorts`, `DevicePorts`, `StoragePorts`, `SystemPorts`):

```rust
pub struct SearchPorts {
    pub search_index: Arc<dyn SearchIndexPort>,
    pub search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
}

pub struct AppDeps {
    // ... existing fields ...
    pub search: Option<SearchPorts>,  // Option so existing boot paths don't break
}
```

---

## Integration Point 3: uc-infra — Adapters

**Status: TWO NEW adapter files + ONE NEW migration**

```
uc-infra/src/
├── search/
│   ├── mod.rs
│   ├── sqlite_search_index.rs     (NEW — SearchIndexPort impl)
│   ├── text_extractor.rs          (NEW — SearchableProjection → tokenized fields)
│   ├── tokenizer.rs               (NEW — NFKC + lowercase + word split + CJK bigram)
│   └── search_key_derivation.rs   (NEW — SearchKeyDerivationPort impl using HKDF)
```

**SQLite search key derivation** (`SearchKeyDerivationPort` impl):

Calls `EncryptionSessionPort::get_master_key()` then runs `HKDF-SHA256(master_key, info="uc-search-index-v1", salt=profile_id_bytes)`. The `profile_id` comes from `KeyScopePort::current_scope()` which already exists. This keeps the search key scoped to the profile (the isolation dimension defined in the architecture spec, question 5).

```rust
pub struct HkdfSearchKeyDerivation {
    session: Arc<dyn EncryptionSessionPort>,
    key_scope: Arc<dyn KeyScopePort>,
}
```

**SQLite search index adapter:**

The existing infra layer uses Diesel ORM via `DbPool`. The search index DEVIATES from this pattern intentionally: posting-list operations (AND = intersect posting lists, OR = union) do not map well to Diesel's query builder. Use `diesel::sql_query()` through the same `DbPool` and `DbConn` (`SqliteConnection`). This is a documented deviation, not an accident.

The `SqliteSearchIndex` adapter holds `Arc<DbPool>` — same as every other infra adapter.

**HMAC term tag generation** is done synchronously in the adapter:

```rust
fn term_tag(search_key: &SearchKey, token: &str) -> String {
    // HMAC-SHA256(search_key, token) → hex string
}
```

**SQLite migration** (`uc-infra/migrations/`):

Add one new migration directory (next after `2026-03-15-000002_upgrade_file_transfer_tracking`):

```
migrations/2026-04-10-000001_create_search_index/
├── up.sql
└── down.sql
```

The three new tables (`search_document`, `search_posting`, `search_index_meta`) have **no foreign keys** to existing clipboard tables. This is deliberate: the index is independently rebuildable and must not be constrained by clipboard table state. The hard-delete semantic (architecture spec question 4) means no `deleted_at_ms` column in `search_document`.

Diesel `schema.rs` must be updated to declare the three new tables. Since they have no joins to existing tables, they do not appear in `diesel::allow_tables_to_appear_in_same_query!()` unless cross-table queries are needed.

The migration runs automatically on startup via the existing `run_migrations()` / `embed_migrations!()` pipeline in `uc-infra/src/db/pool.rs` — no code changes needed to trigger it.

---

## Integration Point 4: uc-daemon — HTTP Routes + Worker Hook

**Status: ONE NEW route file + ONE modified worker**

**New routes file: `uc-daemon/src/api/search.rs`**

Follows the identical pattern as `uc-daemon/src/api/clipboard.rs`:

```rust
pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/search/query",   post(search_entries))
        .route("/search/rebuild", post(rebuild_index))
        .route("/search/status",  get(get_index_status))
}
```

Merged into `router_l2_plus()` in `routes.rs` alongside the existing clipboard, settings, and pairing routers. All three routes require the session to be unlocked — handlers check `EncryptionSessionPort::is_ready()` and return `ApiError::Unauthorized` if not ready.

Route string constants follow the existing `daemon_api_strings` centralization pattern in `uc-core::network::daemon_api_strings`.

**Modified worker: `DaemonClipboardChangeHandler`**

This handler (Phase 61, `uc-daemon/src/workers/`) already calls post-capture orchestration (outbound sync). Add the search indexing call there:

```
capture completes → EntryId returned
    → DaemonClipboardChangeHandler::handle()
        → trigger outbound sync (existing)
        → if encryption_session.is_ready():
            IndexClipboardEntry use case (NEW)
```

The indexing call is gated on `encryption_session.is_ready()`. If the session is not unlocked (edge case: race at startup), skip silently — the full rebuild route recovers from this state.

---

## Integration Point 5: SQLite Migration Strategy

**Approach:** Standard Diesel embedded migration — additive, no foreign keys to existing tables.

The migration pipeline is already in place (`embed_migrations!("migrations")` in `pool.rs`, `run_pending_migrations()` at startup). Adding new migration directories is all that is needed.

Key constraints for the three new tables:

1. `search_document.entry_id` is TEXT PRIMARY KEY — no FK constraint to `clipboard_entry.entry_id`. The index is independently rebuildable.
2. `search_posting` uses a composite PRIMARY KEY `(term_tag, entry_id)`.
3. `search_index_meta` is a single-row config table (use `UPSERT` on a fixed `id = 1`).
4. `index_version` TEXT column in `search_document` enables safe rebuild triggers when the tokenizer normalization version changes.
5. No `deleted_at_ms` column — hard-delete semantic. Rows are physically removed on entry deletion.

For the full rebuild flow (atomic swap), use SQLite's `ALTER TABLE ... RENAME` pattern:

```sql
-- 1. Write into search_document_tmp + search_posting_tmp
-- 2. BEGIN EXCLUSIVE TRANSACTION
-- 3. DROP TABLE search_document; DROP TABLE search_posting;
-- 4. ALTER TABLE search_document_tmp RENAME TO search_document;
-- 5. ALTER TABLE search_posting_tmp RENAME TO search_posting;
-- 6. UPDATE search_index_meta SET rebuild_state='complete';
-- 7. COMMIT
```

The double-write strategy during rebuild (architecture spec question 10) means new captures write to both active and temp tables while rebuild is in progress. The `search_index_meta.rebuild_state` column tracks this.

---

## Integration Point 6: Search Key Derivation — XChaCha20 Session Unlock Hook

**The derivation chain:**

```
Passphrase / Keyring
    → Argon2id KDF → KEK
        → XChaCha20-Poly1305 unwrap → MasterKey (32 bytes)
            → stored in EncryptionSessionPort (in-memory)
                → HKDF-SHA256(MasterKey, info="uc-search-index-v1", salt=profile_id_bytes)
                    → SearchKey (32 bytes, derived on demand per request)
```

**Where the derivation is NOT triggered:**

- Not at unlock time (no eager derivation, no new session port)
- Not stored anywhere (derived on demand per request)

**Where the derivation IS triggered:**

- Inside `HkdfSearchKeyDerivation::derive_search_key()` — called by `IndexClipboardEntry` and `SearchClipboardEntries` use cases when they need to compute HMAC term tags

**Why on-demand derivation:** The `SearchKeyDerivationPort` wraps `EncryptionSessionPort` and `KeyScopePort`. If the session is not ready, `get_master_key()` returns `EncryptionError::KeyNotFound` and the use case returns an appropriate error. No separate unlock flow is needed — search is gated on the existing session state.

**Profile scoping:** `KeyScopePort::current_scope()` returns the active `KeyScope { profile_id }`. The HKDF salt incorporates the `profile_id` bytes. This satisfies the isolation requirement (architecture spec question 5) without requiring per-profile tables.

---

## Component Responsibilities

| Component | Layer | Status | Responsibility |
|-----------|-------|--------|----------------|
| `SearchIndexPort` | uc-core | NEW | Port trait: index/remove/search/rebuild |
| `SearchKeyDerivationPort` | uc-core | NEW | Port trait: derive_search_key() |
| `SearchDocument`, `SearchPosting`, `SearchQuery` | uc-core | NEW | Domain models |
| `SearchClipboardEntries` | uc-app | NEW | Query execution use case |
| `IndexClipboardEntry` | uc-app | NEW | Incremental index add use case |
| `RemoveIndexedEntry` | uc-app | NEW | Incremental delete use case |
| `RebuildSearchIndex` | uc-app | NEW | Full rebuild use case |
| `DeleteClipboardEntry` | uc-app | MODIFIED | Add `.with_search_index()` optional port |
| `AppDeps` | uc-app | MODIFIED | Add `search: Option<SearchPorts>` group |
| `SqliteSearchIndex` | uc-infra | NEW | `SearchIndexPort` impl — raw SQL via DbPool |
| `HkdfSearchKeyDerivation` | uc-infra | NEW | `SearchKeyDerivationPort` impl |
| `text_extractor` | uc-infra | NEW | Clipboard repr → searchable field extraction |
| `tokenizer` | uc-infra | NEW | NFKC + lowercase + word split + CJK bigram |
| Migration `2026-04-10-000001` | uc-infra | NEW | 3 new tables, no FK to existing tables |
| `search.rs` router | uc-daemon | NEW | 3 HTTP routes: query/rebuild/status |
| `DaemonClipboardChangeHandler` | uc-daemon | MODIFIED | Add IndexClipboardEntry call after capture |

---

## Data Flow

### Capture → Index Flow

```
Platform clipboard change
    → ClipboardWatcherWorker (uc-daemon)
        → CaptureClipboardUseCase::execute(snapshot) → EntryId  [uc-app, UNCHANGED]
            → DaemonClipboardChangeHandler::handle(entry_id)     [uc-daemon, MODIFIED]
                → trigger outbound sync (existing)
                → if encryption_session.is_ready():
                    IndexClipboardEntry::execute(entry_id)        [uc-app, NEW]
                        → load entry repr from repo
                        → text_extractor → tokenizer → HMAC tags
                        → SearchIndexPort::index_entry()          [uc-infra, NEW]
```

### Search Query Flow

```
Frontend POST /search/query
    → DaemonApiState::runtime_or_error()
        → SearchClipboardEntries::execute(SearchQuery)           [uc-app, NEW]
            → validate session ready
            → parse query, derive term tags via SearchKeyDerivationPort
            → SearchIndexPort::search(query)                     [uc-infra, NEW]
                → diesel::sql_query() posting-list intersection/union
            → filter by time range + file type on search_document
            → load entry projections via existing ClipboardEntryRepositoryPort
        → serialize to JSON response
```

### Delete Cascade Flow

```
DELETE /clipboard/entries/:id
    → DeleteClipboardEntry::execute(entry_id)                    [uc-app, MODIFIED]
        → delete selection (existing)
        → delete entry (existing)
        → delete event + representations (existing)
        → if search_index present:
            SearchIndexPort::remove_entry(entry_id)              [SYNCHRONOUS, in-transaction]
```

### Rebuild Flow

```
POST /search/rebuild
    → RebuildSearchIndex::execute()                              [uc-app, NEW]
        → scan all clipboard entries
        → write to temp tables (with double-write for concurrent captures)
        → atomic table swap via SQLite RENAME
        → update search_index_meta
```

---

## Suggested Build Order

The compiler-enforced dep graph allows only one valid incremental build order:

| Step | Crate | Work |
|------|-------|------|
| 1 | `uc-core` | Add `search/` domain models and two port traits (`SearchIndexPort`, `SearchKeyDerivationPort`) |
| 2 | `uc-app` | Add four new use cases; modify `DeleteClipboardEntry` with optional `SearchIndexPort`; add `SearchPorts` to `AppDeps` |
| 3 | `uc-infra` | Add migration; implement `SqliteSearchIndex`, `HkdfSearchKeyDerivation`, `text_extractor`, `tokenizer` |
| 4 | `uc-bootstrap` | Wire `SearchPorts` into `AppDeps`; pass `SearchIndexPort` into `DeleteClipboardEntry::with_search_index()` |
| 5 | `uc-daemon` | Add `search.rs` router; modify `DaemonClipboardChangeHandler` to call `IndexClipboardEntry` |
| 6 | Frontend | Add search UI calling `POST /search/query` |

Steps 1–2 compile independently of infra (only traits and use cases). Step 3 requires steps 1–2. Steps 4–5 require step 3. This matches how file sync, observability, and auth subsystems were added.

---

## Architectural Patterns

### Pattern 1: Optional Port via Builder Method

**What:** `.with_search_index(port)` on `DeleteClipboardEntry` — same shape as the existing `.with_file_cache_dir()`.
**When to use:** When a use case needs an optional cross-cutting dependency that did not exist at design time.
**Trade-offs:** Avoids breaking all existing call sites; slightly less explicit than making the port required.

### Pattern 2: On-Demand Key Derivation (no cached SearchKey)

**What:** Derive `SearchKey` from `MasterKey` via HKDF on each use case invocation, not at session unlock.
**When to use:** When the derived key is cheap to compute (one HKDF call) and caching adds surface area.
**Trade-offs:** Tiny per-request HKDF cost (negligible for HKDF-SHA256 on 32 bytes) vs. no risk of key leakage through a cached field.

### Pattern 3: Raw SQL for Posting-List Queries

**What:** `diesel::sql_query()` for search operations instead of Diesel's query builder.
**When to use:** When the query pattern (posting-list intersection/union with GROUP BY + HAVING) does not compose cleanly in Diesel's ORM.
**Trade-offs:** Loses compile-time schema checking for these specific queries; gains full SQL expressiveness. All other infra adapters keep Diesel ORM.

### Pattern 4: No FK Constraint on Index Tables

**What:** `search_document.entry_id` is TEXT, no `REFERENCES clipboard_entry(entry_id)`.
**When to use:** When the index must be independently rebuildable from source data, and deletions are driven by application logic.
**Trade-offs:** Orphaned index rows are possible if deletion crashes mid-way; full rebuild recovers this state.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Hooking IndexClipboardEntry Inside CaptureClipboardUseCase

**What people might do:** Call `IndexClipboardEntry` directly from `CaptureClipboardUseCase::execute()`.
**Why it's wrong:** `CaptureClipboardUseCase` lives in `uc-app` and must not depend on search ports. The use case's responsibility is capture-and-persist; post-capture orchestration belongs in `uc-daemon`'s change handler.
**Do this instead:** Call `IndexClipboardEntry` from `DaemonClipboardChangeHandler` after capture returns `EntryId`.

### Anti-Pattern 2: Eager SearchKey Derivation at Unlock Time

**What people might do:** Derive and cache `SearchKey` in a new `SearchSessionPort` triggered by `UnlockEncryptionWithPassphrase`.
**Why it's wrong:** Adds a lifecycle dependency that must be managed, cleared, and tested. Introduces risk of key leakage through cached state.
**Do this instead:** Derive on-demand via `SearchKeyDerivationPort::derive_search_key()`.

### Anti-Pattern 3: FK Constraint From search_document to clipboard_entry

**What people might do:** Add `REFERENCES clipboard_entry(entry_id) ON DELETE CASCADE` to avoid manual deletion logic.
**Why it's wrong:** Cascades make the index dependent on clipboard table lifecycle, preventing independent rebuild.
**Do this instead:** No FK; drive deletion from application code with `SearchIndexPort::remove_entry()` inside `DeleteClipboardEntry`.

### Anti-Pattern 4: Separate SQLite Database for Search

**What people might do:** Store the search index in a separate `.db` file to isolate it.
**Why it's wrong:** The existing `DbPool` already has WAL mode and r2d2 pooling configured. A second pool doubles resource overhead and complicates transaction semantics for the double-write rebuild strategy.
**Do this instead:** Add the three search tables to the same Diesel migration path, same pool.

---

## Sources

- `src-tauri/crates/uc-core/src/ports/security/encryption_session.rs` — `EncryptionSessionPort::get_master_key()` interface
- `src-tauri/crates/uc-core/src/security/model.rs` — `MasterKey` domain model (32-byte Argon2id → XChaCha20 chain)
- `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` — capture use case boundary (stops at returning `EntryId`)
- `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs` — optional builder pattern for `.with_file_cache_dir()`
- `src-tauri/crates/uc-app/src/deps.rs` — `AppDeps` grouped port structure (5 existing groups)
- `src-tauri/crates/uc-infra/src/security/key_material.rs` — `KeyScopePort` usage pattern
- `src-tauri/crates/uc-infra/src/security/encryption.rs` — XChaCha20-Poly1305 primitives
- `src-tauri/crates/uc-infra/src/db/pool.rs` — `embed_migrations!`, `run_pending_migrations` pipeline
- `src-tauri/crates/uc-infra/src/db/schema.rs` — existing Diesel table definitions and join constraints
- `src-tauri/crates/uc-infra/migrations/` — 11 existing migration directories confirming additive pattern
- `src-tauri/crates/uc-daemon/src/api/clipboard.rs` — router pattern (`pub fn router() -> Router<DaemonApiState>`)
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — L1/L2+ tier split, middleware order
- `docs/architecture/local-encrypted-search.md` — primary architecture spec (all 10 architecture review decisions)

---

_Architecture research for: Local Encrypted Search integration into hexagonal clipboard sync codebase_
_Researched: 2026-04-10_
