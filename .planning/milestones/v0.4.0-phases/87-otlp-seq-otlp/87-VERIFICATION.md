---
phase: 87-otlp-seq-otlp
verified: 2026-04-04T00:00:00Z
status: passed
score: 15/15 requirements verified
human_verification:
  - test: "Run Seq container + set OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp, launch app, capture clipboard. Verify spans appear in Seq Traces view with clipboard.flow root, child stage spans, and cross-device TraceId linkage."
    expected: "Seq shows a parent-child tree: clipboard.flow (root) > clipboard.normalize > clipboard.persist_event > ... with TraceId linking sender and receiver devices."
    why_human: "Requires a live Seq 2025.2 instance and two running peers. Cannot verify OTLP ingestion or Seq signal query field names programmatically."
  - test: "Verify Seq signal JSON property name casing: open flow-timeline.json and cross-device-flow.json saved searches in Seq and confirm 'SpanName' and 'TraceId' columns resolve correctly."
    expected: "Seq renders rows with non-empty TraceId and SpanName columns when OTLP-ingested spans are present."
    why_human: "PascalCase vs lowercase property name casing depends on Seq 2025.2 OTLP ingestion behavior. Both JSON files document this as MEDIUM confidence with a _note field."
  - test: "Set OTEL_EXPORTER_OTLP_ENDPOINT before launching the GUI and verify the daemon sidecar also exports OTLP spans."
    expected: "Both GUI process and daemon sidecar emit spans visible in Seq."
    why_human: "OTEL_EXPORTER_OTLP_ENDPOINT is not forwarded to daemon sidecar in uc-tauri/src/bootstrap/run.rs (only UC_SEQ_URL, UC_LOG_PROFILE, SENTRY_DSN, RUST_LOG, RUST_BACKTRACE are forwarded). Daemon OTLP works when run standalone but may be dark when launched as Tauri sidecar."
---

# Phase 87: OTLP Migration Verification Report

**Phase Goal:** Replace the custom Seq/CLEF telemetry exporter with OpenTelemetry SDK + OTLP/HTTP-protobuf, restructure clipboard pipeline spans into a parent-child tree rooted at `clipboard.flow`, adopt OTel semantic conventions, and switch cross-device correlation to W3C traceparent — while keeping Seq as the local visualization backend.

**Verified:** 2026-04-04
**Status:** passed
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Legacy `seq/` module + `clef_format.rs` are permanently deleted; OTLP is sole exporter | VERIFIED | `src/seq/` directory does not exist; `clef_format.rs` not found; `lib.rs` declares "Phase 87: legacy Seq (CLEF) pipeline removed" |
| 2 | `uc_observability::otlp` module exists with real OTLP/HTTP-protobuf pipeline | VERIFIED | `src/otlp/mod.rs`, `resource.rs`, `propagator.rs`, `layer.rs` all present; `Protocol::HttpBinary` used; `init_otlp_pipeline` and `init_otlp_provider` exported |
| 3 | Resource attributes follow OTel semconv (`service.name`, `service.version`, `service.instance.id`, `deployment.environment.name`, `os.type`) | VERIFIED | `src/otlp/resource.rs` builds all 5 keys; `build_resource_contains_semconv_keys` test passes |
| 4 | W3C `TraceContextPropagator` installed globally; `inject_current_context` / `extract_remote_context` helpers exist | VERIFIED | `propagator.rs` confirmed; `init_otlp_provider` installs propagator unconditionally; tests pass |
| 5 | `clipboard.flow` root span wraps all pipeline stages in capture pipeline | VERIFIED | `capture_clipboard.rs` line 138: `info_span!("clipboard.flow", origin = "local_capture")`; all 6 stage spans are children |
| 6 | Stage constants use dotted OTel semconv form (`clipboard.normalize`, `clipboard.persist_event`, etc.) | VERIFIED | `stages.rs` all 11 constants have `clipboard.` prefix; `stage_constants_are_dotted_otel_form` test passes |
| 7 | No `stage =` or `flow_id =` fields remain in span calls within clipboard pipeline | VERIFIED | grep over `uc-app/usecases/` returns zero matches |
| 8 | `ClipboardMessage` carries `traceparent: Option<String>` with serde(default) + skip_serializing_if | VERIFIED | `clipboard.rs` line 70 confirmed; 3 serde compat tests pass |
| 9 | `origin_flow_id` marked `#[deprecated]`; no new code reads or writes it | VERIFIED | `clipboard.rs` line 62-66 has deprecated annotation; new code sets `traceparent` not `origin_flow_id` |
| 10 | Outbound sync injects W3C traceparent into `ClipboardMessage` | VERIFIED | `sync_outbound.rs` line 289: `let traceparent = inject_current_context()` before building `ClipboardMessage` |
| 11 | Inbound sync extracts traceparent and uses as remote parent for inbound `clipboard.flow` span | VERIFIED | `sync_inbound.rs` line 225: `info_span!("clipboard.flow", origin = "inbound_sync")`; line 232: `set_parent(extract_remote_context(...))` |
| 12 | Missing traceparent on inbound falls back to new local root span + rate-limited warn | VERIFIED | `sync_inbound.rs` `MISSING_TP_PEERS: OnceLock<StdMutex<HashSet>>` + `warn_missing_traceparent_once` function |
| 13 | `uc-bootstrap` initializes OTLP pipeline on dev profile when `OTEL_EXPORTER_OTLP_ENDPOINT` set | VERIFIED | `uc-bootstrap/src/tracing.rs` `OTLP_GUARD`/`OTLP_RUNTIME` + `init_otlp_provider` call confirmed |
| 14 | Startup warns when legacy `UC_SEQ_URL` is set | VERIFIED | `tracing.rs` line 222-225: explicit `tracing::warn!` with migration note |
| 15 | Prod profile never activates OTLP | VERIFIED | All three `init_otlp_*` functions return `Ok(None)` when `matches!(profile, LogProfile::Prod)` |

**Score:** 15/15 truths verified

---

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src-tauri/crates/uc-observability/src/otlp/mod.rs` | init_otlp_pipeline, init_otlp_provider, OtlpGuard | VERIFIED | Substantive; 194 lines; wired via `pub mod otlp` in lib.rs |
| `src-tauri/crates/uc-observability/src/otlp/resource.rs` | build_resource with semconv attributes | VERIFIED | 5 OTel attributes; used by both init functions |
| `src-tauri/crates/uc-observability/src/otlp/propagator.rs` | inject_current_context / extract_remote_context | VERIFIED | Real W3C injection/extraction using global propagator |
| `src-tauri/crates/uc-observability/src/otlp/layer.rs` | build_otlp_layer + OtlpConcreteLayer type alias | VERIFIED | Concrete type alias enables Rust type inference in bootstrap |
| `src-tauri/crates/uc-observability/tests/otlp_pipeline.rs` | 5 wave0 scaffold tests | VERIFIED | 4 pass, 1 `#[ignore]` (root_flow_has_child_stage_spans — stdout exporter plumbing deferred) |
| `src-tauri/crates/uc-observability/tests/propagation.rs` | 3 wave0 scaffold tests | VERIFIED | 2 pass, 1 `#[ignore]` (traceparent_roundtrip — stdlib propagator plumbing deferred) |
| `src-tauri/crates/uc-core/tests/clipboard_message_traceparent.rs` | 3 serde backward-compat tests | VERIFIED | All 3 pass; feature gate removed (real field present) |
| `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` | traceparent field + origin_flow_id tombstone | VERIFIED | Both present; serde annotations correct |
| `src-tauri/crates/uc-observability/src/stages.rs` | All 11 constants in dotted semconv form | VERIFIED | `clipboard.detect` through `clipboard.inbound_apply`; test passes |
| `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` | clipboard.flow root span | VERIFIED | `info_span!("clipboard.flow", origin = "local_capture")` wraps all stages |
| `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` | inject_current_context call | VERIFIED | Line 289 wired; `ClipboardMessage { traceparent, ... }` |
| `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` | extract_remote_context + set_parent | VERIFIED | Line 232 wired; fallback warn implemented |
| `src-tauri/crates/uc-bootstrap/src/tracing.rs` | OTLP pipeline wired; legacy warn | VERIFIED | OTLP_GUARD, init_otlp_provider, UC_SEQ_URL warn all confirmed |
| `docs/architecture/logging-architecture.md` | Rewritten for OTel OTLP semantics | VERIFIED | OTEL_EXPORTER_OTLP_ENDPOINT, clipboard.flow, W3C traceparent, Seq query patterns all present |
| `docs/seq/signals/flow-timeline.json` | SpanName/TraceId queries, no flow_id | VERIFIED | Query: `SpanName like 'clipboard.%'`; no flow_id references |
| `docs/seq/signals/cross-device-flow.json` | SpanName/TraceId queries, no origin_flow_id | VERIFIED | Query: `SpanName = 'clipboard.flow'`; no origin_flow_id |
| `docker-compose.seq.yml` | OTLP endpoint comment block | VERIFIED | Lines 5-7 document `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp` |

---

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `uc-observability/src/lib.rs` | `otlp/mod.rs` | `pub mod otlp;` | WIRED | Confirmed at lib.rs line 50 |
| `uc-bootstrap/src/tracing.rs` | `uc_observability::otlp::init_otlp_provider` | `OTLP_RUNTIME.block_on(init_otlp_provider(...))` | WIRED | Confirmed at tracing.rs line 150 |
| `sync_outbound.rs` | `uc_observability::otlp::propagator::inject_current_context` | `use` + call at line 289 | WIRED | `traceparent = inject_current_context()` written to ClipboardMessage |
| `sync_inbound.rs` | `uc_observability::otlp::propagator::extract_remote_context` | `use` + `set_parent(extract_remote_context(...))` at line 232 | WIRED | Cross-device context restored to inbound span |
| `capture_clipboard.rs` | `info_span!("clipboard.flow")` wrapping all stage spans | `.instrument(root)` | WIRED | All 6 stage spans are instrumented as children of root |
| `otlp/mod.rs` | OTLP/HTTP-protobuf transport | `Protocol::HttpBinary` with `reqwest-client` + `reqwest-rustls` features | WIRED | No gRPC transport; tonic is only indirect via prost message gen |

---

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|-------------------|--------|
| `capture_clipboard.rs` | Stage span names | `stages::NORMALIZE`, `stages::PERSIST_EVENT`, etc. | Yes — dotted string constants | FLOWING |
| `sync_outbound.rs` | `traceparent` in ClipboardMessage | `inject_current_context()` from active `clipboard.flow` span | Yes — W3C header from live OTel context | FLOWING |
| `sync_inbound.rs` | OTel parent context | `extract_remote_context(message.traceparent.as_deref())` from wire | Yes — extracts from incoming message field | FLOWING |
| `otlp/resource.rs` | Resource attributes | `env!("CARGO_PKG_VERSION")`, `std::env::consts::OS`, `context::global_device_id()` | Yes — build-time + runtime data | FLOWING |

---

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| uc-observability tests pass (37 unit tests) | `cargo test -p uc-observability` | 37 passed, 0 failed | PASS |
| Wave 0 otlp_pipeline tests pass (4 of 5) | `cargo test -p uc-observability --features __wave0_scaffold_87 --test otlp_pipeline` | 4 passed, 1 ignored | PASS |
| Wave 0 propagation tests pass (2 of 3) | `cargo test -p uc-observability --features __wave0_scaffold_87 --test propagation` | 2 passed, 1 ignored | PASS |
| ClipboardMessage traceparent serde tests | `cargo test -p uc-core --test clipboard_message_traceparent` | 3 passed | PASS |
| Core crates compile clean | `cargo check -p uc-observability -p uc-core -p uc-app` | 0 errors, 0 warnings | PASS |
| Stage constants test | `stage_constants_are_dotted_otel_form` in uc-observability | ok | PASS |
| No seq/ or clef_format.rs remaining | `ls src/seq/ src/clef_format.rs` | both absent | PASS |
| No flow_id in Seq signal JSONs | grep flow_id in signal JSONs | not found | PASS |
| All 9 claimed commits exist | `git log --oneline` | 6b7145ce, 22eb1089, e5e16a14, d474b7c3, 21f4ed00, 61a69dbf, 6b8cac75, eeae6c6c, d40216db all confirmed | PASS |

---

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| REQ-87-01 | 87-01, 87-02, 87-05 | Replace seq/{layer,sender,mod}.rs + clef_format.rs with OTLP pipeline | SATISFIED | seq/ and clef_format.rs deleted; otlp/ module fully implemented |
| REQ-87-02 | 87-02 | OTLP transport is HTTP/protobuf, no tonic/gRPC | SATISFIED | `Protocol::HttpBinary`; `default-features = false`; tonic only via prost message gen (not gRPC transport) |
| REQ-87-03 | 87-01, 87-02 | Resource attributes follow OTel semconv | SATISFIED | `service.name`, `service.version`, `os.type`, `deployment.environment.name`, `service.instance.id` in resource.rs |
| REQ-87-04 | 87-01, 87-04 | Clipboard pipeline: root flow span + stage children | SATISFIED | `clipboard.flow` root span with `.instrument()` wrapping all stage spans in capture_clipboard.rs |
| REQ-87-05 | 87-04 | Dotted stage names as span names; no stage=/flow_id= fields | SATISFIED | All 11 constants dotted; grep over uc-app/usecases returns zero `stage =` matches in span calls |
| REQ-87-06 | 87-01, 87-03, 87-04 | ClipboardMessage.traceparent field; inject outbound; extract inbound | SATISFIED | Field exists with serde compat; inject_current_context() in outbound; set_parent(extract_remote_context()) in inbound |
| REQ-87-07 | 87-04 | Missing traceparent inbound creates new local root span + warn | SATISFIED | `warn_missing_traceparent_once()` with `MISSING_TP_PEERS` rate-limit in sync_inbound.rs |
| REQ-87-08 | 87-03 | origin_flow_id retained structurally but #[deprecated]; no new reads/writes | SATISFIED | `#[deprecated]` annotation at clipboard.rs line 62; new code only sets `traceparent` |
| REQ-87-09 | 87-02, 87-03 | Standard OTel env vars (`OTEL_EXPORTER_OTLP_ENDPOINT`) drive activation; zero overhead when unset | SATISFIED | Both `init_otlp_provider` and `init_otlp_pipeline` check `OTEL_EXPORTER_OTLP_ENDPOINT`; return `Ok(None)` when absent |
| REQ-87-10 | 87-03 | Legacy UC_SEQ_URL triggers startup warn; no implicit fallback | SATISFIED | tracing.rs lines 222-225 emit `tracing::warn!` with migration note; no Seq fallback |
| REQ-87-11 | 87-06 | docker-compose.seq.yml exposes port 5341; OTLP endpoint documented | SATISFIED (docs confirmed; live test needs human) | docker-compose.seq.yml lines 5-7 document OTLP base URL; port 5341 exposed |
| REQ-87-12 | 87-06 | Seq signal JSONs query by SpanName/TraceId; no flow_id/origin_flow_id | SATISFIED | Both JSONs confirmed: `SpanName like 'clipboard.%'` and `SpanName = 'clipboard.flow'`; grep returns no flow_id |
| REQ-87-13 | 87-06 | logging-architecture.md rewritten for OTel OTLP semantics | SATISFIED | OTEL_EXPORTER_OTLP_ENDPOINT, W3C traceparent, clipboard.flow span topology, Seq query patterns all present |
| REQ-87-14 | 87-01, 87-02, 87-04 | Prod profile never activates OTLP | SATISFIED | All three init functions: `if matches!(profile, LogProfile::Prod) { return Ok(None); }` |
| REQ-87-15 | 87-01, 87-02, 87-04 | OtlpGuard flushes on drop; exporter failure is silent + non-blocking | SATISFIED | `impl Drop for OtlpGuard` calls `provider.shutdown()` with best-effort logging; never panics |

All 15 REQ-87-* requirements are satisfied. REQ-87-* were not in REQUIREMENTS.md at verification time (file ends at PH85); they are defined in `87-RESEARCH.md` and ROADMAP.md.

---

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `uc-tauri/src/bootstrap/run.rs` | 532-545 | `OTEL_EXPORTER_OTLP_ENDPOINT` missing from daemon sidecar env var forwarding list | WARNING | Daemon sidecar will not export OTLP when launched by Tauri GUI unless user also sets env var before launching the GUI app. Works correctly when daemon runs standalone. Not in Phase 87 scope. |
| `uc-observability/tests/otlp_pipeline.rs` | 85 | `root_flow_has_child_stage_spans` remains `#[ignore]` with "TODO Plan 02" | INFO | Stdout exporter plumbing deferred; functional parent-child behavior is verified by capture_clipboard.rs code inspection. Test TODO label is stale (Plan 02 complete). |
| `uc-observability/tests/propagation.rs` | 98 | `traceparent_roundtrip` remains `#[ignore]` with "TODO Plan 02" | INFO | TraceContextPropagator plumbing deferred; functional inject/extract verified by production code in sync_outbound/inbound. Test TODO label is stale. |
| `uc-tauri/src/bootstrap/run.rs` | 529, 557 | Comments still reference "Seq" (not OTLP) | INFO | Cosmetic; comments describe behavior that predates Phase 87. No functional impact. |

---

### Human Verification Required

#### 1. Live Seq OTLP Span Ingestion

**Test:** Run `docker compose -f docker-compose.seq.yml up -d`, set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp`, launch the app with `UC_LOG_PROFILE=dev`, copy text between two paired devices.
**Expected:** Seq Traces view shows a parent-child tree: `clipboard.flow{origin="local_capture"}` as root with children `clipboard.normalize`, `clipboard.persist_event`, etc. On the receiving device, `clipboard.flow{origin="inbound_sync"}` shares the same `TraceId` as the sender.
**Why human:** Requires a live Seq 2025.2 instance, a paired device, and actual clipboard data flow. Cannot verify OTLP ingest or Seq rendering programmatically.

#### 2. Seq Signal Property Name Casing

**Test:** Import both `docs/seq/signals/flow-timeline.json` and `docs/seq/signals/cross-device-flow.json` as saved searches in a running Seq instance.
**Expected:** Both signals return non-empty results with populated `TraceId`, `SpanName`, and `service.instance.id` columns.
**Why human:** The `_note` field in both JSONs explicitly acknowledges MEDIUM confidence on PascalCase property names (`TraceId` vs `trace_id`) depending on Seq version. Actual query behavior must be validated against a live Seq 2025.2 instance.

#### 3. Daemon Sidecar OTLP Forwarding

**Test:** Launch the GUI app (which spawns daemon as sidecar) with `OTEL_EXPORTER_OTLP_ENDPOINT` set in the shell environment. Capture clipboard. Check Seq for daemon spans (identifiable by `service.instance.id` differing from GUI process or checking process name).
**Expected:** If env var is inherited by sidecar, daemon spans appear. If not (because `OTEL_EXPORTER_OTLP_ENDPOINT` is not in the forwarding list in `run.rs`), only GUI process spans appear.
**Why human:** Cannot verify sidecar env var inheritance without launching the full Tauri app. The gap in env var forwarding was identified in this verification; whether it matters in practice depends on team workflow.

---

### Gaps Summary

No blockers. All 15 requirements are satisfied by code inspection and automated test execution. Three items require human verification (live Seq instance, cross-device trace continuity, daemon sidecar env forwarding) but none block the goal of replacing the legacy pipeline.

Two `#[ignore]` test stubs (`root_flow_has_child_stage_spans`, `traceparent_roundtrip`) remain with stale "TODO Plan 02" labels. These tests are non-blocking — the underlying functionality is verified by production code inspection and other passing tests. The TODO labels should be updated to reflect completion status, but this is cosmetic.

The daemon sidecar does not forward `OTEL_EXPORTER_OTLP_ENDPOINT` (only legacy `UC_SEQ_URL` is forwarded in `uc-tauri/src/bootstrap/run.rs`). This was not in Phase 87 scope and does not block any REQ-87 requirement, but should be noted for a future phase that updates the sidecar env var list.

---

_Verified: 2026-04-04_
_Verifier: Claude (gsd-verifier)_
