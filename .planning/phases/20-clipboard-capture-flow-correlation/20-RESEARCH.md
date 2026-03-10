# Phase 20: Clipboard Capture Flow Correlation - Research

**Researched:** 2026-03-11
**Domain:** Rust tracing span instrumentation, flow correlation, structured logging
**Confidence:** HIGH

## Summary

Phase 20 adds a `flow_id` (UUID v7) to every clipboard capture flow so developers can filter logs by a single identifier and see the full pipeline. The work is primarily instrumentation: generating a UUID at the AppRuntime entry point, attaching it as a span field, and adding `stage`-labeled sub-spans at each pipeline step.

The existing infrastructure is well-suited. Phase 19's `FlatJsonFormat` already flattens parent span fields to the top-level JSON object, so a `flow_id` on a root span automatically appears on every child event. The `tracing` crate's span inheritance handles the local capture path (no explicit parameter passing). The spawned outbound sync task already captures `parent_span` for instrumentation.

**Primary recommendation:** Add FlowId newtype + stage constants to uc-observability, generate flow_id in `AppRuntime::on_clipboard_changed`, instrument each pipeline step with `info_span!("step_name", stage = STAGE_X)` inside `CaptureClipboardUseCase::execute_with_origin`.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- flow_id generated at the App layer (AppRuntime::on_clipboard_changed) -- the business logic entry point
- Format: UUID v7 (time-sortable), using the existing `uuid` crate dependency
- Displayed as full UUID string in logs (e.g., `flow_id=019526a7-3b4c-7def-8123-456789abcdef`)
- flow_id injected as a span field on the root capture span; downstream UseCase and infra layers inherit it via tracing span context -- no explicit parameter passing within the local capture path
- Stage names follow the actual code structure, not strictly the requirements document's 7-stage list
- Each major capture step gets one span with a `stage` field -- no sub-spans within stages
- Span naming style: flat names (e.g., `info_span!("normalize", stage = "normalize")`)
- Scope limited to local capture: detect -> normalize -> persist_event -> cache_representations -> select_policy -> persist_entry. The "publish" stage (outbound sync) is deferred to Phase 21
- For spawned async tasks: flow_id is explicitly passed as a plain Uuid value into the spawn closure, which creates its own span with the flow_id attached
- For the local capture path: relies on tracing span inheritance -- no explicit flow_id passing
- try_join_all() in the normalizer does not need special handling
- Each layer directly uses the tracing API -- no custom span builder abstraction
- flow_id generation is inline in AppRuntime (one-liner: Uuid::now_v7())
- uc-observability crate gets FlowId newtype and stage name constants
- uc-observability does NOT need changes to its subscriber/format infrastructure

### Claude's Discretion

- Exact list of stage constants (based on code audit during implementation)
- Whether to add flow_id to existing `usecase.capture_clipboard.execute` span or replace it with a new root span
- Internal module organization for FlowId and stage constants within uc-observability
- Test strategy for verifying flow_id propagation across spans
- Whether detect stage span wraps the watcher callback or just the AppRuntime entry point

### Deferred Ideas (OUT OF SCOPE)

- Outbound sync (publish) flow correlation -- Phase 21
- Inbound sync flow correlation -- Phase 21
- Representation-level sub-spans with representation_id, mime_type, size_bytes (OBS-02) -- future milestone
- FlowContext struct wrapping flow_id + metadata -- not needed for current scope
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                                                             | Research Support                                                                                                                      |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| FLOW-01 | Each clipboard capture flow is assigned a unique `flow_id` at the platform entry point and this `flow_id` is attached to the root span. | FlowId newtype in uc-observability, UUID v7 generation in AppRuntime::on_clipboard_changed, added as field to `#[instrument]` span    |
| FLOW-02 | All spans and events participating in a clipboard capture flow carry the same `flow_id` field.                                          | FlatJsonFormat already flattens parent span fields; flow_id on root span auto-propagates to all child events via tracing span context |
| FLOW-03 | Each major step of the capture pipeline is represented by a named span with a `stage` field.                                            | Stage constants in uc-observability, `info_span!` with `stage` field wrapping each step in CaptureClipboardUseCase                    |
| FLOW-04 | Cross-layer operations preserve `flow_id` and `stage` context, including across `tokio::spawn` boundaries.                              | Explicit flow_id passing into spawn closure (already has `parent_span` pattern at runtime.rs:1064-1085)                               |

</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose                        | Why Standard                     |
| ------- | ------- | ------------------------------ | -------------------------------- |
| tracing | 0.1     | Structured logging with spans  | Already in use across all crates |
| uuid    | 1.19.0  | UUID v7 generation for flow_id | Already a workspace dependency   |

### Supporting

| Library          | Version       | Purpose                          | When to Use                         |
| ---------------- | ------------- | -------------------------------- | ----------------------------------- |
| uc-observability | 0.1.0 (local) | FlowId newtype + stage constants | New public API for flow correlation |

### Dependency Changes Required

**uuid feature flag addition:** The `uuid` crate in uc-observability (or whichever crate hosts FlowId) needs the `v7` feature. Currently:

- uc-core: `features = ["v4", "fast-rng", "serde"]`
- uc-app: `features = ["v4"]`
- uc-tauri: does not depend on uuid directly

UUID v7 requires: `features = ["v7"]`. This must be added to the crate that generates the flow_id. Since generation happens in AppRuntime (uc-tauri crate) but FlowId lives in uc-observability, the `uuid` dependency with `v7` feature needs to be added to uc-observability's Cargo.toml.

**Confidence:** HIGH -- verified uuid 1.19.0 is resolved in the workspace, and v7 feature is available since uuid 1.3.0.

## Architecture Patterns

### Injection Point Map

```
PlatformRuntime::handle_event(ClipboardChanged)     [uc-platform]
  -> handler.on_clipboard_changed(snapshot)
    -> AppRuntime::on_clipboard_changed(snapshot)    [uc-tauri]  <-- FLOW_ID GENERATED HERE
      -> #[instrument] span with flow_id field
      -> CaptureClipboardUseCase::execute_with_origin()  [uc-app]
        -> info_span!("usecase.capture_clipboard.execute", ...)
          -> info_span!("normalize", stage = "normalize")
          -> info_span!("persist_event", stage = "persist_event")
          -> info_span!("cache_representations", stage = "cache_representations")
          -> info_span!("select_policy", stage = "select_policy")
          -> info_span!("persist_entry", stage = "persist_entry")
      -> tokio::spawn (outbound sync)                [DEFERRED to Phase 21]
```

### Pattern 1: Root Span with flow_id

The existing `#[tracing::instrument]` on `on_clipboard_changed` creates the root span. flow_id must be added as a field:

```rust
// Option A: Replace #[instrument] with manual span (recommended)
async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
    let flow_id = FlowId::generate();
    let span = info_span!(
        "runtime.on_clipboard_changed",
        %flow_id,
        stage = stages::DETECT,
    );
    async move {
        // ... existing body ...
    }.instrument(span).await
}
```

Rationale: `#[instrument]` does not support computed field values (flow_id must be generated at runtime). A manual span is needed.

### Pattern 2: Stage Sub-Spans in UseCase

Each step inside `execute_with_origin` gets a stage span:

```rust
// Normalize step
let normalized_reps = async {
    let normalized_futures: Vec<_> = snapshot
        .representations
        .iter()
        .map(|rep| self.representation_normalizer.normalize(rep))
        .collect();
    try_join_all(normalized_futures).await
}
.instrument(info_span!("normalize", stage = uc_observability::stages::NORMALIZE))
.await?;
```

### Pattern 3: Cross-Spawn flow_id Propagation

Already established in runtime.rs:1064-1085. For Phase 21, the outbound sync spawn will create its own span with flow_id:

```rust
let flow_id = flow_id.clone(); // plain Uuid value
tauri::async_runtime::spawn(
    async move {
        // Phase 21 will add: info_span!("publish", %flow_id, stage = "publish")
        // ... sync logic ...
    }
    .instrument(info_span!("outbound_sync", %flow_id)),
);
```

### Anti-Patterns to Avoid

- **Passing flow_id as function parameter through the capture path:** Tracing span inheritance handles this automatically. Only pass explicitly across spawn boundaries.
- **Creating sub-spans within stages:** CONTEXT.md locks "one span per stage, no sub-spans."
- **Using `#[instrument]` for spans needing runtime-computed fields:** `#[instrument]` fields are limited to function parameters. Use manual `info_span!` when flow_id is generated inside the function.

## Don't Hand-Roll

| Problem                | Don't Build               | Use Instead                                 | Why                                             |
| ---------------------- | ------------------------- | ------------------------------------------- | ----------------------------------------------- |
| UUID v7 generation     | Custom timestamp-based ID | `uuid::Uuid::now_v7()`                      | Standard, time-sortable, 122 bits of uniqueness |
| Span field propagation | Manual field threading    | tracing span parent-child inheritance       | FlatJsonFormat already flattens parent fields   |
| Cross-spawn context    | Custom context struct     | Explicit Uuid value + `info_span!` in spawn | Simple, no framework needed                     |

## Common Pitfalls

### Pitfall 1: #[instrument] Cannot Use Runtime-Generated Values

**What goes wrong:** Trying to add `flow_id` to `#[instrument(fields(flow_id = ...))]` when flow_id is computed inside the function body.
**Why it happens:** `#[instrument]` evaluates field expressions at span creation time, before the function body runs.
**How to avoid:** Replace `#[instrument]` with a manual `info_span!` + `.instrument(span).await` pattern. Generate flow_id first, then create span with it.
**Warning signs:** Compile errors about `flow_id not found in this scope` in instrument attribute.

### Pitfall 2: UUID v7 Feature Flag Missing

**What goes wrong:** `Uuid::now_v7()` does not compile.
**Why it happens:** The `v7` feature is not enabled in Cargo.toml. Currently only `v4` is enabled.
**How to avoid:** Add `features = ["v7"]` to the uuid dependency in the crate that calls `Uuid::now_v7()`.
**Warning signs:** Compile error `no method named 'now_v7' found`.

### Pitfall 3: Span Not Entered for Synchronous Code

**What goes wrong:** Stage spans created with `info_span!` but not entered (`.enter()` or `.instrument()`) -- events inside don't carry the span context.
**Why it happens:** Creating a span does not activate it. It must be entered.
**How to avoid:** Always use `.instrument(span).await` for async blocks, or `let _guard = span.enter()` for sync blocks.
**Warning signs:** flow_id appears on root events but not on events inside a stage.

### Pitfall 4: Sync select() Call Needs Special Span Handling

**What goes wrong:** `representation_policy.select(&snapshot)` is synchronous (not async), so `.instrument()` doesn't apply.
**Why it happens:** `.instrument()` is for futures. Sync calls need `span.enter()`.
**How to avoid:** Use `let _guard = info_span!("select_policy", stage = ...).entered();` for the sync call.

### Pitfall 5: try_join_all Span Context

**What goes wrong:** Concern that concurrent futures in `try_join_all` lose span context.
**Why it happens:** Misunderstanding of tokio's task model.
**How to avoid:** CONTEXT.md confirms: `try_join_all()` runs futures concurrently within the same task, so span context is preserved. No special handling needed.

## Code Examples

### FlowId Newtype in uc-observability

```rust
// uc-observability/src/flow.rs
use std::fmt;
use uuid::Uuid;

/// A unique identifier for a clipboard capture or sync flow.
///
/// Wraps UUID v7 for time-sortable, globally unique flow correlation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowId(Uuid);

impl FlowId {
    /// Generate a new time-sortable flow ID (UUID v7).
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for FlowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
```

### Stage Constants in uc-observability

```rust
// uc-observability/src/stages.rs
/// Stage name constants for clipboard capture flow correlation.
///
/// Used as the `stage` field value in tracing spans to ensure consistency.

pub const DETECT: &str = "detect";
pub const NORMALIZE: &str = "normalize";
pub const PERSIST_EVENT: &str = "persist_event";
pub const CACHE_REPRESENTATIONS: &str = "cache_representations";
pub const SELECT_POLICY: &str = "select_policy";
pub const PERSIST_ENTRY: &str = "persist_entry";
```

### Modified on_clipboard_changed (Root Span)

```rust
// Replace #[instrument] with manual span
async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
    let flow_id = uc_observability::FlowId::generate();
    let span = tracing::info_span!(
        "runtime.on_clipboard_changed",
        %flow_id,
        stage = uc_observability::stages::DETECT,
    );
    async move {
        // ... existing body unchanged ...
    }
    .instrument(span)
    .await
}
```

### Stage Spans in execute_with_origin

```rust
// Inside execute_with_origin async block:

// Normalize stage
let normalized_reps = async {
    let normalized_futures: Vec<_> = snapshot
        .representations
        .iter()
        .map(|rep| self.representation_normalizer.normalize(rep))
        .collect();
    try_join_all(normalized_futures).await
}
.instrument(info_span!("normalize", stage = uc_observability::stages::NORMALIZE))
.await?;

// Persist event stage
async {
    self.event_writer
        .insert_event(&new_event, &normalized_reps)
        .await
}
.instrument(info_span!("persist_event", stage = uc_observability::stages::PERSIST_EVENT))
.await?;

// Select policy stage (sync call -- use entered())
let (entry_id, new_selection) = {
    let _guard = info_span!("select_policy", stage = uc_observability::stages::SELECT_POLICY).entered();
    let entry_id = EntryId::new();
    let selection = self.representation_policy.select(&snapshot)?;
    let new_selection = ClipboardSelectionDecision::new(entry_id.clone(), selection);
    (entry_id, new_selection)
};
```

## State of the Art

| Old Approach         | Current Approach                 | When Changed   | Impact                                        |
| -------------------- | -------------------------------- | -------------- | --------------------------------------------- |
| No flow correlation  | flow_id + stage spans            | Phase 20 (now) | Enables filtering logs by single capture flow |
| `#[instrument]` only | Manual spans for computed fields | Phase 20       | Supports runtime-generated flow_id            |

## Open Questions

1. **Detect stage boundary**
   - What we know: CONTEXT.md says flow_id is generated at AppRuntime::on_clipboard_changed. The "detect" stage represents entry into the capture pipeline.
   - What's unclear: Whether the detect stage span should be the root span itself (on_clipboard_changed) or a separate span inside it.
   - Recommendation: Make the root span double as the detect stage (add `stage = DETECT` to the root span). This avoids an extra span layer and matches the CONTEXT.md "detect -> normalize -> ..." flow.

2. **uc-observability dependency on uuid**
   - What we know: uc-observability currently has no uuid dependency. FlowId needs uuid with v7 feature.
   - What's unclear: Whether to add uuid to uc-observability or put FlowId elsewhere.
   - Recommendation: Add `uuid = { version = "1", features = ["v7"] }` to uc-observability. This keeps flow correlation concerns in the observability crate.

3. **uc-app dependency on uc-observability**
   - What we know: uc-app currently does not depend on uc-observability. Stage spans in CaptureClipboardUseCase need stage constants.
   - What's unclear: Whether to add the dependency or inline constants.
   - Recommendation: Add `uc-observability` as a dependency of `uc-app` for stage constants. This is a lightweight, observability-only dependency. Alternative: put constants in uc-core, but that pollutes the domain layer.

## Sources

### Primary (HIGH confidence)

- **Source code audit**: `uc-tauri/src/bootstrap/runtime.rs` lines 1000-1099 (AppRuntime::on_clipboard_changed)
- **Source code audit**: `uc-app/src/usecases/internal/capture_clipboard.rs` (full CaptureClipboardUseCase)
- **Source code audit**: `uc-observability/src/format.rs` (FlatJsonFormat field flattening behavior)
- **Source code audit**: `uc-platform/src/runtime/runtime.rs` (PlatformRuntime event handling)

### Secondary (MEDIUM confidence)

- **uuid crate**: v1.19.0 resolved in workspace; `v7` feature available since uuid 1.3.0 (verified via `cargo tree`)
- **tracing span inheritance**: Well-documented behavior -- parent span fields visible to child spans via subscriber extensions

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - all libraries already in use, only feature flag addition needed
- Architecture: HIGH - injection points clearly identified in source code, patterns match existing codebase conventions
- Pitfalls: HIGH - identified from direct code analysis of actual span usage patterns

**Research date:** 2026-03-11
**Valid until:** 2026-04-11 (stable domain, no external dependency changes expected)
