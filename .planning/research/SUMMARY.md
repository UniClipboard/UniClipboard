# Project Research Summary

**Project:** UniClipboard Desktop — v0.5.0 Local Encrypted Search
**Domain:** HMAC-keyed inverted index search over encrypted clipboard history
**Researched:** 2026-04-10
**Confidence:** HIGH

## Executive Summary

UniClipboard v0.5.0 adds local full-text search over clipboard history without ever writing plaintext search terms to disk. The approach is an HMAC-keyed inverted index: each token is hashed with a key derived from the session master key before being stored, so the SQLite database on disk contains no recoverable plaintext. This is a unique security posture compared to Alfred, Raycast, and Maccy, none of which encrypt their search indexes. The architecture is fully designed and documented; implementation is a matter of following the established hexagonal crate structure with high confidence.

The recommended implementation strategy is strictly layer-ordered: domain models and port traits in `uc-core` first, use cases in `uc-app` second, SQLite adapters and tokenizer in `uc-infra` third, wiring in `uc-bootstrap`, HTTP routes in `uc-daemon`, and React UI last. This order is enforced by the compiler — the dep graph does not allow shortcuts. Two new crates (`unicode-normalization`, `unicode-segmentation`) are the only new dependencies; all crypto work reuses existing crates already in the lockfile. The front-end surfaces the feature in two places: QuickPanel (query-only, replaces client-side substring filter) and Dashboard (query + content-type and time-range filters, revealing an already-wired-but-hidden Header component).

The highest-priority risks are security or data-integrity issues that are invisible to automated tests unless explicitly verified: missing HKDF domain separation for the search key, missing profile isolation columns in search tables, missing delete cascade, and search routes reachable in locked state. Each has a "looks done but isn't" failure mode that only manifests in production. The pitfall research provides a checklist of eight verification steps that must be gated before each relevant phase is marked complete.

## Key Findings

### Recommended Stack

The search feature requires only two new crate dependencies added to `uc-infra/Cargo.toml`: `unicode-normalization = "0.1"` (NFKC normalization, unicode-rs organization, stable since 0.1.x) and `unicode-segmentation = "1"` (UAX#29 word-boundary splitting, same organization). Everything else — keyed hashing, key derivation, SQLite via Diesel 2.3.5, HTTP via axum 0.7, async runtime via tokio — is already in the lockfile. HTML tag stripping and CJK bigram generation are implemented inline with no additional dependencies (~30 lines each).

**Note on search key derivation — cross-document discrepancy:** STACK.md recommends `blake3::derive_key()` (no new dep). ARCHITECTURE.md and PITFALLS.md both specify HKDF-SHA256. The architecture spec is the primary source; HKDF-SHA256 with a profile-scoped info context is the authoritative approach. The STACK.md blake3 recommendation should be treated as an alternative if HKDF is unavailable, but the architecture spec wins. This must be resolved in Phase 3 before any HMAC call is written.

**Core technologies:**

- `blake3 1.8.2` (existing): keyed hash for term tags; `derive_key` proposed as search key derivation alternative — already in uc-core and uc-infra; no new dep
- HKDF-SHA256 (architecture spec primary): search key derivation with `info="uc-search-index-v1\x00{profile_id}"` — may require `hkdf` crate; confirm against architecture spec before Phase 3
- `unicode-normalization 0.1` (NEW): NFKC normalization before tokenization — required for version-stable index behavior
- `unicode-segmentation 1` (NEW): UAX#29 word-boundary splitting for Latin text, paths, URLs, code tokens
- Diesel 2.3.5 + `diesel::sql_query()` (existing): all index reads/writes; raw SQL for posting-list AND/OR via GROUP BY + HAVING
- axum 0.7 (existing): search HTTP routes in `uc-daemon`, following existing clipboard router pattern

### Expected Features

The feature targets two UI surfaces with different scope. QuickPanel replaces the current client-side substring filter (`item.preview.toLowerCase().includes(q)`) with server-side HMAC exact-match — a behavior change users will notice. Dashboard reveals the currently-hidden `Header` component and adds richer filters. Both surfaces share the same daemon API.

**Must have (table stakes) — V1 launch:**

- Search input auto-focused on open in both surfaces
- Debounced query (200–300ms) — HMAC derivation cost makes per-keystroke firing expensive
- Result count shown alongside query ("12 results")
- Meaningful empty state with hint to try fewer words or check spelling
- Content-type filter pills (text / link / image / file) — Dashboard only; `Filter` enum already exists
- Time range presets (today / last 7 days / last 30 days) — Dashboard only
- Locked state gate: show unlock prompt, no partial results
- Results rendered via existing `ClipboardItemRow` — no new row component

**Should have (competitive) — V1.x after feedback:**

- File extension filter (`.pdf`, `.md`, `.png`) — developer workflow use case
- Boolean AND/OR syntax hint in placeholder or tooltip — power user differentiator
- Encrypted-at-rest search indicator in UI — trust-building, unique to UniClipboard

**Defer (V2+):**

- Term highlighting in result rows — HMAC returns entry IDs only, not match positions; requires position-aware index extension and security review
- Fuzzy/typo-tolerant search — incompatible with HMAC model without storing plaintext
- Semantic/embedding search — requires different security model entirely
- Custom absolute date range picker — API already supports `from_ms`/`to_ms`; presets cover >90% of intent

### Architecture Approach

The search subsystem integrates into all five Rust layers following the existing hexagonal pattern, with a strict compiler-enforced build order. Two new port traits live in `uc-core` (`SearchIndexPort`, `SearchKeyDerivationPort`). Four new use cases and one modified use case live in `uc-app`. Two new adapters (SQLite index, key derivation) and a tokenizer pipeline live in `uc-infra`. One new Diesel migration adds three tables (`search_document`, `search_posting`, `search_index_meta`) with no foreign keys to existing clipboard tables — the index is independently rebuildable. One new router file and one modified worker live in `uc-daemon`. The search key is derived on-demand per request, never cached or persisted.

**Major components:**

1. `SearchIndexPort` / `SearchKeyDerivationPort` (uc-core) — port traits; domain models `SearchQuery`, `SearchDocument`, `SearchPosting`, `SearchResult`; `SearchKey` newtype that never exposes raw `MasterKey` bytes
2. `SearchClipboardEntries`, `IndexClipboardEntry`, `RemoveIndexedEntry`, `RebuildSearchIndex` (uc-app) — use cases; `DeleteClipboardEntry` modified via optional builder to accept `SearchIndexPort`; `AppDeps` gains `SearchPorts` group
3. `SqliteSearchIndex`, search key derivation adapter, `text_extractor`, `tokenizer` (uc-infra) — adapters and tokenization pipeline (NFKC → lowercase → word-boundary split → CJK bigram → HMAC tags); one Diesel migration with profile-scoped tables
4. `search.rs` router (uc-daemon) — three HTTP routes (`/search/query`, `/search/rebuild`, `/search/status`) each with per-handler session unlock guard; `DaemonClipboardChangeHandler` modified to call `IndexClipboardEntry` after capture
5. Search UI (React frontend) — QuickPanel gets query input replacing client-side filter; Dashboard reveals hidden Header with full filter controls; debounce and stale-response cancellation from first implementation

### Critical Pitfalls

1. **Delete cascade missing search cleanup** — Inject `SearchIndexPort` into `DeleteClipboardEntry` and call `remove_entry(entry_id)` synchronously as part of the delete chain. Best-effort async cleanup is explicitly ruled out by the architecture spec. Verify: delete an entry, confirm zero `search_posting` rows for that `entry_id`.

2. **HMAC key without domain separation** — Must use HKDF-SHA256 (or `blake3::derive_key` as alternative — see discrepancy note) with a purpose+profile-scoped info context, never raw `MasterKey` bytes. The `SearchKey` newtype must be the only input to any HMAC call. Verify by code search: no direct `MasterKey` reference in the search module.

3. **Profile isolation failure** — Both `search_document` and `search_posting` must have a `profile_id` column from the first migration. All queries must include `WHERE profile_id = ?`. Adding this column after the fact requires a full rebuild and non-trivial migration.

4. **Search routes reachable in locked state** — `router_l2_plus` has no L3 session-unlock layer. Each search/rebuild handler must check `encryption_session.is_ready()` and return 423 Locked. Verify with integration test: valid JWT + locked session returns 423, not 500.

5. **Rebuild blocks async runtime** — HMAC over thousands of entries is CPU-bound. Spawn rebuild on `tokio::task::spawn_blocking`. Emit progress via existing `DaemonApiEventEmitter` WS mechanism. Frontend must show "rebuilding index" banner.

## Implications for Roadmap

The compiler-enforced dep graph mandates a layer-by-layer build order. The following phase structure follows that constraint while grouping work that can be independently reviewed and tested.

### Phase 1: Core Domain and Port Contracts

**Rationale:** `uc-core` has zero deps on other crates. Nothing in uc-app, uc-infra, or uc-daemon can compile with search until these traits and models exist. Define the contract before any implementation.
**Delivers:** `SearchIndexPort`, `SearchKeyDerivationPort` traits; `SearchQuery`, `SearchDocument`, `SearchPosting`, `SearchResult`, `SearchKey` (newtype) domain models; `SearchPorts` stub in `AppDeps`
**Addresses:** Foundation for all subsequent features; `SearchKey` newtype established here prevents key misuse in later phases
**Avoids:** Domain model drift — locking the interface before infra choices constrain it

### Phase 2: Use Cases and Delete Integration

**Rationale:** `uc-app` depends only on `uc-core`. Use cases can be written and unit-tested with mock port implementations before any SQLite code exists.
**Delivers:** `SearchClipboardEntries`, `IndexClipboardEntry`, `RemoveIndexedEntry`, `RebuildSearchIndex` use cases; `DeleteClipboardEntry` extended with optional `SearchIndexPort` via builder
**Addresses:** Delete cascade pitfall — synchronous search cleanup integrated here, not deferred
**Avoids:** Pitfall 1 (orphaned postings); `SearchKey` newtype boundary enforced at use-case layer

### Phase 3: SQLite Schema Migration and Tokenizer Pipeline

**Rationale:** `uc-infra` can now implement ports. The migration and tokenizer are the most technically complex backend work and must be correct before the adapters that depend on them. The key derivation discrepancy (blake3 vs HKDF-SHA256) must be resolved here before any HMAC call is written.
**Delivers:** Diesel migration (`search_document`, `search_posting`, `search_index_meta` with `profile_id` columns); tokenizer pipeline (NFKC + lowercase + word-boundary + CJK bigram); search key derivation adapter (HKDF-SHA256 per architecture spec — confirm `hkdf` dep need before starting)
**Uses:** `unicode-normalization`, `unicode-segmentation` (new deps); keyed hash via existing crate (blake3 or HKDF as resolved)
**Avoids:** Pitfall 6 (profile isolation — `profile_id` column required from first migration); Pitfall 3 (key derivation implemented before any HMAC call)

### Phase 4: SQLite Index Adapter and Rebuild Strategy

**Rationale:** The `SqliteSearchIndex` adapter is the most risk-laden infra piece: posting-list AND/OR queries via `diesel::sql_query()`, rebuild dual-write strategy, atomic swap design. Isolated in its own phase so the swap mechanism is reviewed before it is wired to live endpoints.
**Delivers:** `SqliteSearchIndex` implementing `SearchIndexPort`; full rebuild with atomic swap (version-flag strategy in `search_index_meta` recommended over `RENAME TABLE` to avoid SQLite exclusive lock timeout); composite index on `(profile_id, term_tag)`; `search_blocked` flag and version-mismatch detection
**Avoids:** Pitfall 4 (SQLite exclusive lock timeout); Pitfall 5 (tokenizer version mismatch — `search_blocked` flag and query guard implemented here)

### Phase 5: Bootstrap Wiring and Daemon HTTP Routes

**Rationale:** `uc-bootstrap` wires `SearchPorts` into `AppDeps`. `uc-daemon` adds the router and modifies `DaemonClipboardChangeHandler`. Both proceed once infra adapters exist. Full end-to-end backend is delivered and verifiable here.
**Delivers:** Full backend: capture → index → search → delete cascade; three HTTP endpoints; `DaemonClipboardChangeHandler` calls `IndexClipboardEntry` after capture (gated on session ready); WS rebuild progress events
**Avoids:** Pitfall 2 (per-handler session unlock guard returning 423, from first commit); Pitfall 7 (rebuild on `spawn_blocking` with WS progress)

### Phase 6: Frontend Search UI

**Rationale:** Backend is fully functional. Frontend work is UI wiring against a known API contract.
**Delivers:** QuickPanel search replacing client-side substring filter; Dashboard Header revealed with query input, content-type pills, time-range presets; debounced query (200–300ms) with stale-response cancellation; locked state shows unlock prompt; result count display; `ClipboardItemRow` reused unchanged
**Addresses:** All V1 must-have features from FEATURES.md
**Avoids:** Pitfall 8 (debounce and stale-response logic in component from first implementation)

### Phase Ordering Rationale

- Phases 1–2 are purely trait/use-case code and compile without infra or daemon — fast iteration, mockable
- Phase 3 (migration + tokenizer) must precede Phase 4 (adapter) because the adapter depends on the schema and tokenizer module
- Phases 3–4 together constitute all uc-infra search work — splitting allows the riskier swap strategy to be reviewed in isolation
- Phase 5 (daemon wiring) is last Rust work; the compiler validates full integration at this point
- Phase 6 (frontend) has a known API contract from Phase 5 and proceeds without guessing at backend behavior

### Research Flags

Phases likely needing deeper research during planning:

- **Phase 3 (Key derivation):** The blake3 vs HKDF-SHA256 discrepancy between STACK.md and ARCHITECTURE.md must be resolved before implementation. If HKDF-SHA256 is chosen, confirm whether the `hkdf` crate is needed or whether blake3 `derive_key` satisfies the spec. Read the architecture doc section on key derivation directly.
- **Phase 4 (Rebuild swap strategy):** The version-flag vs. `RENAME TABLE` trade-off should be reviewed against the actual `busy_timeout` configuration and pool concurrency before committing to an approach.
- **Phase 5 (WS progress events):** The existing `DaemonApiEventEmitter` pattern (e.g., file sync worker) should be read before adding search rebuild progress events to ensure consistent event naming and frontend handler compatibility.

Phases with standard patterns (no additional research needed):

- **Phase 1 (Core domain):** Follows established `uc-core/src/ports/` patterns exactly.
- **Phase 2 (Use cases):** Follows existing `delete_clipboard_entry.rs` optional builder pattern.
- **Phase 6 (Frontend):** React debounce and stale-response cancellation are standard patterns; `ClipboardItemRow` reuse is explicitly specified.

## Confidence Assessment

| Area         | Confidence | Notes                                                                                                     |
| ------------ | ---------- | --------------------------------------------------------------------------------------------------------- |
| Stack        | HIGH       | All crates verified against Cargo.lock and crates.io. Only 2 new deps. One discrepancy flagged (blake3 vs HKDF). |
| Features     | HIGH       | Verified against live competitor docs (Alfred, Raycast, Maccy) and existing codebase components.          |
| Architecture | HIGH       | Based on primary source code reading across all 5 Rust crates. Port patterns and migration pipeline confirmed. |
| Pitfalls     | HIGH       | Each pitfall grounded in specific source file and line numbers in the actual codebase.                    |

**Overall confidence:** HIGH

### Gaps to Address

- **Key derivation mechanism (HIGH PRIORITY):** STACK.md says `blake3::derive_key()` (no new dep). ARCHITECTURE.md and PITFALLS.md both say HKDF-SHA256. This must be resolved before Phase 3 begins — confirm against the architecture spec and decide whether the `hkdf` crate is required or blake3 is acceptable.
- **Multi-profile current state:** The existing schema has no `profile_id` in any table. The exact shape of `KeyScopePort::current_scope()` and whether `profile_id` is currently meaningful should be confirmed before the Phase 3 migration is written.
- **QuickPanel behavior regression:** The current client-side filter supports substring mid-word matching (`insta` matching `instagram`). Replacing it with HMAC exact-token search is a breaking UX change. A user-facing communication strategy (placeholder text, tooltip, or release note) should be decided before Phase 6 begins.
- **Rebuild progress event schema:** Read a working WS progress event example (e.g., file sync worker) before Phase 5 to ensure consistent field names and event type discriminator with existing frontend event handlers.

## Sources

### Primary (HIGH confidence)

- `src-tauri/Cargo.lock` — blake3 1.8.2, all existing crate versions confirmed
- `src-tauri/crates/uc-core/src/ports/security/encryption_session.rs` — `EncryptionSessionPort` interface
- `src-tauri/crates/uc-app/src/usecases/delete_clipboard_entry.rs` — optional builder pattern, hardcoded delete chain
- `src-tauri/crates/uc-app/src/deps.rs` — `AppDeps` grouped port structure (5 existing groups)
- `src-tauri/crates/uc-infra/src/db/pool.rs` — WAL mode, `busy_timeout = 5000`, `diesel::sql_query` pattern confirmed
- `src-tauri/crates/uc-infra/src/db/schema.rs` — existing table definitions, no `profile_id` confirmed
- `src-tauri/crates/uc-infra/migrations/` — 11 existing migration directories, additive pattern confirmed
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — L2+ tier, explicit note that L3/L4 not implemented
- `src-tauri/crates/uc-daemon/src/api/clipboard.rs` — router pattern (`pub fn router() -> Router<DaemonApiState>`)
- `docs/architecture/local-encrypted-search.md` — primary architecture spec (all 10 review decisions)
- `src/quick-panel/ClipboardHistoryPanel.tsx` — client-side filter at `includes(q)` confirmed
- `src/pages/DashboardPage.tsx` — hidden Header component confirmed

### Secondary (MEDIUM confidence)

- Alfred Clipboard History documentation (https://www.alfredapp.com/help/features/clipboard/) — competitor feature baseline
- Raycast Clipboard History manual (https://manual.raycast.com/windows/clipboard-history) — competitor feature baseline
- Maccy open source (https://github.com/p0deje/Maccy) — competitor feature baseline

### Tertiary (LOW confidence)

- crates.io for unicode-normalization (0.1.25) and unicode-segmentation (1.12.0) — version currency only; API stability is HIGH from unicode-rs organization track record

---

_Research completed: 2026-04-10_
_Ready for roadmap: yes_
