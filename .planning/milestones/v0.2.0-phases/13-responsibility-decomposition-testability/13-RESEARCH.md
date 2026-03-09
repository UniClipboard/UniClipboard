# Phase 13: Responsibility Decomposition & Testability - Research

**Researched:** 2026-03-06
**Domain:** Rust module decomposition, test infrastructure, hexagonal architecture refactoring
**Confidence:** HIGH

## Summary

Phase 13 targets four god-object orchestrators and a monolithic dependency container (`AppDeps`) that have accumulated excessive responsibility, making testing slow and coupling broad. The primary decomposition targets are:

- **`setup/orchestrator.rs`** (2827 lines) -- drives setup state machine AND executes space-access join, pairing session management, encryption initialization, and lifecycle boot side effects inline
- **`pairing/orchestrator.rs`** (2107 lines) -- manages multi-session pairing state machines AND directly handles all protocol message types, timer scheduling, persistence, and event emission
- **`space_access/orchestrator.rs`** (1045 lines) + related adapters (~1800 lines total in `space_access/` module) -- coordinates space-access state machine with multiple adapter types
- **`AppDeps`** (97 lines, 30+ fields) -- flat god-container where every use case receives all ports

The secondary target is test infrastructure: mock/noop port implementations are duplicated across at least 3 locations (`setup/orchestrator.rs` tests, `pairing/orchestrator.rs` tests, `setup_flow_integration_test.rs`) with ~200 lines of identical `PairedDeviceRepositoryPort` noop implementations repeated verbatim.

**Primary recommendation:** Extract action-handler methods from orchestrators into focused service structs (e.g., `SetupActionExecutor`, `PairingProtocolHandler`), group `AppDeps` fields into domain-scoped sub-structs, and consolidate shared noop/mock implementations into a `uc-app/src/testing.rs` module gated behind `#[cfg(test)]`.

<phase_requirements>

## Phase Requirements

| ID        | Description                                                                                                                                       | Research Support                                                                                                                                                                                                                                                 |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DECOMP-01 | High-risk use-case modules (sync_inbound, sync_outbound, setup orchestration) are decomposed so business intent and infra mechanics are separated | Setup orchestrator has 25+ methods mixing dispatch logic with side-effect execution; pairing orchestrator has 30+ methods mixing protocol handling with state machine operation. Both need action-executor extraction.                                           |
| DECOMP-02 | Dependency organization for use cases is grouped to reduce god-container coupling and improve maintainability                                     | `AppDeps` has 30+ flat `Arc<dyn Port>` fields covering clipboard, security, device, pairing, network, storage, settings, and system domains. Grouping into domain sub-structs reduces constructor parameter lists and clarifies bounded context ownership.       |
| DECOMP-03 | Shared test helpers/noop implementations reduce duplicated mock scaffolding and speed up feature-level test setup                                 | `NoopPairedDeviceRepository` is implemented 3 times identically; `MockNetworkControl`, `MockWatcherControl`, `NoopDiscoveryPort`, `NoopSetupEventPort` etc. are duplicated across inline test modules and integration tests. ~500 lines of duplicated mock code. |
| DECOMP-04 | Regression checks cover core user flows (pairing, sync, setup transitions) during decomposition refactors                                         | Existing `setup_flow_integration_test.rs` (1029 lines) covers setup flows. Pairing orchestrator has extensive inline tests (~900 lines). Need to verify these pass after decomposition and add any missing flow-level coverage.                                  |

</phase_requirements>

## Standard Stack

### Core

| Library     | Version | Purpose                                                    | Why Standard                                |
| ----------- | ------- | ---------------------------------------------------------- | ------------------------------------------- |
| Rust std    | stable  | Module system, trait extraction                            | Built-in language feature for decomposition |
| async-trait | 0.1     | Async trait definitions for extracted service traits       | Already in use throughout codebase          |
| thiserror   | 1.x     | Typed error enums for new service modules                  | Already in use for error types              |
| mockall     | 0.13    | Already a dev-dep; can generate mocks for extracted traits | Already in Cargo.toml dev-dependencies      |

### Supporting

| Library  | Version | Purpose                      | When to Use       |
| -------- | ------- | ---------------------------- | ----------------- |
| tokio    | 1.x     | Test runtime for async tests | Already in use    |
| tempfile | 3.x     | Test temp directories        | Already a dev-dep |

### Alternatives Considered

| Instead of                 | Could Use             | Tradeoff                                                                                                                         |
| -------------------------- | --------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Hand-written noop impls    | mockall `#[automock]` | Automock adds proc-macro compile time; hand-written noops are simpler for ports with many methods where all return `Ok(default)` |
| Module-level decomposition | Crate-level splits    | Crate splits add Cargo.toml overhead; module splits within uc-app are sufficient for this phase                                  |

## Architecture Patterns

### Recommended Project Structure (after decomposition)

```
src-tauri/crates/uc-app/src/
├── usecases/
│   ├── setup/
│   │   ├── orchestrator.rs        # Slim: dispatch loop + public API only
│   │   ├── action_executor.rs     # NEW: Extracted action handlers
│   │   ├── pairing_bridge.rs      # NEW: Setup-pairing integration logic
│   │   ├── context.rs             # Existing
│   │   ├── mark_complete.rs       # Existing
│   │   └── mod.rs
│   ├── pairing/
│   │   ├── orchestrator.rs        # Slim: session management + dispatch
│   │   ├── protocol_handler.rs    # NEW: Protocol message handlers
│   │   ├── session_manager.rs     # NEW: Session lifecycle (create/cleanup/timeout)
│   │   ├── events.rs              # Existing
│   │   ├── facade.rs              # Existing
│   │   └── mod.rs
│   └── ...
├── deps.rs                        # Grouped sub-structs
└── testing.rs                     # NEW: Shared noop/mock impls (cfg(test))
```

### Pattern 1: Action Executor Extraction

**What:** Extract the body of each `SetupAction` match arm into a dedicated struct method or separate service.
**When to use:** When an orchestrator's `execute_actions` method has 5+ match arms each with 10+ lines of async side-effect code.
**Example:**

```rust
// BEFORE: setup/orchestrator.rs (inline in execute_actions)
SetupAction::StartJoinSpaceAccess => {
    self.start_join_space_access().await?;  // 140 lines of method
}

// AFTER: setup/action_executor.rs
pub struct SetupActionExecutor {
    initialize_encryption: Arc<InitializeEncryption>,
    mark_setup_complete: Arc<MarkSetupComplete>,
    app_lifecycle: Arc<AppLifecycleCoordinator>,
    // ... only the ports needed for action execution
}

impl SetupActionExecutor {
    pub async fn execute(&self, action: SetupAction, ctx: &SetupContext) -> Result<Vec<SetupEvent>, SetupError> {
        match action {
            SetupAction::CreateEncryptedSpace => self.create_encrypted_space(ctx).await,
            SetupAction::MarkSetupComplete => self.mark_complete(ctx).await,
            // ...
        }
    }
}

// orchestrator.rs becomes thin dispatch:
impl SetupOrchestrator {
    async fn dispatch(&self, event: SetupEvent) -> Result<SetupState, SetupError> {
        // ... state machine transition (pure) ...
        let follow_ups = self.action_executor.execute(actions, &self.context).await?;
        // ...
    }
}
```

### Pattern 2: Dependency Sub-Grouping

**What:** Group `AppDeps` fields into domain-scoped sub-structs that use cases can accept individually.
**When to use:** When a container has 20+ fields and individual use cases only need 3-5.
**Example:**

```rust
// BEFORE: 30+ flat fields
pub struct AppDeps {
    pub clipboard: Arc<dyn PlatformClipboardPort>,
    pub encryption: Arc<dyn EncryptionPort>,
    pub encryption_session: Arc<dyn EncryptionSessionPort>,
    // ... 27 more fields
}

// AFTER: Grouped sub-structs
pub struct ClipboardPorts {
    pub clipboard: Arc<dyn PlatformClipboardPort>,
    pub system_clipboard: Arc<dyn SystemClipboardPort>,
    pub entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    pub event_repo: Arc<dyn ClipboardEventWriterPort>,
    pub representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    // ... clipboard-specific ports
}

pub struct SecurityPorts {
    pub encryption: Arc<dyn EncryptionPort>,
    pub encryption_session: Arc<dyn EncryptionSessionPort>,
    pub encryption_state: Arc<dyn EncryptionStatePort>,
    pub key_scope: Arc<dyn KeyScopePort>,
    pub secure_storage: Arc<dyn SecureStoragePort>,
    pub key_material: Arc<dyn KeyMaterialPort>,
}

pub struct AppDeps {
    pub clipboard: ClipboardPorts,
    pub security: SecurityPorts,
    pub device: DevicePorts,
    pub network: Arc<NetworkPorts>,  // Already exists!
    pub storage: StoragePorts,
    pub settings: Arc<dyn SettingsPort>,
    pub system: SystemPorts,
}
```

### Pattern 3: Shared Test Noops Module

**What:** Centralized `#[cfg(test)]` module with noop implementations for commonly-mocked ports.
**When to use:** When the same port noop appears in 3+ test files.
**Example:**

```rust
// uc-app/src/testing.rs (gated behind #[cfg(test)])
#[cfg(test)]
pub mod testing {
    use async_trait::async_trait;
    use std::sync::Arc;

    pub struct NoopPairedDeviceRepository;

    #[async_trait]
    impl PairedDeviceRepositoryPort for NoopPairedDeviceRepository {
        // ... single canonical implementation
    }

    pub struct NoopDiscoveryPort;
    pub struct NoopNetworkControl;
    pub struct NoopSetupEventPort;
    pub struct NoopPairingTransport;
    pub struct NoopSpaceAccessTransport;
    // ... all commonly used noops
}
```

### Anti-Patterns to Avoid

- **Premature crate splitting:** Don't split uc-app into separate crates for this phase. Module-level decomposition within the existing crate is sufficient and avoids Cargo.toml/dependency overhead.
- **Over-abstracting orchestrators:** Don't introduce a generic "orchestrator framework" or trait hierarchy. Keep it simple: extract methods into new structs with clear ownership.
- **Breaking public API surface:** The `SetupOrchestrator`, `PairingOrchestrator` public methods (`new_space`, `join_space`, `initiate_pairing`, etc.) must remain stable. Only internal implementation changes.
- **Moving test code before decomposing production code:** Decompose production code first, then consolidate test helpers. Moving tests first risks churn when the production API changes.

## Don't Hand-Roll

| Problem                   | Don't Build                    | Use Instead                                                            | Why                                                                                           |
| ------------------------- | ------------------------------ | ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Mock generation for ports | Per-test inline mock structs   | Shared noop module + mockall for counting/asserting variants           | Eliminates 500+ lines of duplicated code                                                      |
| Async trait dispatch      | Custom vtable or enum dispatch | `async-trait` crate (already in use)                                   | Standard, well-tested, already adopted                                                        |
| State machine extraction  | Custom FSM framework           | Keep existing `SetupStateMachine`/`PairingStateMachine` pure functions | They're already well-separated; the problem is in the action handlers, not the state machines |

**Key insight:** The state machines themselves (`SetupStateMachine`, `PairingStateMachine`, `SpaceAccessStateMachine`) are already clean pure-function implementations in `uc-core`. The decomposition target is the _orchestrator action execution layer_ that sits between state machines and ports, NOT the state machines themselves.

## Common Pitfalls

### Pitfall 1: Breaking Arc Sharing Semantics

**What goes wrong:** When extracting methods into a new struct, forgetting to share the same `Arc` instances leads to state divergence.
**Why it happens:** The original orchestrator holds `Arc<Mutex<Option<T>>>` for session state. If the extracted struct creates new Arcs, state changes are invisible.
**How to avoid:** Pass `Arc` references from the orchestrator to the extracted executor. The executor does not own session state; it borrows it.
**Warning signs:** Tests pass individually but fail when run together; state appears "reset" between operations.

### Pitfall 2: Circular Module Dependencies

**What goes wrong:** Extracted `action_executor.rs` needs types from `orchestrator.rs`, and orchestrator needs types from executor.
**Why it happens:** The error types, context types, and event types are defined inline in the orchestrator module.
**How to avoid:** Extract shared types (errors, events, context) into their own submodule first, then extract the executor. Dependency flow: `types.rs` <- `action_executor.rs` <- `orchestrator.rs`.
**Warning signs:** Compiler errors about circular `use` statements.

### Pitfall 3: Test Regression from Visibility Changes

**What goes wrong:** Inline `#[cfg(test)]` modules have access to private methods via `super::*`. Moving tests to separate files loses this access.
**Why it happens:** Inline test modules in Rust can access private members of their parent module.
**How to avoid:** Keep inline tests that need private access. Only consolidate _noop/mock implementations_ and _integration-style tests_ that use the public API.
**Warning signs:** Compilation errors in tests after moving them.

### Pitfall 4: SetupOrchestrator Constructor Explosion

**What goes wrong:** SetupOrchestrator already takes 16 parameters in `new()`. Extracting an action executor adds another struct to construct.
**Why it happens:** The decomposition creates more types to wire together.
**How to avoid:** The action executor should absorb most of the ports, reducing SetupOrchestrator's direct dependencies. The orchestrator keeps: context, dispatch machinery, and a reference to the executor. Net constructor complexity should decrease.
**Warning signs:** `new()` parameter list grows rather than shrinks after refactoring.

## Code Examples

### Current SetupOrchestrator Constructor (16 params)

```rust
// Source: src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs:91-131
pub fn new(
    initialize_encryption: Arc<InitializeEncryption>,
    mark_setup_complete: Arc<MarkSetupComplete>,
    setup_status: Arc<dyn SetupStatusPort>,
    app_lifecycle: Arc<AppLifecycleCoordinator>,
    pairing_orchestrator: Arc<PairingOrchestrator>,
    setup_event_port: Arc<dyn SetupEventPort>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    discovery_port: Arc<dyn DiscoveryPort>,
    network_control: Arc<dyn NetworkControlPort>,
    crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
    pairing_transport: Arc<dyn PairingTransportPort>,
    transport_port: Arc<Mutex<dyn SpaceAccessTransportPort>>,
    proof_port: Arc<dyn ProofPort>,
    timer_port: Arc<Mutex<dyn TimerPort>>,
    persistence_port: Arc<Mutex<dyn PersistencePort>>,
) -> Self { ... }
```

### Duplicate Noop Example (appears 3 times)

```rust
// Source: setup_flow_integration_test.rs:154, pairing/orchestrator.rs:1195, setup/orchestrator.rs tests
struct NoopPairedDeviceRepository;

#[async_trait]
impl PairedDeviceRepositoryPort for NoopPairedDeviceRepository {
    async fn get_by_peer_id(&self, _: &PeerId) -> Result<Option<PairedDevice>, ...> { Ok(None) }
    async fn list_all(&self) -> Result<Vec<PairedDevice>, ...> { Ok(Vec::new()) }
    async fn upsert(&self, _: PairedDevice) -> Result<(), ...> { Ok(()) }
    async fn set_state(&self, _: &PeerId, _: PairingState) -> Result<(), ...> { Ok(()) }
    async fn update_last_seen(&self, _: &PeerId, _: DateTime<Utc>) -> Result<(), ...> { Ok(()) }
    async fn delete(&self, _: &PeerId) -> Result<(), ...> { Ok(()) }
}
```

### AppDeps Flat Structure (30+ fields)

```rust
// Source: src-tauri/crates/uc-app/src/deps.rs:47-97
pub struct AppDeps {
    // Clipboard: 12 fields
    // Security: 6 fields
    // Device: 2 fields
    // Pairing: 1 field
    // Network: 2 fields (already has NetworkPorts sub-struct)
    // Setup: 1 field
    // Storage: 5 fields
    // Settings: 1 field
    // System: 2 fields
}
```

## State of the Art

| Old Approach                                         | Current Approach                              | When Changed    | Impact                                                                |
| ---------------------------------------------------- | --------------------------------------------- | --------------- | --------------------------------------------------------------------- |
| Monolithic orchestrator with all side-effects inline | Extract action handlers into focused services | Phase 13 target | Reduces file sizes by ~50%, enables isolated testing                  |
| Flat dependency container                            | Domain-grouped port bundles                   | Phase 13 target | `NetworkPorts` pattern already exists in codebase as proof-of-concept |
| Per-test duplicate mock implementations              | Shared `#[cfg(test)]` testing module          | Phase 13 target | Eliminates ~500 lines of duplicated code                              |

## Validation Architecture

### Test Framework

| Property           | Value                                                                        |
| ------------------ | ---------------------------------------------------------------------------- |
| Framework          | cargo test (Rust built-in)                                                   |
| Config file        | `src-tauri/Cargo.toml` workspace members                                     |
| Quick run command  | `cd src-tauri && cargo test -p uc-app --lib -- --test-threads=1`             |
| Full suite command | `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-core -p uc-platform` |

### Phase Requirements -> Test Map

| Req ID    | Behavior                                                 | Test Type   | Automated Command                                                          | File Exists?  |
| --------- | -------------------------------------------------------- | ----------- | -------------------------------------------------------------------------- | ------------- |
| DECOMP-01 | Setup orchestrator dispatch still works after extraction | integration | `cd src-tauri && cargo test -p uc-app --test setup_flow_integration_test`  | Yes           |
| DECOMP-01 | Pairing orchestrator protocol handling post-extraction   | unit        | `cd src-tauri && cargo test -p uc-app --lib pairing::orchestrator::tests`  | Yes (inline)  |
| DECOMP-02 | AppDeps sub-struct compilation and wiring                | unit        | `cd src-tauri && cargo test -p uc-app --lib deps::tests`                   | Yes (minimal) |
| DECOMP-02 | UseCases accessor still compiles with grouped deps       | unit        | `cd src-tauri && cargo test -p uc-tauri --test usecases_accessor_test`     | Yes           |
| DECOMP-03 | Shared testing module compiles and is importable         | unit        | `cd src-tauri && cargo test -p uc-app --lib testing`                       | No - Wave 0   |
| DECOMP-04 | Setup flow end-to-end regression                         | integration | `cd src-tauri && cargo test -p uc-app --test setup_flow_integration_test`  | Yes           |
| DECOMP-04 | Pairing flow regression                                  | unit        | `cd src-tauri && cargo test -p uc-app --lib pairing`                       | Yes           |
| DECOMP-04 | Bootstrap integration regression                         | integration | `cd src-tauri && cargo test -p uc-tauri --test bootstrap_integration_test` | Yes           |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-app --lib -- --test-threads=1`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-core -p uc-platform`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-app/src/testing.rs` -- shared noop/mock module (covers DECOMP-03)
- [ ] Verify all existing tests pass before starting decomposition: `cd src-tauri && cargo test -p uc-app -p uc-tauri`

## Open Questions

1. **Should `StagedPairedDeviceStore` be injected via AppDeps or remain orchestrator-local?**
   - What we know: Phase 12 converted it to an injectable struct with `std::sync::Mutex`. Currently created fresh during `build_setup_orchestrator`.
   - What's unclear: Whether it should be a top-level AppDeps field or remain wiring-local.
   - Recommendation: Keep it wiring-local (in `runtime.rs`); it's a setup-specific concern, not a cross-cutting dependency.

2. **How far to decompose the pairing orchestrator?**
   - What we know: It manages concurrent sessions via `HashMap<SessionId, PairingSessionContext>` and handles 10+ protocol message types.
   - What's unclear: Whether to extract a `ProtocolHandler` trait or keep concrete methods.
   - Recommendation: Extract concrete `PairingProtocolHandler` struct (not trait) to avoid over-abstraction. It receives the sessions map reference and handles message-to-event conversion.

## Sources

### Primary (HIGH confidence)

- Direct codebase analysis of all target files
- `src-tauri/crates/uc-app/src/deps.rs` -- current dependency container structure
- `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs` -- 2827 lines, 25+ methods
- `src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs` -- 2107 lines, 30+ methods
- `src-tauri/crates/uc-app/tests/setup_flow_integration_test.rs` -- 1029 lines, existing regression coverage
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` -- AppRuntime/UseCases wiring pattern

### Secondary (MEDIUM confidence)

- Rust module system best practices for decomposition (standard Rust patterns)
- `#[cfg(test)]` module gating for shared test helpers (standard Rust testing pattern)

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - all tools already in Cargo.toml, no new dependencies needed
- Architecture: HIGH - based on direct analysis of 5000+ lines of code with clear decomposition boundaries
- Pitfalls: HIGH - derived from actual codebase patterns (Arc sharing, constructor params, visibility rules)

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable internal refactoring, no external dependency risk)
