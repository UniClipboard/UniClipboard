# Project Research Summary

**Project:** UniClipboard Desktop v0.3.0 — Log Observability
**Domain:** Structured logging and observability for Tauri 2 / Rust desktop application
**Researched:** 2026-03-09
**Confidence:** HIGH

## Executive Summary

UniClipboard v0.3.0 adds structured log observability to an existing Tauri 2 clipboard sync app that already has a solid `tracing` foundation. The core challenge is not "add logging from scratch" but rather "upgrade existing tracing infrastructure to produce structured, correlated, machine-readable output that can be visualized in Seq." The existing `tracing` + `tracing-subscriber` stack handles 90% of the work; this milestone is primarily configuration changes, span field additions, and one custom Layer implementation for Seq ingestion.

The recommended approach is incremental: first refactor the subscriber pipeline to support dual output (pretty console + JSON file) with per-layer filtering and log profiles, then instrument the clipboard capture pipeline with `flow_id` correlation and `stage` fields using tracing's built-in span context inheritance, and finally build a thin custom CLEF HTTP layer to push structured events to a local Seq instance. Only one new crate dependency is needed (`reqwest` for HTTP POST to Seq); everything else leverages existing dependencies with feature flag additions. This minimal-dependency approach avoids the premature complexity of OpenTelemetry (explicitly deferred to v0.4.0) while producing immediately useful observability.

The primary risks are subscriber type complexity when composing 4+ conditional layers (solved with boxed Vec or Option pattern), `flow_id` context loss across `tokio::spawn` boundaries (solved with explicit span propagation helpers), and unbounded JSON log file growth on desktop (solved with rolling file appender). All risks have well-documented mitigation patterns in the tracing ecosystem. The dual `log`/`tracing` system coexistence is an existing technical debt that should be audited but not fully resolved in this milestone.

## Key Findings

### Recommended Stack

The stack strategy is conservative by design: one new dependency, one feature flag change, zero architectural disruptions. See [STACK.md](STACK.md) for full details.

**Core technologies:**

- **tracing-subscriber `json` feature** (MODIFY existing dep) — enables `.json()` formatter for NDJSON file output with `flatten_event`, `with_current_span`, `with_span_list`
- **reqwest 0.13** (NEW dep, `json` + `rustls-tls` features) — HTTP client for POSTing CLEF batches to Seq's `/ingest/clef` endpoint; Tokio-native, no official Seq Rust client exists
- **uuid** (REUSE from uc-core) — `Uuid::new_v4()` for `flow_id` correlation IDs; already available, no change needed
- **No new deps for profiles or dual output** — `LogProfile` enum, per-layer `EnvFilter`, and `Option<Layer>` composition all use existing tracing-subscriber capabilities

### Expected Features

See [FEATURES.md](FEATURES.md) for full feature landscape and dependency graph.

**Must have (table stakes):**

- `flow_id` correlation on clipboard capture pipeline (without this, structured logging is noise)
- `stage` field on business spans (normalize, persist, publish)
- Dual output: pretty console (dev) + JSON file (all environments)
- Structured JSON with span context (parent fields inherited in output)
- Log profiles: dev / prod / debug_clipboard
- Seq local CLEF ingestion

**Should have (differentiators):**

- Cross-layer context propagation (flow_id visible from platform through infra)
- Representation-level tracing (per-mime-type spans with size metadata)
- Seq trace/waterfall visualization via @tr/@ps CLEF properties
- Configurable Seq endpoint (env var / settings)
- Sync flow tracing (inbound + outbound, same flow_id pattern)

**Defer (v2+ / later milestones):**

- Full OpenTelemetry integration (v0.4.0 per PROJECT.md)
- Log profile hot-switch at runtime (restart-based switching is sufficient)
- Frontend React log integration (no value for backend pipeline observability)
- Log rotation / retention policies (defer within milestone, add in polish phase)
- Custom log viewer UI (Seq web UI is superior)
- Distributed tracing across devices (use device_id + timestamp in Seq queries)
- `log` crate bridge removal (not required, keep dual-track)

### Architecture Approach

The observability system integrates at a single point: `init_tracing_subscriber()` in `uc-tauri/src/bootstrap/tracing.rs`. The existing `Registry + EnvFilter + fmt layer` pipeline gains two new layers (JSON file, Seq CLEF) controlled by a `LogProfile` enum. Business span fields (`flow_id`, `stage`) are added to existing spans in `uc-app` use cases and the AppRuntime callback; tracing's span inheritance propagates them automatically without polluting port traits. See [ARCHITECTURE.md](ARCHITECTURE.md) for full component details.

**Major components:**

1. **LogProfile enum** (NEW, `uc-tauri`) — preset filter+output configurations for dev/prod/debug_clipboard
2. **JSON file layer** (NEW, `uc-tauri`) — replaces plain-text file output with structured NDJSON using `fmt::layer().json()`
3. **SeqLayer** (NEW, `uc-tauri`) — custom `tracing::Layer` that serializes CLEF JSON, buffers via mpsc channel, background task batches and POSTs to Seq
4. **Business span fields** (MODIFY, `uc-app` + `uc-tauri`) — `flow_id` on root capture span, `stage` on sub-operation spans

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for 13 identified pitfalls with prevention strategies. Top 5:

1. **Type hell with conditional layers (P1)** — Use `Option<Layer>` wrapping (existing pattern) or boxed Vec for 4+ layers. Must solve in Phase 1 before adding any new layers.
2. **WorkerGuard lifetime for dual file output (P2)** — Replace `OnceLock<WorkerGuard>` with `OnceLock<Vec<WorkerGuard>>`. Guard drop = silent log loss.
3. **flow_id lost across tokio::spawn (P3)** — Every spawned task continuing a business flow must explicitly receive and re-attach `flow_id` via a span. Create `spawn_instrumented` helper.
4. **Seq ingestion strategy lock-in vs OTel (P4)** — Use CLEF for v0.3.0, feature-flag the SeqLayer so it can be swapped for OTel in v0.4.0. ~100 lines of custom code, acceptable throwaway.
5. **EnvFilter is global, not per-layer (P6)** — Use per-layer `.with_filter()` for profiles. `RUST_LOG` should only override console layer, not JSON/Seq layers.

## Implications for Roadmap

Based on research, the milestone naturally decomposes into 4 phases with clear dependency ordering.

### Phase 1: Dual Output Foundation

**Rationale:** Everything downstream depends on the subscriber pipeline supporting multiple independently-filtered layers. Must solve type composition and guard lifetime before adding business logic.
**Delivers:** LogProfile enum, refactored subscriber with pretty console + JSON file output, per-layer filtering, multi-guard storage.
**Addresses:** Dual output, log profiles, structured JSON output (table stakes)
**Avoids:** Type hell (P1), guard lifetime (P2), dual log/tracing conflict (P7), EnvFilter global scope (P6), RUST_LOG override (P10), Sentry interference (P13)

### Phase 2: Business Flow Instrumentation

**Rationale:** Requires Phase 1 JSON output to verify structured fields appear correctly. Pure Rust span additions, no external dependencies.
**Delivers:** flow_id on capture pipeline, stage fields on sub-operations, child spans for normalize/persist/save steps, spawn-boundary propagation helper.
**Addresses:** flow_id correlation, stage fields, business span hierarchy, cross-layer context propagation (table stakes + differentiators)
**Avoids:** flow_id lost across spawns (P3), fields not crossing hex boundaries (P8), sensitive content leaks (P12)

### Phase 3: Seq CLEF Integration

**Rationale:** Depends on Phase 1 (subscriber pipeline) and Phase 2 (structured fields to ingest). Highest complexity, requires running Seq in Docker for testing.
**Delivers:** Custom SeqLayer with CLEF serialization, async channel + background flusher, reqwest HTTP POST, configurable endpoint.
**Uses:** reqwest 0.13 (only new dependency)
**Implements:** SeqLayer + SeqFlusher architecture from ARCHITECTURE.md
**Avoids:** Blocking async runtime (P9), OTel lock-in (P4), timestamp format mismatch (P11)

### Phase 4: Polish and Hardening

**Rationale:** System is functional after Phase 3. This phase handles operational robustness and edge cases.
**Delivers:** Rolling file appender (daily rotation), channel overflow handling, graceful SeqFlusher shutdown via TaskRegistry, env var profile override, startup self-check.
**Addresses:** Unbounded disk growth (P5), Seq offline resilience

### Phase Ordering Rationale

- **Phase 1 before Phase 2:** JSON output is needed to verify that flow_id and stage fields actually appear in structured output. Without it, you are instrumenting blind.
- **Phase 2 before Phase 3:** Seq ingestion is only valuable when there are meaningful structured fields to search and correlate. Ingesting unstructured events into Seq adds no value over grep.
- **Phase 3 before Phase 4:** Get the full pipeline working end-to-end, then harden. Rotation and overflow handling are operational concerns, not correctness concerns.
- **All phases within one milestone:** Each phase builds on the previous. No phase delivers standalone user value without the others (except Phase 1 which improves dev experience immediately).

### Research Flags

Phases likely needing deeper research during planning:

- **Phase 3 (Seq CLEF Integration):** Custom tracing Layer implementation has MEDIUM confidence. The pattern is documented in blog posts but not in official examples. ~150-200 lines of custom code with CLEF field mapping, channel buffering, and batch HTTP POST. Recommend `/gsd:research-phase` to validate CLEF format details and `on_event` field extraction API.

Phases with standard patterns (skip research-phase):

- **Phase 1 (Dual Output):** Well-documented tracing-subscriber patterns. Optional layer composition and per-layer filtering are in official docs. The codebase already uses the Option pattern.
- **Phase 2 (Business Flow):** Pure span instrumentation following existing patterns in `capture_clipboard.rs`. The spawn-boundary helper is the only novel element and follows Tokio's documented approach.
- **Phase 4 (Polish):** Rolling appender is a one-line change. Channel overflow is standard Tokio pattern.

## Confidence Assessment

| Area         | Confidence                         | Notes                                                                                                                             |
| ------------ | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Stack        | HIGH                               | Only 1 new dep (reqwest), rest is feature flags on existing deps. All versions verified on crates.io.                             |
| Features     | HIGH                               | Feature landscape grounded in existing codebase analysis + official tracing/Seq docs. Clear dependency graph.                     |
| Architecture | HIGH (pipeline), MEDIUM (SeqLayer) | Subscriber composition is well-documented. Custom CLEF Layer is based on community patterns, not official examples.               |
| Pitfalls     | HIGH                               | 10 of 13 pitfalls verified against official docs or known issues. Codebase-specific pitfalls (P2, P7) verified by reading source. |

**Overall confidence:** HIGH

### Gaps to Address

- **SeqLayer `on_event` field extraction:** The exact API for extracting span fields in a custom Layer's `on_event` callback needs validation during Phase 3 implementation. The `tracing_subscriber::registry::SpanRef` API for walking parent spans is documented but nuanced.
- **Seq CLEF trace visualization (@tr/@ps):** Blog-sourced information (MEDIUM confidence) about Seq 2024.1+ waterfall rendering. Should be validated against a running Seq instance before investing in span lifecycle tracking.
- **Dual log/tracing audit scope:** The extent of remaining `log::*` usage across crates is unknown. A grep audit should happen at Phase 1 start to determine whether the bridge configuration is sufficient or if migration is needed.
- **reqwest version alignment:** STACK.md recommends 0.13, ARCHITECTURE.md mentions 0.12. Use 0.13 (latest, verified on crates.io 2026-02-06).

## Sources

### Primary (HIGH confidence)

- [tracing-subscriber docs](https://docs.rs/tracing-subscriber/) — layer composition, JSON format, per-layer filtering, Optional layers
- [tracing-subscriber JSON format](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html) — flatten_event, with_current_span, with_span_list
- [tracing-appender docs](https://docs.rs/tracing-appender/) — NonBlocking, WorkerGuard, rolling appender
- [Seq CLEF ingestion](https://datalust.co/docs/posting-raw-events) — HTTP API, CLEF format spec, field mapping
- [Seq Docker setup](https://docs.datalust.co/docs/getting-started-with-docker) — Local development instance
- [reqwest on crates.io](https://crates.io/crates/reqwest) — Version 0.13.2 verified
- [Tokio tracing guide](https://tokio.rs/tokio/topics/tracing) — Async instrumentation, spawn boundary context

### Secondary (MEDIUM confidence)

- [Seq trace visualization blog](https://datalust.co/blog/tracing-first-look) — @tr, @ps, @st CLEF properties for waterfall views
- [Custom tracing Layer tutorial (Bryan Burgers)](https://burgers.io/custom-logging-in-rust-using-tracing) — Pattern for SeqLayer implementation
- [Structured JSON logs with tracing](https://oneuptime.com/blog/post/2026-01-25-structured-json-logs-tracing-rust/view) — Dual output patterns
- [Rust Forum: Type Hell in Tracing](https://users.rust-lang.org/t/type-hell-in-tracing-multiple-output-layers/126764) — Pitfall P1 community validation

### Tertiary (LOW confidence)

- [Seq GitHub Rust discussion](https://github.com/datalust/seq-tickets/discussions/1873) — Confirms no existing Rust CLEF crate (discussion thread, not official)

---

_Research completed: 2026-03-09_
_Ready for roadmap: yes_
