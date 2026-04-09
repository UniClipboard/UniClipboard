# Phase 57: Daemon Clipboard Watcher Integration - Context

**Gathered:** 2026-03-25
**Status:** Ready for planning

<domain>
## Phase Boundary

Integrate clipboard watching into the daemon process as the sole clipboard monitor. Migrate the existing `PlatformRuntime`-based clipboard watching from GUI/Tauri to the daemon's `ClipboardWatcherWorker`. After this phase, only the daemon activates OS clipboard observation; GUI operates in Passive mode and receives clipboard updates via daemon WebSocket events.

Additionally, migrate clipboard write (inbound sync → system clipboard) to daemon ownership so the daemon is the single process interacting with the OS clipboard.

</domain>

<decisions>
## Implementation Decisions

### Monitoring Ownership Migration

- **D-01:** GUI (Tauri) side sets `ClipboardIntegrationMode::Passive` — no longer starts `ClipboardWatcherContext`. The daemon is the only process that observes OS clipboard changes.
- **D-02:** The existing `ClipboardWatcherWorker` placeholder in `uc-daemon/workers/clipboard_watcher.rs` is replaced with a real implementation that uses `clipboard_rs::ClipboardWatcherContext`.
- **D-03:** PlatformRuntime's `start_clipboard_watcher()` method and the `WatcherControlPort`-based startup path in GUI are deactivated or removed for GUI mode. GUI's `PlatformRuntime` no longer owns clipboard watching.

### Event Flow Architecture

- **D-04:** Daemon captures clipboard changes via `ClipboardWatcher` (from `uc-platform/clipboard/watcher.rs`) and triggers business logic through the existing `ClipboardChangeHandler` trait (port in `uc-core`).
- **D-05:** An `AppClipboardChangeHandler` is constructed in daemon startup (similar to how `AppRuntime` builds it in GUI), calling `CaptureClipboardUseCase` to persist entries and emit events.
- **D-06:** Daemon notifies GUI of new clipboard content via the existing `DaemonWsBridge` WebSocket infrastructure — a `clipboard.new_content` event is broadcast through `event_tx`, received by `DaemonWsBridge` in the Tauri process, and translated to the existing frontend event contract.

### clipboard_rs Thread Model

- **D-07:** Use `tokio::task::spawn_blocking` to run the blocking `ClipboardWatcherContext::start_watch()` loop, matching the proven pattern from `PlatformRuntime::start_clipboard_watcher()`.
- **D-08:** `DaemonService::start()` spawns the blocking watcher, holds the `WatcherShutdown` channel, and on cancellation calls `shutdown.stop()` to cleanly exit the watcher thread.

### Clipboard Write (Inbound Sync)

- **D-09:** Daemon owns the `SystemClipboardPort` and performs `write_snapshot()` directly when receiving inbound sync content from remote peers. This ensures the daemon is the single process interacting with the OS clipboard for both reads and writes.

### Claude's Discretion

- Exact event payload structure for `clipboard.new_content` WS event (can follow existing realtime patterns)
- Whether to keep `PlatformRuntime` alive in a reduced form or further simplify it
- Error handling and retry strategy for clipboard watcher failures in daemon
- How to suppress self-triggered clipboard change events after daemon writes to clipboard (write-back loop prevention)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Clipboard Watcher Implementation

- `src-tauri/crates/uc-platform/src/clipboard/watcher.rs` — Current `ClipboardWatcher` with dedup logic, file time-window suppression
- `src-tauri/crates/uc-platform/src/runtime/runtime.rs` — `PlatformRuntime::start_clipboard_watcher()` showing spawn_blocking + WatcherShutdown pattern
- `src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs` — `StartClipboardWatcher` use case with `WatcherControlPort` and integration mode check

### Daemon Architecture

- `src-tauri/crates/uc-daemon/src/app.rs` — `DaemonApp::run()` service lifecycle, JoinSet pattern
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — Current placeholder `ClipboardWatcherWorker`
- `src-tauri/crates/uc-daemon/src/service.rs` — `DaemonService` trait definition
- `src-tauri/crates/uc-daemon/src/main.rs` — Service registration and CoreRuntime construction

### Event System

- `src-tauri/crates/uc-core/src/ports/clipboard_change_handler.rs` — `ClipboardChangeHandler` trait
- `src-tauri/crates/uc-core/src/clipboard/integration_mode.rs` — `ClipboardIntegrationMode` enum
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — `NoopWatcherControl`, `build_non_gui_runtime()`, `ClipboardIntegrationMode` resolution

### GUI Wiring

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — GUI background task wiring (inbound sync, peer events)
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — `AppRuntime` and `ClipboardChangeHandler` implementation

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `ClipboardWatcher` (`uc-platform/clipboard/watcher.rs`): Full dedup logic with hash-based and time-window suppression — reuse directly in daemon
- `ClipboardChangeHandler` trait (`uc-core/ports`): Established callback interface for clipboard change events
- `CaptureClipboardUseCase` (`uc-app/usecases/clipboard`): Business logic for persisting clipboard entries — already used by GUI
- `DaemonApiEventEmitter` (`uc-daemon/api/event_emitter.rs`): Emits `HostEvent` as `DaemonWsEvent` — already wired in daemon startup
- `DaemonWsBridge` (`uc-daemon-client`): Receives WS events and translates to Tauri frontend events

### Established Patterns

- `spawn_blocking` + `WatcherShutdown` for clipboard_rs integration (proven in `PlatformRuntime`)
- `DaemonService` trait with `start(CancellationToken)` / `stop()` / `health_check()` for daemon service lifecycle
- `broadcast::Sender<DaemonWsEvent>` for daemon-to-GUI event propagation via WebSocket
- `ClipboardIntegrationMode::Passive` disables OS clipboard observation (used in non-GUI modes)

### Integration Points

- `DaemonApp::new()` receives services vec — `ClipboardWatcherWorker` already registered
- `CoreRuntime` provides access to `CoreUseCases` for `CaptureClipboardUseCase`
- `event_tx: broadcast::Sender<DaemonWsEvent>` shared across all daemon services
- `SystemClipboardPort` available via `uc-platform/clipboard/LocalClipboard`

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches following established daemon service and event patterns.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 57-daemon-daemon-daemon-daemon_
_Context gathered: 2026-03-25_
