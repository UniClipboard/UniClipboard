# Phase 87: 全面迁移 OTLP, 兼容 Seq 展示, 采用 OTLP 最佳实践 - Context

**Gathered:** 2026-04-04
**Status:** Ready for planning

<domain>
## Phase Boundary

将现有基于自定义 `SeqLayer` + `CLEFFormat` + reqwest 批量 POST (`/api/events/raw`) 的 observability 导出路径,全面替换为基于 OpenTelemetry SDK + OTLP 的标准化 telemetry 导出,同时保证 Seq 作为本地可视化后端继续可用。本期采纳 OTel 最佳实践:标准 semantic conventions 对齐、真正的 parent-child span 层级、W3C trace context 跨设备传播、标准 OTLP 环境变量配置。

**In scope:**
- 用 `opentelemetry` + `opentelemetry-otlp` + `tracing-opentelemetry` 替换 `uc-observability/src/seq/` 下的 layer/sender/clef_format 实现(硬切换)
- 将现有平铺的 stage span 重构为 root flow span + stage child span 的 parent-child 层级
- 全面对齐 OTel semantic conventions(resource 和 span 命名都对齐)
- `ClipboardMessage` 协议头新增 `traceparent` 字段,实现 W3C 标准的跨设备 distributed trace
- 同步重写 Phase 22/23 留下的 `docs/seq/signals/*.json`、`docs/architecture/logging-architecture.md` 中的 query/field 示例
- 切换 docker-compose.seq.yml 到 Seq OTLP ingestion(`/ingest/otlp/v1/{traces,logs}`)
- 使用 OTLP/HTTP-protobuf 作为传输协议

**Out of scope:** metrics(histograms/gauges/counters)本期不引入;生产环境仍然不启用 Seq/OTLP 导出(dev-only 工具);多跳 relay 场景(A→B→C)。

</domain>

<decisions>
## Implementation Decisions

### 信号类型范围

- **D-01:** 本期导出 **Traces + Logs** 两类信号。Metrics 推迟到后续 phase
  - Span 通过 `tracing-opentelemetry` bridge → OTLP traces
  - `tracing::info!/error!/warn!` 等 event → OTLP logs
  - Seq 原生支持两者,并可在 trace 详情页直接关联 logs

### 迁移策略: 硬切换

- **D-02:** 一步到位硬切换 — 直接删除 `uc-observability/src/seq/layer.rs`、`sender.rs` 以及 `clef_format.rs`
- **D-03:** 不保留 feature flag、不双写、不并行运行 — Seq 是 dev-only 工具,回归风险低,减少长期维护代价
- **D-04:** `docker-compose.seq.yml` 本期更新为启用 Seq OTLP ingestion endpoint

### Span 拓扑重构

- **D-05:** 将现有的平铺 sibling stage span 重构为 **parent-child 层级**
  - 新增一个 root flow span(覆盖整个 clipboard pipeline 的生命周期)
  - 各 stage (detect / classify / cache_representations / spool_blobs / outbound_sync / inbound_apply 等)作为该 root 的 child span
  - 跨 crate 的 instrumentation 改造:clipboard capture、outbound sync、inbound apply 等入口都需要在 root flow span 下打开
- **D-06:** Root flow span 的 `SpanContext` 即是 OTLP 中该 flow 的 trace context

### 字段命名与 semantic conventions 对齐

- **D-07:** 全面对齐 OTel semantic conventions(非保守策略 — 用户明确选择)
  - `flow_id` (UUID v7) 不再作为 span attribute 存在 — 其语义由 OTel `trace_id` 承担
  - `stage` 作为 **span name**(e.g. `clipboard.cache_representations`),不再以 `stage=xxx` 字段形式出现
  - `device_id` 提升为 resource attribute,命名候选 `service.instance.id` 或项目自有 `uc.device.id`(Claude's discretion — 参考 semconv)
  - `service.name = uniclipboard-desktop` / `service.version` / `deployment.environment` 等基础 resource attribute 按 semconv 填充
- **D-08:** Phase 22/23 留下的 Seq saved searches / signals 必须同步重写以适配新字段
  - `docs/seq/signals/flow-timeline.json` — 从 `@Properties.flow_id` 查询改为 `trace_id` / span name 过滤
  - `docs/seq/signals/cross-device-flow.json` — 改用 traceparent 链路语义查询
  - `docs/architecture/logging-architecture.md` — 更新 field 命名、query 示例、span 最佳实践章节

### 跨设备传播: W3C TraceContext

- **D-09:** 使用 **W3C traceparent** 作为跨设备 flow 链接的标准机制
- **D-10:** `ClipboardMessage` 协议头新增 `traceparent: Option<String>` 字段
  - 序列化时用 `serde(default)` + `skip_serializing_if` 以兼容旧 peer(Phase 21 `origin_flow_id` 同款模式)
  - 发送端:从当前 root flow span 导出 traceparent header 写入
  - 接收端:解析 traceparent,作为 inbound flow span 的 parent context(跨进程 trace continuation)
  - 旧 peer 不带 traceparent 时:inbound 端照常创建本地新 root flow span,log warn 提示并优雅降级(Phase 23 `origin_flow_id` 相同模式的参考)
- **D-11:** `origin_flow_id` 字段在本期从协议层移除其**逻辑依赖**(W3C trace context 代替),但协议结构体字段保留向后兼容(serde(default) 允许旧 peer 继续发送);新代码不再读取 / 写入 origin_flow_id,该字段进入墓碑状态由后续 phase 清理

### 导出协议与配置

- **D-12:** 使用 **OTLP/HTTP-protobuf** 作为 transport
  - 理由:复用已有的 reqwest + rustls-tls 依赖,不引入 tonic/gRPC 新栈
  - `opentelemetry-otlp` crate 直接支持 HTTP protobuf 模式
- **D-13:** 环境变量全面切换到 **OTel 标准环境变量**(非项目自有命名)
  - `OTEL_EXPORTER_OTLP_ENDPOINT` — Seq dev 示例值 `http://localhost:5341/ingest/otlp/v1`
  - `OTEL_EXPORTER_OTLP_HEADERS` — 可选 headers(e.g. `X-Seq-ApiKey=...`)
  - `OTEL_SERVICE_NAME` — 默认写死为 `uniclipboard-desktop`,允许 env 覆盖
  - 其他 OTel 标准 env 按 SDK 默认行为生效
- **D-14:** `UC_SEQ_URL` / `UC_SEQ_API_KEY` 在本期**停用**(Phase 22 引入的)
  - 启动时如检测到旧的 `UC_SEQ_URL` 仍被设置:log warn 提醒用户迁移到 `OTEL_EXPORTER_OTLP_ENDPOINT`,不做隐式 fallback(避免命名和实现长期不一致)

### Claude's Discretion

- OTLP exporter 的 batch size / flush interval / 超时参数(沿用 opentelemetry-otlp 默认或根据 dev 体验微调)
- `device_id` 具体映射为 `service.instance.id` 还是 `uc.device.id`(查 semconv 后决定;优先 semconv 标准字段)
- Root flow span 的具体 span name 选择(`clipboard.flow` vs `clipboard.capture_flow` 等)
- Cross-crate instrumentation 改造的具体切入点与 Span scope 传递方式(thread-local / explicit parameter / tokio task-local)
- Seq OTLP ingestion 的具体 endpoint 路径(需研究 Seq 当前版本文档确认)
- `tracing-opentelemetry` bridge 配置与 Layer 组合顺序
- 重写后的 Seq signals JSON 结构(需要对照 Seq 最新版 signal schema)
- 旧 `origin_flow_id` 字段标注 `#[allow(dead_code)]` / deprecated 注释的具体方式
- 测试策略(mock OTLP collector、集成测试 Seq 可见性等)

</decisions>

<specifics>
## Specific Ideas

- Phase 22 建立的 `build_seq_layer()` 接口可参考,但实现完全替换 — 新 API 可能叫 `build_otlp_layer()` 或直接 `init_otlp_pipeline()`,返回 `Option<(Layer, OtlpGuard)>` 仍沿用 Phase 22 的 guard 生命周期模式(`OnceLock` 静态存储)
- 用户明确选择 "全面对齐 semconv" 而非 "混合策略" — 表达了对长期技术方向的倾向:宁可一次性破坏 Phase 22/23 留下的 Seq query,也要让 telemetry 数据模型彻底 OTel 化
- `ClipboardMessage` 协议兼容模式遵循已有先例:Phase 21 `origin_flow_id` 的 serde(default) + 后续 Phase 才真正启用 — 本期 `traceparent` 用相同模式引入
- reqwest 已在 uc-observability 依赖中(`rustls-tls` feature),OTLP/HTTP-protobuf 选择与之自然衔接
- `tracing-opentelemetry` 是 tokio-rs 生态的官方 bridge,与现有 `tracing` + `tracing-subscriber` 组合无缝

</specifics>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 当前 observability 实现(必须读 — 硬切换的起点)

- `src-tauri/crates/uc-observability/src/lib.rs` — 公共 API 导出清单,改动范围的锚点
- `src-tauri/crates/uc-observability/src/seq/mod.rs` — 现有 Seq 模块入口,本期将被替换
- `src-tauri/crates/uc-observability/src/seq/layer.rs` — 现有 `SeqLayer` 实现,待删除
- `src-tauri/crates/uc-observability/src/seq/sender.rs` — 现有 CLEF HTTP 批量 sender,待删除
- `src-tauri/crates/uc-observability/src/clef_format.rs` — CLEF 格式化器,待删除
- `src-tauri/crates/uc-observability/src/init.rs` — `build_console_layer` / `build_json_layer` / `init_tracing_subscriber`,需要追加 OTLP layer 组合
- `src-tauri/crates/uc-observability/src/context.rs` — `global_device_id` 设置逻辑,OTLP resource attribute 填充的数据源
- `src-tauri/crates/uc-observability/src/flow.rs` — `FlowId` newtype,部分用法在 D-07 之后会改变
- `src-tauri/crates/uc-observability/src/stages.rs` — stage 常量定义,将变成 span name 来源
- `src-tauri/crates/uc-observability/Cargo.toml` — 依赖清单,需要新增 `opentelemetry`、`opentelemetry-otlp`、`tracing-opentelemetry`

### 接入层与协议(本期需要修改)

- `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` — dual-output tracing 初始化入口,OTLP pipeline 的组装点
- `src-tauri/src/main.rs` — `init_tracing_subscriber()` 的调用方,env 变量读取入口
- `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` — `ClipboardMessage` 结构定义,`origin_flow_id` 现有字段位置,本期新增 `traceparent` 字段
- 上述 protocol 文件的发送/接收路径(grep `origin_flow_id` 找到所有相关 flow span 打开点)

### Phase 22/23 遗留文档(本期需要重写)

- `docs/architecture/logging-architecture.md` — 需要更新 span naming 章节、Seq query 示例、字段命名说明、OTLP 配置指南替换 `UC_SEQ_URL` 段落
- `docs/seq/signals/flow-timeline.json` — 基于 `@Properties.flow_id` 分组,需迁移到 trace_id 语义
- `docs/seq/signals/cross-device-flow.json` — 基于 `origin_flow_id` 链接,需迁移到 traceparent 语义
- `docker-compose.seq.yml` — 切换到 Seq OTLP ingestion 配置(如需开启 OTLP endpoint)

### Phase 历史决策背景(理解语境所需)

- `.planning/milestones/v0.3.0-phases/19-dual-output-logging-foundation/19-CONTEXT.md` — 最初 dual-output 架构决策
- `.planning/milestones/v0.3.0-phases/20-clipboard-capture-flow-correlation/20-CONTEXT.md` — flow_id + stage 模型起源
- `.planning/milestones/v0.3.0-phases/22-seq-local-visualization/22-CONTEXT.md` — Seq 集成与 CLEF 的原始决策,本期正在颠覆其中部分
- `.planning/milestones/v0.3.0-phases/23-distributed-tracing-with-trace-view-visualization-for-cross-device-observability/23-CONTEXT.md` — origin_flow_id 跨设备传播的原始设计,本期用 traceparent 替换

### 外部标准(OTel 规范)

- OpenTelemetry Semantic Conventions — Resource attributes: `service.name`, `service.version`, `service.instance.id`, `deployment.environment` 等
- OpenTelemetry Logs Data Model & Traces Data Model
- W3C Trace Context Level 1(traceparent / tracestate header 格式)
- Seq OTLP ingestion 文档(需 researcher 确认最新版 Seq 支持的 OTLP endpoint 路径与 protobuf/JSON 编码支持情况)

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `LogProfile` (`uc-observability/src/profile.rs`) — Dev / Prod / DebugClipboard profile 过滤器仍然适用,OTLP layer 复用同一 profile filter
- `build_console_layer` / `build_json_layer` (`init.rs`) — Layer 组合模式不变,OTLP layer 加入同一 registry
- `WorkerGuard` + `OnceLock` 生命周期模式(Phase 19)— OTLP 后台批处理 task 的 guard 沿用该模式
- `global_device_id` / `set_global_device_id` (`context.rs`) — device_id 读取入口,OTLP resource attribute 构建时复用
- `reqwest` + `rustls-tls` — 已在依赖中,OTLP/HTTP-protobuf 走同一 HTTP 栈
- `tokio` runtime 与 mpsc — OTLP exporter 背景 task 架构复用
- `FlowId::new_v7()` — 本期字段语义变化,但 UUID v7 生成工具仍可作为补充 span attribute(若需要 Seq 端业务字段 fallback)
- `docker-compose.seq.yml` — 基础 Seq 容器定义,只需修改端口/endpoint 配置

### Established Patterns

- `fmt::layer().event_format(...).fmt_fields(...).with_writer(...).with_filter(...)` — tracing-subscriber 分层组合,OTLP layer 作为新的一层加入
- Env 变量驱动可选功能(Phase 22 `UC_SEQ_URL` 存在→启用)— 本期迁移到 OTel 标准 env,但存在性驱动启用的模式沿用
- Dev-only telemetry 后端(Phase 22 决策)— 生产环境不开启 OTLP 导出,Prod profile 下跳过 layer 构建
- 协议字段向后兼容(`serde(default)` + `skip_serializing_if`)— Phase 21 origin_flow_id 先例,本期 traceparent 照抄
- `info_span!("name", field = %value)` + `.instrument()` async 模式 — 本期需要在更高层次包一层 root flow span

### Integration Points

- `uc-observability/src/lib.rs` — 新增 `init_otlp_pipeline` / `build_otlp_layer` 等公共导出,移除 `build_seq_layer` / `SeqGuard` / `CLEFFormat`
- `uc-observability/Cargo.toml` — 依赖替换:新增 opentelemetry 三件套,删除 / 保留 reqwest(由 opentelemetry-otlp 内部使用或复用我们的)
- `uc-tauri/src/bootstrap/tracing.rs` — OTLP pipeline 组装、device_id 注入、env 解析
- `uc-core/src/network/protocol/clipboard.rs` — ClipboardMessage 结构扩展 traceparent 字段
- ClipboardMessage 所有发送点(outbound sync use case)— 写入 traceparent 前注入当前 span context
- ClipboardMessage 所有接收点(inbound apply use case / WS message handler)— 提取 traceparent 作为 inbound root flow span 的 parent
- 所有 clipboard pipeline stage 的 instrumentation 点(capture / classify / cache / spool / outbound / inbound)— 在 root flow span 下重新组织层级
- `src-tauri/src/main.rs` — tracing init 调用链
- `docs/seq/signals/*.json` — JSON 重写
- `docs/architecture/logging-architecture.md` — 文档更新
- `docker-compose.seq.yml` — Seq 容器 OTLP 配置

</code_context>

<deferred>
## Deferred Ideas

- **Metrics 导出**(OTLP metrics: histograms / gauges / counters)— 本期用户明确排除;未来可以加 sync latency histogram、queue depth gauge、错误率 counter 等
- **生产环境启用 OTLP**(远程 telemetry backend 如 Honeycomb / Datadog / Grafana Cloud)— 本期维持 dev-only 定位
- **多跳 trace chain**(A→B→C 场景)— W3C traceparent 本来就支持多跳,但本期 P2P 拓扑不覆盖多设备 relay 场景,验证与测试留待实际需求出现时处理
- **清理 ClipboardMessage.origin_flow_id 字段**(从协议结构体中删除)— 本期只墓碑化(保留字段 + 不再读写),完整移除需要一个独立的协议清理 phase 以处理向后兼容窗口
- **Seq 之外的 OTLP 后端对接**(Tempo / Jaeger / Grafana)— 理论上本期迁移后会自动支持,但不作为验证范围
- **Tracing profile 热切换**(OBS-01,Phase 22 就已 deferred)— 继续 deferred

</deferred>

---

_Phase: 87-otlp-seq-otlp_
_Context gathered: 2026-04-04_
