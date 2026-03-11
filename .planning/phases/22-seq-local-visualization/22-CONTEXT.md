# Phase 22: Seq Local Visualization - Context

**Gathered:** 2026-03-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Deliver configurable Seq ingestion so developers can stream structured tracing events to a local Seq instance and query a single clipboard capture or sync flow as an ordered sequence of stages. Covers requirements SEQ-01 through SEQ-06. This phase does NOT include OTel integration, remote/cloud log shipping, or runtime profile switching.

</domain>

<decisions>
## Implementation Decisions

### CLEF Format Strategy

- Create a new `CLEFFormat` formatter in uc-observability, separate from the existing `FlatJsonFormat`
- CLEFFormat outputs Seq-native CLEF fields: `@t` (timestamp), `@l` (level), `@m` (rendered message)
- Business fields (`flow_id`, `stage`, etc.) are flattened to the CLEF JSON top level — same approach as FlatJsonFormat — so Seq auto-indexes them as queryable properties
- Existing JSON file layer continues using FlatJsonFormat unchanged
- The Seq layer uses CLEFFormat exclusively; the two formatters coexist independently

### CLEF Message Template

- Claude's Discretion — decide whether to use `@m` only or include `@mt` based on CLEF spec and Seq query experience

### Transport & Batching

- HTTP client: reqwest (already a project dependency), async with tokio runtime
- Architecture: Seq Layer formats events to CLEF strings, sends via mpsc channel to a background tokio task that batches and HTTP POSTs to Seq's `/api/events/raw` endpoint
- Batch trigger: time + count dual trigger (flush when N events accumulated OR T seconds elapsed, whichever comes first). Exact values are Claude's discretion
- Error handling: Seq unavailable → silently discard events. Console and JSON file outputs are unaffected. Seq is an optional dev tool, not a critical path
- Lifecycle: `build_seq_layer()` returns a `SeqGuard` (similar to existing `WorkerGuard` pattern). Dropping the guard sends a shutdown signal and flushes remaining buffered events

### Configuration Surface

- Seq enabled/disabled: presence of `UC_SEQ_URL` environment variable controls activation. If set → Seq layer created. If unset → no Seq layer, zero overhead
- Endpoint: `UC_SEQ_URL` (e.g., `http://localhost:5341`). No default value — must be explicitly set to enable
- API key: `UC_SEQ_API_KEY` optional. If set → added as `X-Seq-ApiKey` HTTP header. If unset → no auth header sent. Local Seq dev mode works without auth
- Log filter level: Seq layer follows the same `LogProfile` filter as JSON file layer. No separate filter configuration
- Public API: `build_seq_layer(config, profile)` returns `Option<(Layer, SeqGuard)>` — caller checks env vars, calls builder if Seq is configured, composes with existing layers

### Developer Setup Flow

- Provide a `docker-compose.seq.yml` in the project root for one-command Seq startup
- Seq uses default ports: 5341 (data ingestion) and 80 (UI dashboard)
- Documentation: update existing `docs/architecture/logging-architecture.md` with Seq integration section covering setup, configuration, and usage

### Claude's Discretion

- Exact batch size and flush interval parameters
- CLEFFormat internal implementation details (span traversal can share logic with FlatJsonFormat)
- `@mt` message template handling
- Channel buffer size for the mpsc channel
- Docker Compose file specifics (Seq version, volume mounts, ACCEPT_EULA)
- Test strategy for Seq layer (mock HTTP server vs. integration tests)
- Whether to extract shared span-traversal logic between FlatJsonFormat and CLEFFormat

</decisions>

<specifics>
## Specific Ideas

- Phase 19 pre-reserved module structure in uc-observability for Seq integration — the new `seq` module fits naturally there
- The `build_seq_layer` function follows the same composable pattern as `build_console_layer` / `build_json_layer` — caller composes all layers on a shared Registry
- SeqGuard follows the same lifecycle pattern as WorkerGuard (OnceLock static storage in the caller)
- CLEFFormat can reuse the span-traversal and field-flattening logic from FlatJsonFormat, just with different output field names (@t, @l, @m instead of timestamp, level, message)

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `FlatJsonFormat` (format.rs): Span traversal and field flattening logic — CLEFFormat can reuse the same approach for collecting span fields
- `build_console_layer` / `build_json_layer` (init.rs): Composable layer builder pattern — `build_seq_layer` follows the same signature style
- `LogProfile` (profile.rs): Profile-based filter construction — Seq layer reuses `profile.json_filter()` or equivalent
- `WorkerGuard` + `OnceLock` pattern (init.rs): Guard lifecycle management — SeqGuard follows the same approach
- `FlowId` / stage constants (flow.rs, stages.rs): Already exist and will appear in CLEF output via span field flattening
- `reqwest` crate: Already in workspace dependencies, supports async HTTP

### Established Patterns

- `fmt::layer().event_format(Format).fmt_fields(JsonFields).with_writer(writer).with_filter(filter)` — layer composition pattern
- `tracing_appender::non_blocking()` + `WorkerGuard` — async writer with guard lifecycle
- `mpsc::channel` + background task — used in runtime for clipboard/network command dispatch
- Environment variable configuration — `UC_LOG_PROFILE` precedent for observability config

### Integration Points

- `uc-observability/src/lib.rs`: New public exports for `build_seq_layer`, `SeqGuard`, `CLEFFormat`
- `uc-observability/Cargo.toml`: Add `reqwest` and `tokio` dependencies
- `uc-tauri/bootstrap/tracing.rs`: Compose Seq layer alongside console and JSON layers on the Registry
- `src-tauri/Cargo.toml`: Workspace dependency for reqwest (already exists)
- Project root: New `docker-compose.seq.yml`
- `docs/architecture/logging-architecture.md`: Seq integration documentation section

</code_context>

<deferred>
## Deferred Ideas

- Full OpenTelemetry integration (traces/logs/metrics) — future milestone
- Remote/cloud log shipping (Datadog, Honeycomb) — out of scope, local-only
- Runtime profile hot-switching (OBS-01) — future milestone
- In-app log viewer UI — Seq handles this
- Distributed tracing across devices — future milestone

</deferred>

---

_Phase: 22-seq-local-visualization_
_Context gathered: 2026-03-11_
