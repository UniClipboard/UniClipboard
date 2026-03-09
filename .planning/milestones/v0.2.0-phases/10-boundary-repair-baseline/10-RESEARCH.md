# Phase 10: Boundary Repair Baseline - Research

**Researched:** 2026-03-06
**Domain:** Rust hexagonal architecture boundary enforcement (Tauri 2 / workspace crates)
**Confidence:** HIGH

## Summary

This phase repairs three categories of dependency-direction violations in the UniClipboard hexagonal architecture: (1) command-layer code bypassing use cases via public `runtime.deps` access, (2) a platform adapter (`uc-platform`) directly calling an infrastructure crate (`uc-infra`) for streaming decode, and (3) non-domain port traits living in `uc-core` when they belong in `uc-platform`. All violations are structural and can be enforced at compile time by making fields private and removing Cargo dependencies.

The codebase is well-organized into workspace crates (`uc-core`, `uc-infra`, `uc-app`, `uc-platform`, `uc-tauri`), and the existing patterns (port injection via constructor, `runtime.usecases()` accessor) provide clear templates for the fixes. The primary enforcement mechanism is the Rust compiler: once `deps` is private and `uc-infra` is removed from `uc-platform/Cargo.toml`, regressions are impossible.

**Primary recommendation:** Execute the three plans sequentially (10-01, 10-02, 10-03) with `cargo check` gating each commit. The compiler is the primary safety signal; no new integration tests are needed beyond existing coverage.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Make `pub deps: AppDeps` **private** on `AppRuntime`
- Add **thin facade methods** directly on `AppRuntime` for simple read-only state access (e.g., `runtime.device_id()`, `runtime.is_encryption_ready()`)
- Commands that perform business operations must be **routed through existing use cases**; if no use case exists, create one
- **No exceptions for tracing** -- all `deps` access in command files goes through facade methods
- **AppDeps struct shape is not changed** -- only visibility, not restructuring (Phase 13)
- Bootstrap/wiring: use **constructor injection** -- `AppRuntime::new()` takes pre-assembled use cases rather than raw AppDeps
- **Document the approved access pattern** in `///` doc block on `AppRuntime`
- **Inject `TransferPayloadDecryptorPort` into `Libp2pNetworkAdapter::new()`** alongside existing ports
- **Inject `TransferPayloadEncryptorPort` at the same time** for consistency
- Concrete `ChunkedDecoder`/`TransferPayloadDecryptorAdapter` wired at bootstrap (uc-tauri), not inside the platform adapter
- **Remove `uc-infra` from `uc-platform/Cargo.toml`** once the single call is replaced
- **Evict all 6 non-domain ports**: `AutostartPort`, `UiPort`, `AppDirsPort`, `WatcherControlPort`, `IdentityStorePort`, `ObservabilityPort`
- Evicted ports **move to `uc-platform/src/ports/`**
- Affected use cases in `uc-app` that import evicted ports are **moved into `uc-platform`** rather than adding `uc-platform` dep to `uc-app`
- **Primary safety signal: Rust compiler** -- no pre-refactor integration tests needed
- **Plan order**: 10-01 (runtime/command), 10-02 (decode port), 10-03 (port eviction)
- **Each plan must compile and pass `cargo check`** before commit

### Claude's Discretion

- Exact set of facade method names on AppRuntime beyond discussed examples
- Whether any of the 6 ports need re-classification after closer inspection (esp. ObservabilityPort)
- Exact wording of the approved-access doc block on AppRuntime

### Deferred Ideas (OUT OF SCOPE)

- **AppDeps decomposition** (splitting into SecurityDeps, ClipboardDeps etc.) -- Phase 13
- **Typed error migration into port surfaces** -- Phase 11
- **Lifecycle task tracking** -- Phase 12
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID       | Description                                                                                                                            | Research Support                                                                                                                                                                                                                                                                                                               |
| -------- | -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| BOUND-01 | User-triggered Tauri commands invoke clipboard/business operations through use cases rather than direct runtime dependency access      | 13 `runtime.deps.*` violations identified across 4 command files (clipboard.rs, settings.rs, encryption.rs, pairing.rs). Categorized into facade-eligible (device_id, encryption_session.is_ready) and use-case-required (settings access)                                                                                     |
| BOUND-02 | Runtime composition keeps dependency containers private to wiring/bootstrap modules and prevents command-layer penetration             | `pub deps: AppDeps` on AppRuntime at line 89 is the single point of exposure. Making it private + constructor injection closes the path structurally                                                                                                                                                                           |
| BOUND-03 | Network payload decode path uses a uc-core port abstraction so platform adapters do not depend directly on infra crate implementations | Single violation at libp2p_network.rs:972 calling `uc_infra::clipboard::ChunkedDecoder::decode_from()`. Existing `TransferPayloadDecryptorPort` takes `&[u8]` but streaming path needs `Read`-based interface -- port needs streaming variant or the call site must buffer first                                               |
| BOUND-04 | Non-domain ports are placed outside uc-core so core remains focused on domain contracts                                                | 6 ports identified: AutostartPort, UiPort, AppDirsPort, WatcherControlPort, IdentityStorePort, ObservabilityPort (module with TraceMetadata/extract_trace, no trait). Use cases affected: `apply_autostart.rs` (AutostartPort), `start_clipboard_watcher.rs` (WatcherControlPort), `lib.rs` App struct (AutostartPort, UiPort) |

</phase_requirements>

## Standard Stack

### Core

| Library               | Version | Purpose                                               | Why Standard                                       |
| --------------------- | ------- | ----------------------------------------------------- | -------------------------------------------------- |
| Rust workspace crates | N/A     | Structural boundary enforcement via `Cargo.toml` deps | Removing a crate dep makes violations uncompilable |
| async-trait           | 0.1     | Async port traits                                     | Already used throughout for port definitions       |
| Arc\<dyn Trait\>      | std     | Port injection pattern                                | Established pattern in all constructors            |
| tokio                 | 1       | Async runtime, spawn_blocking for decode              | Already the runtime throughout                     |

### Supporting

| Library   | Version | Purpose                              | When to Use                                              |
| --------- | ------- | ------------------------------------ | -------------------------------------------------------- |
| thiserror | 2.0.17  | Typed errors for port contracts      | Already used in TransferCryptoError, WatcherControlError |
| tracing   | 0.1     | Structured logging in facade methods | Already used in commands for spans                       |

### Alternatives Considered

None. All decisions are locked -- this phase uses existing libraries and patterns.

## Architecture Patterns

### Current Violation Map

```
BOUND-01/02: Command → runtime.deps.* (BYPASS)
  commands/clipboard.rs:29    → deps.device_identity.current_device_id()
  commands/clipboard.rs:53    → deps.encryption_session.is_ready()
  commands/clipboard.rs:147   → deps.device_identity.current_device_id()
  commands/encryption.rs:53   → deps.device_identity.current_device_id()  (×4 more)
  commands/encryption.rs:957  → deps.encryption_session.is_ready()
  commands/settings.rs:29     → deps.device_identity.current_device_id()
  commands/settings.rs:78     → deps.device_identity.current_device_id()
  commands/settings.rs:107    → deps.settings.clone()
  commands/pairing.rs:146     → deps.device_identity.current_device_id()
  commands/pairing.rs:211     → deps.device_identity.current_device_id()

BOUND-03: Platform → Infra (BYPASS)
  uc-platform/adapters/libp2p_network.rs:972 → uc_infra::clipboard::ChunkedDecoder::decode_from()

BOUND-04: Non-domain ports in uc-core
  uc-core/src/ports/autostart.rs       → AutostartPort
  uc-core/src/ports/ui_port.rs         → UiPort
  uc-core/src/ports/app_dirs.rs        → AppDirsPort
  uc-core/src/ports/watcher_control.rs → WatcherControlPort
  uc-core/src/ports/identity_store.rs  → IdentityStorePort
  uc-core/src/ports/observability.rs   → TraceMetadata, extract_trace (no trait)
```

### Pattern 1: Facade Methods on AppRuntime (BOUND-01/02)

**What:** Thin read-only accessors on `AppRuntime` that delegate to `deps` fields internally.
**When to use:** Commands need simple state reads (device_id, session readiness) for tracing attributes or conditional logic.
**Example:**

```rust
// In uc-tauri/src/bootstrap/runtime.rs
impl AppRuntime {
    /// Returns the current device ID for use in tracing and session context.
    /// For business operations, use `self.usecases()` instead.
    pub fn device_id(&self) -> String {
        self.deps.device_identity.current_device_id()
    }

    pub async fn is_encryption_ready(&self) -> bool {
        self.deps.encryption_session.is_ready().await
    }
}

// In commands:
// Before: runtime.deps.device_identity.current_device_id()
// After:  runtime.device_id()
```

### Pattern 2: Constructor Injection for AppRuntime (BOUND-02)

**What:** `AppRuntime::new()` takes pre-assembled use case accessors + facade dependencies, making `deps` fully private.
**When to use:** Bootstrap/wiring constructs `AppRuntime`.
**Example:**

```rust
pub struct AppRuntime {
    deps: AppDeps,  // private -- no `pub`
    // ... other fields unchanged
}

impl AppRuntime {
    pub fn new(
        deps: AppDeps,
        // ... other existing params
    ) -> Self {
        Self { deps, /* ... */ }
    }
}
```

**Key insight:** The current `AppRuntime::new()` already takes `AppDeps` as a parameter. The only change is removing `pub` from the `deps` field. All existing `usecases()` methods already access `self.deps` internally -- they are unaffected.

### Pattern 3: Port Injection for Streaming Decode (BOUND-03)

**What:** Inject `Arc<dyn TransferPayloadDecryptorPort>` into `Libp2pNetworkAdapter` struct; the call site buffers the stream into `Vec<u8>` then calls `port.decrypt()`.
**When to use:** The streaming decode path in `libp2p_network.rs`.

**Critical design decision:** The existing `TransferPayloadDecryptorPort::decrypt(&self, encrypted: &[u8], master_key: &MasterKey)` takes `&[u8]`, not a `Read` stream. The current call site at line 972 uses `ChunkedDecoder::decode_from(sync_reader, &master_key)` with a `SyncIoBridge<Reader>`. Two approaches:

1. **Buffer-then-decrypt:** Read the remaining stream into a `Vec<u8>` inside `spawn_blocking`, then call `port.decrypt(&buf, &master_key)`. Simple, uses existing port signature. Viable because clipboard payloads have bounded size.
2. **Add streaming port variant:** Create `TransferPayloadStreamDecryptorPort` with `fn decrypt_stream<R: Read>(&self, reader: R, master_key: &MasterKey)`. More complex, adds a new port trait.

The existing `TransferPayloadDecryptorAdapter` in `uc-infra` already implements `decrypt(&[u8])` by wrapping `ChunkedDecoder::decode_from(Cursor::new(encrypted))` -- so option 1 is straightforward. The planner should decide but option 1 is simpler and sufficient.

### Pattern 4: Port Eviction to uc-platform (BOUND-04)

**What:** Move non-domain port trait definitions from `uc-core/src/ports/` to `uc-platform/src/ports/`.
**When to use:** Ports that serve platform concerns, not domain logic.

**Example for AutostartPort:**

```rust
// Move from: uc-core/src/ports/autostart.rs
// Move to:   uc-platform/src/ports/autostart.rs

// In uc-platform/src/ports/mod.rs:
pub mod autostart;
pub use autostart::AutostartPort;
```

**Use case relocation:** `apply_autostart.rs` currently in `uc-app/src/usecases/` imports `AutostartPort` from `uc-core`. After eviction, the use case moves to `uc-platform` since adding `uc-platform` as a dep of `uc-app` would violate dependency direction.

### Anti-Patterns to Avoid

- **Re-exporting evicted ports from uc-core:** Defeats the purpose. Imports must change.
- **Adding uc-platform dep to uc-app:** Violates hexagonal dependency direction.
- **Making deps `pub(crate)` instead of private:** Still allows command files within the same crate to access it. Commands are in `uc-tauri`, and `deps` is on `AppRuntime` also in `uc-tauri`, so `pub(crate)` would NOT solve the problem. Must be truly private with facade methods.

## Don't Hand-Roll

| Problem                      | Don't Build                  | Use Instead                                                      | Why                                              |
| ---------------------------- | ---------------------------- | ---------------------------------------------------------------- | ------------------------------------------------ |
| Boundary enforcement         | Runtime checks / code review | Cargo.toml dep removal + private fields                          | Compiler catches violations at build time        |
| Streaming decode abstraction | New streaming port trait     | Buffer + existing `TransferPayloadDecryptorPort::decrypt(&[u8])` | Clipboard payloads are bounded; simpler approach |
| Access pattern documentation | External docs                | `///` doc blocks on `AppRuntime` struct                          | Lives with the code, checked during review       |

## Common Pitfalls

### Pitfall 1: Forgetting to Update Test Mocks

**What goes wrong:** Tests in `uc-tauri/src/commands/*.rs` create mock `AppRuntime` instances that access `deps` directly. After making `deps` private, these tests won't compile.
**Why it happens:** Test code often mirrors production patterns, including the violations.
**How to avoid:** Search all `#[cfg(test)]` modules in command files for `deps` access. Update tests to use facade methods or provide test constructors.
**Warning signs:** `cargo test` fails even though `cargo check` passes.

### Pitfall 2: ObservabilityPort Module Has No Trait

**What goes wrong:** The CONTEXT.md lists `ObservabilityPort` as a port to evict, but `uc-core/src/ports/observability.rs` contains `TraceMetadata`, `OptionalTrace`, and `extract_trace()` -- data types and a helper function, not a port trait. There is no `ObservabilityPort` trait.
**Why it happens:** The module name suggests it's a port, but it's really a shared data model for trace propagation.
**How to avoid:** Evaluate whether `TraceMetadata`/`extract_trace` are truly non-domain. If trace propagation is cross-cutting infra, move the module. If it's a domain concept (trace context for clipboard events), keep it in core.
**Warning signs:** Moving it causes widespread import breakage if domain code depends on `TraceMetadata`.

### Pitfall 3: IdentityStorePort Used Heavily in uc-platform

**What goes wrong:** `IdentityStorePort` is imported by `libp2p_network.rs`, `network.rs`, and `identity_store.rs` in `uc-platform`. After moving the trait to `uc-platform/src/ports/`, internal imports change but this is straightforward since both the trait and its users are in the same crate.
**Why it happens:** This port was always a platform concern (libp2p identity persistence) but was placed in core.
**How to avoid:** After moving, update all `use uc_core::ports::IdentityStorePort` to the new local path.

### Pitfall 4: Streaming Decode Port Signature Mismatch

**What goes wrong:** The existing `TransferPayloadDecryptorPort::decrypt` takes `&[u8]` but the libp2p streaming path uses `ChunkedDecoder::decode_from(reader)` with a `Read` impl. If you inject the port as-is, you need to buffer the stream first.
**Why it happens:** The port was designed for in-memory payloads, not streaming.
**How to avoid:** Buffer the stream contents into a `Vec<u8>` inside `spawn_blocking` before calling `port.decrypt()`. The `SyncIoBridge` is already in a blocking context, so `read_to_end` is safe. Clipboard payloads are bounded (~50MB max).

### Pitfall 5: AppDirs Types Remain in uc-core

**What goes wrong:** `AppDirsPort` returns `AppDirs` (defined in `uc-core/src/app_dirs.rs`). Moving only the port trait to `uc-platform` without moving `AppDirs` data type means `uc-platform` still depends on `uc-core` for the return type -- which is fine since that dependency direction is allowed.
**Why it happens:** Port trait moves but its associated types may stay.
**How to avoid:** This is actually correct -- `uc-platform` depends on `uc-core` by design. Only the trait definition moves; the data types it references can stay in `uc-core` if they are domain models.

### Pitfall 6: WatcherControlPort in deps.rs

**What goes wrong:** `AppDeps` in `uc-app/src/deps.rs` has `pub watcher_control: Arc<dyn WatcherControlPort>`. After evicting `WatcherControlPort` from `uc-core`, `uc-app` can't import it unless `uc-app` depends on `uc-platform` (forbidden).
**Why it happens:** `AppDeps` aggregates all ports including non-domain ones.
**How to avoid:** The use case `start_clipboard_watcher.rs` moves to `uc-platform`. The `watcher_control` field must also be removed from `AppDeps` or `AppDeps` must import from the new location. Since adding `uc-platform` dep to `uc-app` is forbidden, `watcher_control` must be removed from `AppDeps` and handled at the platform/wiring layer.

## Code Examples

### Facade Method Implementation (BOUND-01)

```rust
// Source: uc-tauri/src/bootstrap/runtime.rs (to be modified)
impl AppRuntime {
    /// Get the current device ID.
    ///
    /// This is a thin read-only accessor for tracing attributes and session context.
    /// For business operations involving device identity, use `self.usecases()`.
    pub fn device_id(&self) -> String {
        self.deps.device_identity.current_device_id()
    }

    /// Check if encryption session is ready.
    pub async fn is_encryption_ready(&self) -> bool {
        self.deps.encryption_session.is_ready().await
    }
}
```

### Command Migration (BOUND-01)

```rust
// Before (clipboard.rs):
let device_id = runtime.deps.device_identity.current_device_id();
let session_ready = runtime.deps.encryption_session.is_ready().await;

// After:
let device_id = runtime.device_id();
let session_ready = runtime.is_encryption_ready().await;
```

### Decode Port Injection (BOUND-03)

```rust
// Source: uc-platform/src/adapters/libp2p_network.rs (to be modified)
pub struct Libp2pNetworkAdapter {
    // ... existing fields ...
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
    // NEW:
    transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
    transfer_encryptor: Arc<dyn TransferPayloadEncryptorPort>,
}

impl Libp2pNetworkAdapter {
    pub fn new(
        identity_store: Arc<dyn IdentityStorePort>,
        policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
        // NEW:
        transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
        transfer_encryptor: Arc<dyn TransferPayloadEncryptorPort>,
    ) -> Result<Self> { /* ... */ }
}

// In the streaming decode path (replacing line 972):
let decode_result = tokio::task::spawn_blocking(move || {
    use tokio_util::io::SyncIoBridge;
    let mut sync_reader = SyncIoBridge::new(reader);
    let mut buf = Vec::new();
    sync_reader.read_to_end(&mut buf)
        .map_err(|e| format!("stream read failed: {e}"))?;
    transfer_decryptor.decrypt(&buf, &master_key)
        .map_err(|e| format!("decrypt failed: {e}"))
}).await.map_err(|e| format!("decode task panicked: {e}"))?;
```

### Port Eviction (BOUND-04)

```rust
// uc-platform/src/ports/autostart.rs (new file)
use anyhow::Result;

pub trait AutostartPort: Send + Sync {
    fn is_enabled(&self) -> Result<bool>;
    fn enable(&self) -> Result<()>;
    fn disable(&self) -> Result<()>;
}

// uc-platform/src/ports/mod.rs (updated)
pub mod app_event_handler;
pub mod autostart;
pub mod clipboard_runtime;
pub mod command_executor;
// ... other evicted ports

pub use autostart::AutostartPort;
pub use clipboard_runtime::ClipboardRuntimePort;
pub use command_executor::PlatformCommandExecutorPort;
```

## State of the Art

| Old Approach                               | Current Approach                           | When Changed | Impact                                                   |
| ------------------------------------------ | ------------------------------------------ | ------------ | -------------------------------------------------------- |
| `pub deps: AppDeps` on AppRuntime          | Private deps + facade methods + usecases() | This phase   | Commands can only access deps through controlled surface |
| `uc-infra` dep in `uc-platform/Cargo.toml` | Port injection from bootstrap              | This phase   | Platform layer truly isolated from infra implementation  |
| Non-domain ports in `uc-core`              | Ports in `uc-platform/src/ports/`          | This phase   | Core stays focused on domain contracts                   |

## Open Questions

1. **ObservabilityPort classification**
   - What we know: `uc-core/src/ports/observability.rs` contains `TraceMetadata`, `OptionalTrace`, and `extract_trace()` -- no trait, just data types and a utility function.
   - What's unclear: Whether `TraceMetadata` is a domain concept (trace context traveling with clipboard events) or purely infra concern. No usages found in `uc-app`.
   - Recommendation: If no `uc-app` code imports from this module, it's safe to move to `uc-platform`. The planner should verify with a grep at implementation time.

2. **Settings access in commands/settings.rs:107**
   - What we know: `runtime.deps.settings.clone()` is used directly to call `resolve_pairing_device_name()`. This is a business operation, not a simple read.
   - What's unclear: Whether an existing use case covers this, or if a new one is needed.
   - Recommendation: Create a thin use case or route through an existing settings-related use case.

3. **Buffer size for streaming decode**
   - What we know: The streaming path reads clipboard payloads that can be large (up to ~50MB based on chunked encoding design).
   - What's unclear: Whether buffering into a single `Vec<u8>` before decrypt is acceptable for all payload sizes.
   - Recommendation: Acceptable -- the current `TransferPayloadDecryptorAdapter::decrypt` already works on `&[u8]` (full buffer), and the existing V3 streaming path was added for transfer efficiency, not to avoid in-memory buffering at the decrypt step.

## Validation Architecture

### Test Framework

| Property           | Value                              |
| ------------------ | ---------------------------------- |
| Framework          | cargo test (Rust built-in)         |
| Config file        | `src-tauri/Cargo.toml` (workspace) |
| Quick run command  | `cd src-tauri && cargo check`      |
| Full suite command | `cd src-tauri && cargo test`       |

### Phase Requirements -> Test Map

| Req ID   | Behavior                              | Test Type           | Automated Command                            | File Exists?               |
| -------- | ------------------------------------- | ------------------- | -------------------------------------------- | -------------------------- |
| BOUND-01 | Commands use facade methods, not deps | compile-time        | `cd src-tauri && cargo check`                | N/A -- compiler enforces   |
| BOUND-02 | deps field is private                 | compile-time        | `cd src-tauri && cargo check`                | N/A -- compiler enforces   |
| BOUND-03 | uc-platform has no uc-infra dep       | compile-time        | `cd src-tauri && cargo check -p uc-platform` | N/A -- Cargo.toml enforces |
| BOUND-04 | Non-domain ports not in uc-core       | compile-time + grep | `cd src-tauri && cargo check`                | N/A -- compiler enforces   |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

None -- existing test infrastructure covers all phase requirements. The primary validation mechanism is the Rust compiler, which enforces boundary violations as compile errors. No additional test files are needed.

## Sources

### Primary (HIGH confidence)

- Direct codebase inspection of `uc-tauri/src/bootstrap/runtime.rs` (AppRuntime struct, pub deps field)
- Direct codebase inspection of `uc-tauri/src/commands/*.rs` (13 runtime.deps violations)
- Direct codebase inspection of `uc-platform/src/adapters/libp2p_network.rs:972` (uc_infra call)
- Direct codebase inspection of `uc-core/src/ports/mod.rs` (6 non-domain port modules)
- Direct codebase inspection of `uc-platform/Cargo.toml:13` (uc-infra dependency)
- Direct codebase inspection of `uc-core/src/ports/security/transfer_crypto.rs` (existing port signatures)
- Direct codebase inspection of `uc-infra/src/clipboard/chunked_transfer.rs` (TransferPayloadDecryptorAdapter impl)

### Secondary (MEDIUM confidence)

- `uc-app/src/deps.rs` field analysis for port eviction impact

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - no new libraries, all patterns established in codebase
- Architecture: HIGH - violations are concrete and enumerated, fixes follow existing patterns
- Pitfalls: HIGH - identified through direct code inspection and dependency analysis

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable -- no external dependency changes expected)
