# Phase 12: Lifecycle Governance Baseline - Research

**Researched:** 2026-03-06
**Domain:** Tokio async task lifecycle, Tauri 2 app shutdown, Rust cancellation patterns
**Confidence:** HIGH

## Summary

Phase 12 addresses a systemic gap: the application spawns 10+ long-lived async tasks during bootstrap (wiring.rs, main.rs) with **no centralized tracking, no cancellation propagation, and no graceful shutdown coordination**. When the Tauri app exits via `.run()`, all spawned tasks are simply abandoned by the Tokio runtime. Additionally, pairing staging state lives in an unmanaged global static (`OnceLock<Mutex<HashMap>>`), and there are two divergent `EncryptionSessionPort` implementations in production code.

The codebase already uses `tokio 1.x` with `features = ["full"]` and `tokio-util 0.7` (in uc-platform), so the standard `CancellationToken` + `JoinSet` patterns are available with minimal dependency changes. The pairing stream service already demonstrates a `watch::channel(bool)` shutdown pattern that can be unified under `CancellationToken`.

**Primary recommendation:** Introduce a `TaskRegistry` holding a root `CancellationToken` and `JoinSet`, wire all spawned tasks through it, trigger cancellation from Tauri's app exit path, and replace the global staged device store with an injected `Arc<Mutex<...>>` component.

<phase_requirements>

## Phase Requirements

| ID      | Description                                                               | Research Support                                                                                                 |
| ------- | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| LIFE-01 | App close/restart does not leave orphaned sync/pairing tasks              | TaskRegistry with CancellationToken propagation + bounded join on shutdown                                       |
| LIFE-02 | Spawned workers are tracked and shutdown with bounded cancellation + join | JoinSet tracks all spawned handles; shutdown awaits join with timeout                                            |
| LIFE-03 | Staging/session state is lifecycle-owned, not unsafe globals              | Replace `staged_paired_device_store` global static with injected `Arc<Mutex<HashMap>>` in AppDeps                |
| LIFE-04 | Encryption/session behavior has one authoritative implementation path     | Remove `uc-infra::InMemoryEncryptionSession`, keep `uc-platform::InMemoryEncryptionSessionPort` as single source |

</phase_requirements>

## Standard Stack

### Core

| Library    | Version                      | Purpose                                      | Why Standard                                                                    |
| ---------- | ---------------------------- | -------------------------------------------- | ------------------------------------------------------------------------------- |
| tokio      | 1.x (already present)        | Async runtime, `JoinSet`, `JoinHandle`       | Already used; `JoinSet` is the idiomatic Tokio way to track spawned tasks       |
| tokio-util | 0.7 (already in uc-platform) | `CancellationToken` for cooperative shutdown | Standard Tokio ecosystem pattern; child tokens enable hierarchical cancellation |

### Supporting

| Library | Version           | Purpose                                  | When to Use                                           |
| ------- | ----------------- | ---------------------------------------- | ----------------------------------------------------- |
| tracing | (already present) | Structured logging for shutdown sequence | Log task cancellation, join results, timeout warnings |

### Alternatives Considered

| Instead of                    | Could Use                          | Tradeoff                                                                                                                                       |
| ----------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| CancellationToken             | `watch::channel(bool)`             | Already used in pairing_stream; CancellationToken is more ergonomic (`.cancelled()` future, child tokens) and is the Tokio-recommended pattern |
| JoinSet                       | Vec<JoinHandle> + manual join      | JoinSet handles dynamic task addition/removal, auto-cleanup of finished tasks                                                                  |
| Manual shutdown orchestration | `tokio::signal::ctrl_c` everywhere | Current approach; fragile, not triggered by Tauri app exit                                                                                     |

**Installation:**

```toml
# In uc-platform/Cargo.toml, ADD "sync" feature to tokio-util:
tokio-util = { version = "0.7", features = ["io", "io-util", "compat", "sync"] }

# In uc-tauri/Cargo.toml, ADD tokio-util dependency:
tokio-util = { version = "0.7", features = ["sync"] }
```

## Architecture Patterns

### Recommended Task Registry Structure

```
src-tauri/crates/uc-tauri/src/bootstrap/
├── task_registry.rs    # TaskRegistry: CancellationToken + JoinSet wrapper
├── runtime.rs          # AppRuntime gains shutdown() method
└── wiring.rs           # All spawn() calls go through TaskRegistry
```

### Pattern 1: TaskRegistry with CancellationToken

**What:** A centralized struct that owns a root `CancellationToken` and a `JoinSet<()>`, providing `spawn()` and `shutdown()` methods.
**When to use:** Every long-lived async task spawned during bootstrap.
**Example:**

```rust
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use std::time::Duration;

pub struct TaskRegistry {
    token: CancellationToken,
    tasks: tokio::sync::Mutex<JoinSet<()>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: tokio::sync::Mutex::new(JoinSet::new()),
        }
    }

    /// Get a child token for a spawned task to select! on
    pub fn child_token(&self) -> CancellationToken {
        self.token.child_token()
    }

    /// Spawn a tracked task that receives a CancellationToken
    pub async fn spawn<F, Fut>(&self, name: &'static str, f: F)
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let token = self.token.child_token();
        let mut tasks = self.tasks.lock().await;
        tasks.spawn(async move {
            f(token).await;
            tracing::debug!(task = name, "Task completed");
        });
    }

    /// Cancel all tasks and join with timeout
    pub async fn shutdown(&self, timeout_duration: Duration) {
        tracing::info!("Initiating graceful shutdown");
        self.token.cancel();

        let mut tasks = self.tasks.lock().await;
        let deadline = tokio::time::sleep(timeout_duration);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                result = tasks.join_next() => {
                    match result {
                        Some(Ok(())) => {},
                        Some(Err(e)) => tracing::warn!(error = %e, "Task join error"),
                        None => {
                            tracing::info!("All tasks joined cleanly");
                            return;
                        }
                    }
                }
                _ = &mut deadline => {
                    tracing::warn!(remaining = tasks.len(), "Shutdown timeout reached, aborting remaining tasks");
                    tasks.abort_all();
                    return;
                }
            }
        }
    }
}
```

### Pattern 2: Task-side Cooperative Cancellation

**What:** Each spawned task uses `tokio::select!` with `token.cancelled()` to exit gracefully.
**When to use:** Every loop-based task (clipboard receive, pairing event loop, spooler, janitor).
**Example:**

```rust
// Before (current code in wiring.rs):
async_runtime::spawn(async move {
    loop {
        interval.tick().await;
        janitor.run_once().await;
    }
});

// After:
registry.spawn("spool_janitor", |token| async move {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                tracing::info!("Spool janitor shutting down");
                return;
            }
            _ = interval.tick() => {
                let _ = janitor.run_once().await;
            }
        }
    }
}).await;
```

### Pattern 3: Injected Staging State (replacing global static)

**What:** Replace `staged_paired_device_store` global `OnceLock<Mutex<HashMap>>` with an `Arc<Mutex<HashMap>>` passed through dependency injection.
**When to use:** For LIFE-03 - any mutable state that was previously a global static.
**Example:**

```rust
// Before (global static):
static STAGED_PAIRED_DEVICES: OnceLock<Mutex<HashMap<String, PairedDevice>>> = OnceLock::new();

// After (injected component):
pub struct StagedPairedDeviceStore {
    devices: std::sync::Mutex<HashMap<String, PairedDevice>>,
}

impl StagedPairedDeviceStore {
    pub fn new() -> Self { ... }
    pub fn stage(&self, session_id: &str, device: PairedDevice) { ... }
    pub fn take_by_peer_id(&self, peer_id: &str) -> Option<PairedDevice> { ... }
    pub fn clear(&self) { ... }  // Called during shutdown
}
// Wire as Arc<StagedPairedDeviceStore> in AppDeps
```

### Anti-Patterns to Avoid

- **Fire-and-forget spawn without tracking:** Every `tokio::spawn` / `async_runtime::spawn` in production code MUST go through TaskRegistry
- **Relying on channel closure for shutdown:** Channels close when senders drop, but this is non-deterministic; use CancellationToken for explicit signal
- **Blocking in shutdown path:** Shutdown must use async join with timeout, never `block_on` in async context
- **Adding CancellationToken to domain ports:** The token is a runtime/infra concern; domain code should not depend on `tokio-util`

## Don't Hand-Roll

| Problem                     | Don't Build                                   | Use Instead                                 | Why                                                                          |
| --------------------------- | --------------------------------------------- | ------------------------------------------- | ---------------------------------------------------------------------------- |
| Task cancellation signaling | Custom `watch::channel(bool)` pattern         | `tokio_util::sync::CancellationToken`       | Built-in child token hierarchy, `.cancelled()` future, no boolean ambiguity  |
| Task set management         | `Vec<JoinHandle<()>>` with manual iteration   | `tokio::task::JoinSet`                      | Handles dynamic add/remove, auto-prunes finished tasks, `join_next()` stream |
| Shutdown timeout            | Manual `tokio::time::sleep` + select per task | Single `shutdown(Duration)` on TaskRegistry | Centralized timeout policy, consistent logging                               |

**Key insight:** Tokio's ecosystem already provides `CancellationToken` + `JoinSet` as the standard composition for lifecycle management. The project's existing `watch::channel(bool)` pattern in pairing_stream is a less ergonomic version of the same idea.

## Common Pitfalls

### Pitfall 1: Forgetting to Wire Cancellation Through Nested Spawns

**What goes wrong:** Wiring.rs has nested `async_runtime::spawn` inside other spawns (e.g., spool scanner spawns spooler which spawns worker). If only the outer task gets a token, inner tasks are orphaned.
**Why it happens:** Developers pass the token to the outer closure but forget inner spawns need their own child tokens.
**How to avoid:** Every `spawn()` call, including nested ones, must go through TaskRegistry. Review all ~8 production spawn sites in wiring.rs.
**Warning signs:** Tasks still running after `shutdown()` returns; log messages appearing after "All tasks joined cleanly".

### Pitfall 2: Deadlock in Shutdown if Tasks Hold Shared Locks

**What goes wrong:** A task holds a `Mutex` lock when cancellation fires; the shutdown code tries to acquire the same lock.
**Why it happens:** `select!` with `token.cancelled()` drops the future mid-execution, but `Mutex` guards are released on drop (this is safe for `std::sync::Mutex` and `tokio::sync::Mutex`). The real risk is if shutdown code calls methods that also lock.
**How to avoid:** Shutdown path should only call `TaskRegistry::shutdown()` and then clean up state that no longer has contending tasks.
**Warning signs:** Shutdown hangs indefinitely (the timeout handles this).

### Pitfall 3: Tauri `.run()` Does Not Trigger Async Cleanup

**What goes wrong:** `Builder::run()` blocks until the app exits. The Tokio runtime drops after `.run()` returns, aborting all pending tasks without graceful shutdown.
**Why it happens:** Tauri's `.run()` owns the event loop. There is no built-in "before-exit" async hook.
**How to avoid:** Use Tauri's `RunEvent::ExitRequested` or `RunEvent::Exit` callback (via `.build()` + manual `.run()` instead of builder `.run()`) to trigger `TaskRegistry::shutdown()`. Alternatively, hook into `on_window_event` with `WindowEvent::Destroyed` for the main window.
**Warning signs:** Log lines like "Platform runtime task ended" never appear.

### Pitfall 4: The Two EncryptionSession Implementations

**What goes wrong:** `uc-infra::InMemoryEncryptionSession` (uses tokio `RwLock`) and `uc-platform::InMemoryEncryptionSessionPort` (uses std `Arc<Mutex>`) both exist. Only the platform one is wired, but the infra one could be accidentally used.
**Why it happens:** Historical code duplication during the architecture migration.
**How to avoid:** Remove `uc-infra::InMemoryEncryptionSession` entirely. Keep `uc-platform::InMemoryEncryptionSessionPort` as the single authoritative implementation.
**Warning signs:** Tests that use the wrong implementation pass but production behavior differs.

## Code Examples

### Current Spawn Sites in Production (wiring.rs + main.rs)

These are the spawn calls that must be migrated to TaskRegistry:

```
wiring.rs:1222  async_runtime::spawn → Spool scanner + nested spooler + worker + janitor (4 tasks)
wiring.rs:1283  async_runtime::spawn → Space access completion loop
wiring.rs:1297  async_runtime::spawn → Clipboard receive loop (with retry backoff)
wiring.rs:1389  async_runtime::spawn → Pairing event/action loops (nested spawn at 1396)
main.rs:613     async_runtime::spawn → UC protocol request handler (per-request, short-lived - OK)
main.rs:711     async_runtime::spawn → Backend initialization + PlatformRuntime.start()
main.rs:753     async_runtime::spawn → Auto-unlock encryption session
runtime.rs:1002 async_runtime::spawn → Outbound clipboard sync (per-event, short-lived - OK)
```

Long-lived tasks to track: ~7-8 (excluding per-request/per-event short-lived spawns).

### Tauri App Exit Hook Pattern

```rust
// Instead of:
builder.run(tauri::generate_context!()).expect("error while running tauri application");

// Use:
let app = builder.build(tauri::generate_context!()).expect("error building app");
app.run(|app_handle, event| {
    if let tauri::RunEvent::ExitRequested { .. } = event {
        // Trigger graceful shutdown
        let registry = app_handle.state::<Arc<TaskRegistry>>();
        // Note: This callback is sync; need to use block_on or pre-signal
        registry.token().cancel();
    }
    if let tauri::RunEvent::Exit = event {
        // Final cleanup after all windows closed
        tracing::info!("Application exiting");
    }
});
```

## State of the Art

| Old Approach                        | Current Approach                          | When Changed                       | Impact                                                      |
| ----------------------------------- | ----------------------------------------- | ---------------------------------- | ----------------------------------------------------------- |
| `watch::channel(bool)` for shutdown | `CancellationToken` from tokio-util       | tokio-util 0.7 (stable since 2022) | Hierarchical cancellation, child tokens, more ergonomic API |
| Manual `Vec<JoinHandle>`            | `JoinSet` from tokio                      | tokio 1.21+ (2022)                 | Dynamic task tracking, `join_next()` stream, auto-cleanup   |
| `.run()` on Tauri builder           | `.build()` + `.run()` for lifecycle hooks | Tauri 2.0                          | Access to `RunEvent::ExitRequested` and `RunEvent::Exit`    |

## Open Questions

1. **PlatformRuntime.start() loop shutdown**
   - What we know: PlatformRuntime has a `PlatformCommand::Shutdown` variant and a `shutting_down` flag
   - What's unclear: Whether the platform command sender is kept accessible for shutdown signaling from the exit hook
   - Recommendation: Ensure `platform_cmd_tx` is stored in a shutdown-accessible location (TaskRegistry or AppRuntime)

2. **Short-lived per-event spawns**
   - What we know: `runtime.rs:1002` spawns outbound sync per clipboard change; `main.rs:613` handles per-request UC protocol
   - What's unclear: Whether these need tracking or can remain fire-and-forget (they complete quickly)
   - Recommendation: Short-lived tasks (<1s expected) can remain untracked, but should respect a global cancellation token to avoid starting new work during shutdown

## Validation Architecture

### Test Framework

| Property           | Value                                                           |
| ------------------ | --------------------------------------------------------------- |
| Framework          | cargo test (Rust, built-in)                                     |
| Config file        | src-tauri/Cargo.toml (workspace)                                |
| Quick run command  | `cd src-tauri && cargo test -p uc-tauri --lib -- task_registry` |
| Full suite command | `cd src-tauri && cargo test`                                    |

### Phase Requirements -> Test Map

| Req ID  | Behavior                                                    | Test Type   | Automated Command                                                                               | File Exists?                        |
| ------- | ----------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------- | ----------------------------------- |
| LIFE-01 | Shutdown cancels all tracked tasks                          | unit        | `cd src-tauri && cargo test -p uc-tauri -- task_registry::tests::shutdown_cancels_all`          | No - Wave 0                         |
| LIFE-02 | JoinSet tracks spawned tasks, timeout aborts stragglers     | unit        | `cd src-tauri && cargo test -p uc-tauri -- task_registry::tests::timeout_aborts`                | No - Wave 0                         |
| LIFE-03 | Staged device store is injected, not global                 | unit        | `cd src-tauri && cargo test -p uc-app -- staged_paired_device_store::tests`                     | Partial (existing tests use global) |
| LIFE-04 | Only one EncryptionSessionPort impl exists in non-test code | manual-only | `grep -r "impl EncryptionSessionPort" crates/ --include="*.rs" \| grep -v test \| grep -v mock` | N/A                                 |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-tauri --lib`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/task_registry.rs` -- new file with TaskRegistry + unit tests
- [ ] Add `tokio-util = { version = "0.7", features = ["sync"] }` to uc-tauri/Cargo.toml

## Sources

### Primary (HIGH confidence)

- Codebase analysis: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - all spawn sites identified
- Codebase analysis: `src-tauri/crates/uc-app/src/usecases/pairing/staged_paired_device_store.rs` - global static identified
- Codebase analysis: `src-tauri/crates/uc-platform/src/adapters/encryption.rs` vs `src-tauri/crates/uc-infra/src/security/encryption_session.rs` - dual implementation confirmed
- Codebase analysis: `src-tauri/src/main.rs` - app lifecycle, `.run()` call at line 846
- tokio-util CancellationToken: standard Tokio ecosystem pattern (tokio-util 0.7.x)
- tokio JoinSet: stable since tokio 1.21

### Secondary (MEDIUM confidence)

- Tauri 2 RunEvent lifecycle: `.build()` + `.run()` pattern for exit hooks (from Tauri 2 docs)

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - all libraries already in use, just need feature flags
- Architecture: HIGH - patterns are well-established Tokio idioms; codebase spawn sites fully audited
- Pitfalls: HIGH - based on direct codebase analysis of actual code paths

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable domain, patterns won't change)
