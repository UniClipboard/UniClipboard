# Phase 54: Extract daemon client and realtime infrastructure from uc-tauri - Research

**Researched:** 2026-03-24
**Domain:** Rust crate extraction (hexagonal architecture refactoring)
**Confidence:** HIGH

## Summary

This is a pure architectural refactoring phase: extract the Tauri-free daemon communication layer from `uc-tauri` into a new `uc-daemon-client` crate. The extracted code includes HTTP clients, WebSocket bridge, realtime runtime, and connection state -- none of which have any Tauri dependencies. After extraction, `uc-tauri` becomes a thin Tauri-specific adapter over `uc-daemon-client`.

All decisions are locked in `54-CONTEXT.md`. This research validates the implementation path and surfaces mechanical details not covered in the context.

**Primary recommendation:** Follow the atomic commit strategy from D-12: create crate → move files → update call-sites → verify clean build.

---

<user_constraints>

## User Constraints (from 54-CONTEXT.md)

### Locked Decisions

- **D-01:** New crate `uc-daemon-client` at `src-tauri/crates/uc-daemon-client/`
- **D-02:** Single responsibility: "daemon communication" (HTTP + WebSocket)
- **D-03:** Not placed in `uc-daemon` (daemon is server, should not contain client code)
- **D-04:** Not placed in `uc-bootstrap` (bootstrap is for dependency assembly, not runtime communication)
- **D-05:** Directory structure locked: `src/lib.rs`, `src/connection.rs`, `src/http/{mod,pairing,query,setup}.rs`, `src/ws_bridge.rs`, `src/realtime.rs`
- **D-06:** Dependencies: `uc-core`, `uc-app`, `uc-daemon` (for `DaemonConnectionInfo`), `reqwest`, `tokio-tungstenite`, `tokio`, `futures-util`, `async-trait`, `serde`, `serde_json`, `anyhow`, `tracing`
- **D-07:** `DaemonConnectionState` moved to `uc-daemon-client/src/connection.rs`
- **D-08:** `uc-daemon-client` depends on `uc-daemon` (lib) for `DaemonConnectionInfo` type
- **D-09:** Dependency chain: `uc-tauri → uc-daemon-client → uc-daemon (lib)`
- **D-10:** No re-export stub in `uc-tauri` -- direct import update at all call sites
- **D-11:** All call-sites updated in the same phase
- **D-12:** Atomic commits per logical step
- **D-13:** `uc-tauri` directly depends on `uc-daemon-client` in `Cargo.toml`
- **D-14:** Commands directly `use uc_daemon_client::http::{DaemonPairingClient, ...}` etc.

### Claude's Discretion (not constrained)

- Internal module organization of `ws_bridge.rs` (sub-module split or not)
- Test file ownership (move with source or stay in `uc-tauri tests/`)
- Whether `DaemonWsBridgeConfig` moves with `ws_bridge` (it should -- it is tightly coupled)

### Deferred Ideas (OUT OF SCOPE)

- EventHub generalization (Issue #316)
- `daemon_lifecycle.rs` extraction → Phase 55
- `setup_pairing_bridge.rs` extraction → Phase 55
- `wiring.rs` / `run.rs` partial extraction → unprioritized
  </user_constraints>

---

## Standard Stack

### Core Crate Dependencies

| Library                | Version   | Purpose                                           | Why Standard                                       |
| ---------------------- | --------- | ------------------------------------------------- | -------------------------------------------------- |
| `reqwest`              | `0.12`    | HTTP client for daemon HTTP API calls             | Already used in `uc-tauri`                         |
| `tokio-tungstenite`    | `0.24`    | Async WebSocket client for daemon realtime bridge | Already used in `uc-tauri`                         |
| `tokio`                | `1`       | Async runtime                                     | Already used everywhere                            |
| `tokio-util`           | `0.7`     | `CancellationToken`, codec helpers                | Already used in `uc-tauri`                         |
| `futures-util`         | `0.3`     | `SinkExt`, `StreamExt`                            | Already used in `uc-tauri`                         |
| `async-trait`          | `0.1`     | `#[async_trait]` on trait methods                 | Already used in `uc-tauri`                         |
| `serde` / `serde_json` | `1` / `1` | Serialization of HTTP/WS payloads                 | Already used in `uc-tauri`                         |
| `anyhow`               | `1.0`     | Error handling                                    | Already used everywhere                            |
| `tracing`              | `0.1`     | Structured logging                                | Already used everywhere                            |
| `url`                  | `2`       | URL parsing for WS host/port extraction           | Already used in `uc-tauri` (`daemon_ws_bridge.rs`) |

### Workspace Crate Dependencies

| Crate       | Version    | Purpose                                                    |
| ----------- | ---------- | ---------------------------------------------------------- |
| `uc-core`   | local path | `HostEventEmitterPort`, `RealtimeTopic` trait              |
| `uc-app`    | local path | `SetupPairingEventHub`, realtime consumers, `TaskRegistry` |
| `uc-daemon` | local path | `DaemonConnectionInfo` type, `DaemonWsEvent` types         |

**Note:** `uc-daemon` (lib) does NOT depend on `uc-daemon-client`, so no circular dependency risk.

### Alternative Crate Configurations Considered

| Instead of                 | Could Use                   | Tradeoff                                                                                                                            |
| -------------------------- | --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `reqwest` default features | `reqwest` with `rustls-tls` | uc-tauri already uses `rustls-tls`; uc-daemon-client should match (but daemon is local loopback only, so default TLS is acceptable) |

---

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-daemon-client/
├── Cargo.toml
└── src/
    ├── lib.rs              ← crate root + pub mod declarations
    ├── connection.rs        ← DaemonConnectionState (moved from uc-tauri bootstrap/runtime.rs)
    ├── http/
    │   ├── mod.rs          ← authorized_daemon_request helper
    │   ├── pairing.rs      ← DaemonPairingClient (renamed from TauriDaemonPairingClient)
    │   ├── query.rs        ← DaemonQueryClient (renamed from TauriDaemonQueryClient)
    │   └── setup.rs        ← DaemonSetupClient (renamed from TauriDaemonSetupClient)
    ├── ws_bridge.rs        ← DaemonWsBridge (moved from uc-tauri bootstrap/)
    └── realtime.rs         ← start_realtime_runtime + install_daemon_setup_pairing_facade
```

### Pattern 1: Type Rename Without Stubs

The context mandates **no re-export stubs** (D-10). This means the migration is a direct rename:

```
TauriDaemonPairingClient  →  DaemonPairingClient   (pub struct)
TauriDaemonQueryClient    →  DaemonQueryClient     (pub struct)
TauriDaemonSetupClient   →  DaemonSetupClient    (pub struct)
```

All call-sites in `uc-tauri` are updated simultaneously. No backward-compatibility aliases needed.

### Pattern 2: DaemonConnectionState Location

`DaemonConnectionState` is a shared mutable cell holding `Option<DaemonConnectionInfo>`. It is currently in `uc-tauri/src/bootstrap/runtime.rs` alongside `AppRuntime`. Moving it to `uc-daemon-client/src/connection.rs` is straightforward since:

- It has no Tauri dependencies (just `Arc<RwLock<Option<DaemonConnectionInfo>>`)
- `DaemonConnectionInfo` comes from `uc_daemon::api::auth`
- The RwLock poison-recovery pattern is self-contained

After the move, `uc-tauri` imports `DaemonConnectionState` as `uc_daemon_client::connection::DaemonConnectionState`.

### Pattern 3: Dependency Arrow Direction

```
uc-tauri  ────────►  uc-daemon-client  ────────►  uc-daemon (lib only)
  │                       │                           │
  │ (Tauri-specific)      │ (HTTP+WS client)          │ (server-side types)
  └────────────────────────┘                           │
         (no reverse deps)                            │
                              (uc-daemon does NOT depend on uc-daemon-client)
```

This is a clean acyclic dependency graph.

### Pattern 4: Test File Ownership

**Decision needed (Claude's discretion):** Move tests with their source files or keep them in `uc-tauri tests/`?

**Recommendation:** Move tests with source. The tests for `daemon_ws_bridge.rs` (`uc-tauri/tests/daemon_ws_bridge.rs`) are tightly coupled to `DaemonWsBridge` internals (e.g., `ScriptedDaemonWsConnector`). Moving them to `uc-daemon-client/tests/` is cleaner and avoids cross-crate test imports. The `daemon_bootstrap_contract.rs` and `daemon_command_shell.rs` tests in `uc-tauri/tests/` test integration across multiple modules and should remain in `uc-tauri`.

**Specific tests that need consideration:**

- `uc-tauri/src/daemon_client/pairing.rs` has `#[cfg(test)]` tests using `DaemonConnectionState` -- these move to `uc-daemon-client/src/http/pairing.rs`
- `uc-tauri/src/daemon_client/setup.rs` has `#[cfg(test)]` tests -- these move to `uc-daemon-client/src/http/setup.rs`
- `uc-tauri/src/daemon_client/mod.rs` has a `query_tests` module (`#[cfg(test)] mod query_tests`) -- moves to `uc-daemon-client/src/http/query.rs`
- `uc-tauri/src/bootstrap/daemon_ws_bridge.rs` has `#[cfg(test)] mod tests` -- moves to `uc-daemon-client/src/ws_bridge.rs`
- `uc-tauri/src/bootstrap/realtime_runtime.rs` has `#[cfg(test)] mod tests` -- moves to `uc-daemon-client/src/realtime.rs`
- `uc-tauri/src/bootstrap/runtime.rs` has one test `daemon_connection_state_stores_connection_info_in_memory` -- moves to `uc-daemon-client/src/connection.rs`

---

## Don't Hand-Roll

| Problem                      | Don't Build                 | Use Instead                           | Why                                                             |
| ---------------------------- | --------------------------- | ------------------------------------- | --------------------------------------------------------------- |
| WebSocket client             | Custom TCP + manual framing | `tokio-tungstenite`                   | Handles protocol negotiation, ping/pong, backpressure correctly |
| HTTP client with auth        | Manual HTTP construction    | `reqwest`                             | Connection pooling, TLS, automatic redirect handling            |
| Async mutex poison recovery  | Ignore poison or panic      | `RwLock::poisoned()` guard recovery   | Already in codebase, preserves availability                     |
| Cancellation propagation     | Manual flag polling         | `CancellationToken` from `tokio-util` | Already in codebase, clean cancellation                         |
| Realtime consumer management | Manual task tracking        | `TaskRegistry::spawn`                 | Already in codebase, provides graceful shutdown                 |

---

## Common Pitfalls

### Pitfall 1: Missing Workspace Member Registration

**What goes wrong:** `cargo build` fails with "package `uc-daemon-client` is not a member of the workspace"

**Why it happens:** Forgetting to add `"crates/uc-daemon-client"` to the `members` list in `src-tauri/Cargo.toml`

**How to avoid:** Add the new member to `workspace.members` in `src-tauri/Cargo.toml` alongside existing crates

**Warning signs:** Any `cargo` command at workspace root fails immediately

---

### Pitfall 2: Circular Dependency After Adding `uc-daemon-client` to `uc-daemon`

**What goes wrong:** `uc-daemon-client` imports types from `uc-daemon`, but `uc-daemon` transitively depends on `uc-app` / `uc-core`. Could `uc-daemon-client` accidentally re-export something that creates a cycle?

**Why it happens:** `uc-daemon` does NOT depend on `uc-daemon-client` (D-03 explicitly excludes this). The cycle would only happen if `uc-daemon-client` transitively pulled in something that depends on `uc-daemon-client`.

**How to avoid:** The dependency chain is `uc-daemon-client → uc-daemon` (lib). `uc-daemon` (lib) does not depend on any client crate. The existing `uc-daemon-client → uc-app → uc-core` chain is already satisfied since `uc-daemon` already depends on both.

**Verification:** Run `cargo check -p uc-daemon-client` before committing.

---

### Pitfall 3: Module Path Updates Missed in Tests

**What goes wrong:** Tests in `uc-tauri/tests/` still import from `uc_tauri::bootstrap::daemon_ws_bridge` and `uc_tauri::bootstrap::DaemonConnectionState`, causing compile errors

**Why it happens:** The context lists test files that need import updates but the migration plan may only update source files

**How to avoid:** Update all test import paths in the same atomic commit as the source file deletion:

- `uc-tauri/tests/daemon_ws_bridge.rs`: `uc_tauri::bootstrap::daemon_ws_bridge::{BridgeState, DaemonWsBridge, DaemonWsBridgeConfig, ScriptedDaemonWsConnector}` → `uc_daemon_client::ws_bridge::{BridgeState, DaemonWsBridge, DaemonWsBridgeConfig, ScriptedDaemonWsConnector}`
- `uc-tauri/tests/daemon_bootstrap_contract.rs`: `uc_tauri::bootstrap::runtime::DaemonConnectionState` → `uc_daemon_client::connection::DaemonConnectionState`
- `uc-tauri/tests/daemon_command_shell.rs`: `uc_tauri::bootstrap::DaemonConnectionState` → `uc_daemon_client::connection::DaemonConnectionState`

---

### Pitfall 4: `#[cfg(test)]` Module Visibility After Move

**What goes wrong:** Tests inside `pairing.rs`, `setup.rs`, `daemon_ws_bridge.rs` that were accessible from `mod.rs` via `mod tests` no longer compile after moving

**Why it happens:** When a file with `#[cfg(test)] mod tests { ... }` is moved, the module structure changes

**How to avoid:** The test files use `super::*` to access the parent module's items. After moving:

- In `uc-daemon-client/src/http/pairing.rs`: `mod tests { super::*; ... }` still works (no `super` needed for module-local tests)
- `mod query_tests` in `uc-daemon-client/src/http/query.rs` becomes `#[cfg(test)] mod tests` (inline or separate)

**Note:** The `daemon_client/mod.rs` has `#[cfg(test)] mod query_tests;` -- this should become a `#[cfg(test)] mod tests` inside `query.rs` and the `mod.rs` line should be removed.

---

### Pitfall 5: Tauri `#[tauri::command]` Functions Using `DaemonConnectionState`

**What goes wrong:** `DaemonConnectionState` is registered via `.manage()` in `main.rs` and accessed via `State<'_, DaemonConnectionState>` in commands. After moving, the type is still used the same way, but the import path changes.

**Why it happens:** The type lives in `uc-daemon-client` but the commands still access it via `State<'_, DaemonConnectionState>`. The `TauriState` pattern doesn't change -- only the type's crate path changes.

**How to avoid:** After the move, `uc-tauri` adds `uc-daemon-client` as a direct dependency. Commands import `uc_daemon_client::connection::DaemonConnectionState`. The `.manage()` call in `main.rs` also imports from `uc_daemon_client`. This is the only change needed.

---

## Code Examples

### New Crate Cargo.toml (template)

```toml
[package]
name = "uc-daemon-client"
version.workspace = true
edition = "2021"
description = "Daemon HTTP + WebSocket client for UniClipboard"

[lib]
name = "uc_daemon_client"
path = "src/lib.rs"

[dependencies]
# Workspace crates
uc-core = { path = "../uc-core" }
uc-app = { path = "../uc-app" }
uc-daemon = { path = "../uc-daemon" }

# HTTP + WS
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
tokio-tungstenite = "0.24"

# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }
futures-util = "0.3"
async-trait = "0.1"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# URL parsing
url = "2"

# Error handling + logging
anyhow = "1.0"
tracing = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
```

### lib.rs (template)

```rust
//! # uc-daemon-client
//!
//! Daemon HTTP and WebSocket client for UniClipboard.
//! Zero Tauri dependencies -- usable from any async context.

pub mod connection;
pub mod http;
pub mod realtime;
pub mod ws_bridge;

pub use connection::DaemonConnectionState;
pub use http::{DaemonPairingClient, DaemonQueryClient, DaemonSetupClient};
pub use realtime::{install_daemon_setup_pairing_facade, start_realtime_runtime};
pub use ws_bridge::{BridgeState, DaemonWsBridge, DaemonWsBridgeConfig, DaemonWsBridgeError};
```

### Import Changes in uc-tauri Commands

**Before** (`commands/pairing.rs`):

```rust
use crate::daemon_client::{
    DaemonPairingRequestError, TauriDaemonPairingClient, TauriDaemonQueryClient,
};
```

**After**:

```rust
use uc_daemon_client::http::{
    DaemonPairingClient, DaemonPairingRequestError, DaemonQueryClient,
};
```

**Before** (`commands/setup.rs`):

```rust
use crate::daemon_client::TauriDaemonSetupClient;
```

**After**:

```rust
use uc_daemon_client::http::DaemonSetupClient;
```

### bootstrap/mod.rs Changes

**Before** (current):

```rust
pub use runtime::{create_app, create_runtime, AppRuntime, AppUseCases, DaemonConnectionState};
pub use daemon_ws_bridge::DaemonWsBridge;
pub use realtime_runtime::{install_daemon_setup_pairing_facade, start_realtime_runtime};
```

**After** (uc-tauri re-exports from uc-daemon-client):

```rust
pub use uc_daemon_client::connection::DaemonConnectionState;
pub use uc_daemon_client::ws_bridge::DaemonWsBridge;
pub use uc_daemon_client::realtime::{install_daemon_setup_pairing_facade, start_realtime_runtime};
```

Wait -- context D-10 says **no re-export stub**. So `uc-tauri/bootstrap/mod.rs` should NOT re-export these. Instead, call sites use `uc_daemon_client::*` directly.

The bootstrap module declaration `pub mod daemon_client;` in `uc-tauri/src/lib.rs` should be removed (the module no longer exists in `uc-tauri`).

### DaemonConnectionState Import in main.rs

**Before**:

```rust
use uc_tauri::bootstrap::{DaemonConnectionState, GuiOwnedDaemonState};
let daemon_connection_state = DaemonConnectionState::default();
```

**After**:

```rust
use uc_daemon_client::connection::DaemonConnectionState;
let daemon_connection_state = DaemonConnectionState::default();
```

Note: `uc-tauri` will still need to register `DaemonConnectionState` with `.manage()` in main.rs. This is unchanged behavior -- only the import path changes.

---

## State of the Art

| Old Approach                                               | Current Approach                              | When Changed | Impact                                      |
| ---------------------------------------------------------- | --------------------------------------------- | ------------ | ------------------------------------------- |
| `TauriDaemonPairingClient`                                 | `DaemonPairingClient` (removing Tauri prefix) | Phase 54     | Cleaner API, clarifies crate responsibility |
| `daemon_client/` subdirectory in `uc-tauri`                | `uc-daemon-client` workspace crate            | Phase 54     | Enables future non-Tauri daemon clients     |
| `DaemonConnectionState` in `uc-tauri/bootstrap/runtime.rs` | Moved to `uc-daemon-client/connection.rs`     | Phase 54     | Clean separation of concerns                |

**Deprecated/outdated:**

- None in scope for this phase

---

## Open Questions

1. **`query_tests.rs` file move**
   - What we know: `query_tests.rs` is a separate file (92 lines, 2 `#[tokio::test]` tests) referenced via `#[cfg(test)] mod query_tests;` in `daemon_client/mod.rs`
   - Resolution: Move the contents of `query_tests.rs` into `query.rs` as `#[cfg(test)] mod tests { ... }` and delete `query_tests.rs`. The `mod query_tests;` declaration in `mod.rs` should be removed entirely.

2. **`setup_pairing_bridge.rs` uses `TauriDaemonPairingClient`**
   - What we know: `setup_pairing_bridge.rs` (NOT extracted in this phase) imports `TauriDaemonPairingClient` from `crate::daemon_client`
   - What's unclear: After Phase 54, `crate::daemon_client` no longer exists. But `setup_pairing_bridge.rs` is Phase 55 scope.
   - **Recommendation:** `setup_pairing_bridge.rs` will need import update in Phase 55 when it's extracted. For Phase 54, since it's NOT being extracted, its imports need to point to `uc_daemon_client`. This is a call-site update within Phase 54's scope.

3. **`uc-daemon-client` depending on `uc-daemon` (lib) -- version consistency**
   - What we know: `DaemonConnectionInfo` and `DaemonWsEvent` types come from `uc-daemon/lib`
   - What's unclear: Whether `uc-daemon/lib` is stable enough to be a dependency for `uc-daemon-client`
   - **Resolution:** `uc-daemon/lib` is already depended on by `uc-cli` (CLI status command). This pattern is established. `uc-daemon-client` is in the same position.

---

## Validation Architecture

> Skip this section entirely if `workflow.nyquist_validation` is explicitly set to `false` in `.planning/config.json`. Key absent, treat as enabled.

### Test Framework

| Property           | Value                                            |
| ------------------ | ------------------------------------------------ |
| Framework          | Rust built-in `#[test]` / `#[tokio::test]`       |
| Config file        | None                                             |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon-client` |
| Full suite command | `cd src-tauri && cargo test --workspace`         |

### Phase Requirements → Test Map

This is a pure refactoring phase with no new functional requirements. Validation is compile + test coverage.

| Req ID | Behavior                                                  | Test Type | Automated Command                                                                   | File Exists? |
| ------ | --------------------------------------------------------- | --------- | ----------------------------------------------------------------------------------- | ------------ |
| N/A    | `uc-daemon-client` crate compiles cleanly                 | unit      | `cargo check -p uc-daemon-client`                                                   | n/a          |
| N/A    | `uc-tauri` compiles after extraction                      | unit      | `cargo check -p uc-tauri`                                                           | n/a          |
| N/A    | `uc-daemon-client` unit tests pass                        | unit      | `cargo test -p uc-daemon-client`                                                    | n/a          |
| N/A    | `uc-tauri` integration tests pass                         | unit      | `cargo test -p uc-tauri`                                                            | n/a          |
| N/A    | No `crate::daemon_client` references remain in `uc-tauri` | grep      | `rg 'crate::daemon_client\|use crate::daemon_client' src-tauri/crates/uc-tauri/src` | n/a          |

### Wave 0 Gaps

After Phase 54 extraction, `uc-daemon-client` has no pre-existing test infrastructure. The following test files are moved from `uc-tauri` (NOT new gaps, but moved):

- [ ] `src-tauri/crates/uc-daemon-client/src/http/pairing.rs` -- covers `DaemonPairingClient`, moved from `uc-tauri/src/daemon_client/pairing.rs`
- [ ] `src-tauri/crates/uc-daemon-client/src/http/query.rs` -- covers `DaemonQueryClient`, `query_tests` module inlined
- [ ] `src-tauri/crates/uc-daemon-client/src/http/setup.rs` -- covers `DaemonSetupClient`, moved from `uc-tauri/src/daemon_client/setup.rs`
- [ ] `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` -- covers `DaemonWsBridge`, moved from `uc-tauri/src/bootstrap/daemon_ws_bridge.rs`
- [ ] `src-tauri/crates/uc-daemon-client/src/realtime.rs` -- covers `start_realtime_runtime`, moved from `uc-tauri/src/bootstrap/realtime_runtime.rs`
- [ ] `src-tauri/crates/uc-daemon-client/src/connection.rs` -- covers `DaemonConnectionState`, moved from `uc-tauri/src/bootstrap/runtime.rs`

---

## Sources

### Primary (HIGH confidence)

- Source files read directly from filesystem
- `src-tauri/crates/uc-tauri/src/daemon_client/` -- 4 files, all read
- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` -- 887 lines, fully read
- `src-tauri/crates/uc-tauri/src/bootstrap/realtime_runtime.rs` -- 285 lines, fully read
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` -- `DaemonConnectionState` section (lines 1-79), fully read
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` -- re-exports fully read
- `src-tauri/crates/uc-tauri/src/lib.rs` -- module declarations read
- `src-tauri/src/main.rs` -- grep for `DaemonConnectionState` usage
- `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` -- import paths read
- `src-tauri/crates/uc-daemon/src/api/auth.rs` -- `DaemonConnectionInfo` definition confirmed

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon/Cargo.toml` -- `[lib]` section confirmed for dependency safety
- `src-tauri/crates/uc-tauri/Cargo.toml` -- existing dep versions confirmed

### Tertiary (LOW confidence)

- None needed for this refactoring phase

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH -- all dependencies are from existing uc-tauri Cargo.toml, versions verified
- Architecture: HIGH -- all decisions are locked in CONTEXT.md, no interpretation needed
- Pitfalls: HIGH -- mechanical pitfalls (workspace member, import paths) are predictable from code reading

**Research date:** 2026-03-24
**Valid until:** 2026-04-23 (30 days -- pure refactoring, stable patterns)
