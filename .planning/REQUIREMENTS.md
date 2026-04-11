# Requirements: UniClipboard Desktop

**Defined:** 2026-04-10
**Core Value:** Seamless clipboard synchronization across devices — copy on one, paste on another

## v0.5.0 Requirements

Requirements for Local Encrypted Search milestone. Each maps to roadmap phases.

### Search Index Core

- [x] **SIDX-01**: User's clipboard entries are automatically indexed when captured in unlocked state
- [x] **SIDX-02**: Deleting a clipboard entry synchronously removes its search_document and all search_posting rows
- [x] **SIDX-03**: Index terms are stored as HMAC(search_key, normalized_token) — no plaintext tokens written to disk
- [x] **SIDX-04**: search_key is derived from master key via HKDF, scoped per profile
- [x] **SIDX-05**: Text is extracted from plain text, HTML, URL, file paths, and file names for indexing
- [x] **SIDX-06**: Tokenization uses word-boundary splitting for Latin text and bigram generation for CJK text
- [x] **SIDX-07**: Index schema includes index_version field to enable safe full rebuild when normalization rules change

### Search Query API

- [x] **SQRY-01**: User can search clipboard history with exact keyword matching using AND/OR boolean operators
- [x] **SQRY-02**: User can filter search results by time range (presets: today/yesterday/last_24h/last_7d/last_30d/this_week/this_month; or absolute from_ms/to_ms)
- [x] **SQRY-03**: User can filter search results by content type (multi-select: text/html/link/file/image/other)
- [x] **SQRY-04**: User can filter search results by file extension as a first-class filter
- [x] **SQRY-05**: Search routes validate session unlock state; locked session returns 423 Locked
- [x] **SQRY-06**: Mixing AND and OR operators in a single query returns a structured invalid_query error

### Index Rebuild

- [x] **REBLD-01**: User can trigger a full index rebuild when the encryption session is unlocked
- [x] **REBLD-02**: Full rebuild uses version-flag atomic swap strategy (not RENAME TABLE) to avoid exclusive lock contention
- [x] **REBLD-03**: New entries captured during a rebuild window are double-written to both active and temp tables
- [x] **REBLD-04**: Rebuild progress is broadcast to frontend via WebSocket events

### Search UI

- [ ] **SUI-01**: QuickPanel search input invokes backend HMAC search, replacing current client-side substring filter
- [ ] **SUI-02**: Dashboard exposes search input + content-type filter pills + time range preset selector
- [ ] **SUI-03**: Search results reuse existing ClipboardItemRow component for rendering
- [ ] **SUI-04**: Search input is debounced 200–300ms; stale in-flight requests are cancelled on new input
- [ ] **SUI-05**: Search input auto-focuses on open; results support keyboard navigation
- [ ] **SUI-06**: Empty state and no-results state show distinct, actionable messages
- [ ] **SUI-07**: Total result count is displayed alongside search results

## V2 Requirements

Deferred to future releases. Tracked but not in current roadmap.

### Search Enhancements

- **SRCH-V2-01**: Phrase search (exact multi-word sequence matching)
- **SRCH-V2-02**: Match highlighting in result rows (requires position information — blocked by HMAC architecture in V1)
- **SRCH-V2-03**: Short-form / partial-word search (fuzzy or prefix matching)
- **SRCH-V2-04**: Semantic / vector search

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|---------|
| Fuzzy / partial word matching | Requires different index structure; V1 architecture decision to keep exact-token HMAC |
| Semantic search | High complexity, no local model infrastructure |
| Match highlighting | HMAC returns only entry IDs, not token positions — V1 architectural constraint |
| NOT query operator | Excluded from V1 query grammar by architecture spec |
| Nested boolean expressions (parentheses) | Excluded from V1 query grammar; mixed AND/OR returns invalid_query |
| Search in locked state | Security design decision — daemon does not capture in locked state |
| Remote / SSE trapdoor search | No server-side query executor in current architecture |
| Phrase search | Deferred to V2 |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase    | Status  |
|-------------|----------|---------|
| SIDX-01     | Phase 89 | Complete |
| SIDX-02     | Phase 89 | Complete |
| SIDX-03     | Phase 90 | Complete |
| SIDX-04     | Phase 90 | Complete |
| SIDX-05     | Phase 90 | Complete |
| SIDX-06     | Phase 90 | Complete |
| SIDX-07     | Phase 90 | Complete |
| SQRY-01     | Phase 92 | Complete |
| SQRY-02     | Phase 92 | Complete |
| SQRY-03     | Phase 92 | Complete |
| SQRY-04     | Phase 92 | Complete |
| SQRY-05     | Phase 92 | Complete |
| SQRY-06     | Phase 92 | Complete |
| REBLD-01    | Phase 91 | Complete |
| REBLD-02    | Phase 91 | Complete |
| REBLD-03    | Phase 91 | Complete |
| REBLD-04    | Phase 92 | Complete |
| SUI-01      | Phase 93 | Pending |
| SUI-02      | Phase 93 | Pending |
| SUI-03      | Phase 93 | Pending |
| SUI-04      | Phase 93 | Pending |
| SUI-05      | Phase 93 | Pending |
| SUI-06      | Phase 93 | Pending |
| SUI-07      | Phase 93 | Pending |

**Coverage:**

- v0.5.0 requirements: 22 total
- Mapped to phases: 22
- Unmapped: 0 ✓

---

_Requirements defined: 2026-04-10_
_Last updated: 2026-04-10 after roadmap creation (Phases 88-93)_
