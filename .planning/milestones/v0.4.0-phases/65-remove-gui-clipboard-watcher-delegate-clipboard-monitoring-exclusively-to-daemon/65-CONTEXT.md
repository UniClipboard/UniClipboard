# Phase 65: Remove GUI clipboard watcher — delegate clipboard monitoring exclusively to daemon - Context

**Gathered:** 2026-03-26
**Status:** Ready for planning

<domain>
## Phase Boundary

Remove all GUI-side clipboard monitoring infrastructure now that daemon is the sole clipboard observer (Phase 57). This is a dead-code removal and architectural cleanup phase — no user-visible behavior changes.

**In scope:**

- Delete `PlatformRuntime` struct and its event loop from `uc-platform/runtime/`
- Delete `PlatformCommand` enum, `PlatformEvent` enum, event bus channels
- Delete `PlatformCommandExecutorPort` trait
- Delete `WatcherControlPort` trait + `InMemoryWatcherControl` adapter
- Delete `StartClipboardWatcher` use case from `uc-platform/usecases/`
- Delete `StartClipboardWatcherPort` trait from `uc-core/ports`
- Remove watcher step from `AppLifecycleCoordinator` (+ remove `LifecycleState::WatcherFailed`)
- Remove `AppRuntime.watcher_control` field and `NoopWatcherControl` stubs
- Remove `PlatformLayer.watcher_control` from `uc-bootstrap/assembly.rs`
- Remove platform channel creation from `builders.rs` (`platform_event_tx/rx`, `platform_cmd_tx/rx`)
- Remove `PlatformRuntime` creation and `.start().await` from `main.rs`
- Remove `SimplePlatformCommandExecutor` from `main.rs`
- Remove `ClipboardChangeHandler` callback wiring from `main.rs` (no longer needed — daemon handles capture)

**Explicitly NOT in scope:**

- `uc-platform/clipboard/` module (LocalClipboard, ClipboardWatcher) — still used by daemon
- `clipboard_rs` dependency in `uc-platform` — still needed for LocalClipboard and ClipboardWatcher
- `SystemClipboardPort` trait in `uc-core` — still used by daemon and GUI `restore_clipboard_entry`
- `ClipboardChangeHandler` trait in `uc-core/ports` — still used by daemon's `DaemonClipboardChangeHandler`
- `ClipboardIntegrationMode` in `uc-core` — still used for sync gating decisions

</domain>

<decisions>
## Implementation Decisions

### PlatformRuntime Disposal

- **D-01:** Delete `PlatformRuntime<E>` struct entirely from `uc-platform/runtime/runtime.rs`. In Passive mode it runs an empty event loop with no incoming events or commands — it is dead code.
- **D-02:** Delete `SimplePlatformCommandExecutor` and all PlatformRuntime creation/startup from `main.rs`. The `platform_runtime.start().await` call that currently blocks the init task is removed.
- **D-03:** Delete `PlatformCommandExecutorPort` trait from `uc-platform/ports/`.
- **D-04:** Delete `PlatformCommand` enum and `PlatformEvent` enum from `uc-platform/ipc/`.
- **D-05:** Delete event bus module (`uc-platform/runtime/event_bus.rs`) and channel types (`PlatformEventSender/Receiver`, `PlatformCommandSender/Receiver`).

### Port/Trait Cleanup

- **D-06:** Delete `WatcherControlPort` trait from `uc-platform/ports/watcher_control.rs` and `InMemoryWatcherControl` adapter from `uc-platform/adapters/`.
- **D-07:** Delete `StartClipboardWatcher` use case from `uc-platform/usecases/start_clipboard_watcher.rs`.
- **D-08:** Delete `StartClipboardWatcherPort` trait and `StartClipboardWatcherError` from `uc-core/ports/`. Remove re-exports from `uc-app/usecases/mod.rs`.
- **D-09:** Remove watcher field and step 2 from `AppLifecycleCoordinator`. Remove `LifecycleState::WatcherFailed` variant and `LifecycleEvent::WatcherFailed` variant. The coordinator becomes: Pending → Network → Announce → Ready.
- **D-10:** Remove `AppRuntime.watcher_control` field, `NoopWatcherControl` inline struct, and `UseCases::start_clipboard_watcher()` accessor from `uc-tauri/bootstrap/runtime.rs`.
- **D-11:** Remove `watcher_control` from `PlatformLayer` struct and `SetupAssemblyPorts` in `uc-bootstrap/assembly.rs`. Remove watcher parameter from `build_setup_orchestrator()`.
- **D-12:** Remove `platform_event_tx/rx` and `platform_cmd_tx/rx` channel creation from `builders.rs` (`build_gui_app`, `build_daemon_app`, `build_cli_context`). Remove corresponding fields from `GuiBootstrapContext` and `DaemonBootstrapContext`.

### clipboard_rs Dependency

- **D-13:** Keep `clipboard_rs` in `uc-platform/Cargo.toml`. The `uc-platform/clipboard/` module (`LocalClipboard`, `ClipboardWatcher`) is still used by daemon via `uc_platform::clipboard::*`. Only the `PlatformRuntime` and watcher control infrastructure is removed.

### ClipboardChangeHandler Callback

- **D-14:** Remove `ClipboardChangeHandler` impl on `AppRuntime` (if it exists as a trait impl). The `clipboard_handler: Arc<dyn ClipboardChangeHandler>` wiring in `main.rs` is deleted since daemon's `DaemonClipboardChangeHandler` is the sole consumer.
- **D-15:** Keep the `ClipboardChangeHandler` trait itself in `uc-core/ports/` — daemon uses it.

### Claude's Discretion

- Whether to keep or remove `uc-platform/runtime/` module entirely (if only PlatformRuntime was in it)
- Whether `uc-platform/ipc/` module can be fully deleted or needs partial retention
- Test file cleanup scope (runtime_test.rs in uc-platform/tests/)
- Exact order of file deletions to keep compilation green at each step

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Files to delete or heavily modify

- `src-tauri/crates/uc-platform/src/runtime/runtime.rs` — PlatformRuntime (DELETE entire struct + impls)
- `src-tauri/crates/uc-platform/src/runtime/event_bus.rs` — Platform channel types (DELETE if only consumer was PlatformRuntime)
- `src-tauri/crates/uc-platform/src/ipc/command.rs` — PlatformCommand enum (DELETE or strip watcher variants)
- `src-tauri/crates/uc-platform/src/ports/watcher_control.rs` — WatcherControlPort trait (DELETE)
- `src-tauri/crates/uc-platform/src/adapters/in_memory_watcher_control.rs` — InMemoryWatcherControl (DELETE)
- `src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs` — StartClipboardWatcher use case (DELETE)
- `src-tauri/crates/uc-platform/tests/runtime_test.rs` — PlatformRuntime tests (DELETE)

### Files to modify (remove watcher/platform channel references)

- `src-tauri/src/main.rs` — Remove PlatformRuntime creation, SimplePlatformCommandExecutor, clipboard_handler wiring, platform channel usage
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — Remove watcher_control field, NoopWatcherControl, start_clipboard_watcher accessor
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — Remove PlatformLayer.watcher_control, SetupAssemblyPorts watcher, build_setup_orchestrator watcher param
- `src-tauri/crates/uc-bootstrap/src/builders.rs` — Remove platform channel fields from GuiBootstrapContext/DaemonBootstrapContext
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — Remove NoopWatcherControl
- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` — Remove watcher from AppLifecycleCoordinator
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — Remove StartClipboardWatcherPort re-export
- `src-tauri/crates/uc-core/src/ports/` — Remove StartClipboardWatcherPort trait file

### Daemon references (DO NOT modify — verify completeness only)

- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — ClipboardWatcherWorker + DaemonClipboardChangeHandler (daemon's clipboard monitoring)
- `src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs` — InboundClipboardSyncWorker (daemon's inbound clipboard handling)
- `src-tauri/crates/uc-daemon/src/workers/file_sync_orchestrator.rs` — FileSyncOrchestratorWorker (daemon's file transfer + clipboard restore)

### Prior phase context

- `.planning/phases/57-daemon-daemon-daemon-daemon/57-CONTEXT.md` — Daemon clipboard watcher integration decisions
- `.planning/phases/64-tauri-sync-retirement/64-VALIDATION.md` — Tauri sync retirement validation

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- None needed — this is a deletion phase

### Established Patterns

- Phase 54/58 deletion pattern: direct delete + update imports, no re-export stubs
- Phase 64 retirement pattern: remove daemon-duplicated code from Tauri, verify daemon handles it

### Integration Points

- `main.rs` startup sequence: PlatformRuntime creation and `.start().await` must be removed without breaking the init task flow (backend_ready barrier, auto-unlock, etc.)
- `AppLifecycleCoordinator.ensure_ready()`: watcher step removal changes the state machine (Pending → Network → Ready)
- `build_setup_orchestrator()`: watcher_control parameter removal cascades through assembly.rs, runtime.rs, and SetupOrchestrator construction

### Key Insight

`PlatformRuntime` in Passive mode is a completely idle event loop — no watcher starts, no commands arrive, no events flow. The `ClipboardChangeHandler` callback on `AppRuntime` was only invoked by `PlatformRuntime` event dispatch, which never fires in Passive mode. All clipboard operations (capture, inbound sync, file restore) are now handled by daemon workers.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — standard dead code removal following established deletion patterns.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 65-remove-gui-clipboard-watcher-delegate-clipboard-monitoring-exclusively-to-daemon_
_Context gathered: 2026-03-26_
