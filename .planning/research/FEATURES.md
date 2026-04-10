# Feature Research

**Domain:** Local encrypted clipboard history search
**Researched:** 2026-04-10
**Confidence:** HIGH (competitors verified via live docs + existing codebase examined)

---

## Context: What Is Already Built

The existing app has a working clipboard history list in two surfaces:

- **Dashboard** (`DashboardPage.tsx`): Full sidebar list with `ClipboardItemRow` — shows type icon, 80-char text preview, relative timestamp. A `Header` component with filter controls exists but is hidden (`className="hidden"`). `SearchContext` holds a bare string.
- **QuickPanel** (`ClipboardHistoryPanel.tsx`): Floating panel with an inline search input. Currently does **client-side substring filtering** (`item.preview.toLowerCase().includes(q)`). Shows type icon, truncated preview, relative time, keyboard shortcuts (`⌘1–0`). Has a preview pane that expands on hover/selection.

Search V1 replaces the QuickPanel client-side filter with server-side HMAC exact-match. This is a **behavior change** users will notice: substring mid-word matching (`insta` matching `instagram`) stops working. Plan for it.

---

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist in any clipboard history search. Missing any of these makes the feature feel unfinished.

| Feature | Why Expected | Complexity | Notes |
| --- | --- | --- | --- |
| Search input auto-focused on open | Alfred, Raycast both open with cursor in search field — users immediately type | LOW | QuickPanel already does this; Dashboard Header needs it when search is revealed |
| Instant result update while typing (debounced) | Users expect live filtering; no separate submit button | MEDIUM | With HMAC index, each token requires HMAC derivation. Debounce to 200–300 ms; do not fire on every keystroke |
| Result count shown alongside query | Users need to see "3 results" vs "0 results" to know if query worked | LOW | QuickPanel already shows count when query is non-empty; Dashboard needs same |
| Meaningful empty state | "No matches" with suggestion to relax the query; not a blank panel | LOW | QuickPanel shows "No matches"; Dashboard needs guidance text explaining what to try |
| Content-type filter (text / link / image / file) | Raycast and Alfred both offer type filters; users copy many types and want to narrow | MEDIUM | Arch spec has `fileTypes` as structured param. Existing `Filter` enum in `ClipboardContent` partially covers this. Dashboard `Header` `onFilterChange` is the natural attachment point |
| Time range filter presets (today / last 7d / last 30d) | Users recall roughly when they copied something; presets cover 90% of cases | LOW | Arch spec defines: `today`, `yesterday`, `last_24h`, `last_7d`, `last_30d`, `this_week`, `this_month`. Render as a select or pill group |
| Keyboard navigation of results | Arrow keys + Enter to paste; Esc to dismiss — table stakes for keyboard-first tools | LOW | QuickPanel already has this. Dashboard needs keyboard nav wired to search results when search is active |
| Clear search input | Users need to reset without selecting all and deleting | LOW | Standard UX; a clear button (×) on the input |
| Result items reuse existing ClipboardItemRow display | Consistent row format — icon, preview, timestamp — no visual disjunction between search and non-search views | LOW | HIGH dependency. Do not invent a new row component. `ClipboardItemRow` already renders what is needed |

### Differentiators (Competitive Advantage)

Features the architecture makes possible that competitors lack or charge extra for.

| Feature | Value Proposition | Complexity | Notes |
| --- | --- | --- | --- |
| Encrypted-at-rest search index | Clipboard content (including search terms) never stored as plaintext on disk — unique in the category | HIGH (backend, already designed) | HMAC-keyed inverted index is the architectural centerpiece. The UX implication: users should see a visible indicator that search is private ("searching encrypted history") — small but trust-building |
| Boolean AND / OR queries | Power users searching for entries matching multiple terms — rare in clipboard managers | MEDIUM | Arch spec supports `foo AND bar` and `foo OR bar`. Mixed AND+OR returns `invalid_query` error. Surface this limitation clearly in UI (e.g., tooltip on syntax help icon) |
| File extension filtering | Filter by `.pdf`, `.md`, `.png` — distinct from content-type filter | MEDIUM | First-class filter per arch spec (`extensions: ["md", "txt"]`). Useful for developers who frequently copy file paths. Render as a tag-input or multi-select |
| Cross-type text extraction | URLs indexed by host/path/query, HTML by stripped text, files by name/path — all searchable with one query | HIGH (backend) | UX: users do not need to know the internals. Document search scope in placeholder: "Search text, links, file names…" |
| Index-backed speed | Results fast regardless of history size — no scanning decrypted blobs on query | MEDIUM (backend) | UX: no loading spinner for typical queries. Show skeleton only if daemon round-trip exceeds ~200 ms |

### Anti-Features (Avoid These)

| Feature | Why Requested | Why Problematic | Alternative |
| --- | --- | --- | --- |
| Fuzzy / typo-tolerant search | Users misspell; competitors sometimes offer it | Impossible with HMAC index: HMAC(`search_key`, `insagram`) ≠ HMAC(`search_key`, `instagram`). Adding fuzzy requires storing plaintext or a bloom filter, breaking the security model | Accept exact-token match in V1; document it clearly. Revisit in V3 only if security model allows |
| Highlight matching terms in result rows | Alfred and Raycast highlight matched text in result rows | HMAC returns entry IDs, not positions. The backend cannot return "match was at offset 12." A frontend re-scan would need to decrypt every visible result row on every keystroke. Arch spec explicitly defers this (line 665–666 of arch doc) | Use existing row format unchanged; rely on match count and correct result ordering. Revisit in V2 with a position-aware index extension |
| Semantic / embedding search | Users want to find "that email about the meeting" without remembering keywords | HMAC architecture cannot support embedding-based retrieval without storing embeddings as plaintext, which violates the security model | Exact keyword + AND/OR covers most practical clipboard lookup; defer semantic to V3 or never |
| NOT queries | "text but NOT password" | Requires scanning all documents in complement set; inverted index AND/OR covers most cases | Out of scope V1 per arch spec |
| Nested boolean / parentheses | Complex queries like `(foo AND bar) OR baz` | Parser complexity, rare clipboard use case, arch spec explicitly excludes | V2 consideration |
| Real-time search on every keystroke (no debounce) | Users expect instant feedback | Each keystroke triggers HMAC derivation for each query token, then a SQLite query. Without debounce, overlapping requests arrive out-of-order and UI flickers | Debounce at 200–300 ms. Show a spinner indicator while result is pending |
| Custom date range picker UI | Users want precise date control | Date range pickers are complex UI components (calendar widget, validation, timezone handling). Presets cover >90% of real search intent for clipboard | Ship presets only in V1; expose `from_ms`/`to_ms` at API level for future use. Defer date-picker UI to V1.x |
| Search in locked state | Accessing any clipboard content while locked | Arch spec explicitly disallows: daemon does not capture while locked, search key not available while locked | Gate the search UI entirely behind the unlock state — show the existing lock screen; no partial results |

---

## Feature Dependencies

```
Search Input (query string)
└──requires──> Encryption session unlocked (daemon-side gate)
└──requires──> HMAC search key derived from session master key
└──requires──> search_document + search_posting tables populated (incremental index)

Content-Type Filter
└──requires──> search_document.file_type field populated at index time
└──enhances──> Search Input (structural parameter, not free text)
└──can coexist with──> Existing Dashboard Filter (Filter.All / Filter.Text etc.)
    NOTE: Dashboard already has a filter mechanism. Decide whether search filters
    replace or augment the existing content-type filter pills.

Time Range Filter
└──requires──> search_document.active_time_ms field populated at index time
└──enhances──> Search Input (structural parameter)

File Extension Filter
└──requires──> search_document.file_extensions[] field populated at index time
└──enhances──> Content-Type Filter (most useful when file_type = "file")

Boolean AND/OR
└──requires──> query parser on daemon side
└──conflicts_with──> Fuzzy search (incompatible with HMAC model)

Result display (ClipboardItemRow)
└──requires──> Existing ClipboardItemRow component (already built)
└──requires──> Search results return entry_ids that resolve via existing clipboard use cases
└──NOTE──> Do NOT build a new row component. Reuse ClipboardItemRow.

SearchContext (frontend)
└──currently holds──> bare searchValue: string
└──needs expansion to──> { query, fileTypes, timePreset, extensions }
    This is a frontend state model change, not a backend change.
```

### Dependency Notes

- **HMAC search returns entry IDs, existing use case loads rows**: The search pipeline output feeds directly into the same data path as the clipboard list. No new display component is needed — only the query mechanism changes.
- **QuickPanel client-side filter replaced by server-side HMAC**: Current `filteredItems` useMemo in `ClipboardHistoryPanel.tsx` does `includes(q)`. After V1, this becomes an async HMAC query. Users who relied on substring matching (partial URLs, mid-word) will notice exact-token semantics. Migration path: keep QuickPanel search query-only (no filter dropdowns) due to UI space constraints. Richer filters live in Dashboard.
- **Dashboard Header already wired for filter changes**: `onFilterChange` prop on `Header` component is connected but the Header is hidden. The hidden `<Header>` can be revealed and extended with a search input and filter controls. This is the right home for rich search UX.
- **SearchContext expansion**: Current context stores only `searchValue`. It will need structured fields for time range and file type when Dashboard search ships. QuickPanel can remain query-only and not share context state.

---

## MVP Definition

### Launch With (V1 — this milestone)

- [ ] Query input in QuickPanel — replaces client-side filter with HMAC-backed exact search
- [ ] Query input in Dashboard — reveals hidden `Header` with search wired to HMAC backend
- [ ] Result count shown next to query (e.g., "12 results")
- [ ] Empty state: "No matches" with hint to try fewer words or check spelling
- [ ] Content-type filter pills (text / link / image / file) — available in Dashboard
- [ ] Time range presets (today / last 7 days / last 30 days) — available in Dashboard
- [ ] Debounced query (200–300 ms) to avoid HMAC overload on every keystroke
- [ ] Locked state gate: show lock screen, no search available
- [ ] Results rendered using existing `ClipboardItemRow` — no new row UI

### Add After Validation (V1.x)

- [ ] File extension filter — add when users with developer workflows provide feedback
- [ ] Boolean AND/OR syntax hint in search placeholder or tooltip — add when power users request it
- [ ] Time range filter in QuickPanel — space is limited; defer until QuickPanel layout allows

### Future Consideration (V2+)

- [ ] Term highlighting in result rows — requires position-aware index extension; security review needed
- [ ] Custom absolute date range picker (from/to) — API already supports it; build UI when presets prove insufficient
- [ ] Phrase search — requires positional postings; explicitly deferred in arch spec
- [ ] Semantic / embedding search — different security model needed; out of scope for HMAC architecture

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
| --- | --- | --- | --- |
| Query input (QuickPanel) | HIGH | MEDIUM | P1 |
| Query input (Dashboard) | HIGH | LOW (Header already exists, unhide it) | P1 |
| Result count | MEDIUM | LOW | P1 |
| Meaningful empty state | MEDIUM | LOW | P1 |
| Content-type filter (Dashboard) | HIGH | LOW (Filter enum exists) | P1 |
| Time range presets (Dashboard) | HIGH | LOW | P1 |
| Debounce on query | HIGH | LOW | P1 |
| Locked state gate | HIGH | LOW (lock screen already built) | P1 |
| Reuse ClipboardItemRow | HIGH | LOW (no new code) | P1 |
| File extension filter | MEDIUM | MEDIUM | P2 |
| Boolean AND/OR syntax hint | MEDIUM | LOW | P2 |
| Time range filter (QuickPanel) | LOW | MEDIUM | P3 |
| Term highlighting | HIGH | HIGH + security review | P3 |
| Custom date range picker | LOW | HIGH | P3 |

**Priority key:**
- P1: Must have for launch
- P2: Should have, add when possible
- P3: Nice to have, future consideration

---

## Search Result Display Requirements

What each result row must show, based on existing `ClipboardItemRow` and competitor analysis:

| Field | Source | Display Format |
| --- | --- | --- |
| Type icon | `item.type` | Existing icons (FileText, ExternalLink, Image, Code, File) |
| Content preview | Decrypted content, first 80 chars | Truncated text, same as current `getPreviewText()` |
| Relative timestamp | `active_time_ms` (per arch spec — primary time axis) | e.g., "2 hours ago", "Yesterday" |
| Transfer status badges | Existing (for file items) | Reuse existing Loader2 / AlertCircle / Clock badges |
| Match count (aggregate) | Count of matched entries | Shown in search input area, not per-row |

**What NOT to show per row (V1):**
- Highlighted match positions — not available from HMAC index (see anti-features)
- Which field was matched (body vs URL vs filename) — `field_mask` available in index but not needed for V1 UX
- Relevance score — sort by `active_time_ms` desc is sufficient

**Row ordering (per arch spec):**
1. `active_time_ms` descending (most recently used first)
2. Hit term count descending (more terms matched = ranked higher)
3. `captured_at_ms` descending (tiebreaker)

---

## UX Surface Decision (Open — Not in Scope of This Research)

The arch spec says search UI lives in "dashboard or quick-panel" — both surfaces are affected.

Key UX differences between surfaces:

| Concern | QuickPanel | Dashboard |
| --- | --- | --- |
| Space for filter controls | Minimal (no room for dropdowns) | Full (sidebar layout, collapsible filter panel) |
| Primary interaction model | Keyboard-first, dismiss on paste | Mouse + keyboard, stay open |
| Current search behavior | Client-side substring (to be replaced) | Hidden header (to be revealed) |
| Filter complexity | Query only (V1) | Query + content type + time range (V1) |

Recommendation: ship query-only in QuickPanel, full filter set in Dashboard. They share the same backend API but different frontend state management.

---

## Competitor Feature Analysis

| Feature | Alfred | Raycast | Maccy (open source) | Our Approach |
| --- | --- | --- | --- | --- |
| Search input | Text filter on open | Text filter on open | Text filter on open | Same — auto-focus input on open |
| Content type filter | No explicit type filter | Yes — filter by type button | No | Yes — content-type pill filter |
| Time range filter | Retention dropdown only, not search filter | No search-time filter | No | Yes — presets (today / last 7d / last 30d) |
| Result row: icon | Yes | Yes | Yes | Yes (existing `typeIcons`) |
| Result row: preview | Yes, truncated | Yes, truncated | Yes, truncated | Yes (existing `getPreviewText()`) |
| Result row: timestamp | No | Yes | No | Yes (existing `item.time`) |
| Result row: match highlight | Yes (bold matched term) | Yes | No | No in V1 — HMAC architecture prevents it without additional work |
| Keyboard shortcuts for paste | Yes (⌘1–9) | Yes | Yes | Yes (existing ⌘1–0 in QuickPanel) |
| Encrypted search index | No | No | No | Yes — unique differentiator |
| Boolean operators | No | No | No | Yes (AND/OR) — differentiator |
| File extension filter | No | No | No | Yes — differentiator |

---

## Sources

- Alfred Clipboard History documentation: https://www.alfredapp.com/help/features/clipboard/
- Raycast Clipboard History manual: https://manual.raycast.com/windows/clipboard-history
- Raycast core features page: https://www.raycast.com/core-features/clipboard-history
- Maccy open source clipboard manager: https://github.com/p0deje/Maccy
- Pasta clipboard manager: https://getpasta.com/
- Architecture spec (primary reference): `docs/architecture/local-encrypted-search.md`
- Existing codebase: `src/quick-panel/ClipboardHistoryPanel.tsx`, `src/components/clipboard/ClipboardItemRow.tsx`, `src/pages/DashboardPage.tsx`

---

*Feature research for: Local Encrypted Clipboard Search (v0.5.0)*
*Researched: 2026-04-10*
