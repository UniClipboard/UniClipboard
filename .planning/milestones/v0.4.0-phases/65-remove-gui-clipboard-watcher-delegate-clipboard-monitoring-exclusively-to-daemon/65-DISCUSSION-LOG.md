# Phase 65: Remove GUI clipboard watcher — Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-26
**Phase:** 65-remove-gui-clipboard-watcher-delegate-clipboard-monitoring-exclusively-to-daemon
**Areas discussed:** PlatformRuntime disposal, Port/trait cleanup boundary, clipboard_rs dependency ownership

---

## PlatformRuntime Disposal

| Option                        | Description                                                                                                                                                             | Selected |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| Delete entirely (recommended) | PlatformRuntime is idle in Passive mode. Remove struct, event bus, PlatformCommand, SimplePlatformCommandExecutor. main.rs no longer starts PlatformRuntime event loop. | ✓        |
| Keep simplified version       | Keep PlatformRuntime but remove watcher code. Retain WriteClipboard/ReadClipboard as backup path.                                                                       |          |
| Claude decides                | Let Claude choose based on code analysis                                                                                                                                |          |

**User's choice:** Delete entirely (recommended)
**Notes:** User confirmed daemon has complete implementation

---

## AppLifecycleCoordinator Watcher Dependency

| Option                            | Description                                                                                                                               | Selected |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| Remove watcher step (recommended) | Remove watcher field, step 2, LifecycleState::WatcherFailed from AppLifecycleCoordinator. Daemon manages watcher lifecycle independently. | ✓        |
| Replace with no-op                | Keep StartClipboardWatcherPort trait, inject NoopStartClipboardWatcher. Keep interface unchanged.                                         |          |
| Claude decides                    | Let Claude choose based on impact analysis                                                                                                |          |

**User's choice:** Remove watcher step (recommended)
**Notes:** None

---

## clipboard_rs Dependency Ownership

| Option                            | Description                                                                                                                                                  | Selected |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------- |
| Keep in uc-platform (recommended) | LocalClipboard and ClipboardWatcher remain in uc-platform. Daemon uses them via uc-platform dependency. Only PlatformRuntime and WatcherControl are removed. | ✓        |
| Migrate to uc-daemon              | Move clipboard_rs related code from uc-platform to uc-daemon. Larger blast radius since LocalClipboard is shared.                                            |          |
| Claude decides                    | Let Claude choose based on dependency analysis                                                                                                               |          |

**User's choice:** Keep in uc-platform (recommended)
**Notes:** User noted these need to ensure daemon has complete implementation

---

## Claude's Discretion

- Whether to keep or remove uc-platform/runtime/ module entirely
- Whether uc-platform/ipc/ module can be fully deleted
- Test file cleanup scope
- Exact deletion order for compilation safety

## Deferred Ideas

None
