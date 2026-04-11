# Roadmap: UniClipboard Desktop

## Milestones

- ✅ **v0.1.0 Daily Driver** — shipped 2026-03-06
- ✅ **v0.2.0 Architecture Remediation** — shipped 2026-03-09
- ✅ **v0.3.0 Log Observability & Feature Expansion** — shipped 2026-03-17
- ✅ **v0.4.0 Runtime Mode Separation** — archived 2026-04-09 with known gaps accepted
- 🚧 **v0.5.0 Local Encrypted Search** — Phases 88-93 (in progress)

## Archived Milestones

<details>
<summary>✅ v0.1.0 Daily Driver</summary>

See:

- `.planning/milestones/v0.1.0-ROADMAP.md`
- `.planning/milestones/v0.1.0-REQUIREMENTS.md`
- `.planning/milestones/v0.1-MILESTONE-AUDIT.md`

</details>

<details>
<summary>✅ v0.2.0 Architecture Remediation</summary>

See:

- `.planning/milestones/v0.2.0-ROADMAP.md`
- `.planning/milestones/v0.2.0-REQUIREMENTS.md`
- `.planning/milestones/v0.2.0-MILESTONE-AUDIT.md`

</details>

<details>
<summary>✅ v0.3.0 Log Observability & Feature Expansion</summary>

See:

- `.planning/milestones/v0.3.0-ROADMAP.md`
- `.planning/milestones/v0.3.0-REQUIREMENTS.md`
- `.planning/milestones/v0.3.0-MILESTONE-AUDIT.md`

</details>

<details>
<summary>✅ v0.4.0 Runtime Mode Separation</summary>

See:

- `.planning/milestones/v0.4.0-ROADMAP.md`
- `.planning/milestones/v0.4.0-REQUIREMENTS.md`
- `.planning/milestones/v0.4.0-MILESTONE-AUDIT.md`
- `.planning/milestones/v0.4.0-phases/`

Archive note:

- Archived on 2026-04-09
- Archived with known gaps accepted
- Main remaining gaps at archive time:
  - planning files and requirement bookkeeping still needed cleanup
  - GUI-launched daemon still did not inherit OTLP endpoint automatically
  - some verification files were still missing or stale

</details>

## 🚧 v0.5.0 Local Encrypted Search (In Progress)

**Milestone Goal:** Add searchable clipboard history via a local HMAC-keyed inverted index, so users can find past clipboard entries by keyword without exposing plaintext on disk.

### Phases

- [ ] **Phase 88: Core Domain and Port Contracts** - Define SearchIndexPort, SearchKeyDerivationPort, domain models, and SearchKey newtype in uc-core
- [x] **Phase 89: Use Cases and Delete Integration** - Implement four search use cases in uc-app and extend DeleteClipboardEntry with synchronous search cascade (completed 2026-04-10)
- [x] **Phase 90: SQLite Schema Migration and Tokenizer Pipeline** - Add Diesel migration for search tables and implement tokenizer pipeline with HKDF key derivation in uc-infra (completed 2026-04-11)
- [x] **Phase 91: SQLite Index Adapter and Rebuild Strategy** - Implement SqliteSearchIndex adapter with version-flag atomic swap rebuild and search_blocked guard in uc-infra (completed 2026-04-11)
- [x] **Phase 92: Bootstrap Wiring and Daemon HTTP Routes** - Wire search ports into AppDeps and expose /search endpoints in uc-daemon with per-handler unlock guard and WS rebuild events (completed 2026-04-11)
- [ ] **Phase 93: Frontend Search UI** - Replace QuickPanel client-side filter and reveal Dashboard search with content-type/time-range controls

## Phase Details

### Phase 88: Core Domain and Port Contracts

**Goal**: The compiler-enforced search contract exists in uc-core so all downstream crates can reference search types and traits without any implementation yet
**Depends on**: Phase 87 (OTLP migration, v0.4.0 complete)
**Requirements**: (foundation for all v0.5.0 requirements; no individual requirement is fully satisfied here — this phase enables all downstream phases)
**Success Criteria** (what must be TRUE):

1. SearchIndexPort and SearchKeyDerivationPort traits compile in uc-core with no downstream implementations required
2. SearchKey newtype exists and exposes no raw bytes — the only way to produce a term tag is through the port interface
3. SearchQuery domain model encodes AND/OR operator, time range, content-type multi-select, and file-extension filter fields
4. SearchResult domain model carries entry_id and metadata sufficient for the UI to render a result row without additional queries
5. uc-app, uc-infra, and uc-daemon all compile after these additions with no broken imports

**Plans**: 1 plan

- [x] 88-01-PLAN.md — Define SearchKey, SearchQuery, SearchResult, SearchDocument, SearchError, RebuildProgress domain types and SearchIndexPort/SearchKeyDerivationPort traits in uc-core (completed 2026-04-10)

### Phase 89: Use Cases and Delete Integration

**Goal**: All four search use cases exist in uc-app and DeleteClipboardEntry synchronously cleans up search index entries as part of its delete chain
**Depends on**: Phase 88
**Requirements**: SIDX-01, SIDX-02
**Success Criteria** (what must be TRUE):

1. IndexClipboardEntry use case exists and can be exercised with a mock SearchIndexPort — unit test passes
2. RemoveIndexedEntry use case exists and can be exercised with a mock SearchIndexPort — unit test passes
3. SearchClipboardEntries use case accepts a SearchQuery and returns a SearchResult list via mock port
4. RebuildSearchIndex use case orchestrates full rebuild via mock port
5. DeleteClipboardEntry with an injected SearchIndexPort calls remove_entry synchronously before returning — verified by unit test with a spy mock

**Plans**: 2 plans

- [x] 89-01-PLAN.md — Create IndexClipboardEntry, RemoveIndexedEntry, SearchClipboardEntries, RebuildSearchIndex use cases in uc-app/src/usecases/search/ (SIDX-01)
- [x] 89-02-PLAN.md — Extend DeleteClipboardEntry with optional SearchIndexPort and synchronous search index cleanup (SIDX-02)

### Phase 90: SQLite Schema Migration and Tokenizer Pipeline

**Goal**: The search database schema exists on disk and the tokenizer pipeline correctly converts raw text into HMAC-tagged index terms ready for storage, with the key derivation mechanism fully implemented
**Depends on**: Phase 89
**Requirements**: SIDX-03, SIDX-04, SIDX-05, SIDX-06, SIDX-07
**Success Criteria** (what must be TRUE):

1. Diesel migration creates search_document, search_posting, and search_index_meta tables each with a profile_id column — migration runs cleanly on a fresh database
2. Tokenizer correctly splits Latin text at word boundaries and generates bigrams for CJK input — unit test covers both paths
3. Text extractor pulls content from plain text, HTML (tags stripped), URLs, file paths, and file names — unit test covers each content type
4. Search key derivation produces an HMAC-ready key from master key using a profile-scoped context — no raw MasterKey bytes appear in any HMAC call
5. index_version field exists in search_index_meta and is readable by the adapter layer

**Plans**: TBD

**Research flag**: Key derivation mechanism (blake3::derive_key vs HKDF-SHA256) must be resolved before implementation begins — read docs/architecture/local-encrypted-search.md key derivation section.

### Phase 91: SQLite Index Adapter and Rebuild Strategy

**Goal**: SqliteSearchIndex fully implements SearchIndexPort against the live SQLite schema, and the version-flag atomic swap rebuild strategy works without taking an exclusive lock that blocks other readers
**Depends on**: Phase 90
**Requirements**: REBLD-01, REBLD-02, REBLD-03
**Success Criteria** (what must be TRUE):

1. SqliteSearchIndex.search() executes AND queries (all terms must match) and OR queries (any term matches) correctly against real SQLite — integration test covers both modes
2. Full rebuild uses version-flag strategy in search_index_meta rather than RENAME TABLE — no exclusive lock timeout observed under concurrent read load
3. New entries captured during a rebuild window are double-written to both active and temp tables — integration test inserts an entry mid-rebuild and confirms it appears in results after swap
4. search_blocked flag prevents queries from returning stale results when a version mismatch is detected

**Plans**: TBD

**Research flag**: Review actual busy_timeout value in uc-infra/src/db/pool.rs and pool concurrency before committing to swap strategy details.

### Phase 92: Bootstrap Wiring and Daemon HTTP Routes

**Goal**: The full search backend is reachable end-to-end — capture triggers indexing, /search/query returns results filtered by keyword/type/time, /search/rebuild triggers rebuild with WS progress events, and all routes return 423 when the session is locked
**Depends on**: Phase 91
**Requirements**: SQRY-01, SQRY-02, SQRY-03, SQRY-04, SQRY-05, SQRY-06, REBLD-04
**Success Criteria** (what must be TRUE):

1. Capturing a clipboard entry in unlocked state results in the entry being findable via GET /search/query with the appropriate keyword — verified end-to-end without UI
2. Deleting a clipboard entry results in zero search_posting rows for that entry_id — verified with direct database inspection after delete
3. POST /search/rebuild triggers a background rebuild and emits WebSocket events with at least a start event and a complete event observable from a connected client
4. All three search routes (/search/query, /search/rebuild, /search/status) return HTTP 423 when called with a valid JWT but a locked encryption session
5. Mixed AND/OR query returns a structured invalid_query error response, not 500

**Plans**: TBD

**Research flag**: Read DaemonApiEventEmitter usage in file sync worker before writing rebuild WS progress events to ensure consistent event field names and discriminator with existing frontend handlers.

### Phase 93: Frontend Search UI

**Goal**: Users can search clipboard history from QuickPanel with server-side HMAC matching, and from Dashboard with full content-type and time-range filters, with debounced input and proper empty/locked states
**Depends on**: Phase 92
**Requirements**: SUI-01, SUI-02, SUI-03, SUI-04, SUI-05, SUI-06, SUI-07
**Success Criteria** (what must be TRUE):

1. Typing in QuickPanel search sends a debounced (200-300ms) request to /search/query and renders results using ClipboardItemRow — client-side substring filter is removed
2. Dashboard search input with content-type filter pills and time-range preset selector are visible and functional — the currently-hidden Header component is revealed
3. Search results display a total count alongside the result list (example: "12 results")
4. Locking the encryption session while search is open shows an unlock prompt instead of results — no partial or stale results appear
5. Search input auto-focuses when QuickPanel opens; results support keyboard navigation; empty state and no-results state show distinct, actionable messages

**Plans**: TBD
**UI hint**: yes

## Progress

| Phase                                              | Plans Complete | Status      | Completed |
| -------------------------------------------------- | -------------- | ----------- | --------- |
| 88. Core Domain and Port Contracts                 | 0/1            | Not started | -         |
| 89. Use Cases and Delete Integration               | 2/2 | Complete    | 2026-04-10 |
| 90. SQLite Schema Migration and Tokenizer Pipeline | 2/2 | Complete    | 2026-04-11 |
| 91. SQLite Index Adapter and Rebuild Strategy      | 2/2 | Complete   | 2026-04-11 |
| 92. Bootstrap Wiring and Daemon HTTP Routes        | 4/4 | Complete   | 2026-04-11 |
| 93. Frontend Search UI                             | 0/TBD          | Not started | -         |
