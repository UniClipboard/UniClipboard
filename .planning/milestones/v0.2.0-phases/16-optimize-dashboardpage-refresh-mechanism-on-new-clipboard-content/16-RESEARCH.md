# Phase 16: Optimize DashboardPage Refresh Mechanism - Research

**Researched:** 2026-03-08
**Domain:** Frontend event-driven state management + Tauri command layer extension
**Confidence:** HIGH

## Summary

Phase 16 replaces the DashboardPage's full-reload pattern (every clipboard event triggers re-fetch of the entire list from DB) with an incremental update strategy. Local captures use a new single-entry query (`get_clipboard_entry`) and prepend the result to the Redux store, while remote sync events continue to use throttled full-reload (batch arrivals make per-item queries inefficient). The scattered event/state logic in DashboardPage is extracted into a `useClipboardEvents` custom hook.

The implementation spans three layers: (1) backend -- add `origin` field to `ClipboardEvent::NewContent` and create a new `get_clipboard_entry` Tauri command; (2) Redux store -- add `prependItem` and `removeItem` reducers with dedup and offset tracking; (3) frontend -- extract `useClipboardEvents` hook from DashboardPage, routing local vs remote events to different update paths.

**Primary recommendation:** Implement in three sequential waves: backend event+command changes first, then Redux slice reducers, then hook extraction with DashboardPage simplification.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Local capture events: lightweight event + single-entry query via new `get_clipboard_entry(entry_id)` command, prepend to list
- Remote sync events: throttle (300ms) then full-reload (batch arrivals make per-item queries inefficient)
- Deleted events: incremental removal from Redux store (no re-query)
- Filtered view: silently ignore new content that doesn't match current filter
- Single-entry query failure fallback: silently fall back to existing `fetchClipboardItems` full-reload
- Event payload stays lightweight: `{type, entry_id, preview}` -- no DTO expansion
- Add `origin` field to `NewContent` event: `origin: "local" | "remote"`
- Backend emits origin based on `ClipboardChangeOrigin` (LocalCapture/LocalRestore -> "local", remote sync -> "remote")
- Unified 300ms throttle window for both local and remote (reduced from current 500ms)
- Encryption not-ready: continue ignoring events (current behavior preserved)
- New command: `get_clipboard_entry(entry_id)` -- returns single entry projection matching `get_clipboard_entries` item shape
- Audit `get_clipboard_item` command -- if unused by frontend, remove it as cleanup
- Keep existing Redux thunk pattern -- no RTK Query migration
- New reducer actions: `prependItem` (insert at head) and `removeItem` (remove by entry_id)
- `prependItem` checks for duplicate entry_id before inserting (O(n) scan acceptable)
- Offset tracking: prepend adjusts offset +1, remove adjusts offset -1
- Extract single `useClipboardEvents` custom hook encapsulating event listeners, encryption state, initial load, throttle, incremental dispatch, offset management
- Hook does NOT include: delete operations, favorite toggle, or UI state
- New items prepended without affecting scroll position
- No auto-scroll to top, no "new content" indicator

### Claude's Discretion

- Exact `useClipboardEvents` hook return type and internal state shape
- Whether throttle utility is inline or extracted to a helper
- Exact cleanup of `get_clipboard_item` (depends on usage audit results)
- Test file organization and naming

### Deferred Ideas (OUT OF SCOPE)

- Virtual scrolling / windowed rendering for very large lists
- "N new items" indicator banner (like Twitter)
- RTK Query migration for clipboard data
  </user_constraints>

## Standard Stack

### Core

| Library       | Version    | Purpose                   | Why Standard                                              |
| ------------- | ---------- | ------------------------- | --------------------------------------------------------- |
| Redux Toolkit | (existing) | State management          | Already in project, locked decision to keep thunk pattern |
| React 18      | (existing) | UI framework              | Project standard                                          |
| Tauri 2       | (existing) | Backend commands + events | Project standard                                          |
| Vitest        | ^4.0.17    | Frontend testing          | Already in devDependencies                                |

### Supporting

| Library         | Version    | Purpose                           | When to Use                         |
| --------------- | ---------- | --------------------------------- | ----------------------------------- |
| @tauri-apps/api | (existing) | `listen()` for event subscription | Clipboard and encryption events     |
| serde           | (existing) | Rust serialization                | ClipboardEvent serde tag convention |

### Alternatives Considered

| Instead of      | Could Use       | Tradeoff                                                 |
| --------------- | --------------- | -------------------------------------------------------- |
| Redux thunks    | RTK Query       | Better caching, but locked out of scope by user decision |
| Custom throttle | lodash throttle | Extra dependency; inline is fine for single use          |

## Architecture Patterns

### Backend: ClipboardEvent Origin Extension

The `ClipboardEvent::NewContent` enum variant in `src-tauri/crates/uc-tauri/src/events/mod.rs` needs an `origin` field added:

```rust
// Current
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClipboardEvent {
    NewContent { entry_id: String, preview: String },
    Deleted { entry_id: String },
}

// Target
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClipboardEvent {
    NewContent { entry_id: String, preview: String, origin: String },
    Deleted { entry_id: String },
}
```

**Three emission sites must be updated:**

1. **`runtime.rs` line ~1025** (`AppRuntime::on_clipboard_changed`): The `origin` variable is already computed from `ClipboardChangeOrigin` via `consume_origin_for_snapshot_or_default`. Map `LocalCapture`/`LocalRestore` -> `"local"`, `RemotePush` -> `"remote"`.

2. **`wiring.rs` line ~1602** (inbound sync loop): Already emits `ClipboardEvent::NewContent` after `InboundApplyOutcome::Applied`. Set `origin: "remote"`.

3. **`clipboard.rs` commands** (line ~560, `capture_clipboard` command): Emits after manual capture. Set `origin: "local"`.

### Backend: New `get_clipboard_entry` Command

The existing `get_clipboard_item` command (lines 196-283 in `clipboard.rs`) already does what we need -- it queries via `list_entry_projections` and filters by ID. However, it fetches ALL projections (limit 1000, offset 0) then filters client-side, which is wasteful.

**Better approach**: Create a new `get_clipboard_entry` command that reuses `ListClipboardEntryProjections` but uses the existing repository `get_entry` method to query a single entry, then builds the projection for just that entry. This avoids loading all entries.

**Important finding**: The `ListClipboardEntryProjections` use case does NOT support single-entry lookup. It always calls `list_entries(limit, offset)`. Two options:

1. Add a `execute_single(entry_id)` method to `ListClipboardEntryProjections` that reuses the same projection-building logic
2. Build the projection inline in the command using the same repos

Option 1 is cleaner -- it keeps projection logic in one place.

### Frontend: useClipboardEvents Hook

```
src/hooks/
├── useClipboardEvents.ts   # NEW - extracted from DashboardPage
├── useLifecycleStatus.ts   # existing
└── ...
```

**Hook responsibilities extracted from DashboardPage:**

- Clipboard event listener (`clipboard://event`)
- Encryption state listener (`encryption://event`) with `isReady` state
- Initial data load (first fetch on mount / filter change)
- Throttle logic (300ms, trailing)
- Origin-based routing: local -> single query + prepend, remote -> throttled full reload
- Infinite scroll offset management

**Hook return type (Claude's discretion):**

```typescript
interface UseClipboardEventsReturn {
  hasMore: boolean
  handleLoadMore: () => void
  encryptionReady: boolean
}
```

The hook dispatches Redux actions internally. DashboardPage reads `items`, `loading`, `notReady` from Redux store selectors as before.

### Redux: New Reducer Actions

```typescript
reducers: {
  // Existing
  setDeleteConfirmId, setNotReady, clearError,
  // New
  prependItem: (state, action: PayloadAction<ClipboardItemResponse>) => {
    // Dedup check
    if (state.items.some(item => item.id === action.payload.id)) return
    state.items.unshift(action.payload)
  },
  removeItem: (state, action: PayloadAction<string>) => {
    state.items = state.items.filter(item => item.id !== action.payload)
  },
}
```

Note: Offset tracking (+1/-1) is managed in the hook's `offsetRef`, not in Redux state. The Redux slice only manages the items array.

### Anti-Patterns to Avoid

- **Global mutable state (`globalListenerState`):** The current pattern uses module-level mutable state outside React. The new hook should use React refs and proper cleanup.
- **Fetching all entries to find one:** The current `get_clipboard_item` loads 1000 entries and filters. The new command must query by ID directly.
- **Multiple useEffect with shared refs:** The current DashboardPage has 3+ useEffects sharing refs. The hook consolidates this into a single cohesive unit.

## Don't Hand-Roll

| Problem                 | Don't Build                           | Use Instead                                                        | Why                                                          |
| ----------------------- | ------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------ |
| Throttle                | Custom throttle from scratch          | Simple trailing throttle with `setTimeout` + timestamp check       | Current code already does this correctly; just adjust timing |
| Single-entry projection | Duplicate projection logic in command | Reuse `ListClipboardEntryProjections` with new single-entry method | Keeps DTO mapping in one place                               |
| Event type routing      | Switch on raw strings                 | Extend `ClipboardEvent` TypeScript type with discriminated union   | Type safety                                                  |

## Common Pitfalls

### Pitfall 1: Race Between Single-Query and Full-Reload

**What goes wrong:** Local event triggers single-entry prepend, then a remote event triggers full-reload. If the full-reload finishes first and includes the new item, the subsequent prepend creates a duplicate.
**Why it happens:** Async operations complete out of order.
**How to avoid:** The `prependItem` reducer already has dedup check (scans for existing entry_id). Additionally, `fetchClipboardItems.fulfilled` with offset=0 replaces the entire list, so any prepended items are naturally handled.
**Warning signs:** Duplicate entries appearing in the list.

### Pitfall 2: Offset Desync After Prepend

**What goes wrong:** User scrolls down, new item is prepended. On next "load more", the offset is wrong and items are skipped or duplicated.
**Why it happens:** `offsetRef` tracks cursor position for infinite scroll. Prepend shifts all items by 1.
**How to avoid:** When `prependItem` succeeds, increment `offsetRef.current += 1` in the hook. When `removeItem` succeeds, decrement `offsetRef.current -= 1`. Full-reload resets offset to fetched count.
**Warning signs:** Missing items when scrolling, or duplicate items at page boundaries.

### Pitfall 3: Encryption Not Ready During Single-Entry Query

**What goes wrong:** Event arrives, hook calls `get_clipboard_entry`, but encryption session not ready. Backend returns error or encrypted gibberish.
**Why it happens:** The event gate (`encryptionReadyRef.current !== true`) must also guard the single-entry query path.
**How to avoid:** Keep the existing encryption-ready gate. All event handling (both local and remote) is gated behind encryption readiness check.
**Warning signs:** Garbled preview text, command errors in console.

### Pitfall 4: ClipboardEvent Serde Breaking Change

**What goes wrong:** Adding `origin` field to `ClipboardEvent::NewContent` is a breaking change for any code deserializing the old format (without `origin`).
**Why it happens:** Adding a required field to a serde struct variant.
**How to avoid:** Make the field required (since all emission sites will be updated atomically). The frontend TypeScript type must be updated simultaneously. No external consumers exist.
**Warning signs:** Serde deserialization errors in tests.

### Pitfall 5: `get_clipboard_item` Audit - Frontend Usage

**What goes wrong:** Removing `get_clipboard_item` breaks something.
**Finding from research:** Frontend calls `get_clipboard_item` in `src/api/clipboardItems.ts:225` via `getClipboardItem()`. Test at `src/api/__tests__/clipboardItems.test.ts:123` exercises it. However, **no component currently calls `getClipboardItem()`** -- it is only defined in the API module.
**How to avoid:** Grep the codebase for all `getClipboardItem` usages before removing. If truly unused in components, remove both the frontend function and backend command.

## Code Examples

### Backend: Origin Mapping

```rust
// In runtime.rs on_clipboard_changed, the `origin` variable is already a ClipboardChangeOrigin.
// Map it to the string the frontend expects:
let origin_str = match origin {
    ClipboardChangeOrigin::LocalCapture | ClipboardChangeOrigin::LocalRestore => "local",
    ClipboardChangeOrigin::RemotePush => "remote",
};

let event = ClipboardEvent::NewContent {
    entry_id: entry_id.to_string(),
    preview: "New clipboard content".to_string(),
    origin: origin_str.to_string(),
};
```

### Frontend: Updated ClipboardEvent Type

```typescript
// src/types/events.ts
export interface ClipboardEvent {
  type: 'NewContent' | 'Deleted'
  entry_id?: string
  preview?: string
  origin?: 'local' | 'remote' // Only present on NewContent
}
```

### Frontend: prependItem Reducer

```typescript
prependItem: (state, action: PayloadAction<ClipboardItemResponse>) => {
  if (state.items.some(item => item.id === action.payload.id)) return
  state.items.unshift(action.payload)
},
removeItem: (state, action: PayloadAction<string>) => {
  state.items = state.items.filter(item => item.id !== action.payload)
},
```

### Frontend: API Function for Single Entry

```typescript
// src/api/clipboardItems.ts
export async function getClipboardEntry(entryId: string): Promise<ClipboardItemResponse | null> {
  const response = await invokeWithTrace<ClipboardEntriesResponse>('get_clipboard_entry', {
    entryId,
  })
  if (response.status === 'not_ready') return null
  const entry = response.entries[0]
  if (!entry) return null
  // Transform using same logic as getClipboardItems
  return transformProjectionToResponse(entry)
}
```

### Hook: Event Routing Pattern

```typescript
// In useClipboardEvents.ts
if (event.payload.type === 'NewContent' && event.payload.entry_id) {
  if (!encryptionReadyRef.current) return

  if (event.payload.origin === 'local') {
    // Single-entry query + prepend
    try {
      const item = await getClipboardEntry(event.payload.entry_id)
      if (item) {
        dispatch(prependItem(item))
        offsetRef.current += 1
      }
    } catch {
      // Fallback to full reload
      loadData({ reset: true })
    }
  } else {
    // Remote: throttled full reload
    throttledReload()
  }
}
```

## State of the Art

| Old Approach                             | Current Approach                     | When Changed | Impact                                            |
| ---------------------------------------- | ------------------------------------ | ------------ | ------------------------------------------------- |
| Full reload on every event               | Incremental local + throttled remote | This phase   | Reduces DB queries for local captures from N to 1 |
| 500ms throttle                           | 300ms throttle                       | This phase   | More responsive UI                                |
| Global module-level listener state       | React hook with refs                 | This phase   | Proper cleanup, no memory leaks                   |
| `get_clipboard_item` (fetch all, filter) | `get_clipboard_entry` (single query) | This phase   | O(1) vs O(N) for single item lookup               |

## Open Questions

1. **`get_clipboard_item` usage audit**
   - What we know: Defined in `src/api/clipboardItems.ts`, tested in `clipboardItems.test.ts`, registered in `invoke_handler` in `main.rs`. No component-level import found.
   - What's unclear: Whether any dynamic/lazy usage exists that grep wouldn't catch.
   - Recommendation: Do thorough grep during implementation. If unused, remove. If used, keep but mark deprecated.

2. **Single-entry projection implementation**
   - What we know: `ListClipboardEntryProjections` has no single-entry method. `ClipboardEntryRepositoryPort` has `get_entry(entry_id)`.
   - What's unclear: Whether adding `execute_single` to the use case is worth the complexity vs. building projection inline.
   - Recommendation: Add `execute_single(entry_id)` to `ListClipboardEntryProjections` that calls `get_entry` then applies the same projection logic. This keeps projection mapping DRY.

## Validation Architecture

### Test Framework

| Property           | Value                                                             |
| ------------------ | ----------------------------------------------------------------- |
| Framework          | Vitest ^4.0.17 (frontend), cargo test (backend)                   |
| Config file        | No vitest.config.\* found -- uses package.json `"test": "vitest"` |
| Quick run command  | `bun run test`                                                    |
| Full suite command | `bun run test && cd src-tauri && cargo test`                      |

### Phase Requirements -> Test Map

| Req ID | Behavior                                                      | Test Type   | Automated Command                                                | File Exists?                                              |
| ------ | ------------------------------------------------------------- | ----------- | ---------------------------------------------------------------- | --------------------------------------------------------- |
| P16-01 | `prependItem` reducer dedup + insert at head                  | unit        | `bun run test src/store/slices/__tests__/clipboardSlice.test.ts` | No -- Wave 0                                              |
| P16-02 | `removeItem` reducer removes by entry_id                      | unit        | `bun run test src/store/slices/__tests__/clipboardSlice.test.ts` | No -- Wave 0                                              |
| P16-03 | `get_clipboard_entry` backend command returns single entry    | integration | `cd src-tauri && cargo test get_clipboard_entry`                 | No -- Wave 0                                              |
| P16-04 | ClipboardEvent NewContent serializes with origin field        | unit        | `cd src-tauri && cargo test clipboard_event`                     | Partial (existing serde test covers type tag, not origin) |
| P16-05 | `useClipboardEvents` routes local events to prepend path      | unit        | `bun run test src/hooks/__tests__/useClipboardEvents.test.ts`    | No -- Wave 0                                              |
| P16-06 | `useClipboardEvents` routes remote events to throttled reload | unit        | `bun run test src/hooks/__tests__/useClipboardEvents.test.ts`    | No -- Wave 0                                              |

### Sampling Rate

- **Per task commit:** `bun run test --run`
- **Per wave merge:** `bun run test --run && cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src/store/slices/__tests__/clipboardSlice.test.ts` -- covers P16-01, P16-02
- [ ] `src/hooks/__tests__/useClipboardEvents.test.ts` -- covers P16-05, P16-06
- [ ] Backend test for `get_clipboard_entry` command in `clipboard.rs` test module -- covers P16-03
- [ ] Update existing `ClipboardEvent` serde test to cover `origin` field -- covers P16-04

## Sources

### Primary (HIGH confidence)

- Direct codebase analysis of `DashboardPage.tsx`, `clipboardSlice.ts`, `events/mod.rs`, `clipboard.rs`, `runtime.rs`, `wiring.rs`, `list_entry_projections.rs`
- `ClipboardChangeOrigin` enum in `uc-core/src/clipboard/change.rs` confirms three variants: `LocalCapture`, `LocalRestore`, `RemotePush`
- `invoke_handler` in `main.rs` confirms `get_clipboard_item` is registered
- Frontend test files confirm Vitest is the test framework

### Secondary (MEDIUM confidence)

- `getClipboardItem` frontend usage audit: grep shows no component imports beyond API definition and test

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - all libraries already in use, no new dependencies
- Architecture: HIGH - direct codebase analysis, all integration points verified
- Pitfalls: HIGH - identified from actual code patterns and data flow analysis

**Research date:** 2026-03-08
**Valid until:** 2026-04-08 (stable, internal refactoring)
