# Phase 22: Seq Local Visualization - Research

**Researched:** 2026-03-11
**Domain:** Structured log ingestion via Seq CLEF over HTTP, tracing Layer composition
**Confidence:** HIGH

## Summary

Phase 22 adds a Seq ingestion layer to the existing uc-observability crate. The implementation is well-scoped: a new `CLEFFormat` formatter, a background HTTP sender with mpsc batching, and a `build_seq_layer()` builder function that composes alongside the existing console and JSON layers. The CLEF specification is minimal and stable (7 reified properties, newline-delimited JSON). Seq's `/ingest/clef` endpoint accepts batched CLEF payloads via HTTP POST with optional API key authentication.

The existing codebase provides strong foundations: `FlatJsonFormat` demonstrates the exact span-traversal and field-flattening logic that `CLEFFormat` needs (with different output field names), `build_json_layer` shows the composable layer builder pattern, and the `WorkerGuard`/`OnceLock` lifecycle pattern is already established. reqwest is not currently a direct dependency in any crate (only transitive via tauri plugins), so it must be added to `uc-observability/Cargo.toml` along with tokio.

**Primary recommendation:** Implement CLEFFormat as a new `FormatEvent` impl reusing the span-traversal pattern from FlatJsonFormat, with a background tokio task for batched HTTP delivery to Seq's `/ingest/clef` endpoint.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Create a new `CLEFFormat` formatter in uc-observability, separate from the existing `FlatJsonFormat`
- CLEFFormat outputs Seq-native CLEF fields: `@t` (timestamp), `@l` (level), `@m` (rendered message)
- Business fields (`flow_id`, `stage`, etc.) are flattened to the CLEF JSON top level
- Existing JSON file layer continues using FlatJsonFormat unchanged
- The Seq layer uses CLEFFormat exclusively; the two formatters coexist independently
- HTTP client: reqwest (already a project dependency), async with tokio runtime
- Architecture: Seq Layer formats events to CLEF strings, sends via mpsc channel to a background tokio task that batches and HTTP POSTs to Seq's `/api/events/raw` endpoint
- Batch trigger: time + count dual trigger (flush when N events accumulated OR T seconds elapsed, whichever comes first)
- Error handling: Seq unavailable -> silently discard events. Console and JSON file outputs are unaffected
- Lifecycle: `build_seq_layer()` returns a `SeqGuard` (similar to existing `WorkerGuard` pattern)
- Seq enabled/disabled: presence of `UC_SEQ_URL` environment variable controls activation
- Endpoint: `UC_SEQ_URL` (e.g., `http://localhost:5341`). No default value
- API key: `UC_SEQ_API_KEY` optional. If set -> added as `X-Seq-ApiKey` HTTP header
- Log filter level: Seq layer follows the same `LogProfile` filter as JSON file layer
- Public API: `build_seq_layer(config, profile)` returns `Option<(Layer, SeqGuard)>`
- Provide a `docker-compose.seq.yml` in the project root for one-command Seq startup
- Seq uses default ports: 5341 (data ingestion) and 80 (UI dashboard)
- Documentation: update existing `docs/architecture/logging-architecture.md` with Seq integration section

### Claude's Discretion

- Exact batch size and flush interval parameters
- CLEFFormat internal implementation details (span traversal can share logic with FlatJsonFormat)
- `@mt` message template handling
- Channel buffer size for the mpsc channel
- Docker Compose file specifics (Seq version, volume mounts, ACCEPT_EULA)
- Test strategy for Seq layer (mock HTTP server vs. integration tests)
- Whether to extract shared span-traversal logic between FlatJsonFormat and CLEFFormat

### Deferred Ideas (OUT OF SCOPE)

- Full OpenTelemetry integration (traces/logs/metrics) -- future milestone
- Remote/cloud log shipping (Datadog, Honeycomb) -- out of scope, local-only
- Runtime profile hot-switching (OBS-01) -- future milestone
- In-app log viewer UI -- Seq handles this
- Distributed tracing across devices -- future milestone
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID     | Description                                                                                                | Research Support                                                                                 |
| ------ | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| SEQ-01 | Application can send structured log events to a local Seq instance via HTTP in CLEF-compatible JSON format | CLEFFormat formatter + background HTTP sender to `/ingest/clef` endpoint                         |
| SEQ-02 | Seq integration is a dedicated tracing Layer enabled/disabled via configuration without code changes       | `build_seq_layer()` returns `Option<(Layer, SeqGuard)>` controlled by `UC_SEQ_URL` env var       |
| SEQ-03 | Seq Layer batches events and flushes asynchronously so log ingestion does not block main execution         | mpsc channel + background tokio task with dual-trigger batching (count + time)                   |
| SEQ-04 | Events ingested into Seq include `flow_id` and `stage` fields for flow querying                            | CLEF flattens span fields to top-level JSON -- same approach as FlatJsonFormat                   |
| SEQ-05 | Seq configuration (endpoint URL and API key) can be set via env var with sensible defaults                 | `UC_SEQ_URL` enables Seq, `UC_SEQ_API_KEY` optional, no auth needed for local dev                |
| SEQ-06 | Seq displays capture flows as time-ordered sequences of stages for a given `flow_id`                       | CLEF `@t` provides ordering; `flow_id` and `stage` at top level enable `flow_id = 'xxx'` queries |

</phase_requirements>

## Standard Stack

### Core

| Library            | Version | Purpose                                 | Why Standard                                                 |
| ------------------ | ------- | --------------------------------------- | ------------------------------------------------------------ |
| reqwest            | 0.12    | Async HTTP client for Seq ingestion     | De facto Rust HTTP client; already transitive dep in project |
| tokio              | 1.x     | Async runtime for background flush task | Already project runtime; mpsc channels for batching          |
| tracing-subscriber | 0.3     | Layer trait for Seq layer composition   | Already in uc-observability deps                             |
| serde_json         | 1       | CLEF JSON serialization                 | Already in uc-observability deps                             |
| chrono             | 0.4     | ISO 8601 timestamp formatting (`@t`)    | Already in uc-observability deps                             |

### Supporting

| Library               | Version | Purpose                            | When to Use                                   |
| --------------------- | ------- | ---------------------------------- | --------------------------------------------- |
| datalust/seq (Docker) | 2024.3+ | Local Seq instance for development | `docker compose -f docker-compose.seq.yml up` |

### Alternatives Considered

| Instead of        | Could Use                     | Tradeoff                                                                  |
| ----------------- | ----------------------------- | ------------------------------------------------------------------------- |
| reqwest           | hyper (low-level)             | reqwest is simpler; hyper is overkill for POST-only usage                 |
| Custom CLEF layer | tracing-seq crate             | No mature Rust Seq crate exists; custom is appropriate here               |
| mpsc batching     | tracing-appender non_blocking | non_blocking writes to io::Write; we need HTTP POST batching, not file IO |

**Installation (uc-observability/Cargo.toml additions):**

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
tokio = { version = "1", features = ["sync", "time", "rt"] }
```

## Architecture Patterns

### Recommended Module Structure

```
src-tauri/crates/uc-observability/src/
├── lib.rs              # Add: pub mod seq; + re-exports
├── format.rs           # Existing FlatJsonFormat (unchanged)
├── clef_format.rs      # NEW: CLEFFormat formatter
├── seq/                # NEW: Seq ingestion module
│   ├── mod.rs          # pub mod layer, sender; + build_seq_layer()
│   ├── layer.rs        # SeqLayer impl (formats + sends to channel)
│   └── sender.rs       # Background HTTP sender task + SeqGuard
├── init.rs             # Existing (unchanged)
├── profile.rs          # Existing (unchanged)
├── flow.rs             # Existing (unchanged)
└── stages.rs           # Existing (unchanged)
```

### Pattern 1: CLEFFormat as FormatEvent Implementation

**What:** A new `FormatEvent` impl that produces CLEF-compatible JSON with `@t`, `@l`, `@m` fields and flattened span properties.
**When to use:** Exclusively for the Seq layer.
**Example:**

```rust
// CLEF output example:
// {"@t":"2026-03-11T10:30:00.123Z","@l":"Debug","@m":"Processing clipboard capture","flow_id":"01234...","stage":"normalize","target":"uc_app::usecases"}
pub struct CLEFFormat;

impl<S, N> FormatEvent<S, N> for CLEFFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(&self, ctx: &FmtContext<'_, S, N>, mut writer: Writer<'_>, event: &tracing::Event<'_>) -> fmt::Result {
        // 1. @t - ISO 8601 timestamp
        // 2. @l - Level mapped to Seq values (Verbose/Debug/Information/Warning/Error/Fatal)
        // 3. @m - Rendered message
        // 4. Flatten span fields to top level (reuse FlatJsonFormat pattern)
        // 5. Event fields at top level
        // 6. "target" field for Rust module path
    }
}
```

### Pattern 2: Channel-Based Async Batching

**What:** mpsc channel separates formatting (sync, in tracing pipeline) from HTTP delivery (async, background).
**When to use:** Always for Seq layer -- the tracing `FormatEvent` runs synchronously.
**Architecture:**

```
tracing event -> SeqLayer (format to CLEF string) -> mpsc::Sender
                                                         |
                                                         v
                                        Background tokio task (batches strings)
                                                         |
                                                         v
                                        HTTP POST to /ingest/clef (reqwest)
```

### Pattern 3: Composable Layer Builder

**What:** `build_seq_layer()` follows same signature pattern as `build_console_layer`/`build_json_layer`.
**When to use:** Called from `uc-tauri/bootstrap/tracing.rs` during subscriber initialization.
**Example:**

```rust
pub fn build_seq_layer<S>(
    profile: &LogProfile,
) -> Option<(impl tracing_subscriber::Layer<S> + Send + Sync, SeqGuard)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let url = std::env::var("UC_SEQ_URL").ok()?;
    let api_key = std::env::var("UC_SEQ_API_KEY").ok();
    // Build layer with CLEF format, mpsc channel, background sender
    // Return (layer, guard)
}
```

### Pattern 4: Dual-Trigger Batch Flush

**What:** Background task flushes when batch reaches N events OR T seconds elapse, whichever first.
**Recommended values:**

- Batch size: 100 events (balances HTTP overhead vs latency)
- Flush interval: 2 seconds (responsive enough for dev observation)
- Channel buffer: 1024 (generous; events silently dropped if full to avoid backpressure)

### Anti-Patterns to Avoid

- **Blocking HTTP in tracing pipeline:** Never call reqwest from within `FormatEvent` -- it runs synchronously in the subscriber. Always use the channel + background task pattern.
- **Panicking on Seq errors:** Seq is an optional dev tool. All Seq errors must be silently discarded (or at most logged once to stderr).
- **Sharing writer with JSON layer:** The Seq layer must have its own independent output path. Do not try to reuse `tracing_appender::non_blocking` for HTTP.

## Don't Hand-Roll

| Problem                  | Don't Build              | Use Instead                                 | Why                                                                |
| ------------------------ | ------------------------ | ------------------------------------------- | ------------------------------------------------------------------ |
| HTTP client              | Raw TCP/hyper            | reqwest with rustls-tls                     | Connection pooling, TLS, redirects handled                         |
| JSON serialization       | Manual string formatting | serde_json::Serializer with SerializeMap    | Correct escaping, Unicode handling                                 |
| Async channel            | crossbeam or std::sync   | tokio::sync::mpsc                           | Already on tokio runtime; integrates with select! for dual-trigger |
| Timer for flush interval | Manual Instant tracking  | tokio::time::interval or tokio::time::sleep | Correct async timer integration                                    |
| ISO 8601 timestamps      | Manual formatting        | chrono::Utc::now().to_rfc3339_opts()        | Already used in FlatJsonFormat                                     |

**Key insight:** The CLEF format is simple enough that a custom formatter is the right choice (no mature Rust CLEF library exists), but HTTP delivery and JSON serialization should use proven libraries.

## Common Pitfalls

### Pitfall 1: Tracing Level to CLEF Level Mapping

**What goes wrong:** Rust tracing uses TRACE/DEBUG/INFO/WARN/ERROR; CLEF/Seq expects Verbose/Debug/Information/Warning/Error/Fatal.
**Why it happens:** Different naming conventions between ecosystems.
**How to avoid:** Explicit mapping function:

```rust
fn tracing_level_to_clef(level: &tracing::Level) -> &'static str {
    match *level {
        tracing::Level::TRACE => "Verbose",
        tracing::Level::DEBUG => "Debug",
        tracing::Level::INFO => "Information",
        tracing::Level::WARN => "Warning",
        tracing::Level::ERROR => "Error",
    }
}
```

**Warning signs:** Events appear in Seq with wrong severity or unrecognized level colors.

### Pitfall 2: Information Level Omission

**What goes wrong:** CLEF spec says absence of `@l` implies "informational". Sending `"@l":"Information"` is redundant but harmless.
**How to avoid:** For simplicity, always include `@l`. This avoids edge cases and makes grep/debugging easier. Omitting it is a micro-optimization not worth the complexity.

### Pitfall 3: Seq Docker Port Mapping Confusion

**What goes wrong:** Seq exposes port 80 (UI + API) and port 5341 (ingestion only) inside the container. Mapping `5341:80` means the app POSTs to `localhost:5341` which hits the container's port 80 (full API).
**Why it happens:** Seq's internal port 5341 is ingestion-only, but in Docker you typically map host 5341 to container 80.
**How to avoid:** Use `5341:80` mapping in docker-compose. Document that `http://localhost:5341` serves both UI and ingestion in this setup.

### Pitfall 4: Channel Backpressure Blocking Tracing

**What goes wrong:** If the mpsc channel is full (Seq down, events accumulating), `send()` blocks the tracing pipeline.
**Why it happens:** Using bounded channel with blocking send.
**How to avoid:** Use `try_send()` instead of `send()`. If channel is full, silently drop the event. Seq ingestion must never block application logic.

### Pitfall 5: SeqGuard Drop Order

**What goes wrong:** If SeqGuard is dropped before all events are flushed, final events are lost.
**Why it happens:** Guard stored in wrong scope or dropped early.
**How to avoid:** Store in `OnceLock<SeqGuard>` static, same pattern as `JSON_GUARD` and `SENTRY_GUARD` in `uc-tauri/bootstrap/tracing.rs`. On drop, send shutdown signal and await final flush with timeout.

### Pitfall 6: CLEF Endpoint Path

**What goes wrong:** Using `/api/events/raw` (older Seq versions) instead of `/ingest/clef` (current).
**Why it happens:** Multiple Seq documentation versions reference different endpoints.
**How to avoid:** Use `/ingest/clef` endpoint. This is the current recommended endpoint for CLEF ingestion. The older `/api/events/raw?clef` also works but `/ingest/clef` is cleaner.

## Code Examples

### CLEFFormat Output Structure

```json
{
  "@t": "2026-03-11T10:30:00.123Z",
  "@l": "Debug",
  "@m": "Processing clipboard capture",
  "target": "uc_app::usecases::clipboard",
  "span": "normalize",
  "flow_id": "019579a3-7b4c-7def-8901-234567890abc",
  "stage": "normalize",
  "entry_id": "42"
}
```

### Background Sender Task (tokio::select! dual-trigger)

```rust
// Source: Standard tokio pattern for batched async work
async fn sender_loop(
    mut rx: mpsc::Receiver<String>,
    client: reqwest::Client,
    url: String,
    api_key: Option<String>,
) {
    let mut batch: Vec<String> = Vec::with_capacity(100);
    let mut interval = tokio::time::interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                batch.push(event);
                if batch.len() >= 100 {
                    flush_batch(&client, &url, &api_key, &mut batch).await;
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    flush_batch(&client, &url, &api_key, &mut batch).await;
                }
            }
        }
    }
}

async fn flush_batch(
    client: &reqwest::Client,
    url: &str,
    api_key: &Option<String>,
    batch: &mut Vec<String>,
) {
    let body = batch.join("\n");
    let mut req = client.post(format!("{}/ingest/clef", url))
        .header("Content-Type", "application/vnd.serilog.clef")
        .body(body);

    if let Some(key) = api_key {
        req = req.header("X-Seq-ApiKey", key);
    }

    // Silently discard errors -- Seq is optional dev tooling
    let _ = req.send().await;
    batch.clear();
}
```

### Layer Composition in uc-tauri/bootstrap/tracing.rs

```rust
// After existing console + json layer setup:
let seq_layer_and_guard = uc_observability::build_seq_layer(&profile);
let (seq_layer, seq_guard) = match seq_layer_and_guard {
    Some((layer, guard)) => (Some(layer), Some(guard)),
    None => (None, None),
};

if let Some(guard) = seq_guard {
    if SEQ_GUARD.set(guard).is_err() {
        eprintln!("Seq guard already initialized");
    }
}

tracing_subscriber::registry()
    .with(sentry_layer)
    .with(console_layer)
    .with(json_layer)
    .with(seq_layer)
    .try_init()?;
```

### Docker Compose File

```yaml
# docker-compose.seq.yml
services:
  seq:
    image: datalust/seq:2024.3
    restart: unless-stopped
    environment:
      ACCEPT_EULA: 'Y'
    ports:
      - '5341:80'
    volumes:
      - seq-data:/data

volumes:
  seq-data:
```

### Querying Flow in Seq

```
// Seq filter syntax to find all events for a specific flow:
flow_id = '019579a3-7b4c-7def-8901-234567890abc'

// Filter by flow and stage:
flow_id = '019579a3-7b4c-7def-8901-234567890abc' and stage = 'normalize'
```

## State of the Art

| Old Approach                        | Current Approach                             | When Changed | Impact                                                      |
| ----------------------------------- | -------------------------------------------- | ------------ | ----------------------------------------------------------- |
| `/api/events/raw?clef`              | `/ingest/clef`                               | Seq 2023+    | Cleaner endpoint path, same functionality                   |
| Serilog-only CLEF tooling           | Language-agnostic CLEF spec at clef-json.org | 2017+        | Any language can produce CLEF; Rust impl is straightforward |
| `@tr`/`@sp` for OpenTelemetry spans | Still supported but optional                 | Seq 2023.4+  | Not needed for this phase; deferred to OTel milestone       |

## CLEF @l Level Mapping Reference

| Rust tracing Level | CLEF @l Value | Notes                                     |
| ------------------ | ------------- | ----------------------------------------- |
| TRACE              | Verbose       | Noisiest level                            |
| DEBUG              | Debug         | Internal system events                    |
| INFO               | Information   | Default when @l absent                    |
| WARN               | Warning       | Degraded functionality                    |
| ERROR              | Error         | Broken expectations                       |
| (none in tracing)  | Fatal         | Not used; Rust tracing has no FATAL level |

## @mt Message Template Decision

**Recommendation: Use `@m` only (rendered message), omit `@mt`.**

Rationale:

- `@mt` is for Serilog-style message templates like `"Hello, {User}"` where Seq can re-render with different property values
- Rust tracing's `format!`-style messages are pre-rendered at the callsite; there is no recoverable template
- Including `@mt` with the same value as `@m` wastes bytes and provides no Seq benefit
- Seq indexes and searches `@m` the same way regardless of `@mt` presence

## Open Questions

1. **Shared span-traversal logic extraction**
   - What we know: FlatJsonFormat and CLEFFormat both need identical span-field collection logic (walk root-to-leaf, parse JsonFields extensions, collect into BTreeMap)
   - What's unclear: Whether the extraction overhead (new helper function/trait) is worth it for two callers
   - Recommendation: Extract a `collect_span_fields<S, N>(ctx) -> BTreeMap` helper function. The duplication is ~20 lines and both formatters need identical logic. A shared function prevents drift.

2. **Endpoint path: `/ingest/clef` vs `/api/events/raw?clef`**
   - What we know: Both work. `/ingest/clef` is the current recommendation.
   - Recommendation: Use `/ingest/clef`. The CONTEXT.md mentions `/api/events/raw` but the current Seq docs recommend `/ingest/clef`. Both work with current Seq versions, so either is fine. Use the modern path.

## Validation Architecture

### Test Framework

| Property           | Value                                                        |
| ------------------ | ------------------------------------------------------------ |
| Framework          | cargo test (built-in)                                        |
| Config file        | src-tauri/crates/uc-observability/Cargo.toml                 |
| Quick run command  | `cd src-tauri && cargo test -p uc-observability`             |
| Full suite command | `cd src-tauri && cargo test -p uc-observability -p uc-tauri` |

### Phase Requirements -> Test Map

| Req ID | Behavior                                                | Test Type | Automated Command                                                         | File Exists? |
| ------ | ------------------------------------------------------- | --------- | ------------------------------------------------------------------------- | ------------ |
| SEQ-01 | CLEFFormat produces valid CLEF JSON with @t, @l, @m     | unit      | `cd src-tauri && cargo test -p uc-observability clef_format`              | Wave 0       |
| SEQ-02 | build_seq_layer returns None when UC_SEQ_URL unset      | unit      | `cd src-tauri && cargo test -p uc-observability seq::build_seq_layer`     | Wave 0       |
| SEQ-03 | Sender task batches and flushes without blocking        | unit      | `cd src-tauri && cargo test -p uc-observability seq::sender`              | Wave 0       |
| SEQ-04 | CLEF output includes flow_id and stage from spans       | unit      | `cd src-tauri && cargo test -p uc-observability clef_format::span_fields` | Wave 0       |
| SEQ-05 | Configuration reads UC_SEQ_URL and UC_SEQ_API_KEY       | unit      | `cd src-tauri && cargo test -p uc-observability seq::config`              | Wave 0       |
| SEQ-06 | Events have @t timestamps enabling time-ordered queries | unit      | `cd src-tauri && cargo test -p uc-observability clef_format::timestamp`   | Wave 0       |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-observability`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-observability -p uc-tauri`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-observability/src/clef_format.rs` -- CLEFFormat tests (modeled on existing format.rs tests)
- [ ] `src-tauri/crates/uc-observability/src/seq/` -- Sender and layer tests
- [ ] reqwest + tokio dependencies in uc-observability/Cargo.toml

## Sources

### Primary (HIGH confidence)

- [CLEF Specification](http://clef-json.org/) - Complete CLEF reified properties, batching rules, encoding
- [Seq HTTP Ingestion Docs](https://datalust.co/docs/posting-raw-events) - `/ingest/clef` endpoint, headers, response codes, authentication
- Existing codebase: `uc-observability/src/format.rs` (FlatJsonFormat pattern), `init.rs` (layer builder pattern), `uc-tauri/bootstrap/tracing.rs` (composition site)

### Secondary (MEDIUM confidence)

- [Seq Docker Hub](https://hub.docker.com/r/datalust/seq) - Docker image configuration, port mapping
- [Seq Getting Started with Docker](https://docs.datalust.co/docs/getting-started-with-docker) - Docker Compose patterns, ACCEPT_EULA, volume mounts
- [Seq Level Mapping](https://github.com/datalust/seq-tickets/discussions/2324) - CLEF @l level value names (Verbose/Debug/Information/Warning/Error/Fatal)

### Tertiary (LOW confidence)

- None -- all findings verified with official sources

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - reqwest/tokio/tracing-subscriber are established; CLEF spec is stable and simple
- Architecture: HIGH - Pattern directly follows existing FlatJsonFormat + build_json_layer code in the codebase
- Pitfalls: HIGH - CLEF spec verified against official docs; level mapping confirmed with Seq documentation

**Research date:** 2026-03-11
**Valid until:** 2026-04-11 (stable domain; CLEF spec rarely changes)
