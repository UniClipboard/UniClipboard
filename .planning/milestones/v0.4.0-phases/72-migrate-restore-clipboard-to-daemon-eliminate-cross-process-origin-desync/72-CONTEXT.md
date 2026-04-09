# Phase 72: Migrate restore-clipboard to daemon — eliminate cross-process origin desync - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning

<domain>
## Phase Boundary

Move the clipboard restore operation (`restore_clipboard_entry`) from the GUI Tauri process to the daemon, so that `ClipboardChangeOriginPort::set_next_origin(LocalRestore)` is armed in-process before the OS clipboard write. This eliminates the cross-process origin tracker desync that causes duplicate DB entries and spurious outbound sync when the daemon's `ClipboardWatcherWorker` misclassifies the restore write as `LocalCapture`.

</domain>

<decisions>
## Implementation Decisions

### Daemon API Design

- **D-01:** Restore goes through HTTP POST endpoint (`POST /clipboard/restore/{entry_id}`) following the established daemon mutation pattern (loopback + bearer auth)
- **D-02:** Response is minimal: 200 OK + `{ success: true }` or error code. The `clipboard.new_content` WS event (already emitted by ClipboardWatcherWorker after OS clipboard write) handles frontend state updates

### Sync Behavior After Restore

- **D-03:** Restore triggers outbound sync to peers — behavior preserved from current implementation. The daemon's `DaemonClipboardChangeHandler` will correctly identify the origin as `LocalRestore` and `OutboundSyncPlanner` allows sync for both `LocalCapture` and `LocalRestore`

### GUI Compatibility Path

- **D-04:** No Full mode fallback retained. All restore operations go exclusively through daemon HTTP API. The GUI `restore_clipboard_entry` Tauri command becomes a thin proxy that calls the daemon endpoint via `DaemonHttpClient`
- **D-05:** Direct `RestoreClipboardSelectionUseCase` invocation and local `ClipboardChangeOriginPort` usage removed from uc-tauri command layer

### Claude's Discretion

- Daemon-side implementation details: whether to reuse existing `RestoreClipboardSelectionUseCase` directly or create a new daemon-specific handler
- Error mapping between daemon HTTP errors and Tauri command errors for frontend compatibility
- Whether `touch_clipboard_entry` (active_time bump) stays in the daemon endpoint or moves elsewhere

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Restore Flow

- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — Current `restore_clipboard_entry` and `restore_clipboard_entry_impl` (lines 532-620)
- `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs` — `RestoreClipboardSelectionUseCase` with `build_snapshot` and `restore_snapshot` methods

### Origin Tracking

- `src-tauri/crates/uc-core/src/ports/clipboard/clipboard_change_origin.rs` — `ClipboardChangeOriginPort` trait definition
- `src-tauri/crates/uc-core/src/clipboard/change.rs` — `ClipboardChangeOrigin` enum (LocalCapture, LocalRestore, RemotePush)
- `src-tauri/crates/uc-infra/src/clipboard/change_origin.rs` — `InMemoryClipboardChangeOrigin` implementation

### Daemon Integration Points

- `src-tauri/crates/uc-daemon/src/main.rs` — Daemon composition showing shared `clipboard_change_origin` Arc (line ~122)
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — `DaemonClipboardChangeHandler` using shared origin tracker
- `src-tauri/crates/uc-daemon/src/server.rs` — Daemon HTTP server and route registration
- `src-tauri/crates/uc-daemon/src/routes.rs` — Existing HTTP route handlers (pairing, setup, space-access patterns)

### Daemon Client (GUI side)

- `src-tauri/crates/uc-daemon-client/` — `DaemonHttpClient` used by uc-tauri for daemon API calls

### Outbound Sync

- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound_clipboard.rs` — `SyncOutboundClipboardUseCase`
- `src-tauri/crates/uc-app/src/usecases/clipboard/outbound_sync_planner.rs` — `OutboundSyncPlanner` policy

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `RestoreClipboardSelectionUseCase` — already in uc-app, Tauri-free. Can be called directly from daemon route handler
- `DaemonHttpClient` — existing daemon client in uc-daemon-client, already used by uc-tauri for pairing/setup/space-access API calls
- `InMemoryClipboardChangeOrigin` — shared Arc pattern already established in daemon main.rs (shared between ClipboardWatcherWorker, InboundClipboardSyncWorker, FileSyncOrchestratorWorker)
- `TouchClipboardEntryUseCase` — already in uc-app for bumping active_time

### Established Patterns

- Daemon HTTP mutation: `POST /pairing/initiate`, `POST /setup/...` — JSON body + bearer auth + JSON response
- Daemon route handler accesses `CoreRuntime` and `CoreUseCases` via shared `Arc<NonGuiRuntime>`
- uc-tauri daemon proxy pattern: Tauri command calls `DaemonHttpClient::post(...)` and maps errors to `String` for frontend

### Integration Points

- `restore_clipboard_entry` Tauri command in `commands/clipboard.rs` — rewire from direct use case call to daemon HTTP call
- Daemon `routes.rs` — add new `/clipboard/restore/{entry_id}` route
- Daemon `server.rs` — register new route
- `uc-core::network::daemon_api_strings` — add clipboard restore endpoint constant (Phase 56.1 pattern)

</code_context>

<specifics>
## Specific Ideas

No specific requirements — standard daemon migration following established patterns from pairing/setup/space-access migrations.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 72-migrate-restore-clipboard-to-daemon-eliminate-cross-process-origin-desync_
_Context gathered: 2026-03-29_
