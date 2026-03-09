# Phase 10: Boundary Repair Baseline - Context

**Gathered:** 2026-03-06
**Status:** Ready for planning

<domain>
## Phase Boundary

Restore strict `uc-app → uc-core ← uc-infra/uc-platform` dependency direction and close command-layer penetration paths. This phase does NOT decompose god objects (Phase 13), does NOT introduce typed error contracts (Phase 11), and does NOT add lifecycle governance (Phase 12). It only repairs dependency direction violations and closes the holes where command-layer code bypasses use cases.

</domain>

<decisions>
## Implementation Decisions

### Locking down runtime.deps access (BOUND-01, BOUND-02)

- Make `pub deps: AppDeps` **private** on `AppRuntime`
- Add **thin facade methods** directly on `AppRuntime` for simple read-only state access (e.g., `runtime.device_id()`, `runtime.is_encryption_ready()`) — used for tracing/logging and session checks in commands
- Commands that perform business operations (settings access, etc.) must be **routed through existing use cases**; if no use case exists for the operation, create one
- **No exceptions for tracing** — all `deps` access in command files goes through facade methods, including device_id used in tracing attributes
- **AppDeps struct shape is not changed** — this phase only restricts visibility, not restructures the container (restructuring is Phase 13)
- Bootstrap/wiring code: use **constructor injection** — `AppRuntime::new()` takes pre-assembled use cases rather than raw AppDeps, so deps are fully private even from bootstrap. Bootstrap assembles the wiring, passes use case accessors to AppRuntime
- **Document the approved access pattern** in a `///` doc block on `AppRuntime` explaining: commands use `runtime.usecases()` for business operations and `runtime.device_id()` etc. for state reads

### Platform decode path routing (BOUND-03)

- **Inject `TransferPayloadDecryptorPort` into `Libp2pNetworkAdapter::new()`** — add it to the struct alongside existing `encryption_session` and `policy_resolver` ports
- **Inject `TransferPayloadEncryptorPort` at the same time** for consistency — both ports added in one refactor pass
- The concrete `ChunkedDecoder` / `TransferPayloadDecryptorAdapter` from `uc-infra` is wired at bootstrap (uc-tauri), not inside the platform adapter
- **Remove `uc-infra` from `uc-platform/Cargo.toml`** once the single `uc_infra::clipboard::ChunkedDecoder::decode_from` call is replaced — enforces the boundary structurally at build time

### Non-domain port eviction from uc-core (BOUND-04)

- **Evict all 6 non-domain ports** in this phase: `AutostartPort`, `UiPort`, `AppDirsPort`, `WatcherControlPort`, `IdentityStorePort`, `ObservabilityPort`
- Evicted ports **move to `uc-platform/src/ports/`** — they are platform-facing interface contracts
- Use cases in `uc-app` that currently import these ports from `uc-core` (e.g., `apply_autostart.rs` uses `AutostartPort`) will have their **affected use cases moved into `uc-platform`** rather than adding a `uc-platform` dependency to `uc-app`
- After eviction, `uc-core/src/ports/` should contain only domain-relevant port contracts

### Migration safety & sequencing

- **Primary safety signal: Rust compiler** — boundary enforcement is structural; making `deps` private and removing the `uc-infra` Cargo dependency means violations don't compile
- **No pre-refactor integration tests needed** — existing tests cover behavior; the compiler enforces boundary contracts
- **Plan order preserved as in ROADMAP.md**: 10-01 (runtime/command boundaries), 10-02 (decode port injection), 10-03 (port eviction)
- **Each plan must compile and pass `cargo check` before commit** — no intermediate broken states land on dev/main

### Claude's Discretion

- Exact set of facade method names on AppRuntime beyond the discussed examples
- Whether any ports among the 6 to evict need re-classification after closer inspection (the planner may reassess ObservabilityPort if it has domain-level callers)
- Exact wording of the approved-access doc block on AppRuntime

</decisions>

<specifics>
## Specific Ideas

- Reference issue #214 (https://github.com/UniClipboard/UniClipboard/issues/214) as the canonical description of all boundary defects — researcher and planner should use it for full context beyond what's captured here
- Constructor injection decision for AppRuntime mirrors the "make the wrong thing uncompilable" philosophy: if bootstrap can't access deps through public fields, violations are caught at compile time rather than code review
- The uc-infra Cargo.toml removal from uc-platform is a hard enforcement mechanism — future regressions in the platform→infra direction don't compile

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `TransferPayloadDecryptorPort` / `TransferPayloadEncryptorPort` (`uc-core/src/ports/security/transfer_crypto.rs`): Port contracts already exist — no new abstractions needed for BOUND-03, only wiring changes
- `UseCases` accessor struct (`uc-tauri/src/bootstrap/runtime.rs`): Existing pattern for use case access; facade methods on AppRuntime follow the same file
- `AppDeps` struct (`uc-app/src/deps.rs`): 30+ field god container — shape preserved this phase, only visibility changes

### Established Patterns

- Hexagonal architecture: ports in `uc-core`, implementations in `uc-infra`, adapters in `uc-platform`, wiring in `uc-tauri/bootstrap`
- Port injection via constructor: `Libp2pNetworkAdapter::new()` already receives `encryption_session`, `policy_resolver` this way — decryptor/encryptor follow the same pattern
- `runtime.usecases()` accessor: established approved path for commands; facade methods sit alongside this in AppRuntime

### Integration Points

- `uc-tauri/src/commands/*.rs` (5 files): all direct `runtime.deps.*` access must be replaced — clipboard.rs, settings.rs, encryption.rs, pairing.rs are confirmed violators
- `uc-platform/src/adapters/libp2p_network.rs`: single `uc_infra::clipboard::ChunkedDecoder::decode_from()` call at line ~972 is the only infra violation; struct constructor at ~256
- `uc-core/src/ports/mod.rs`: 6 non-domain ports exported here; after eviction their `pub mod` and `pub use` entries are removed
- `uc-platform/Cargo.toml`: `uc-infra` dependency at line 13 — remove after decode path fix

</code_context>

<deferred>
## Deferred Ideas

- **AppDeps decomposition** (splitting into SecurityDeps, ClipboardDeps etc.) — explicitly deferred to Phase 13
- **Typed error migration into port surfaces** — Phase 11 (CONTRACT-01 through CONTRACT-04) and ARCHNEXT-01
- **Lifecycle task tracking** — Phase 12

</deferred>

---

_Phase: 10-boundary-repair-baseline_
_Context gathered: 2026-03-06_
