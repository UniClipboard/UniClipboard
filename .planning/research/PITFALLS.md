# Domain Pitfalls

**Domain:** Adding structured logging, dual output, Seq integration, and business flow tracing to existing Rust/Tauri desktop app
**Researched:** 2026-03-09
**Overall confidence:** HIGH (verified against codebase + official docs + community reports)

## Critical Pitfalls

### Pitfall 1: Type Hell When Composing Multiple tracing-subscriber Layers

**What goes wrong:** Adding a JSON file layer alongside the existing pretty stdout layer causes Rust type inference to explode. Each `.with(layer)` call changes the subscriber's concrete type, making conditional composition (e.g., "add Seq layer only if configured") impossible without type erasure.

**Why it happens:** `tracing-subscriber`'s `Layered<A, B>` nests generically. Two different layer combinations produce incompatible types. The current `init_tracing_subscriber()` in `tracing.rs` already uses a conditional `if let Some(layer)` pattern for the file layer (lines 187-191), but adding a third conditional layer (JSON file) and fourth (Seq HTTP) will make this pattern unworkable.

**Consequences:** Compilation failure. Developers waste hours fighting the type system instead of implementing features. Temptation to give up and use a single monolithic subscriber.

**Prevention:**

- Use `Vec<Box<dyn Layer<S> + Send + Sync>>` pattern from the start. Push each conditional layer as `.boxed()` into a Vec, then apply the Vec to the registry in one `.with()` call.
- Alternative: use `Option<L>` wrapping (since `Option<L>` implements `Layer` when `L: Layer`), which the codebase already uses for `file_layer` and `sentry_layer`. This works for up to 3-4 optional layers but gets unwieldy beyond that.
- The boxed Vec approach has negligible runtime overhead (dynamic dispatch on log path is not performance-critical).

**Detection:** First attempt to add third or fourth conditional layer hits compile errors with `Layered<Layered<...>>` type mismatches.

**Phase to address:** Phase 1 (dual output subscriber refactoring) -- must be solved before any new layers are added.

**Confidence:** HIGH -- well-documented community issue ([Rust Forum: Type Hell in Tracing](https://users.rust-lang.org/t/type-hell-in-tracing-multiple-output-layers/126764)), verified against official `tracing-subscriber` docs.

---

### Pitfall 2: WorkerGuard Lifetime Mismanagement Causing Lost Log Events

**What goes wrong:** Adding a second `NonBlocking` writer (for JSON file output) produces a second `WorkerGuard`. If either guard is dropped prematurely, buffered log events for that output are silently lost. The current code stores the guard in a `static OnceLock<WorkerGuard>` (line 32 of `tracing.rs`), which only supports ONE guard.

**Why it happens:** `tracing_appender::non_blocking()` returns `(NonBlocking, WorkerGuard)`. The guard must live for the entire program lifetime. With dual output, there are two guards. `OnceLock` can only store one value.

**Consequences:** JSON file logs intermittently empty or truncated. Particularly dangerous because it manifests as silent data loss -- no error, no warning, just missing log lines. Extremely hard to debug.

**Prevention:**

- Replace `static LOG_GUARD: OnceLock<WorkerGuard>` with `static LOG_GUARDS: OnceLock<Vec<WorkerGuard>>` or a struct holding multiple named guards.
- Return all guards from `init_tracing_subscriber()` and bind them in `main()`. The current pattern of storing in `OnceLock` is acceptable but must accommodate multiple guards.
- Never use `_` (discard) binding for guards. Always use `_guard` or named binding.
- Add a startup self-check: after init, emit a test event and verify it appears in both outputs.

**Detection:** JSON log file exists but is empty or has fewer events than console output.

**Phase to address:** Phase 1 (dual output subscriber) -- must handle this when creating the second NonBlocking writer.

**Confidence:** HIGH -- verified against `tracing-appender` official docs and [tokio-rs/tracing#1120](https://github.com/tokio-rs/tracing/issues/1120).

---

### Pitfall 3: flow_id Context Lost Across tokio::spawn Boundaries

**What goes wrong:** A `flow_id` span field set in `CaptureClipboardUseCase::execute_with_origin` is visible within that async block (via `.instrument(span)`), but when downstream work is spawned via `tokio::spawn` (e.g., spool queue processing, sync outbound), the span context is NOT automatically propagated. Child tasks log without `flow_id`, breaking end-to-end flow tracing.

**Why it happens:** `tokio::spawn` creates a new task with no parent span by default. Unlike thread-local contexts, Tokio task contexts require explicit span attachment. The codebase already uses `.instrument(span)` correctly in `capture_clipboard.rs`, but spawned background tasks (spool workers, sync outbound) will not inherit this context.

**Consequences:** Flow visualization in Seq shows disconnected events. The entire purpose of flow_id tracing (seeing capture -> persist -> publish as one flow) is defeated. Debugging cross-layer issues remains as hard as before.

**Prevention:**

- Every `tokio::spawn` that continues a business flow must receive the `flow_id` as a parameter and create its own child span with that `flow_id` field.
- Create a helper: `fn spawn_instrumented<F>(span: Span, future: F)` that wraps `tokio::spawn(future.instrument(span))`.
- For channel-based communication (e.g., `spool_queue.enqueue()`), include `flow_id` in the message payload so the consumer can reconstruct context.
- Do NOT use `Span::current()` across spawn boundaries -- it captures the spawn-site span, not the business flow span.

**Detection:** Query Seq for events with a specific `flow_id` and find gaps where downstream operations have no `flow_id` field.

**Phase to address:** Phase 2 (business flow instrumentation) -- the span hierarchy design must account for spawn boundaries from the start.

**Confidence:** HIGH -- verified against [Tokio tracing docs](https://tokio.rs/tokio/topics/tracing) and `tracing` crate documentation on async instrumentation.

---

### Pitfall 4: Seq Ingestion Choice Locks In or Locks Out Future OTel Migration

**What goes wrong:** Choosing direct CLEF HTTP ingestion to Seq (simple, no dependencies) makes it hard to later add OpenTelemetry support. Choosing OTel from the start adds 4-5 new crate dependencies and significant complexity for what is currently a local-only desktop app.

**Why it happens:** Seq supports two ingestion paths: (1) direct CLEF via `/ingest/clef` (simple HTTP POST of newline-delimited JSON), and (2) OTLP via `/ingest/otlp/v1/logs` (requires `opentelemetry-otlp` + `opentelemetry-appender-tracing` crates). The PROJECT.md explicitly lists "OTel trace/log layer" as a v0.4.0 goal, creating a sequencing tension.

**Consequences:**

- If CLEF chosen now: must build a custom tracing Layer that formats CLEF and POSTs via HTTP. Works, but custom code to maintain. Later OTel migration replaces this entirely.
- If OTel chosen now: heavy dependency burden (`opentelemetry`, `opentelemetry-sdk`, `opentelemetry-otlp`, `opentelemetry-appender-tracing`, `tonic` or `reqwest`). Overkill for local desktop Seq. But aligns with v0.4.0 goals.

**Prevention:**

- Use CLEF for v0.3.0. It is the simplest path for local Seq and avoids premature OTel complexity. The custom Layer is ~100 lines of code (format CLEF JSON, batch, POST via reqwest).
- Design the Layer as a standalone module behind a feature flag, so it can be swapped for OTel in v0.4.0 without touching the rest of the subscriber pipeline.
- Ensure the JSON file output uses a format that Seq can also import (CLEF-compatible), so the file itself serves as a backup ingestion source.

**Detection:** Architecture review finds either (a) OTel dependencies added prematurely, or (b) CLEF Layer tightly coupled to subscriber init making future replacement painful.

**Phase to address:** Phase 3 (Seq integration) -- the ingestion strategy must be decided before implementation begins.

**Confidence:** MEDIUM -- Seq's CLEF and OTLP endpoints verified via [official docs](https://datalust.co/docs/posting-raw-events). The OTel-Rust ecosystem maturity assessment is based on WebSearch findings and may shift.

---

### Pitfall 5: JSON File Logging Produces Unbounded Disk Growth

**What goes wrong:** The current file appender uses `tracing_appender::rolling::never` (line 201 of `tracing.rs`), which creates a single non-rotating log file. Adding a second JSON file output with the same strategy doubles disk growth. On a desktop app running 24/7 with clipboard events, the JSON file grows without bound.

**Why it happens:** `rolling::never` was fine for a single human-readable log. JSON structured logs are typically larger per event (more fields, full span context). Combined with clipboard capture events (potentially dozens per hour), disk usage accumulates.

**Consequences:** Users with limited disk space eventually see degraded performance or disk-full errors. Desktop apps cannot assume server-class storage.

**Prevention:**

- Use `tracing_appender::rolling::daily` or `rolling::hourly` for the JSON file output.
- Implement a retention policy: delete JSON files older than N days (7 days default). This is NOT built into `tracing-appender` and must be implemented manually (a startup task that scans the log directory).
- Set a reasonable max file size alert or implement size-based rotation using a custom appender.
- Keep the pretty console log on `rolling::never` (stdout doesn't grow on disk) but switch both file outputs to rolling.

**Detection:** Monitor log directory size over a week of normal use. If it exceeds 100MB, rotation policy is needed.

**Phase to address:** Phase 1 (dual output) -- file rotation must be configured when the JSON file writer is created.

**Confidence:** HIGH -- verified against current `tracing.rs` code and `tracing-appender` rolling API docs.

---

## Moderate Pitfalls

### Pitfall 6: EnvFilter Applies Globally, Not Per-Layer

**What goes wrong:** The current single `EnvFilter` (line 117 of `tracing.rs`) controls filtering for ALL layers. When adding a JSON layer intended for debug-level clipboard events alongside a console layer at info level, a single filter cannot serve both purposes.

**Why it happens:** `EnvFilter` is applied to the registry, not to individual layers. This is a common misunderstanding.

**Prevention:**

- Use per-layer filtering: `tracing_subscriber` supports `.with_filter()` on individual layers (requires the `registry` feature, which is already in use).
- Design the "log profiles" feature (dev/prod/debug_clipboard) as sets of per-layer filter configurations, not global filter switches.
- Example: console layer gets `info` filter, JSON file layer gets `debug` filter for `uc_app::usecases::clipboard` target.

**Detection:** Setting `RUST_LOG=debug` floods the console with events that should only go to the JSON file.

**Phase to address:** Phase 1 (log profiles design).

**Confidence:** HIGH -- verified against `tracing-subscriber` per-layer filtering docs.

---

### Pitfall 7: Dual log/tracing Systems Creating Duplicate or Missing Events

**What goes wrong:** The codebase has TWO logging systems: the `tauri-plugin-log` setup in `logging.rs` (using the `log` crate) and the `tracing-subscriber` setup in `tracing.rs`. The `tracing-log` bridge (in Cargo.toml) can create duplicate events or circular forwarding.

**Why it happens:** Historical migration path. `tauri-plugin-log` uses the `log` crate facade. `tracing-subscriber` uses `tracing` macros. With `tracing-log` bridge active, `log::info!()` events are forwarded to `tracing` subscriber AND processed by `tauri-plugin-log`, creating duplicates in console. Without the bridge, `log::info!()` events bypass the JSON structured output entirely.

**Consequences:** Duplicate events in console output. Or, events from legacy `log::*` calls missing from JSON structured output (breaking the "complete capture" goal).

**Prevention:**

- Audit all crate files for remaining `log::info!()` / `log::debug!()` usage. The `uc-tauri` Cargo.toml still lists `log = "0.4"` as a dependency.
- Decide: either complete the migration to `tracing::*` macros before adding JSON output, or configure the `tracing-log` bridge carefully to avoid duplicates.
- Remove or disable `tauri-plugin-log` once tracing subscriber handles all output targets. The plugin and the tracing subscriber should not both be active.

**Detection:** Same event appears twice in console output, or events from certain modules are missing from JSON output.

**Phase to address:** Phase 1 (subscriber refactoring) -- resolve the dual-system conflict before adding new outputs.

**Confidence:** HIGH -- verified by reading both `logging.rs` and `tracing.rs` in the codebase.

---

### Pitfall 8: Structured Fields Not Propagating Through Hexagonal Boundaries

**What goes wrong:** A span with `flow_id` created at the use case layer (uc-app) includes the field, but infrastructure calls (uc-infra database queries, uc-platform clipboard reads) do not automatically inherit structured fields. Seq queries for `flow_id = "abc"` miss infrastructure-level events.

**Why it happens:** Span fields are attached to the span, not to child events. Infrastructure methods create their own spans (or none at all). Unless infrastructure explicitly enters or instruments with a child span of the use case span, the parent span's fields are not included in the infrastructure event's structured output.

**Prevention:**

- Use `#[tracing::instrument(skip_all, fields(flow_id))]` on port trait implementations in uc-infra, and populate `flow_id` from the current span context using `Span::current().record("flow_id", ...)`.
- Alternatively, ensure infrastructure methods are called within the use case's instrumented async block (already the case for `capture_clipboard.rs`), and configure the JSON formatter with `.with_current_span(true)` and `.with_span_list(true)` to include parent span fields.
- Test by querying Seq: `flow_id = "test-id"` should return events from all layers.

**Detection:** Seq query for a `flow_id` returns use-case events but not infrastructure events.

**Phase to address:** Phase 2 (business flow instrumentation).

**Confidence:** MEDIUM -- the `.with_current_span(true)` behavior is documented but the actual field inheritance depends on formatter configuration.

---

### Pitfall 9: Seq HTTP Layer Blocking the Async Runtime

**What goes wrong:** A naive Seq CLEF ingestion layer makes synchronous HTTP calls in the `on_event` callback of the tracing Layer trait. Since `on_event` is called synchronously in the logging path, this blocks the Tokio runtime, causing latency spikes in clipboard capture.

**Why it happens:** The `Layer` trait's `on_event` method is synchronous. You cannot `.await` inside it. Developers unfamiliar with this constraint try to use `reqwest::blocking` or `tokio::runtime::Handle::block_on`, both of which cause deadlocks or thread starvation in an async context.

**Consequences:** Clipboard capture latency increases by 10-100ms per event (HTTP round trip to local Seq). Under burst clipboard activity, the app becomes unresponsive.

**Prevention:**

- Use an async channel buffer pattern: the Layer's `on_event` formats the CLEF JSON and sends it to an `mpsc::Sender`. A separate background task (spawned at init) batches events and POSTs them to Seq asynchronously.
- Set a bounded channel (e.g., 1000 events). If Seq is unreachable, drop events rather than applying backpressure to the logging path.
- Use `tracing_appender::non_blocking` as inspiration: same pattern of buffered async writes.

**Detection:** Clipboard capture latency noticeably increases when Seq layer is enabled. Flamegraph shows time spent in HTTP calls on the tracing subscriber path.

**Phase to address:** Phase 3 (Seq integration).

**Confidence:** HIGH -- the `Layer::on_event` synchronous constraint is well-documented in `tracing-subscriber` trait docs.

---

### Pitfall 10: Log Profiles Conflicting with RUST_LOG Environment Override

**What goes wrong:** The app introduces named log profiles (dev/prod/debug_clipboard) that configure per-layer filters. But `RUST_LOG` environment variable overrides the EnvFilter, wiping out profile-specific filter configurations.

**Why it happens:** `EnvFilter::try_from_default_env()` (line 117 of current `tracing.rs`) takes precedence when `RUST_LOG` is set. Profile-based filters set programmatically are ignored.

**Prevention:**

- Establish clear precedence: `RUST_LOG` > profile config > defaults. Document this.
- When `RUST_LOG` is set, apply it only to the console layer (developer override), not to the JSON/Seq layers (which should always follow profile config).
- Use per-layer filters so `RUST_LOG` only affects the console layer via `EnvFilter`, while JSON/Seq layers use programmatic `Targets` or `LevelFilter`.

**Detection:** Developer sets `RUST_LOG=trace` and JSON file floods with trace-level events from all crates.

**Phase to address:** Phase 1 (log profiles design).

**Confidence:** HIGH -- verified against current `tracing.rs` EnvFilter usage.

---

## Minor Pitfalls

### Pitfall 11: CLEF Timestamp Format Mismatch

**What goes wrong:** Seq expects `@t` in ISO 8601 format with timezone. The current tracing timestamp uses `ChronoUtc` with format `%Y-%m-%d %H:%M:%S%.3f` (no timezone suffix). If the JSON layer reuses this format for `@t`, Seq may misparse timestamps.

**Prevention:** Use RFC 3339 / ISO 8601 with explicit `Z` suffix for CLEF `@t` field: `2026-03-09T10:30:45.123Z`.

**Phase to address:** Phase 3 (Seq integration).

---

### Pitfall 12: Sensitive Clipboard Content Leaking Into Structured Fields

**What goes wrong:** Adding structured logging with fields like `title = %entry.title` or `content_preview = ...` inadvertently logs clipboard text content into JSON files and Seq, where it persists and is searchable.

**Prevention:** Only log content-free metadata: `entry_id`, `mime_type`, `byte_count`, `representation_count`. Never log `title`, `content`, or `bytes` fields. Add a code review checklist item for this.

**Phase to address:** Phase 2 (business flow instrumentation) -- must be enforced before any business span fields are defined.

---

### Pitfall 13: Sentry Layer Interference With New Layers

**What goes wrong:** The existing Sentry integration (`sentry_tracing::layer()` at line 137 of `tracing.rs`) captures events and sends them to Sentry. Adding flow_id and verbose business spans may cause excessive Sentry event volume and costs, or Sentry may strip custom fields that are needed for Seq correlation.

**Prevention:** Apply a filter to the Sentry layer that only captures `warn` and above. Business flow spans at `info`/`debug` level should not reach Sentry. This is independent of the JSON/Seq layer filters.

**Phase to address:** Phase 1 (subscriber refactoring) -- when restructuring the layer composition.

---

## Phase-Specific Warnings

| Phase Topic                            | Likely Pitfall                                                                               | Mitigation                                                                     |
| -------------------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Phase 1: Dual Output Subscriber        | Type hell with conditional layers (P1), Guard lifetime (P2), Dual log systems (P7)           | Use boxed Vec pattern, multi-guard storage, resolve log/tracing conflict first |
| Phase 1: Log Profiles                  | EnvFilter global scope (P6), RUST_LOG override (P10)                                         | Per-layer filters, clear precedence documentation                              |
| Phase 2: Business Flow Instrumentation | flow_id lost across spawn (P3), Fields not crossing hex boundaries (P8), Content leaks (P12) | spawn helper with explicit context, test Seq queries, metadata-only fields     |
| Phase 3: Seq Integration               | Ingestion strategy lock-in (P4), Blocking async runtime (P9), Timestamp format (P11)         | CLEF with async channel buffer, RFC 3339 timestamps, feature-flagged Layer     |
| Phase 3: File Rotation                 | Unbounded disk growth (P5)                                                                   | Rolling daily + retention cleanup task                                         |

## "Looks Done But Isn't" Checklist

- [ ] **Dual output working:** Both console and JSON file show events; event counts match for same time window.
- [ ] **flow_id end-to-end:** Query Seq for a single clipboard capture flow_id and see events from capture, persist, and publish stages.
- [ ] **Spawn boundary coverage:** Spawned background tasks (spool worker, sync outbound) include flow_id in their spans.
- [ ] **No content leaks:** JSON log file and Seq contain no clipboard text content, only metadata.
- [ ] **Disk bounded:** JSON log directory has rotation policy; 7-day simulation shows stable disk usage.
- [ ] **Seq offline resilience:** Disabling Seq does not cause errors, log loss, or performance degradation in other outputs.
- [ ] **Legacy log migration:** No remaining `log::info!()` calls in uc-app/uc-core/uc-infra crates (or bridge correctly configured).

## Recovery Strategies

| Pitfall                                    | Recovery Cost | Recovery Steps                                                                    |
| ------------------------------------------ | ------------- | --------------------------------------------------------------------------------- |
| Type hell in subscriber composition (P1)   | LOW           | Refactor to boxed Vec pattern; mostly mechanical change                           |
| Lost log events from guard drop (P2)       | MEDIUM        | Audit guard storage, add multi-guard support, verify with integration test        |
| flow_id missing across spawns (P3)         | MEDIUM        | Audit all tokio::spawn sites in clipboard pipeline, add instrumented spawn helper |
| Wrong Seq ingestion strategy (P4)          | HIGH          | Must rewrite Layer implementation if switching between CLEF and OTel              |
| Unbounded disk growth discovered late (P5) | LOW           | Switch to rolling appender + add retention task; no data model changes            |
| Blocking Seq HTTP in Layer (P9)            | MEDIUM        | Refactor to async channel + background task pattern                               |

## Sources

- [tracing-subscriber Layer composition docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/)
- [Rust Forum: Type Hell in Tracing](https://users.rust-lang.org/t/type-hell-in-tracing-multiple-output-layers/126764)
- [tracing-appender WorkerGuard docs](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/struct.WorkerGuard.html)
- [tokio-rs/tracing#1120 - WorkerGuard flush guarantee](https://github.com/tokio-rs/tracing/issues/1120)
- [Tokio: Diagnostics with Tracing](https://tokio.rs/blog/2019-08-tracing)
- [Tokio: Getting Started with Tracing](https://tokio.rs/tokio/topics/tracing)
- [Seq: Ingestion with HTTP (CLEF)](https://datalust.co/docs/posting-raw-events)
- [Seq: What's New in 2025.2](https://datalust.co/docs/whats-new)
- [OpenTelemetry Context Propagation](https://opentelemetry.io/docs/concepts/context-propagation/)
- Current codebase: `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs`, `logging.rs`, `main.rs`

---

_Pitfalls research for: UniClipboard v0.3.0 Log Observability_
_Researched: 2026-03-09_
