# Findings & Decisions (Issue #549)

## 单一真相源

`docs/architecture/telemetry-events.md` —— v1 schema 已定稿，所有字段、命名约定、隐私契约、演化策略都在这里。任何字段改动必须先改文档再改代码。

## 后端选型对照（决策结果：PostHog Cloud EU）

| 候选 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| **PostHog Cloud（EU endpoint）** | 开源、原生漏斗 / 留存、Rust SDK、免费额度每月 100 万事件 | 第三方依赖 | ✅ 选用 |
| PostHog self-host | 完全自控、数据不出公司 | 早期 < 10 用户维护成本不划算 | 后期可迁移，schema 不动 |
| Mixpanel / Amplitude | 成熟、产品强 | 闭源 SaaS、免费额度小、对开源项目不友好 | 拒绝 |
| Plausible / Umami | 轻量、隐私友好 | 偏页面 PV/UV，做不了"首次配对漏斗"事件级 | 拒绝 |
| OpenTelemetry + ClickHouse 自建 | 完全自控 | 回到"自研后端"陷阱，不符合 issue 的"低维护"原则 | 拒绝 |

## PostHog Rust SDK（`posthog-rs`）调研

- 当前版本 0.7.0，4 天前发（2026-05-05），50 stars，3 open issues，活跃维护
- 核心 API：`capture(event)` 单条 / `capture_batch(events)` 批量、async + blocking 两套 client
- 配置项：自托管 `host`、`EU_INGESTION_ENDPOINT`、`disable_geoip`、构造时 `disabled` 开关
- 依赖：`reqwest+rustls`（不引 openssl）、`uuid v7`、`tokio` 可选
- MSRV 1.78，与本仓库工具链兼容
- **接入复杂度：低**——wrapper ~100-200 行 Rust

### SDK 不直接给的能力

| 缺什么 | 影响 | 解决方案 |
|---|---|---|
| 运行时 opt-out | SDK 的 `disabled` 是构造时定的 | wrapper 查 `analytics_gate::is_analytics_enabled()` 再决定是否 capture |
| 磁盘持久化队列 | 进程崩溃时内存中未发出事件丢失 | 早期不做，可接受；产品分析数据丢失 1% 不影响决策 |
| `anonymous_user_id` 持久化 | SDK 不管 ID 生成 | 自己生成 UUIDv7 写到配置目录（已在 `analytics::ids` 落地） |

## Sentry vs PostHog（不互相替代）

| 维度 | Sentry | PostHog |
|---|---|---|
| 解决问题 | "哪里坏了" — 错误、崩溃、性能异常 | "用户在干嘛" — 行为、漏斗、留存 |
| 数据模型 | Issue（同类错误聚合） | Event（用户每个动作一条记录） |
| 核心能力 | error grouping、source map、release health | 漏斗、留存矩阵、事件切片、Feature Flag |
| 在本项目的开关 | `general.telemetry_enabled`（已存在） | `general.usage_analytics_enabled`（Slice 5 新增） |

## 关键技术决策（与 schema doc 对应）

### ID 设计（§3）

- **三层 ID**：`anonymous_user_id` / `analytics_device_id` / `session_id`
- **`analytics_device_id` 独立于业务 `DeviceId`**：防 cross-system correlation。即便有人同时拿到 PostHog 数据与 p2p 网络可观测信息，也无法对两侧做关联。约束写在 schema doc §3.1，代码注释在 `analytics/ids.rs`、`analytics/context.rs`
- **UUIDv7 全用**：自带时间戳，便于排查时按时间排序
- **持久化策略**：两个 ID 各自一个文件，在 `<analytics_dir>/installation_id` 与 `<analytics_dir>/analytics_device_id`。原子写（`<file>.tmp` → `rename(2)`）防崩溃半截
- **`is_first_run` 严格语义**：只有"两个 ID 都新生成"才标 true。任意一个已存在都是老安装——避免"分区损坏后修复"被误算成首次运行

### 隐私契约（§6）

**永不上传**：剪贴板原文、文件名原文、文件路径、用户名/hostname、客户端原始 IP、Sentry 已 redact 字段

**必须脱敏**：
- `space_id` → SHA-256 取前 16 hex
- `peer_device_id` → 同上
- `error_message` → 仅保留 `failure_reason` 枚举值，不传原始 message

**区间化**：payload 大小、耗时只上报区间（`Lt1Kb` / `1Kb_to_100Kb` 等）。例外：`sync_latency_ms` 需要 p95 分析的可上报精确值

### EventContext 设计（§4）

- **session 级共享**：`anonymous_user_id` / `analytics_device_id` / `session_id` / `app_version` / `os` / `arch` / `locale` / `timezone` / `install_source` / `is_first_run` / `active_device_count` / `space_id_hash`
- **`timestamp` 不在 context**：是事件级，由 sink 在 capture 时打或交给 PostHog SDK 自动注入
- **`active_device_count` 进程启动读一次后缓存**：每事件实时算太贵，session 内设备增减由 `pairing_succeeded` 事件覆盖
- **全局存储用 `RwLock<Option<Arc<...>>>`**：用户重置 telemetry IDs 后需要原地替换，`OnceLock` 不支持

### 命名约束（§5 / §8）

- 事件名 `{domain}_{action}_{state}`，全 snake_case，**永不重命名**
- 演化走 `*_v2` 新 variant，旧变量标注 deprecated 至少 90 天
- `enum_variants_serialize_to_documented_strings` 测试把 wire 形态钉死，CI 守住向后兼容

### Sink 抽象（`AnalyticsPort`）

- **同步 fire-and-forget**：`capture(event)` 不返回 `Result` / `Future`。产品事件丢失少量可接受，绝不阻塞业务路径
- **不传 EventContext**：sink 内部从 `global_event_context()` 读快照
- **trait object safe**：`Box<dyn AnalyticsPort>` 编译期断言，为 use case DI 注入做准备

### Sink wire 合并规则（Slice 7a 落地）

`build_event_payload(event, ctx) -> Map<String, Value>` 在 `analytics/sinks/mod.rs`，跨 sink 共用：

1. context 先 serialize 平铺
2. event.properties() 平铺（events 模块的 `properties_are_pure_event_fields_only` 测试守住与 context 无重叠）
3. 顶层加 `event` 字段 = event.name()
4. 顶层加 `distinct_id` = anonymous_user_id（PostHog 漏斗主键）

输出形态对所有 sink 一致——切换 sink 不需要改 dashboard 字段。

### StdoutSink 取舍

- 走 `tracing::debug!`（target = `uc_observability::analytics`）而非裸 `println!`，理由：schema doc §6.5 + 与 dual-output 风格一致 + release 默认级别天然过滤
- context 缺失 → 丢事件 + warn 节流（`AtomicBool::swap`，一次/sink 实例）。半截事件比缺事件更难排查
- 测试用自定义 `MakeWriter`+`tracing_subscriber::registry()` capture，不引 `tracing-test` / `tracing-mock` 等额外依赖

## 现有代码库观察

### 已有 observability 基础设施

`uc-observability` crate 已经存在：
- `tracing` dual-output（pretty console + JSON file）
- Sentry 集成（Issues + Logs + Performance）
- `telemetry_gate`（process-wide gate，已有 `set_global_device_id` 类似的 OnceLock 模式）
- `redact::is_sensitive_key` 与 `REDACTED_PLACEHOLDER`

### settings 流转链路（5 个文件，对称扩展）

```
uc-core/settings/model.rs              ← 业务真相
   ↓ From
uc-application/facade/settings/models  ← 应用 view + patch DTO
   ↓ From / settings_view_to_dto
uc-daemon-contract/api/dto/settings    ← 跨进程 wire DTO
   ↓ HTTP
uc-webserver/api/settings              ← PUT handler，先写盘后推 gate
   ↓ uc_observability::set_*
process-wide AtomicBool gates          ← Sentry / analytics 在 capture 时查
```

### 业务 `DeviceId` 现状

定义在 `uc-core/membership/`、`uc-core/ports/device_identity.rs`。`uc-bootstrap/tracing.rs:104` 通过 `paths.device_id_path()` 读 `vault/device_id.txt`。这是 **业务身份**（用于 pairing、membership），与 analytics 不要混淆。schema doc §3.1 明确 disjoint 约束。

### `AppPaths` 入口

`uc-application/facade/app_paths.rs:5-15`：
```
db_path / vault_dir / settings_path / logs_dir / cache_dir
file_cache_dir / spool_dir / app_data_root_dir
```

Analytics 持久化推荐落在 `app_data_root_dir/analytics/`（不进 vault——vault 是密钥级敏感目录）。Slice 6 拼装 EventContext 时再决定具体路径。

## v1 已知局限（待 polish）

- `detect_os_version`：当前 stub 返回 `"unknown"`。真实探测需要 `os_info` crate 或平台特定调用
- `detect_locale`：Windows 上 `LANG` 等环境变量基本不存在，会返回 `"unknown"`。需要 `sys-locale` crate
- `detect_timezone`：返回 UTC offset 字符串（`"+08:00"`），非 IANA 名（`"Asia/Shanghai"`）。需要 `iana-time-zone` crate
- 上述三处 polish 一起做，可一次性引入 3 个 crate

## 测试矩阵

`uc-observability` lib tests：47 passed
- analytics_gate: 3
- analytics::context: 8（含全局生命周期合并测试）
- analytics::events: 9
- analytics::ids: 10
- analytics::port: 2
- analytics::probe: 10
- telemetry_gate: 2 + 其他既有测试

跨 crate 验证：`uc-core` (377) / `uc-application` (84) / `uc-webserver` (39) / `uc-bootstrap` (4) / `uc-daemon-contract` (27) 全绿

## Schema 演化策略

| 变更类型 | 处理 |
|---|---|
| 新增事件 / property | 直接加，旧事件 null 兼容 |
| 重命名事件 / property | **禁止**。新建 `*_v2` |
| 删除事件 | 标 deprecated，至少保留 90 天 |
| 改变 property 语义（区间边界） | 必须新建 `*_v2` |

每次 schema 变更必须更新 `docs/architecture/telemetry-events.md` + `docs/changelog/*.md`
