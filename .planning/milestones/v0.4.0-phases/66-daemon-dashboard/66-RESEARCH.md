# Phase 66: daemon-dashboard - Research

**Researched:** 2026-03-27
**Domain:** Daemon WebSocket topic registration, GUI event bridge, frontend reconnect compensation
**Confidence:** HIGH

## Summary

Phase 66 fixes a broken event chain that prevents the frontend Dashboard from auto-refreshing clipboard history when the app runs in daemon mode. The daemon correctly captures clipboard changes and broadcasts `clipboard.new_content` WS events on topic `"clipboard"`, but the GUI never receives them because `is_supported_topic()` in `uc-daemon/src/api/ws.rs` (L167-179) does not include `ws_topic::CLIPBOARD`. The subscription request from the GUI client is silently dropped by `normalize_topics()` before it ever reaches the fan-out logic.

All the downstream plumbing — `DaemonWsBridge::map_daemon_ws_event()` parsing `clipboard.new_content`, `realtime.rs` subscribing to `RealtimeTopic::Clipboard` and running `run_clipboard_realtime_consumer_with_rx()`, `TauriEventEmitter` emitting `clipboard://event`, and `useClipboardEventStream` listening to that event — is already fully implemented and correct. The sole blocking gap is the one-line topic registration omission in the daemon server.

Similarly, `ws_topic::FILE_TRANSFER` ("file-transfer") is also absent from `is_supported_topic()`. The daemon's `DaemonApiEventEmitter` broadcasts file-transfer events on that topic (confirmed in `event_emitter.rs`), but no GUI consumer currently subscribes to it via `DaemonWsBridge`. That topic must also be added to `is_supported_topic()` per D-03, even if the GUI consumer is not yet wired (the server must allow the subscription). Reconnection compensation — triggering a full Dashboard refetch after WS reconnects — is the other deliverable (D-05/D-06), requiring a mechanism to detect bridge state transitions and invoke `loadData({ reset: true })`.

**Primary recommendation:** Add `ws_topic::CLIPBOARD` (and `ws_topic::FILE_TRANSFER`) to `is_supported_topic()`, then wire a reconnect callback from `DaemonWsBridge` to trigger frontend clipboard list invalidation.

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Full chain audit of ALL RealtimeEvent variants from Daemon WS → GUI Bridge → Tauri emit → Frontend hook. Not just clipboard — every event type must be verified end-to-end.
- **D-02:** Any broken links discovered in the audit must be fixed in this phase, not deferred.
- **D-03:** All missing WS topic registrations in `is_supported_topic()` must be fixed in this phase, not just `clipboard`. If `file-transfer` or other topics are missing, fix them together.
- **D-04:** The root fix is adding `ws_topic::CLIPBOARD` (and any other missing topics) to `is_supported_topic()` in `src-tauri/crates/uc-daemon/src/api/ws.rs`.
- **D-05:** When WS reconnects successfully, trigger a full Dashboard clipboard list refresh to compensate for events missed during disconnection.
- **D-06:** This is a simple "reconnect → refetch" pattern, not a complex delta-sync mechanism.

### Claude's Discretion

- Implementation details of the reconnection detection and refresh trigger mechanism
- Whether to add integration tests for the WS topic subscription flow
- How to structure the audit findings (inline fixes vs. separate commits)

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

---

## Standard Stack

This phase makes targeted fixes to existing code. No new libraries are needed.

### Core (existing, verified by code inspection)

| Layer                | File                                          | Role                                                      |
| -------------------- | --------------------------------------------- | --------------------------------------------------------- |
| Daemon WS server     | `uc-daemon/src/api/ws.rs`                     | Topic gatekeeper via `is_supported_topic()`               |
| WS topic constants   | `uc-core/src/network/daemon_api_strings.rs`   | Single source of truth for topic/event strings            |
| GUI WS client        | `uc-daemon-client/src/ws_bridge.rs`           | `DaemonWsBridge`, `map_daemon_ws_event()`, `topic_name()` |
| GUI realtime runtime | `uc-daemon-client/src/realtime.rs`            | `start_realtime_runtime()`, consumer task spawning        |
| Tauri event emitter  | `uc-tauri/src/adapters/host_event_emitter.rs` | Maps `HostEvent` → `clipboard://event`                    |
| Frontend hook        | `src/hooks/useClipboardEventStream.ts`        | Listens on `clipboard://event`                            |
| Frontend hook        | `src/hooks/useClipboardEvents.ts`             | Calls `loadData({ reset: true })` on remote invalidate    |

### Architecture Patterns

The existing subscription model:

```
Client → {"action":"subscribe","topics":["clipboard"]}
           ↓
   normalize_topics() → is_supported_topic() → CURRENTLY BLOCKS "clipboard"
           ↓ (after fix: passes through)
   topics HashSet per connection
           ↓
   fanout_task: broadcast_rx.recv() → topic_matches() → send to outbound_tx
           ↓
   DaemonWsBridge.connect_and_process() → map_daemon_ws_event() → dispatch_event()
           ↓
   run_clipboard_realtime_consumer_with_rx() → HostEventEmitterPort::emit()
           ↓
   TauriEventEmitter → app.emit("clipboard://event", payload)
           ↓
   useClipboardEventStream → onLocalItem | onRemoteInvalidate
```

## Don't Hand-Roll

| Problem             | Don't Build                        | Use Instead                                                            |
| ------------------- | ---------------------------------- | ---------------------------------------------------------------------- |
| Topic string values | Hardcoded string literals in ws.rs | `ws_topic::CLIPBOARD` from `daemon_api_strings.rs` (PH561-01 complete) |
| WS reconnect retry  | Custom retry loop                  | `DaemonWsBridge::run()` already handles backoff reconnection           |
| Dashboard refetch   | Custom polling                     | `loadData({ reset: true })` already exists in `useClipboardEvents.ts`  |

## Audit: Complete RealtimeEvent Chain

Verified by code inspection of `ws_bridge.rs` (`map_daemon_ws_event` + `event_topic` + `topic_name`):

| RealtimeEvent Variant         | WS Event Type                              | `map_daemon_ws_event` | `event_topic`   | `topic_name`               | `is_supported_topic` | Status     |
| ----------------------------- | ------------------------------------------ | --------------------- | --------------- | -------------------------- | -------------------- | ---------- |
| `ClipboardNewContent`         | `clipboard.new_content`                    | YES (L754-775)        | `Clipboard`     | `ws_topic::CLIPBOARD`      | **MISSING**          | **BROKEN** |
| `PairingUpdated`              | `pairing.updated`                          | YES                   | `Pairing`       | `ws_topic::PAIRING`        | YES                  | OK         |
| `PairingVerificationRequired` | `pairing.verification_required`            | YES                   | `Pairing`       | `ws_topic::PAIRING`        | YES                  | OK         |
| `PairingComplete`             | `pairing.complete`                         | YES                   | `Pairing`       | `ws_topic::PAIRING`        | YES                  | OK         |
| `PairingFailed`               | `pairing.failed`                           | YES                   | `Pairing`       | `ws_topic::PAIRING`        | YES                  | OK         |
| `PeersChanged`                | `peers.changed`                            | YES                   | `Peers`         | `ws_topic::PEERS`          | YES                  | OK         |
| `PeersNameUpdated`            | `peers.name_updated`                       | YES                   | `Peers`         | `ws_topic::PEERS`          | YES                  | OK         |
| `PeersConnectionChanged`      | `peers.connection_changed`                 | YES                   | `Peers`         | `ws_topic::PEERS`          | YES                  | OK         |
| `PairedDevicesChanged`        | `paired-devices.changed`                   | YES                   | `PairedDevices` | `ws_topic::PAIRED_DEVICES` | YES                  | OK         |
| `SetupStateChanged`           | `setup.state_changed`                      | YES                   | `Setup`         | `ws_topic::SETUP`          | YES                  | OK         |
| `SetupSpaceAccessCompleted`   | `setup.space_access_completed`             | YES                   | `Setup`         | `ws_topic::SETUP`          | YES                  | OK         |
| `SpaceAccessStateChanged`     | `space_access.state_changed` + `.snapshot` | YES                   | `SpaceAccess`   | `ws_topic::SPACE_ACCESS`   | YES                  | OK         |

**Additional topic missing from `is_supported_topic`:** `ws_topic::FILE_TRANSFER` ("file-transfer")

- Daemon `DaemonApiEventEmitter` broadcasts `file-transfer.status_changed` events on this topic (confirmed `event_emitter.rs:116-117`)
- `RealtimeTopic` enum does NOT have a `FileTransfer` variant — file-transfer events flow via `HostEvent::Transfer` path, not `RealtimeEvent`
- No GUI `DaemonWsBridge` consumer currently subscribes to this topic
- Per D-03: the topic must be added to `is_supported_topic()` so future consumers can subscribe without a server-side change

**Note:** `ws_topic::PAIRING_SESSION` and `ws_topic::PAIRING_VERIFICATION` are in `is_supported_topic()` but NOT in `topic_name()` (no corresponding `RealtimeTopic` variants). They are handled server-side for HTTP snapshot routes only. This is expected — no fix needed.

## Root Cause: Single-Line Fix

The complete root cause (HIGH confidence, verified by code):

```rust
// src-tauri/crates/uc-daemon/src/api/ws.rs — current (L167-179)
fn is_supported_topic(topic: &str) -> bool {
    matches!(
        topic,
        ws_topic::STATUS
            | ws_topic::PEERS
            | ws_topic::PAIRED_DEVICES
            | ws_topic::PAIRING
            | ws_topic::PAIRING_SESSION
            | ws_topic::PAIRING_VERIFICATION
            | ws_topic::SETUP
            | ws_topic::SPACE_ACCESS
            // ws_topic::CLIPBOARD is MISSING   ← root cause
            // ws_topic::FILE_TRANSFER is MISSING ← D-03
    )
}
```

Fix:

```rust
fn is_supported_topic(topic: &str) -> bool {
    matches!(
        topic,
        ws_topic::STATUS
            | ws_topic::PEERS
            | ws_topic::PAIRED_DEVICES
            | ws_topic::PAIRING
            | ws_topic::PAIRING_SESSION
            | ws_topic::PAIRING_VERIFICATION
            | ws_topic::SETUP
            | ws_topic::SPACE_ACCESS
            | ws_topic::CLIPBOARD        // ADD
            | ws_topic::FILE_TRANSFER    // ADD (D-03)
    )
}
```

Also: `build_snapshot_event()` must handle `ws_topic::CLIPBOARD` and `ws_topic::FILE_TRANSFER` to avoid the `unsupported topic` bail. Clipboard has no snapshot (return `Ok(None)` like `PAIRING_VERIFICATION` and `SETUP`). File-transfer also has no snapshot (return `Ok(None)`).

## Reconnection Compensation (D-05/D-06)

### Current Reconnection Behavior

`DaemonWsBridge::run()` already implements exponential backoff reconnection:

- On connection failure: sets `BridgeState::Degraded`, waits with jitter, retries
- On reconnect success: calls `connect_and_process()` which sets `BridgeState::Subscribing` then `BridgeState::Ready`

There is currently no callback hook to notify the GUI when the bridge transitions from Degraded → Ready.

### Options for Reconnection Detection

**Option A: Expose `BridgeState` via watch channel** (recommended)

- Add a `tokio::sync::watch::Sender<BridgeState>` to `DaemonWsBridge`
- GUI consumers can watch for `Ready` state after `Degraded` to trigger refetch
- Clean, no polling, no Tauri-specific code in the bridge

**Option B: Emit a `HostEvent::DaemonWsReconnected` event**

- Emit via `HostEventEmitterPort` when bridge transitions to `Ready` from non-Ready
- Frontend listens on a new Tauri event channel (e.g., `daemon://ws-reconnected`)
- Requires adding new event variant to `HostEvent` — more invasive

**Option C: Monitor `BridgeState` in `start_realtime_runtime` task**

- Spawn a separate monitoring task that polls `bridge.state()` and emits a `HostEvent` on `Degraded → Ready` transition
- Uses existing sync `RwLock`-backed `state()` accessor
- No changes needed to `DaemonWsBridge` struct itself

**Recommendation (Claude's discretion):** Option C is the least invasive:

- Does not modify `DaemonWsBridge`'s public API
- Spawns a lightweight monitor task alongside the existing bridge task in `start_realtime_runtime`
- Reuses `HostEventEmitterPort` → `TauriEventEmitter` path already proven for other reconnection events
- Frontend only needs to listen for one new Tauri event to call `loadData({ reset: true })`

### Frontend Reconnect Refetch Pattern

`useClipboardEvents.ts` already has the right method:

```typescript
onRemoteInvalidate: () => {
  void loadData({ specificFilter: currentFilterRef.current, reset: true })
},
```

The reconnect handler just needs to call the same `loadData({ reset: true })` when the bridge reconnects. This can be hooked into `useClipboardEventStream` or as a separate `useEffect` listening to a new Tauri event.

## Common Pitfalls

### Pitfall 1: `build_snapshot_event` exhaustiveness

**What goes wrong:** Adding topics to `is_supported_topic()` without adding matching arms to `build_snapshot_event()` causes `anyhow::bail!("unsupported websocket topic: {topic}")` at subscription time for the new topics.
**How to avoid:** Add `ws_topic::CLIPBOARD => Ok(None)` and `ws_topic::FILE_TRANSFER => Ok(None)` arms before the `unsupported =>` fallback.

### Pitfall 2: Reconnect event firing on first connect

**What goes wrong:** If the monitor task tracks "previous state was non-Ready" including the initial `Disconnected` state, it fires a refetch on startup before the initial load completes.
**How to avoid:** Only emit the reconnect signal when transitioning from `Degraded` (not from `Disconnected` or `Connecting`) to `Ready`. Track last-seen state in the monitor task.

### Pitfall 3: Duplicate refetch on reconnect

**What goes wrong:** Both `onRemoteInvalidate` from a clipboard event AND the reconnect handler trigger `loadData({ reset: true })` concurrently.
**How to avoid:** `useClipboardEvents.loadData` already guards against concurrent loads via `loadInFlightRef.current`. A second call during in-flight load returns early — no data race.

### Pitfall 4: `is_supported_topic` missing `FILE_TRANSFER` snapshot arm

**What goes wrong:** Adding `FILE_TRANSFER` to `is_supported_topic` without a `build_snapshot_event` arm causes panic/error on subscription.
**How to avoid:** Same as Pitfall 1 — return `Ok(None)` for file-transfer topic (no snapshot available).

## Code Examples

### Existing: Topic subscription flow (ws.rs)

```rust
// Source: src-tauri/crates/uc-daemon/src/api/ws.rs
fn normalize_topics(topics: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for topic in topics {
        if !is_supported_topic(topic.as_str()) {
            continue;  // silently drops "clipboard" today
        }
        if seen.insert(topic.clone()) {
            normalized.push(topic);
        }
    }
    normalized
}
```

### Existing: Clipboard event parsing (ws_bridge.rs)

```rust
// Source: src-tauri/crates/uc-daemon-client/src/ws_bridge.rs L754-775
ws_event::CLIPBOARD_NEW_CONTENT => {
    match serde_json::from_value::<ClipboardPayload>(event.payload) {
        Ok(payload) => Some(RealtimeEvent::ClipboardNewContent(
            ClipboardNewContentEvent {
                entry_id: payload.entry_id,
                preview: payload.preview,
                origin: payload.origin,
            },
        )),
        Err(err) => { warn!(...); None }
    }
}
```

### Existing: Clipboard realtime consumer (realtime.rs)

```rust
// Source: src-tauri/crates/uc-daemon-client/src/realtime.rs
let clipboard_rx = match bridge
    .subscribe("clipboard_realtime_consumer", &[RealtimeTopic::Clipboard])
    .await { Ok(rx) => Some(rx), Err(err) => { warn!(...); None } };

// Consumer task already spawned:
task_registry.spawn("realtime_clipboard_consumer", |_token| async move {
    let result = match clipboard_rx {
        Some(mut rx) => run_clipboard_realtime_consumer_with_rx(&mut rx, clipboard_emitter).await,
        None => run_clipboard_realtime_consumer(clipboard_bridge, clipboard_emitter).await,
    };
    if let Err(err) = result { warn!(error = %err, "clipboard realtime consumer stopped"); }
}).await;
```

### Existing: Bridge state accessor (ws_bridge.rs)

```rust
// Source: src-tauri/crates/uc-daemon-client/src/ws_bridge.rs
pub fn state(&self) -> BridgeState {
    match self.state.read() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}
// BridgeState: Disconnected | Connecting | Subscribing | Ready | Degraded
```

### Existing: Frontend loadData reset (useClipboardEvents.ts)

```typescript
// Source: src/hooks/useClipboardEvents.ts
onRemoteInvalidate: () => {
  void loadData({ specificFilter: currentFilterRef.current, reset: true })
},
```

## State of the Art

| Component                             | Status      | Notes                                                                |
| ------------------------------------- | ----------- | -------------------------------------------------------------------- |
| Daemon clipboard capture → WS event   | Working     | `clipboard_watcher.rs` broadcasts on `clipboard` topic               |
| GUI WS bridge event parsing           | Working     | `map_daemon_ws_event` handles `clipboard.new_content`                |
| GUI clipboard realtime consumer       | Working     | `run_clipboard_realtime_consumer_with_rx` in `realtime.rs`           |
| Tauri `clipboard://event` emission    | Working     | `TauriEventEmitter` handles `ClipboardHostEvent::NewContent`         |
| Frontend `useClipboardEventStream`    | Working     | Listens on `clipboard://event`, calls onLocalItem/onRemoteInvalidate |
| `is_supported_topic("clipboard")`     | **BROKEN**  | Missing — root cause                                                 |
| `is_supported_topic("file-transfer")` | **BROKEN**  | Missing — D-03                                                       |
| Reconnect refetch                     | **MISSING** | No mechanism to detect bridge reconnect and trigger refetch          |

## Open Questions

1. **Should `file-transfer` events flow through `DaemonWsBridge`/`RealtimeTopic` eventually?**
   - What we know: Currently file-transfer events only flow via `DaemonApiEventEmitter` → `HostEvent::Transfer` → `TauriEventEmitter`. `RealtimeTopic` has no `FileTransfer` variant.
   - What's unclear: Is there a future need to subscribe to file-transfer via the WS bridge from GUI?
   - Recommendation: D-03 only requires adding to `is_supported_topic()`. Don't add `RealtimeTopic::FileTransfer` unless there's an active consumer — over-engineering.

2. **Reconnect signal: new HostEvent variant vs. monitor task approach?**
   - What we know: Both approaches work. Monitor task requires no API changes. New HostEvent variant is more formal.
   - What's unclear: Whether other features will need reconnect notification.
   - Recommendation: Monitor task (Option C) for now — simpler, minimal surface area change.

## Environment Availability

Step 2.6: SKIPPED — this phase is purely code changes with no new external dependencies. All required tools (cargo, bun, TypeScript) are part of the existing development environment.

## Validation Architecture

### Test Framework

| Property           | Value                                     |
| ------------------ | ----------------------------------------- |
| Rust framework     | `cargo test` (built-in)                   |
| Frontend framework | Vitest (`bun test`)                       |
| Rust quick run     | `cd src-tauri && cargo test -p uc-daemon` |
| Rust full suite    | `cd src-tauri && cargo test`              |
| Frontend quick run | `bun test -- src/hooks`                   |

### Phase Requirements → Test Map

| Behavior                                                     | Test Type    | Automated Command                                              | Notes                                                  |
| ------------------------------------------------------------ | ------------ | -------------------------------------------------------------- | ------------------------------------------------------ |
| `is_supported_topic("clipboard")` returns true               | unit         | `cd src-tauri && cargo test -p uc-daemon normalize_topics`     | New test in `ws.rs`                                    |
| `is_supported_topic("file-transfer")` returns true           | unit         | `cd src-tauri && cargo test -p uc-daemon normalize_topics`     | Same test file                                         |
| `build_snapshot_event` returns Ok(None) for clipboard topic  | unit         | `cd src-tauri && cargo test -p uc-daemon build_snapshot`       | New test in `ws.rs`                                    |
| `DaemonWsBridge` subscribes to clipboard and delivers events | integration  | `cd src-tauri && cargo test -p uc-daemon-client`               | Existing `ScriptedDaemonWsConnector` harness available |
| Reconnect triggers Dashboard refetch                         | manual/smoke | Run daemon mode, kill/restart daemon, verify Dashboard updates | No automated test                                      |

### Wave 0 Gaps

- [ ] `uc-daemon/src/api/ws.rs` — add `#[cfg(test)]` unit tests for `is_supported_topic` covering all registered topics including `clipboard` and `file-transfer`
- [ ] `uc-daemon/src/api/ws.rs` — add test for `build_snapshot_event` returning `Ok(None)` for newly added topics

_(Existing integration test infrastructure in `uc-daemon-client` via `ScriptedDaemonWsConnector` covers the WS bridge delivery chain.)_

## Sources

### Primary (HIGH confidence)

- Direct code inspection: `src-tauri/crates/uc-daemon/src/api/ws.rs` — confirmed `is_supported_topic()` missing `clipboard` and `file-transfer`
- Direct code inspection: `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — confirmed `ws_topic::CLIPBOARD = "clipboard"` and `ws_topic::FILE_TRANSFER = "file-transfer"` are defined
- Direct code inspection: `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` — confirmed `map_daemon_ws_event` handles `CLIPBOARD_NEW_CONTENT`, `topic_name` maps `Clipboard → ws_topic::CLIPBOARD`
- Direct code inspection: `src-tauri/crates/uc-daemon-client/src/realtime.rs` — confirmed clipboard consumer task is already spawned in `start_realtime_runtime`
- Direct code inspection: `src/hooks/useClipboardEventStream.ts` — confirmed listens on `clipboard://event`
- Direct code inspection: `src/hooks/useClipboardEvents.ts` — confirmed `loadData({ reset: true })` available for reconnect refetch

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` — confirmed `file-transfer.status_changed` events broadcast on `ws_topic::FILE_TRANSFER` by daemon

## Metadata

**Confidence breakdown:**

- Root cause identification: HIGH — confirmed by direct code inspection of `is_supported_topic()`
- Full chain audit: HIGH — all RealtimeEvent variants traced through both bridge and server code
- Reconnection compensation approach: MEDIUM — multiple valid options; Option C recommended but implementation details at Claude's discretion
- No-op nature of fix (one-liner + snapshot arm): HIGH — code paths verified end-to-end

**Research date:** 2026-03-27
**Valid until:** 2026-04-27 (stable code, no external dependencies)
