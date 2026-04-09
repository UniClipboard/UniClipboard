---
phase: 65-remove-gui-clipboard-watcher-delegate-clipboard-monitoring-exclusively-to-daemon
plan: 02
type: execute
wave: 2
depends_on: ['65-01']
autonomous: true
completed: 2026-03-26
---

# Phase 65 Plan 02: Remove GUI Clipboard Watcher — Cascade Deletions Through Consumer Crates Summary

**Removed watcher_control from AppRuntime, bootstrap assembly, and main.rs — completing dead-code removal from Phase 65**

## Performance

- **Duration:** ~10 min (combined with Wave 1)
- **Started:** 2026-03-26T13:15:39Z
- **Completed:** 2026-03-26T22:45:00Z
- **Tasks:** 2 (Wave 1) + 1 (Wave 2 continuation)
- **Files modified:** ~19

## Accomplishments

### Wave 1 (d3972b97) — AppLifecycleCoordinator & Bootstrap Assembly
- Removed `watcher` field and `WatcherFailed` variant from `AppLifecycleCoordinator`
- Removed `watcher_control` from `WiredDependencies`, `PlatformLayer`, `build_setup_orchestrator`
- Removed platform channel fields (`platform_event_tx/rx`, `platform_cmd_tx/rx`) from `GuiBootstrapContext` and `DaemonBootstrapContext`
- Removed `NoopWatcherControl` from `non_gui_runtime.rs`
- Updated all uc-app tests and setup orchestrator to remove watcher references

### Wave 2 (72341398) — uc-tauri & main.rs
- Removed `watcher_control` field and `NoopWatcherControl` from `AppRuntime` (uc-tauri/bootstrap/runtime.rs) — 656 lines removed
- Removed `impl ClipboardChangeHandler for AppRuntime` block
- Removed `start_clipboard_watcher()` accessor from `AppUseCases`
- Removed `SimplePlatformCommandExecutor` struct from `main.rs`
- Removed `PlatformRuntime` creation and `.start().await` from `main.rs`
- Preserved `mark_backend_ready()` and startup barrier
- Wrapped `AppRuntime` in `Arc::new()` for Tauri state management
- Fixed test files across uc-daemon and uc-tauri to use updated API signatures

## Decisions Made

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Test files with 4-argument API calls**

- **Found during:** cargo check after uc-bootstrap API changes
- **Issue:** Multiple test files still called `build_non_gui_runtime_with_setup` with 4 arguments (including `watcher_control`/`NoopWatcherControl`) and `wire_dependencies_with_identity_store` with `cmd_tx` parameter
- **Fix:** Removed `watcher_control` and `cmd_tx` arguments from all test call sites; removed `NoopWatcherControl` and `NoopStartClipboardWatcher` mock structs from daemon tests
- **Files modified:** setup_api.rs, pairing_api.rs, pairing_host.rs, spool_cleanup_test.rs, bootstrap_integration_test.rs

**2. [Rule 3 - Blocking] `Arc::new(runtime)` wrapper for Tauri state management**

- **Found during:** cargo check after runtime.rs cleanup
- **Issue:** Workspace main.rs called `.manage(runtime.clone())` and `runtime.clone()` but `AppRuntime` is a plain struct (no Clone, no Arc wrapper)
- **Fix:** Added `let runtime = Arc::new(runtime);` after `AppRuntime::with_setup()` to wrap in Arc, preserving the `Arc<AppRuntime>` pattern used throughout main.rs
- **Files modified:** src-tauri/src/main.rs

## Files Modified

### Deleted (0)

No new files deleted in Wave 2.

### Modified

- `src-tauri/src/main.rs` — Remove PlatformRuntime, SimplePlatformCommandExecutor, imports, fix Arc wrapper
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — Remove watcher_control, NoopWatcherControl, ClipboardChangeHandler impl (656 lines)
- `src-tauri/crates/uc-tauri/src/models/mod.rs` — Update LifecycleStatusDto comment
- `src-tauri/crates/uc-tauri/tests/models_serialization_test.rs` — Replace WatcherFailed assertion
- `src-tauri/crates/uc-tauri/tests/spool_cleanup_test.rs` — Remove cmd_tx/mpsc args
- `src-tauri/crates/uc-tauri/tests/bootstrap_integration_test.rs` — Remove cmd_tx/mpsc args (9 locations)
- `src-tauri/crates/uc-app/src/runtime.rs` — Update comments
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — Update comment
- `src-tauri/crates/uc-daemon/tests/setup_api.rs` — Remove NoopWatcherControl, NoopStartClipboardWatcher, fix function calls
- `src-tauri/crates/uc-daemon/tests/pairing_api.rs` — Remove watcher_control arg
- `src-tauri/crates/uc-daemon/tests/pairing_host.rs` — Remove watcher_control arg

## Verification

```
$ grep -r "PlatformRuntime\|WatcherControlPort\|StartClipboardWatcherPort\|SimplePlatformCommandExecutor\|WatcherFailed" src-tauri/crates/ --include="*.rs"
(no matches)
```

- `cargo check -p uc-app -p uc-bootstrap -p uc-tauri -p uc-daemon` → exits 0
- `cargo test -p uc-app -p uc-bootstrap -p uc-tauri -p uc-daemon` → 266+ tests pass

## Deviations

None — execution followed the plan closely. Two blocking auto-fixes were necessary due to test files that called the old API.

## Next Phase Readiness

Phase 65 complete. All GUI clipboard watcher infrastructure removed. Daemon is now the sole clipboard monitor.

---
_Phase: 65-remove-gui-clipboard-watcher-delegate-clipboard-monitoring-exclusively-to-daemon_
_Completed: 2026-03-26_
