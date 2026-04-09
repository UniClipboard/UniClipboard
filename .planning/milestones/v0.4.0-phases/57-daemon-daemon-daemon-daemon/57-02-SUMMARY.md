---
phase: 57-daemon-daemon-daemon-daemon
plan: 02
subsystem: daemon-clipboard-realtime-pipeline
tags: [clipboard, realtime, websocket, passive-mode, daemon-client]
dependency_graph:
  requires:
    - 57-01 (daemon clipboard watcher that emits clipboard.new_content WS events)
  provides:
    - Clipboard realtime pipeline: daemon WS event -> RealtimeEvent -> HostEvent -> clipboard://event
    - GUI ClipboardIntegrationMode::Passive (daemon is sole clipboard observer)
  affects:
    - uc-core/ports/realtime.rs (new Clipboard topic and ClipboardNewContent event)
    - uc-daemon-client/ws_bridge.rs (clipboard.new_content translation)
    - uc-daemon-client/realtime.rs (clipboard consumer task)
    - uc-tauri/bootstrap/runtime.rs (GUI forced to Passive mode)
    - uc-tauri/adapters/host_event_emitter.rs (Clipboard topic string mapping)
tech_stack:
  added: []
  patterns:
    - clipboard realtime consumer following existing pairing/peers/setup consumer pattern
    - Passive clipboard integration: GUI receives events from daemon, not local OS clipboard
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/ports/realtime.rs
    - src-tauri/crates/uc-daemon-client/src/ws_bridge.rs
    - src-tauri/crates/uc-daemon-client/src/realtime.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs
decisions:
  - Phase 57-02: realtime_topic_to_str in uc-tauri host_event_emitter.rs required update for Clipboard variant (Rule 3: auto-fix blocking exhaustive match)
  - Phase 57-02: GUI ClipboardIntegrationMode hardcoded to Passive directly in AppRuntime::with_setup() call site, not via a new function — single-line change at the precise decision point
metrics:
  duration: 5min
  completed: 2026-03-25
  tasks: 2
  files: 5
---

# Phase 57 Plan 02: Wire Daemon Clipboard Events to Frontend Summary

GUI wires clipboard events from daemon WebSocket bridge into frontend clipboard://event stream, with GUI clipboard watcher disabled via Passive mode.

## Tasks Completed

### Task 1: Add Clipboard topic and ClipboardNewContent event to realtime types

Added to `uc-core/src/ports/realtime.rs`:

- `Clipboard` variant in `RealtimeTopic` enum
- `ClipboardNewContentEvent` struct with `entry_id: String`, `preview: String`, `origin: String` fields
- `ClipboardNewContent(ClipboardNewContentEvent)` variant in `RealtimeEvent` enum

Commit: `5c9f423b`

### Task 2: Wire DaemonWsBridge translation, clipboard consumer, and GUI Passive mode

**ws_bridge.rs changes:**

- Added `ClipboardNewContentEvent` import
- Added `ws_event::CLIPBOARD_NEW_CONTENT` match arm in `map_daemon_ws_event()` — deserializes camelCase JSON payload into `ClipboardNewContentEvent`
- Added `RealtimeEvent::ClipboardNewContent(_) => RealtimeTopic::Clipboard` in `event_topic()`
- Added `RealtimeTopic::Clipboard => ws_topic::CLIPBOARD` in `topic_name()`

**realtime.rs changes:**

- Added `ClipboardHostEvent`, `ClipboardOriginKind`, `HostEvent`, `ClipboardNewContentEvent` imports
- Added clipboard subscription: `bridge.subscribe("clipboard_realtime_consumer", &[RealtimeTopic::Clipboard])`
- Added `realtime_clipboard_consumer` task spawn following existing pairing/peers consumer pattern
- Implemented `run_clipboard_realtime_consumer` and `run_clipboard_realtime_consumer_with_rx` functions
- Consumer translates `RealtimeEvent::ClipboardNewContent` to `HostEvent::Clipboard(ClipboardHostEvent::NewContent{...})`
- Origin "remote" maps to `ClipboardOriginKind::Remote`, everything else maps to `Local`

**runtime.rs change:**

- `AppRuntime::with_setup()` no longer calls `super::resolve_clipboard_integration_mode()`
- Hardcoded `clipboard_integration_mode = uc_core::clipboard::ClipboardIntegrationMode::Passive`
- Comment explains: "GUI always runs in Passive mode — daemon is the sole clipboard observer (Phase 57, D-01)"

Commit: `fd743682`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added Clipboard variant to realtime_topic_to_str in host_event_emitter.rs**

- **Found during:** Task 2 compilation
- **Issue:** `uc-tauri/src/adapters/host_event_emitter.rs` has an exhaustive match on `RealtimeTopic` in `realtime_topic_to_str()` function that did not handle the new `Clipboard` variant, causing compile error E0004
- **Fix:** Added `RealtimeTopic::Clipboard => "clipboard"` arm
- **Files modified:** `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs`
- **Commit:** `fd743682` (included in same commit as Task 2)

## Known Stubs

None — all data flows are wired. The clipboard realtime consumer translates real daemon WS events into real HostEvent emissions that flow to the existing TauriEventEmitter → clipboard://event → frontend pipeline.

## Self-Check: PASSED

- realtime.rs contains `Clipboard` variant in `RealtimeTopic` and `ClipboardNewContentEvent` struct and `ClipboardNewContent` variant: FOUND
- ws_bridge.rs contains `ws_event::CLIPBOARD_NEW_CONTENT` match arm: FOUND
- realtime.rs contains `RealtimeTopic::Clipboard` subscription: FOUND
- runtime.rs contains `ClipboardIntegrationMode::Passive` for GUI: FOUND
- Commit 5c9f423b exists: FOUND
- Commit fd743682 exists: FOUND
