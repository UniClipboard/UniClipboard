# Testability & Evolution Reviewer — Phase 3 Cross-Review

## Review of: contract-error-reviewer's Phase 2 Proposal

### A. Points of Agreement

1. **DTO layer at the IPC boundary (H10)** improves testability significantly. When domain models are decoupled from their serialized IPC representation, tests can validate domain logic independently from serialization format. Mapping tests (`From<DomainModel> for Dto`) become simple, focused unit tests rather than end-to-end command invocation tests. This also improves evolvability: domain models can change shape without breaking the frontend contract, and vice versa.

2. **Removing `EncryptionState::Initializing` (M9)** is the correct testability choice. Phantom variants create dead branches in `match` arms that can never be reached in tests. This means any test that exercises all variants is forced to include an unreachable case, and coverage tools report misleading numbers. Removing it simplifies the state space that tests must cover.

3. **Per-cluster error enums (M11)** are a major testability win over `anyhow::Result`. Typed errors allow tests to assert on specific failure modes (`assert!(matches!(result, Err(ClipboardRepositoryError::EntryNotFound(_))))`) rather than doing string matching on error messages. This makes tests resilient to error message rewording.

4. **`CommandError` structured enum (H11)** with a fixed, category-based variant set (NotFound, NotReady, InvalidInput, Unauthorized, Internal) is a sound approach. The category-based design avoids the per-command variant explosion problem and gives the frontend a stable contract to match on.

5. **Renaming `AppConfig` to `BootstrapConfig` (M13)** and removing overlapping fields clarifies the configuration contract, making it easier to test configuration-dependent behavior: tests that exercise user preferences use `Settings`, tests that exercise infrastructure bootstrap use `BootstrapConfig`, with no ambiguity.

### B. Points of Conflict

1. **`CommandError` enum as a coupling point for test maintenance**. The proposal defines `CommandError` with 5 variants. While the category-based approach limits growth, the _mapping logic_ in each command (`match e.downcast_ref::<ClipboardError>()`) creates per-command mapping code that requires its own tests. With 7+ command families, that is 7+ sets of mapping tests that must be written and maintained. The proposal does not address how to test these mappings efficiently. If each command independently maps errors, there is no shared mapping infrastructure to test once — the same mapping patterns will be duplicated across commands.

2. **`anyhow` migration creates a testing burden the proposal underestimates**. Migrating 33 port traits from `anyhow::Result` to typed errors means:
   - 33 new error enums must be defined
   - Each infra implementation must add `.map_err()` calls converting infrastructure errors (IO, SQL, serde) to domain errors
   - Each `.map_err()` call is a mapping that should be tested (does `diesel::Error::NotFound` map to `ClipboardRepositoryError::EntryNotFound` or `ClipboardRepositoryError::Internal`?)
   - The proposal says "~2-3 PRs per port cluster" but does not specify a testing strategy for verifying error mappings. Without this, typed errors give a false sense of type safety — the types are correct but the mappings may be wrong.

3. **DTO location in `uc-tauri/models/` creates test harness friction**. The proposal places DTOs in `uc-tauri`, which depends on the Tauri framework. Testing DTO mapping logic (`From<PairedDevice> for PairedDeviceDto`) requires either:
   - Running tests within `uc-tauri` (which pulls in Tauri dependencies and longer compile times), or
   - Moving DTO definitions to a lightweight crate

   The proposal does not address this. DTO serialization round-trip tests are important for contract stability, and they should run fast without framework overhead.

### C. Constraints They Missed

1. **Error context preservation for test assertions**. The proposal's typed errors (e.g., `ClipboardRepositoryError::EntryNotFound(id)`) carry the entity ID, which is good. But infrastructure errors wrapped as `Internal { source: ... }` lose the error chain. Tests that need to verify _why_ an operation failed at the infrastructure level (e.g., "was it a connection timeout or a constraint violation?") cannot make that assertion through the `Internal` variant. The proposal should specify whether `Internal` variants preserve the error chain (via `#[source]` or `anyhow` wrapping) or discard it.

2. **Backward compatibility of `CommandError` serialization format**. The `#[serde(tag = "code", content = "detail")]` format becomes a contract that the frontend depends on. If a variant is renamed or its `detail` shape changes, frontend tests break. The proposal does not establish a versioning or stability policy for this serialization format. From an evolution standpoint, this is a new coupling surface that needs a compatibility test (snapshot test or golden file test for the JSON shape).

3. **`PeerId` migration (M12) testing gap**. Changing 8+ `NetworkEvent` variants from `String` to `PeerId` is described as "mechanical," but the proposal does not mention regression tests. Event serialization is used in logging and potentially in event persistence. A round-trip test (`PeerId -> JSON -> PeerId`) should be added to verify that the newtype's `Serialize/Deserialize` implementation is wire-compatible with the previous `String` format. The proposal notes `PeerId` serializes identically to `String`, but this invariant should be verified by a test, not by documentation.

### D. Risk If Adopted As-Is

1. **Error mapping code proliferates without shared testing patterns**. Each command independently maps use-case errors to `CommandError`. Without a shared test helper (e.g., `assert_maps_to_not_found!(use_case_error, "clipboard_entry", entry_id)`), each command's error mapping tests will be written differently, some thoroughly and others not at all. The likely outcome: error mapping is tested for clipboard commands (migrated first) and undertested for later migrations.

2. **DTO tests slow down or are skipped**. If DTOs live in `uc-tauri` and test compilation requires the full Tauri dependency tree, developers will avoid writing DTO mapping tests. Over time, DTO mappings will drift from domain models without test coverage catching it.

3. **Typed error enums accumulate orphaned variants**. As features are removed or refactored, error variants may become unreachable (like `EncryptionState::Initializing` was). Without a test or lint that ensures every error variant is reachable from at least one code path, phantom error variants will appear. The proposal addresses this for `EncryptionState` but does not establish a general guard.

### E. Suggested Revisions

1. **Move DTO definitions to a lightweight `uc-ipc-models` crate** (or a `models` module in `uc-core` if the shapes are truly domain-adjacent). This crate depends only on `serde` and `uc-core` types, enabling fast compilation and isolated DTO mapping tests without Tauri dependencies.

2. **Add a shared error mapping test utility** in `uc-test-support`:

   ```rust
   // uc-test-support/src/assertions/errors.rs
   pub fn assert_command_error_not_found(result: Result<_, CommandError>, resource: &str) {
       match result {
           Err(CommandError::NotFound { resource: r, .. }) => assert_eq!(r, resource),
           other => panic!("Expected NotFound for {}, got {:?}", resource, other),
       }
   }
   ```

3. **Require `#[source]` on `Internal` error variants** to preserve error chains for debugging and test assertions:

   ```rust
   Internal { message: String, #[serde(skip)] source: Option<Box<dyn std::error::Error + Send + Sync>> }
   ```

4. **Add a golden-file test for `CommandError` JSON serialization** to detect breaking contract changes automatically.

5. **Add a `PeerId` serialization compatibility test** as part of the M12 migration:

   ```rust
   #[test]
   fn peer_id_serializes_same_as_string() {
       let id = PeerId::from("12D3KooW...");
       assert_eq!(serde_json::to_value(&id).unwrap(), serde_json::to_value("12D3KooW...").unwrap());
   }
   ```

6. **Establish a CI lint for reachable error variants**: every error enum variant in `uc-core` should be constructed by at least one code path (checked via a `grep` or dead-code analysis step).

---

## Review of: app-flow-reviewer's Phase 2 Proposal

### A. Points of Agreement

1. **SetupOrchestrator decomposition into action executors (H3)** is a strong testability improvement. The current 15-dependency, 860-line orchestrator is essentially untestable in isolation. Decomposing into focused executors means each executor can be tested with only its 2-3 required ports. The `SetupActionExecutor` trait with `execute(context) -> Result<Vec<SetupEvent>>` is a clean, testable contract: input is immutable context, output is a list of events. Pure functions of this shape are trivially testable.

2. **SyncInbound decomposition (H4)** into decoder + deduplicator + applier follows the "pipeline of transformations" pattern, which is inherently testable. Each stage has a clear input type and output type. The `InboundDeduplicator` with `is_duplicate/record/rollback` methods is a clean state machine that can be tested with property-based testing (e.g., "a message recorded once is duplicate, a message never recorded is not").

3. **SyncOutbound infra leakage fix (H5)** — moving `tokio::runtime::Handle::try_current()` and wire encoding out of the use case is critical for testability. Currently, testing `SyncOutboundClipboardUseCase` requires a running Tokio runtime even if the test only cares about business logic (peer selection, origin filtering). Making the use case purely async removes this runtime coupling.

4. **Anemic domain model enrichment (M3)** — adding `is_stale()`, `touch()`, `has_title()` to `ClipboardEntry` is good testability practice. These become pure functions that are trivially unit-testable (`assert!(entry.is_stale(now + threshold + 1, threshold))`). More importantly, they prevent test duplication: without these methods, every test that checks staleness reimplements the comparison logic.

5. **Removing `pairing_transport` from SetupOrchestrator (M2)** reduces the dependency surface for setup tests, which is a direct testability improvement.

### B. Points of Conflict

1. **SetupOrchestrator state machine transition testing**. The proposal decomposes action _execution_ into separate executors but does not address how state _transitions_ are tested. The `SetupStateMachine::transition` is described as a "pure function" that remains unchanged, but the interaction between the state machine and executors creates a state-dependent dispatch that needs testing:
   - Does the state machine correctly select `CreateSpaceExecutor` vs `PairingExecutor` based on current state?
   - Does the state machine correctly update its state after an executor returns events?
   - If an executor returns an error, does the state machine transition to an error state?

   These are integration tests for the dispatcher, not unit tests for individual executors. The proposal must specify how the thin dispatcher's routing logic is tested. If transitions are implicit (pattern matching in `execute_actions`), they are hard to test exhaustively. If transitions are represented as explicit types (e.g., `Transition { from: State, action: Action, to: State }`), they can be tested as a table.

2. **SyncInbound decoder-deduplicator-applier composition testing gap**. The three extracted services form a sequential pipeline: decoder output feeds deduplicator, deduplicator output feeds applier. Testing each in isolation is valuable, but the proposal does not address composition testing. Specifically:
   - What happens if the decoder succeeds but the deduplicator detects a duplicate? Does the applier still run?
   - What happens if the decoder succeeds, deduplicator passes, but the applier fails? Does the deduplicator rollback?
   - The current `SyncInboundClipboardUseCase` handles these transitions inline. After decomposition, the orchestrating use case must coordinate these failure modes.

   The proposal should define a composition test strategy — either integration tests that wire all three real implementations together, or a test for the orchestrating use case that verifies the error flow between stages.

3. **AppDeps port bundle decomposition may INCREASE test coupling**. The proposal groups 12 clipboard ports into `ClipboardPorts`. A use case like `DeleteClipboardEntry` that only needs `ClipboardEntryRepositoryPort` and `ClipboardSelectionRepositoryPort` would now receive the entire `ClipboardPorts` bundle containing 12 ports. In tests:
   - **Before**: Test constructs `DeleteClipboardEntry` with 2 specific mocks. Clear what is tested.
   - **After**: Test constructs `ClipboardPorts` with 12 fields, 10 of which are irrelevant. Test author must either fill all 12 with noops or use a builder.

   This is a testability regression for fine-grained use cases. The proposal mentions "UseCase factory methods should only receive the relevant port bundle," but for use cases that need a subset of a bundle, this means either (a) the use case takes the whole bundle and ignores most of it, violating ISP at the test level, or (b) the use case still takes individual ports, making the bundle irrelevant.

4. **`OutboundPayloadPreparerPort` may be over-abstracted for testability**. This port combines three operations (encoding + encryption + framing) into one method. From a testability perspective, this makes the port a black box — tests cannot verify that encoding happened correctly but framing failed, for example. The original inline code, while messy, was at least step-debuggable. The port should either expose the three sub-steps (allowing tests to verify each) or provide a composition that returns intermediate results for assertion.

### C. Constraints They Missed

1. **SetupContext mutability testing**. The proposal moves mutable state (selected*peer_id, pairing_session_id, passphrase, joiner_offer) from SetupOrchestrator fields to a `SetupContext` struct. But `SetupContext` is shared across executors via `&SetupContext` (immutable reference). If executors need to update context (e.g., `PairingExecutor` sets `selected_peer_id`), the context must be interior-mutable (`RwLock<SetupContextInner>` or similar). This introduces locking in tests and makes assertion on context state require explicit lock acquisition. The proposal does not address this — it shows `&SetupContext` but some executors clearly need to \_write* to context.

2. **Executor dependency wiring in tests**. The proposal creates 7 executor structs, each with their own dependencies. In integration tests that need to test the full setup flow (e.g., "create space then mark complete"), the test must construct multiple executors and wire them into the dispatcher. This is the same AppDeps wiring problem at a smaller scale. Without a `TestSetupDispatcherBuilder`, the setup flow integration test will have 7 executor constructions with ~3-4 mocks each = ~25 mock objects.

3. **`ClipboardFanoutPort` return type `FanoutResult`**. This type is introduced but not defined. For testability, its shape matters: does it report per-peer success/failure? A `Vec<(PeerId, Result<()>)>` is testable (assert that specific peers succeeded/failed). An opaque `FanoutResult` with only aggregate info (e.g., "3 of 5 succeeded") loses information needed for targeted tests.

### D. Risk If Adopted As-Is

1. **State machine transitions become undertested**. The SetupOrchestrator refactor separates the state machine (transitions) from side effects (executors). This is architecturally sound but creates a testing gap at the integration point. If the dispatcher's routing logic is not explicitly tested, bugs can emerge where an executor is called in the wrong state, or events from an executor trigger incorrect state transitions. The state machine is "pure" but the dispatch table is not — it depends on runtime executor availability and correct routing.

2. **SyncInbound error recovery is undertested**. The deduplicator's `rollback()` method is called on applier failure. But after decomposition, this cross-service coordination lives in the orchestrating use case, which must be tested separately. If the orchestrator test is skipped (because each service was "already tested"), the rollback path may break silently.

3. **Port bundle coupling spreads to test fixtures**. Once `ClipboardPorts` exists as a bundle, `uc-test-support` must provide `TestClipboardPorts` with all 12 fields defaulted. Any new port added to the bundle requires updating the test fixture. If `ClipboardPorts` grows (e.g., new `ClipboardMetadataPort`), every test using `TestClipboardPorts::default()` automatically gains a noop for the new port without any test author being aware. This can mask missing test coverage for the new port.

### E. Suggested Revisions

1. **Make state machine transitions explicit and table-testable**:

   ```rust
   // In SetupOrchestrator (thin dispatcher)
   fn route_action(&self, state: &SetupState, action: &SetupAction) -> &dyn SetupActionExecutor {
       match (state, action) {
           (SetupState::Initial, SetupAction::CreateSpace { .. }) => &self.create_space_executor,
           // ... exhaustive routing table
       }
   }
   ```

   Then test the routing function independently from the executors.

2. **Add composition tests for SyncInbound pipeline**:

   ```rust
   #[test]
   fn applier_failure_triggers_dedup_rollback() {
       let dedup = InMemoryDeduplicator::new();
       let applier = FailingApplier; // always returns Err
       let uc = SyncInboundClipboardUseCase::new(decoder, dedup.clone(), applier, ...);
       let result = uc.execute(message).await;
       assert!(result.is_err());
       assert!(!dedup.is_duplicate(message.id)); // rollback happened
   }
   ```

   This test template should be specified in the proposal.

3. **Do not force port bundles on use cases that need fewer ports**. Use cases should accept either:
   - The full bundle (for use cases that need most ports in the bundle), OR
   - Individual ports (for use cases that need 1-2 ports from a bundle)

   The bundle is an _option_, not a _requirement_. The `UseCases` factory method can pass individual ports even if `AppDeps` stores bundles:

   ```rust
   pub fn delete_clipboard_entry(&self) -> DeleteClipboardEntry {
       DeleteClipboardEntry::new(
           self.runtime.deps.clipboard.entry_repo.clone(),
           self.runtime.deps.clipboard.selection_repo.clone(),
       )
   }
   ```

   This preserves ISP at the use case level while organizing storage at the AppDeps level.

4. **Define `FanoutResult` explicitly with per-peer outcomes** to enable targeted test assertions:

   ```rust
   pub struct FanoutResult {
       pub outcomes: Vec<(PeerId, Result<(), FanoutError>)>,
   }
   ```

5. **Specify `SetupContext` mutability model explicitly**. If executors need to write to context, use:
   ```rust
   trait SetupActionExecutor: Send + Sync {
       async fn execute(&self, context: &mut SetupContext) -> Result<Vec<SetupEvent>, SetupError>;
   }
   ```
   `&mut SetupContext` eliminates interior mutability complexity and makes test assertions straightforward (`assert_eq!(context.selected_peer_id, Some(expected_id))`).

---

## Cross-Cutting Observations

### 1. Testing Strategy Gap Across All Proposals

Both proposals describe decomposition and new abstractions but neither provides a concrete testing strategy for the _new_ abstractions they introduce. The contract-error proposal introduces `CommandError` mapping logic without specifying how to test mappings. The app-flow proposal introduces 7 executor structs, 3 pipeline services, and 2 new ports without specifying how to test their composition. Each proposal assumes "decomposition implies testability" — but decomposition only creates the _possibility_ of testability. Actual testability requires test patterns, shared fixtures, and composition tests to be specified alongside the code.

### 2. The Bundle vs. ISP Tension

The arch-guardian proposes aggregate port traits (`ClipboardStoragePorts`), the app-flow reviewer proposes port bundle structs (`ClipboardPorts`), and the testability proposal recommends bundle-level noop mocks (`NoopClipboardPorts`). These three approaches must converge on a single pattern. Currently:

- Aggregate traits (arch-guardian): `trait ClipboardStoragePorts` with accessor methods
- Bundle structs (app-flow): `struct ClipboardPorts` with pub fields
- Test mocks (testability): `NoopClipboardPorts` implementing... which pattern?

If aggregate traits are chosen, `NoopClipboardPorts` implements `ClipboardStoragePorts`. If bundle structs are chosen, `NoopClipboardPorts` is a function returning a `ClipboardPorts` with noop fields. These are different designs with different test ergonomics. The three proposals must align on one pattern before implementation.

### 3. Error Type Testing Burden

The contract-error proposal introduces ~5-8 new error enums. The app-flow proposal introduces `SetupError` (implicit in executor trait). The infra-runtime proposal will likely need `TaskRegistryError`, `ShutdownError`. Combined, we are looking at 10-15 new error types across the codebase. Each needs:

- Construction tests (can all variants be created)
- Mapping tests (do infrastructure errors map to correct variants)
- Display/serialization tests (do error messages make sense)

Without a coordinated error testing strategy (perhaps in `uc-test-support`), error testing will be inconsistent across domains. The error type proliferation risk flagged by contract-error-reviewer is real, but the mitigation (group by domain cluster) does not address the testing burden.
