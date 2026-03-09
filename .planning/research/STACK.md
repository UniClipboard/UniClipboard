# Technology Stack: Log Observability (v0.3.0)

**Project:** UniClipboard Desktop
**Researched:** 2026-03-09
**Focus:** Stack additions for structured business logging, dual output, configurable profiles, and Seq ingestion

## Current Tracing Stack (Already In Place -- DO NOT modify)

| Technology              | Version | Crate                                            | Status                                            |
| ----------------------- | ------- | ------------------------------------------------ | ------------------------------------------------- |
| tracing                 | 0.1     | uc-core, uc-app, uc-infra, uc-platform, uc-tauri | In use across all crates                          |
| tracing-subscriber      | 0.3     | uc-tauri (features: env-filter, fmt, chrono)     | Subscriber init in bootstrap/tracing.rs           |
| tracing-appender        | 0.2     | uc-tauri                                         | Non-blocking file writer with WorkerGuard         |
| tracing-log             | 0.2     | uc-tauri                                         | Legacy log bridge                                 |
| sentry + sentry-tracing | 0.46.1  | uc-tauri                                         | Error reporting layer (conditional on SENTRY_DSN) |

## Recommended Stack Additions

### 1. JSON Feature Flag on tracing-subscriber (MODIFY existing dep)

| Property       | Value                                                                                                                                                                     |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **What**       | Add `"json"` to tracing-subscriber features in uc-tauri/Cargo.toml                                                                                                        |
| **Current**    | `tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "chrono"] }`                                                                                    |
| **Target**     | `tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "chrono", "json"] }`                                                                            |
| **Why**        | Enables `.json()` on `fmt::layer()` for NDJSON file output. Built-in, zero extra deps, supports `flatten_event(true)`, `with_current_span(true)`, `with_span_list(true)`. |
| **Confidence** | HIGH -- verified via official docs.rs documentation                                                                                                                       |

This is the single most important change. It unlocks:

- `tracing_subscriber::fmt::layer().json()` for the JSON file output layer
- `flatten_event(true)` to put business fields (`flow_id`, `stage`) at the root of each JSON line
- `with_current_span(true)` to include the active span context in every event
- Native NDJSON output compatible with Seq CLEF ingestion

### 2. reqwest (NEW dependency, for Seq HTTP ingestion)

| Property       | Value                                                                                                                                                                                                                 |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Crate**      | `reqwest`                                                                                                                                                                                                             |
| **Version**    | `0.13`                                                                                                                                                                                                                |
| **Features**   | `["json", "rustls-tls"]`                                                                                                                                                                                              |
| **Where**      | uc-tauri                                                                                                                                                                                                              |
| **Why**        | HTTP client for POSTing NDJSON batches to Seq's `/ingest/clef` endpoint. Tokio-native async, ecosystem standard (306M+ downloads). No official Seq Rust client exists -- direct HTTP POST is the documented approach. |
| **Confidence** | HIGH -- reqwest 0.13.2 is current (released 2026-02-06), Seq HTTP API is stable                                                                                                                                       |

**Why reqwest over alternatives:**

- `ureq` is sync-only -- this app is fully async/Tokio
- `hyper` is too low-level for simple POST requests
- Tauri's HTTP plugin is frontend-oriented, not suitable for background Rust tasks
- `reqwest` matches the existing Tokio runtime and is battle-tested

**Why `rustls-tls` feature:** Avoids native OpenSSL dependency, consistent cross-platform behavior (macOS/Windows/Linux). The app already uses Rust-native crypto (XChaCha20-Poly1305), so staying Rust-native for TLS is consistent.

### 3. uuid (ALREADY present in uc-core -- reuse, no change)

| Property       | Value                                                                                                       |
| -------------- | ----------------------------------------------------------------------------------------------------------- |
| **Current**    | `uuid = { version = "1", features = ["v4", "fast-rng", "serde"] }` in uc-core                               |
| **Action**     | Reuse from uc-core for `flow_id` generation. No new dependency needed.                                      |
| **Why**        | UUID v4 is correct for flow correlation IDs -- globally unique, no ordering assumptions, already available. |
| **Confidence** | HIGH                                                                                                        |

### 4. No New Dependencies for Configurable Log Profiles

Log profiles (dev/prod/debug_clipboard) require zero new crates:

| Component                     | Implementation                  | Existing Dep                       |
| ----------------------------- | ------------------------------- | ---------------------------------- |
| Profile selection             | Enum + match in tracing init    | None needed                        |
| Per-profile filters           | `tracing_subscriber::EnvFilter` | Already in use                     |
| Per-profile layer composition | `Option<Layer>` pattern         | Already in tracing.rs              |
| Profile config storage        | TOML settings                   | `toml = "0.8"` already in uc-tauri |

The current `build_filter_directives()` in `bootstrap/tracing.rs` already branches on `is_development()`. Extending to a profile enum is a code change, not a dependency change.

### 5. No New Dependencies for Dual Output

The existing `tracing-subscriber` + `tracing-appender` combo supports dual output natively:

```rust
// Pretty console layer (existing pattern, keep)
let console_layer = fmt::layer().pretty().with_writer(io::stdout);

// JSON file layer (NEW, enabled by json feature flag)
let json_layer = fmt::layer()
    .json()
    .flatten_event(true)
    .with_current_span(true)
    .with_writer(non_blocking_file_writer);

registry()
    .with(env_filter)
    .with(console_layer)
    .with(json_layer)
    .init();
```

No additional crates needed beyond the `json` feature flag.

## What NOT to Add

| Library                                | Why NOT                                                                                                                                     |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `tracing-opentelemetry`                | Overkill for v0.3.0. Seq CLEF ingestion via HTTP is simpler and sufficient. OTel is explicitly a v0.4.0 scope item per PROJECT.md.          |
| `opentelemetry` + `opentelemetry-otlp` | Same as above. Reserve for future multi-backend/collector phase.                                                                            |
| `json-subscriber`                      | Drop-in replacement for tracing-subscriber JSON, but unnecessary -- the built-in `json` feature covers all needs. Extra dep for no benefit. |
| `tracing-bunyan-formatter`             | Bunyan format is not CLEF. Built-in JSON layer produces the right format.                                                                   |
| `tracing-flame` / `tracing-chrome`     | Performance profiling tools, not log observability. Out of scope.                                                                           |
| `tracing-forest`                       | Pretty-prints span trees. Built-in `.pretty()` format covers dev console.                                                                   |
| `seq` crate (crates.io)                | Unrelated crate (sequence generation), not a Seq logging client.                                                                            |
| `slog` / `log4rs`                      | Competing logging frameworks. Project is committed to `tracing`.                                                                            |
| `reqwest-middleware`                   | Retry/middleware overkill for fire-and-forget log ingestion. Simple retry loop in the background task is sufficient.                        |

## Seq Integration Architecture

### Approach: Custom tracing Layer + reqwest

Build a thin custom `Layer` implementation in uc-tauri that:

1. Captures events via `on_event()` -- filter to business-level events only
2. Formats them as CLEF JSON (`@t`, `@mt`, `@l`, plus structured fields like `flow_id`, `stage`)
3. Buffers events in a `tokio::sync::mpsc` channel (already available, no new dep)
4. Background task drains the channel and POSTs NDJSON batches to Seq via reqwest

**Why custom Layer over tailing the JSON log file:**

- Real-time ingestion (no file-tailing lag)
- Selective: can send only business events, reducing noise in Seq
- Proper backpressure via channel capacity
- Clean shutdown integration with existing TaskRegistry/CancellationToken pattern

### CLEF Format Mapping

tracing event fields map to CLEF reified properties:

| CLEF Property | Source                   | Description                                                                          |
| ------------- | ------------------------ | ------------------------------------------------------------------------------------ |
| `@t`          | Event timestamp          | ISO 8601 UTC (from tracing's timer)                                                  |
| `@mt`         | `message` field          | Message template                                                                     |
| `@l`          | Event level              | Mapped: TRACE->Verbose, DEBUG->Debug, INFO->Information, WARN->Warning, ERROR->Error |
| `@x`          | Error field (if present) | Exception/error text                                                                 |
| `flow_id`     | Span field               | Business flow correlation ID                                                         |
| `stage`       | Span field               | Pipeline stage (capture/persist/publish)                                             |
| `target`      | Event metadata           | Rust module path                                                                     |

### Seq Server (Development)

```bash
docker run --name seq -d --restart unless-stopped \
  -e ACCEPT_EULA=Y \
  -p 5341:80 \
  datalust/seq:latest
```

- UI: `http://localhost:5341`
- Ingestion: `POST http://localhost:5341/ingest/clef`
- Content-Type: `application/vnd.serilog.clef`
- No API key for local dev
- Free Individual license: single-developer use, no restrictions

## Installation Summary

```toml
# uc-tauri/Cargo.toml -- ONLY CHANGES

# MODIFY existing line (add "json" feature):
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "chrono", "json"] }

# ADD new dependency:
reqwest = { version = "0.13", features = ["json", "rustls-tls"] }
```

**Total new crate additions: 1** (reqwest + its transitive deps). Everything else is feature-flag activation or code changes.

## Version Verification

| Crate              | Recommended         | Latest on crates.io | Verified Date      |
| ------------------ | ------------------- | ------------------- | ------------------ |
| tracing-subscriber | 0.3 (+json feature) | 0.3.20              | 2026-03-09         |
| tracing-appender   | 0.2 (no change)     | 0.2.4               | 2026-03-09         |
| reqwest            | 0.13                | 0.13.2 (2026-02-06) | 2026-03-09         |
| tracing            | 0.1 (no change)     | 0.1.x               | Already in use     |
| uuid               | 1 (no change)       | 1.x                 | Already in uc-core |

## Dependency Graph Impact

```
uc-tauri (bootstrap/tracing.rs)
├── tracing-subscriber [+json feature]  <-- MODIFY
│   ├── Pretty console layer (existing, switch to .pretty())
│   ├── JSON file layer (NEW via .json() + flatten_event)
│   └── Seq CLEF layer (NEW custom Layer impl)
├── tracing-appender (existing, reuse for JSON file writer)
├── reqwest 0.13 [json, rustls-tls]  <-- NEW
│   └── Used by Seq layer background task
├── sentry-tracing (existing, no change)
└── tracing-log (existing, no change)

uc-core (NO dependency changes)
└── uuid (reuse for flow_id)

uc-app (NO dependency changes)
└── tracing (existing, add business span instrumentation)

uc-infra, uc-platform (NO dependency changes)
└── tracing (existing)
```

## Confidence Assessment

| Decision                                | Confidence | Rationale                                                                           |
| --------------------------------------- | ---------- | ----------------------------------------------------------------------------------- |
| JSON feature flag on tracing-subscriber | HIGH       | Official docs, built-in feature, verified API                                       |
| reqwest for Seq ingestion               | HIGH       | No official Seq Rust client; HTTP API is documented approach; reqwest is standard   |
| Custom Layer for Seq (not file tailing) | MEDIUM     | Standard pattern from tracing ecosystem, but requires ~150-200 lines of custom code |
| CLEF format compatibility               | HIGH       | Seq docs explicitly document CLEF HTTP ingestion                                    |
| Reuse uuid for flow_id                  | HIGH       | Already in project, correct tool for the job                                        |
| No OTel in v0.3.0                       | HIGH       | PROJECT.md explicitly lists OTel as v0.4.0 scope                                    |
| No new deps for profiles/dual output    | HIGH       | Built-in tracing-subscriber capabilities cover the need                             |

## Sources

- [tracing-subscriber JSON format docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html)
- [tracing-subscriber layer composition](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/)
- [tracing-subscriber crates.io](https://crates.io/crates/tracing-subscriber)
- [Seq HTTP Ingestion (CLEF)](https://datalust.co/docs/posting-raw-events)
- [Seq Docker setup](https://docs.datalust.co/docs/getting-started-with-docker)
- [Seq pricing (free Individual license)](https://datalust.co/pricing)
- [reqwest on crates.io](https://crates.io/crates/reqwest)
- [Custom tracing Layer tutorial (Bryan Burgers)](https://burgers.io/custom-logging-in-rust-using-tracing)
- [tracing-appender on crates.io](https://crates.io/crates/tracing-appender)

---

_Stack research for: UniClipboard v0.3.0 Log Observability milestone_
_Researched: 2026-03-09_
