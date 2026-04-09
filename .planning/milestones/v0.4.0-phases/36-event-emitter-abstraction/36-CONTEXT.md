# Phase 36: Event Emitter Abstraction - Context

**Gathered:** 2026-03-17
**Status:** Ready for planning

<domain>
## Phase Boundary

Define `HostEventEmitterPort` trait and two adapters (Tauri, Logging). Migrate **only** the three EVNT-04 component categories (clipboard watcher, peer discovery, sync scheduler) **plus** `AppRuntime`'s clipboard emit path to use the new port. All other `app.emit()` call sites in wiring.rs (setup-state-changed, space-access, pairing-verification, setting-changed, etc.) remain untouched and are **out of scope** for this phase.

**In-scope emit sites (exhaustive):**

- Clipboard watcher emit in `AppRuntime::on_clipboard_changed` (runtime.rs:1189-1205)
- Peer discovery changed events (wiring.rs:2336, 2364)
- Peer connection changed events (wiring.rs:2379, 2391, 2404)
- Peer name updated events (wiring.rs:2418)
- Sync/inbound clipboard events (wiring.rs:1376, 1860)
- File transfer completed (wiring.rs:2546)
- File transfer status changed (wiring.rs:1774, file_transfer_wiring.rs:85)
- Transfer progress (transfer_progress.rs:42)

**Out-of-scope emit sites (remain as direct app.emit()):**

- setup-state-changed (wiring.rs:204)
- space-access-completed / p2p-space-access-completed (wiring.rs:2115, 2119)
- pairing-verification (wiring.rs:2671, 3110, 3124, 3241, 3249, 3272)
- pairing-events-subscribe-failure/recovered (wiring.rs:3300, 3314)
- inbound-clipboard-subscribe-error/retry (wiring.rs:1418, 1434)
- emit_to for quick-panel/preview-panel (quick_panel/mod.rs, preview_panel/mod.rs)
- Commands-layer emits (encryption.rs, pairing.rs, tray.rs)
- libp2p start-failed (events/mod.rs:48)
- clipboard monitor heartbeat (clipboard_monitor.rs:43)

</domain>

<decisions>
## Implementation Decisions

### Trait design

- Single method: `fn emit(&self, event: HostEvent) -> Result<(), EmitError>`
- Trait name: `HostEventEmitterPort`
- Trait and all event types live in `uc-core/ports/`
- Components receive the port via `Arc<dyn HostEventEmitterPort>` constructor injection (consistent with existing port patterns like `Arc<dyn SettingsPort>`)
- Trait is `Send + Sync` (required for async contexts)

### Event model identity

- **HostEvent is a core semantic model, NOT a frontend protocol DTO**
- HostEvent uses pure Rust types, no serde annotations, no camelCase rename — uc-core stays clean
- TauriEventEmitter is solely responsible for converting HostEvent -> Tauri event name string + serde-annotated payload struct
- This means TauriEventEmitter internally defines its own payload DTOs with `#[serde(rename_all = "camelCase")]` and `#[serde(tag = "type")]` as needed to match current frontend contracts

### Event type system

- Strong-typed `HostEvent` enum with nested sub-enums per domain: `HostEvent::Clipboard(ClipboardHostEvent)`, `HostEvent::PeerDiscovery(PeerDiscoveryHostEvent)`, `HostEvent::Transfer(TransferHostEvent)`, etc.
- **Only events for the in-scope components are defined** in Phase 36. No enum variants for setup, pairing-verification, space-access, etc.
- Event types are newly defined in uc-core — NOT moved from uc-tauri
- Event name mapping (e.g., `ClipboardHostEvent::NewContent` -> `"clipboard://event"`) is the adapter's internal responsibility

### Failure semantics

- **Best-effort: warn + continue.** Emit failure must never interrupt business flow
- Trait returns `Result<(), EmitError>` for observability, but the **mandatory calling convention** is: log the error, then continue
- Non-GUI mode (LoggingEventEmitter) is infallible by design — Result is always Ok
- No buffering, backpressure, or noop mode needed in Phase 36

### AppRuntime restructuring (in-scope)

- `AppRuntime` gets a NEW `event_emitter: RwLock<Arc<dyn HostEventEmitterPort>>` field ALONGSIDE the existing `app_handle: Arc<RwLock<Option<AppHandle>>>` field
- `app_handle` is KEPT — it is still needed by out-of-scope callers (commands/pairing.rs, commands/clipboard.rs, apply_autostart, build_setup_orchestrator, app_lifecycle_coordinator)
- `set_app_handle()` and `app_handle()` methods are KEPT for those out-of-scope callers
- NEW `event_emitter()` accessor and `set_event_emitter()` setter are added
- The emitter starts as LoggingEventEmitter at construction time (before Tauri setup), then is swapped to TauriEventEmitter via `set_event_emitter()` after the AppHandle is available in the setup callback
- AppRuntime::on_clipboard_changed uses the port instead of direct AppHandle read

### Migration strategy

- **Strict EVNT-04 scope** — only the three component categories + AppRuntime clipboard emit
- All other wiring.rs emit calls remain as direct `app.emit()` (deferred to Phase 37+)
- Existing `uc-tauri/events/` types that are referenced ONLY by in-scope sites are deleted; types still used by out-of-scope sites are preserved

### Commit split strategy (MANDATORY — satisfies AGENTS.md hex boundary + atomic commit rules)

1. `arch:` HostEventEmitterPort trait + HostEvent enums + EmitError (uc-core only) — `cargo check -p uc-core` passes
2. `impl:` TauriEventEmitter adapter + event contract tests (uc-tauri only) — `cargo check -p uc-tauri` passes
3. `impl:` LoggingEventEmitter adapter (uc-tauri or uc-infra) — `cargo check` passes
4. `refactor:` Wire emitter port into in-scope components + AppRuntime restructuring + delete obsolete code — `cargo test` passes

### Frontend compatibility verification

- **Event contract tests are mandatory** for every migrated event
- Each test asserts: exact event name string, JSON key naming (camelCase), required fields, tag values
- Tests live in TauriEventEmitter module (commit 2)
- Reference pattern: existing `test_setting_changed_event_camelcase_serialization` in events/mod.rs

### LoggingEventEmitter behavior

- Logs all events — filtering controlled by tracing level configuration
- Log levels vary by event type: error events -> `warn!`, key business events (clipboard new, transfer complete) -> `info!`, discovery changes -> `debug!`
- Output uses structured tracing fields: `info!(event_type = "clipboard.new_content", entry_id = %id)` — consistent with existing tracing patterns
- Sensitive field policy: follows AGENTS.md existing rules (no raw keys, passphrases, decrypted content). Current in-scope events only carry IDs/summaries, not raw clipboard content

### Claude's Discretion

- Exact HostEvent sub-enum variant names and field structures
- EmitError type design (simple string vs structured)
- Internal implementation of TauriEventEmitter's event name mapping (match arms, const table, etc.)
- Whether LoggingEventEmitter lives in uc-tauri/adapters or uc-infra
- Specific tracing level assignment per event variant in LoggingEventEmitter

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements and constraints

- `.planning/REQUIREMENTS.md` — EVNT-01 through EVNT-04 define the four success criteria for this phase
- `AGENTS.md` Atomic Commit Rule — Port + adapter split, hex boundary commit rules, revert safety
- `AGENTS.md` Tauri Event Payload Serialization — camelCase mandate, known incident reference
- `AGENTS.md` Rust Logging — Sensitive field policy, structured tracing conventions

### Current event implementation (to be partially replaced)

- `src-tauri/crates/uc-tauri/src/events/mod.rs` — ClipboardEvent, EncryptionEvent, SettingChangedEvent, forward\_\* functions
- `src-tauri/crates/uc-tauri/src/events/transfer_progress.rs` — TransferProgressEvent and forward function
- `src-tauri/crates/uc-tauri/src/events/p2p_pairing.rs` — P2P pairing event types
- `src-tauri/crates/uc-tauri/src/events/p2p_peer.rs` — P2P peer discovery/connection event types

### Primary emit call sites (in-scope)

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — Lines 1376, 1774, 1860, 2336, 2364, 2379, 2391, 2404, 2418, 2546
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — Lines 1189-1205 (AppRuntime clipboard emit), lines 99-123 (AppRuntime struct with app_handle field)
- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` — Line 85

### Port patterns (reference for trait design)

- `src-tauri/crates/uc-core/src/ports/mod.rs` — Existing port registry, Arc<dyn Port> injection pattern
- `src-tauri/crates/uc-core/src/ports/setup_event_port.rs` — Similar event port pattern
- `src-tauri/crates/uc-core/src/ports/transfer_progress.rs` — TransferProgressPort as event delivery precedent

### Logging patterns

- `src-tauri/crates/uc-observability/` — Tracing configuration, structured logging patterns

### Crate-level rules

- `src-tauri/crates/uc-core/AGENTS.md` — No Tauri/system imports, port conventions
- `src-tauri/crates/uc-tauri/AGENTS.md` — Bootstrap editing rules, event payload conventions

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `SetupEventPort` in uc-core/ports: Existing event port pattern — structural reference for HostEventEmitterPort
- `TransferProgressPort`: Event delivery port with structured data — validates Arc<dyn Port> injection pattern
- `uc-observability` crate: Dual-output tracing already configured — LoggingEventEmitter uses tracing macros directly

### Established Patterns

- Port injection via `Arc<dyn XxxPort>` through constructors — all infrastructure ports follow this
- Event payload DTOs in uc-tauri use `#[serde(tag = "type")]` for discriminated unions and `#[serde(rename_all = "camelCase")]` for field names
- Error types in ports use simple string-based errors or dedicated error enums
- `AppRuntime::app_handle` currently stored as `Arc<RwLock<Option<AppHandle>>>` set post-init via `set_app_handle()` — this field and its accessors are KEPT alongside the new event_emitter field

### Integration Points

- wiring.rs: `app.clone()` captured in closures -> in-scope emit sites replaced with `Arc<dyn HostEventEmitterPort>`; out-of-scope closures continue to use `app.clone()` as before
- `AppRuntime` struct: `event_emitter` field ADDED alongside existing `app_handle`; both coexist
- `AppDeps` domain sub-structs may need to carry the emitter port for components that need it
- Out-of-scope emit calls in wiring.rs continue to use `app.clone()` as before

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

- Migrate remaining wiring.rs emit calls (setup, pairing-verification, space-access, setting-changed) to HostEventEmitterPort — Phase 37 (wiring decomposition) is the natural home
- Define HostEvent variants for out-of-scope event domains — add when those components are migrated
- Event buffering/backpressure for daemon mode — future daemon milestone if needed

</deferred>

---

_Phase: 36-event-emitter-abstraction_
_Context gathered: 2026-03-17_
