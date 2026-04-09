# Phase 66: daemon-dashboard - Context

**Gathered:** 2026-03-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Fix the broken event chain that prevents the frontend Dashboard from auto-refreshing clipboard history when running in daemon mode. The daemon correctly captures clipboard changes and broadcasts WS events, but the GUI never receives them because `is_supported_topic()` in the daemon WS API does not include the `clipboard` topic.

Scope extends to:

1. Fixing the clipboard topic registration gap
2. Full audit of all RealtimeEvent variants' WS→Tauri emit chains
3. Fixing any other missing WS topic registrations discovered
4. Adding WS reconnection compensation (dashboard refresh on reconnect)

</domain>

<decisions>
## Implementation Decisions

### Event Bridge Completeness

- **D-01:** Full chain audit of ALL RealtimeEvent variants from Daemon WS → GUI Bridge → Tauri emit → Frontend hook. Not just clipboard — every event type must be verified end-to-end.
- **D-02:** Any broken links discovered in the audit must be fixed in this phase, not deferred.

### Missing Topic Registration

- **D-03:** All missing WS topic registrations in `is_supported_topic()` must be fixed in this phase, not just `clipboard`. If `file-transfer` or other topics are missing, fix them together.
- **D-04:** The root fix is adding `ws_topic::CLIPBOARD` (and any other missing topics) to `is_supported_topic()` in `src-tauri/crates/uc-daemon/src/api/ws.rs`.

### Reconnection Compensation

- **D-05:** When WS reconnects successfully, trigger a full Dashboard clipboard list refresh to compensate for events missed during disconnection.
- **D-06:** This is a simple "reconnect → refetch" pattern, not a complex delta-sync mechanism.

### Claude's Discretion

- Implementation details of the reconnection detection and refresh trigger mechanism
- Whether to add integration tests for the WS topic subscription flow
- How to structure the audit findings (inline fixes vs. separate commits)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Daemon WS API

- `src-tauri/crates/uc-daemon/src/api/ws.rs` — WS server with `is_supported_topic()` filter (root cause at L167-179)
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — Centralized WS topic and event string constants

### Daemon Clipboard Workers

- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — ClipboardWatcherWorker that broadcasts `clipboard.new_content` WS events

### GUI Daemon Client

- `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` — DaemonWsBridge with `map_daemon_ws_event()` for event parsing
- `src-tauri/crates/uc-daemon-client/src/realtime.rs` — RealtimeEvent consumption and clipboard topic subscription

### Frontend Event Hooks

- `src/hooks/useClipboardEventStream.ts` — Listens to `clipboard://event` Tauri events for Dashboard updates
- `src/hooks/useClipboardEvents.ts` — Dashboard data management using clipboard event stream

### Prior Phase Context

- Phase 57: Daemon became sole clipboard observer; GUI operates in Passive mode
- Phase 62: Daemon inbound clipboard sync via WS events
- Phase 64: Removed daemon-duplicated sync loops from wiring.rs

</canonical_refs>

<code_context>

## Existing Code Insights

### Root Cause

- `is_supported_topic()` in `ws.rs:167-179` lists 8 topics but omits `ws_topic::CLIPBOARD` (and potentially others like `file-transfer`)
- GUI's `ws_bridge.rs` already implements `clipboard.new_content` event parsing in `map_daemon_ws_event()`
- GUI's `realtime.rs` already subscribes to `RealtimeTopic::Clipboard`

### Reusable Assets

- `DaemonWsBridge` already has full clipboard event parsing — just needs the subscription to actually work
- `useClipboardEventStream` hook is already wired to handle both local prepend and remote invalidate
- `ws_topic` and `ws_event` constants are centralized in `daemon_api_strings.rs`

### Established Patterns

- WS topic subscription: client sends `{"action":"subscribe","topics":[...]}` → daemon validates via `is_supported_topic()` → events filtered by subscribed topics
- Tauri event emission: RealtimeEvent variants mapped to `clipboard://event`, `file-transfer://status`, etc.
- Dashboard refresh: `useClipboardEvents` dispatches `prependItem` for local, `loadData(reset)` for remote

### Integration Points

- `is_supported_topic()` is the single gatekeeper for all WS subscriptions
- `map_daemon_ws_event()` is the single translator from WS events to RealtimeEvent
- Frontend hooks listen on specific Tauri event channels (`clipboard://event`, etc.)

</code_context>

<specifics>
## Specific Ideas

No specific requirements — the fix targets are well-defined by the root cause analysis. Standard approaches apply.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 66-daemon-dashboard_
_Context gathered: 2026-03-27_
