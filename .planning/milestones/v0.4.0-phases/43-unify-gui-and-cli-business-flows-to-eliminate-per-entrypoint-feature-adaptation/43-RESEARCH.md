# Phase 43: Unify GUI and CLI Business Flows - Research

**Researched:** 2026-03-19
**Domain:** Shared application-flow entrypoints across Tauri GUI and CLI
**Confidence:** HIGH

## User Constraints

- No `43-CONTEXT.md` exists for this phase.
- No locked phase-specific decisions were provided before research.
- Research scope is constrained by repo rules: keep `uc-core` pure, keep business-flow logic out of `uc-tauri` commands and bootstrap wiring, and preserve hexagonal layering.

## Summary

The codebase already has the right structural base for Phase 43: `uc-app::runtime::CoreRuntime` is Tauri-free, `uc-bootstrap` can build both GUI and non-GUI runtimes, `CoreUseCases` centralizes a large set of shared business operations, and Tauri setup flow already routes through a thin command layer into `SetupOrchestrator`. The main remaining gap is not raw dependency wiring. It is that GUI and CLI still enter the app through different shapes: CLI commands repeatedly assemble a non-GUI runtime and manually shape outputs, while several Tauri commands still aggregate multiple use cases and perform business-adjacent branching in the adapter layer.

The biggest existing adaptation seam is the CLI bootstrap pattern duplicated in every direct command. `devices`, `space-status`, and all `clipboard` subcommands each repeat `build_cli_context_with_profile()` -> `get_storage_paths()` -> `build_non_gui_runtime()` -> `CoreUseCases::new(&runtime)`. On the GUI side, `AppRuntime::usecases()` already gives a shared accessor, but some commands still compose cross-use-case flows themselves, especially clipboard and pairing status commands. That is the real duplication risk for future feature work: each new surface would need its own “assembled” business flow.

There is one important limit to current unification scope: `build_non_gui_runtime()` intentionally uses `SetupAssemblyPorts::placeholder(&deps)` and a no-op watcher control. That is sufficient for current direct CLI query/history flows, but it is not a safe foundation for full setup/pairing parity. If Phase 43 is planned as “unify the way both surfaces invoke existing business flows,” it is ready now. If it is planned as “make all GUI flows available from CLI,” bootstrap work must expand first.

**Primary recommendation:** Plan Phase 43 around shared `uc-app` flow entrypoints and a shared non-GUI runtime accessor, not around moving GUI/CLI response DTOs across layers. Put business decisions and cross-use-case aggregation into `uc-app`; keep Tauri payload models and CLI `Display` formatting at the edges.

## Standard Stack

### Core

| Library / Module                  | Version           | Purpose                         | Why Standard                                                                 |
| --------------------------------- | ----------------- | ------------------------------- | ---------------------------------------------------------------------------- |
| `uc-app::runtime::CoreRuntime`    | workspace `0.1.0` | Tauri-free runtime host         | Already the shared runtime boundary for non-GUI and GUI-backed flows         |
| `uc-app::usecases::CoreUseCases`  | workspace `0.1.0` | Shared app-layer entrypoint set | Already exposes most reusable business operations without Tauri dependencies |
| `uc-bootstrap`                    | workspace `0.1.0` | Sole composition root           | Already owns scene-specific builders and non-GUI runtime construction        |
| `uc-tauri::bootstrap::AppRuntime` | workspace `0.1.0` | GUI adapter runtime wrapper     | Already cleanly layers Tauri-only accessors on top of `CoreRuntime`          |
| `tokio`                           | `1.x` (`full`)    | Async runtime                   | Already used by `uc-app`, `uc-bootstrap`, `uc-cli`, and `uc-tauri`           |
| `clap`                            | `4.5.54` resolved | CLI parsing                     | Current CLI entrypoint stack                                                 |
| `serde` / `serde_json`            | `1.x`             | Surface DTO serialization       | Required for both Tauri payloads and CLI `--json` mode                       |
| `tracing`                         | `0.1.x`           | Structured observability        | Required by repo rules and already pervasive                                 |

### Supporting

| Library / Module                            | Version           | Purpose                                           | When to Use                                                                                   |
| ------------------------------------------- | ----------------- | ------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `uc-app::usecases::SetupOrchestrator`       | workspace `0.1.0` | Thin shared setup state machine entrypoint        | Use as the reference pattern for “surface command -> shared flow”                             |
| `uc-app::usecases::AppLifecycleCoordinator` | workspace `0.1.0` | Lifecycle boot sequencing                         | Use for shared lifecycle flow contracts, but keep Tauri-only emitter assembly out of `uc-app` |
| `uc-bootstrap::build_non_gui_runtime()`     | workspace `0.1.0` | Non-GUI runtime construction                      | Use for shared CLI/daemon access, but recognize its current placeholder setup wiring          |
| `uc-app::usecases::pairing::PairingFacade`  | workspace `0.1.0` | Existing abstraction around pairing orchestration | Extend this pattern instead of binding commands directly to `PairingOrchestrator` state       |

### Alternatives Considered

| Instead of                             | Could Use                                     | Tradeoff                                                                 |
| -------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------ |
| Shared `uc-app` flow entrypoints       | Keep adding surface-specific command logic    | Faster locally, but guarantees continued GUI/CLI drift                   |
| Shared runtime/facade acquisition      | Macro/helper duplication in `uc-cli` commands | Reduces boilerplate only; does not fix business-flow duplication         |
| App-layer query DTOs plus edge mapping | Move Tauri/CLI DTOs into `uc-app`             | Breaks adapter boundaries and couples app layer to presentation concerns |

**Version verification:** Current crate versions were verified from local `Cargo.toml` files and a successful `cargo check -p uc-cli -p uc-tauri -p uc-app` run on 2026-03-19. `clap` resolved as `4.5.54` during that build.

## Architecture Patterns

### Recommended Project Structure

```text
src-tauri/crates/
├── uc-app/
│   └── src/usecases/
│       ├── mod.rs                    # expose new shared flow accessors here
│       ├── clipboard/                # add shared clipboard flow/query modules here
│       ├── pairing/                  # add shared pairing snapshot/facade entrypoints here
│       └── setup/                    # keep setup orchestration here; already the reference shape
├── uc-bootstrap/
│   ├── src/builders.rs               # keep scene selection and config resolution here
│   └── src/non_gui_runtime.rs        # add shared non-GUI runtime/context helper here
├── uc-cli/
│   └── src/commands/                 # thin shell: parse args, call shared flow, format output
└── uc-tauri/
    ├── src/bootstrap/runtime.rs      # expose shared app-layer accessors to GUI
    └── src/commands/                 # thin shell: tracing, error mapping, payload mapping
```

### Pattern 1: Shared Flow in `uc-app`, Thin Surface Adapters

**What:** Put cross-use-case decisions and aggregation into `uc-app`, then let GUI and CLI map the shared result into surface-specific DTOs.

**When to use:** Any flow that currently requires multiple use cases or business-condition checks in a command handler.

**Example:**

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/setup.rs
#[tauri::command]
pub async fn start_join_space(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<SetupState, CommandError> {
    let orchestrator = runtime.usecases().setup_orchestrator();
    orchestrator.join_space().await.map_err(CommandError::internal)
}
```

**Why this is the reference:** `setup.rs` already behaves like the target architecture. The command is an adapter, not a workflow host.

### Pattern 2: One Shared Non-GUI Runtime Acquisition Path

**What:** CLI direct-mode commands should stop rebuilding `CoreRuntime` independently. Add one shared helper in `uc-bootstrap` or `uc-cli` that returns a ready non-GUI app context.

**When to use:** Every direct CLI flow that currently repeats bootstrap setup.

**Example:**

```rust
// Source: src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs
pub fn build_non_gui_runtime(
    deps: AppDeps,
    storage_paths: AppPaths,
) -> anyhow::Result<CoreRuntime> {
    let emitter: Arc<dyn HostEventEmitterPort> = Arc::new(LoggingHostEventEmitter);
    let emitter_cell = Arc::new(std::sync::RwLock::new(emitter));
    // ...
    Ok(CoreRuntime::new(/* ... */))
}
```

**Planning implication:** Phase 43 should add a higher-level helper around this, not duplicate the current four-step bootstrap sequence again.

### Pattern 3: Keep Surface Models at the Edge

**What:** Shared app flows should return app-layer DTOs or domain results. CLI `Display` structs and Tauri payload structs remain surface adapters.

**When to use:** Any unification of clipboard rows, paired-peer snapshots, lifecycle status, or similar read models.

**Example:**

```rust
// Source: src-tauri/crates/uc-cli/src/commands/clipboard.rs
#[derive(Serialize)]
struct ClipboardListOutput {
    entries: Vec<ClipboardEntryRow>,
    count: usize,
}
```

**Planning implication:** Do not move `ClipboardListOutput`, `PairedPeer`, `P2PPeerInfo`, or `ClipboardEntriesResponse` into `uc-app`.

### Pattern 4: Pairing and Lifecycle Should Follow a Facade, Not Raw State Injection

**What:** GUI commands that currently take `State<Arc<PairingOrchestrator>>` or reconstruct lifecycle coordinator state should be routed through shared app-layer accessors or façade traits.

**When to use:** Pairing actions and any future CLI parity for pairing/setup/lifecycle.

**Example:**

```rust
// Source: src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs
#[async_trait::async_trait]
impl PairingFacade for PairingOrchestrator {
    async fn initiate_pairing(&self, peer_id: String) -> anyhow::Result<SessionId> {
        Self::initiate_pairing(self, peer_id).await
    }
}
```

**Planning implication:** Reuse or expand `PairingFacade`; do not couple future shared flows to Tauri-managed state injection.

### Anti-Patterns to Avoid

- **Moving GUI/CLI DTOs into `uc-app`:** this turns application code into presentation code.
- **Adding more command-local orchestration:** examples already exist in Tauri clipboard and pairing status commands; do not copy that pattern into CLI.
- **Exposing `AppDeps` to commands for convenience:** `uc-tauri` rules explicitly forbid this for business IO.
- **Trying to unify setup/pairing CLI behavior on top of placeholder non-GUI setup wiring:** current `build_non_gui_runtime()` is not assembled for that.

## Don't Hand-Roll

| Problem                          | Don't Build                                                           | Use Instead                                                              | Why                                                                                  |
| -------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| CLI direct-mode startup          | Per-command bootstrap sequence                                        | One shared CLI app/runtime helper                                        | Repeating bootstrap in each command guarantees drift and inconsistent error handling |
| Clipboard surface aggregation    | Separate GUI and CLI “clipboard flow” logic                           | Shared `uc-app` clipboard flow/query entrypoints                         | Same query, readiness, and aggregation logic will otherwise diverge                  |
| Pairing peer snapshots           | Multiple command-local joins across paired/discovered/connected peers | Shared `uc-app` pairing snapshot/query use case                          | The aggregation already spans several ports and is easy to fork incorrectly          |
| Pairing command access           | Raw `State<Arc<PairingOrchestrator>>` in entrypoints                  | `PairingFacade`-backed runtime/usecase accessor                          | Keeps future CLI and GUI on the same business interface                              |
| Lifecycle ready flow composition | Rebuilding coordinator assembly per surface                           | Shared app-layer lifecycle entrypoint plus adapter-only emitter assembly | Lifecycle sequencing is already a use-case concern, not an entrypoint concern        |

**Key insight:** The thing to unify is not presentation output. It is the application entrypoint shape: “surface adapter calls one shared flow object.” If the phase only removes boilerplate but leaves command-local orchestration intact, the adaptation problem remains.

## Common Pitfalls

### Pitfall 1: Unifying Presentation Instead of Flow

**What goes wrong:** The refactor moves Tauri response structs or CLI `Display` structs into `uc-app`.
**Why it happens:** The visible duplication is DTO mapping, so it looks like the easiest thing to centralize.
**How to avoid:** Centralize app-layer query/command results, then keep surface-specific mapping one layer out.
**Warning signs:** `uc-app` starts importing Tauri-facing models or formatting-oriented fields.

### Pitfall 2: Planning Pairing/Setup CLI Parity on a Placeholder Runtime

**What goes wrong:** The plan assumes all GUI flows can immediately run through current non-GUI runtime helpers.
**Why it happens:** `build_non_gui_runtime()` returns a real `CoreRuntime`, so it looks equivalent to GUI.
**How to avoid:** Treat current non-GUI setup wiring as read/query-capable only until `SetupAssemblyPorts::placeholder()` is replaced for the needed flows.
**Warning signs:** The plan references CLI pairing/setup commands without also changing `uc-bootstrap/src/non_gui_runtime.rs` and `uc-bootstrap/src/assembly.rs`.

### Pitfall 3: Leaving Cross-Use-Case Aggregation in Tauri Commands

**What goes wrong:** Commands still call multiple use cases, merge state, and branch on business conditions.
**Why it happens:** The command already has access to `runtime.usecases()`, so aggregation feels “close enough.”
**How to avoid:** If a command needs more than one use case or shared readiness logic, add an app-layer flow.
**Warning signs:** Command files grow new helper functions like `map_*`, `*_snapshot`, or manual joins over several use-case results.

### Pitfall 4: Breaking Hex Boundaries While Chasing Convenience

**What goes wrong:** New shared flows in `uc-app` start depending on `uc-tauri`, `tauri`, or concrete `uc-infra` adapters.
**Why it happens:** GUI behavior is currently the most complete surface.
**How to avoid:** Keep `uc-app` on ports and app-layer abstractions only; leave concrete adapter creation in `uc-bootstrap` or `uc-tauri`.
**Warning signs:** New `uc-app` modules need `tauri::State`, `AppHandle`, or direct adapter imports.

## Code Examples

Verified patterns from the current codebase:

### Thin Command Wrapper

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/setup.rs
#[tauri::command]
pub async fn cancel_setup(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<SetupState, CommandError> {
    let orchestrator = runtime.usecases().setup_orchestrator();
    orchestrator.cancel_setup().await.map_err(CommandError::internal)
}
```

### Shared Tauri/Core Accessor Split

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
pub struct AppUseCases<'a> {
    app_runtime: &'a AppRuntime,
    core: uc_app::usecases::CoreUseCases<'a>,
}

impl<'a> std::ops::Deref for AppUseCases<'a> {
    type Target = uc_app::usecases::CoreUseCases<'a>;
    fn deref(&self) -> &Self::Target { &self.core }
}
```

### Placeholder Non-GUI Setup Wiring

```rust
// Source: src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs
let setup_ports = SetupAssemblyPorts::placeholder(&deps);
let watcher_control: Arc<dyn WatcherControlPort> = Arc::new(NoopWatcherControl);
```

### Business Rule Already Documented in Bootstrap Guidance

```text
// Source: src-tauri/crates/uc-tauri/src/bootstrap/README.md
Any "if X then Y" logic about business flows belongs in use cases.
```

## State of the Art

| Old Approach                                                       | Current Approach                                               | When Changed | Impact                                                                  |
| ------------------------------------------------------------------ | -------------------------------------------------------------- | ------------ | ----------------------------------------------------------------------- |
| Each runtime built its own business entrypoint shape               | `CoreRuntime` + `CoreUseCases` + `uc-bootstrap` scene builders | Phases 38-41 | Shared runtime exists, but shared flow entrypoints are still incomplete |
| GUI setup flow partly wired in runtime/bootstrap layers            | `SetupOrchestrator` assembled once and used from thin commands | Phase 38     | Setup is the best pattern to copy for Phase 43                          |
| CLI added features by direct runtime bootstrap inside each command | Current clipboard/device/status commands still do this         | Phase 41-42  | This is the first duplication seam to eliminate                         |
| Tauri commands directly own some cross-use-case aggregation        | Clipboard/pairing commands still do this                       | Current      | Future CLI parity will fork unless moved into `uc-app`                  |

**Deprecated/outdated:**

- Command-local business-flow assembly as the default pattern: it conflicts with both repo rules and the bootstrap README boundary guidance.
- Treating `build_non_gui_runtime()` as GUI-equivalent for setup/pairing flows: current placeholder setup assembly makes that assumption unsafe.

## Open Questions

1. **Does Phase 43 include interactive CLI pairing/setup commands, or only unification of already-shared flows?**
   - What we know: current CLI only covers direct read/history flows plus daemon status.
   - What's unclear: whether planner should expand runtime assembly for interactive pairing/setup now.
   - Recommendation: split if necessary. Phase 43 should first unify shared flow entrypoints for existing clipboard/device/status operations. Make broader CLI parity a follow-up unless explicitly required.

2. **Should shared flow DTOs live as new `uc-app` read models or as existing use-case result compositions?**
   - What we know: current Tauri and CLI each shape their own adapter DTOs; `CoreUseCases` mostly returns granular use cases.
   - What's unclear: whether planner prefers a small number of new flow/query modules or a wider expansion of `CoreUseCases`.
   - Recommendation: add dedicated flow/query modules in `uc-app` for cross-use-case compositions, then expose them through `CoreUseCases`.

3. **How much of `AppUseCases` should move or be mirrored for non-GUI access?**
   - What we know: `AppUseCases` owns Tauri-only accessors and uc-infra-backed clipboard sync helpers.
   - What's unclear: whether planner should unify lifecycle/pairing access through `CoreRuntime` or keep certain flows GUI-only.
   - Recommendation: only lift flows whose dependencies can remain port-based or bootstrap-assembled without Tauri. Keep Tauri-only operations in `AppUseCases`.

## Validation Architecture

### Test Framework

| Property           | Value                                                        |
| ------------------ | ------------------------------------------------------------ |
| Framework          | Rust `cargo test` + crate integration tests                  |
| Config file        | none                                                         |
| Quick run command  | `cd src-tauri && cargo test -p uc-cli --test cli_smoke`      |
| Full suite command | `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-cli` |

### Phase Requirements -> Test Map

| Req ID  | Behavior                                                                           | Test Type        | Automated Command                                                                | File Exists? |
| ------- | ---------------------------------------------------------------------------------- | ---------------- | -------------------------------------------------------------------------------- | ------------ |
| PH43-01 | CLI direct commands acquire shared app/runtime context through one path            | integration      | `cd src-tauri && cargo test -p uc-cli --test cli_smoke`                          | ✅           |
| PH43-02 | GUI and CLI clipboard flows call the same app-layer business entrypoint            | integration      | `cd src-tauri && cargo test -p uc-tauri clipboard_commands_stats_favorites_test` | ✅           |
| PH43-03 | Pairing/device status aggregation moves out of Tauri commands into shared app flow | unit/integration | `cd src-tauri && cargo test -p uc-app pairing`                                   | ✅ partial   |
| PH43-04 | Setup/lifecycle shared flow access remains thin at adapter layer                   | integration      | `cd src-tauri && cargo test -p uc-app --test setup_flow_integration_test`        | ✅           |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-cli --test cli_smoke`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-app -p uc-tauri -p uc-cli`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-app/tests/shared_flow_clipboard_test.rs` — proves one app-layer clipboard flow drives both CLI and GUI callers
- [ ] `src-tauri/crates/uc-app/tests/shared_flow_pairing_snapshot_test.rs` — covers paired/discovered/connected peer aggregation outside Tauri commands
- [ ] `src-tauri/crates/uc-cli/tests/cli_flow_parity_test.rs` — verifies CLI command output still works after shared runtime/helper extraction
- [ ] `src-tauri/crates/uc-tauri/tests/shared_flow_command_contract_test.rs` — asserts Tauri commands remain thin wrappers over shared flow accessors

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-app/src/runtime.rs` - current `CoreRuntime` boundary and shared runtime state
- `src-tauri/crates/uc-app/src/usecases/mod.rs` - current shared app-layer accessor surface
- `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs` - reference thin-command/shared-flow pattern
- `src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs` - existing `PairingFacade` implementation
- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` - lifecycle sequencing responsibilities
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` - current non-GUI runtime construction and placeholder setup wiring
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` - `SetupAssemblyPorts::from_network()` vs `placeholder()` distinction
- `src-tauri/crates/uc-bootstrap/src/builders.rs` - scene-specific builder responsibilities
- `src-tauri/crates/uc-cli/src/commands/devices.rs` - duplicated CLI direct bootstrap pattern
- `src-tauri/crates/uc-cli/src/commands/clipboard.rs` - duplicated CLI direct bootstrap pattern and CLI-specific output mapping
- `src-tauri/crates/uc-cli/src/commands/space_status.rs` - duplicated CLI direct bootstrap pattern
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - current GUI accessor split between `CoreUseCases` and Tauri-only accessors
- `src-tauri/crates/uc-tauri/src/commands/setup.rs` - thin command pattern already in place
- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` - command-local clipboard aggregation and readiness branching
- `src-tauri/crates/uc-tauri/src/commands/pairing.rs` - command-local peer snapshot aggregation and raw orchestrator injection
- `src-tauri/crates/uc-tauri/src/bootstrap/README.md` - repo-local boundary guidance for business-flow logic
- `src-tauri/crates/uc-cli/tests/cli_smoke.rs` - current CLI validation coverage
- `src-tauri/crates/uc-tauri/tests/usecases_accessor_test.rs` - current runtime accessor coverage
- `src-tauri/crates/uc-app/tests/setup_flow_integration_test.rs` - current shared-flow integration coverage

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-app/AGENTS.md` - crate-level guidance on keeping orchestration in `uc-app`
- `src-tauri/crates/uc-app/src/usecases/AGENTS.md` - rule that command-side “read state then execute” flows should become dedicated usecase entrypoints

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - verified from local workspace manifests and successful crate build
- Architecture: HIGH - based on current crate boundaries and concrete command/runtime code paths
- Pitfalls: HIGH - directly inferred from current duplication seams and existing repo boundary rules

**Research date:** 2026-03-19
**Valid until:** 2026-04-18
