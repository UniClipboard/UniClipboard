# Phase 16: Optimize DashboardPage Refresh Mechanism on New Clipboard Content - Context

**Gathered:** 2026-03-08
**Status:** Ready for planning

<domain>
## Phase Boundary

Optimize the DashboardPage refresh mechanism when new clipboard content arrives. Replace the current full-reload pattern (event → re-fetch entire list from DB) with incremental updates for local captures and throttled full-reload for remote sync. Extract event/state management into a dedicated custom hook. Does NOT add new UI features, new event types beyond origin field, or change the clipboard capture pipeline.

</domain>

<decisions>
## Implementation Decisions

### Update strategy

- Local capture events: lightweight event + single-entry query via new `get_clipboard_entry(entry_id)` command → prepend to list
- Remote sync events: throttle (300ms) then full-reload (batch arrivals make per-item queries inefficient)
- Deleted events: incremental removal from Redux store (no re-query)
- Filtered view (e.g., "Favorites only"): silently ignore new content that doesn't match current filter — visible when user switches back to "All"
- Single-entry query failure fallback: silently fall back to existing `fetchClipboardItems` full-reload

### Event architecture

- Event payload stays lightweight: `{type, entry_id, preview}` — no DTO expansion
- Add `origin` field to `NewContent` event: `origin: "local" | "remote"` — frontend routes to different update paths based on origin
- Backend emits origin based on `ClipboardChangeOrigin` (LocalCapture/LocalRestore → "local", remote sync → "remote")
- Unified 300ms throttle window for both local and remote (reduced from current 500ms)
- Encryption not-ready: continue ignoring events (current behavior preserved)

### Backend commands

- New command: `get_clipboard_entry(entry_id)` — returns single entry projection matching `get_clipboard_entries` item shape
- Audit `get_clipboard_item` command — if unused by frontend, remove it as cleanup

### State management

- Keep existing Redux thunk pattern — no RTK Query migration
- New reducer actions: `prependItem` (insert at head) and `removeItem` (remove by entry_id)
- `prependItem` checks for duplicate entry_id before inserting (O(n) scan, acceptable for typical list sizes)
- Offset tracking: prepend adjusts offset +1, remove adjusts offset -1, to keep infinite scroll cursor consistent

### Hook design

- Extract single `useClipboardEvents` custom hook encapsulating:
  - Clipboard event listener (`clipboard://event`)
  - Encryption state listener (`encryption://event`) with `isReady` state
  - Initial data load (first fetch)
  - Throttle logic (300ms, trailing)
  - Incremental update dispatching (local → single query + prepend, remote → throttled full reload)
  - Infinite scroll offset management
- Hook does NOT include: delete operations (user-initiated, stays in DashboardPage), favorite toggle, or UI state (modals, confirmations)
- DashboardPage becomes primarily a render layer consuming hook outputs

### Scroll behavior

- New items prepended without affecting scroll position — user stays where they are
- No auto-scroll to top, no "new content" indicator (keep simple)

### Testing

- Backend: cargo test for `get_clipboard_entry` command (uc-tauri integration test)
- Frontend: Vitest tests for reducer actions (`prependItem`, `removeItem` with dedup and offset adjustment) and `useClipboardEvents` hook logic

### Claude's Discretion

- Exact `useClipboardEvents` hook return type and internal state shape
- Whether throttle utility is inline or extracted to a helper
- Exact cleanup of `get_clipboard_item` (depends on usage audit results)
- Test file organization and naming

</decisions>

<specifics>
## Specific Ideas

- Current 500ms throttle with trailing was introduced in commit e1b57ec6 — this phase replaces it with origin-aware routing (local → incremental, remote → throttled full reload) at 300ms
- The `globalListenerState` pattern in DashboardPage with multiple useRefs should be fully replaced by the new hook
- Dedup check in `prependItem` reducer prevents duplicates from event replay or race conditions between single-query and full-reload paths

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `clipboardSlice.ts` (`src/store/slices/clipboardSlice.ts`): Existing Redux slice with `fetchClipboardItems` thunk — add new `prependItem`/`removeItem` reducers here
- `ClipboardEvent` type (`src/types/events.ts`): Extend with optional `origin` field
- `forward_clipboard_event` (`src-tauri/crates/uc-tauri/src/events/mod.rs`): Modify `ClipboardEvent::NewContent` variant to include `origin` field
- `getClipboardItems` API (`src/api/clipboardItems.ts`): Reference for DTO shape that `get_clipboard_entry` should match
- `list_entry_projections` use case: Reusable for single-entry query with entry_id filter

### Established Patterns

- Tauri commands follow `runtime.usecases().xxx()` accessor pattern (Phase 10 decision)
- Commands return DTOs, not domain models (Phase 11 decision)
- Event types use serde `tag = "type"` convention
- Redux slice pattern with createAsyncThunk for async operations

### Integration Points

- `DashboardPage.tsx` (src/pages/DashboardPage.tsx): Main refactor target — extract logic into `useClipboardEvents` hook
- `AppRuntime::on_clipboard_changed` (runtime.rs ~line 978): Add origin to emitted `ClipboardEvent::NewContent`
- `sync_inbound.rs`: Add origin "remote" to emitted clipboard events from inbound sync
- `clipboard.rs` commands: Add new `get_clipboard_entry` command
- `main.rs` invoke_handler: Register new command

</code_context>

<deferred>
## Deferred Ideas

- Virtual scrolling / windowed rendering for very large lists — separate performance phase
- "N new items" indicator banner (like Twitter) — future UX enhancement
- RTK Query migration for clipboard data — separate architecture phase

</deferred>

---

_Phase: 16-optimize-dashboardpage-refresh-mechanism-on-new-clipboard-content_
_Context gathered: 2026-03-08_
