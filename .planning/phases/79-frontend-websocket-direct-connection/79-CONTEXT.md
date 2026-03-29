# Phase 79: Frontend WebSocket Direct Connection & Event Migration - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)

<domain>
## Phase Boundary

Create DaemonWsClient for direct WebSocket connection from frontend to daemon. Implement topic-based subscription with reconnect/backoff. Migrate clipboard/encryption/lifecycle event listeners from Tauri listen() to daemon WS. Add React hooks for event consumption.

</domain>

<decisions>
## Implementation Decisions

### DaemonWsClient Class (src/lib/daemon-ws.ts)

- Singleton `daemonWs` instance
- `connect()` method: opens WS to daemon wsUrl from DaemonConfig
- `subscribe(topics, callback)` method: returns unsubscribe function
- Topic-based dispatch: exact topic match + wildcard `*` subscriber
- Reconnect with exponential backoff (1s → 30s max, 10 attempts max)
- Subscribe message format: `{ action: "subscribe", topics: [...], nonce: "..." }`

### Event Types

- DaemonWsEvent<T>: { topic, type, ts, sessionId, payload: T }
- WsEventCallback<T>: (event: DaemonWsEvent<T>) => void

### React Hooks (src/hooks/useDaemonEvents.ts)

- `useClipboardNewContent(callback)` — clipboard new-content events
- `usePairingEvents(callbacks)` — pairing verification/complete/failed
- `useEncryptionState(onReady, onFailed)` — encryption session events
- Each hook uses useEffect with subscribe/unsubscribe pattern

### Migration: Tauri Events → Daemon WS

- `listen('daemon://realtime')` → `daemonWs.subscribe(['clipboard', ...])`
- `listen('clipboard://event')` → `daemonWs.subscribe(['clipboard'], ...)`
- `listen('daemon://ws-reconnected')` → `daemonWs.subscribe(['lifecycle'], ...)`
- Existing DaemonWsBridge in Rust still operates but frontend bypasses it

### WebSocket Topics to Subscribe

- clipboard: new-content, updated, deleted
- encryption: session-ready, session-failed
- lifecycle: ready
- status: system state changes
- peers: device state changes
- pairing: verification, complete, failed
- setup: flow events
- file-transfer: status changes

### Claude's Discretion

- Whether to keep DaemonWsBridge in uc-tauri as fallback during transition
- Connection auth for WebSocket (token in query param vs first message)
- React context provider for WS client lifecycle
- Event buffering during reconnection
- How to handle missed events during disconnection

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Current Event System

- `src/api/realtime.ts` — Current Tauri event listening setup
- `src/store/` — Redux slices that consume realtime events
- `src/hooks/` — Existing custom hooks

### Daemon WebSocket

- `src-tauri/crates/uc-daemon/src/api/ws_handler.rs` — Daemon WS server implementation
- `src-tauri/crates/uc-daemon/src/api/` — Topic/event system

### Tauri Bridge (being bypassed)

- `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` — Current DaemonWsBridge
- `src-tauri/crates/uc-tauri/src/bootstrap/` — Bridge setup

### Daemon Client

- Phase 77 output: `src/api/daemon/client.ts` — Connection config

</canonical_refs>

<specifics>
## Specific Ideas

- The daemon WS server already supports topic-based subscriptions (is_supported_topic)
- Frontend currently receives events through: Daemon WS → DaemonWsBridge (Rust) → Tauri emit → frontend listen()
- This phase creates a direct path: Daemon WS → frontend JS WebSocket → React hooks
- Reconnection compensation logic (re-fetch data on reconnect) already exists in Phase 66
- The nonce in subscribe message prevents replay attacks

</specifics>

<deferred>
## Deferred Ideas

- Removing DaemonWsBridge from uc-tauri (Phase 80)
- Settings/storage event subscriptions
- WebSocket compression (permessage-deflate)

</deferred>

---

_Phase: 79-frontend-websocket-direct-connection_
_Context gathered: 2026-03-29 via PRD Express Path_
