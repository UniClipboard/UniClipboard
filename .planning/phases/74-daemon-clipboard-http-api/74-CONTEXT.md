# Phase 74: Daemon Clipboard HTTP API - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)

<domain>
## Phase Boundary

Add full clipboard CRUD HTTP endpoints to the daemon: list entries with pagination, get entry detail, delete entry, toggle favorite, get stats, and get entry resource/blob content. Also add clipboard WebSocket topics for real-time event delivery (new-content, updated, deleted).

This phase extends the existing daemon HTTP API (Phase 45 foundation) with clipboard-specific endpoints that will replace Tauri invoke() calls in later phases.

</domain>

<decisions>
## Implementation Decisions

### HTTP Endpoints

- GET `/clipboard/entries` — list with pagination (limit, offset params), permission L2
- GET `/clipboard/entries/:id` — single entry detail, permission L3
- DELETE `/clipboard/entries/:id` — delete entry, permission L3
- POST `/clipboard/entries/:id/restore` — restore to system clipboard, permission L3
- POST `/clipboard/entries/:id/favorite` — toggle favorite, permission L2
- GET `/clipboard/stats` — statistics, permission L2
- GET `/clipboard/entries/:id/resource` — resource URL/content, permission L3

### WebSocket Topics

- Topic `clipboard` event `new-content` — new entry created, permission L2
- Topic `clipboard` event `updated` — entry updated (favorite, active_time), permission L2
- Topic `clipboard` event `deleted` — entry deleted, permission L2

### Event Payload Types

- ClipboardNewContentPayload: { entry_id, preview, origin ("local"|"remote"), content_type }
- ClipboardDeletedPayload: { entry_id }
- ClipboardUpdatedPayload: { entry_id, changes: Vec<String> }

### API Response Format

- Success: `{ "data": { ... }, "ts": 1234567890 }`
- Error: `{ "error": { "code": "...", "message": "...", "details": { ... } }, "ts": 1234567890 }`

### Route Registration

- Routes registered under axum Router with existing bearer token auth middleware
- Permission levels (L2/L3) checked per endpoint — L3 requires encryption session ready

### Claude's Discretion

- Handler module structure and file organization within daemon API
- DTO type naming and field serialization details
- Query parameter parsing approach for pagination
- Resource endpoint content-type negotiation for blob vs metadata
- Whether to use existing EntryProjectionDto from uc-app or create daemon-specific DTOs

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Daemon API

- `src-tauri/crates/uc-daemon/src/api/` — Existing daemon HTTP API structure and patterns
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — Route registration patterns
- `src-tauri/crates/uc-daemon/src/api/ws_handler.rs` — WebSocket handler and topic system

### Use Cases and DTOs

- `src-tauri/crates/uc-app/src/usecases/clipboard/` — Clipboard use cases (get entries, delete, restore, favorite, stats)
- `src-tauri/crates/uc-app/src/dtos/` — EntryProjectionDto, ClipboardStats definitions

### Existing Tauri Commands (reference implementation)

- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — Current clipboard command implementations to replicate
- `src-tauri/crates/uc-tauri/src/commands/storage.rs` — Resource/blob serving patterns

### Wire Protocol Constants

- `src-tauri/crates/uc-core/src/daemon_api_strings.rs` — Daemon API string constants (Phase 56.1)

</canonical_refs>

<specifics>
## Specific Ideas

- Permission L2 endpoints (list, stats, favorite) require valid bearer token only
- Permission L3 endpoints (detail, delete, restore, resource) additionally require encryption session to be ready
- The restore endpoint mirrors the Phase 72 daemon restore route (`POST /clipboard/restore/:entry_id`)
- WebSocket events should integrate with existing DaemonApiEventEmitter broadcast mechanism
- Clipboard WS topic already registered in `is_supported_topic()` (Phase 66 fix)

</specifics>

<deferred>
## Deferred Ideas

- JWT session tokens and advanced security middleware (Phase 75)
- Frontend client consuming these endpoints (Phase 77-78)
- Settings, encryption, storage endpoints (Phase 76)

</deferred>

---

_Phase: 74-daemon-clipboard-http-api_
_Context gathered: 2026-03-29 via PRD Express Path_
