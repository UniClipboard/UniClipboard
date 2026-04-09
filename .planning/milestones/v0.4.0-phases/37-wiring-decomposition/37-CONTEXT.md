# Phase 37: Wiring Decomposition - Context

**Gathered:** 2026-03-17
**Status:** Ready for planning

<domain>
## Phase Boundary

Split `wiring.rs` (6328 lines) into a pure Rust assembly module (`assembly.rs`, no Tauri types) and a Tauri-specific event loop module (retains `wiring.rs` name). Migrate ALL remaining `app.emit()` calls in wiring.rs and file_transfer_wiring.rs to `HostEventEmitterPort`, adding new HostEvent domain variants. After migration, `start_background_tasks` loses its `AppHandle<R>` parameter entirely. Move command registration (`invoke_handler` macro) from main.rs into the Tauri-specific module so it owns event loop setup, app handle wiring, AND command registration.

**⚠ ROADMAP SC#2 and SC#4 interpretation adjustment (explicit, not silent):**

ROADMAP SC#2 says "it is the only place that imports tauri types." Taken literally across the entire uc-tauri crate, this is unachievable in Phase 37 — commands/, adapters/, events/, preview_panel/, quick_panel/, tray/, services/ all import tauri unconditionally, and uc-tauri's Cargo.toml has `tauri` as a non-optional dependency (Cargo.toml:20). Gating the entire crate would require making tauri optional + `#[cfg(feature)]` on most of the 10 pub modules in lib.rs — that is Phase 40 (uc-bootstrap) scope.

**Phase 37 interpretation:** SC#2's "only place" applies **within the wiring split pair only** — after the split, assembly.rs has zero tauri imports and wiring.rs is the Tauri-side counterpart. Other bootstrap/ modules (runtime.rs holds `AppHandle`, run.rs uses `tauri::AppHandle`) are NOT changed in this phase and continue to import tauri. Commands, adapters, and other uc-tauri modules are also out of scope for this constraint.

ROADMAP SC#4 says "`cargo check` on the pure-assembly module succeeds without tauri in its dependency tree." Since assembly.rs lives inside uc-tauri (which unconditionally depends on tauri), a real `cargo check` without tauri requires either crate extraction or crate-wide feature gating — both disproportionate for Phase 37.

**Phase 37 interpretation:** SC#4 is satisfied by structural proof: assembly.rs has zero tauri/tauri-plugin imports (verified by grep + CI lint), and its public API surface uses only types from uc-core, uc-app, uc-infra, uc-platform (all Tauri-free crates). The full `cargo check -p` independence is achieved in Phase 40 when assembly code moves to uc-bootstrap. **The ROADMAP.md Phase 37 SC#4 wording should be updated to reflect this staged approach.**

**In scope:**

- File split: wiring.rs → assembly.rs (pure) + wiring.rs (Tauri event loops)
- Migrate all 14 remaining app.emit() calls in wiring.rs to HostEventEmitterPort
- Migrate 5 AppHandle<R> functions in file_transfer_wiring.rs to HostEventEmitterPort
- Add new HostEvent sub-enums: PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent
- Remove AppHandle<R> parameter from start_background_tasks after all emits migrated
- Extend TauriEventEmitter with new event → Tauri event name + payload mappings
- Move command registration (invoke_handler![...] block, currently main.rs:852-927) into Tauri-specific module — wiring.rs or a dedicated commands_registration.rs
- Grep/CI lint rule: assembly.rs must contain zero `tauri::`, `AppHandle`, `Emitter`, `Runtime` imports
- Update ROADMAP.md Phase 37 SC#2 and SC#4 wording to match the staged interpretation above

**Out of scope:**

- Moving files to other crates (deferred to Phase 38+)
- Creating independent crate for assembly module (deferred to Phase 40)
- Crate-wide feature gating of tauri dependency (deferred to Phase 40)
- Making tauri an optional dependency in uc-tauri/Cargo.toml (deferred to Phase 40)
- Gating non-bootstrap modules (commands/, adapters/, events/, etc.) behind feature flags (deferred to Phase 40)
- Commands-layer emits (commands/pairing.rs, commands/clipboard.rs, encryption.rs, tray.rs)
- emit_to for quick-panel/preview-panel (window-targeted, not broadcast)
- clipboard monitor heartbeat (clipboard_monitor.rs:43)

</domain>

<decisions>
## Implementation Decisions

### Split boundary

- Pure assembly module contains dependency construction AND Tauri-free utility functions: `wire_dependencies`, `wire_dependencies_with_identity_store`, `get_storage_paths`, `create_infra_layer`, `resolve_pairing_device_name`, `resolve_pairing_config`, and related helper functions
- `resolve_pairing_device_name` and `resolve_pairing_config` belong in assembly.rs because they are pure helpers (take `Arc<dyn SettingsPort>`, no Tauri types) and are called from multiple non-event-loop sites: commands/settings.rs, adapters/lifecycle.rs, AND wiring.rs event loops
- `start_background_tasks` and all event loop code stay in wiring.rs (Tauri module) — but after all app.emit() calls are migrated, AppHandle<R> is removed from its signature
- `WiredDependencies` struct definition lives in assembly.rs (it's the return type of wire_dependencies)
- `BackgroundRuntimeDeps` struct definition stays in wiring.rs (only used by start_background_tasks)

### app.emit() migration — complete

- ALL 14 remaining app.emit() calls in wiring.rs are migrated to HostEventEmitterPort in this phase
- This includes: setup-state-changed (1), space-access-completed + p2p-space-access-completed (2), pairing-verification (7), pairing-events-subscribe-failure/recovered (2), inbound-clipboard-subscribe-error/retry (2)
- New HostEvent sub-enums follow Phase 36 pattern: `HostEvent::Pairing(PairingHostEvent)`, `HostEvent::Setup(SetupHostEvent)`, `HostEvent::SpaceAccess(SpaceAccessHostEvent)`
- TauriEventEmitter is extended with event name mapping and camelCase payload DTOs for each new domain
- Frontend contract tests mandatory for every new event mapping (same pattern as Phase 36)

### file_transfer_wiring.rs migration — complete

- All 5 functions (handle_transfer_progress, handle_transfer_completed, handle_transfer_failed, spawn_timeout_sweep, reconcile_on_startup) migrated from AppHandle<R> to HostEventEmitterPort
- File stays in uc-tauri/bootstrap/ — no location change in this phase
- FileTransferStatusPayload remains as-is (already has serde annotations for Tauri)
- After migration, file_transfer_wiring.rs has zero Tauri imports (but stays in uc-tauri crate for now)

### Module naming and organization

- Pure assembly: `assembly.rs` in uc-tauri/src/bootstrap/
- Tauri event loops: retains `wiring.rs` name in uc-tauri/src/bootstrap/
- mod.rs uses `pub use assembly::*` and `pub use wiring::*` — external import paths unchanged (transparent refactor)

### Command registration ownership

- ROADMAP SC#2 requires the Tauri-specific module to own "event loop setup, app handle wiring, and command registration"
- Within the wiring split pair, this is achieved: wiring.rs (Tauri-side) owns event loops + app handle; command registration moves from main.rs into wiring.rs or a dedicated bootstrap helper
- Currently command registration lives in main.rs:852-927 (`invoke_handler![...]` macro with ~60 commands)
- Move the `invoke_handler` generation into the Tauri module (wiring.rs or a dedicated helper function) so main.rs delegates to it
- main.rs becomes a thin entry point: config → assembly → tauri-module (which provides event loops + command handler)
- SC#2's "only place that imports tauri types" is scoped to the wiring split pair (assembly.rs vs wiring.rs) — see interpretation adjustment in domain section. Other bootstrap/ modules (runtime.rs, run.rs) still import tauri

### Tauri-purity verification (ROADMAP SC#4 — staged, not downgraded)

- **What Phase 37 achieves:** assembly.rs has zero tauri imports, verified by grep + CI lint rule. Its public API surface (return types, parameter types) uses only types from Tauri-free crates (uc-core, uc-app, uc-infra, uc-platform). This is a structural guarantee that assembly.rs IS extractable to a Tauri-free crate
- **What Phase 37 does NOT achieve:** `cargo check` on assembly.rs as an independent compilation unit. This requires either crate extraction or making tauri optional + gating all 10+ non-assembly modules behind `#[cfg(feature)]`. Both are disproportionate — that's Phase 40's job when uc-bootstrap is created
- **Why feature gating is infeasible here:** uc-tauri has `tauri` as unconditional dep (Cargo.toml:20), and lib.rs unconditionally compiles 10 pub modules (commands, events, adapters, preview_panel, quick_panel, tray, services, etc.) that all import tauri. `--no-default-features` would require gating ALL of them — effectively restructuring the entire crate
- **ROADMAP update required:** Phase 37 SC#4 wording should be amended to: "assembly.rs contains zero tauri imports (verified by CI lint) and its public API is Tauri-type-free, preparing it for independent `cargo check` in Phase 40"
- The full `cargo check -p uc-bootstrap --no-default-features` independence is the Phase 40 deliverable

### Commit split strategy (MANDATORY — hex boundary + atomic commit rules)

- Commits MUST respect hex boundaries per AGENTS.md: uc-core changes in separate commits from uc-tauri changes
- Minimum commit structure:
  1. `arch:` New HostEvent sub-enums (PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent) in uc-core — `cargo check -p uc-core` passes
  2. `impl:` TauriEventEmitter + LoggingEventEmitter extended with new variants + contract tests — `cargo check -p uc-tauri` passes
  3. `refactor:` Migrate app.emit() calls + file_transfer_wiring.rs to HostEventEmitterPort, remove AppHandle<R> from start_background_tasks — `cargo test` passes
  4. `refactor:` Split wiring.rs → assembly.rs + wiring.rs, move command registration — `cargo check -p uc-tauri` passes + grep lint confirms assembly.rs has zero tauri imports
  5. `docs:` Update ROADMAP.md Phase 37 SC#2/SC#4 wording to reflect staged interpretation
- Planner may further split these if individual commits are too large, but must NOT merge uc-core and uc-tauri changes into a single commit

### Claude's Discretion

- Exact PairingHostEvent / SetupHostEvent / SpaceAccessHostEvent variant names and field structures
- Internal refactoring of wiring.rs closure patterns to accommodate emitter injection
- Order of migration (which event domain first)
- Whether command registration moves into wiring.rs or a separate helper function

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements and phase definition

- `.planning/REQUIREMENTS.md` — RNTM-02 defines the success criteria for this phase
- `.planning/ROADMAP.md` — Phase 37 success criteria (4 items)

### Phase 36 context (predecessor decisions)

- `.planning/phases/36-event-emitter-abstraction/36-CONTEXT.md` — HostEventEmitterPort design, event model identity, TauriEventEmitter pattern, migration strategy, commit split approach
- Key decisions carried forward: HostEvent is core semantic model (not DTO), TauriEventEmitter owns payload conversion, best-effort emit (warn + continue)

### Primary code targets

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — 6328-line file to be split; contains all 14 remaining app.emit() calls
- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` — 5 functions with AppHandle<R> to migrate
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — Module declarations and re-exports (needs updating)
- `src-tauri/src/main.rs` — Lines 852-927: invoke_handler![...] command registration to be moved into Tauri module
- `src-tauri/crates/uc-tauri/src/lib.rs` — Module declarations; lib.rs:8-17 declares 10 unconditional pub mod; most (commands, bootstrap, events, preview_panel, quick_panel, tray, services) import tauri directly or transitively — context for why crate-wide feature gating is Phase 40 scope

### Existing HostEvent implementation (from Phase 36)

- `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` — HostEvent enum, sub-enums (ClipboardHostEvent, PeerDiscoveryHostEvent, PeerConnectionHostEvent, TransferHostEvent), EmitError
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` — TauriEventEmitter and LoggingEventEmitter with event name mapping and payload DTOs

### Current event types (to be replaced by HostEvent variants)

- `src-tauri/crates/uc-tauri/src/events/mod.rs` — SettingChangedEvent, forward\_\* functions
- `src-tauri/crates/uc-tauri/src/events/p2p_pairing.rs` — P2PPairingVerificationEvent and related types
- `src-tauri/crates/uc-tauri/src/events/p2p_peer.rs` — P2P peer event types (likely already migrated in Phase 36)

### Crate-level rules

- `src-tauri/crates/uc-core/AGENTS.md` — No Tauri/system imports, port conventions
- `src-tauri/crates/uc-tauri/AGENTS.md` — Bootstrap editing rules, event payload conventions, camelCase mandate
- `AGENTS.md` — Atomic commit rules, hex boundary, revert safety

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `HostEventEmitterPort` trait + `HostEvent` enum: Already defined in uc-core/ports from Phase 36 — extend with new domain sub-enums
- `TauriEventEmitter`: Already has pattern for event name mapping + payload DTO conversion — extend with new match arms
- `LoggingEventEmitter`: Already handles all existing HostEvent variants — extend with new variants
- `RecordingEmitter` (test helper in file_transfer_wiring.rs): Reusable test pattern for verifying emitted events
- Frontend contract test pattern from Phase 36: Assert exact event name string + camelCase payload fields

### Established Patterns

- Port injection via `Arc<dyn HostEventEmitterPort>` through closures — used throughout wiring.rs event loops
- `emit_pending_status` in file_transfer_wiring.rs: Already migrated in Phase 36, serves as reference pattern for the remaining 5 functions
- Best-effort emit convention: `if let Err(err) = emitter.emit(...) { warn!(...) }` — consistent across all migrated sites

### Integration Points

- wiring.rs line 1166-1170: `app_handle.clone()` captured in multiple closures — these clones become `event_emitter.clone()` after migration
- wiring.rs `start_background_tasks` signature: `AppHandle<R>` parameter removed after all emits migrated; `R: Runtime` generic parameter also removed
- mod.rs: Needs `pub mod assembly;` declaration and `pub use assembly::*` re-export
- AppRuntime (runtime.rs): May still hold app_handle for commands-layer (out of scope), but event loop code no longer needs it
- main.rs:852-927: `invoke_handler![...]` macro moved into Tauri module; main.rs calls a function that returns the handler
- `resolve_pairing_device_name` callers outside event loops: commands/settings.rs:4,140 and adapters/lifecycle.rs:18,148 — these import from bootstrap::assembly after the move
- Grep/CI lint: assembly.rs must pass `grep -c 'tauri::' assembly.rs` == 0; no `AppHandle`, `Emitter`, `Runtime` imports

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

- Move assembly.rs to independent crate (uc-bootstrap) with real `cargo check -p` independence — Phase 40 (completes ROADMAP SC#4 full intent)
- Make tauri an optional dependency in uc-tauri + feature-gate all Tauri-heavy modules — Phase 40 prerequisite for crate extraction
- Enforce SC#2 "only place" constraint across ALL of uc-tauri (not just bootstrap/) — Phase 40
- Move file_transfer_wiring.rs out of uc-tauri — Phase 38+ when CoreRuntime is extracted
- Migrate commands-layer emits (pairing.rs, clipboard.rs, encryption.rs, tray.rs) — future phase
- Migrate emit_to for quick-panel/preview-panel (window-targeted, requires different abstraction) — future phase
- Split wiring.rs further by domain (clipboard_loop.rs, pairing_loop.rs, etc.) — optional future cleanup

</deferred>

---

_Phase: 37-wiring-decomposition_
_Context gathered: 2026-03-17_
