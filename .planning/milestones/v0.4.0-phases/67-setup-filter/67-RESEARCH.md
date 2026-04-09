# Phase 67: Setup Filter - Research

**Researched:** 2026-03-27
**Domain:** Daemon lifecycle — conditional PeerDiscoveryWorker startup gated on encryption state
**Confidence:** HIGH

## Summary

This phase prevents uninitialized devices from appearing in peer discovery lists by delaying `PeerDiscoveryWorker` startup until after a successful `AutoUnlockEncryptionSession`. The mechanism is purely in the daemon binary composition root (`main.rs`) and `DaemonApp::run()` — no new ports or cross-crate interfaces are needed.

The existing `recover_encryption_session` helper in `app.rs` already returns a `bool` (true = unlocked, false = uninitialized). The fix has two parts: (1) skip registering `PeerDiscoveryWorker` in the initial `services` vec when the daemon starts with `EncryptionState::Uninitialized`, and (2) add a one-shot channel so that the setup completion path (via `AppLifecycleCoordinator::ensure_ready`) can signal `DaemonApp` to spawn `PeerDiscoveryWorker` dynamically after setup finishes.

The `SetupAction::EnsureDiscovery` path (used by the Joiner flow) already calls `start_network()` directly via the `NetworkControlPort`, which is idempotent — this continues to work unchanged for Joiners.

**Primary recommendation:** In `main.rs`, capture the encryption state returned by `recover_encryption_session`; skip including `PeerDiscoveryWorker` in the services vec when state is `Uninitialized`; wire a `tokio::sync::oneshot` (or `watch`) channel from `DaemonApp` to the setup completion path so that after `MarkSetupComplete` fires, `DaemonApp` spawns `PeerDiscoveryWorker` via `tokio::spawn`.

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Daemon startup must check encryption session state BEFORE starting PeerDiscoveryWorker. If encryption session is not unlocked (i.e., `AutoUnlockEncryptionSession` fails or state is `Uninitialized`), PeerDiscoveryWorker must NOT be started.
- **D-02:** For devices that have completed setup, daemon starts PeerDiscoveryWorker normally after successful `AutoUnlockEncryptionSession`.
- **D-03:** Sponsor — libp2p network starts only AFTER setup completes and encryption session is available.
- **D-04:** Joiner — libp2p network is temporarily started during setup flow via existing `SetupAction::EnsureDiscovery` mechanism. Joiner being briefly visible to other devices during setup is acceptable.
- **D-05:** "Setup complete" is determined by encryption session being unlocked (`AutoUnlockEncryptionSession` succeeds). Reuses existing Phase 50 mechanism already in `DaemonApp::run()`.
- **D-06:** `EncryptionState::Uninitialized` (first run, no space created yet) → do NOT start network.
- **D-07:** `EncryptionState::Initialized` + successful unlock → start network.
- **D-08:** `EncryptionState::Initialized` + unlock failure → daemon already refuses to start (Phase 50 behavior, no change needed).
- **D-09:** When setup completes during a running daemon session (new space created or successfully joined), an internal event must trigger PeerDiscoveryWorker to start. Daemon should NOT need a restart.
- **D-10:** The event mechanism should notify DaemonApp to start PeerDiscoveryWorker dynamically after setup flow completes.
- **D-11:** Filtering is done at the daemon level (don't start libp2p) — NOT at the mDNS layer or business layer. This is the simplest approach.
- **D-12:** No additional business-layer filtering of discovered peers is needed for this phase.

### Claude's Discretion

- Internal event mechanism design (channel type, event structure) for notifying DaemonApp of setup completion
- Whether PeerDiscoveryWorker gets a new `start_delayed()` method or DaemonApp manages the delayed spawn externally
- How to handle the PeerDiscoveryWorker's current unconditional `start_network()` call

### Deferred Ideas (OUT OF SCOPE)

- mDNS-level selective broadcast control (only listen, don't advertise)
- Business-layer peer filtering as defense-in-depth
- "修复 setup 配对确认提示缺失" (UI-level fix)
  </user_constraints>

---

## Architecture Patterns

### Current Daemon Startup Flow (Phase 50 baseline)

```
DaemonApp::run()
  └── recover_encryption_session()        ← calls AutoUnlockEncryptionSession.execute()
        returns Ok(true)  → Initialized, session unlocked
        returns Ok(false) → Uninitialized, skip recovery
        returns Err       → Initialized but failed → daemon aborts (already handled)
  └── [all services start uniformly via JoinSet]
        includes PeerDiscoveryWorker  ← PROBLEM: always started regardless of state
```

### Target Flow After Phase 67

```
DaemonApp::run()
  └── recover_encryption_session()
        Ok(true)  → Initialized, unlocked  → include PeerDiscoveryWorker in initial services
        Ok(false) → Uninitialized           → SKIP PeerDiscoveryWorker from initial services
                                              set up deferred-start channel
  └── [services start via JoinSet]
  └── [select loop also listens on setup_complete_rx]
        on setup_complete_rx fires → tokio::spawn PeerDiscoveryWorker

Setup flow (SetupAction::MarkSetupComplete)
  └── AppLifecycleCoordinator::ensure_ready()
        └── StartNetworkAfterUnlock.execute() → start_network() (for the network layer)
        └── signals setup_complete_tx (new)   → DaemonApp receives and spawns PeerDiscoveryWorker
```

### Pattern: Deferred Worker via oneshot Channel

The cleanest approach is a `tokio::sync::oneshot::channel::<()>()` passed from `main.rs` into `DaemonApp` (or into a wrapper that `DaemonApp` exposes). The sender is passed to `AppLifecycleCoordinator` (or its downstream adapter) via a new `SessionReadyEmitter` impl that fires the oneshot on `emit_ready()`.

This avoids introducing a new global state flag and keeps the signal flow unidirectional: setup → daemon.

```rust
// main.rs: Create the deferred-start channel
let (setup_complete_tx, setup_complete_rx) = tokio::sync::oneshot::channel::<()>();

// Wire setup_complete_tx into the SessionReadyEmitter adapter
// (new adapter implementing SessionReadyEmitter that fires the oneshot)

// Pass setup_complete_rx to DaemonApp::new() (new field)

// DaemonApp::run() select loop:
tokio::select! {
    _ = wait_for_shutdown_signal() => { ... }
    result = &mut rpc_handle => { ... }
    result = &mut http_handle => { ... }
    Some(result) = service_tasks.join_next() => { ... }
    Ok(_) = &mut setup_complete_rx => {
        // spawn PeerDiscoveryWorker
        let svc = Arc::clone(&peer_discovery_worker);
        let token = self.cancel.child_token();
        service_tasks.spawn(async move { svc.start(token).await });
    }
}
```

### Alternative: watch channel

A `tokio::sync::watch::channel(false)` sender/receiver pair works if the signal needs to be re-readable or if the sender might not exist at construction time. Given this is a one-shot "setup completed" signal, `oneshot` is simpler and prevents double-firing. Use `watch` only if the `DaemonApp` needs to handle the case where setup completes before the select loop starts.

**Recommendation:** Use `tokio::sync::oneshot`. If `setup_complete_rx` is already consumed before the loop starts (setup ran very fast), the `Ok(_)` arm still fires in the select. Safe.

### PeerDiscoveryWorker Refactor: Pre-build, Conditional Register

In `main.rs`, build `PeerDiscoveryWorker` unconditionally (it is cheap to construct), but only push it into `services` if encryption is initialized:

```rust
// Build the worker
let peer_discovery = Arc::new(PeerDiscoveryWorker::new(
    daemon_network_control,
    daemon_network_events,
    daemon_peer_directory,
    daemon_settings,
));

// Conditional inclusion
let (services, deferred_peer_discovery) = if encryption_unlocked {
    (
        vec![
            ...,
            Arc::clone(&peer_discovery) as Arc<dyn DaemonService>,
        ],
        None,
    )
} else {
    (
        vec![...], // without peer_discovery
        Some(peer_discovery),
    )
};
```

`DaemonApp` receives `deferred_peer_discovery: Option<Arc<PeerDiscoveryWorker>>` and the `setup_complete_rx`. When the oneshot fires, it spawns the worker using the deferred reference.

### AppLifecycleCoordinator::ensure_ready() Integration Point

`SetupAction::MarkSetupComplete` in `action_executor.rs` (line 93-98) calls `self.app_lifecycle.ensure_ready()`. `AppLifecycleCoordinator::ensure_ready()` ends with `self.emitter.emit_ready().await?` (a `SessionReadyEmitter` call).

The cleanest integration point is a new `SessionReadyEmitter` impl that fires `setup_complete_tx` on `emit_ready()`. This avoids modifying any use-case logic — the signal is emitted as part of the existing lifecycle ready notification.

```rust
// New type in uc-daemon or uc-bootstrap (discretion):
struct SetupCompletionEmitter {
    tx: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl SetupCompletionEmitter {
    fn new(tx: tokio::sync::oneshot::Sender<()>) -> Self {
        Self { tx: tokio::sync::Mutex::new(Some(tx)) }
    }
}

#[async_trait]
impl SessionReadyEmitter for SetupCompletionEmitter {
    async fn emit_ready(&self) -> anyhow::Result<()> {
        if let Some(tx) = self.tx.lock().await.take() {
            let _ = tx.send(()); // fire-and-forget; receiver may already be gone
        }
        Ok(())
    }
}
```

### Where to put SetupCompletionEmitter

Options:

1. `uc-daemon/src/workers/peer_discovery.rs` — keeps it close to the consumer
2. `uc-daemon/src/app.rs` — alongside `DaemonApp` that consumes the signal
3. `uc-bootstrap/src/assembly.rs` — if it needs to be shared across entry points

**Recommendation:** Put it in `uc-daemon/src/app.rs` or a new `uc-daemon/src/lifecycle.rs`. The daemon binary is the only consumer and the signal is daemon-specific. No need to put it in shared crates.

---

## Don't Hand-Roll

| Problem                    | Don't Build                | Use Instead                                                     | Why                                           |
| -------------------------- | -------------------------- | --------------------------------------------------------------- | --------------------------------------------- |
| One-shot signal            | Custom AtomicBool + notify | `tokio::sync::oneshot`                                          | Built-in, type-safe, integrates with select!  |
| Deferred worker management | Worker registry, lazy init | Pre-build Arc, conditional spawn in select loop                 | Simpler, no new abstraction                   |
| Encryption state detection | New port or query          | Return value from `recover_encryption_session()` already has it | Already in `DaemonApp::run()` at line 112-114 |

---

## Common Pitfalls

### Pitfall 1: Forgetting to pass PeerDiscoveryWorker arcs to DaemonApp

**What goes wrong:** `main.rs` builds `PeerDiscoveryWorker`, puts it in `services`, passes it to `DaemonApp::new()`. If the worker is now conditionally excluded from `services`, the `Arc<PeerDiscoveryWorker>` must be passed separately to `DaemonApp` so it can spawn it later.

**How to avoid:** Add `deferred_peer_discovery: Option<Arc<dyn DaemonService>>` field to `DaemonApp`. Keep the typed `Arc<PeerDiscoveryWorker>` in `main.rs` and pass erased `Arc<dyn DaemonService>` to `DaemonApp`.

### Pitfall 2: Double-start when both the deferred path AND initial services both start the worker

**What goes wrong:** If encryption IS initialized but the setup_complete oneshot fires anyway (e.g., from a duplicate `ensure_ready` call), the worker could be spawned twice.

**How to avoid:** When `encryption_unlocked == true`, do NOT create `setup_complete_rx` at all (or create it but never connect the sender). The simplest guard: `DaemonApp` only listens on `setup_complete_rx` when `deferred_peer_discovery.is_some()`. Since `deferred_peer_discovery` is `None` when encryption is initialized, the select arm is never armed.

In code: use `futures::future::OptionFuture` or conditional future:

```rust
let deferred_fut = async {
    match setup_complete_rx {
        Some(rx) => rx.await,
        None => std::future::pending().await,
    }
};
```

### Pitfall 3: setup_complete_tx is wired into a non-daemon SessionReadyEmitter

**What goes wrong:** `AppLifecycleCoordinator` is also used in the GUI (Tauri) runtime. If the `SetupCompletionEmitter` is placed in the composition path shared with the GUI, the GUI's `ensure_ready` would fire it too.

**How to avoid:** Wire `SetupCompletionEmitter` only in `uc-daemon/src/main.rs` when building the daemon's `AppLifecycleCoordinator`. The GUI builds its own `AppLifecycleCoordinator` with a different `SessionReadyEmitter`. This is already the case — each entry point (daemon vs GUI) assembles its own coordinator.

### Pitfall 4: PeerDiscoveryWorker not in initial_statuses when deferred

**What goes wrong:** `main.rs` creates `initial_statuses` with `"peer-discovery": ServiceHealth::Healthy`. If the worker is not running initially, health reporting is misleading.

**How to avoid:** When `encryption_unlocked == false`, initialize `"peer-discovery"` as `ServiceHealth::Stopped` (or omit it entirely from the initial statuses). After the deferred spawn, update the state to `Healthy`.

### Pitfall 5: Existing tests in uc-daemon tests/ dir have pre-existing failures

**What goes wrong:** Running `cargo test -p uc-daemon` shows 5 pre-existing failures in `pairing_api.rs` (tests asserting 409/412 HTTP status codes but getting 400). These are unrelated to Phase 67 work.

**How to avoid:** Phase 67 tests should use lib tests or a separate integration test file. The pre-existing failures are not caused by Phase 67. Run only lib tests with `cargo test -p uc-daemon --lib` to get a clean baseline (72 tests, 0 failures).

---

## Code Examples

### Existing recovery_encryption_session (app.rs, line 35-55)

```rust
// Source: src-tauri/crates/uc-daemon/src/app.rs
async fn recover_encryption_session(runtime: &CoreRuntime) -> anyhow::Result<()> {
    let usecases = CoreUseCases::new(runtime);
    let uc = usecases.auto_unlock_encryption_session();
    match uc.execute().await {
        Ok(true) => {
            info!("Encryption session recovered from disk");
            Ok(())
        }
        Ok(false) => {
            info!("Encryption not initialized, skipping session recovery");
            Ok(())
        }
        Err(e) => {
            error!(error = %e, "Encryption session recovery failed");
            anyhow::bail!(
                "Cannot start daemon: encryption session recovery failed: {}",
                e
            )
        }
    }
}
```

**Required change:** The function signature must return `anyhow::Result<bool>` (not `()`) so the caller can distinguish `Initialized + unlocked` (true) from `Uninitialized` (false):

```rust
async fn recover_encryption_session(runtime: &CoreRuntime) -> anyhow::Result<bool> {
    match uc.execute().await {
        Ok(true)  => { info!(...); Ok(true)  }
        Ok(false) => { info!(...); Ok(false) }
        Err(e)    => { error!(...); anyhow::bail!(...) }
    }
}
```

### PeerDiscoveryWorker start() — unconditional start_network (peer_discovery.rs, line 43)

```rust
// Source: src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs
async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
    let mut event_rx = self.network_events.subscribe_events().await?;
    self.network_control.start_network().await?;   // ← always called
    info!("peer discovery started");
    // ... event loop
}
```

This is fine — `start_network()` is idempotent. When the deferred spawn occurs (setup just completed, network was started by `SetupAction::EnsureDiscovery` or `AppLifecycleCoordinator`), calling `start_network()` again is a no-op. No change needed to `PeerDiscoveryWorker.start()`.

### oneshot-based deferred spawn pattern

```rust
// Source: standard tokio pattern — HIGH confidence
let (setup_tx, setup_rx) = tokio::sync::oneshot::channel::<()>();

// In select loop:
let mut deferred_rx = if deferred_worker.is_some() { Some(setup_rx) } else { None };

loop {
    tokio::select! {
        _ = wait_for_shutdown_signal() => break,
        // ... other arms ...
        Some(Ok(_)) = async { match deferred_rx.as_mut() {
            Some(rx) => Some(rx.await),
            None => None,
        } } => {
            if let Some(worker) = deferred_peer_discovery.take() {
                let token = self.cancel.child_token();
                service_tasks.spawn(async move { worker.start(token).await });
            }
            deferred_rx = None; // disarm after first fire
        }
    }
}
```

Or more cleanly using `futures::future::OptionFuture`:

```rust
use futures::future::OptionFuture;

let deferred_future: OptionFuture<_> = deferred_rx.into();

tokio::select! {
    _ = wait_for_shutdown_signal() => { ... }
    Some(Ok(_)) = &mut deferred_future => {
        if let Some(worker) = deferred_peer_discovery.take() { ... }
    }
}
```

---

## Project Constraints (from CLAUDE.md)

- All `cargo` commands MUST run from `src-tauri/` (never from project root)
- Never use `unwrap()` or `expect()` in production code — use `match` or `?`
- Use `match` over `if let` when the error/None case must be reported
- Spans over events for async operations: use `info_span!` + `.instrument()`
- Logging: use `tracing::{info, error, warn, debug}` (not `log::*`)
- No fixed pixel values in UI; this phase has no UI changes
- Test run commands:
  - Unit tests: `cd src-tauri && cargo test -p uc-daemon --lib`
  - Integration tests: `cd src-tauri && cargo test -p uc-daemon` (note: 5 pre-existing failures in pairing_api.rs unrelated to this phase)

---

## Standard Stack

### Core (no new dependencies needed)

| Library                               | Version                  | Purpose                                  | Why Standard                                       |
| ------------------------------------- | ------------------------ | ---------------------------------------- | -------------------------------------------------- |
| `tokio::sync::oneshot`                | (tokio 1.x, already dep) | One-shot deferred-start signal           | Built into tokio, integrates with `select!`        |
| `tokio::task::JoinSet`                | (tokio 1.x, already dep) | Dynamic service spawning after startup   | Already used for service management in `DaemonApp` |
| `tokio_util::sync::CancellationToken` | (already dep)            | Cooperative shutdown for deferred worker | Already used by all DaemonService implementations  |
| `tracing`                             | (already dep)            | Span instrumentation for new code paths  | Project standard logging                           |

**No new dependencies required.** All primitives are already in the project's dependency graph.

---

## Environment Availability

Step 2.6: SKIPPED (no external tool dependencies — pure Rust code changes in existing crates)

---

## Validation Architecture

### Test Framework

| Property           | Value                                           |
| ------------------ | ----------------------------------------------- |
| Framework          | Rust built-in + tokio-test                      |
| Config file        | `src-tauri/Cargo.toml` (workspace)              |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon --lib` |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon`       |

### Phase Requirements → Test Map

| Req ID                                  | Behavior                                                                              | Test Type | Automated Command                                                                                              | File Exists? |
| --------------------------------------- | ------------------------------------------------------------------------------------- | --------- | -------------------------------------------------------------------------------------------------------------- | ------------ |
| D-01/D-06                               | Daemon does NOT start PeerDiscoveryWorker when encryption is Uninitialized            | unit      | `cd src-tauri && cargo test -p uc-daemon --lib -- app::tests::peer_discovery_not_started_when_uninitialized`   | ❌ Wave 0    |
| D-02/D-07                               | Daemon starts PeerDiscoveryWorker when encryption is Initialized and unlocked         | unit      | `cd src-tauri && cargo test -p uc-daemon --lib -- app::tests::peer_discovery_started_when_initialized`         | ❌ Wave 0    |
| D-09/D-10                               | PeerDiscoveryWorker starts dynamically after setup completion event                   | unit      | `cd src-tauri && cargo test -p uc-daemon --lib -- app::tests::peer_discovery_deferred_start_on_setup_complete` | ❌ Wave 0    |
| D-04                                    | EnsureDiscovery in setup flow continues to call start_network (Joiner path unchanged) | existing  | `cd src-tauri && cargo test -p uc-app -- setup`                                                                | ✅ exists    |
| recover_encryption_session returns bool | Existing tests for Ok(true)/Ok(false)/Err arms                                        | existing  | `cd src-tauri && cargo test -p uc-daemon --lib -- app::tests`                                                  | ✅ exists    |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon --lib`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon --lib && cd src-tauri && cargo test -p uc-app -- setup`
- **Phase gate:** Full suite `cargo test -p uc-daemon --lib` green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/app.rs` — add `#[cfg(test)] mod tests` block with 3 behavioral tests for the deferred-start mechanism. These tests require a refactored `DaemonApp` that accepts `Option<Arc<dyn DaemonService>>` as deferred worker and `Option<oneshot::Receiver<()>>` as setup signal — tests will be added alongside the production changes.

_(Note: "uc-daemon tests/" integration tests have 5 pre-existing failures in `pairing_api.rs`. Phase 67 must not introduce additional failures, but those 5 are out of scope.)_

---

## Open Questions

1. **`recover_encryption_session` return type change**
   - What we know: currently returns `anyhow::Result<()>`, losing the `bool` from `AutoUnlockEncryptionSession::execute()`
   - What's unclear: whether changing to `anyhow::Result<bool>` breaks the existing structural test `run_method_contains_encryption_recovery_call` (which checks source text)
   - Recommendation: The structural test checks for function name and `.execute().await` — a signature change to `Result<bool>` won't break it. Verify after refactor.

2. **Where to place `SetupCompletionEmitter`**
   - What we know: it implements `SessionReadyEmitter` and is daemon-specific
   - What's unclear: whether it belongs in `uc-daemon/src/app.rs` or a new file
   - Recommendation: Place in `uc-daemon/src/app.rs` or new `uc-daemon/src/lifecycle.rs` to keep daemon-specific types together. No shared crate needed.

3. **`OptionFuture` dependency**
   - What we know: `futures::future::OptionFuture` requires the `futures` crate
   - What's unclear: whether `futures` is already a dep of `uc-daemon`
   - Recommendation: Check `uc-daemon/Cargo.toml`. If not present, use a manual pattern with `async { None::<()> }` or restructure the select to avoid it. The standard `tokio::select!` supports `if` guards: `_ = rx, if deferred.is_some() => { ... }`.

---

## Sources

### Primary (HIGH confidence)

- Direct code reading: `src-tauri/crates/uc-daemon/src/main.rs` — composition root, service assembly
- Direct code reading: `src-tauri/crates/uc-daemon/src/app.rs` — `DaemonApp::run()`, `recover_encryption_session()`
- Direct code reading: `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` — `PeerDiscoveryWorker.start()` unconditional `start_network()` call
- Direct code reading: `src-tauri/crates/uc-app/src/usecases/auto_unlock_encryption_session.rs` — `Ok(true)/Ok(false)/Err` contract
- Direct code reading: `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` — `AppLifecycleCoordinator::ensure_ready()` and `SessionReadyEmitter`
- Direct code reading: `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs` — `SetupAction::MarkSetupComplete` and `EnsureDiscovery`
- Direct code reading: `src-tauri/crates/uc-daemon/src/service.rs` — `DaemonService` trait
- Direct code reading: `src-tauri/crates/uc-core/src/ports/network_control.rs` — `NetworkControlPort` trait (idempotent `start_network`)
- Direct code reading: `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` — START_STATE atomic state machine confirming idempotency

### Secondary (MEDIUM confidence)

- tokio documentation (from training knowledge, verified against patterns already in codebase): `oneshot::channel`, `JoinSet::spawn`, `select!` macro behavior

---

## Metadata

**Confidence breakdown:**

- Current code structure: HIGH — read directly from source
- Change approach: HIGH — follows existing patterns in codebase (JoinSet, CancellationToken, oneshot channels)
- Test gaps: HIGH — confirmed via cargo test run showing 3 required tests don't exist yet
- Deferred-start mechanism: HIGH — standard tokio pattern, codebase already uses all primitives

**Research date:** 2026-03-27
**Valid until:** 2026-04-27 (stable Rust/tokio ecosystem — no breakage expected)
