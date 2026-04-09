# Phase 87: 全面迁移 OTLP, 兼容 Seq 展示, 采用 OTLP 最佳实践 - Research

**Researched:** 2026-04-04
**Domain:** OpenTelemetry Rust SDK + OTLP/HTTP-protobuf exporter, W3C trace context propagation, Seq OTLP ingestion, tracing-opentelemetry bridge
**Confidence:** HIGH (core crate versions, API surface, Seq endpoints verified against official docs.rs and datalust.co)

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**信号类型范围**

- **D-01:** 本期导出 **Traces + Logs** 两类信号。Metrics 推迟到后续 phase
  - Span 通过 `tracing-opentelemetry` bridge → OTLP traces
  - `tracing::info!/error!/warn!` 等 event → OTLP logs
  - Seq 原生支持两者,并可在 trace 详情页直接关联 logs

**迁移策略: 硬切换**

- **D-02:** 一步到位硬切换 — 直接删除 `uc-observability/src/seq/layer.rs`、`sender.rs` 以及 `clef_format.rs`
- **D-03:** 不保留 feature flag、不双写、不并行运行 — Seq 是 dev-only 工具,回归风险低,减少长期维护代价
- **D-04:** `docker-compose.seq.yml` 本期更新为启用 Seq OTLP ingestion endpoint

**Span 拓扑重构**

- **D-05:** 将现有的平铺 sibling stage span 重构为 **parent-child 层级** — 新增 root flow span,stage 作为其 child
- **D-06:** Root flow span 的 `SpanContext` 即是 OTLP 中该 flow 的 trace context

**字段命名与 semantic conventions 对齐**

- **D-07:** 全面对齐 OTel semantic conventions
  - `flow_id` 不再作为 span attribute — 语义由 OTel `trace_id` 承担
  - `stage` 作为 **span name**(e.g. `clipboard.cache_representations`),不再以 `stage=xxx` 字段形式出现
  - `device_id` 提升为 resource attribute(`service.instance.id` 或 `uc.device.id` — Claude 选择)
  - `service.name = uniclipboard-desktop` / `service.version` / `deployment.environment` 等按 semconv 填充
- **D-08:** Phase 22/23 留下的 Seq saved searches / signals 必须同步重写

**跨设备传播: W3C TraceContext**

- **D-09:** 使用 **W3C traceparent** 作为跨设备 flow 链接的标准机制
- **D-10:** `ClipboardMessage` 协议头新增 `traceparent: Option<String>` 字段
  - serde(default) + skip_serializing_if 兼容旧 peer
  - 发送端导出当前 root flow span 的 traceparent
  - 接收端用 traceparent 作为 inbound span 的 remote parent
  - 旧 peer 不带 traceparent 时:本地新 root span + warn
- **D-11:** `origin_flow_id` 在本期从**逻辑**层移除(W3C trace context 代替),协议结构体字段**保留**做向后兼容,新代码不再读写,由后续 phase 清理

**导出协议与配置**

- **D-12:** 使用 **OTLP/HTTP-protobuf** 作为 transport(复用 reqwest + rustls-tls,不引入 tonic/gRPC)
- **D-13:** 环境变量全面切换到 **OTel 标准**(`OTEL_EXPORTER_OTLP_ENDPOINT` / `OTEL_EXPORTER_OTLP_HEADERS` / `OTEL_SERVICE_NAME`)
- **D-14:** `UC_SEQ_URL` / `UC_SEQ_API_KEY` 本期**停用**,检测到仍被设置时 log warn 不做隐式 fallback

### Claude's Discretion

- OTLP exporter 的 batch size / flush interval / 超时参数(沿用默认或微调)
- `device_id` 映射为 `service.instance.id` 还是 `uc.device.id`(研究 semconv 后决定;优先 semconv 标准字段)
- Root flow span 的具体 span name(`clipboard.flow` vs `clipboard.capture_flow` 等)
- Cross-crate instrumentation 改造的具体切入点与 Span scope 传递方式
- Seq OTLP ingestion 具体 endpoint 路径
- `tracing-opentelemetry` bridge 配置与 Layer 组合顺序
- 重写后的 Seq signals JSON 结构
- 旧 `origin_flow_id` 字段 deprecated 注释方式
- 测试策略(mock OTLP collector、集成测试 Seq 可见性等)

### Deferred Ideas (OUT OF SCOPE)

- **Metrics 导出**(OTLP metrics: histograms / gauges / counters)
- **生产环境启用 OTLP**(远程 telemetry backend)— 维持 dev-only 定位
- **多跳 trace chain**(A→B→C 场景)
- **清理 ClipboardMessage.origin_flow_id 字段**(本期只墓碑化)
- **Seq 之外的 OTLP 后端对接**(Tempo / Jaeger / Grafana)
- **Tracing profile 热切换**(OBS-01)

</user_constraints>

<phase_requirements>

## Phase Requirements

Roadmap marks requirements as TBD; the following are derived from CONTEXT.md D-01..D-14 as must-haves. Planner should canonicalize these into REQUIREMENTS.md during PLAN synthesis.

| ID     | Description                                                                                                                                                                                   | Research Support                                                                                                                                                         |
| ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| REQ-87-01 | Replace `uc-observability/src/seq/{layer,sender,mod}.rs` + `clef_format.rs` with `opentelemetry` + `opentelemetry-otlp` + `tracing-opentelemetry` OTLP pipeline exporting traces & logs   | Standard Stack (opentelemetry-otlp 0.31.x, tracing-opentelemetry 0.32.x), Architecture Pattern §2 (pipeline init), Code Example §A                                      |
| REQ-87-02 | OTLP transport is HTTP/protobuf, reusing existing `reqwest` + `rustls-tls` stack (no tonic/gRPC dependency added)                                                                             | Standard Stack (feature flags `http-proto` + `reqwest-rustls`), D-12                                                                                                     |
| REQ-87-03 | Resource attributes conform to OTel semantic conventions: `service.name=uniclipboard-desktop`, `service.version` from Cargo, `service.instance.id` (or `uc.device.id`) from device_id, `deployment.environment` from build type | Semantic Conventions §3, Code Example §C                                                                                                                                 |
| REQ-87-04 | Clipboard pipeline becomes a parent-child span tree: one root flow span covers the whole pipeline; `normalize` / `persist_event` / `cache_representations` / etc. become its direct children | Architecture Pattern §4 (root flow span), Current Code Audit (all 11 stage spans currently flat), Code Example §D                                                        |
| REQ-87-05 | Stage names use dotted semconv-aligned form as span names (`clipboard.normalize`, `clipboard.cache_representations`, …); `stage = xxx` field is removed; `flow_id` attribute is removed in favor of OTel `trace_id` | Semantic Conventions §3, D-07                                                                                                                                            |
| REQ-87-06 | `ClipboardMessage` gains `traceparent: Option<String>` with `serde(default)` + `skip_serializing_if`; outbound sync injects current context; inbound sync extracts and uses as remote parent | W3C Context Propagation §5, Code Example §E (inject/extract), existing `origin_flow_id` backward-compat pattern                                                          |
| REQ-87-07 | Missing `traceparent` on inbound falls back to a new locally-rooted flow span and logs `warn` (same graceful-degradation pattern as Phase 23's `origin_flow_id` fallback)                     | Architecture Pattern §5, Pitfall #4                                                                                                                                      |
| REQ-87-08 | `origin_flow_id` protocol field is retained structurally (serde default) but no longer **read or written** by any code path; field is marked `#[deprecated]` / with a tombstone comment      | D-11, Pitfall #6                                                                                                                                                         |
| REQ-87-09 | Environment variables switch to OTel standard: `OTEL_EXPORTER_OTLP_ENDPOINT` (auto-read by SDK), `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`. Presence of `OTEL_EXPORTER_OTLP_ENDPOINT` activates the layer (zero overhead otherwise) | Environment Variables §6, D-13                                                                                                                                           |
| REQ-87-10 | Legacy `UC_SEQ_URL` / `UC_SEQ_API_KEY` are no longer consulted; if set, startup logs a warn pointing users at `OTEL_EXPORTER_OTLP_ENDPOINT`; no implicit fallback                             | D-14                                                                                                                                                                     |
| REQ-87-11 | `docker-compose.seq.yml` exposes Seq such that HTTP POST to `http://localhost:5341/ingest/otlp/v1/{traces,logs}` is reachable and accepts OTLP/HTTP-protobuf payloads                         | Seq OTLP Ingestion §2 (Seq has built-in OTLP; port 5341 serves both UI and OTLP ingestion on same binding)                                                               |
| REQ-87-12 | `docs/seq/signals/flow-timeline.json` + `docs/seq/signals/cross-device-flow.json` are rewritten to query by `trace_id` / span name / resource attributes (no `flow_id`, no `origin_flow_id`) | Seq Queries Post-Migration §7                                                                                                                                            |
| REQ-87-13 | `docs/architecture/logging-architecture.md` Seq section is rewritten: new env vars, new query examples, new span topology, span naming conventions updated                                   | Documentation section §8                                                                                                                                                 |
| REQ-87-14 | Production builds (release profile) do **NOT** activate the OTLP exporter even if `OTEL_EXPORTER_OTLP_ENDPOINT` happens to be set; Prod profile skips layer construction                      | Dev-only constraint, Phase 22 precedent                                                                                                                                  |
| REQ-87-15 | OTLP exporter background task respects `WorkerGuard`-style lifecycle: shutdown is invoked on app exit to flush pending spans; exporter failure (Seq down) is silent + non-blocking            | `OtlpGuard` pattern §4, SDK `shutdown_tracer_provider` semantics                                                                                                         |

</phase_requirements>

## Summary

Phase 87 replaces UniClipboard's custom CLEF-over-HTTP Seq exporter with the OpenTelemetry Rust SDK, using OTLP/HTTP-protobuf as transport. This is a **hard switch** — the old `SeqLayer` + `CLEFFormat` + hand-written batching `sender_loop` in `uc-observability/src/seq/` is deleted in the same phase. The new pipeline is built from three official crates (`opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry`) that compose naturally with the existing `tracing-subscriber` Registry pattern (console layer + JSON file layer stay unchanged; the Seq layer slot is replaced by an `OpenTelemetryLayer` backed by an OTLP `SpanExporter`). Seq 2025.x natively accepts OTLP/HTTP-protobuf on `/ingest/otlp/v1/{traces,logs}` on the same port as its UI, so Phase 22/23's docker-compose setup only needs a minor binding adjustment (port 5341 already serves both).

The second theme is **span topology + semantic conventions**. Today the clipboard pipeline emits 11 flat sibling spans (`detect`, `normalize`, `persist_event`, `cache_representations`, `select_policy`, `persist_entry`, `spool_blobs`, `outbound_prepare`, `outbound_send`, `inbound_decode`, `inbound_apply`) linked loosely by a `flow_id` attribute. This is not how OTel waterfalls work. Phase 87 introduces a **root flow span** (`clipboard.flow`) and restructures all 11 stages as its direct children — which means the root span's `SpanContext` is now the OTel trace identity, and `flow_id` disappears as a user field. Cross-device linking switches from the custom `origin_flow_id` header to W3C **traceparent**, using `OpenTelemetrySpanExt::context()` + `TraceContextPropagator::inject` on send and `propagator.extract` + `Span::set_parent` on receive.

The third theme is **environment variable discipline**: the project-private `UC_SEQ_URL` / `UC_SEQ_API_KEY` are replaced by the OTel standard `OTEL_EXPORTER_OTLP_ENDPOINT` / `OTEL_EXPORTER_OTLP_HEADERS` / `OTEL_SERVICE_NAME`, which the `opentelemetry-otlp` SDK reads automatically.

**Primary recommendation:** Build the new pipeline as `init_otlp_pipeline(profile: &LogProfile, device_id: Option<&str>) -> Option<(OpenTelemetryLayer, OtlpGuard)>` in a new `uc-observability/src/otlp/` module, return `None` when `OTEL_EXPORTER_OTLP_ENDPOINT` is unset (zero-overhead parity with the old Seq layer), keep the `OnceLock<OtlpGuard>` + dedicated `OnceLock<tokio::runtime::Runtime>` pattern from `uc-bootstrap/src/tracing.rs` intact, and delete `uc-observability/src/{seq,clef_format,span_fields}.rs` in the same commit that adds the new module.

## Standard Stack

### Core

| Library                | Version | Purpose                                                | Why Standard                                                                                                                                                                          |
| ---------------------- | ------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `opentelemetry`        | 0.31.x  | Core API (SpanContext, Tracer, KeyValue, propagator)   | Official OTel Rust API surface; required by every other otel-* crate                                                                                                                  |
| `opentelemetry_sdk`    | 0.31.x  | SDK side of core API: `SdkTracerProvider`, `BatchSpanProcessor`, `Resource` | The exporter pipeline lives here. Version MUST match `opentelemetry` major/minor (cross-crate breakage is the #1 support issue in this ecosystem)                    |
| `opentelemetry-otlp`   | 0.31.x  | OTLP exporter (HTTP/protobuf + gRPC variants)          | The only "official" OTLP exporter for Rust. Matches `opentelemetry` 0.31                                                                                                              |
| `tracing-opentelemetry`| 0.32.x  | Bridge: `tracing` spans/events → OTel spans/logs        | Official tokio-rs bridge. 0.32.x pairs with opentelemetry 0.31.x (dev-dep pins in the Cargo.toml confirm this)                                                                        |

**Cross-crate version pinning is mandatory.** `opentelemetry`, `opentelemetry_sdk`, and `opentelemetry-otlp` MUST share the same minor version (all three at 0.31.x). `tracing-opentelemetry` runs one minor ahead (0.32.x pairs with 0.31.x core). Mismatch causes compile errors with cryptic `Tracer` trait bounds, OR worse: compiles but the bridge silently drops spans.

### Feature Flags

For `opentelemetry-otlp` (required for Phase 87 per D-12):

```toml
opentelemetry-otlp = { version = "0.31", default-features = false, features = [
  "http-proto",      # HTTP/protobuf transport (D-12)
  "reqwest-client",  # Use reqwest instead of tonic
  "reqwest-rustls",  # rustls TLS via rustls-native-certs (reuses project's existing rustls stack)
  "trace",           # Traces signal
  "logs",            # Logs signal (D-01)
] }
```

Note: `http-proto` is default-enabled in 0.31, but listing it explicitly with `default-features = false` avoids accidentally pulling the `tonic`/`grpc-tonic` defaults, which would add ~15 transitive crates and an entirely separate gRPC stack (D-12 explicitly forbids this).

### Supporting

| Library                         | Version | Purpose                                              | When to Use                                                                       |
| ------------------------------- | ------- | ---------------------------------------------------- | --------------------------------------------------------------------------------- |
| `opentelemetry-semantic-conventions` | 0.31.x | Compile-time constants for resource/span attributes (`SERVICE_NAME`, `SERVICE_VERSION`, etc.) | Optional but recommended — eliminates typos in semconv keys                |
| `opentelemetry-appender-tracing` | 0.31.x  | Emit OTel logs from `tracing::event` outside the span bridge | Only needed if we want `tracing::info!` → OTLP **logs** (separate from span events). tracing-opentelemetry already records events as span events, so this is optional; consult §2 |

### Alternatives Considered

| Instead of                  | Could Use                  | Tradeoff                                                                                                       |
| --------------------------- | -------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `opentelemetry-otlp` (HTTP) | `opentelemetry-otlp` (gRPC via `grpc-tonic`) | gRPC is the SDK default and arguably more efficient, but D-12 explicitly rules it out to avoid adding tonic. |
| `opentelemetry-otlp`        | `opentelemetry-stdout`     | stdout exporter is useful in tests (mock-free assertion of emitted spans) — include as `[dev-dependencies]`.   |
| `tracing-opentelemetry`     | Direct SDK span API        | Would require abandoning the project-wide `tracing::*` call sites. Not viable.                                 |

**Installation:**

```toml
# uc-observability/Cargo.toml additions
opentelemetry = "0.31"
opentelemetry_sdk = { version = "0.31", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.31", default-features = false, features = [
  "http-proto", "reqwest-client", "reqwest-rustls", "trace", "logs"
] }
tracing-opentelemetry = "0.32"
opentelemetry-semantic-conventions = "0.31"

# optional
opentelemetry-appender-tracing = "0.31"  # only if we want events → OTLP logs

[dev-dependencies]
opentelemetry-stdout = "0.31"  # for test assertions
```

**Version verification:** Before locking, run from `src-tauri/`:

```bash
cd src-tauri && cargo add --dry-run opentelemetry opentelemetry_sdk opentelemetry-otlp tracing-opentelemetry -p uc-observability
cargo tree -p uc-observability | grep -E 'opentelemetry|tracing-opentelemetry'
```

Confirm all four crates resolve to the 0.31.x / 0.32.x line documented on docs.rs as of 2026-03-19 (opentelemetry-otlp 0.31.1, tracing-opentelemetry 0.32.1). HIGH confidence — verified via docs.rs.

## Architecture Patterns

### §1. Recommended Project Structure

```
src-tauri/crates/uc-observability/src/
├── lib.rs                  # Re-exports; remove build_seq_layer, SeqGuard, CLEFFormat
├── profile.rs              # UNCHANGED
├── format.rs               # UNCHANGED (FlatJsonFormat for JSON file output)
├── init.rs                 # UNCHANGED (build_console_layer, build_json_layer)
├── context.rs              # UNCHANGED (global_device_id)
├── flow.rs                 # KEEP for now (used by tests, and outbound code during migration); mark for removal later
├── stages.rs               # REWRITE: constants become dotted span names (e.g. "clipboard.normalize")
├── otlp/                   # NEW module
│   ├── mod.rs              # init_otlp_pipeline + OtlpGuard
│   ├── resource.rs         # build_resource(device_id) — semconv attributes
│   ├── propagator.rs       # install_global_propagator + inject/extract helpers
│   └── layer.rs            # wire OpenTelemetryLayer with tracer from SdkTracerProvider
└── [DELETE] seq/           # entire directory
└── [DELETE] clef_format.rs
└── [DELETE] span_fields.rs # used only by CLEFFormat
```

### §2. Pattern: OTLP Pipeline Initialization

**What:** Build an `SdkTracerProvider` with a `BatchSpanProcessor` wrapping an OTLP HTTP exporter, wrap its tracer in `tracing_opentelemetry::OpenTelemetryLayer`, and compose it into the existing Registry.

**When to use:** Exactly once, at process startup, from `uc-bootstrap/src/tracing.rs` — replacing the current Seq branch.

**Example (synthesized from official docs + docs.rs):**

```rust
// uc-observability/src/otlp/mod.rs
// Source: https://docs.rs/opentelemetry-otlp/0.31.0/opentelemetry_otlp/
//         https://docs.rs/tracing-opentelemetry/0.32.1/tracing_opentelemetry/

use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    trace::{BatchConfig, SdkTracerProvider},
    Resource,
};
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{registry::LookupSpan, Layer};

use crate::profile::LogProfile;

pub struct OtlpGuard {
    provider: SdkTracerProvider,
}

impl Drop for OtlpGuard {
    fn drop(&mut self) {
        // Flush pending spans on shutdown. SdkTracerProvider::shutdown is sync
        // but internally drains the batch processor.
        let _ = self.provider.shutdown();
    }
}

/// Build the OTLP tracing layer if `OTEL_EXPORTER_OTLP_ENDPOINT` is set AND
/// profile is dev-category. Returns None in production or when unconfigured.
pub fn init_otlp_pipeline<S>(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(impl Layer<S> + Send + Sync + 'static, OtlpGuard)>>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    // Dev-only gate (REQ-87-14)
    if matches!(profile, LogProfile::Prod) {
        return Ok(None);
    }
    // Activation gate (REQ-87-09)
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err() {
        return Ok(None);
    }

    // 1. Install W3C propagator as the global default (REQ-87-06)
    global::set_text_map_propagator(TraceContextPropagator::new());

    // 2. Build OTLP HTTP/protobuf exporter (reuses reqwest + rustls)
    //    The SDK auto-reads OTEL_EXPORTER_OTLP_ENDPOINT and _HEADERS.
    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary) // HTTP/protobuf
        .build()?;

    // 3. Build resource with semconv attributes (REQ-87-03)
    let resource = crate::otlp::resource::build_resource(device_id);

    // 4. Provider with batch processor
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer("uniclipboard-desktop");

    // 5. Bridge layer (tracing → OTel)
    let otel_layer = OpenTelemetryLayer::new(tracer).with_filter(profile.json_filter());

    Ok(Some((otel_layer, OtlpGuard { provider })))
}
```

### §3. Pattern: Resource with Semantic Conventions

**Semantic convention keys** (from `opentelemetry_semantic_conventions::resource`):

| Key                         | Value                                  | Source                                          |
| --------------------------- | -------------------------------------- | ----------------------------------------------- |
| `service.name`              | `"uniclipboard-desktop"`               | hardcoded, overridable via `OTEL_SERVICE_NAME`  |
| `service.version`           | `env!("CARGO_PKG_VERSION")`            | compile-time                                    |
| `service.instance.id`       | `device_id` (UUID)                     | `global_device_id()` from `context.rs`          |
| `deployment.environment.name` | `"development"` \| `"debug_clipboard"` | derived from `LogProfile`                       |
| `os.type`                   | `std::env::consts::OS`                 | compile-time                                    |

**Decision on `service.instance.id` vs `uc.device.id`:** OTel semconv 1.27+ officially defines `service.instance.id` as "a string uniquely identifying the instance of the service that emitted the signal". UniClipboard's `device_id` is stable per-install per-device — this is semantically exactly what `service.instance.id` is for. **Recommendation: use `service.instance.id`** and do NOT introduce a project-local `uc.device.id` attribute. This gives forward compatibility with any future OTel backend (Tempo/Jaeger/Honeycomb) which know the standard key but would not know the custom one.

```rust
// uc-observability/src/otlp/resource.rs
use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource as semconv;

pub fn build_resource(device_id: Option<&str>) -> Resource {
    let mut kvs = vec![
        KeyValue::new(semconv::SERVICE_NAME, "uniclipboard-desktop"),
        KeyValue::new(semconv::SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
        KeyValue::new(semconv::OS_TYPE, std::env::consts::OS),
        KeyValue::new(semconv::DEPLOYMENT_ENVIRONMENT_NAME,
            if cfg!(debug_assertions) { "development" } else { "production" }),
    ];
    if let Some(did) = device_id.or_else(crate::context::global_device_id) {
        kvs.push(KeyValue::new(semconv::SERVICE_INSTANCE_ID, did.to_string()));
    }
    Resource::builder().with_attributes(kvs).build()
}
```

### §4. Pattern: Root Flow Span + Stage Children (D-05, REQ-87-04)

**What:** A single outer span covers the clipboard pipeline; every existing stage span becomes its child by virtue of being created inside the outer span's async scope. tracing-opentelemetry uses the current tracing span's position in the subscriber's stack as the OTel parent — `.instrument()` is sufficient, no explicit `set_parent` calls needed.

**Current code (after Phase 20-21):**

```rust
// capture_clipboard.rs — CURRENT (flat, Phase 20 pattern)
self.normalize(...).instrument(info_span!("normalize", stage = stages::NORMALIZE)).await?;
self.persist_event(...).instrument(info_span!("persist_event", stage = stages::PERSIST_EVENT)).await?;
// ... 5 more sibling stages, all with flow_id field
```

**Target code (Phase 87):**

```rust
// capture_clipboard.rs — AFTER
use tracing::{info_span, Instrument};

pub async fn execute_with_snapshot(&self, snapshot: Snapshot) -> Result<Entry> {
    // Root flow span — represents the whole pipeline
    let root = info_span!("clipboard.flow", origin = "local_capture");
    async move {
        self.normalize(...).instrument(info_span!("clipboard.normalize")).await?;
        self.persist_event(...).instrument(info_span!("clipboard.persist_event")).await?;
        self.cache_representations(...).instrument(info_span!("clipboard.cache_representations")).await?;
        // ... all children automatically attach to the root
    }
    .instrument(root)
    .await
}
```

**Critical:** The stage spans MUST no longer carry the `stage = stages::XYZ` field (D-07) — the span **name** carries that information. Stages MUST NOT carry `flow_id` either — the OTel trace_id is the flow identity.

### §5. Pattern: W3C Trace Context Propagation (D-10, REQ-87-06)

**Sender side — `uc-app/src/usecases/clipboard/sync_outbound.rs`:**

```rust
use opentelemetry::global;
use opentelemetry::propagation::TextMapPropagator;
use std::collections::HashMap;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

// Inside the outbound send path, while still inside the root flow span:
let mut carrier = HashMap::<String, String>::new();
let current_ctx = Span::current().context(); // tracing → OTel context bridge
global::get_text_map_propagator(|propagator| {
    propagator.inject_context(&current_ctx, &mut carrier);
});
let traceparent = carrier.remove("traceparent"); // canonical W3C header name
message.traceparent = traceparent; // Option<String> field
```

**Receiver side — `uc-app/src/usecases/clipboard/sync_inbound.rs`:**

```rust
use opentelemetry::global;
use tracing::info_span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

let mut carrier = HashMap::<String, String>::new();
if let Some(tp) = &message.traceparent {
    carrier.insert("traceparent".to_string(), tp.clone());
}

let remote_ctx = global::get_text_map_propagator(|propagator| {
    propagator.extract(&carrier)
});

let inbound_span = info_span!("clipboard.flow", origin = "inbound_sync");
// Attach remote context as parent of this new span
inbound_span.set_parent(remote_ctx);

async move {
    // decode / apply inside child spans, which now become part of the sender's trace
}
.instrument(inbound_span)
.await
```

**Fallback path (REQ-87-07):** If `message.traceparent` is `None` or `extract` returns an empty context (`SpanContext::is_valid()` is false), the `set_parent` call is a no-op and `inbound_span` starts a fresh trace — exactly the behavior we want for legacy peers. Emit a `warn!` once per legacy peer (rate-limit via a `HashSet<peer_id>` or just log once at debug level).

### §6. Environment Variables (REQ-87-09, D-13)

| Variable                       | Read by                                | Default                               | Notes                                                                |
| ------------------------------ | -------------------------------------- | ------------------------------------- | -------------------------------------------------------------------- |
| `OTEL_EXPORTER_OTLP_ENDPOINT`  | `SpanExporter::builder().with_http()` (SDK auto-read) | Unset → disables OTLP exporter (zero overhead) | For Seq: `http://localhost:5341/ingest/otlp/v1` (base URL; SDK appends `/traces` and `/logs`) |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | SDK auto-read (overrides base)   | —                                     | Use only if traces and logs need different endpoints                 |
| `OTEL_EXPORTER_OTLP_HEADERS`   | SDK auto-read                          | —                                     | Example: `X-Seq-ApiKey=abcd1234`                                     |
| `OTEL_EXPORTER_OTLP_PROTOCOL`  | SDK auto-read                          | `http/protobuf` (per our feature)     | Could be `grpc` but we don't build with that feature                 |
| `OTEL_SERVICE_NAME`            | SDK auto-read; overrides `Resource`    | `"uniclipboard-desktop"` hardcoded    | —                                                                    |
| `OTEL_RESOURCE_ATTRIBUTES`     | SDK auto-read; merges with `Resource`  | —                                     | E.g. `deployment.environment=dev,foo=bar`                            |
| `OTEL_EXPORTER_OTLP_TIMEOUT`   | SDK auto-read                          | 10s                                   | —                                                                    |
| `OTEL_TRACES_SAMPLER`          | SDK auto-read                          | `parentbased_always_on`               | Leave default                                                        |

**CRUCIAL Seq endpoint detail:** Seq's own docs say the HTTP/protobuf endpoint is `/ingest/otlp/v1/traces` (full path). The OTLP SDK's convention, however, is that `OTEL_EXPORTER_OTLP_ENDPOINT` is a **base URL** to which it appends `/v1/traces` and `/v1/logs` per the OTLP spec. Two configurations work and the planner should pick one:

1. **Recommended:** Set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp` (SDK appends `/v1/traces` + `/v1/logs` → `http://localhost:5341/ingest/otlp/v1/traces`). Matches Seq exactly.
2. Set per-signal overrides: `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=http://localhost:5341/ingest/otlp/v1/traces` and same for logs.

Option 1 is shorter and is what Seq's own docs show. Document this in `docs/architecture/logging-architecture.md` explicitly — it is a common trap.

### §7. Seq Query Migration (REQ-87-12)

Phase 22/23's `docs/seq/signals/*.json` query by `@Properties.flow_id` and `@Properties.origin_flow_id`. After migration, Seq receives OTLP spans which Seq renders as tracing events where:

- `trace_id` → top-level property on each span/event
- `span_id` / `parent_span_id` → top-level property
- `span_name` → the semconv span name (`clipboard.flow`, `clipboard.normalize`, …)
- Resource attributes become properties prefixed with the resource name (e.g. `service.name`, `service.instance.id`)
- Span attributes become regular properties at the event level

Rewrite queries:

| Old (Phase 22/23)                          | New (Phase 87)                                                                 |
| ------------------------------------------ | ------------------------------------------------------------------------------ |
| `Has(flow_id)`                             | `Has(TraceId)` or `SpanName like 'clipboard.%'`                                |
| `flow_id = 'abc'`                          | `TraceId = 'abc'`                                                              |
| `stage = 'normalize'`                      | `SpanName = 'clipboard.normalize'`                                             |
| `Has(flow_id) or Has(origin_flow_id)`      | `Has(TraceId)` (cross-device uses same trace_id, nothing further needed)      |
| `device_id = 'x'`                          | `"service.instance.id" = 'x'` (resource attribute)                             |

**MEDIUM confidence** on exact Seq property names for OTel-ingested data — planner SHOULD verify by running Seq locally once during implementation and copying the actual field names from the Seq event detail panel. Seq's own UI shows the mapping unambiguously and this is a 5-minute check.

### §8. Documentation Section Rewrite (REQ-87-13)

`docs/architecture/logging-architecture.md` currently has:
- "Seq Integration (Local Visualization)" section citing `UC_SEQ_URL`, `/ingest/clef`, CLEF format
- "Cross-Device Tracing" section citing `origin_flow_id`

Both need full rewrite. Replacement shape:
1. "OpenTelemetry OTLP Integration (Local Visualization)" — new env vars, semconv resource attrs, span naming, activation model, endpoint construction rules
2. "Distributed Tracing with W3C Trace Context" — traceparent header, inject/extract flow, legacy peer fallback
3. Cross-link to `docs/seq/signals/*.json` new queries

### Anti-Patterns to Avoid

- **Building a custom propagator.** Use `TraceContextPropagator::new()` — it's the W3C standard and Seq supports it natively. Custom propagators exist for Jaeger/B3 and are irrelevant here.
- **Holding `.entered()` across `.await`** (project-wide rule from tracing-best-practices skill) — doubly important here because each stage is async.
- **Calling `set_parent` before the span is entered** — per docs, `set_parent` works on any tracing span regardless of entered state, BUT the parent context must be read BEFORE entering the child span (otherwise you capture the child's own context).
- **Forgetting to install the global propagator** — without `global::set_text_map_propagator(TraceContextPropagator::new())`, `get_text_map_propagator` returns a no-op propagator and inject/extract silently produce empty carriers. This is a famous, silent failure mode.
- **Mixing OTel SDK 0.31 types with 0.30 types** — cargo will compile if both versions end up in the graph (duplicate crates), but spans from one SDK are invisible to the other. Check `cargo tree -d | grep opentelemetry` for duplicates before finalizing.
- **Putting OTel SDK init inside tokio::spawn** — `global::set_text_map_propagator` must run BEFORE any inject/extract call, anywhere in the process. Init synchronously in `uc-bootstrap/src/tracing.rs` on the main thread.
- **Leaving `stage = xxx` fields on stage spans** — cost zero in OTel but violates D-07 and causes dual-field redundancy in Seq.

## Don't Hand-Roll

| Problem                                         | Don't Build                                       | Use Instead                                             | Why                                                                                                     |
| ----------------------------------------------- | ------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| HTTP batching of CLEF events                    | The current `sender_loop` in `seq/sender.rs`      | `opentelemetry_sdk::trace::BatchSpanProcessor`          | SDK handles backpressure, retry, drop-on-overflow, per-signal timeouts, graceful shutdown               |
| CLEF format encoding                            | `clef_format.rs` + `SeqLayer` event visitor       | `opentelemetry-otlp` + `OpenTelemetryLayer`             | OTLP/protobuf is the industry-standard wire format; avoids CLEF's Seq-specific lock-in                 |
| Cross-device correlation header                 | Custom `origin_flow_id` field                      | W3C `traceparent` header + `TraceContextPropagator`     | W3C trace context works with every OTel backend; multi-hop comes for free                              |
| Flow correlation IDs                            | `FlowId` (UUID v7) as span field                   | OTel `trace_id` (auto-generated per root span)          | Every OTel backend already indexes trace_id; duplicate IDs are pure overhead                            |
| Device ID field injection per event             | `SeqLayer::on_event` injecting `device_id`         | `Resource` attribute `service.instance.id` (per-provider) | Resource is sent once per OTLP export call, not per span — much cheaper and semconv-compliant          |
| Exporter lifecycle / flush-on-shutdown          | `SeqGuard` with `std::thread::spawn` + `block_on`  | `SdkTracerProvider::shutdown()` in `Drop`               | SDK's shutdown drains batch and is sync-safe; no thread-in-runtime tricks                               |
| Graceful degradation when backend is down       | Custom silent-drop error handling in `flush_batch` | BatchSpanProcessor default (drops on full queue, logs)  | Already best-in-class; exposes metrics to its own `ErrorHandler` hook for future observability-of-obs  |
| Context extraction from HTTP-like headers       | Custom `parse_traceparent` function                | `propagator.extract(&carrier)` where carrier is `&HashMap<String,String>` | Handles malformed headers, version prefixes, flags, and spec edge cases for free             |

**Key insight:** The entire `uc-observability/src/seq/` directory (layer.rs + sender.rs + mod.rs, ~450 LOC) plus `clef_format.rs` (~390 LOC) and `span_fields.rs` (~50 LOC) is replaced by ~150 LOC of OTel SDK wiring — all the custom infrastructure was solving problems the SDK solves better. This is the kind of simplification hard switch (D-02) makes possible.

## Runtime State Inventory

Phase 87 is a telemetry-layer migration. The relevant "runtime state" category is **developer environments** (engineers currently running Seq locally with Phase 22/23 setup).

| Category            | Items Found                                                                                                                                                                      | Action Required                                                                                                          |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Stored data         | None — Seq data in the `seq-data` Docker volume is dev-only and disposable. Old CLEF events and new OTLP spans can co-exist in the same Seq instance, no migration needed. | None (document in migration notes: developers can `docker compose down -v` to clear, but not required)                    |
| Live service config | Developer shell env currently exports `UC_SEQ_URL` / `UC_SEQ_API_KEY`. These are set in personal dotfiles or `.envrc`, NOT in the repo.                                          | REQ-87-10: startup logs a warn when legacy vars are set, telling developers to switch to `OTEL_EXPORTER_OTLP_ENDPOINT`   |
| OS-registered state | None. No systemd/launchd units register Seq-specific state.                                                                                                                      | None                                                                                                                     |
| Secrets/env vars    | `UC_SEQ_API_KEY` (optional). Not a real secret in dev (local Seq has no auth). No SOPS/keyring involvement.                                                                      | Code stops reading these vars; developers remove from personal dotfiles                                                  |
| Build artifacts     | None — observability is statically linked into the binary.                                                                                                                       | None. Rebuild gives full migration.                                                                                      |

**Nothing found in most categories** — verified by `git grep UC_SEQ_URL` showing only source + test files, and no CI/release artifact carries baked-in Seq URLs.

## Common Pitfalls

### Pitfall 1: Cross-crate version drift

**What goes wrong:** `cargo add opentelemetry` pulls 0.31.x, but a transitive dep pulls 0.30.x, and both end up in the compile graph. Compiles cleanly. Spans emitted via 0.30 types are invisible to the 0.31 provider — Seq shows zero traces.
**Why it happens:** opentelemetry-rust does not follow semver for 0.x bumps between minor versions. Any patch bump to a sub-crate can break compatibility.
**How to avoid:** After every `cargo add`/`cargo update`, run `cargo tree -p uc-observability -d | grep opentelemetry` and fail if any duplicates appear. Pin all four crates to exact minor version in `Cargo.toml`.
**Warning signs:** Compile succeeds; exporter builds; app runs; NO spans in Seq; no error messages.

### Pitfall 2: Forgetting to install the global propagator

**What goes wrong:** `global::get_text_map_propagator(|p| p.inject_context(...))` returns a no-op propagator by default. `carrier` remains empty. `traceparent` field is always `None`. Cross-device linking silently fails.
**Why it happens:** Unlike tracers (which are installed via `global::set_tracer_provider` which SdkTracerProvider::builder() doesn't auto-call), propagators must be manually installed.
**How to avoid:** Put `global::set_text_map_propagator(TraceContextPropagator::new());` as the FIRST line of `init_otlp_pipeline`, BEFORE the activation check, so that even if `OTEL_EXPORTER_OTLP_ENDPOINT` is unset, cross-device headers still work (for completeness of the protocol field; they'll just not be correlated in Seq without the exporter).
**Warning signs:** `ClipboardMessage.traceparent` is always `None` in outbound sync; inbound fallback warn fires for every message, even from peers running the new version.

### Pitfall 3: `SdkTracerProvider::shutdown` in `Drop` during Tokio runtime teardown

**What goes wrong:** `Drop` for `OtlpGuard` calls `provider.shutdown()`, which internally polls the batch processor's async flush. If the tokio runtime is already being destroyed (late in Tauri shutdown), shutdown hangs or panics with "runtime not available".
**Why it happens:** OTel SDK's sync-looking `shutdown()` is really a wrapper that uses the current runtime.
**How to avoid:** Use the same pattern as the current `SeqGuard` + `SEQ_RUNTIME` `OnceLock` in `uc-bootstrap/src/tracing.rs`: keep a dedicated tokio runtime alive in a separate `OTLP_RUNTIME: OnceLock<Runtime>`. Call `shutdown` from a thread that uses that runtime's handle, with a 5-second timeout. This is literally the same pattern Phase 22 established — keep it.
**Warning signs:** App hangs on exit for exactly 10s (SDK default timeout); or panic "there is no reactor running".

### Pitfall 4: Missing traceparent on receive → spurious warns

**What goes wrong:** Every message from a legacy peer triggers a warn-level log. Development logs are flooded.
**Why it happens:** Graceful degradation path logs unconditionally.
**How to avoid:** Track "peer X has sent N messages without traceparent" in a per-peer counter; log warn only once per peer per session. Or log at debug level, not warn.
**Warning signs:** `grep "missing traceparent" uniclipboard.json | wc -l` returns thousands during a normal sync session.

### Pitfall 5: Resource attributes set after provider construction are ignored

**What goes wrong:** Developer builds `SdkTracerProvider`, then later calls `provider.set_resource(...)`. That method doesn't exist / doesn't affect already-created tracers.
**Why it happens:** In SDK 0.31, `Resource` is passed at builder time and is immutable thereafter.
**How to avoid:** Build `Resource` fully before `SdkTracerProvider::builder()`. `device_id` must be resolved from `resolve_device_id_for_logging` BEFORE `init_otlp_pipeline` is called — which is the existing bootstrap order (line 91 of `uc-bootstrap/src/tracing.rs`). Good.
**Warning signs:** First spans emitted have no `service.instance.id`; after rebuild with device_id present, still no `service.instance.id`.

### Pitfall 6: `origin_flow_id` tombstone field is accidentally reintroduced

**What goes wrong:** A later refactor (or cherry-pick) wires `origin_flow_id` back into code because the struct field is still present.
**Why it happens:** D-11 keeps the field for wire compat but forbids reading/writing it. Easy to forget.
**How to avoid:** Annotate the field with `#[deprecated(note = "Phase 87: replaced by W3C traceparent. Do not read or write. Scheduled for removal in a future cleanup phase.")]` AND add a doc-comment tombstone. `cargo clippy` surfaces any new reads. Add a grep-based CI check in a follow-up phase.
**Warning signs:** A new PR touches `origin_flow_id` without a CODEOWNERS-level review.

### Pitfall 7: Seq endpoint base URL vs full URL confusion

**What goes wrong:** Developer sets `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp/v1/traces` (full traces path), SDK appends `/v1/traces` → `http://localhost:5341/ingest/otlp/v1/traces/v1/traces` → 404.
**Why it happens:** Seq docs show the full path; OTLP spec says endpoint is a base URL.
**How to avoid:** Document the canonical value `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp` **explicitly** in `docs/architecture/logging-architecture.md` and in `docker-compose.seq.yml` as a comment. Add an `OTEL_EXPORTER_OTLP_ENDPOINT` example in `.env.example` if one exists.
**Warning signs:** Startup succeeds, OTLP exporter logs "export failed: 404 Not Found" at debug level, no traces in Seq.

## Code Examples

### §A. Full OTLP pipeline init (verified pattern)

```rust
// uc-observability/src/otlp/mod.rs
// Source: https://docs.rs/opentelemetry-otlp/0.31.0/opentelemetry_otlp/
// Source: https://docs.rs/tracing-opentelemetry/0.32.1/tracing_opentelemetry/

use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
};
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{registry::LookupSpan, Layer};

pub fn init_otlp_pipeline<S>(
    profile: &crate::profile::LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(impl Layer<S> + Send + Sync + 'static, OtlpGuard)>>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    global::set_text_map_propagator(TraceContextPropagator::new());

    if matches!(profile, crate::profile::LogProfile::Prod) {
        return Ok(None);
    }
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err() {
        return Ok(None);
    }

    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()?;

    let resource = crate::otlp::resource::build_resource(device_id);

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer("uc-observability");
    let layer = OpenTelemetryLayer::new(tracer).with_filter(profile.json_filter());

    Ok(Some((layer, OtlpGuard { provider })))
}

pub struct OtlpGuard { provider: SdkTracerProvider }
impl Drop for OtlpGuard {
    fn drop(&mut self) {
        let _ = self.provider.shutdown();
    }
}
```

### §B. Wiring into `uc-bootstrap/src/tracing.rs` (diff shape)

```rust
// Replace the Seq block (lines 132-164 of current file):
static OTLP_GUARD: OnceLock<uc_observability::otlp::OtlpGuard> = OnceLock::new();
static OTLP_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

let otlp_layer = if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
    && !matches!(profile, LogProfile::Prod)
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_all().build()?;
    let result = rt.block_on(async {
        uc_observability::otlp::init_otlp_pipeline(&profile, device_id.as_deref())
    })?;
    match result {
        Some((layer, guard)) => {
            let _ = OTLP_GUARD.set(guard);
            let _ = OTLP_RUNTIME.set(rt);
            Some(layer)
        }
        None => None,
    }
} else {
    if std::env::var("UC_SEQ_URL").is_ok() {
        tracing::warn!("UC_SEQ_URL is set but legacy Seq ingestion was removed in Phase 87. \
            Migrate to OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp");
    }
    None
};

tracing_subscriber::registry()
    .with(sentry_layer)
    .with(console_layer)
    .with(json_layer)
    .with(otlp_layer)  // was .with(seq_layer)
    .try_init()?;
```

### §C. Resource builder with semconv

(See §3 above — full file listed.)

### §D. Root flow span + child stages (capture_clipboard.rs shape)

```rust
// uc-app/src/usecases/internal/capture_clipboard.rs
use tracing::{info_span, Instrument};

pub async fn execute_with_snapshot(&self, snapshot: Snapshot) -> anyhow::Result<Entry> {
    let root = info_span!("clipboard.flow",
        origin = "local_capture",
        // No more flow_id / stage fields — they're implicit via trace_id + span name
    );
    async move {
        let normalized = self.normalize(snapshot)
            .instrument(info_span!("clipboard.normalize"))
            .await?;
        let event_id = self.persist_event(&normalized)
            .instrument(info_span!("clipboard.persist_event"))
            .await?;
        let reps = self.cache_representations(event_id, &normalized)
            .instrument(info_span!("clipboard.cache_representations"))
            .await?;
        // … rest of stages, all children of `root`
        Ok(entry)
    }
    .instrument(root)
    .await
}
```

### §E. Outbound inject / inbound extract

(See §5 above — full pattern listed.)

### §F. Test: mock OTLP via stdout exporter

```rust
// uc-observability/src/otlp/tests.rs
// Using opentelemetry-stdout for assertion-friendly testing without a live Seq
#[cfg(test)]
mod tests {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing::info_span;
    use tracing_opentelemetry::OpenTelemetryLayer;
    use tracing_subscriber::prelude::*;

    #[test]
    fn root_flow_has_child_stage_spans() {
        let exporter = opentelemetry_stdout::SpanExporter::default(); // captures to stdout
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter)
            .build();
        let tracer = provider.tracer("test");
        let layer = OpenTelemetryLayer::new(tracer);

        tracing_subscriber::registry().with(layer).init();

        let root = info_span!("clipboard.flow");
        let _e = root.enter();
        {
            let child = info_span!("clipboard.normalize");
            let _ce = child.enter();
        }
        // With simple_exporter, spans flush on drop.
        // Assertion: parse stdout capture and verify parent_span_id relationship.
    }
}
```

## State of the Art

| Old Approach (pre-Phase 87)                                        | Current Approach (Phase 87)                                   | When Changed                    | Impact                                                                    |
| ------------------------------------------------------------------ | ------------------------------------------------------------- | ------------------------------- | ------------------------------------------------------------------------- |
| Custom CLEF over `/ingest/clef`                                    | OTLP/HTTP-protobuf over `/ingest/otlp/v1/{traces,logs}`       | This phase                      | Industry-standard wire format; future-proof for any OTLP backend         |
| Hand-written `sender_loop` with mpsc + manual batching              | `BatchSpanProcessor` in opentelemetry_sdk                     | This phase                      | Drops 400+ LOC of infrastructure                                          |
| `flow_id` as UUID v7 span attribute                                 | OTel `trace_id` (128-bit, auto-generated)                     | This phase                      | trace_id indexed for free in every backend                                |
| `stage = xxx` span field                                            | span **name** (e.g. `clipboard.normalize`)                    | This phase                      | Enables OTel span name filters, waterfalls                                |
| Flat sibling spans                                                  | Parent-child tree rooted at `clipboard.flow`                  | This phase                      | True waterfall visualization in Seq/Tempo/Jaeger                          |
| `origin_flow_id` header field (project-specific)                    | W3C `traceparent` header                                      | This phase                      | Standard, multi-hop-ready, works with every tracer                        |
| `device_id` as per-event field (CLEF layer injection)               | `service.instance.id` as Resource attribute                   | This phase                      | Sent once per export; semconv-compliant                                   |
| `UC_SEQ_URL` / `UC_SEQ_API_KEY` (project-specific env)              | `OTEL_EXPORTER_OTLP_ENDPOINT` / `_HEADERS` (OTel std env)      | This phase                      | Developers reuse OTel muscle memory from other projects                   |

**Deprecated / outdated:**

- `CLEFFormat`, `SeqLayer`, `sender_loop`, `SeqGuard` — deleted (D-02).
- `FlowId::generate()` — no longer called in the new pipeline; the type and constructor remain in `flow.rs` temporarily because removing them would churn tests. Schedule full deletion for a post-Phase-87 cleanup.
- `stages::NORMALIZE` etc. constants — either repurposed as span-name constants (`pub const NORMALIZE: &str = "clipboard.normalize";`) or deleted in favor of inlined string literals. Claude's discretion; repurposing is slightly cleaner and lets grep find all call sites.
- `docs/seq/signals/*.json` CLEF query shapes — rewritten.

## Open Questions

1. **Does `opentelemetry-appender-tracing` duplicate what `tracing-opentelemetry` already does?**
   - What we know: `tracing-opentelemetry::OpenTelemetryLayer` records tracing events as OTel span events (attached to the span they occurred in), which Seq renders as log-like entries inside a trace. `opentelemetry-appender-tracing` instead routes events as OTel **logs** (separate signal).
   - What's unclear: Whether Seq UI differentiates span events vs logs strongly enough that we'd want both, or whether span events alone suffice. D-01 says "traces + logs" but doesn't mandate two separate SDK code paths.
   - Recommendation: Start with tracing-opentelemetry only (simpler, one layer); if Seq UX feels cramped, add `opentelemetry-appender-tracing` in a follow-up. Do NOT add it in Phase 87 unless planner confirms Seq's log/trace separation warrants it.

2. **Should stage constants in `stages.rs` be repurposed or deleted?**
   - What we know: Current values are bare words (`"normalize"`); new values need dotted form (`"clipboard.normalize"`).
   - What's unclear: Whether other crates import these constants (`grep` shows they do — yes, in capture_clipboard.rs + sync_inbound.rs + sync_outbound.rs). Deleting forces inline strings everywhere; repurposing updates one file.
   - Recommendation: Repurpose in place — `pub const NORMALIZE: &str = "clipboard.normalize";`. Minimal churn, keeps typo protection.

3. **What happens to `FlowId` (`flow.rs`) and its UUID v7 generator?**
   - What we know: Nothing in the new pipeline calls `FlowId::generate()`. The type is `pub`.
   - What's unclear: Whether any test/fixture depends on it.
   - Recommendation: Leave the file in place with a deprecation comment; schedule removal as a Quick Task after Phase 87 merges and is verified stable.

4. **How does `OpenTelemetryLayer` interact with a per-layer `EnvFilter`?**
   - What we know: `tracing-opentelemetry` docs show `.with_filter(filter)` is supported.
   - What's unclear: Whether the filter applies to the span-recording side (skipping span creation at the OTel layer only) or whether it prunes span events only. A too-loose filter could accidentally OTLP-export spans that the JSON file layer excludes, making the two outputs inconsistent.
   - Recommendation: Use `profile.json_filter()` (same as JSON file layer) so both outputs see the same span set. This is the existing Phase 22 pattern.

5. **Does Seq 2025.2 (pinned in docker-compose.seq.yml) support OTLP natively?**
   - What we know: Seq 2024.1+ (per datalust.co/docs/tracing-from-opentelemetry-sdks) supports OTLP natively on `/ingest/otlp/v1/`. 2025.2 definitely includes it.
   - Recommendation: Keep the 2025.2 tag. Confirm once during implementation by curling `http://localhost:5341/ingest/otlp/v1/traces` — a GET should return 405 Method Not Allowed (endpoint exists) vs 404 (endpoint doesn't exist).

## Environment Availability

| Dependency                    | Required By                         | Available                  | Version          | Fallback                                                   |
| ----------------------------- | ----------------------------------- | -------------------------- | ---------------- | ---------------------------------------------------------- |
| Rust (cargo)                  | Building uc-observability           | ✓ (project requirement)    | per project MSRV | —                                                          |
| `cd src-tauri && cargo tree`  | Version conflict check (Pitfall 1)  | ✓                          | —                | —                                                          |
| Docker / docker compose       | Running Seq locally for manual validation | ✓ (devs run it for Phase 22) | —             | skip manual Seq validation; rely on `opentelemetry-stdout` tests |
| Seq 2025.2 image (datalust/seq:2025.2) | Manual cross-device validation | pullable on demand | 2025.2 | 2024.1+ also works per Seq docs                            |
| Internet access for `cargo add` of new otel crates | Phase setup | ✓ | — | vendor crates offline if CI has no registry access |

**No missing dependencies with no fallback.** All external requirements are already in place (project devs run Seq via docker-compose today).

## Validation Architecture

### Test Framework

| Property           | Value                                                                            |
| ------------------ | -------------------------------------------------------------------------------- |
| Framework          | `cargo test` (Rust built-in) + `tokio::test` for async                           |
| Config file        | `src-tauri/Cargo.toml` workspace; per-crate `Cargo.toml`                         |
| Quick run command  | `cd src-tauri && cargo test -p uc-observability`                                 |
| Full suite command | `cd src-tauri && cargo test`                                                     |

### Phase Requirements → Test Map

| Req ID   | Behavior                                                             | Test Type   | Automated Command                                                                                     | File Exists?                                             |
| -------- | -------------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| REQ-87-01 | `init_otlp_pipeline` returns `Some(layer, guard)` when env var set and dev profile | unit        | `cd src-tauri && cargo test -p uc-observability otlp::tests::init_returns_layer_when_configured`     | ❌ Wave 0 — create `uc-observability/src/otlp/mod.rs` with `#[cfg(test)] mod tests` |
| REQ-87-01 | `init_otlp_pipeline` returns `None` when env var unset                | unit        | `cargo test -p uc-observability otlp::tests::init_returns_none_when_env_missing`                      | ❌ Wave 0                                                 |
| REQ-87-02 | No tonic/gRPC crates in dep graph                                    | build assertion | `cargo tree -p uc-observability 2>&1 \| grep -q tonic && exit 1 || exit 0`                        | ❌ Wave 0 — add as xtask or CI step                       |
| REQ-87-03 | `build_resource` emits `service.name`, `service.version`, `service.instance.id` | unit        | `cargo test -p uc-observability otlp::resource::tests::resource_contains_semconv_keys`                | ❌ Wave 0                                                 |
| REQ-87-04 | `clipboard.flow` root span becomes parent of `clipboard.normalize` child | integration | `cargo test -p uc-observability otlp::tests::root_flow_has_child_stage_spans` (uses `opentelemetry-stdout`) | ❌ Wave 0 — pattern in Code Example §F             |
| REQ-87-05 | Stage spans have no `stage` or `flow_id` attributes                   | grep assertion | `! grep -r 'stage\s*=\s*stages::' src-tauri/crates/uc-app/ src-tauri/crates/uc-tauri/`             | manual review + rg CI check                              |
| REQ-87-06 | traceparent roundtrip: inject from span → extract → parent matches original | unit        | `cargo test -p uc-observability otlp::propagator::tests::traceparent_roundtrip`                       | ❌ Wave 0                                                 |
| REQ-87-06 | `ClipboardMessage` serializes `traceparent` field with serde(default) compat | unit        | `cd src-tauri && cargo test -p uc-core network::protocol::clipboard::tests::traceparent_serde_compat` | ❌ Wave 0 — extend existing `clipboard.rs` test module    |
| REQ-87-07 | Inbound without traceparent creates new root span and logs warn       | integration | `cargo test -p uc-app usecases::clipboard::sync_inbound::tests::missing_traceparent_creates_new_root` | ❌ Wave 0                                                 |
| REQ-87-08 | `origin_flow_id` has `#[deprecated]` annotation                       | compile warning | `cargo build -p uc-core 2>&1 \| grep -q 'origin_flow_id.*deprecated'` (or rustdoc check)          | ❌ Wave 0                                                 |
| REQ-87-09 | SDK reads `OTEL_SERVICE_NAME` override                                | unit        | `cargo test -p uc-observability otlp::resource::tests::env_overrides_service_name` (sets env then calls build) | ❌ Wave 0                                   |
| REQ-87-10 | Legacy `UC_SEQ_URL` triggers deprecation warn on startup              | unit        | `cargo test -p uc-bootstrap tracing::tests::legacy_seq_url_logs_warn` (capture tracing events)        | ❌ Wave 0                                                 |
| REQ-87-11 | `docker-compose.seq.yml` exposes port 5341 and documents OTLP endpoint path | manual      | `docker compose -f docker-compose.seq.yml up -d && curl -s -o /dev/null -w '%{http_code}' -X POST http://localhost:5341/ingest/otlp/v1/traces` expects 415 or 400 (wrong content-type) not 404 | manual-only — developer smoke test, documented in phase plan |
| REQ-87-12 | `docs/seq/signals/flow-timeline.json` has no `flow_id` references     | grep assertion | `! grep -q flow_id docs/seq/signals/flow-timeline.json docs/seq/signals/cross-device-flow.json`    | CI check                                                 |
| REQ-87-13 | `docs/architecture/logging-architecture.md` references `OTEL_EXPORTER_OTLP_ENDPOINT` and no longer references `UC_SEQ_URL` | grep assertion | `grep -q OTEL_EXPORTER_OTLP_ENDPOINT docs/architecture/logging-architecture.md && ! grep -q UC_SEQ_URL docs/architecture/logging-architecture.md` | CI check |
| REQ-87-14 | Prod profile never activates OTLP even with env var set               | unit        | `cargo test -p uc-observability otlp::tests::prod_profile_never_activates`                            | ❌ Wave 0                                                 |
| REQ-87-15 | `OtlpGuard::drop` invokes `SdkTracerProvider::shutdown`               | integration | `cargo test -p uc-observability otlp::tests::guard_drop_flushes` (uses `opentelemetry-stdout` + captured output) | ❌ Wave 0                                      |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-observability -p uc-core` (fast — ~5s)
- **Per wave merge:** `cd src-tauri && cargo test` (full workspace ~90s; excludes integration_tests feature flag by default)
- **Phase gate:** Full suite green + manual Seq smoke test (see REQ-87-11) + visual waterfall verification in Seq UI (capture clipboard on Peer A, verify complete trace appears on both Peer A and Peer B signals)

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-observability/src/otlp/mod.rs` — with `#[cfg(test)] mod tests` covering REQ-87-{01,04,14,15}
- [ ] `src-tauri/crates/uc-observability/src/otlp/resource.rs` — with tests covering REQ-87-{03,09}
- [ ] `src-tauri/crates/uc-observability/src/otlp/propagator.rs` — with tests covering REQ-87-06 roundtrip
- [ ] New test in `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` `mod tests` — `traceparent_serde_compat` covering REQ-87-06
- [ ] New test in `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` `mod tests` — `missing_traceparent_creates_new_root` covering REQ-87-07
- [ ] New test in `src-tauri/crates/uc-bootstrap/src/tracing.rs` `mod tests` — `legacy_seq_url_logs_warn` covering REQ-87-10
- [ ] Add `opentelemetry-stdout = "0.31"` to `uc-observability/Cargo.toml` `[dev-dependencies]` — required for span assertion tests
- [ ] No framework install needed (cargo built-in)

## Sources

### Primary (HIGH confidence)

- [docs.rs/opentelemetry-otlp/0.31.0](https://docs.rs/opentelemetry-otlp/0.31.0/opentelemetry_otlp/) — feature flags, `SpanExporter::builder().with_http().with_protocol()`, Protocol::HttpBinary, env var auto-read
- [docs.rs/tracing-opentelemetry/0.32.1](https://docs.rs/tracing-opentelemetry/0.32.1/tracing_opentelemetry/) — `OpenTelemetryLayer::new(tracer)`, `OpenTelemetrySpanExt::{context, set_parent}`
- [datalust.co/docs/tracing-from-opentelemetry-sdks](https://datalust.co/docs/tracing-from-opentelemetry-sdks) — Seq OTLP endpoint paths `/ingest/otlp/v1/traces` and `/logs`, HTTP/protobuf + gRPC support, `X-Seq-ApiKey` header
- [datalust.co/docs/ingestion-with-opentelemetry](https://datalust.co/docs/ingestion-with-opentelemetry) — logs ingestion specifics
- Existing source files in repo: `uc-observability/src/seq/{mod,layer,sender}.rs`, `uc-observability/src/clef_format.rs`, `uc-observability/src/init.rs`, `uc-observability/src/context.rs`, `uc-observability/src/stages.rs`, `uc-bootstrap/src/tracing.rs`, `uc-core/src/network/protocol/clipboard.rs`, `uc-app/src/usecases/internal/capture_clipboard.rs` (grep-verified span call sites)
- [opentelemetry.io/docs/specs/otlp/](https://opentelemetry.io/docs/specs/otlp/) — OTLP spec: base endpoint conventions, `/v1/traces` + `/v1/logs` path appending
- [W3C Trace Context](https://www.w3.org/TR/trace-context/) — traceparent header format

### Secondary (MEDIUM confidence)

- [github.com/open-telemetry/opentelemetry-rust](https://github.com/open-telemetry/opentelemetry-rust) — release cadence, cross-crate version pinning convention
- [signoz.io/blog/opentelemetry-rust](https://signoz.io/blog/opentelemetry-rust/) — setup walkthroughs
- [uptrace.dev/get/opentelemetry-rust/propagation](https://uptrace.dev/get/opentelemetry-rust/propagation) — inject/extract patterns
- Seq property name mapping for OTel-ingested traces (`TraceId`, `SpanName`, resource attribute prefixing) — inferred from Seq docs but should be confirmed by a live 5-minute check during implementation

### Tertiary (LOW confidence — flagged for validation)

- Exact Seq 2025.2 OTLP endpoint behavior on `OTEL_EXPORTER_OTLP_ENDPOINT=.../ingest/otlp` (base) vs `.../ingest/otlp/v1/traces` (full) — documented by Seq but the SDK's actual append behavior should be tested once at implementation time
- Whether `opentelemetry-appender-tracing` is needed in addition to `tracing-opentelemetry` for D-01 ("traces + logs") to be considered complete — Open Question #1

## Metadata

**Confidence breakdown:**

- Standard stack: **HIGH** — crate versions verified via docs.rs on 2026-04-04 (opentelemetry-otlp 0.31.1, tracing-opentelemetry 0.32.1, compatible minor 0.31)
- Architecture patterns (OTel pipeline, propagator, resource, OpenTelemetryLayer): **HIGH** — patterns directly from official docs.rs examples
- Seq OTLP endpoints & protocol support: **HIGH** — verified via datalust.co official docs
- Parent-child span restructuring for clipboard pipeline: **HIGH** — current code audited directly (grep output shows exact span call sites in capture_clipboard.rs, sync_outbound.rs, sync_inbound.rs)
- W3C traceparent inject/extract: **HIGH** — docs.rs + W3C spec
- Seq query field names post-migration (TraceId/SpanName): **MEDIUM** — documented by Seq but planner should verify during implementation
- Whether `opentelemetry-appender-tracing` is needed: **LOW** — planner discretion; recommendation is "start without, add later if needed"
- Exact Seq endpoint base-URL vs full-URL behavior: **MEDIUM** — documented both places but OTLP SDK path-append semantics need one manual test to confirm
- Pitfalls (cross-crate drift, propagator install, Drop/runtime): **HIGH** — known ecosystem issues, documented in opentelemetry-rust GitHub issues

**Research date:** 2026-04-04
**Valid until:** 2026-05-04 (30 days — opentelemetry-rust ecosystem has quarterly minor bumps that can shift feature flag names; re-verify crate versions if Phase 87 slips past May)
