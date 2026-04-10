# Pitfalls Research

**Domain:** Local HMAC-keyed encrypted search added to existing Rust/Tauri/SQLite clipboard app
**Researched:** 2026-04-10
**Confidence:** HIGH — grounded in actual codebase code paths, not generic patterns

---

## Critical Pitfalls

### Pitfall 1: Delete Cascade Missing Search Cleanup

**What goes wrong:**
`DeleteClipboardEntry::execute` runs a fixed sequential chain: delete selection → delete entry → delete event+representations. There is no extension point and no event emitted after deletion completes. `search_document` and `search_posting` rows for the deleted `entry_id` are never removed. Orphaned postings accumulate silently in perpetuity. Future searches return dead results that resolve to missing clipboard entries — the caller gets a result set with IDs that cannot be loaded.

**Why it happens:**
The use case (`uc-app/src/usecases/delete_clipboard_entry.rs`) has no knowledge of search. Adding the `RemoveIndexedClipboardEntry` use case is a separate feature that must be wired into the delete path, but there is no hook or event bus to trigger it after deletion. Developers see the deletion "work" in tests and miss the search-index side effect entirely.

**How to avoid:**
Choose exactly one integration approach before implementation begins:
- **Option A (recommended):** Inject a `SearchIndexPort` into `DeleteClipboardEntry` and call `remove_indexed_entry(entry_id)` as step 1.5 (after fetching the entry, before deleting the selection). This keeps the deletion contract synchronous and atomic within the use case.
- **Option B:** Emit a typed `EntryDeleted` domain event from the use case and have the search subsystem subscribe. Requires an event bus that does not currently exist.

The architecture spec states explicitly: deletion must be part of the normal deletion workflow, not best-effort async cleanup. Option A satisfies this; Option B with a fire-and-forget emitter does not.

**Warning signs:**
- Deleting a clipboard entry and then searching for its content still returns a result
- `search_posting` row count grows while `clipboard_entry` row count does not
- Rebuild produces a smaller posting list than the live index

**Phase to address:**
The phase that implements `RemoveIndexedClipboardEntry` — must be done in the same phase as delete-path integration, not deferred.

---

### Pitfall 2: L3/L4 Unlock Gate Missing on Search Routes

**What goes wrong:**
The daemon route comment in `api/routes.rs` (lines 71-73) states explicitly: "L3/L4 permission enforcement is NOT implemented in Phase 75 (deferred to future phases)." All routes in `router_l2_plus` are callable with a valid JWT even when the encryption session is locked. Search routes added to this router are reachable in locked state. The search key is unavailable when locked, so the handler either panics, returns an internal error, or — worst — calls `HMAC(None, token)` with a zero key if the derivation path is not guarded.

**Why it happens:**
The router is split into L1 (no auth) and L2+ (JWT required) — but there is no L3 layer for "encryption session must be unlocked." Adding search routes to `router_l2_plus` feels complete because auth is enforced, but the session state check is absent. Each handler must guard independently or a middleware gate is needed.

**How to avoid:**
Implement a per-handler encryption-session guard as the first action in every search and rebuild handler:
```rust
let search_key = state.runtime
    .as_ref()
    .and_then(|r| r.search_key())
    .ok_or_else(|| ApiError::session_locked())?;
```
Add an integration test that hits the search endpoint with a valid JWT while the session is in locked state and asserts `423 Locked` (or equivalent domain error), not `500`.

**Warning signs:**
- Search endpoint returns 500 with a key-not-found message rather than a clear "session locked" error
- Frontend does not distinguish "session locked" from "search failed" — both show the same error toast
- No test covers the locked-state search scenario

**Phase to address:**
The phase that adds daemon search HTTP routes — the guard must be present from the first commit, not added in a follow-up phase.

---

### Pitfall 3: HMAC Key Derivation Without Domain Separation

**What goes wrong:**
The existing `EncryptionRepository` uses `MasterKey` bytes directly as the XChaCha20 content encryption key (see `encryption.rs` — `XChaCha20Poly1305::new_from_slice(master_key.as_bytes())`). If the search subsystem also uses `MasterKey` bytes directly as the HMAC-SHA256 key for term tags, a side-channel on the index (posting list sizes, tag frequency analysis) becomes a partial side-channel on the content key material. Additionally, if the HMAC derivation context does not bind to `profile_id`, the same search term produces the identical `term_tag` across all profiles — allowing cross-profile correlation of which terms appear in each profile's history.

**Why it happens:**
Developers see that `MasterKey` is available in the unlocked session and use it directly for HMAC to avoid adding new derivation code. The cryptographic consequence (purpose conflation) is non-obvious and does not cause any functional test failure.

**How to avoid:**
Derive a dedicated search key using HKDF-SHA256 from `MasterKey` with a distinct info string that includes both the purpose and the profile ID:
```
search_key = HKDF-SHA256(
    ikm  = master_key.as_bytes(),
    salt = b"",
    info = b"uniclipboard-search-v1\x00{profile_id}"
)
```
The search key must never be stored to disk. It must be rederived on each unlock and held only in memory within the session. The `SearchIndexPort` trait must accept the derived `SearchKey` newtype as a parameter, never raw `MasterKey`.

**Warning signs:**
- The search subsystem imports `MasterKey` directly rather than a separate `SearchKey` newtype
- No HKDF or equivalent derivation step exists between `MasterKey` and the HMAC call
- The `term_tag` for the same plaintext token is identical across two different profiles

**Phase to address:**
The phase that implements key derivation and `SearchIndexPort` — HKDF derivation must be the first thing implemented, before any HMAC call is written.

---

### Pitfall 4: SQLite Exclusive Lock Timeout During Full Rebuild Atomic Swap

**What goes wrong:**
The spec's rebuild strategy is: write to a temp table, then atomically swap (rename) the active tables. In SQLite WAL mode, a `RENAME TABLE` or equivalent atomic swap requires briefly acquiring an EXCLUSIVE lock on the database file. If a concurrent search query holds an open read transaction at that moment, SQLite must wait. The pool customizer sets `busy_timeout = 5000` ms (`db/pool.rs` line 34). A rebuild that finishes writing but then waits 5+ seconds for the exclusive lock to swap fails with `SQLITE_BUSY`, leaving the database in a state where the old (pre-rebuild) index is still active and the temp table is stranded.

**Why it happens:**
The WAL-mode documentation emphasizes that readers do not block writers for ordinary INSERT/UPDATE. It is less obvious that `RENAME TABLE` still requires exclusive access in SQLite. Developers test rebuild in isolation (no concurrent readers) and assume the lock is fine.

**How to avoid:**
Design the rebuild swap to be exclusive-lock-aware. Choose one approach:
- **Option A (recommended):** Instead of renaming tables, use a version flag in `search_index_meta.active_index_version` to point at the completed temp data. Queries read from the version-tagged rows. No exclusive lock needed.
- **Option B:** Use a short serialization window — pause new search queries for the duration of the rename only (not the entire rebuild). The rebuild write phase runs concurrently; only the final rename is serialized.

**Warning signs:**
- Integration tests pass without concurrent search requests; rebuild fails intermittently under load
- `SQLITE_BUSY` errors appear in logs during rebuild completion but not during the write phase
- Rebuild state in `search_index_meta` shows `started` but never transitions to `completed`

**Phase to address:**
The phase that implements `RebuildSearchIndex` — the swap mechanism must be designed atomically from the start.

---

### Pitfall 5: Tokenizer Version Change Leaves Stale Index Without Query Guard

**What goes wrong:**
The schema includes `index_version` in `search_document` — correct. The pitfall is the query path: when normalization rules change and the version is bumped, the HMAC tags for new queries are derived from the new normalization, but existing rows in `search_posting` were derived from the old normalization. A search for "café" after a NFKC rule change produces `HMAC(k, "cafe")` but the index still contains `HMAC(k, "café")` (pre-normalization). The query returns zero results until rebuild completes, with no user-facing explanation.

**Why it happens:**
Developers bump `index_version` in code and trigger a background rebuild, but do not gate queries on rebuild completion. The query path runs against a mix of old-version and new-version data with no awareness of the mismatch.

**How to avoid:**
- On session unlock, read `search_index_meta.active_index_version` and compare to the code's declared `CURRENT_INDEX_VERSION` constant.
- If they differ, immediately initiate a rebuild and set a `search_blocked` flag in memory.
- All search handlers must check this flag and return a `503 index_rebuild_in_progress` (or equivalent typed error) until the rebuild completes and the version is updated.
- The rebuild's dual-write strategy (new entries written to both active and temp tables during rebuild) handles new captures during the rebuild window. The only gap is queries against old-version data — the `search_blocked` flag closes this.

**Warning signs:**
- Zero search results after any code change that touches the tokenizer or normalization rules
- `search_index_meta.active_index_version` does not match the binary's declared version constant after an upgrade
- No mechanism exists to block search queries while rebuild is in progress

**Phase to address:**
The phase that implements index schema and tokenizer — the version-mismatch detection and query-block logic must be added in the same phase, not as a follow-up.

---

### Pitfall 6: Profile Isolation Failure — Cross-Profile Data Leakage

**What goes wrong:**
The architecture spec (review item 5) requires search key derivation and index tables to be scoped to `profile_id`. If `search_document` and `search_posting` tables have no `profile_id` column and a single global `search_key` is used, a user with two profiles (or a future multi-user local installation) finds entries from other profiles in search results. The HMAC tags match across profiles because the same key was used.

**Why it happens:**
The current database schema (`schema.rs`) shows no multi-profile concept — all tables are single-tenant. It is easy to implement search as single-tenant "because that is what everything else does" and defer profile isolation to a later phase that never arrives.

**How to avoid:**
Add `profile_id` as a column in both `search_document` and `search_posting` from day one. All queries must include `WHERE profile_id = ?`. The HKDF derivation for the search key must include the profile ID in the info context (covered in Pitfall 3). The rebuild use case must accept a `profile_id` parameter and rebuild only that profile's index. This is explicitly called out in the architecture spec as a required isolation dimension.

**Warning signs:**
- `search_document` table has no `profile_id` column
- The `RebuildSearchIndex` use case takes no profile scope parameter
- Searches return entries that belong to a different profile context

**Phase to address:**
The phase that defines the SQLite schema for search tables — isolation must be designed in from the first migration, not added later.

---

### Pitfall 7: Index Rebuild Blocks the Async Runtime

**What goes wrong:**
A full index rebuild iterates all clipboard entries and performs HMAC for every token in every entry. For a large history (thousands of entries with thousands of tokens each), this is CPU-intensive and starves the tokio async runtime if run on the async executor directly. The daemon's clipboard sync, peer events, and WS broadcasts all stall during the rebuild. Additionally, rebuild progress is invisible to the user, who sees the search UI stop responding without explanation.

**Why it happens:**
Developers implement rebuild as a straightforward async loop over entries and forget that HMAC is CPU-bound. In tests with small datasets, it completes instantly. In production with large clipboard history, it occupies the runtime for several seconds.

**How to avoid:**
- Spawn the rebuild on `tokio::task::spawn_blocking` or a dedicated thread pool rather than directly on an async task.
- Emit incremental progress events via the existing WS broadcast mechanism. The daemon already has `DaemonApiEventEmitter` for this pattern — `FileSyncOrchestratorWorker` is a working example of WS progress events.
- The frontend search UI should show a "rebuilding index" banner when `search_index_meta.rebuild_state = in_progress`, rather than simply disabling search silently.

**Warning signs:**
- Other daemon operations (clipboard sync, peer events) stall during a rebuild
- No WS event is emitted when rebuild starts or completes
- The search UI has no visual indication that a rebuild is running

**Phase to address:**
The phase that implements `RebuildSearchIndex` use case and its daemon route — WS progress events must be part of the definition of done for this phase.

---

### Pitfall 8: Frontend Search Debounce and Out-of-Order Result Flickering

**What goes wrong:**
Without a frontend debounce, every keystroke fires a new HTTP request to the search endpoint. If responses arrive out-of-order (response for query "fo" arrives after response for "foo"), the UI displays stale results. This is particularly visible during the first few keystrokes when results change rapidly, causing the results list to flash between multiple intermediate states.

**Why it happens:**
Developers wire the search input's `onChange` directly to the fetch call with no debounce or stale-response guard. React state updates cause the results list to flash with each keystroke.

**How to avoid:**
- Debounce the search input with a 150-300ms delay before firing the HTTP request.
- Track the current query string in a `useRef` and discard responses whose query does not match the current ref value (stale-response cancellation).
- For React 18+, wrap the results state update in `startTransition` so the browser can interrupt the render if a newer query arrives.
- Prefer a single search result state that transitions atomically — new results replace old atomically, never partially populated.

**Warning signs:**
- Typing quickly causes the results list to flicker between multiple intermediate states
- Old results are briefly visible while a newer query is in-flight
- No debounce utility is imported in the search input component

**Phase to address:**
The phase that implements the search UI component — debounce and stale-response logic must be in the component from the first implementation.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|---|---|---|---|
| Use `MasterKey` bytes directly as HMAC key | No derivation code needed | Purpose conflation; side-channel risk; no profile isolation | Never |
| Add search routes to `router_l2_plus` without unlock gate | Fast to wire | Search callable in locked state; crashes or returns wrong error | Never |
| Store `search_posting` without `profile_id` column | Simpler schema | Cannot isolate profiles; migration requires full rebuild later | Never |
| Run rebuild on async task without `spawn_blocking` | Simpler code | Starves runtime during CPU-intensive HMAC loop | Never |
| Skip debounce on search input | Simpler frontend | Flickering; out-of-order responses; poor UX | Never |
| Implement delete cascade as best-effort async cleanup | Avoids modifying `DeleteClipboardEntry` | Orphaned postings; stale search results | Never |
| No query guard during rebuild window | Simpler rebuild logic | Users see zero results for valid terms during version migration | Never |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|---|---|---|
| `DeleteClipboardEntry` + search index | Add search cleanup as a separate async cleanup job | Inject `SearchIndexPort` into the use case and call synchronously as part of the delete chain |
| `router_l2_plus` + search routes | Assume L2 JWT auth is sufficient | Add per-handler encryption-session guard; return 423 when locked |
| `MasterKey` → HMAC | Use `MasterKey` bytes directly | HKDF-SHA256 with `"uniclipboard-search-v1\x00{profile_id}"` info context |
| SQLite rebuild swap | Use `RENAME TABLE` under WAL without reader coordination | Use version flag in `search_index_meta` or pause readers during exclusive rename window |
| Tokenizer version bump | Query against stale index during rebuild | Set `search_blocked` flag; return 503 until rebuild completes |
| Rebuild CPU work | Run HMAC loop on tokio executor | `spawn_blocking` for rebuild; use `DaemonApiEventEmitter` for WS progress |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|---|---|---|---|
| Synchronous HMAC loop for all tokens in rebuild | Tokio runtime stalls; clipboard sync delays during rebuild | `spawn_blocking` for rebuild; dedicated thread pool | ~500+ entries with large text content |
| No composite index on `(profile_id, term_tag)` in `search_posting` | AND/OR intersection queries do full table scans | Composite index at migration time | ~10K+ posting rows |
| Frontend fires HTTP request on every keystroke | High request rate; out-of-order responses | 150-300ms debounce; stale-response discard | Any user who types faster than 1 char/300ms |
| Rebuild double-write without `ON CONFLICT` clause | Temp table accumulates duplicates for rapidly-added entries | `INSERT OR REPLACE` or `ON CONFLICT DO UPDATE` in temp writes | During any rebuild that overlaps with active capture |
| No `busy_timeout` increase for rebuild connection | Exclusive lock timeout during table swap | Increase timeout only for rebuild connection, or use version-flag approach | Under concurrent search load during rebuild |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---|---|---|
| HMAC key without domain separation from content key | Side-channel on index may partially reveal content key material | HKDF derivation with distinct purpose string |
| HMAC derivation without profile binding | Cross-profile term correlation attack | Include `profile_id` in HKDF info context |
| Storing derived `search_key` to disk | Key exposure on disk theft | Hold search key in memory only; rederive on each unlock |
| Search endpoint accessible in locked state | Search key unavailable; potential panic or zero-key HMAC | Per-handler encryption-session guard; return 423 when locked |
| Logging normalized tokens before HMAC | Plaintext search terms appear in structured logs | Ensure tokenizer and HMAC code paths do not emit normalized tokens at INFO/DEBUG level |
| Logging query text in HTTP access logs | Search history exposed in logs | Log only query hash or query length, never plaintext query content |

---

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---|---|---|
| No indication that index rebuild is running | User sees empty results and thinks search is broken | Show "rebuilding index" banner; disable search with explanation |
| No indication that session must be unlocked to search | User sees generic error toast with no actionable message | Return a distinct `session_locked` error code; frontend shows unlock prompt |
| Search results flash between old and new during typing | Disorienting; makes app feel buggy | Debounce input; discard out-of-order responses; use `startTransition` |
| Rebuild triggered silently on version mismatch after app update | User confused why searches return nothing after update | Show progress UI immediately when version mismatch detected at unlock |
| Extension filter applied silently to non-file entries | User searches for `md` files and gets no text entries back | Define and document filter semantics; return a warning if extension filter applied to non-file context |

---

## "Looks Done But Isn't" Checklist

- [ ] **Delete cascade:** Search index cleanup is synchronous in the delete path — verify by deleting an entry and confirming zero rows in `search_posting` for that `entry_id`
- [ ] **Unlock gate:** Search endpoint returns 423 (or domain-equivalent) when session is locked with a valid JWT — verify with an integration test
- [ ] **Key derivation:** A `SearchKey` newtype exists and is derived via HKDF; `MasterKey` is never passed directly to any HMAC call — verify by code search for `MasterKey` in the search module
- [ ] **Profile isolation:** `search_document` and `search_posting` both have a `profile_id` column; all queries are scoped — verify via schema and query code review
- [ ] **Rebuild blocks queries:** `search_index_meta.rebuild_state = in_progress` causes search handlers to return 503 — verify with an integration test that starts a rebuild and immediately fires a search
- [ ] **Version mismatch detection:** On unlock, code compares `active_index_version` to `CURRENT_INDEX_VERSION` and triggers rebuild if mismatched — verify by manually writing an old version into `search_index_meta` and unlocking
- [ ] **Rebuild progress events:** WS broadcasts a rebuild-started and rebuild-completed event — verify with the existing WS test harness
- [ ] **Frontend debounce:** Search input fires no request within 150ms of a keystroke — verify by inspecting component code and network tab

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---|---|---|
| Orphaned postings from missing delete cascade | MEDIUM | Run a full rebuild to clear orphans; add the delete-cascade integration; future deletes are clean |
| Wrong search key (reused `MasterKey` without HKDF) | HIGH | All existing search tags are invalidated; rederive key with HKDF; full index rebuild required; no content data loss |
| Cross-profile posting leak (missing `profile_id` column) | HIGH | Schema migration to add `profile_id`; rebuild each profile's index separately; audit any cross-profile query paths |
| Stale index after tokenizer version bump (no query guard) | LOW | Add query guard; trigger rebuild; serve 503 until rebuild completes |
| Stranded rebuild from exclusive lock timeout | LOW | Clear `rebuild_state` in `search_index_meta`; retry rebuild with version-flag approach instead of table rename |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---|---|---|
| Delete cascade missing search cleanup | Phase: `IndexClipboardEntry` + `RemoveIndexedClipboardEntry` integration | Delete an entry; confirm zero `search_posting` rows for that `entry_id` |
| L3/L4 unlock gate missing | Phase: Daemon search HTTP routes | Integration test: valid JWT + locked session → 423, not 500 |
| HMAC key without domain separation | Phase: Key derivation + `SearchIndexPort` | Code review: no direct `MasterKey`→HMAC call; HKDF with purpose string present |
| SQLite exclusive lock timeout during rebuild swap | Phase: `RebuildSearchIndex` | Concurrent search queries + rebuild under load; rebuild completes without `SQLITE_BUSY` |
| Tokenizer version mismatch — no query guard | Phase: Index schema + tokenizer | Manually insert old version in meta; confirm search returns 503, not empty results |
| Profile isolation failure | Phase: SQLite schema migration | Confirm `profile_id` column in both tables; cross-profile query returns zero results |
| Rebuild blocks async runtime | Phase: `RebuildSearchIndex` | Rebuild with 1000-entry dataset; confirm clipboard sync events continue uninterrupted during rebuild |
| Frontend result flickering | Phase: Search UI component | Keystroke test at 100ms intervals; confirm results don't flash intermediate states |

---

## Sources

- Codebase: `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs` — hardcoded delete chain with no search extension point
- Codebase: `src-tauri/crates/uc-daemon/src/api/routes.rs` lines 71-73 — explicit note that L3/L4 not implemented, deferred to future phases
- Codebase: `src-tauri/crates/uc-infra/src/db/pool.rs` — WAL mode setup, `busy_timeout = 5000`, pool customizer pattern
- Codebase: `src-tauri/crates/uc-infra/src/security/encryption.rs` — `MasterKey` direct-use pattern; no HKDF currently in the codebase
- Codebase: `src-tauri/crates/uc-infra/src/db/schema.rs` — no `profile_id` in any current table; single-tenant schema
- Architecture spec: `docs/architecture/local-encrypted-search.md` — rebuild dual-write (review item 10), hard-delete semantics (item 4), profile isolation requirement (item 5), query execution order, `index_version` requirement, unlock-gate constraint, no-update immutable entries (item 3)

---

_Pitfalls research for: local HMAC-keyed encrypted search on existing Tauri/Rust/SQLite clipboard app_
_Researched: 2026-04-10_
