# Phase 78: Frontend Clipboard API Migration - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)

<domain>
## Phase Boundary

Migrate all frontend clipboard API calls from Tauri invoke() to daemon HTTP client. This covers: list entries, entry detail, delete, restore, favorite toggle, stats, and resource/blob content. Preserve existing UI behavior and data contracts.

</domain>

<decisions>
## Implementation Decisions

### New Daemon Clipboard API Module (src/api/daemon/clipboard.ts)

- `getClipboardEntries(limit, offset)` → GET `/clipboard/entries?limit=N&offset=N`
- `getClipboardEntry(id)` → GET `/clipboard/entries/:id`
- `deleteClipboardEntry(id)` → DELETE `/clipboard/entries/:id`
- `restoreClipboardEntry(id)` → POST `/clipboard/entries/:id/restore`
- `toggleFavorite(id, favorited)` → POST `/clipboard/entries/:id/favorite`
- `getClipboardStats()` → GET `/clipboard/stats`
- `getClipboardEntryResource(id)` → GET `/clipboard/entries/:id/resource`

### Migration Strategy

- Create new daemon API module alongside existing Tauri API
- Update Redux thunks/RTK Query to use new daemon API functions
- Keep old Tauri API functions temporarily for fallback/debugging
- One-by-one migration of each API call with UI testing

### Data Contract Preservation

- Response shapes must match current Tauri command return types
- snake_case field names preserved (EntryProjectionDto, ClipboardStats)
- Pagination parameters match current frontend expectations
- Resource endpoint must serve same content types as current uc:// protocol handler

### Claude's Discretion

- Whether to use RTK Query for daemon endpoints or keep manual thunks
- Migration order of individual API calls
- Error mapping from daemon HTTP errors to existing frontend error handling
- Whether to create a feature flag for switching between Tauri and daemon APIs

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Current Frontend API

- `src/api/` — Current Tauri invoke clipboard API functions
- `src/store/` — Redux slices that consume clipboard API

### Daemon Client

- Phase 77 output: `src/api/daemon/client.ts` — DaemonClient class

### Backend DTOs

- `src-tauri/crates/uc-app/src/dtos/` — EntryProjectionDto, ClipboardStats (source of truth for response shapes)

### Current Tauri Commands

- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — Current implementations being replaced

</canonical_refs>

<specifics>
## Specific Ideas

- The restore endpoint was already moved to daemon in Phase 72 — this just switches the frontend caller
- Resource/blob serving may need special handling for binary content (images, files)
- Pagination must be consistent with current clipboard list behavior
- Stats endpoint used by Dashboard page for entry counts

</specifics>

<deferred>
## Deferred Ideas

- Settings, encryption, storage API migration (can happen later)
- WebSocket event migration (Phase 79)
- Removing old Tauri clipboard commands (Phase 80)

</deferred>

---

_Phase: 78-frontend-clipboard-api-migration_
_Context gathered: 2026-03-29 via PRD Express Path_
