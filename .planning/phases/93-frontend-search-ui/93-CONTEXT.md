# Phase 93: Frontend Search UI - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Replace QuickPanel's client-side substring filter with debounced server-side HMAC search, and reveal the Dashboard Header with search input, content-type filter pills, and time-range preset selector. Both surfaces render results using the existing `ClipboardItemRow` component, display a total result count, handle locked-session state with an unlock prompt, and present distinct empty vs. no-results states. No new backend capabilities — this phase is pure frontend wiring to the already-built `/search/query`, `/search/status`, and `/search/rebuild` daemon endpoints.

</domain>

<decisions>
## Implementation Decisions

### UX Change Communication

- **D-01:** Update placeholder text only. QuickPanel placeholder changes from "Search clipboard history..." to something that implies keyword/word-boundary search (e.g., "Search by keywords..."). No tooltip, modal, or first-run notification. Users will adapt to exact-token behavior through natural use.

### Dashboard Header Layout

- **D-02:** Two-row layout. Row 1: full-width search text input on the left + time-range dropdown on the right. Row 2: existing content-type filter pills (All/Text/Image/Link/File/Code) — already implemented in `Header.tsx`, kept as-is.
- **D-03:** The time-range dropdown shows the currently selected preset as its label (default: "All time"). Click expands a list of presets: All time / Today / Yesterday / Last 7 days / Last 30 days / This week / This month. Implemented using the existing Shadcn/ui `Select` or `DropdownMenu` primitive — no custom dropdown.
- **D-04:** The `Header` component currently has `className="hidden"` in `DashboardPage.tsx`. Phase 93 removes the hidden class to reveal it. The Header gains the search input + time-range selector in a new top row.

### Dashboard Search vs. Browse Mode

- **D-05:** Seamless replacement — no separate "search mode" view. When the Dashboard search input is empty, `ClipboardContent` receives no search query and shows the normal browsing list (paginated, recency-sorted). When the user types, search results replace the list in place. Clearing the input reverts to browsing. No overlay, no separate component mount.
- **D-06:** Content-type filter pills apply in both browse mode and search mode. When a query is active, the content-type pill selection is passed as a `fileTypes` filter to `/search/query`. Time-range filter applies only in search mode (ignored during browse).

### QuickPanel Search Migration

- **D-07:** Replace the client-side `filteredItems` memo (lines 467–471 in `ClipboardHistoryPanel.tsx`) with a debounced server-side call to `GET /search/query`. When `searchQuery` is empty, fall back to the existing `useClipboardCollection` items (normal list). When non-empty, call the search endpoint.
- **D-08:** Debounce: 200–300ms. Use `AbortController` to cancel in-flight requests on new input (satisfies SUI-04 stale-request cancellation).
- **D-09:** During search loading in QuickPanel, show a spinner or the last results until new results arrive — do not flash an empty state while debouncing.

### Result Count Display

- **D-10:** When a search query is active (in both QuickPanel and Dashboard), display the total result count inline in or near the search input — e.g., "12 results" as a muted label to the right of the input. When no query, hide the count (same as current behavior). This satisfies SUI-07.

### Locked State

- **D-11:** QuickPanel already has a locked-state UI (Lock icon + "Unlock" button). No changes needed there beyond ensuring the HMAC search path also triggers the locked state when the daemon returns HTTP 423. If the session locks while a query is in-flight, cancel the request and show the locked state.
- **D-12:** Dashboard locked state: when `/search/query` returns 423, show a locked indicator (lock icon + unlock prompt) instead of search results. If the user unlocks, the search query should re-fire automatically.

### Empty and No-Results States

- **D-13:** Distinct messages per SUI-06:
  - **Empty state** (no query entered, no items in history): "Your clipboard history is empty" with an actionable sub-message (e.g., "Copy something to get started").
  - **No-results state** (query entered, zero results): "No results for '[query]'" with a sub-message suggesting exact words (e.g., "Try a full word or a different keyword").

### Claude's Discretion

- Exact Shadcn/ui component choice for the time-range dropdown (Select vs. DropdownMenu vs. Popover+Command)
- Exact spinner/skeleton implementation during search loading
- Exact CSS layout for two-row Header (padding, gap, widths)
- Debounce hook implementation (custom `useDebounce` vs. library)
- Abort controller wiring detail

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase Scope and Requirements

- `.planning/ROADMAP.md` §Phase 93 — Goal, success criteria, and "UI hint: yes" tag.
- `.planning/REQUIREMENTS.md` §SUI-01–§SUI-07 — All 7 search UI requirements, their acceptance criteria, and current pending status.

### Backend Contract (what Phase 93 calls)

- `src-tauri/crates/uc-daemon-contract/src/api/dto/search.rs` — `SearchResultDto`, `SearchQueryResponse`, `SearchStatusResponse` — the exact response shapes the frontend must parse.
- `.planning/phases/92-bootstrap-wiring-and-daemon-http-routes/92-CONTEXT.md` §Query Contract (D-01–D-03) — Query-string parameter shape, result envelope fields (`total`, `hasMore`), and error codes (`invalid_query`, `session_locked`, `index_not_ready`).

### Existing Frontend Files to Modify

- `src/quick-panel/ClipboardHistoryPanel.tsx` — Lines 467–471 (client-side filter memo to replace), search input rendering, locked state UI pattern to reuse.
- `src/pages/DashboardPage.tsx` — Line 27 (`className="hidden"` to remove from Header).
- `src/components/layout/Header.tsx` — Add search input row + time-range selector; keep existing filter pills in second row.
- `src/contexts/SearchContext.tsx` and `src/contexts/search-context.ts` — Extend to carry search query and active filters for Dashboard; QuickPanel manages its own local state.

### Reusable Components

- `src/components/clipboard/ClipboardItemRow.tsx` — Render each search result (SUI-03 requirement).
- `src/components/clipboard/ClipboardContent.tsx` — Integration point for Dashboard search results; currently receives `searchQuery` prop.

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `ClipboardItemRow.tsx`: Already renders a single clipboard entry row — SUI-03 says use this for search results, no new component needed.
- `Header.tsx`: Already has content-type filter pills with animated Framer Motion selection indicator. Phase 93 adds a new row above, keeping pills untouched.
- `ClipboardHistoryPanel.tsx` locked-state UI (lines 165–204): Full lock screen with unlock button already exists in QuickPanel. Reuse the same lock/unlock pattern for Dashboard locked state rather than reinventing.
- `SearchContext` (`search-context.ts`): Currently holds only `searchValue: string`. Needs extension to carry time-range preset and (possibly) content-type for Dashboard coordinated state. QuickPanel uses its own local state.
- `useClipboardCollection` hook: Used by QuickPanel to load items. The search path replaces this hook's results when a query is active — or wraps it with search-aware logic.

### Established Patterns

- Daemon API calls go through `src/api/daemon/` typed client modules (see `src/api/daemon/clipboard.ts` as reference). A new `src/api/daemon/search.ts` file follows the same pattern.
- Debounce + AbortController: No existing shared hook found — implement a `useDebounce` hook in `src/hooks/` following the project's camelCase hook convention.
- Error handling: `toast.error(t('...'))` for user-visible errors; `console.error(...)` for logging — consistent with existing patterns in `ClipboardHistoryPanel.tsx`.
- Tailwind + Shadcn/ui: Use `Select` or `DropdownMenu` from `src/components/ui/` for the time-range selector rather than a native `<select>`.

### Integration Points

- `src/api/daemon/search.ts` (new) → calls `GET /search/query` with typed params; returns `SearchQueryResponse`.
- `ClipboardHistoryPanel.tsx` → replace `filteredItems` memo with search hook; wire debounce + abort.
- `DashboardPage.tsx` → remove `className="hidden"` from Header; wire `searchQuery` + `fileTypes` + `timeRange` state into Header props + ClipboardContent props.
- `Header.tsx` → add search text input + time-range `Select`; lift state up to DashboardPage or extend `SearchContext`.
- `SearchContext` → extend type to include `timeRange` and `contentTypeFilter` if Dashboard needs shared state across Header and ClipboardContent.

</code_context>

<specifics>
## Specific Ideas

- Header two-row layout confirmed: Row 1 = `[🔍 Search by keywords...] [All time ▼]`, Row 2 = `[All] [Text] [Image] [Link] [File] [Code]`.
- Time-range dropdown with preset list (not pills), default "All time".
- Placeholder text: "Search by keywords..." (communicates exact-word search without a tooltip).
- No-results message should include the query text: `No results for "clipboard"`.

</specifics>

<deferred>
## Deferred Ideas

- Absolute `from_ms`/`to_ms` custom date range picker — REQUIREMENTS.md lists it but Dashboard V1 uses presets only; absolute range can be a follow-up.
- Match highlighting in result rows (SRCH-V2-02) — blocked by HMAC architecture; V2 only.
- Rebuild progress indicator in Dashboard — could surface `/search/status` rebuilding state; not in Phase 93 scope.

### Reviewed Todos (not folded)

- `修复 setup 配对确认提示缺失` — unrelated to search UI; kept out of Phase 93 scope.

</deferred>

---

*Phase: 93-frontend-search-ui*
*Context gathered: 2026-04-11*
