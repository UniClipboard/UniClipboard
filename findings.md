# Findings & Decisions (Issue #549)

## 单一真相源

`docs/architecture/telemetry-events.md` —— v1 schema 已定稿，所有字段、命名约定、隐私契约、演化策略都在这里。任何字段改动必须先改文档再改代码。

## 后端选型对照（决策结果：PostHog Cloud US，2026-05-09 实际注册区域）

| 候选 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| **PostHog Cloud（US endpoint）** | 开源、原生漏斗 / 留存、Rust SDK、免费额度每月 100 万事件；US/EU region 隐私模型等价 | 第三方依赖；EU 用户数据驻留 US（PostHog DPA 已含 SCC） | ✅ 选用 |
| PostHog self-host | 完全自控、数据不出公司 | 早期 < 10 用户维护成本不划算 | 后期可迁移，schema 不动 |
| Mixpanel / Amplitude | 成熟、产品强 | 闭源 SaaS、免费额度小、对开源项目不友好 | 拒绝 |
| Plausible / Umami | 轻量、隐私友好 | 偏页面 PV/UV，做不了"首次配对漏斗"事件级 | 拒绝 |
| OpenTelemetry + ClickHouse 自建 | 完全自控 | 回到"自研后端"陷阱，不符合 issue 的"低维护"原则 | 拒绝 |

## PostHog Rust SDK（`posthog-rs`）调研

- 当前版本 0.7.0，4 天前发（2026-05-05），50 stars，3 open issues，活跃维护
- 核心 API：`capture(event)` 单条 / `capture_batch(events)` 批量、async + blocking 两套 client
- 配置项：自托管 `host`、`US_INGESTION_ENDPOINT`、`disable_geoip`、构造时 `disabled` 开关
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

## Slice 8b 探索发现（pairing 三事件埋点）

### 模块拓扑

- **没有独立 PairingFacade**——pairing 全归 `SpaceSetupFacade` 管（`uc-application/src/facade/space_setup/facade.rs`）
- **两端 use case**：
  - sponsor 端：`IssuePairingInvitationUseCase`（`usecases/pairing/issue_invitation.rs:58-84`）
  - joiner 端：`RedeemPairingInvitationUseCase`（`usecases/pairing/redeem_invitation.rs:97-172`）
- **AppDeps 已有 analytics 字段**（`uc-application/src/deps.rs:167`），但 `SpaceSetupDeps`（`facade/space_setup/deps.rs:23-90`）目前未持有

### Joiner 端同步语义（关键）

`RedeemPairingInvitationUseCase::execute()` 返回 `Ok(...)` = 完整握手 + admit_member + trust_peer + setup_status persist 全部完成。
返回 `Err(...)` = 失败终态。

→ 三事件全部可在同步路径内 fire：
- `pairing_started`：execute() 入口
- `pairing_succeeded`：return Ok 之前
- `pairing_failed`：每个 Err 路径之前（或在 execute 末尾按 Result match）

### Sponsor 端异步语义

`IssuePairingInvitationUseCase::execute()` 仅把 invitation 落到内存 holder + 通过 rendezvous 发布，**不等待 joiner 接入**。

真正的握手完成由 `PairingInboundOrchestrator` 在异步 task 内通过 `broadcast::Sender<PairingOutcome>` 发出：

```rust
pub enum PairingOutcome {
    Success { peer_device_id, peer_device_name, peer_fingerprint },
    Failure { reason: String },  // 注意是 String, 不是 enum
}
```

订阅入口：`SpaceSetupFacade::subscribe_pairing_completion()`（`facade.rs:429-437`）

→ sponsor 端如果要发三事件，需要在 `PairingInboundOrchestrator` 自己持有 `Arc<dyn AnalyticsPort>`，在内部各分支处发，因为 broadcast channel 上的 `PairingOutcome::Failure { reason: String }` 已经丢失结构化信息（只是格式化字符串）。

### PairingMethod 字段在代码里不存在

`IssuePairingInvitationUseCase::execute()` **零参数**：
```rust
pub(crate) async fn execute(&self) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError>
```

`RedeemPairingInvitationUseCase::execute(cmd)` 仅 `code: InvitationCode + passphrase: Passphrase`，没有"哪种方式发起"维度。

QR 扫码 vs 6 位 code 输入 vs 自动发现的区分 **当前完全在 GUI/CLI 层**——底层 use case 不感知。

→ 要填 `PairingMethod` schema 字段，必须新增一条参数从 GUI/CLI 一路传到 use case，或 v1 用单一占位值。

### FailureReason 不对齐

| 来源 | 变体数 | 变体清单 |
|---|---|---|
| schema 现有 `FailureReason` | 8 | PeerOffline / Timeout / PermissionDenied / NetworkError / FileTooLarge / ClipboardPermission / EncryptionMismatch / Unknown |
| `RedeemPairingInvitationError` | 12 | InvitationNotFound / InvitationExpired / SponsorUnreachable / ServiceUnavailable / PassphraseMismatch / CorruptedKeyMaterial / DeviceNameRequired / SponsorRejectedInvitation / SponsorDeclined / SponsorTimedOut / Timeout / ConnectionLost / Internal |

完全错位——`PassphraseMismatch` / `SponsorDeclined` / `SponsorRejectedInvitation` / `CorruptedKeyMaterial` 等 pairing 专属错误在 schema 现有 enum 里没有对应。挤进 `Unknown` 会丢失关键 funnel 漏点信号。

→ 决策点：扩展 `FailureReason` vs 新增专用 `PairingFailureReason` enum。schema doc §8 演化策略允许新增 variant（旧 sink 兼容 unknown variant）。

### 测试基础

- `NoopAnalyticsSink` 已可用（`port.rs:42-49`），单测可作 fake 注入起点
- 现有 pairing 路径 **无单元测试** —— `tests/` 目录下只有 `file_transfer.rs`
- `SpaceSetupFacade::new()` 构造器需要补 analytics 字段；`*UseCase` 同样需要

## Slice 8c-2 探索发现（FirstSyncStatePort + first_* 事件）

### 事件 schema 现状

`uc-observability/src/analytics/events.rs:57-73` 三个 first_* 事件 **已存在**（schema doc §7 预留）：
- `FirstClipboardSyncAttempted { direction: Direction }`
- `FirstClipboardSyncSucceeded { direction, peer_os: Option<Os>, transport_type, duration_ms }`
- `FirstFileSyncSucceeded { peer_os, transport_type, payload_size_bucket }`

事件名映射（`Event::name`）已钉死 wire 形态 → `first_clipboard_sync_attempted` / `first_clipboard_sync_succeeded` / `first_file_sync_succeeded`（events.rs:94-96）。Slice 8c-2 仅需 wire fire 点 + 去重 port，不动 event 定义。

### `AppVersionStatePort` 模板（FirstSyncStatePort 完整对照范本）

| 维度 | AppVersionStatePort | FirstSyncStatePort（本 slice） |
|---|---|---|
| trait 文件 | `uc-core/src/ports/app_version.rs:18-47` | `uc-core/src/ports/first_sync_state.rs`（新增） |
| 错误 enum | `AppVersionStateError` Read/Write/Corrupt 三变体 | `FirstSyncStateError` 同三变体 |
| infra 文件 | `uc-infra/src/app_version_state.rs` 整文件可参照 | `uc-infra/src/first_sync_state.rs`（新增） |
| 文件名 | `upgrade-cursor.json` | `first-sync-state.json` |
| 路径 | `app_data_root.join(DEFAULT_FILE_NAME)` | 同左（同一 root） |
| schema 字段 | `{schema_version:1, last_seen_version: String}` | `{schema_version:1, attempted: bool, succeeded: bool, file_succeeded: bool}` |
| 原子写 | tempfile + sync_all + rename | 同左 |
| 测试 | 7 个 tokio test（infra 内嵌 mod tests） | 7 个 + race 测试 |
| 装配点 | `uc-bootstrap/src/assembly.rs:404-410` InfraLayer + `deps.rs:148-150` AppDeps | 同区域插入 |

**唯一架构差别**：FirstSyncStatePort 需要 **`tokio::sync::Mutex` 串行 read-check-write**（race 防护），AppVersionStatePort 无并发 mark 场景所以无锁。

### `app_data_root_dir` 落点确认

`uc-application/src/facade/app_paths.rs:14,34-56` 字段 `pub app_data_root_dir: PathBuf`。已有调用：`upgrade-cursor.json` / `.daemon-token` / `.daemon-pid` 都直接 `app_data_root_dir.join(name)`。**`first-sync-state.json` 同位入驻。** Windows 上 cache_dir 与 data_root 重合时已被 `AppPaths::from_app_dirs` 自动避让到子目录，无需在 port 层处理。

### 4 个构造点精确位置

| # | 文件 | 行号 | 改动 |
|---|---|---|---|
| 1 | `uc-application/src/deps.rs` | 139-168 | `AppDeps` 加 `first_sync_state: Arc<dyn FirstSyncStatePort>` 字段（与 `app_version_state`/`analytics` 同层） |
| 1 | `uc-bootstrap/src/assembly.rs` | 404-410 | InfraLayer 构造 + AppDeps 聚合点 |
| 2 | `uc-application/src/facade/clipboard/facade.rs` | 42-57 | `ClipboardSyncDeps` 加字段；facade::new 透传 |
| 2 | `uc-bootstrap/src/space_setup.rs` | 390-402 | 构造点 `Arc::clone(&deps.first_sync_state)` |
| 3 | `uc-application/src/usecases/clipboard_sync/dispatch_entry.rs` | 158-198, 287-323 | use case struct field + new 参数；spawn 内 mark + 条件 fire |
| 4 | `uc-bootstrap/tests/slice2_phase2_clipboard_e2e.rs` | 测试构造点 | 补 fake/file impl `first_sync_state` |

### 当前 sync spawn 块结构（dispatch_entry.rs:287-323，Slice 8c-1 落地）

```
fan_out per peer {
    spawn {
        analytics.capture(SyncAttempted)        // line 288-296
        // ← 在此插：if first_sync_state.mark_first_sync_attempted()? { fire FirstClipboardSyncAttempted }
        let started_at = Instant::now()         // line 297
        match dispatch.dispatch(...).await {
            Ok(_) => {
                fire SyncSucceeded { duration_ms = elapsed }
                // ← 在此插：if mark_first_sync_succeeded()? { fire FirstClipboardSyncSucceeded }
                // ← 在此插：if payload_type==File && mark_first_file_sync_succeeded()? { fire FirstFileSyncSucceeded }
            }
            Err(e) => fire SyncFailed { failure_reason }
        }
    }
}
```

失败路径不触 `_succeeded` 系列 mark；attempted 在 SyncAttempted 之后无论后续成功失败都已被 mark。

### Race 模型与去重策略

**场景**：用户首次复制时若已 paired N 个 peer，dispatch_entry 同时 spawn N task；每个都进入"我是不是首次"判断。若 port impl 是非原子的 read-check-write，N 个 spawn 都可能看到 `attempted=false`、都 fire 事件、都 race 写 `attempted=true`——重复上报。

**裁决**：port impl 内部用 `tokio::sync::Mutex` 把整个 `read JSON → check flag → set flag → write JSON` 包成 critical section。N 个 spawn 串行过此锁，第一个置位返回 `true`（fire 事件），其余返回 `false`（不 fire）。fan-out 量级 < 10，磁盘 IO 不构成瓶颈。

**测试**：`tokio::join!(spawn1.mark(), spawn2.mark(), ..., spawn8.mark())` 收集 N 个 Result，断言 `iter().filter(|r| **r == true).count() == 1`。

### `payload_type_from_categories` 复用

Slice 8c-1 已在 `dispatch_entry.rs` 私有 fn 实现 File > Image > Text 优先级推导。8c-2 直接复用——首次成功时 `if matches!(payload_type, PayloadType::File)` 触 file 分支额外 mark + fire。

### Port 命名 / 错误粒度自我审查（uc-core AGENTS.md §12）

- ✅ 业务能力命名 `FirstSyncStatePort`（不出现 file/json/sqlite 等技术词）
- ✅ `bool` 返回值是领域语义"是否首次置位"，非 IO 状态
- ✅ 三个 method 是同一持久化资源的不同 fact，单 port 合理（不拆 `FirstAttemptedPort` / `FirstSucceededPort` / `FirstFileSucceededPort` 三个）
- ✅ `FirstSyncStateError` 三变体与 `AppVersionStateError` 一致，错误语义稳定
- ✅ uc-core 不感知 `Mutex` / `tokio::fs` / JSON 等实现——全部留 uc-infra

## Slice 7b 探索发现（PostHog Cloud 接入）

### SENTRY_DSN 注入范本（PosthogSink key 注入完全镜像）

**代码侧**（`uc-bootstrap/src/tracing.rs:155-170`）：

```rust
let runtime_dsn = std::env::var("SENTRY_DSN").ok().filter(|s| !s.is_empty());
let compile_time_dsn = option_env!("SENTRY_DSN").filter(|s| !s.is_empty());
let dsn = runtime_dsn.or_else(|| compile_time_dsn.map(String::from));
```

三级回退语义：
1. **运行时 env** —— dev / 自部署用户运行时覆盖（也含 `cargo run` 本地调试）
2. **编译期 `option_env!`** —— CI release build 时把 secret 烤进 binary（终端用户机器上没人会设这个 env）
3. **都缺** —— 安静关闭，不打印 warn（缺 key 是合法配置）

**CI 注入**（`.github/workflows/build.yml:172-180` 与 `.github/workflows/alpha-build.yml`）：在 `tauri-action` 与 `bun run tauri build` 两段的 `env:` 块里同位写 `SENTRY_DSN: ${{ secrets.SENTRY_DSN }}`。Empty secret = 该 telemetry 通道 disabled，等价于不传。

**前端对照**（`VITE_SENTRY_DSN`）：vite 环境变量在 build 时通过 `import.meta.env` 烤进 JS bundle，与后端 `option_env!` 等价。前后端 Sentry 项目相互独立（CI workflow 注释 `MUST be a separate Sentry project`）。

**Slice 7b 直接复用**：
- 后端 PosthogSink → secret `POSTHOG_PROJECT_KEY`（与 `SENTRY_DSN` 同位、同语义）
- 前端目前不接 PostHog（schema doc 约定产品 telemetry 在后端集中发；前端 GUI 事件由 Tauri command 反传到 daemon 再 capture）
- 不引第三种 secret 注入机制

### posthog-rs 0.7 API 调研（context7 / `/posthog/posthog-rs` 命中）

#### 客户端构造形态

```rust
use posthog_rs::{client, ClientOptionsBuilder, US_INGESTION_ENDPOINT};

let opts = ClientOptionsBuilder::default()
    .api_key("phc_xxx".to_string())
    .host(US_INGESTION_ENDPOINT.to_string())
    .disable_geoip(true)            // schema doc §6：客户端 IP 不上传
    .disabled(false)                // true = 测试 fake；prod 当然 false
    .request_timeout_seconds(30)    // 默认即 30，写出来更显式
    .build()
    .expect("build options");

let client: posthog_rs::Client = posthog_rs::client(opts).await;
```

要点：
- `client(...)` 是 async fn（这一点决定了 `build_analytics_sink` 必须转 async）
- `US_INGESTION_ENDPOINT` 是常量字符串（`https://us.i.posthog.com`，posthog-rs 默认值）；`EU_INGESTION_ENDPOINT` 同等地位（`https://eu.i.posthog.com`），切 region 仅改 host 字符串
- `disabled(true)` 是测试黄金钥匙：构造合法 client、`capture` 不真发 HTTP，可在单测里安全用

#### 事件 capture 形态

```rust
let mut event = posthog_rs::Event::new("app_first_open", distinct_id);
event.insert_prop("app_version", "0.7.0-alpha.7")?;
event.insert_prop("space_id_hash", "abc...")?;
client.capture(event).await?;
```

`Event` 构造签名：`Event::new(event_name: &str, distinct_id: &str)`。`insert_prop` 内部用 `serde_json::Value`，与本仓库 `build_event_payload` 的 `Map<String, Value>` 完全兼容——可以直接 iterate 后 `insert_prop`，不需要再做类型转换。

#### posthog-rs 不直接给的能力（与 findings.md 早期调研对齐）

| 缺什么 | 影响 | Slice 7b 解决方案 |
|---|---|---|
| 运行时 opt-out（`disabled` 是构造时定的） | gate 翻关后已构造的 client 还会 capture | 外层 `GatedAnalyticsSink` 在 capture 入口拦截（**已有**，复用 7a 落地） |
| 同步 capture 入口 | `AnalyticsPort::capture` 是 sync fn，`client.capture` 是 async fn | sink 内 `tokio::spawn(async move { client.capture(...).await })` fire-and-forget |
| 进程退出显式 flush | client Drop 行为依赖运行时；tauri main loop 退出时机不一定让 task 跑完 | v1 不挂；schema doc §10 已允许 < 1% 丢失 |
| 磁盘持久化队列 | 进程崩溃时内存未发出事件丢失 | 同上，v1 不做 |

#### Cargo features 选择

```toml
posthog-rs = { version = "0.7", default-features = false, features = ["async-client"] }
```

不要 default features：默认含 blocking client 与可能的 reqwest openssl backend。我们已用 tokio runtime，async-client 是唯一需要的。明确禁默认 + 显式 features = 防 transitive openssl 引入（项目早期已统一 reqwest+rustls，与 sentry 一致）。

**验证命令**：`cargo tree -p uc-observability -e features | rg -i 'openssl|native-tls'` 应为空。出现就是 features 选错。

### `build_event_payload` 与 PosthogSink 的字段映射

`sinks/mod.rs::build_event_payload` 已经把 wire 形态钉死成顶层带 `event` / `distinct_id` + 平铺 context + 平铺 event-specific props 的 `Map<String, Value>`。

PosthogSink 集成走法（避免字段冲突）：

```rust
let mut payload = build_event_payload(&event, &ctx);

// posthog Event::new 第二参数已经吃了 distinct_id；payload 里再把
// distinct_id / event 留下会变成事件属性而非顶层主键，触发 PostHog
// 端的 distinct_id property collision 警告。
let distinct_id = payload
    .remove("distinct_id")
    .and_then(|v| v.as_str().map(String::from))
    .unwrap_or_default();
payload.remove("event");

let mut ph_event = posthog_rs::Event::new(event.name(), &distinct_id);
for (k, v) in payload {
    let _ = ph_event.insert_prop(k, v);
}
```

`event.name()` 与 `event` 字段相同来源（`Event::name` 是单一真相），不会偏离 wire 钉死测试。

### 测试取舍

- ✅ 用 `disabled(true)` 构造的 client 跑 PosthogSink lifecycle 单测 —— 验证 capture 不 panic、context 缺失分支节流 warn 一次
- ✅ wire 字段冲突 invariant：单测断言 payload 处理后 `distinct_id` / `event` 不在 props 里
- ❌ HTTP 行为（batching、retry、backoff） —— SDK 内部职责，不归本仓库
- ❌ 真实 US endpoint 联通性 —— 7b-4 走人工 dev 验证，不进 CI

### 进程退出 flush 风险评估

| 风险 | 概率 | 后果 | v1 处置 |
|---|---|---|---|
| 用户操作后秒杀进程 | 低（GUI 用户不太会直接 kill -9） | 队列里事件丢 | 不处理；后续观察实际丢失率 |
| daemon 主动重启（settings 修改触发） | 中 | 同上 | 不处理；事件丢失为 onboarding 中段（funnel 端点 `pairing_succeeded` / `first_clipboard_sync_succeeded` 已被新进程的下次 capture 覆盖） |
| 系统休眠 / 网络断开 | 高（移动设备常态） | client 内部 retry 队列吃下；唤醒后续传 | SDK 自带，不需要额外处理 |
| 立刻退出（compose 完 → app_first_open spawn 出去 → 进程结束） | 关键 | 首条 `app_first_open` 可能丢 | **若实际丢失率 > 5%** 才补 `tauri::App::on_exit` 钩子做 best-effort drain |

### 关键决策（写到 task_plan.md Decisions Made 表的对应内容已就位）

1. fire-and-forget = `tokio::spawn`，不自建队列、不阻塞业务
2. key 注入完全镜像 SENTRY_DSN 三级回退
3. release 缺 key → `Gated(NoopAnalyticsSink)`，启动不失败
4. ~~`disable_geoip = true`~~（SDK 选项）→ 改为"client 不主动 inject IP 字段"（自写路径自然成立）
5. v1 不挂进程退出 flush 钩子
6. ~~`build_analytics_sink` 转 async~~ → 改为保持 sync（自写 client 构造同步，传染面 0）
7. CI 不联真实 PostHog

## ⚠️ Slice 7b 实现路径转向：posthog-rs SDK → 自写 reqwest client（2026-05-09）

### 问题

cargo tree 验证 posthog-rs 0.7 引入 `aws-lc-rs` 依赖：

```
posthog-rs v0.7.0
  └── reqwest v0.13.2 (hardcoded by posthog-rs Cargo.toml)
        └── feature "rustls" → __rustls-aws-lc-rs
              └── hyper-rustls feature "aws-lc-rs"
                    ├── rustls feature "aws-lc-rs"
                    └── aws-lc-rs v1.16.2 (含 aws-lc-sys C 库 + CMake 编译)
```

### 与项目硬约束的冲突

`uc-bootstrap/Cargo.toml:27-34` sentry 配置注释明确：

> reqwest 0.13 的 rustls feature 硬绑定 aws-lc-rs (C 库，CMake 编译，musl cross 不友好);
> ureq 3.x 的 rustls feature 只走 ring, 与 workspace 其他 rustls 用户对齐。这样 uc-cli 走
> musl 静态编译时依赖图里没有任何 C 工具链强依赖。

也就是说项目刻意为 sentry 选 ureq 来避开 reqwest 0.13 + aws-lc-rs。posthog-rs 0.7 的 Cargo.toml 把 reqwest 0.13 + features 写死，没有提供 features flag 切回 ring。

### 为什么不能用 cargo features 把 uc-cli 排除

`uc-cli → uc-bootstrap → uc-observability`，`uc-cli → uc-observability` 两条直接依赖路径都存在。cargo features unification 是 workspace 级 union：任何依赖路径启用 `posthog` feature 都会传染到所有 uc-observability 实例化。所以 `optional + dep:` + feature gate 行不通。

### 为什么不挪到独立 crate

理论上把 PosthogSink 抽到 `uc-analytics-posthog` 新 crate，只让 desktop 入口（uc-tauri/uc-desktop）依赖。但 `build_analytics_sink()` 在 uc-bootstrap，而 uc-bootstrap 是 uc-cli 的依赖。即使 sink 实现挪走，sink 注入点也得跟着挪到 desktop 一侧 builder，破坏了"所有 entry 走 build_*_facade 统一构造 AppDeps"的一致性。

### 选择路径：自写 minimal reqwest client

PostHog capture API 极简：

```
POST https://us.i.posthog.com/i/v0/e/
Content-Type: application/json

{
  "api_key": "phc_xxx",
  "event": "app_first_open",
  "distinct_id": "<anonymous_user_id uuid>",
  "properties": { ...所有 EventContext + event-specific 字段 },
  "timestamp": "2026-05-09T12:34:56Z"   (optional, ISO 8601)
}
```

参考：[PostHog REST API capture endpoint](https://posthog.com/docs/api/capture)

### 自写 client 的依赖图

```toml
# uc-observability/Cargo.toml 新增
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "rustls-tls-webpki-roots"] }
tokio = { version = "1", default-features = false, features = ["rt"] }
```

reqwest 0.12 的 `rustls-tls` feature 走 ring，**不引 aws-lc-rs**。这与 uc-infra/uc-daemon-client/uc-cli/uc-desktop 已用模式完全一致——加新 dep 但不加新约束。

### SDK vs 自写：能力对比

| 能力 | posthog-rs SDK 0.7 | 自写 reqwest client |
|---|---|---|
| 基础 capture POST | ✅ | ✅（~30 行） |
| 内置批量队列（多事件合并 1 个 HTTP） | ✅ | ❌（每事件 1 个 POST） |
| 自动 retry / backoff | ✅ | ❌（fire-and-forget 失败 warn） |
| Feature flag local evaluation | ✅ | ❌（v1 不需要） |
| Group analytics / identify alias | ✅ | ❌（v1 不需要） |
| `disable_geoip` 配置项 | ✅ | 通过"不主动 inject IP 字段"自然达成 |
| reqwest 版本固定 0.13 + aws-lc-rs | ⚠️ 强绑 | 无（用 0.12 + ring） |
| 代码量 | 0 | ~100 行（含测试） |
| 维护负担 | SDK 演进 + 我们 wrapper | 全部我们维护 |

**结论**：v1 我们只用基础 capture POST，SDK 70% 的能力都用不上。失去 batching + retry 的代价 < 1% 事件丢失（schema doc §10 已允许）。换来零 aws-lc 污染，值得。

### 后续切换 SDK 的路径（不阻塞 v1）

未来如果出现以下任一信号才考虑切回 SDK 或自建队列：

- 实测事件丢失率 > 5%（产品 telemetry 数据可信度不够）
- 用户量 > 1k 后单事件 POST 量过高（PostHog 免费额度压力）
- 需要 feature flag local evaluation（产品方向变化）

任一信号触发 → 重启"SDK vs 自建队列 vs HTTP/3"评估；schema 与 wire 形态零改动（`build_event_payload` 不变，仅 sink 实现替换）。

### 自写 client 实现要点

```rust
pub struct PosthogSink {
    client: reqwest::Client,        // 复用连接池
    api_key: String,
    endpoint: String,                // 默认 https://us.i.posthog.com/i/v0/e/
    warned_missing_context: AtomicBool,
}

impl PosthogSink {
    pub fn new(api_key: String) -> Self { ... }
    pub fn with_endpoint(api_key: String, endpoint: String) -> Self { ... } // mock test 用
}

// 纯 fn，便于单测
fn build_capture_body(event_name: &str, payload: Map<String, Value>, api_key: &str) -> Value {
    // 1. payload.remove("distinct_id") → distinct_id 顶层
    // 2. payload.remove("event")        → 顶层 event 用 event_name
    // 3. 剩余 payload → properties
    // 4. timestamp = chrono::Utc::now().to_rfc3339()
}

impl AnalyticsPort for PosthogSink {
    fn capture(&self, event: Event) {
        let Some(ctx) = global_event_context() else {
            self.warn_missing_context_once(event.name());
            return;
        };
        let payload = build_event_payload(&event, &ctx);
        let body = build_capture_body(event.name(), payload, &self.api_key);
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move {
            match client.post(&endpoint).json(&body).send().await {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => warn!(target: TRACE_TARGET, status = ?r.status(), "posthog capture non-2xx"),
                Err(e) => warn!(target: TRACE_TARGET, error = %e, "posthog capture failed"),
            }
        });
    }
}
```

测试：
- `build_capture_body_*` 纯 fn 测试 3-4 case
- `posthog_sink_lifecycle` 烟测：`wiremock` 起 mock server，验证 POST 1 次 + body 字段正确
- `posthog_sink_drops_event_without_context` 同 stdout 节流断言

### 调用点（key 注入降级路径）

`uc-bootstrap/src/analytics.rs::build_analytics_sink` **保持 sync**（自写 client 全同步构造，与 SDK 方案不同；`assembly.rs:947` 调用点零改动）：

```rust
pub fn build_analytics_sink() -> Arc<dyn AnalyticsPort> {
    let inner: Arc<dyn AnalyticsPort> = if cfg!(debug_assertions) {
        Arc::new(StdoutSink::new())
    } else {
        match resolve_posthog_key(
            std::env::var("POSTHOG_PROJECT_KEY").ok().filter(|s| !s.is_empty()),
            option_env!("POSTHOG_PROJECT_KEY"),
        ) {
            Some(key) => Arc::new(PosthogSink::new(key)),
            None => {
                tracing::info!("POSTHOG_PROJECT_KEY 未配置，产品 telemetry 关闭");
                Arc::new(NoopAnalyticsSink)
            }
        }
    };
    Arc::new(GatedAnalyticsSink::new(inner))
}

fn resolve_posthog_key(runtime: Option<String>, compile: Option<&'static str>) -> Option<String> {
    runtime.or_else(|| compile.filter(|s| !s.is_empty()).map(String::from))
}
```

## Session 2026-05-12 — Slice 7b-4 CI workflow 注入

用户确认继续后，补完 7b-4 中不依赖 PostHog 账号的第一项：CI workflow secret 注入。

### 发现

- `.github/workflows/build.yml` 有两个构建入口：macOS 走 `tauri-action`，非 macOS 走 `bun run tauri build`，两处都已有 `SENTRY_DSN` env。
- `.github/workflows/alpha-build.yml` 当前只有 `tauri-action` 构建入口，没有单独的 `bun run tauri build` 构建段；因此只补实际存在的构建 env 块。

### 改动

- 在 `.github/workflows/build.yml` 的 macOS 与非 macOS 构建 env 中加入 `POSTHOG_PROJECT_KEY: ${{ secrets.POSTHOG_PROJECT_KEY }}`。
- 在 `.github/workflows/alpha-build.yml` 的 alpha 构建 env 中加入同一项。

### 剩余

- GitHub repository secret `POSTHOG_PROJECT_KEY` 仍需外部注入。
- 真实 dev 验证仍需 PostHog Cloud project key。
