# Phase 65: Remove GUI Clipboard Watcher — Research

**Researched:** 2026-03-26
**Domain:** Dead-code removal — Rust Tauri codebase, uc-platform / uc-bootstrap / uc-tauri / uc-app crates
**Confidence:** HIGH

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Delete `PlatformRuntime<E>` struct entirely from `uc-platform/runtime/runtime.rs`. In Passive mode it runs an empty event loop — it is dead code.
- **D-02:** Delete `SimplePlatformCommandExecutor` and all PlatformRuntime creation/startup from `main.rs`. The `platform_runtime.start().await` call that currently blocks the init task is removed.
- **D-03:** Delete `PlatformCommandExecutorPort` trait from `uc-platform/ports/`.
- **D-04:** Delete `PlatformCommand` enum and `PlatformEvent` enum from `uc-platform/ipc/`.
- **D-05:** Delete event bus module (`uc-platform/runtime/event_bus.rs`) and channel types.
- **D-06:** Delete `WatcherControlPort` trait and `InMemoryWatcherControl` adapter.
- **D-07:** Delete `StartClipboardWatcher` use case from `uc-platform/usecases/`.
- **D-08:** Delete `StartClipboardWatcherPort` trait and `StartClipboardWatcherError` from `uc-core/ports/`. Remove re-exports from `uc-app/usecases/mod.rs`.
- **D-09:** Remove watcher field and step 2 from `AppLifecycleCoordinator`. Remove `LifecycleState::WatcherFailed` variant and `LifecycleEvent::WatcherFailed` variant.
- **D-10:** Remove `AppRuntime.watcher_control` field, `NoopWatcherControl` inline struct, and `UseCases::start_clipboard_watcher()` accessor from `uc-tauri/bootstrap/runtime.rs`.
- **D-11:** Remove `watcher_control` from `PlatformLayer` struct and `SetupAssemblyPorts` in `uc-bootstrap/assembly.rs`. Remove watcher parameter from `build_setup_orchestrator()`.
- **D-12:** Remove `platform_event_tx/rx` and `platform_cmd_tx/rx` channel creation from `builders.rs`. Remove corresponding fields from `GuiBootstrapContext` and `DaemonBootstrapContext`.
- **D-13:** Keep `clipboard_rs` in `uc-platform/Cargo.toml`. The `uc-platform/clipboard/` module is still used by daemon.
- **D-14:** Remove `ClipboardChangeHandler` impl on `AppRuntime` (trait impl block). The `clipboard_handler: Arc<dyn ClipboardChangeHandler>` wiring in `main.rs` is deleted.
- **D-15:** Keep the `ClipboardChangeHandler` trait itself in `uc-core/ports/` — daemon uses it.

### Claude's Discretion

- Whether to keep or remove `uc-platform/runtime/` module entirely (if only PlatformRuntime was in it)
- Whether `uc-platform/ipc/` module can be fully deleted or needs partial retention
- Test file cleanup scope (runtime_test.rs in uc-platform/tests/)
- Exact order of file deletions to keep compilation green at each step

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

---

## Summary

Phase 65 is a pure dead-code removal phase with no user-visible behavior changes. The GUI process (`uc-tauri`) previously owned a `PlatformRuntime<E>` event loop that started the system clipboard watcher and routed `PlatformEvent::ClipboardChanged` through `ClipboardChangeHandler` to `AppRuntime`. Since Phase 57, the daemon's `ClipboardWatcherWorker` owns clipboard monitoring exclusively. The GUI has `ClipboardIntegrationMode::Passive` hardcoded, meaning `StartClipboardWatcher` is always a no-op, and `PlatformRuntime.start()` runs an event loop that never receives clipboard events.

All the items targeted for deletion are confirmed to exist in their expected forms. The deletion spans five crates: `uc-platform` (runtime, ipc, ports, adapters, usecases), `uc-core` (ports), `uc-app` (usecases lifecycle coordinator), `uc-bootstrap` (assembly, builders, non_gui_runtime), and `uc-tauri` (runtime, main.rs). Test files in `uc-platform/tests/` and `uc-app/tests/` reference the deleted items and must be cleaned up.

**Primary recommendation:** Follow a bottom-up deletion order: delete leaf files first (runtime_test.rs, watcher_control_test.rs, in_memory_watcher_control.rs, start_clipboard_watcher.rs, event_bus.rs, runtime.rs) then work upward through ports and traits, then update build_setup_orchestrator signature, then update AppLifecycleCoordinator, and finally update main.rs and builder context structs.

---

## Standard Stack

No new dependencies required. This is a deletion phase — zero installs.

### Crates Involved (for reference)

| Crate          | Role in Phase                                                                           | Action                                            |
| -------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `uc-platform`  | Owns PlatformRuntime, WatcherControlPort, InMemoryWatcherControl, StartClipboardWatcher | Delete files, update mod.rs                       |
| `uc-core`      | Owns StartClipboardWatcherPort trait                                                    | Delete file, update ports/mod.rs                  |
| `uc-app`       | Owns AppLifecycleCoordinator (references watcher port)                                  | Remove watcher field/step                         |
| `uc-bootstrap` | Owns build_setup_orchestrator, GuiBootstrapContext, DaemonBootstrapContext              | Remove watcher_control param + channel fields     |
| `uc-tauri`     | Owns AppRuntime (watcher_control field), main.rs (PlatformRuntime creation)             | Remove watcher field, ClipboardChangeHandler impl |

---

## Architecture Patterns

### Pattern: Phase 54/58 Deletion Pattern

This project uses direct deletion with no re-export stubs (per STATE.md Phase 54 and Phase 58 decisions). When a file is deleted:

1. Delete the file
2. Remove `pub use` and `mod` declarations from parent `mod.rs`
3. Update all import paths at call sites directly

No compatibility shims. No deprecated re-exports.

### Pattern: build_setup_orchestrator Parameter Removal

`build_setup_orchestrator()` currently takes `watcher_control: Arc<dyn WatcherControlPort>` as its last parameter. Removing it cascades through:

1. `uc-bootstrap/assembly.rs` — function signature change
2. `uc-tauri/bootstrap/runtime.rs` — `AppRuntime::with_setup()` no longer passes watcher_control to build_setup_orchestrator; `AppRuntime::new()` removes `NoopWatcherControl` inline struct
3. `uc-bootstrap/non_gui_runtime.rs` — `build_non_gui_runtime_with_setup()` no longer accepts/passes watcher_control
4. `uc-app/tests/setup_flow_integration_test.rs` — test uses `NoopWatcherControl` + `StartClipboardWatcher` for `AppLifecycleCoordinator` fixture; test must be updated

### Pattern: AppLifecycleCoordinator Simplification

After removing the watcher step, the state machine becomes:

```
Idle → Pending → (Network) → (Announce) → Ready
                           ↘ NetworkFailed
```

The `watcher` field and `WatcherFailed`/`WatcherFailed` variants are removed from:

- `AppLifecycleCoordinator` struct and `AppLifecycleCoordinatorDeps` struct
- `LifecycleState` enum
- `LifecycleEvent` enum

### Recommended Deletion Order

To keep `cargo check` green at each step:

**Wave 1 — Test files first (don't affect compilation of production code):**

- `src-tauri/crates/uc-platform/tests/runtime_test.rs`
- `src-tauri/crates/uc-platform/tests/watcher_control_test.rs`

**Wave 2 — Platform implementation files:**

- `src-tauri/crates/uc-platform/src/adapters/in_memory_watcher_control.rs` (+ update `adapters/mod.rs`)
- `src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs` (+ update `usecases/mod.rs`)
- `src-tauri/crates/uc-platform/src/runtime/event_bus.rs`
- `src-tauri/crates/uc-platform/src/runtime/runtime.rs`
- `src-tauri/crates/uc-platform/src/runtime/mod.rs` (delete entire module — only contained event_bus and runtime)
- `src-tauri/crates/uc-platform/src/ipc/command.rs` + `src-tauri/crates/uc-platform/src/ipc/event.rs` + `src-tauri/crates/uc-platform/src/ipc/mod.rs` (delete all three — entire ipc module)
- `src-tauri/crates/uc-platform/src/ports/watcher_control.rs` (+ update `ports/mod.rs`)
- `src-tauri/crates/uc-platform/src/ports/command_executor.rs` (+ update `ports/mod.rs`)
- `src-tauri/crates/uc-platform/src/ports/clipboard_runtime.rs` (+ update `ports/mod.rs`) — uses PlatformEvent

**Wave 3 — Core trait:**

- `src-tauri/crates/uc-core/src/ports/start_clipboard_watcher.rs` (+ update `ports/mod.rs`)
- Update `src-tauri/crates/uc-app/src/usecases/mod.rs` — remove `StartClipboardWatcherPort`/`StartClipboardWatcherError` re-exports

**Wave 4 — AppLifecycleCoordinator:**

- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` — remove `watcher` field from struct, `WatcherFailed` from enums, step 2 from `ensure_ready()`
- Update `src-tauri/crates/uc-app/tests/app_lifecycle_coordinator_test.rs` — remove watcher mock + field from fixtures
- Update `src-tauri/crates/uc-app/tests/app_lifecycle_status_test.rs` — remove watcher mock + field from fixtures
- Update `src-tauri/crates/uc-app/tests/setup_flow_integration_test.rs` — remove `NoopWatcherControl` + `StartClipboardWatcher` from fixtures

**Wave 5 — Bootstrap and assembly:**

- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — remove `PlatformLayer.watcher_control`, remove `watcher_control` param from `build_setup_orchestrator()`, remove `PlatformCommandSender` import
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — remove `NoopWatcherControl`, remove `watcher_control` param from `build_non_gui_runtime_with_setup()`
- `src-tauri/crates/uc-bootstrap/src/builders.rs` — remove platform channel fields from `GuiBootstrapContext`, `DaemonBootstrapContext`, `build_gui_app()`, `build_daemon_app()`

**Wave 6 — uc-tauri and main.rs:**

- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — remove `watcher_control` field from `AppRuntime`, remove `NoopWatcherControl` inline struct from `AppRuntime::new()`, remove `watcher_control` param from `AppRuntime::with_setup()`, remove `start_clipboard_watcher()` accessor from `AppUseCases`, remove `ClipboardChangeHandler` impl block
- `src-tauri/src/main.rs` — remove `SimplePlatformCommandExecutor`, remove `PlatformRuntime` creation and `.start().await`, remove `clipboard_handler` wiring, remove `platform_event_tx/rx/cmd_tx/rx` destructuring

**Wave 7 — Frontend-facing models (if needed):**

- `src-tauri/crates/uc-tauri/src/models/mod.rs` — `LifecycleStatusDto::from_state()` and tests reference `WatcherFailed` variant; these must be updated after the enum change in Wave 4
- `src-tauri/crates/uc-tauri/tests/lifecycle_command_contract_test.rs` — references `LifecycleState::WatcherFailed`; update after Wave 4

### Anti-Patterns to Avoid

- **Re-export stubs:** Per project convention (Phase 54/58 decisions), never create re-export stubs for deleted items. Update import paths directly.
- **Partial `ipc/` module deletion:** The entire `uc-platform/ipc/` module can be deleted because nothing outside uc-platform imports it after PlatformRuntime and InMemoryWatcherControl are gone. Verify before deleting.
- **Forgetting ClipboardRuntimePort:** `src-tauri/crates/uc-platform/src/ports/clipboard_runtime.rs` uses `PlatformEvent`. It has zero callers (only declared in `ports/mod.rs`). It should be deleted along with the ipc module.

---

## Don't Hand-Roll

| Problem                            | Don't Build | Use Instead                                                                  |
| ---------------------------------- | ----------- | ---------------------------------------------------------------------------- |
| Tracking callers of deleted items  | Manual grep | `cargo check` — Rust compiler identifies every broken import at compile time |
| Incremental compilation validation | CI run      | `cd src-tauri && cargo check` after each wave                                |

**Key insight:** Rust's type system is the planner's best tool here. Delete a file, run `cargo check`, fix every compilation error reported. The compiler will catch every forgotten reference.

---

## Verified File States

### Files Confirmed to Exist and Match CONTEXT.md Description

| File                                                    | Status | Key Content                                                                       |
| ------------------------------------------------------- | ------ | --------------------------------------------------------------------------------- |
| `uc-platform/src/runtime/runtime.rs`                    | EXISTS | `PlatformRuntime<E>` struct with watcher logic, `ClipboardChangeHandler` callback |
| `uc-platform/src/runtime/event_bus.rs`                  | EXISTS | `PlatformEventSender/Receiver`, `PlatformCommandSender/Receiver` type aliases     |
| `uc-platform/src/runtime/mod.rs`                        | EXISTS | Only declares `event_bus` and `runtime` submodules                                |
| `uc-platform/src/ipc/command.rs`                        | EXISTS | `PlatformCommand` enum with 5 variants                                            |
| `uc-platform/src/ipc/event.rs`                          | EXISTS | `PlatformEvent` enum with 6 variants + `PlatformStatus`/`PlatformState`           |
| `uc-platform/src/ipc/mod.rs`                            | EXISTS | Re-exports `PlatformCommand`, `PlatformEvent`                                     |
| `uc-platform/src/ports/watcher_control.rs`              | EXISTS | `WatcherControlPort` trait + `WatcherControlError`                                |
| `uc-platform/src/ports/command_executor.rs`             | EXISTS | `PlatformCommandExecutorPort` trait                                               |
| `uc-platform/src/ports/clipboard_runtime.rs`            | EXISTS | `ClipboardRuntimePort` trait — uses `PlatformEvent`, zero external callers        |
| `uc-platform/src/adapters/in_memory_watcher_control.rs` | EXISTS | `InMemoryWatcherControl` struct                                                   |
| `uc-platform/src/usecases/start_clipboard_watcher.rs`   | EXISTS | `StartClipboardWatcher` use case + inline tests                                   |
| `uc-core/src/ports/start_clipboard_watcher.rs`          | EXISTS | `StartClipboardWatcherPort` trait + `StartClipboardWatcherError`                  |
| `uc-platform/tests/runtime_test.rs`                     | EXISTS | Tests for PlatformRuntime (must delete)                                           |
| `uc-platform/tests/watcher_control_test.rs`             | EXISTS | Tests for InMemoryWatcherControl (must delete)                                    |

### Files to Modify — Key Reference Points

| File                                                | What to Remove                                                                                                                                                                                                                                                                                                                                                                                                                               |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/main.rs`                             | `SimplePlatformCommandExecutor` struct (lines 46-72), `PlatformRuntime` creation (lines 614-631), `platform_runtime.start().await` (line 674), `clipboard_handler` Arc wiring (lines 358-360), `platform_event_tx/rx/cmd_tx/rx` destructuring from `GuiBootstrapContext` (lines 320-323), imports (lines 22-24)                                                                                                                              |
| `uc-tauri/bootstrap/runtime.rs`                     | `watcher_control` field on `AppRuntime` (line 151), `NoopWatcherControl` inline struct in `AppRuntime::new()` (lines 158-168), watcher_control param in `with_setup()` (line 186), `start_clipboard_watcher()` accessor on `AppUseCases` (lines 376-382), `app_lifecycle_coordinator()` inner watcher construction (lines 392-394), entire `ClipboardChangeHandler` impl block (lines 543-735)                                               |
| `uc-bootstrap/assembly.rs`                          | `PlatformLayer.watcher_control` field (line 275), `watcher_control` parameter from `build_setup_orchestrator()` signature (line 1069), `start_watcher` local var and `AppLifecycleCoordinatorDeps.watcher` field inside the function body, `PlatformCommandSender` import (line 76)                                                                                                                                                          |
| `uc-bootstrap/non_gui_runtime.rs`                   | `NoopWatcherControl` struct (lines 78-88), `watcher_control` param in `build_non_gui_runtime_with_setup()` (line 122), passing watcher_control to `build_setup_orchestrator()`                                                                                                                                                                                                                                                               |
| `uc-bootstrap/builders.rs`                          | `watcher_control` field from `GuiBootstrapContext` (line 57), `platform_event_tx/rx` and `platform_cmd_tx/rx` from `GuiBootstrapContext` (lines 60-63), `watcher_control` from `DaemonBootstrapContext` (line 86), `platform_cmd_tx/rx` and `platform_event_tx/rx` from `DaemonBootstrapContext` (lines 87-90), channel creation in `build_gui_app()` (lines 145-150), watcher_control extraction from wired in `build_gui_app()` (line 156) |
| `uc-app/src/usecases/app_lifecycle/mod.rs`          | `watcher` field from `AppLifecycleCoordinator` (line 98), `WatcherFailed` from `LifecycleState` (line 29), `WatcherFailed` from `LifecycleEvent` (line 43), `watcher` field from `AppLifecycleCoordinatorDeps` (line 108), step 2 from `ensure_ready()` (lines 178-187)                                                                                                                                                                      |
| `uc-app/src/usecases/mod.rs`                        | `pub use uc_core::ports::{StartClipboardWatcherError, StartClipboardWatcherPort};` (line 60)                                                                                                                                                                                                                                                                                                                                                 |
| `uc-tauri/models/mod.rs`                            | `WatcherFailed` references in `LifecycleStatusDto` tests (after enum change in uc-app)                                                                                                                                                                                                                                                                                                                                                       |
| `uc-tauri/tests/lifecycle_command_contract_test.rs` | `LifecycleState::WatcherFailed` test assertion (lines 31-33)                                                                                                                                                                                                                                                                                                                                                                                 |
| `uc-tauri/tests/models_serialization_test.rs`       | Any `WatcherFailed` assertions (verify)                                                                                                                                                                                                                                                                                                                                                                                                      |
| `uc-app/tests/app_lifecycle_coordinator_test.rs`    | `MockWatcherControl`, `StartClipboardWatcher` fixture usage, `watcher` field in `AppLifecycleCoordinatorDeps`                                                                                                                                                                                                                                                                                                                                |
| `uc-app/tests/app_lifecycle_status_test.rs`         | Same as above — `MockWatcherControl`, `StartClipboardWatcher`, `watcher` field                                                                                                                                                                                                                                                                                                                                                               |
| `uc-app/tests/setup_flow_integration_test.rs`       | `NoopWatcherControl` + `StartClipboardWatcher` in coordinator fixture (lines 31-37)                                                                                                                                                                                                                                                                                                                                                          |

---

## Common Pitfalls

### Pitfall 1: ClipboardRuntimePort Not In CONTEXT.md List But Depends on Deleted Types

**What goes wrong:** `uc-platform/src/ports/clipboard_runtime.rs` imports `PlatformEvent` from `uc-platform/src/ipc/`. When the ipc module is deleted, this file breaks compilation.
**Why it happens:** CONTEXT.md did not mention `ClipboardRuntimePort` explicitly because it was not directly in the deletion scope.
**How to avoid:** Delete `clipboard_runtime.rs` and remove its `pub use` from `ports/mod.rs` as part of Wave 2. It has zero external callers (verified: only `ports/mod.rs` re-exports it, no other file imports `ClipboardRuntimePort`).
**Warning signs:** `error[E0432]: unresolved import uc_platform::ipc::PlatformEvent` in `clipboard_runtime.rs` during compilation.

### Pitfall 2: LifecycleStatusDto and Tests Reference WatcherFailed Variant

**What goes wrong:** `uc-tauri/src/models/mod.rs` contains `LifecycleStatusDto` with inline tests that serialize `LifecycleState::WatcherFailed`. Two test files also check the `WatcherFailed` JSON value. After removing the variant from `LifecycleState`, these tests break.
**Why it happens:** The DTO and model tests mirror all enum variants. Removing a variant from the domain enum requires updating all serialization tests.
**How to avoid:** Update `models/mod.rs` tests and `lifecycle_command_contract_test.rs` in Wave 7, after `LifecycleState::WatcherFailed` is removed in Wave 4.
**Warning signs:** `error[E0599]: no variant or associated item named WatcherFailed found for enum LifecycleState` in test files.

### Pitfall 3: ClipboardChangeHandler Impl on AppRuntime Uses wiring_deps()

**What goes wrong:** The `ClipboardChangeHandler` impl block on `AppRuntime` (lines 543-735 of `runtime.rs`) references `self.wiring_deps()`, `self.event_emitter()`, and `self.usecases()`. After the impl block is removed in Wave 6, the import `use uc_core::ports::ClipboardChangeHandler;` at the top of `runtime.rs` becomes unused.
**Why it happens:** Rust warns on unused imports, which may become compile errors under strict settings.
**How to avoid:** Remove both the impl block AND the `ClipboardChangeHandler` import from `runtime.rs`. Also remove `use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};` and other imports only used in the impl block.
**Warning signs:** `warning: unused import: uc_core::ports::ClipboardChangeHandler` after impl deletion.

### Pitfall 4: main.rs startup_barrier Ordering

**What goes wrong:** In `main.rs`, `startup_barrier_for_backend.mark_backend_ready()` (line 636) is called BEFORE `platform_runtime.start().await` (line 674). The startup barrier logic must remain correct after removing PlatformRuntime. The barrier should still be marked before the auto-unlock task.
**Why it happens:** The task currently does: (1) create PlatformRuntime, (2) mark_backend_ready, (3) auto-unlock, (4) platform_runtime.start(). After deletion: (1) mark_backend_ready, (2) auto-unlock, and the step-4 infinite loop is gone entirely. The `tauri::async_runtime::spawn` block can be simplified to just run the mark_backend_ready + auto-unlock sequence.
**How to avoid:** Read the init task carefully and preserve the startup barrier call. Do not accidentally remove it.
**Warning signs:** App hangs on startup, `try_finish` never called.

### Pitfall 5: DaemonBootstrapContext Still Used in daemon main.rs

**What goes wrong:** `DaemonBootstrapContext` has `watcher_control`, `platform_cmd_tx/rx`, and `platform_event_tx/rx` fields. The daemon's `main.rs` destructures this context. Removing those fields from the struct requires finding all daemon call sites.
**Why it happens:** CONTEXT.md mentions `builders.rs` cleanup but does not explicitly list daemon `main.rs` as a modify target.
**How to avoid:** After updating `DaemonBootstrapContext` struct definition, run `cargo check` in `src-tauri/` to find all destructuring sites in daemon code.
**Warning signs:** `error[E0026]: struct DaemonBootstrapContext has no field named watcher_control` in daemon main.rs.

---

## Code Examples

### Current AppRuntime::with_setup() signature (to be modified)

```rust
// Current — src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs line 183
pub fn with_setup(
    deps: AppDeps,
    setup_ports: super::assembly::SetupAssemblyPorts,
    watcher_control: Arc<dyn uc_platform::ports::WatcherControlPort>,  // REMOVE
    storage_paths: uc_app::app_paths::AppPaths,
    event_emitter: Arc<dyn HostEventEmitterPort>,
) -> Self
```

After Phase 65:

```rust
pub fn with_setup(
    deps: AppDeps,
    setup_ports: super::assembly::SetupAssemblyPorts,
    storage_paths: uc_app::app_paths::AppPaths,
    event_emitter: Arc<dyn HostEventEmitterPort>,
) -> Self
```

### Current build_setup_orchestrator() signature (to be modified)

```rust
// Current — src-tauri/crates/uc-bootstrap/src/assembly.rs line 1062
pub fn build_setup_orchestrator(
    deps: &uc_app::AppDeps,
    ports: SetupAssemblyPorts,
    lifecycle_status: Arc<dyn LifecycleStatusPort>,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    clipboard_integration_mode: ClipboardIntegrationMode,
    session_ready_emitter: Arc<dyn SessionReadyEmitter>,
    watcher_control: Arc<dyn uc_platform::ports::WatcherControlPort>,  // REMOVE
) -> Arc<SetupOrchestrator>
```

### Current AppLifecycleCoordinator::ensure_ready() step 2 (to be deleted)

```rust
// uc-app/src/usecases/app_lifecycle/mod.rs lines 178-187
// 2. Start clipboard watcher
if let Err(e) = self.watcher.execute().await {
    let msg = e.to_string();
    warn!(error = %msg, "Clipboard watcher failed to start");
    self.status.set_state(LifecycleState::WatcherFailed).await?;
    self.lifecycle_emitter
        .emit_lifecycle_event(LifecycleEvent::WatcherFailed(msg.clone()))
        .await?;
    return Err(anyhow::anyhow!(msg));
}
```

### Current main.rs init task (key sections to remove)

```rust
// src-tauri/src/main.rs — items to delete from the init spawn block

// D-02: Delete SimplePlatformCommandExecutor instantiation (lines 615-616)
let executor = Arc::new(SimplePlatformCommandExecutor);

// D-02: Delete PlatformRuntime creation (lines 616-631)
let platform_runtime = match PlatformRuntime::new(...) { ... };

// D-02: Delete platform_runtime.start().await (line 674)
platform_runtime.start().await;

// D-14: Delete clipboard_handler wiring (lines 358-360, before setup block)
let clipboard_handler: Arc<dyn ClipboardChangeHandler> = runtime_for_handler.clone();
```

---

## Runtime State Inventory

This is a code/architecture cleanup phase — no runtime state is renamed or migrated.

| Category            | Items Found                                              | Action Required |
| ------------------- | -------------------------------------------------------- | --------------- |
| Stored data         | None — no database schema changes                        | None            |
| Live service config | None — no external service configuration                 | None            |
| OS-registered state | None — no process registration or task scheduler entries | None            |
| Secrets/env vars    | None — no env var names change                           | None            |
| Build artifacts     | None — no package renames                                | None            |

---

## Environment Availability

Step 2.6: SKIPPED (no external dependencies — pure code deletion with `cargo check` validation)

---

## Validation Architecture

### Test Framework

| Property           | Value                                                                             |
| ------------------ | --------------------------------------------------------------------------------- |
| Framework          | Rust built-in test + tokio-test                                                   |
| Config file        | `src-tauri/Cargo.toml` workspace                                                  |
| Quick run command  | `cd src-tauri && cargo check`                                                     |
| Full suite command | `cd src-tauri && cargo test -p uc-platform -p uc-app -p uc-tauri -p uc-bootstrap` |

### Phase Requirements → Test Map

| Behavior                                           | Test Type    | Automated Command                        | Notes                              |
| -------------------------------------------------- | ------------ | ---------------------------------------- | ---------------------------------- |
| PlatformRuntime deleted, compilation green         | Build check  | `cd src-tauri && cargo check`            | Primary validation                 |
| WatcherControlPort deleted, no callers broken      | Build check  | `cd src-tauri && cargo check`            | Rust compiler catches all          |
| AppLifecycleCoordinator works without watcher step | Unit test    | `cd src-tauri && cargo test -p uc-app`   | After updating coordinator tests   |
| LifecycleState serializes without WatcherFailed    | Unit test    | `cd src-tauri && cargo test -p uc-tauri` | lifecycle_command_contract_test    |
| GUI startup completes (backend_ready barrier)      | Manual smoke | `bun tauri dev`                          | Verify startup barrier still fires |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-platform -p uc-app -p uc-tauri -p uc-bootstrap`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

None — existing test infrastructure covers all phase requirements. No new test files needed. The phase only deletes tests.

---

## Open Questions

1. **ClipboardRuntimePort fate**
   - What we know: `clipboard_runtime.rs` declares `ClipboardRuntimePort` trait that uses `PlatformEvent`. It has zero callers in the codebase.
   - What's unclear: Whether it was intentionally kept for future use or is simply forgotten dead code.
   - Recommendation: Delete it in Wave 2 along with the `ipc/` module it depends on. It has no callers and its only dependency (`PlatformEvent`) is also being deleted.

2. **DaemonBootstrapContext channel fields — daemon main.rs call sites**
   - What we know: `DaemonBootstrapContext` has `platform_cmd_tx/rx` and `platform_event_tx/rx` and `watcher_control` fields.
   - What's unclear: Whether daemon's `main.rs` destructures these or whether the daemon already ignores them.
   - Recommendation: Run `cargo check` after removing the fields from the struct to surface all remaining destructuring patterns in daemon code.

3. **`uc-platform/src/runtime/` module — keep `mod.rs` or delete entirely**
   - What we know: `runtime/mod.rs` only declares `pub mod event_bus` and `pub mod runtime`. Both submodules are deleted in Wave 2.
   - What's unclear: Whether anything imports from `uc_platform::runtime`.
   - Recommendation: After deleting `event_bus.rs` and `runtime.rs`, delete `runtime/mod.rs` and remove `pub mod runtime;` from `uc-platform/src/lib.rs`.

---

## Sources

### Primary (HIGH confidence)

All findings verified directly from source code inspection via Read tool:

- `src-tauri/crates/uc-platform/src/runtime/runtime.rs` — PlatformRuntime confirmed
- `src-tauri/crates/uc-platform/src/runtime/event_bus.rs` — channel types confirmed
- `src-tauri/crates/uc-platform/src/ipc/command.rs` — PlatformCommand confirmed
- `src-tauri/crates/uc-platform/src/ipc/event.rs` — PlatformEvent confirmed
- `src-tauri/crates/uc-platform/src/ports/watcher_control.rs` — WatcherControlPort confirmed
- `src-tauri/crates/uc-platform/src/ports/command_executor.rs` — PlatformCommandExecutorPort confirmed
- `src-tauri/crates/uc-platform/src/ports/clipboard_runtime.rs` — ClipboardRuntimePort confirmed, zero external callers
- `src-tauri/crates/uc-platform/src/adapters/in_memory_watcher_control.rs` — InMemoryWatcherControl confirmed
- `src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs` — StartClipboardWatcher confirmed
- `src-tauri/crates/uc-core/src/ports/start_clipboard_watcher.rs` — StartClipboardWatcherPort confirmed
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — build_setup_orchestrator signature confirmed
- `src-tauri/crates/uc-bootstrap/src/builders.rs` — GuiBootstrapContext/DaemonBootstrapContext fields confirmed
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — NoopWatcherControl confirmed
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — AppRuntime watcher_control field, ClipboardChangeHandler impl confirmed
- `src-tauri/src/main.rs` — PlatformRuntime creation, SimplePlatformCommandExecutor confirmed
- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` — AppLifecycleCoordinator watcher dependency confirmed
- Test files — all confirmed to exist with specific line references

---

## Metadata

**Confidence breakdown:**

- File existence and content: HIGH — verified via Read tool on every file
- Deletion order: HIGH — derived from Rust compilation dependency graph
- Test file impact: HIGH — all test files inspected directly
- Missing callers: HIGH — grep over entire codebase confirmed no hidden importers
- ClipboardRuntimePort (not in CONTEXT.md): MEDIUM — zero callers verified, but deletion decision is Claude's discretion

**Research date:** 2026-03-26
**Valid until:** This is a stable codebase snapshot — valid indefinitely until next code change
