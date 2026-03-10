# Phase 20: Clipboard Capture Flow Correlation - Context

**Gathered:** 2026-03-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Instrument the clipboard capture pipeline so every step — from detection through persistence — is correlated by a single `flow_id` and tagged with `stage` fields in structured logs. Developers can filter logs by one `flow_id` and see the entire local capture flow. This phase covers requirements FLOW-01 through FLOW-04. Outbound sync/publish flow correlation (FLOW-05) belongs to Phase 21.

</domain>

<decisions>
## Implementation Decisions

### flow_id Generation Strategy

- flow_id generated at the App layer (AppRuntime::on_clipboard_changed) — the business logic entry point
- Format: UUID v7 (time-sortable), using the existing `uuid` crate dependency
- Displayed as full UUID string in logs (e.g., `flow_id=019526a7-3b4c-7def-8123-456789abcdef`)
- flow_id injected as a span field on the root capture span; downstream UseCase and infra layers inherit it via tracing span context — no explicit parameter passing within the local capture path

### Stage Naming and Granularity

- Stage names follow the actual code structure, not strictly the requirements document's 7-stage list
- Each major capture step gets one span with a `stage` field — no sub-spans within stages
- Span naming style: flat names (e.g., `info_span!("normalize", stage = "normalize")`)
- Scope limited to local capture: detect → normalize → persist_event → cache_representations → select_policy → persist_entry. The "publish" stage (outbound sync) is deferred to Phase 21
- Requirements mapping: FLOW-03 stages will reflect the actual pipeline steps rather than the exact names in the requirements text

### Cross-spawn Context Propagation

- For spawned async tasks (outbound sync uses `tokio::spawn` + `spawn_blocking`): flow_id is explicitly passed as a plain Uuid value into the spawn closure, which creates its own span with the flow_id attached
- For the local capture path (AppRuntime → CaptureClipboard UseCase → infra repos): relies on tracing span inheritance — no explicit flow_id passing needed since everything runs in the same async task chain
- `try_join_all()` in the normalizer does not need special handling — it runs futures concurrently within the same task, so span context is naturally preserved

### Observability Tooling Placement

- Each layer directly uses the tracing API (`info_span!`, `info!`, etc.) to create stage spans — no custom span builder abstraction
- flow_id generation is inline in AppRuntime (one-liner: `Uuid::now_v7()`)
- uc-observability crate gets additions:
  - `FlowId` newtype wrapping Uuid with `Display`, `Debug` implementations and `FlowId::generate()` factory
  - Stage name constants: `pub const STAGE_DETECT: &str = "detect"`, etc. — prevents typos and ensures consistency across layers
- uc-observability does NOT need changes to its subscriber/format infrastructure — the existing JSON flattening already handles span fields

### Claude's Discretion

- Exact list of stage constants (based on code audit during implementation)
- Whether to add flow_id to existing `usecase.capture_clipboard.execute` span or replace it with a new root span
- Internal module organization for FlowId and stage constants within uc-observability
- Test strategy for verifying flow_id propagation across spans
- Whether detect stage span wraps the watcher callback or just the AppRuntime entry point

</decisions>

<specifics>
## Specific Ideas

- The existing `#[instrument(name = "runtime.on_clipboard_changed")]` on AppRuntime is the natural injection point for flow_id — can add `flow_id = %flow_id` to the instrument fields
- The existing `usecase.capture_clipboard.execute` span already has `source`, `origin`, `representations` fields — stage sub-spans go inside this span
- Phase 19's JSON format already flattens parent span fields to top level, so flow_id on a parent span automatically appears on every child event

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `AppRuntime::on_clipboard_changed()` (runtime.rs:1001): Already has `#[instrument]` — flow_id can be added as a span field
- `CaptureClipboard::execute_with_origin()` (capture_clipboard.rs:133): Has wrapping `info_span!` — sub-stage spans go inside this
- `uuid` crate: Already in workspace dependencies, supports v7 generation
- uc-observability `FlatJsonFormat`: Already flattens span fields to JSON top level — flow_id will automatically appear on all events

### Established Patterns

- `info_span!("name", field = %value)` + `.instrument(span).await` for async operations
- `#[tracing::instrument]` macro for function-level instrumentation
- `tracing::Span::current()` capture + `.instrument(parent_span)` for spawn context propagation (already used in runtime.rs:1065)

### Integration Points

- `runtime.rs` on_clipboard_changed(): Root flow_id generation and span injection
- `capture_clipboard.rs` execute_with_origin(): Stage sub-spans for normalize, persist, cache, select, persist_entry
- `uc-observability/src/lib.rs`: New public exports for FlowId and stage constants
- `spawn` block in runtime.rs:1066-1085: Explicit flow_id passing into outbound sync closure

</code_context>

<deferred>
## Deferred Ideas

- Outbound sync (publish) flow correlation — Phase 21
- Inbound sync flow correlation — Phase 21
- Representation-level sub-spans with representation_id, mime_type, size_bytes (OBS-02) — future milestone
- FlowContext struct wrapping flow_id + metadata — not needed for current scope, reconsider in Phase 21 if sync needs more context fields

</deferred>

---

_Phase: 20-clipboard-capture-flow-correlation_
_Context gathered: 2026-03-11_
