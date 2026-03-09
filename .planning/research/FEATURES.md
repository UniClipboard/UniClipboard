# Feature Landscape

**Domain:** Log observability for Tauri 2 desktop app (Rust + React clipboard sync)
**Researched:** 2026-03-09
**Milestone:** v0.3.0 Log Observability

## Table Stakes

Features developers debugging the app expect. Missing = observability feels incomplete.

| Feature                                         | Why Expected                                                                                                                                                                                                                                                       | Complexity | Dependencies                                                                                                      | Notes                                                                                                                                                                                                                                                                                           |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **flow_id on clipboard capture pipeline**       | Without a correlation ID, interleaved async ops are impossible to trace end-to-end. The capture pipeline (watcher -> normalize -> persist -> publish) spans multiple use cases and layers.                                                                         | Med        | Existing `info_span!` in `CaptureClipboardUseCase`, `SyncOutboundClipboardUseCase`, `SyncInboundClipboardUseCase` | Generate `Uuid::new_v4()` at pipeline entry (platform callback), propagate as span field. tracing's parent-child hierarchy carries fields to children automatically. `TraceMetadata` already exists in `uc-platform/src/ports/observability.rs` with a `trace_id: Uuid` -- extend this concept. |
| **stage field on business spans**               | Knowing "which step failed" requires a `stage` field (e.g., `normalize`, `persist_event`, `select_representation`, `persist_entry`, `spool_blob`). Currently spans exist but stages are implicit in span names only.                                               | Low        | Existing span hierarchy in `capture_clipboard.rs`                                                                 | Add `stage = "X"` field to each sub-span within the capture flow. Simple field additions to existing `info_span!` calls.                                                                                                                                                                        |
| **Dual output: pretty console + JSON file**     | Dev needs human-readable terminal output; production/debugging needs machine-parseable JSON for Seq/grep. Current system outputs the same human-readable `fmt` format to both stdout and file.                                                                     | Med        | `tracing-subscriber` already in `uc-tauri/Cargo.toml` with `fmt` feature. Need to add `json` feature flag.        | Compose two `fmt::Layer`s on the shared `Registry`: one with pretty/compact format to stdout, one `.json()` to file writer. Both already use `tracing_appender::non_blocking`.                                                                                                                  |
| **Structured JSON output with span context**    | JSON logs must include span fields (flow_id, stage, entry_id) not just the event message. Default `.json()` formatter includes current span and parent spans.                                                                                                      | Low        | Dual output feature                                                                                               | Enable `.with_current_span(true).with_span_list(true)` on the JSON layer. These are defaults in `tracing-subscriber`'s JSON formatter.                                                                                                                                                          |
| **Log profiles (dev / prod / debug_clipboard)** | Different scenarios need different filter+format combos. `dev` = pretty+debug, `prod` = json+info, `debug_clipboard` = json+trace for clipboard modules only.                                                                                                      | Med        | Existing `build_filter_directives()` in `tracing.rs`                                                              | Replace current boolean `is_dev` with a `LogProfile` enum. Each profile defines filter directives and output layers. Config via env var `UC_LOG_PROFILE` or settings TOML.                                                                                                                      |
| **Business span hierarchy for capture flow**    | The capture flow needs explicit span nesting: `capture_clipboard` -> `normalize_representations` -> `persist_event` -> `apply_selection_policy` -> `persist_entry` -> `spool_blobs`. Currently only the top-level `usecase.capture_clipboard.execute` span exists. | Med        | Existing `CaptureClipboardUseCase::execute_with_origin`                                                           | Add child `info_span!` around each logical step inside the `async move` block. Each span gets `stage` field and inherits `flow_id` from parent.                                                                                                                                                 |
| **Seq local ingestion**                         | Seq is the target visualization tool per PROJECT.md. Must send structured events to a local Seq instance for search, filtering, and flow visualization.                                                                                                            | Med-High   | Seq running in Docker locally; no first-party Rust->Seq crate exists                                              | Two viable approaches: (1) Custom `tracing::Layer` that batches CLEF JSON and POSTs to Seq `/api/events/raw?clef`, or (2) pipe JSON file output through `seqcli ingest`. Approach 1 preferred for real-time ingestion.                                                                          |

## Differentiators

Features that set the observability apart. Not expected, but enable significantly better debugging.

| Feature                                    | Value Proposition                                                                                                                                                                                       | Complexity | Dependencies                                             | Notes                                                                                                                                                                                                         |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Cross-layer context propagation**        | flow_id visible from platform watcher through app layer to infra persist. Currently spans exist per-layer but no explicit business correlation field bridges them.                                      | Med        | flow_id generation at platform callback entry point      | Platform layer generates `flow_id` in `PlatformEvent::ClipboardChanged`, passes through `ClipboardChangeHandler` callback into `CaptureClipboardUseCase`. tracing span field inheritance handles propagation. |
| **Representation-level tracing**           | Each representation (text/plain, image/png, text/html) gets its own span with `representation_id`, `mime_type`, `size_bytes`. Enables per-format debugging ("why did the PNG normalize take 800ms?").   | Med        | Business span hierarchy                                  | Wrap each `normalize()` future and each `spool_queue.enqueue()` in individual spans with representation metadata.                                                                                             |
| **Seq trace/waterfall visualization**      | Seq 2024.1+ renders parent-child span hierarchies as waterfall diagrams when `@tr` (trace_id) and `@ps` (parent_span_id) CLEF properties are present. Turns Seq from "log search" into "flow debugger". | High       | Seq CLEF layer + span lifecycle tracking in custom layer | Custom CLEF layer must track `on_new_span` / `on_close` to emit `@st` (span start), `@t` (span end), `@ps` (parent), `@tr` (trace/flow_id).                                                                   |
| **Configurable Seq endpoint**              | Allow pointing at different Seq instances (local dev, team shared, CI) via settings TOML or env var.                                                                                                    | Low        | Seq CLEF layer                                           | `SEQ_URL` env var with fallback to settings. Default `http://localhost:5341`.                                                                                                                                 |
| **Log profile hot-switch**                 | Change log profile at runtime without restart (switch from `prod` to `debug_clipboard` when investigating a bug).                                                                                       | High       | Log profiles + `tracing_subscriber::reload::Layer`       | Use `reload::Layer` to swap filter directives at runtime. Expose via Tauri command for frontend toggle.                                                                                                       |
| **Sync flow tracing (inbound + outbound)** | Same flow_id/stage pattern for sync_inbound (receive -> decrypt -> apply -> capture) and sync_outbound (read -> encrypt -> broadcast).                                                                  | Med        | flow_id infrastructure                                   | Both `SyncInboundClipboardUseCase` and `SyncOutboundClipboardUseCase` already use `info_span!`. Add flow_id and stage fields following the same pattern as capture.                                           |

## Anti-Features

Features to explicitly NOT build in this milestone.

| Anti-Feature                           | Why Avoid                                                                                                                                                                                                                 | What to Do Instead                                                                                                                                              |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Full OpenTelemetry integration**     | OTel adds `tracing-opentelemetry`, `opentelemetry-otlp`, `opentelemetry-sdk` with significant dependency weight (~15+ transitive crates) and configuration complexity. Premature for a desktop app's local observability. | Use custom CLEF layer for Seq. Leave OTel as v0.4.0 "collector & multi-backend" goal per PROJECT.md. Design the CLEF layer so it can be replaced by OTel later. |
| **Remote/cloud log shipping**          | Desktop app logs should stay local. Clipboard content in logs is privacy-sensitive. No Datadog/Honeycomb/cloud backends.                                                                                                  | Seq runs locally in Docker. Logs stay on developer's machine.                                                                                                   |
| **Frontend (React) log integration**   | Bridging browser console logs into the Rust tracing pipeline adds cross-boundary complexity for minimal gain. Frontend state is visible in React DevTools + Redux DevTools.                                               | Keep frontend logging separate. Backend is where pipeline observability matters.                                                                                |
| **Log rotation / retention policies**  | Over-engineering for a dev-focused observability milestone. Current `tracing_appender::rolling::never` is adequate for development.                                                                                       | Defer to a later milestone. Can switch to `rolling::daily` when needed (one-line change).                                                                       |
| **Custom log viewer UI in the app**    | Building a log viewer inside the Tauri app is high effort, low value when Seq provides a superior web UI with search, filtering, and dashboards.                                                                          | Use Seq's web UI at `localhost:5341`.                                                                                                                           |
| **Metrics / counters**                 | Prometheus-style metrics (clipboard captures/sec, sync latency percentiles) are a different concern from structured logging.                                                                                              | Defer to OTel metrics in v0.4.0 per PROJECT.md next milestone goals.                                                                                            |
| **`log` crate bridge removal**         | Phase 4 of the tracing migration is explicitly marked "not required". Removing `log` compatibility risks breaking `tauri-plugin-log` and any remaining legacy code paths.                                                 | Keep dual-track. Focus on `tracing` for all new instrumentation.                                                                                                |
| **Distributed tracing across devices** | Propagating trace context over libp2p network protocol adds protocol-level changes for a niche debugging scenario.                                                                                                        | Trace within a single device's pipeline. Cross-device correlation can use device_id + timestamp matching in Seq queries.                                        |

## Feature Dependencies

```
flow_id generation (platform layer)
  -> flow_id propagation to CaptureClipboardUseCase
    -> Business span hierarchy (stage fields per step)
      -> Representation-level tracing (optional depth)
      -> Sync flow tracing (reuses same pattern)

Dual output (pretty console + JSON file)
  -> Log profiles (dev/prod/debug_clipboard)
    -> Log profile hot-switch (optional, uses reload::Layer)

JSON output with span context
  -> CLEF formatting layer (custom tracing::Layer impl)
    -> Seq HTTP ingestion (POST to /api/events/raw?clef)
      -> Seq trace visualization (@tr, @ps, @st properties)
      -> Configurable Seq endpoint (env var / settings)
```

## MVP Recommendation

Prioritize in this order:

1. **flow_id + stage fields on capture pipeline** -- Core business value. Without correlation IDs, the rest of the observability stack is prettier noise. Generate `Uuid::new_v4()` at platform callback, attach as span field `flow_id`. Add `stage` field to sub-spans. Builds on existing `TraceMetadata` in `uc-platform/src/ports/observability.rs`.

2. **Dual output (pretty console + JSON file)** -- Foundation for everything downstream. JSON file is what Seq ingests. Pretty console is what developers read during `bun tauri dev`. This is primarily a configuration change to existing `init_tracing_subscriber()` in `uc-tauri/src/bootstrap/tracing.rs`. Add `json` feature to `tracing-subscriber` dependency.

3. **Log profiles** -- Small increment on top of dual output. Three profiles cover all use cases: `dev` (pretty stdout, debug level), `prod` (JSON file, info level), `debug_clipboard` (JSON file + stdout, trace level for `uc_app::usecases::clipboard` + `uc_platform::clipboard` modules). Controlled by `UC_LOG_PROFILE` env var.

4. **Business span hierarchy** -- Add child spans inside `CaptureClipboardUseCase::execute_with_origin` for each pipeline step. Enables stage-based filtering and future waterfall views. Six spans: normalize, persist_event, select_policy, persist_entry, spool_blobs, complete.

5. **Seq CLEF layer + local ingestion** -- Custom `tracing::Layer` implementation. Highest complexity item but provides the most powerful debugging capability. Can be feature-gated behind `#[cfg(feature = "seq")]` to avoid production overhead. Start with event-only ingestion (no span lifecycle), add @tr/@ps for trace view as follow-up.

Defer within milestone:

- **Seq trace visualization** (@tr/@ps/@st): Build basic event ingestion first, then add span lifecycle tracking.
- **Log profile hot-switch**: Restart-based profile switching covers 95% of use cases.
- **Representation-level tracing**: Add after the basic span hierarchy proves useful.
- **Sync flow tracing**: Apply the pattern from capture flow; lower priority since capture is the primary debugging target.

## Sources

- [tracing-subscriber fmt module](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html) -- JSON formatter, layer composition (HIGH confidence)
- [tracing-subscriber JSON format](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html) -- `.with_current_span()`, `.with_span_list()` (HIGH confidence)
- [Seq CLEF ingestion docs](https://datalust.co/docs/posting-raw-events) -- HTTP API, CLEF format spec (HIGH confidence)
- [Seq Docker getting started](https://docs.datalust.co/docs/getting-started-with-docker) -- Local Seq setup (HIGH confidence)
- [Seq tracing visualization](https://datalust.co/blog/tracing-first-look) -- @tr, @ps, @st properties in Seq 2024.1+ (MEDIUM confidence, blog post)
- [seqcli command-line client](https://github.com/datalust/seqcli) -- Alternative pipe-based ingestion (HIGH confidence)
- [Custom tracing layers tutorial](https://burgers.io/custom-logging-in-rust-using-tracing) -- Building custom Layer implementations (MEDIUM confidence, blog)
- [tracing-subscriber reload layer](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/reload/index.html) -- Runtime filter switching (HIGH confidence)
- [tracing span docs](https://docs.rs/tracing/latest/tracing/struct.Span.html) -- Field inheritance, `Span::or_current` (HIGH confidence)
- Existing code: `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` -- Current subscriber setup
- Existing code: `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` -- Current span instrumentation pattern
- Existing code: `src-tauri/crates/uc-platform/src/ports/observability.rs` -- Existing `TraceMetadata` with `trace_id: Uuid`
