# Architecture Patterns

**Domain:** Log observability for Tauri 2 / Rust clipboard sync app
**Researched:** 2026-03-09
**Confidence:** HIGH (tracing-subscriber layer composition), MEDIUM (Seq CLEF custom layer)

## Recommended Architecture

### High-Level Design

The log observability system adds three capabilities to the existing tracing infrastructure:

1. **Structured business fields** (flow_id, stage) propagated via tracing span context
2. **Dual output** (pretty console + JSON file) replacing current single-format stdout+file
3. **Seq ingestion** via a custom CLEF HTTP layer posting structured events to local Seq

All three integrate at a single point: `init_tracing_subscriber()` in `uc-tauri/src/bootstrap/tracing.rs`. The existing `Registry + EnvFilter + fmt layer` pipeline gains additional layers; no changes needed to the hexagonal boundary contracts.

```
                     tracing::Registry
                           |
                     ┌─────┴─────┐
                     │ EnvFilter  │  (existing, extended with profile presets)
                     └─────┬─────┘
                           |
              ┌────────────┼────────────┐
              |            |            |
     ┌────────┴───┐  ┌────┴────┐  ┌───┴──────────┐
     │ Pretty fmt │  │JSON fmt │  │ Seq CLEF     │
     │ (stdout)   │  │ (file)  │  │ HTTP Layer   │
     └────────────┘  └─────────┘  └──────────────┘
         existing       NEW           NEW
        (modified)
```

### Component Boundaries

| Component                                 | Responsibility                                                               | Crate      | Status                                       |
| ----------------------------------------- | ---------------------------------------------------------------------------- | ---------- | -------------------------------------------- |
| `tracing.rs` (bootstrap)                  | Build subscriber pipeline, apply log profile                                 | `uc-tauri` | MODIFY                                       |
| `LogProfile` enum                         | Define preset filter+output configurations                                   | `uc-tauri` | NEW                                          |
| Pretty stdout layer                       | Human-readable console output with ANSI                                      | `uc-tauri` | MODIFY (already exists)                      |
| JSON file layer                           | Machine-readable JSON output to rotating file                                | `uc-tauri` | NEW (replaces current plain-text file layer) |
| `SeqLayer`                                | Custom `tracing_subscriber::Layer` that batches CLEF events and POSTs to Seq | `uc-tauri` | NEW                                          |
| Business span fields (`flow_id`, `stage`) | Structured context on capture pipeline spans                                 | `uc-app`   | MODIFY (add fields to existing spans)        |

### Data Flow

**Clipboard capture with observability:**

```
1. ClipboardWatcher detects change
   -> PlatformEvent::ClipboardChanged { snapshot }

2. AppRuntime callback invoked
   -> Creates root span: info_span!("clipboard.capture", flow_id = %Uuid::new_v4())

3. CaptureClipboardUseCase::execute_with_origin
   -> Existing span "usecase.capture_clipboard.execute" inherits flow_id from parent
   -> Child spans: normalize, persist_event, select_policy, save_entry

4. Each span/event flows through Registry to ALL layers:
   |-- Pretty layer -> stdout (dev: colored, shows flow_id)
   |-- JSON layer -> file (structured, searchable)
   +-- Seq layer -> HTTP POST batched CLEF to localhost:5341
```

**Context propagation is automatic:** `tracing`'s span inheritance means a `flow_id` field set on the root span is visible to all child spans and events without explicit threading. No ports or cross-layer plumbing needed.

## New Components Detail

### 1. LogProfile (uc-tauri/src/bootstrap/log_profile.rs)

A profile selects which layers are active and at what filter level.

```rust
/// Log output profile controlling subscriber composition.
///
/// Each profile defines: filter directives, active layers, and format options.
#[derive(Debug, Clone)]
pub enum LogProfile {
    /// Development: pretty stdout (debug) + JSON file (debug)
    Dev,
    /// Production: pretty stdout (info) + JSON file (info) + Seq (info)
    Prod,
    /// Debug clipboard pipeline: pretty stdout (trace for uc_app) + JSON file (trace)
    DebugClipboard,
}

impl LogProfile {
    pub fn filter_directives(&self) -> Vec<String> { /* ... */ }
    pub fn enable_json_file(&self) -> bool { true } // all profiles
    pub fn enable_seq(&self) -> bool {
        matches!(self, Self::Prod)
    }
}
```

**Integration point:** `init_tracing_subscriber()` gains a `LogProfile` parameter. Current `build_filter_directives(is_dev)` logic moves into `LogProfile::Dev` / `LogProfile::Prod`. The function signature changes from `fn init_tracing_subscriber() -> Result<()>` to `fn init_tracing_subscriber(profile: LogProfile) -> Result<()>`.

**Profile selection:** Determined at startup by `cfg!(debug_assertions)` (Dev vs Prod), overridable via `UNICLIPBOARD_LOG_PROFILE` env var for developer use.

### 2. JSON File Layer (uc-tauri/src/bootstrap/tracing.rs)

Replace the current plain-text file `fmt::layer()` with a JSON-formatted one.

```rust
let json_file_layer = if profile.enable_json_file() {
    let writer = build_file_writer()?; // existing function, reused
    Some(
        fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(true)
            .with_timer(fmt::time::ChronoUtc::new(
                "%Y-%m-%dT%H:%M:%S%.3fZ".to_string(),
            ))
            .with_ansi(false)
            .with_writer(writer)
    )
} else {
    None
};
```

**Dependency:** Requires `tracing-subscriber` `json` feature flag. Current `uc-tauri/Cargo.toml` has:

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "chrono"] }
```

Change to:

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "chrono", "json"] }
```

### 3. SeqLayer - Custom CLEF HTTP Layer (NEW file)

**Location:** `uc-tauri/src/bootstrap/seq_layer.rs`

No existing Rust crate provides a tracing-subscriber Layer for Seq CLEF ingestion. A custom layer is the correct approach because:

- CLEF is a simple newline-delimited JSON format (trivial to serialize with serde_json)
- The HTTP API is a single POST endpoint (`/ingest/clef`)
- Avoids pulling in the heavy OpenTelemetry SDK stack (~15 transitive deps) for a local dev tool
- Seq's OTLP endpoint only supports HTTP/protobuf and gRPC (no HTTP/JSON), making CLEF simpler

**Architecture:**

```rust
/// Custom tracing Layer that batches events as CLEF JSON and POSTs to Seq.
///
/// Design:
/// - Events serialized to CLEF in on_event() (sync, fast)
/// - Buffered in mpsc channel
/// - Background tokio task flushes batches every N seconds or N events
/// - Non-blocking: subscriber thread never waits for HTTP
pub struct SeqLayer {
    event_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

/// Background flush task, spawned once at init
struct SeqFlusher {
    event_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    endpoint: String,       // "http://localhost:5341/ingest/clef"
    api_key: Option<String>,
    client: reqwest::Client,
    flush_interval: Duration,
    batch_size: usize,
}
```

**CLEF field mapping from tracing:**

| CLEF Field               | Source                                                                                     |
| ------------------------ | ------------------------------------------------------------------------------------------ |
| `@t`                     | Event timestamp (ISO 8601)                                                                 |
| `@mt`                    | Message template from `tracing::Event` format string                                       |
| `@l`                     | Level mapped: ERROR->Error, WARN->Warning, INFO->Information, DEBUG->Debug, TRACE->Verbose |
| `@i`                     | Event target or span name                                                                  |
| `flow_id`, `stage`, etc. | Flattened from span context fields                                                         |
| All other fields         | Flattened into top-level CLEF properties                                                   |

**HTTP details:**

- Endpoint: `POST http://localhost:5341/ingest/clef`
- Content-Type: `application/vnd.serilog.clef`
- Body: Newline-delimited JSON (one CLEF object per line)
- Optional: `X-Seq-ApiKey` header or `?apiKey=` query param

**Dependencies needed (uc-tauri/Cargo.toml):**

- `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }` -- HTTP client for Seq POST

### 4. Business Span Fields (uc-app use cases)

**Modify existing spans** in `CaptureClipboardUseCase` and the AppRuntime callback to include structured business fields.

**flow_id generation point:** The AppRuntime callback in `uc-tauri/src/bootstrap/runtime.rs` where `CaptureClipboardUseCase` is invoked. This is the natural entry point for a clipboard capture flow.

```rust
// In AppRuntime clipboard change handler (runtime.rs)
let flow_id = uuid::Uuid::new_v4();
let span = info_span!(
    "clipboard.capture",
    flow_id = %flow_id,
    stage = "initiated",
);
// CaptureClipboardUseCase::execute runs inside this span
```

**Existing span in capture_clipboard.rs (line 133-138) gains stage field:**

```rust
let span = info_span!(
    "usecase.capture_clipboard.execute",
    source = "callback",
    origin = ?origin,
    representations = snapshot.representations.len(),
    stage = "execute",  // NEW
);
```

**New child spans for sub-operations:**

```rust
// Normalize representations
async { /* ... */ }
    .instrument(info_span!("normalize_representations", stage = "normalize"))
    .await?;

// Persist event
self.event_writer
    .insert_event(&new_event, &normalized_reps)
    .instrument(info_span!("persist_event", stage = "persist"))
    .await?;

// Save entry
self.entry_repo
    .save_entry_and_selection(&new_entry, &new_selection)
    .instrument(info_span!("save_entry", stage = "save"))
    .await?;
```

The `flow_id` does NOT need to be threaded explicitly. Tracing's span context inheritance propagates it automatically from the parent `clipboard.capture` span to all children. This respects hexagonal architecture: no observability concerns leak into port traits.

## Patterns to Follow

### Pattern 1: Optional Layer Composition

**What:** Wrap each layer in `Option<L>` so profiles can enable/disable layers without changing subscriber types.
**When:** Building the subscriber pipeline in `init_tracing_subscriber()`.
**Why this works:** `tracing-subscriber` natively supports `Option<L>: Layer` -- an `Option::None` is a no-op layer. This pattern is already used in the codebase for `sentry_layer` and `file_layer`.

```rust
registry()
    .with(env_filter)
    .with(sentry_layer)    // existing Option<Layer>
    .with(stdout_layer)    // always present
    .with(json_file_layer) // Option<Layer>, profile-controlled
    .with(seq_layer)       // Option<Layer>, profile-controlled
    .try_init()?;
```

### Pattern 2: Non-Blocking Channel-Based Ingestion

**What:** SeqLayer sends serialized CLEF strings through an unbounded mpsc channel to a background Tokio task that batches and POSTs.
**When:** Always for the Seq layer.
**Why:** `tracing-subscriber` Layer callbacks (`on_event()`) run synchronously on the logging thread. Any blocking (HTTP, disk flush) stalls the application. The channel decouples serialization (sync, fast) from network I/O (async, slow).

### Pattern 3: Span Field Inheritance for Cross-Layer Context

**What:** Set `flow_id` on a root span once; all child spans and events inherit it through tracing's built-in context propagation.
**When:** Any time you need a correlation ID across use case steps.
**Why:** Avoids threading `flow_id` through function parameters, port traits, or shared state.

**Critical detail:** The JSON layer must use `.with_current_span(true)` and `.with_span_list(true)` to capture inherited fields in output. The SeqLayer must implement `on_new_span()` to collect span fields for CLEF serialization.

## Anti-Patterns to Avoid

### Anti-Pattern 1: OpenTelemetry SDK for Local Seq

**What:** Using `tracing-opentelemetry` + `opentelemetry-otlp` + protobuf to send to Seq's OTLP endpoint.
**Why bad:** Pulls in ~15 transitive dependencies (opentelemetry, tonic, prost, http, tower), adds compile time, and OTLP is designed for distributed multi-service tracing. Massive overkill for a single desktop app sending to a local Seq instance.
**Instead:** Custom SeqLayer with direct CLEF HTTP POST via reqwest.

### Anti-Pattern 2: Passing flow_id Through Port Traits

**What:** Adding `flow_id: &str` parameters to `ClipboardEntryRepositoryPort::save_entry_and_selection()` etc.
**Why bad:** Pollutes domain port contracts with observability concerns. Violates hexagonal architecture.
**Instead:** Use tracing span context. The flow_id lives in the span hierarchy, not in function signatures.

### Anti-Pattern 3: Separate Log Configuration File

**What:** Creating a `log.toml` or `log.yaml` for users to configure logging.
**Why bad:** Log profiles are a developer tool for v0.3.0, not user-facing. Config files add UX surface area with no user value.
**Instead:** Compile-time `LogProfile` enum + optional `UNICLIPBOARD_LOG_PROFILE` env var override.

### Anti-Pattern 4: Runtime Profile Switching

**What:** Making the log profile changeable at runtime via settings UI.
**Why bad:** `tracing-subscriber` global subscriber is set once with `try_init()`. Changing requires `reload::Layer` adding fragile complexity.
**Instead:** Set profile at startup. Restart to change. Acceptable for developer tooling.

## Scalability Considerations

| Concern            | Low Volume (dev) | High Volume (rapid clipboard)   | Mitigation                                                       |
| ------------------ | ---------------- | ------------------------------- | ---------------------------------------------------------------- |
| JSON file size     | Negligible       | Grows with image capture spans  | `tracing-appender::rolling::daily` instead of `rolling::never`   |
| Seq backpressure   | N/A              | Burst could overwhelm local Seq | Bounded batch size, drop-on-overflow in channel                  |
| Serialization cost | Negligible       | JSON per event per layer        | `serde_json::to_string` is fast; profile filtering reduces count |
| CLEF buffer memory | Negligible       | Burst buffers many events       | Cap channel at ~10K events, drop oldest on overflow              |

## Integration Points: Existing Code Changes

### Files to MODIFY

| File                                                | Change                                                                        | Reason                       |
| --------------------------------------------------- | ----------------------------------------------------------------------------- | ---------------------------- |
| `uc-tauri/src/bootstrap/tracing.rs`                 | Add LogProfile param, JSON layer, Seq layer composition, profile-based filter | Central subscriber init      |
| `uc-tauri/src/bootstrap/mod.rs`                     | Export new `seq_layer` and `log_profile` modules                              | Module visibility            |
| `uc-tauri/Cargo.toml`                               | Add `json` feature to tracing-subscriber, add `reqwest`                       | JSON format + HTTP for Seq   |
| `uc-app/src/usecases/internal/capture_clipboard.rs` | Add `stage` field to existing spans, add child spans for sub-operations       | Business span hierarchy      |
| `uc-tauri/src/bootstrap/runtime.rs`                 | Add root `clipboard.capture` span with `flow_id` at callback entry            | Flow correlation root        |
| `src-tauri/src/main.rs`                             | Pass `LogProfile` to `init_tracing_subscriber()`                              | Profile selection at startup |

### Files to CREATE

| File                                    | Purpose                                     |
| --------------------------------------- | ------------------------------------------- |
| `uc-tauri/src/bootstrap/seq_layer.rs`   | Custom SeqLayer + SeqFlusher implementation |
| `uc-tauri/src/bootstrap/log_profile.rs` | LogProfile enum and preset configurations   |

### Files UNCHANGED

| File                                          | Why                                             |
| --------------------------------------------- | ----------------------------------------------- |
| All port traits in `uc-core/ports/`           | Observability is orthogonal to domain contracts |
| All repository implementations in `uc-infra/` | No logging changes at infra layer for v0.3.0    |
| Frontend (React/TypeScript)                   | No frontend changes for backend logging         |
| `uc-tauri/src/bootstrap/logging.rs`           | Legacy log bridge, independent of tracing       |

## Suggested Build Order

### Phase 1: Dual Output Foundation

**Build:** LogProfile enum + JSON file layer + modified pretty stdout layer
**Why first:** Zero external dependencies beyond a Cargo feature flag. Immediately testable by running the app and checking file output. Establishes the subscriber pipeline structure.
**Validates:** JSON output format correct, profile switching works, no regression on console.

### Phase 2: Business Span Hierarchy

**Build:** flow_id on root span, stage fields on capture pipeline, child spans for sub-operations
**Why second:** Requires Phase 1 JSON output to verify structured fields. Pure Rust changes in uc-app and runtime.rs.
**Validates:** flow_id appears in both console and JSON, stage traces through lifecycle, hierarchy correct.

### Phase 3: Seq CLEF Layer

**Build:** SeqLayer, SeqFlusher, CLEF serialization, HTTP batched POST
**Why third:** Depends on Phases 1-2 for pipeline and fields. Requires running Seq for integration testing. Most complex new code.
**Validates:** Events appear in Seq UI with flow_id/stage searchable, batch flushing works.

### Phase 4: Polish and Hardening

**Build:** Rolling file appender (daily rotation), channel overflow handling, graceful SeqFlusher shutdown (integrate with TaskRegistry), env var override for profile.
**Why last:** Non-blocking improvements. System works without these but they prevent operational issues.

## Sources

- [tracing-subscriber docs](https://docs.rs/tracing-subscriber/) - Layer composition, JSON format, Optional layers (HIGH confidence)
- [tracing-subscriber fmt JSON format](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html) - JSON output configuration (HIGH confidence)
- [Seq raw event ingestion / CLEF format](https://datalust.co/docs/posting-raw-events) - CLEF field spec, HTTP endpoint (HIGH confidence)
- [Seq OTLP ingestion](https://datalust.co/docs/ingestion-with-opentelemetry) - OTLP alternative evaluated and rejected (MEDIUM confidence)
- [Seq GitHub Rust discussion](https://github.com/datalust/seq-tickets/discussions/1873) - Confirms no existing Rust CLEF crate (MEDIUM confidence)
- [Structured JSON logs with tracing in Rust](https://oneuptime.com/blog/post/2026-01-25-structured-json-logs-tracing-rust/view) - Dual output patterns (MEDIUM confidence)

---

_Architecture research for: UniClipboard v0.3.0 Log Observability_
_Researched: 2026-03-09_
