# 产品 Telemetry 事件 Schema（v1）

> 起草时间：2026-05-09
> 涉及 Issue：UniClipboard#549
> 状态：**v1 schema 已定稿**，§10 开放问题已全部裁决。
> 本文件只定义事件 schema、身份标识与隐私契约；
> 上报通道（PostHog Cloud SDK 接入、本地队列、批量发送）属于子任务 2，不在此处展开。
> 后端选型：**PostHog Cloud（US ingestion endpoint，2026-05-09 起）**，理由见 §10。

## 1. 范围与目标

### 1.1 目标

为 UniClipboard 客户端建立一套 **结构化的产品事件模型**，让早期增长与可靠性
判断有据可依。本草案聚焦 issue #549 的"第一版必须埋点"中 **最关键的两段**：

- **Activation 漏斗**：`app_first_open` → `pairing_succeeded` → `first_clipboard_sync_succeeded`
- **Reliability**：`sync_attempted` / `sync_succeeded` / `sync_failed` / `sync_deferred`

Acquisition、Retention、Engagement、Friction 沿用同一 schema 与命名规则，但
事件清单延后到 v1 实施过程中按需补齐，不在本文件穷举。

### 1.2 非目标

- 不定义后端 / SDK / 上报通道。本文件 **与 PostHog 无任何耦合**，schema 可
  迁移到任意支持 properties 的 analytics 后端。
- 不定义 dashboard 与查询 SQL。
- 不上传剪贴板内容、文件内容、原始文件名、原始 IP——这是隐私契约，详见 §6。

## 2. 与现有可观测性体系的关系

| 体系 | 解决问题 | 数据形态 | crate |
|---|---|---|---|
| `tracing` + Sentry Logs | "为什么坏了" — 错误、性能 | issue / span 聚合 | `uc-observability` |
| **本文档定义的 telemetry** | "用户在干嘛" — 漏斗、留存 | event 流 | 计划：`uc-observability::analytics` 子模块 |

两者 **不互相替代**。Sentry 不应承担产品分析职责（不擅长事件级聚合）；产品
telemetry 也不应记录错误堆栈（隐私不可控）。

## 3. 身份标识

### 3.1 三层 ID

| ID | 生命周期 | 生成时机 | 持久化 | 用途 |
|---|---|---|---|---|
| `anonymous_user_id` | 永久（直到用户卸载或主动重置） | 首次启动 | 配置目录文件 | Solo 状态下的 distinct_id；设备级 retention 兜底 |
| `analytics_device_id` | 永久（每台设备一个） | 首次启动 | 配置目录文件 | 设备级切片，如 OS 组合分析 |
| `session_id` | 单次进程运行 | 进程启动 | 内存 | 单次会话漏斗对齐、调试 |
| `space_person_id` | 与 Space 绑定（跨设备共享） | A1 sponsor 创建 Space 时 / A2 joiner 接受 sponsor 派发时 | 配置目录文件 | **v2 跨设备 person 聚合**：同 Space 多设备 → PostHog 上同一 person |

**为什么不叫 `device_id`**：仓库里已有 `uc-core` 域内的 `DeviceId`（pairing /
membership 的 **业务身份**）。analytics 必须使用一个完全独立的 ID，确保即便
有人同时拿到 PostHog 数据与 p2p 网络可观测信息，也无法对两侧做 cross-system
correlation——这是 §6 隐私契约"匿名"承诺的字面落实。

具体约束：

- analytics 模块 **不允许** 读取或派生自 `uc-core::DeviceId`。
- analytics 模块 **不允许** 把 `analytics_device_id` 写入任何业务持久化层
  （settings 之外不允许出现）。
- 业务层不应消费 `analytics_device_id`——它只用于 telemetry sink。
- **v2 红线**：`space_person_id` 必须独立 UUIDv7，**不** 可从 `space_id`
  反推（独立生成），**不** 可从业务 `DeviceId` 派生，业务层不消费此字段。
  `space_person_id` 与 future 账号 ID 也必须 disjoint（保留"匿名"语义）。

### 3.2 生成与存储

- 全部使用 **UUIDv7**（与 SDK 依赖对齐，且自带时间戳便于排查）。
- 持久化路径（实施重构：从 settings 命名空间移到 `<app_data>/analytics/` 目录，
  避免与业务 settings 同步污染）：

  ```text
  <app_data>/analytics/
  ├── installation_id        # 文本，单行 UUID（= anonymous_user_id）
  ├── analytics_device_id    # 同上
  └── space_person_id        # 同上，可缺失（未加入 Space 时不存在）
  ```

  本目录的所有文件 **不进入** settings 导出 / 同步范围：
  - `anonymous_user_id` / `analytics_device_id`：必须随设备绑定。
  - `space_person_id`：必须随 Space 绑定（v2 通过 pairing 加密通道下发，
    **不** 走 settings export-to-file 路径，避免泄露给 backup 文件）。

- `session_id` 不持久化，每次进程启动重新生成。

### 3.3 重置语义

- 用户在设置页可"重置 telemetry ID"——同时清空
  `installation_id` / `analytics_device_id` / `space_person_id`（v2），
  下次启动重新生成 anonymous + device，`space_person_id` 维持空（直到下次有
  新设备 pairing 才再下发）。
- 关闭 telemetry 开关 **不** 清除 ID（避免误关后再开导致用户跨次被算作新人）。
- `anonymous_user_id` ≠ 账号 ID。即便后续上线账号体系，也不要把这两个 ID
  关联起来——保留"匿名"语义。
- `analytics_device_id` ≠ 业务 `DeviceId`。重置 analytics ID 不影响业务
  pairing / membership 状态。
- **本机 reset 不影响其他设备**（v2）：其他设备仍持有原 `space_person_id`，
  Space 维度的 person 不会消失，只是本机被切回 Solo + 全新 anonymous。
  reset 完成后会同时发一条 `$identify` 把本机近期事件归并到新 anonymous
  (`old_distinct_id` = 旧 person id，`new_distinct_id` = 新 anonymous_user_id)。

### 3.4 多设备同 Space 的关系（v2 跨设备 person 聚合）

**v1 口径**：留存指"这台设备隔天还回来"，每台设备一个 `anonymous_user_id`、
一个 `analytics_device_id`，distinct_id = `anonymous_user_id`。

**v2 升级（Phase 098）**：引入 `AnalyticsPersonId` enum 显式区分 distinct_id
来源：

```rust
// uc-observability::analytics::context
pub enum AnalyticsPersonId {
    /// 未加入 Space：distinct_id = anonymous_user_id（与 v1 兼容）。
    Solo(Uuid),
    /// 已加入 Space：distinct_id = sponsor 派发的 space_person_id。
    /// 同 Space 多设备共享同一个 space_person_id → PostHog 聚合为同一 person。
    SpaceShared(Uuid),
}
```

`EventContext.analytics_person_id` 字段标 `#[serde(skip)]` —— 不进 wire，
仅作 sink 派生 distinct_id 的输入。`build_event_payload` 在拼 payload 时
取 `ctx.analytics_person_id.as_uuid()` 作 distinct_id；`anonymous_user_id`
flat 字段保留在 properties 里用于设备级切片。

**`space_person_id` 的生成与下发**：

| 时机 | 谁生成 | 谁持久化 | 触发的 `$identify` |
|---|---|---|---|
| A1 sponsor `setup_completed` | sponsor 自己 `Uuid::now_v7()` | sponsor 落 `<analytics_dir>/space_person_id` | `old=anon`, `new=space_person_id` |
| A2 joiner `pairing_succeeded` | sponsor 派发（pairing 加密通道） | joiner 落 `<analytics_dir>/space_person_id` | `old=anon`, `new=space_person_id` |
| `switch_space` commit 完成 | 目标 Space 的 sponsor 派发 | 替换本机 `space_person_id` | `old=旧 person`, `new=目标 person` |
| 用户重置 telemetry | — | 清 `space_person_id` + 重新生成 anonymous | `old=旧 person`, `new=新 anon` |

**v1 → v2 升级**（task_plan §开放问题 1 决策 A）：v1 时已配对的 Space 在
v2 上线后 **不做迁移**——已升级设备继续按 Solo 上报，直到该 Space 内有新
设备 pairing 时才生成 `space_person_id` 并下发。

**Pairing wire 互操作**（task_plan §开放问题 2 决策 A）：
`SponsorConfirm.sponsor_space_person_id` 类型为 `Option<Uuid>`。joiner 收到
`None` 时退回 Solo，等下次有新 sponsor 派发再切。两端 wire version 必须
对齐（postcard 严格，跨版本互操作通过 `WIRE_VERSION` 升版本拒连）。

## 4. 共享上下文 EventContext

每个事件 **必须** 携带的字段。SDK 接入层负责把这些字段拼到事件 properties
里，调用方不需要重复传。

```rust
pub struct EventContext {
    // 身份（分别独立持久化；analytics_device_id 与业务 DeviceId 完全 disjoint）
    pub anonymous_user_id: Uuid,
    pub analytics_device_id: Uuid,
    pub session_id: Uuid,

    // 应用版本
    pub app_version: String,        // crate version, e.g. "0.7.0-alpha.6"
    pub app_channel: AppChannel,    // alpha | beta | stable

    // 平台
    pub os: Os,                     // macos | windows | linux | ios | android
    pub os_version: String,
    pub arch: Arch,                 // x86_64 | aarch64 | ...
    pub locale: String,             // BCP-47, e.g. "zh-CN"
    pub timezone: String,           // IANA, e.g. "Asia/Shanghai"

    // 安装来源
    pub install_source: InstallSource,  // 见 §4.1

    // 启动状态
    pub is_first_run: bool,         // 仅首启 = true，后续 = false
    pub active_device_count: u32,   // 进程启动时一次性读取并缓存到 session，详见下方说明

    // Space（可选）
    pub space_id_hash: Option<String>,  // SHA-256(space_id), 不可逆

    // v2 跨设备 person 聚合（Phase 098）。`#[serde(skip)]` —— 不进 wire,
    // 仅作 sink 派生 distinct_id 的输入。
    // - Solo(uuid)         → distinct_id = uuid（= anonymous_user_id, v1 兼容）
    // - SpaceShared(uuid)  → distinct_id = uuid（= sponsor 派发的 space_person_id）
    pub analytics_person_id: AnalyticsPersonId,
}
```

**关于时间戳**：每个事件的 `timestamp` 由 sink 在 capture 时打（`Utc::now()`），
**不** 作为 EventContext 字段——context 是 session 级共享数据，timestamp 是
事件级数据。若使用 PostHog，SDK 会自动注入 `$timestamp` 字段，sink 不需要
显式处理。

**`active_device_count` 的语义**：进程启动时从配对状态读取一次，写入
`EventContext` 缓存，整个 session 期间不刷新。理由：

- 实时计算每事件都贵，且大多数 session 内设备数不会变化。
- "session 内新增设备"这件事本身有专门的 `pairing_succeeded` 事件可以
  追溯，不需要 `active_device_count` 实时更新。
- 长期常驻进程（数天）的偏差可接受——分析时按"事件发生在哪个 session"
  自然分桶。

### 4.1 InstallSource 取值

第一版固定枚举（避免开放字符串导致脏数据）：

```rust
pub enum InstallSource {
    V2ex,
    Reddit,
    HackerNews,
    Github,
    Twitter,
    Direct,
    Unknown,
}
```

获取策略（v1 简化）：

- **桌面**：安装包文件名带后缀（如 `UniClipboard-v2ex.dmg`）→ 解析得到。
  没有后缀 → `Unknown`。
- **官网下载**：URL 带 `?src=v2ex` → 写入 cookie / localStorage → 安装后
  通过 deeplink 回填。**v1 不做**，列入 v2。

### 4.2 哪些字段不放进 EventContext

- `ip_address`：永远不上传，由后端在 ingestion 入口丢弃（PostHog 的
  `disable_geoip` 即可）。
- `hostname` / `username`：永远不上传。
- `clipboard_content_*` / `file_path` / `file_name`：永远不上传。

## 5. 事件命名规范

### 5.1 格式

`{domain}_{action}_{state}`，全部 snake_case，不超过 64 字符。

| 部分 | 取值 | 示例 |
|---|---|---|
| domain | 业务域 | `pairing` / `sync` / `permission` / `app` |
| action | 动作 | `started` / `attempted` / `completed` |
| state | 终态（可选） | `succeeded` / `failed` / `cancelled` |

### 5.2 示例

| 推荐 | 不推荐 | 原因 |
|---|---|---|
| `pairing_succeeded` | `PairingSuccess` / `pair_ok` | 大小写、缩写、不够明确 |
| `sync_failed` | `sync_error` | "error" 与 Sentry 概念冲突 |
| `app_first_open` | `app_opened_first_time` | 过长 |

### 5.3 命名注意

- 一个事件名一旦上线 **不再重命名**——会破坏历史数据聚合。要演化时新建
  `*_v2` 事件，并在文档里标注前者 deprecated。
- 一个事件 **只代表一件事**。不要为了省事把"成功"和"失败"塞进同一个事件
  靠 properties 区分——后端漏斗查询会更复杂。

## 6. 隐私契约

### 6.1 永不上传

- 剪贴板原文（文本 / RTF / HTML / 图片像素 / 文件二进制）。
- 文件名原文、文件路径原文。
- 用户名 / hostname / 邮箱。
- 客户端原始 IP（由后端入口丢弃）。
- Sentry 事件已 redact 的字段，在 telemetry 一侧也禁止再次出现。

### 6.2 必须脱敏

| 字段 | 脱敏方式 | 备注 |
|---|---|---|
| `space_id` | SHA-256 → 取前 16 hex char | 不可逆，足够区分用户 |
| `peer_device_id` | 同上 | 同上 |
| `error_message` | 仅保留 `failure_reason` 枚举值 | 不传原始 message |

### 6.3 类型与区间化

文件大小、payload 大小、耗时 **只上报区间**，不上报精确值：

```rust
pub enum PayloadSizeBucket {
    Lt1Kb,        // < 1 KB
    Kb1To100,     // 1 KB ~ 100 KB
    Kb100ToMb10,  // 100 KB ~ 10 MB
    Gt10Mb,       // > 10 MB
}

pub enum LatencyBucket {
    Lt100ms,
    Ms100To500,
    Ms500To2s,
    S2To10,
    Gt10s,
}
```

例外：`sync_latency_ms` 这类需要做 p95 分析的字段，**可以** 上报精确数值。
精确数值本身不构成隐私泄露（不与内容关联）。

### 6.4 Opt-out（双开关）

**v1 决策：拆成两个独立开关**。理由：欧盟用户视角，"报错给你们"和"统计
我的使用习惯"是两件事；GDPR 友好的产品基本都拆。

| settings 字段 | 控制范围 | 默认值 | 当前状态 |
|---|---|---|---|
| `general.telemetry_enabled` | Sentry（错误 / breadcrumb / Logs） | `true` | 已存在，不改名 |
| `general.usage_analytics_enabled` | 本文档定义的产品 telemetry | `true` | **新增** |

**为什么不改名**：现有 `telemetry_enabled` 已经持久化在所有用户的 settings
文件里，重命名等于做一次迁移。保留原字段语义（= 错误上报），新加一个字段
即可，零迁移成本。文档与 UI 文案上把它表述为"错误与崩溃上报"。

**运行时门控**：

- 在 `uc-bootstrap` 启动时同步两个字段到对应的 process-wide gate：
  - `telemetry_gate::set_telemetry_enabled(...)` → Sentry（已存在）。
  - 新增 `analytics_gate::set_analytics_enabled(...)` → 产品 telemetry。
- `uc-webserver` 的 PUT /settings 处理器同步更新两个 gate，无需重启。
- 在 `uc-observability::analytics` 入口处先查
  `analytics_gate::is_analytics_enabled()`，再决定是否构造和发送事件。即：
  **关闭后连事件对象都不应该被构造**，避免误把内容序列化进内存。

### 6.5 调试可见性

- Dev 构建：事件 **额外** 打印到 stdout（`tracing::debug!`），方便核对。
- Release 构建：不打印，只发往 sink。
- 任何构建下都不写入本地日志文件——否则日志被收走会绕过隐私契约。

## 7. v1 事件清单（Activation + Reliability）

### 7.0 系统事件（Phase 098 / v2 跨设备 person 聚合）

PostHog 标准系统事件，由 sink 在身份切换时显式触发，与业务事件平行。
不进入 funnel / retention 报表，但是 PostHog person 合并 / group analytics
机制的关键。

| 事件名 | 触发时机 | 顶层 | properties |
|---|---|---|---|
| `$identify` | distinct_id 变化时（A1 setup_completed / A2 pairing_succeeded / switch_space commit / 重置 telemetry） | `distinct_id = new_distinct_id` | `$anon_distinct_id = old_distinct_id`, `$set` (可选), `$set_once` (可选) |
| `$groupidentify` | A1 setup_completed 之后立即一次（写入新 Space group 的 `created_at` + `device_count=1`） | `distinct_id = 当前 ctx 的 person id` | `$group_type = "space"`, `$group_key = space_id_hash`, `$group_set = {...}` |

**约束**：
- `$identify` 只在 distinct_id 真正 **变化** 时发一次，不是每事件都发。
- `$anon_distinct_id` 必须在 `properties` 内、**不** 在顶层（PostHog alias 协议硬要求）。
- 调用方义务：先把新 distinct_id 写入 EventContext + 持久化 → 再调
  `analytics.identify(...)` → 再发后续业务事件。这保证后续业务事件已经按
  新 person 上报。
- 失败语义：fire-and-forget，schema doc §10 已允许 < 1% 丢失。罕见情况下
  服务端没收到 alias，老 person 与新 person 不会合并。

### 7.1 Activation

| 事件名 | 触发时机 | 关键 properties |
|---|---|---|
| `app_first_open` | `is_first_run == true` 时进程启动 | （仅 EventContext） |
| `app_opened` | 每次进程启动；`compose_event_context` 在 `set_global_event_context` 之后 emit 一次（PostHog `$pageview` / `$screen` 的桌面端等价物，DAU / WAU / MAU / 留存曲线的数据源） | （仅 EventContext） |
| `setup_started` | 引导页第一帧渲染 | `entry`: `first_run` \| `manual` |
| `device_name_set` | 用户提交设备名 | `name_length_bucket`: `Lt8` \| `8To16` \| `Gt16` |
| `pairing_started` | 用户点击配对 | `method`: `qr` \| `code` \| `discovery` |
| `pairing_succeeded` | 双端握手完成 | `method`, `peer_os`, `duration_ms` |
| `pairing_failed` | 配对中断或超时 | `method`, `failure_reason`（见 §7.4，使用 `PairingFailureReason` 而非 `FailureReason`） |
| `first_clipboard_sync_attempted` | 首次同步发起 | `direction`: `outbound` \| `inbound` |
| `first_clipboard_sync_succeeded` | 首次同步对端确认 | `direction`, `peer_os`, `transport_type`, `duration_ms` |
| `first_file_sync_succeeded` | 文件传输已支持时首次成功 | `peer_os`, `transport_type`, `payload_size_bucket` |
| `setup_completed` | A1 第 7 步 `SetupStatus.has_completed = true` 落地之后 | `has_paired_in_same_flow`: bool, `duration_ms_since_setup_started`: Option&lt;u32&gt;（None 不上 wire） |
| `space_unlocked` | A2 `unlock_space.execute` 成功分支（每次 daemon 重启的可靠性 anchor） | （仅 EventContext） |
| `space_unlock_failed` | A2 `unlock_space.execute` 失败分支；pre-condition `SetupNotCompleted` **不** 上报（不属于"用户能不能继续用产品"语义） | `failure_reason`: `UnlockFailureReason`（见 §7.5） |
| `clipboard_entry_captured` | `clipboard_capture::execute_with_origin` 成功路径，按 `origin` 严格过滤 | `origin`: `system_watcher` \| `manual_restore`, `payload_type`, `payload_size_bucket` |

**`clipboard_entry_captured` 红线**：`ClipboardChangeOrigin::RemotePush`（入站同步写本地剪贴板）**禁止** emit，否则会与入站事件双计、污染 DAU。
mapping：`LocalCapture` → `system_watcher`；`LocalRestore` → `manual_restore`（当前路径在 use case 入口短路 return None，实际不会触发，留 mapping 以便未来扩展）；`RemotePush` → 不 emit。

### 7.2 Reliability

可靠性事件家族：`sync_attempted` / `sync_succeeded` / `sync_failed` / `sync_deferred`。

`sync_attempted` 在 dispatch 之前固定发一次（每个 peer 一条），并与
`sync_succeeded` / `sync_failed` / `sync_deferred` 形成 1:1 配对。前三者共享
`SyncEventProps`；`sync_deferred` 使用 `SyncDeferredProps`（见下文）。

`SyncEventProps`：

```rust
pub struct SyncEventProps {
    pub direction: Direction,           // outbound | inbound
    pub payload_type: PayloadType,      // text | image | file
    pub payload_size_bucket: PayloadSizeBucket,
    pub transport_type: TransportType,  // local | p2p_direct | relay | fallback_cloud
    pub peer_os: Option<Os>,            // 已知则填，不要因为缺失就丢事件
    pub sync_latency_ms: Option<u32>,   // 仅成功事件携带
    pub failure_reason: Option<FailureReason>,  // 仅失败事件携带
    pub failure_stage: Option<SyncFailureStage>, // 仅失败事件携带
}
```

`sync_failed` 表示一次同步相关尝试失败，不等同于用户感知的最终失败。
dashboard 计算"最终同步失败率"时不应直接统计所有 `sync_failed`；应只统计
`failure_stage = terminal_delivery`，或统计明确的终态策略失败（例如
`failure_stage = local_policy`）。

另外有一个非失败事件：

```rust
pub struct SyncDeferredProps {
    pub direction: Direction,
    pub payload_type: PayloadType,
    pub payload_size_bucket: PayloadSizeBucket,
    pub peer_os: Option<Os>,
    pub defer_reason: SyncDeferReason,
}
```

`sync_deferred` 表示发送前就已知目标不可用，本次不可达不计入用户感知失败口径。
当前用于"目标设备已知离线，仍尝试发送但连接不上"的情况。

`sync_deferred` 与 `sync_attempted` 始终成对出现（attempted 在 dispatch 之前
固定发送一次）。dashboard 端聚合关系：

- `attempted = succeeded + failed + deferred`
- 用户感知尝试 = `attempted - deferred`
- 用户感知失败率分子使用 `failure_stage = terminal_delivery`，分母使用
  `attempted - deferred`

不带 `transport_type`：deferred 时本次没有真实发送，记录任何 transport 都是
误导性数据。如果未来要标注"原计划的"transport，请单独命名字段以避免与
`sync_attempted` / `sync_succeeded` / `sync_failed` 上"实际使用的"transport
混淆。

### 7.3 FailureReason 枚举（sync_failed 专用）

```rust
pub enum FailureReason {
    PeerOffline,
    Timeout,
    PermissionDenied,
    NetworkError,
    FileTooLarge,
    ClipboardPermission,
    EncryptionMismatch,
    Unknown,            // 兜底，但占比应被监控并定期拆细
}
```

### 7.3a SyncFailureStage 枚举（sync_failed 专用）

```rust
pub enum SyncFailureStage {
    ImmediateSend,      // 即时发送尝试失败，通常可进入 pending / retry
    LocalPolicy,        // 本机策略在发送前拒绝，如 payload 过大
    TerminalDelivery,   // pending / retry 耗尽后的终态投递失败
}
```

### 7.3b SyncDeferReason 枚举（sync_deferred 专用）

```rust
pub enum SyncDeferReason {
    PeerKnownOffline,   // 发送前 presence 已知对端离线
}
```

**这是一个开放枚举**：`PeerKnownOffline` 是当前唯一变体，但 deferred 概念覆盖
所有"预期不可用 / 本次不应计入失败口径"的场景。未来可能加入的扩展点，例如：

- 对端不在白名单 / send 已被禁用 → 当前在 dispatch 前就被过滤，不会进入 spawn
- 本地策略主动跳过（低电量、不计费网络、暂停同步）
- 网络节流 / 配额耗尽

新增 reason 前先确认：该原因是否真"不应计入用户感知失败率"。如果只是"用户感知
失败的另一种解释"，应进 `FailureReason` + 合适的 `SyncFailureStage`，而不是
deferred。

当前 outbound dispatch 路径的采样规则（每行还隐含一个伴随的 `sync_attempted`，
在 dispatch 之前发出，下表只列出 dispatch 完成后的结果事件）：

| 事件 | 条件 | 关键字段 | 解释 |
|---|---|---|---|
| `sync_deferred` | 发送前 `PresencePort::current_state == Offline`，且 dispatch 仍返回 `Offline` | `defer_reason = peer_known_offline` | 对端本来就已知离线，连接不上是预期不可用，不计入尝试失败 |
| `sync_failed` | 发送前不是已知离线，但 dispatch 返回 `Offline` | `failure_reason = peer_offline`, `failure_stage = immediate_send` | 发送前没有明确离线 verdict，仍不可达；用于网络/恢复诊断，不计入最终失败率 |
| `sync_failed` | dispatch 返回 `Io` / peer wire 边界拒绝 | `failure_reason = network_error`, `failure_stage = immediate_send` | 已进入发送路径但连接 / I/O / wire 边界失败，等待恢复或重试口径处理 |
| `sync_failed` | 本机策略拒绝 | `failure_reason = file_too_large`, `failure_stage = local_policy` | 本机策略确定该 payload 不能按当前通道发送 |
| `sync_failed` | 本机内部错误 | `failure_reason = unknown`, `failure_stage = immediate_send` | 需要排查的内部错误口径 |

`Unknown` 占比是 **架构债务指标**：高于 5% 时就要专门排查并新增枚举值。

> **Domain-specific failure enums**：v1 起 `pairing_failed` 与 `sync_failed`
> 各自使用独立的 failure_reason 枚举（见 §7.4）。不同 domain 的失败语义不重叠（pairing 关心 passphrase / sponsor 决断，sync 关心 transport / payload），共享一份 enum 会让 funnel 漏点信号在跨 domain dashboard 中误聚合。后续每个新 domain（setup / search 等）若需要 failure reason，按相同模式新建专用 enum。

### 7.4 PairingFailureReason 枚举（pairing_failed 专用）

```rust
pub enum PairingFailureReason {
    InvitationNotFound,           // rendezvous 没有该邀请（typo / 过期 / 已被消费）
    InvitationExpired,            // 邀请 TTL 超期
    SponsorUnreachable,           // 无法与 sponsor 建立连接
    ServiceUnavailable,           // rendezvous 服务不可达
    PassphraseMismatch,           // 口令错或 keyslot 校验失败
    CorruptedKeyMaterial,         // keyslot 解析 / 版本故障
    DeviceNameRequired,           // 缺少设备名
    SponsorRejectedInvitation,    // sponsor 未识别 code（race / 过期）
    SponsorDeclined,              // sponsor 主动拒绝
    SponsorTimedOut,              // sponsor TTL 先触发
    SponsorInternal,              // sponsor 端 persist / settings 等内部错误（reject(Internal)）
    Timeout,                      // 本机等待响应超时
    ConnectionLost,               // 握手中途 transport 断线
    Internal,                     // 兜底（本机 admit / trust / setup_status persist / 序列化等内部错误）
}
```

变体与 `RedeemPairingInvitationError`（业务错误）一一映射，使得 funnel
分析能直接定位漏点的具体业务原因。`Internal` 占比同样应监控——高于 5%
说明本机持久化层不稳定。

### 7.5 UnlockFailureReason 枚举（space_unlock_failed 专用）

```rust
pub enum UnlockFailureReason {
    PassphraseMismatch,   // 口令错
    KeyringUnavailable,   // 系统 keyring 不可访问（保留枚举槽位，当前无独立 SpaceAccessError 变体；走 Internal 兜底）
    KeyslotCorrupted,     // keyslot 解析 / 版本故障
    SpaceNotFound,        // 当前 profile 没有可解锁的 space（adapter 报 NotInitialized）
    Internal,             // 兜底：setup_status 读取失败、未分类的 SpaceAccessError 等
}
```

mapping：`SpaceAccessError::WrongPassphrase` → `PassphraseMismatch`；`NotInitialized` → `SpaceNotFound`；`CorruptedKeyMaterial` → `KeyslotCorrupted`；其它（`Internal` / 未分类）→ `Internal`。pre-condition 失败 `SetupNotCompleted` **不** emit（语义不属于"用户能不能继续用产品"）。`Internal` 占比 > 5% 视为本机持久化层不稳定，按 §7.3 末尾原则专门排查。

### 7.6 Mobile Sync 事件清单

P1 落地（2026-05-15）。LAN HTTP 协议（iPhone Shortcut 客户端）三件套：

| 事件名 | 触发位置 | 关键 properties |
|---|---|---|
| `mobile_device_registered` | `mobile_sync/register_device.rs::execute` 成功（`device_repo.save` 之后） | （仅 EventContext） |
| `mobile_clipboard_synced` | `mobile_sync/apply_incoming.rs::execute` SyncDoc arm 的 `Applied` outcome | `direction`: `inbound` \| `outbound`（v1 恒为 `inbound`）, `payload_size_bucket` |
| `mobile_auth_failed` | `mobile_sync/authenticate_basic.rs::execute` 失败分支 | `failure_kind`: `MobileAuthFailureKind`（见 §7.7） |

**`mobile_clipboard_synced` 红线**：仅 `Applied` outcome emit；`Buffered`（两步 PUT 协议中间态）/ `DuplicateSkipped`（命中本机 dedup）/ `DecodeFailed` / 应用层错误一律不上报。沿用 `clipboard_entry_captured` 防 RemotePush 双计的红线哲学——重复埋点会让 dashboard 频率口径双计。

**v1 `direction = Inbound` 恒值**：`GetLatestMobileSyncDoc` 出站埋点延后到 v2。原因——iPhone 客户端的轮询频率会让 outbound 量级比 inbound 高一个数量级，需要单独评估采样口径。`direction` 字段保留枚举槽位是为了 v2 直接扩展（§8：新增 property 取值非破坏式演化，dashboard 零迁移）。

**`mobile_auth_failed` happy path 沉默**：401 响应对外不区分原因（侧信道防御），但 telemetry 仅在失败路径 emit；"成功"信号由 `mobile_clipboard_synced` 间接覆盖，重复埋点会让 401 错误率分母失真。

落地备注（保留以便回溯）：

- `mobile_device_registered`：emit 在 `device_repo.save` 成功之后、QR 渲染之前——后续 QR 渲染失败仍保留事件（schema 主目录说明"已登记但拿不到 install URL"是孤儿记录路径，但设备 IS registered，telemetry 反映事实）。
- `mobile_clipboard_synced`：`payload_size_bucket` 用 `PayloadSizeBucket::from_bytes(snapshot.total_size_bytes())`。BufferFile arm 不 emit（中间态），SyncDoc DuplicateSkipped 不 emit（dedup 双计红线）。
- `mobile_auth_failed`：6 个失败分支统一走 `emit_failure(kind)` 薄包装；happy path **不** emit——产品视角"成功"由 `mobile_clipboard_synced` inbound 间接覆盖。

### 7.7 MobileAuthFailureKind 枚举（mobile_auth_failed 专用）

```rust
pub enum MobileAuthFailureKind {
    UnknownUser,        // 头解析失败 / base64 损坏 / find_by_username 为 None
    PasswordMismatch,   // verify=false + InvalidPhc（PHC 损坏在产品视角与"真实密码错"等价）
    Internal,           // 仓储 Storage 错误 + hasher Internal
}
```

mapping：

- 头解析失败 → `UnknownUser`（与"用户名不存在"在 telemetry 上等价：iPhone 客户端表现都是 401）
- `find_by_username == None` → `UnknownUser`
- hasher `verify == Ok(false)` → `PasswordMismatch`
- hasher `Err(InvalidPhc)` → `PasswordMismatch`（PHC 字符串损坏的兜底；归 PasswordMismatch 让 dashboard 不会被一种罕见 adapter 故障污染 Internal 占比）
- repository `Err(Storage)` → `Internal`
- hasher `Err(Internal)` → `Internal`

与 `sync` / `pairing` / `unlock` 失败枚举不共享——§7.3 末尾 domain-specific failure enum 原则。`Internal` 占比 > 5% 视为本机持久化层 / hasher adapter 不稳定。

**槽位未使用**：`RateLimited` 不在本枚举内——v1 LAN listener 尚未实装速率限制；若未来加 rate limit，独立新增变体（非破坏式扩展）。

### 7.8 Update Lifecycle 事件清单

P2 落地（2026-05-21，与 update scheduler / 系统通知同 PR）。覆盖"后端检查 → 系统通知 → 用户进入对话框 → 决策（下载 / 安装 / 放弃）"全漏斗，服务"为什么用户不更新"的诊断需求。

| 事件名 | 触发位置 | 关键 properties |
|---|---|---|
| `update_check_performed` | `update_scheduler::tick`（`startup` / `scheduled` / `window_show` 三 source）+ `commands/updater.rs::check_for_update`（`manual` source） | `source`: `startup` \| `scheduled` \| `manual` \| `window_show`, `outcome`: `UpdateCheckOutcome`, `failure_kind`: `Option<UpdateFailureKind>`, `install_kind`: `InstallKind` |
| `update_notification_shown` | `update_scheduler::notification::send_update_notification` 返回后（去重通过、`tauri-plugin-notification` 调用完成） | `version`: `String`, `delivery_status`: `NotificationDeliveryStatus`, `install_kind`: `InstallKind` |
| `update_dialog_opened` | 前端 `setUpdateDialogOpen(true)` 或 `setPackageManagerDialogOpen(true)` 之后，经 `capture_update_ui_event` Tauri command 转送 | `source`: `DialogOpenSource`, `phase`: `available` \| `downloading` \| `ready`, `install_kind`: `InstallKind` |
| `update_dismissed` | 前端 AlertDialog Cancel / Content close / PackageManagerDialog close 之后，经 `capture_update_ui_event` 转送 | `phase`: `available` \| `ready`, `source`: `DismissSource` |
| `update_action_invoked` | `commands/updater.rs::download_update` / `install_update` Tauri command body（manual）+ `update_scheduler` 自动下载分支（auto） | `action`: `UpdateAction`, `outcome`: `started` \| `succeeded` \| `failed` \| `cancelled`, `error_kind`: `Option<String>` |

**红线**：

- **同版本通知只发一次**：`update_notification_shown` 由 `last_notified_update.json`（`AppPaths::app_data_root_dir` 下，按 `UpdateChannel` 维度的 `HashMap<Channel, String>`）去重；重复版本的后续轮询循环 **不**emit 该事件——保证"通知到达率"分母准确。
- **`autoCheckUpdate=false` 全链路静默**：该 setting 同时关闭启动检查 / 周期检查 / 通知发送，这批用户在 PostHog 上完全没有 `update_check_performed` 事件——分母自然不含他们，与"漏斗失效定位"诉求一致。
- **setup 期间静默**：`update_scheduler` 等 `SetupStatus.has_completed == true` 后才启动（polling 间隔 30s），避免首次安装 / welcome 流程被任何 update 事件污染分母。
- **scheduler-only 的 source 值**：`startup` / `scheduled` / `window_show` 仅由 `update_scheduler` emit；命令行 / UI 触发的"检查更新"按钮 emit `manual`。两类 source **绝不** 混用同一调用路径——避免"用户主动检查"与"后台检查"分子错位。
- **版本字符串相等比较**：去重与 dashboard slicing 都按字符串相等处理，不引入 semver。channel 切换（如 stable → alpha）导致的版本号变化按"新版本"语义处理，会重新通知一次。

落地备注（保留以便回溯）：

- `update_check_performed`：scheduler 触发由 scheduler 自身 emit；`check_for_update` Tauri command body 只为 `source = manual` 路径 emit；内部抽出的 inner 函数 `do_check_for_update` **不** emit 任何事件，由 caller 决定 source。
- `update_action_invoked`：未引入 `source` 字段——`action: download_bg` 与 `action: install` 已把 lifecycle stage 表达清楚，再扩 source 会与 `update_dialog_opened.source` 语义冲突。scheduler 触发的 auto-download 视为合法 caller，与 manual 走同一事件、同一 properties 形态（caller 类型由 `EventContext.session_id` 与时间序列推断）。
- `update_notification_shown.delivery_status`：`PermissionDenied` 与 `SendFailed` 都视为"事件本身发生但未必到达用户" — schema 上保留 emit，dashboard 端按 `delivery_status = sent` 计算到达率分子。
- `install_kind` 出现在多个事件里：scheduler 任务启动时一次性 probe + 缓存；前端事件由 backend 在 `capture_update_ui_event` 接收时反查 cache 后注入，前端不传也不需要知道。
- `update_dismissed.source.package_manager_dialog_closed`：Linux deb/rpm 路径专属（弹出 `PackageManagerUpdateDialog` 而不是 `AlertDialog`）；其他平台不会出现此值。
- 通知点击 callback 在 Linux 部分桌面环境（Sway / dwm）可能不可用——此时通知仍 emit `update_notification_shown`，但用户没有 `update_dialog_opened.source = notification` 后续事件；这是预期降级路径，由 dashboard 端按平台切片判断。

### 7.9 Update Lifecycle 配套枚举

```rust
pub enum UpdateCheckOutcome {
    Available,     // 检查成功，发现新版本
    UpToDate,      // 检查成功，已是最新
    Failed,        // 检查失败（详见 failure_kind）
}

pub enum UpdateFailureKind {
    Network,       // 连接失败 / DNS / TLS 握手
    HttpError,     // 4xx / 5xx 响应
    ParseError,    // manifest JSON 解析或 minisign 校验失败
    Other,         // 其他（含 panic 兜底）
}

pub enum NotificationDeliveryStatus {
    Sent,              // tauri-plugin-notification 成功投递给 OS
    PermissionDenied,  // macOS / Windows 用户拒绝通知权限
    SendFailed,        // 投递失败（Linux 无 notification daemon / 其他平台错误）
}

pub enum DialogOpenSource {
    Notification,      // 用户点击系统通知打开
    SidebarIcon,       // 用户点击 sidebar 的更新指示器
}

pub enum DismissSource {
    DialogLater,                 // "稍后" 按钮
    DialogClosed,                // X / ESC / 点击外部关闭
    PackageManagerDialogClosed,  // Linux deb/rpm 路径专属
}

pub enum UpdateAction {
    DownloadBg,        // 用户点 "后台下载" 或 scheduler 自动下载触发
    Install,           // 用户点 "安装并重启"
}

pub enum InstallKind {
    Macos,             // .app（走 Tauri updater）
    Windows,           // .exe / .msi（走 Tauri updater）
    AppImage,          // Linux AppImage（走 Tauri updater）
    Deb,               // Debian/Ubuntu 包（走 PackageManagerDialog 引导）
    Rpm,               // RHEL/Fedora 包
    Unknown,           // probe 失败兜底（含 Snap / COPR / 源码构建等）
}
```

**域内独立**：以上枚举不与 `mobile_sync` / `pairing` / `unlock` / `sync` 失败枚举共享——延续 §7.3 末尾的 domain-specific failure enum 原则。`UpdateFailureKind::Other` 占比 > 10% 视为 manifest / 网络栈不稳定信号。

**`InstallKind` 与 Tauri command 共享 wire 形态**：`InstallKind` 已存在于 `commands/updater.rs` 作为 `get_install_kind` Tauri command 返回值（serde `rename_all = "lowercase"`）。本节定义的 telemetry 侧 enum **必须** 与之保持 wire 等价——`macos` / `windows` / `appimage` / `deb` / `rpm` / `unknown`。任何一侧扩枚举必须同步另一侧；变更走 §8 演化策略（重命名禁止，新增允许）。

**未使用槽位**：

- `UpdateFailureKind::SignatureMismatch` 不在 v1——minisign 校验失败由 `ParseError` 兼容；若 dashboard 显示 `ParseError` 占比异常再独立拆分。
- `NotificationDeliveryStatus::PartiallySent` 不在枚举内——`tauri-plugin-notification` 没有"部分成功"语义，三态足够。
- `InstallKind::Snap` / `Copr` 不在 v1——0.11.0-alpha.1 的 Linux 安装脚本支持这两个包源，但运行时 binary 仍归类 `Unknown`（dpkg-query / rpm 都不会认领它们）；若 dashboard 显示 `Unknown` 占比 > 10% 且 Linux 用户占比有相关性，再独立拆分。

## 8. Schema 演化策略

| 变更类型 | 处理方式 |
|---|---|
| 新增事件 | 直接加，不影响现有数据 |
| 新增 property | 直接加，旧事件没有该字段时后端按 `null` 处理 |
| 重命名事件 / property | **禁止**。新建 `*_v2`，文档标注旧的 deprecated |
| 删除事件 | 文档标注 deprecated，至少保留 90 天再下线（保证历史 dashboard 可用） |
| 改变 property 语义（如区间边界） | 必须新建 `*_v2` |

每次 schema 变更必须更新本文件 + 在 `docs/changelog/*.md` 里登记。

## 9. 类型定义落地位置（建议）

```/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-observability/src/analytics/
mod.rs        // pub use 与 sink trait
context.rs    // EventContext 与构造工厂
events.rs     // TelemetryEvent 枚举或 newtype 包装
ids.rs        // anonymous_user_id / analytics_device_id 持久化
buckets.rs    // PayloadSizeBucket / LatencyBucket 等区间类型
```

放 `uc-observability` 而不是 `uc-core` 的理由：

- `uc-core` 不应感知"数据要上传"这件事。
- 但 **事件类型本身** 是 pure data，可被 use case 直接构造——这与 hexagonal
  原则不冲突，因为 use case 调的是一个 `AnalyticsPort` trait（子任务 2 定义），
  port 的入参类型可以驻在 infra 侧。

## 10. 开放问题裁决记录

四项决策于 2026-05-09 与维护者敲定：

1. **Telemetry 开关拆成两个**（§6.4）。保留 `general.telemetry_enabled`
   作为 Sentry 开关不改名；新增 `general.usage_analytics_enabled` 控制
   本文档定义的产品 telemetry。零迁移成本。
2. **`active_device_count` 进程启动时读取一次**，缓存在 `EventContext`
   整个 session 不刷新（§4 末尾说明）。session 内的设备增减由
   `pairing_succeeded` 等事件本身覆盖。
3. **留存口径设备级**，Space 级聚合延后到 v2（§3.4）。`space_id_hash` 维度
   保留在 EventContext 中，v2 切片即可，不需要重发历史事件。
4. **后端选 PostHog Cloud（US ingestion endpoint，2026-05-09 实际注册区域）**。理由：
   - 早期 < 10 用户，self-host 维护成本不划算。
   - US 与 EU region 走相同的隐私模型（SOC 2 + GDPR DPA + SCC），
     物理驻留不同但合规等价；§6 的隐私契约（IP 不上传、`disable_geoip=true`、
     字段脱敏）与 region 选择正交。
   - 免费额度（每月 100 万事件）按当前规模一年都用不完。
   - schema 已与 SDK 解耦（§9），将来迁移 self-host 或切 EU region
     只换 sink host 不动事件。
   - 鉴于 Cloud 是已知第三方且 `anonymous_user_id` 本身已是无 PII 的
     UUIDv7，**不需要在客户端对 ID 再做二次哈希**——再哈希反而会让
     PostHog 的"按 user 聚合"功能失效（它需要稳定 distinct_id）。

后续若决策变更（如改投 self-host），更新本节并在 `docs/changelog/` 登记。

### 10.1 PostHog Cloud 接入实务（v1）

本节记录 PostHog 真实接入时的几个落地决策，避免规范层与实务层混淆——
schema 部分（§3 ~ §9）任何情况下都是单一真相源；本节是"v1 怎么把事件
送出去"。后续切 self-host / 切 SDK 只更新本节，不动 §3 ~ §9。

#### Key 注入策略

PostHog project key（`phc_*`）通过环境变量 `POSTHOG_PROJECT_KEY` 三级回退
注入，与 `SENTRY_DSN` 同位（参考 `uc-bootstrap/src/tracing.rs`）：

1. **运行时 env 优先**：`std::env::var("POSTHOG_PROJECT_KEY")`。覆盖
   编译期烤入值；dev 自部署 / PR review build 用此机制 opt-in。
2. **编译期 `option_env!` 兜底**：CI release build 时把 secret 通过
   `${{ secrets.POSTHOG_PROJECT_KEY }}` 注入到 build env，`option_env!`
   把 secret 烤进 binary。终端用户机器上不会设这个 env。
3. **都缺**：release path 走 `Gated(NoopAnalyticsSink)` + 一条 `info!`，
   不阻塞 daemon / GUI 启动。"没配 key"是合法配置。

空字符串等价于"未设置"——`${{ secrets.X }}` 在 secret 未注入时渲染为空，
绝不能用空 api_key 调 PostHog 触发整批 401。

#### Endpoint 与 region

```
POST https://us.i.posthog.com/i/v0/e/
Content-Type: application/json
{
  "api_key": "phc_xxx",
  "event": "<event_name>",
  "distinct_id": "<anonymous_user_id>",
  "properties": {
    // §4 EventContext 字段（vendor-neutral，schema 单一真相源）
    "anonymous_user_id": "...",
    "analytics_device_id": "...",
    "session_id": "...",
    "app_version": "...",
    "app_channel": "...",
    "os": "...",
    "os_version": "...",
    "arch": "...",
    "locale": "...",
    "timezone": "...",
    "install_source": "...",
    "is_first_run": true,
    "active_device_count": 2,
    "space_id_hash": "...",

    // event-specific 字段
    "<event 自身 properties>": "...",

    // PostHog 标准 $-prefix 字段（仅 PosthogSink 注入，详见 §10.1 字段映射）
    "$device_id": "<analytics_device_id>",
    "$session_id": "<session_id>",
    "$lib": "uniclipboard-rust",
    "$lib_version": "<app_version>",
    "$geoip_disable": true,
    "$set": { "app_version": "...", "os": "...", "active_device_count": 2, ... },
    "$set_once": { "initial_app_version": "...", "initial_install_source": "...", ... },

    // v2 跨设备 person 聚合（Phase 098）。仅当 ctx.space_id_hash 非空时出现：
    "$groups": { "space": "<space_id_hash>" }
  },
  "timestamp": "2026-05-09T12:34:56+00:00"
}
```

**v2 distinct_id 切换**（Phase 098）：顶层 `distinct_id` 由
`ctx.analytics_person_id.as_uuid()` 派生：

- `Solo(uuid)` → `distinct_id = uuid`（= `anonymous_user_id`，与 v1 byte-for-byte 兼容）
- `SpaceShared(uuid)` → `distinct_id = uuid`（= sponsor 派发的 `space_person_id`）

properties 顶层仍保留 `anonymous_user_id` flat 字段，dashboard 可同时按
设备级 anonymous 切片。详见 §3.4。

US ingestion endpoint 是 §10 决策第 4 项的 2026-05-09 实际注册区域。
切 EU 或 self-host 实例只换 endpoint URL（`PosthogSink::with_endpoint`
是测试 / 后续迁移入口），事件 wire 形态零改动。

#### PostHog 标准 `$`-prefix 字段映射

PostHog 服务端识别带 `$` 前缀的 property 解锁三类能力：Person ↔ Device /
Session funnel & Replay、按客户端来源过滤、按 Person 维度切片。自写
HTTP client 没有 SDK 自动注入，需要在 `PosthogSink::build_capture_body`
手动构造。

**重要分层约束**：`$`-prefix 字段 **只在 PosthogSink 内部注入**，
`build_event_payload`（vendor-neutral 共享层）以及 §4 `EventContext`
**绝不** 感知它们。后续切 self-host PostHog 仍走原 wire 形态；切别的
后端（Mixpanel / 自建）只需在新 sink 里写一份等价的字段翻译，§3 ~ §9
schema 不动。

字段映射表：

| PostHog 字段 | 来源 | 解锁能力 |
|---|---|---|
| `$device_id` | `analytics_device_id` | Person ↔ Device 关联，控制台按设备维度切片 |
| `$session_id` | `session_id` | Session funnel、未来接 Session Replay |
| `$lib` | 固定 `"uniclipboard-rust"` | 控制台按客户端来源过滤流量 |
| `$lib_version` | `app_version` | 同上，按版本过滤 |
| `$geoip_disable` | 固定 `true` | 见下方"`disable_geoip` 等价语义" |
| `$set` | EventContext 中 9 个"可变当前状态"字段 | Person Properties 当前快照 |
| `$set_once` | 4 个"安装期不变量" → `initial_*` 前缀 | Person 首次出现时写入，捕获迁移信号 |
| `$groups` | `{ "space": ctx.space_id_hash }`（仅非空时） | v2 group analytics：dashboard 按 Space 维度切片留存 |

**`$set` 与 `$set_once` 的拆分理由**：PostHog 控制台"按 person 切片"
（如"macOS 用户的留存"）读的是 Person Property，**不是** event property。
若所有字段都平铺在 event property，按 person 维度查询要在每个事件
property 上做 distinct/聚合，慢且贵。

- `$set`：每事件覆盖，PostHog 端 person profile 永远反映最近一条事件
  的快照。放可变字段：`app_version` / `app_channel` / `os` / `os_version`
  / `arch` / `locale` / `timezone` / `active_device_count` / `space_id_hash`。
- `$set_once`：仅 person 首次出现时写入，后续被服务端忽略。放安装期
  不变量：`initial_app_version` / `initial_app_channel` / `initial_os` /
  `initial_install_source`。

dashboard 上可同时看到 `$set.os` vs `$set_once.initial_os`，捕获跨平台
迁移信号。

**`$set` 不接受 `null`**：PostHog 把 property `null` 当显式清空指令，
会把已有 person property 抹掉。`space_id_hash` 等 optional 字段在源头为
`None` 时，`build_set_snapshot` 直接跳过该 key 而非写入 `null`。
`$groups` 字段同款语义：Solo 状态下 `space_id_hash` 缺失时整个 `$groups`
key 不进 wire（不写空对象 / null），避免 PostHog 把空对象当"清空 group 归属"。

**Flat-name 字段同时保留**：`anonymous_user_id` / `analytics_device_id` /
`session_id` / `app_version` 等仍在 properties 顶层。理由——
(1) §4 已是 wire 契约，删字段破坏向后兼容；
(2) StdoutSink 输出复用同一个 payload，flat 形态人类更易读；
(3) `$`-prefix 字段是非破坏性扩展，wire 膨胀 ~200 字节，schema doc §10
免费额度内可忽略。

后续迁移决策（>1k 用户后）：若 ingestion 量成为问题，可在 PosthogSink
内单独优化，仍不动 §4。

#### HTTP client 选择

v1 不用 `posthog-rs` SDK，自写 reqwest 0.12 + rustls(ring) ~100 行
minimal client。根因在 cargo 依赖图：

- `posthog-rs 0.7` 的 `Cargo.toml` hardcode `reqwest = "0.13.2"` +
  `features = ["rustls"]`；reqwest 0.13 的 rustls feature 隐式选
  `aws-lc-rs`（C 库 + CMake 编译）。
- uc-cli 走 musl 静态编译（"零 C 工具链"硬约束），sentry 已为此用
  ureq + ring 而非 reqwest 0.13（见 `uc-bootstrap/Cargo.toml` 注释）。
- cargo features unification 是 workspace 级 union，无法用
  `optional` / feature gate 把 uc-cli 排出依赖图。

失去 SDK 的批量 / retry / feature flag 能力，但 v1 只用 capture POST
单一路径，schema §10 已允许 < 1% 事件丢失，自写 client 综合成本最低。
若实测丢失率 > 5% 或用户量 > 1k 后 POST 量过高，再重启"SDK vs 自建队列
vs HTTP/3"评估；那时切换只动 sink 实现，schema 与 wire 形态零改动。

#### Fire-and-forget 与进程退出

`PosthogSink::capture` 内部走 `tokio::spawn` fire-and-forget
（`AnalyticsPort::capture` 同步签名 + 异步 reqwest 的唯一干净桥）。
HTTP 失败仅 `tracing::warn!`，不传播给业务。

v1 **不挂** 进程退出 flush 钩子。理由：

- 自写 client 无应用级队列；reqwest 单次 POST 一旦 spawn 就走自己的
  网络生命周期。
- `app_first_open` / `setup_started` 等 onboarding 起点丢失影响 funnel
  起点统计——但 schema §10 已允许 < 1% 丢失。
- 后续若实测 onboarding 起点丢失率明显高于 1%，再补
  `tauri::App::on_exit` 钩子做 best-effort drain。

#### `disable_geoip` 等价语义（IP 字段处理）

§6.1 明确"客户端原始 IP 永不上传"。posthog-rs SDK 提供 `disable_geoip`
配置项；自写 client 通过两道防线实现等价契约：

1. **不主动 inject IP-derived 字段**——request 从客户端发出时不附带
   user IP（reqwest 默认行为），客户端代码也从未读取或上报本机 IP。
2. **每条事件 `properties` 默认置 `"$geoip_disable": true`**——
   `inject_posthog_standard_fields` 无条件写入。PostHog 服务端的
   GeoIP 推断基于请求 TCP 源 IP，若不显式关闭会反推
   `$geoip_country` / `$geoip_city` 并落到 person property，与
   §6.1 契约直接冲突。

两道防线缺一不可：第 1 道防止"客户端主动泄露 IP 衍生字段"，第 2 道
防止"服务端按请求 IP 反推地理位置"。`build_capture_body_disables_geoip_by_default`
单测守住第 2 道防线在所有事件上生效。

#### CI secret 注入（待 7b-4 落地）

`POSTHOG_PROJECT_KEY` 与 `SENTRY_DSN` / `VITE_SENTRY_DSN` 同属 release
build 时间注入的 secret 列表。CI 注入位置（计划）：
`.github/workflows/build.yml` 与 `.github/workflows/alpha-build.yml`
的 `tauri-action` + `bun run tauri build` 两段 `env:` 块同位添加，
镜像 `SENTRY_DSN` 已有写法。空 secret 等价"未设置"，自动走降级路径。

## 11. 验收检查项

本文件本身的验收：

- [x] `EventContext` 字段穷举且无内容泄露字段（§4）。
- [x] Activation + Reliability 两段事件清单完备，properties schema 闭合（§7）。
- [x] 命名规范、隐私契约、演化策略各自有明确条款（§5 / §6 / §8）。
- [x] §10 四项开放问题已裁决并落到对应章节。
- [x] 无任何 PostHog / Sentry 名称硬编码进 schema 类型签名——`§4` 的
  `EventContext` 与 `§7` 的 properties 类型与 sink 完全解耦。

子任务 2（SDK 接入）启动前需补齐：

- [x] `AnalyticsPort` trait 定义，入参用本文件的事件类型。
- [x] `analytics_gate` 模块实现（与 `telemetry_gate` 对称）。
- [x] 配置目录中 `installation_id` / `analytics_device_id` 持久化逻辑落地（纯模块层，bootstrap 拼装在后续 slice）。
- [x] settings UI 拆分两个开关并补齐文案（`src/components/setting/GeneralSection.tsx` 两个独立 toggle）。
- [x] dev 构建下事件 stdout 打印通路（`uc-bootstrap/src/analytics.rs`：`cfg!(debug_assertions)` 下接 `Gated(StdoutSink)`）。

## 12. 未来事件 roadmap（post-v1 实施计划）

本节列出 **已识别但尚未实施** 的事件埋点。每项按"业务价值 × 实施成本 × 漏斗 anchor 缺口"分级。每个事件标注：触发位置（use case 文件 path）、properties schema、对应漏斗/可回答的产品问题、实施备注（是否需要新增 analytics 依赖 / 新 failure enum）。

§5.3 命名不可变约束在本节同样生效——一旦列入下方表格并落到代码里，事件名 / properties 取值后续不得重命名。

§7.3 末尾的 domain-specific failure enum 原则：每个 domain 的 failure 枚举独立定义（如 `UnlockFailureReason`、`BlobFetchFailureReason`），**禁止** 复用 `FailureReason`（sync 专用）或 `PairingFailureReason`。

### 12.1 P0 — Activation 漏斗 anchor 缺口 + 高频可靠性 ✅ 已落地

**status**: 2026-05-15 落地。事件 schema 已迁入 §7.1，`UnlockFailureReason` 已迁入 §7.5。下表保留作历史与设计回溯，事件本体不再在此节维护。

| 事件名 | 触发位置 | properties | 解决的产品问题 |
|---|---|---|---|
| ~~`setup_completed`~~ ✅ | `setup/initialize_space.rs` 第 7 步 `SetupStatus.has_completed = true` 落地之后 | `{ has_paired_in_same_flow: bool, duration_ms_since_setup_started: Option<u32> }` | Activation 漏斗目前缺这个 anchor，无法区分"启动了引导但没走完"vs"走完引导但没配对"两类流失 |
| ~~`space_unlocked`~~ ✅ | `setup/unlock_space.rs::execute` 成功分支 | （仅 EventContext） | 每次 daemon 重启的可靠性 anchor，对应"用户能不能继续用产品" |
| ~~`space_unlock_failed`~~ ✅ | `setup/unlock_space.rs::execute` 失败分支（pre-condition `SetupNotCompleted` 不上报） | `{ failure_reason: UnlockFailureReason }` | passphrase 错误率 / keyring 解锁失败率定量化；当前完全黑盒 |
| ~~`clipboard_entry_captured`~~ ✅ | `clipboard_capture::execute_with_origin` 成功路径，按 `ClipboardChangeOrigin` 严格过滤 `RemotePush` | `{ origin: system_watcher\|manual_restore, payload_type, payload_size_bucket }` | outbound 同步链路的源头流量；回答"每 DAU 平均产生多少条目"+"捕获了 X 条，dispatch 了多少"漏斗 |

落地备注（保留以便回溯）：

- `setup_completed`：`duration_ms_since_setup_started` 用 monotonic `Instant` 测，溢出 u32 时 fallback `None`；A1 路径 `has_paired_in_same_flow` 恒 false。`setup_status.set_status(has_completed=true)` 失败前 **不** emit（测试 `setup_completed_not_emitted_on_failure_before_status_persist` 守住）。
- `space_unlocked` / `_failed`：`UnlockSpaceUseCase::new` 加 `analytics: Arc<dyn AnalyticsPort>` 入参，bootstrap wiring 在 `facade/space_setup/facade.rs` 补一行 `Arc::clone(&analytics)`。pre-condition `SetupNotCompleted` 短路 **不** emit。
- `clipboard_entry_captured`：`telemetry_capture_origin` 把 `RemotePush` 映射成 `None`、调用方据此跳过 emit（schema doc §12.1 红线）。`payload_type` 按 file > image > text 优先级推断；`payload_size_bucket` 用 `PayloadSizeBucket::from_bytes(snapshot.total_size_bytes())`。

### 12.2 P1 — 新功能线 + 已埋点二段流程的二段验证

| 事件名 | 触发位置 | properties | 解决的产品问题 |
|---|---|---|---|
| `blob_fetch_attempted` | `blob_transfer/fetch_blob.rs::execute` 入口 | `{ payload_size_bucket: PayloadSizeBucket }` | 文件传输实际拉取的 attempted 锚点 |
| `blob_fetch_succeeded` | `fetch_blob::execute` 成功路径 | `{ payload_size_bucket, fetch_latency_ms: u32 }` | 当前 `sync_succeeded.payload_type=file` 只代表 envelope 投递成功，对端实际把字节拉下来的 P95 / 成功率完全不可见 |
| `blob_fetch_failed` | `fetch_blob::execute` 失败路径 | `{ payload_size_bucket, failure_reason: BlobFetchFailureReason }` | 同上，区分 iroh 拨号 / 磁盘满 / 完整性校验 |
| `space_switched` | `setup/switch_space/mod.rs` Phase 4 commit 完成 | `{ duration_ms: u32 }` | 高粘性用户行为信号——只有真在用产品的人才会换空间 |
| `space_switch_failed` | `switch_space` 任一阶段失败 | `{ failure_phase: MigrationPhase, failure_reason: SwitchSpaceFailureReason }` | 4 阶段迁移的失败分布；诊断价值高 |
| ~~`mobile_device_registered`~~ ✅ | `mobile_sync/register_device.rs::execute` 成功 | （仅 EventContext） | iPhone Shortcut 集成的启用计数；当前 0 信号 |
| ~~`mobile_clipboard_synced`~~ ✅ | `mobile_sync/apply_incoming.rs::execute` SyncDoc Applied（出站 GET 埋点延后到 v2） | `{ direction: inbound, payload_size_bucket }` | mobile sync 实际使用频率 |
| ~~`mobile_auth_failed`~~ ✅ | `mobile_sync/authenticate_basic.rs::execute` 失败 | `{ failure_kind: MobileAuthFailureKind }` | iPhone 端密码错误率 |

新增枚举：

```rust
pub enum BlobFetchFailureReason {
    PeerUnreachable,
    NetworkError,
    DigestMismatch,
    DiskFull,
    Timeout,
    Internal,
    Unknown,
}

pub enum SwitchSpaceFailureReason {
    PreparePhase,        // backup 表写入失败
    HandshakeFailed,     // 与目标 sponsor 握手失败
    SwapDecryptError,    // 用 migration_key 解密 backup 行失败
    CommitPersistError,
    Internal,
}

// mobile_sync 三件套已落地（2026-05-15）—— 当前权威 schema 在 §7.6 / §7.7。
// 注意：原稿 RateLimited 在落地时删除，因为 v1 未实装速率限制；§7.7 仅保留 3 变体。
// 历史草稿（保留作设计回溯）：
pub enum MobileAuthFailureKind {
    UnknownUser,
    PasswordMismatch,
    RateLimited,   // ← 落地时移除
    Internal,
}
```

实施备注：

- blob_transfer use case 不持有 analytics，需新加构造参数 + bootstrap wiring。
- `space_switched` 的 `target_space_id_hash` **不传**，避免与 `EventContext.space_id_hash` 重复。
- ~~mobile_sync 三件套埋点是独立 PR~~ ✅ 已落地（2026-05-15）。出站 GET 埋点延后到 v2，`direction` 字段保留枚举槽位备 v2 非破坏式扩展。

### 12.3 P2 — Engagement 与 Retention 信号

| 事件名 | 触发位置 | properties | 解决的产品问题 |
|---|---|---|---|
| `clipboard_entry_restored` | `clipboard_restore/restore_selection.rs::execute` 与 `restore_as_plain_text.rs::execute` 成功路径 | `{ mode: full\|plain_text, age_bucket: lt_1h\|1h_to_1d\|1d_to_7d\|gt_7d, payload_type: PayloadType }` | 衡量"历史"功能价值的核心信号——用户会回头翻历史吗、翻多老的 |
| `search_executed` | `search/search_clipboard_entries.rs::execute` 成功路径 | `{ query_length_bucket, has_filter: bool, result_count_bucket: zero\|1_to_10\|gt_10 }` | 用户找东西的频率 = 是否信任历史的代理指标 |
| `clipboard_history_cleared` | `clipboard_history/clear_history.rs::execute` 成功 | `{ entry_count_bucket: PayloadSizeBucket 形态的条目数桶 }` | 用户主动清空 = 信任 / 隐私 / 性能担忧的负面信号；接近卸载的相关性高 |
| `entry_favorited` | `clipboard_history/toggle_favorite.rs::execute` favoriting 分支 | `{ payload_type }` | 互动信号，区分重度用户 vs 浏览者 |
| `entry_deleted` | `clipboard_history/delete_entry.rs::execute` | `{ payload_type, age_bucket }` | 同上 |

新增 bucket 类型（schema doc §6.3 模式）：

```rust
pub enum QueryLengthBucket { Lt8, Range8To16, Gt16 }   // 复用 NameLengthBucket 形态
pub enum ResultCountBucket { Zero, OneTo10, Gt10 }
pub enum AgeBucket { Lt1H, H1To1D, D1To7D, Gt7D }
```

**隐私红线（必读）**：

- `search_executed` **绝不** 传 query 字面值——仅长度桶 + 是否有 filter + 结果数量桶
- `clipboard_entry_restored.age_bucket` 用桶而非精确时间，避免侧信道还原条目时间戳
- `entry_favorited` / `_deleted` 不传 `entry_id` 也不传 hash——这两个动作单独看就有产品意义，不需要关联到具体条目

### 12.4 P3 — 可观测性 / 运维（视容量决定是否做）

| 事件名 | 触发位置 | properties | 备注 |
|---|---|---|---|
| `app_upgrade_detected` | bootstrap 调 `upgrade/detect.rs` 返回 `UpgradeStatus::{Upgraded, Downgraded}` 时 | `{ from_version: Option<String>, to_version: String, kind: upgrade\|downgrade\|fresh }` | `$set/$set_once` 的 `initial_app_version` vs `app_version` 已能近似推断，本事件是锦上添花 |
| `presence_recovery_completed` | `presence/ensure_reachable_all.rs::execute` 完成 | `{ paired_count, online_count, dial_failure_count, duration_ms }` | 与 `sync_failed.peer_offline` 占比有相关性，不是必须 |
| `daemon_started` / `daemon_stopped` | bootstrap 起点 / `tauri::App::on_exit` | （仅 EventContext） | 精确 DAU / session 时长；但 fire-and-forget HTTP 在 exit 时丢事件率高（§10.1 已说明），收益不稳 |

### 12.5 明确不做的事件（避免噪音）

下列动作即便有埋点能力也不该做，列在此处避免重复讨论：

- `list_entry_projections` / `get_entry_detail` / `get_entry_resource` —— UI 每次刷新都触发，会淹没信号
- `clipboard_history/cleanup` —— 后台 GC，无产品意义
- `mobile_sync/list_devices` / `get_settings` / `update_settings` —— UI 拉取查询，非用户行为
- `mobile_sync/rotate_password` —— 极低频且产品意义已被 `mobile_device_registered` 覆盖
- `blob_publish_*` —— 发布端的失败已在 `sync_failed.failure_stage=immediate_send` 覆盖（dispatch 路径上游就拒了），单独埋点会与 sync 漏斗重复计数

### 12.6 实施节奏建议

- **一次 PR 完成 P0 全部 4 项**：都在 `uc-application` 内部，影响面集中。
- **P1 拆 3 个 PR**：blob_transfer / switch_space / ~~mobile_sync~~，三个 domain 各自独立 review。mobile_sync 已落地（2026-05-15），剩余 blob_transfer / switch_space 待开。
- **P2 视产品侧需求驱动**：哪个漏斗先被产品问到就先实施哪个；不要打包。
- **每个新事件都同步更新本文件 §7 对应分类**：把表格从本节迁出去落到 §7（v1 catalog 的扩展），并把本节对应行 strikethrough 或删除。

跨 PR 共用的隐私契约自查（每次新增事件必过）：

1. 任何 `_id` 字段是否需要 hash？ —— 参照 §6.2。
2. 任何精确数值（size / latency / age / count）是否应桶化？ —— 参照 §6.3。
3. failure_reason 是否复用了别的 domain 的 enum？ —— 违反 §7.3 末尾原则，必须独立定义。
4. 命名是否落到 `{domain}_{action}_{state}`？ —— 参照 §5.1。
5. 是否会与已有事件双计？ —— 参照 §12.1 的 `clipboard_entry_captured` 入站过滤注意事项。
