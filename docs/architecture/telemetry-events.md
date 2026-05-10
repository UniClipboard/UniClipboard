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
- **Reliability**：`sync_attempted` / `sync_succeeded` / `sync_failed`

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
| `anonymous_user_id` | 永久（直到用户卸载或主动重置） | 首次启动 | 配置目录文件 | 留存计算的"用户" |
| `analytics_device_id` | 永久（每台设备一个） | 首次启动 | 配置目录文件 | 设备级切片，如 OS 组合分析 |
| `session_id` | 单次进程运行 | 进程启动 | 内存 | 单次会话漏斗对齐、调试 |

**为什么不叫 `device_id`**：仓库里已有 `uc-core` 域内的 `DeviceId`（pairing /
membership 的 **业务身份**）。analytics 必须使用一个完全独立的 ID，确保即便
有人同时拿到 PostHog 数据与 p2p 网络可观测信息，也无法对两侧做 cross-system
correlation——这是 §6 隐私契约"匿名"承诺的字面落实。

具体约束：

- analytics 模块 **不允许** 读取或派生自 `uc-core::DeviceId`。
- analytics 模块 **不允许** 把 `analytics_device_id` 写入任何业务持久化层
  （settings 之外不允许出现）。
- 业务层不应消费 `analytics_device_id`——它只用于 telemetry sink。

### 3.2 生成与存储

- 全部使用 **UUIDv7**（与 SDK 依赖对齐，且自带时间戳便于排查）。
- 持久化路径：

  ```/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-core/src/settings/...
  // 复用现有 settings 持久化机制，新增字段：
  general.installation_id        // = anonymous_user_id
  general.analytics_device_id    // 独立于业务 DeviceId
  ```

  存放在 `general` 命名空间下，**不进入** settings 导出 / 同步范围
  （`analytics_device_id` 必须随设备绑定，跨设备同步会破坏切片口径）。

- `session_id` 不持久化，每次进程启动重新生成。

### 3.3 重置语义

- 用户在设置页可"重置 telemetry ID"——同时清空 `installation_id` 与
  `analytics_device_id`，下次启动重新生成。
- 关闭 telemetry 开关 **不** 清除 ID（避免误关后再开导致用户跨次被算作新人）。
- `anonymous_user_id` ≠ 账号 ID。即便后续上线账号体系，也不要把这两个 ID
  关联起来——保留"匿名"语义。
- `analytics_device_id` ≠ 业务 `DeviceId`。重置 analytics ID 不影响业务
  pairing / membership 状态。

### 3.4 多设备同 Space 的关系

**v1 报表口径固定为设备级**：留存指"这台设备隔天还回来"，每台设备一个
`anonymous_user_id`、一个 `analytics_device_id`。v1 内两者 1:1，但保留两个
槽位是为了 v2 的 Space 层级聚合（届时同一 Space 内多台设备可共享
`anonymous_user_id`，`analytics_device_id` 仍各自独立）。Space 层面
"一个人 vs 一个团队"的聚合口径推迟到 v2，届时通过新增 `space_id_hash`
维度切片即可，不需要重发历史事件。

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

### 7.1 Activation

| 事件名 | 触发时机 | 关键 properties |
|---|---|---|
| `app_first_open` | `is_first_run == true` 时进程启动 | （仅 EventContext） |
| `setup_started` | 引导页第一帧渲染 | `entry`: `first_run` \| `manual` |
| `device_name_set` | 用户提交设备名 | `name_length_bucket`: `Lt8` \| `8To16` \| `Gt16` |
| `pairing_started` | 用户点击配对 | `method`: `qr` \| `code` \| `discovery` |
| `pairing_succeeded` | 双端握手完成 | `method`, `peer_os`, `duration_ms` |
| `pairing_failed` | 配对中断或超时 | `method`, `failure_reason`（见 §7.4，使用 `PairingFailureReason` 而非 `FailureReason`） |
| `first_clipboard_sync_attempted` | 首次同步发起 | `direction`: `outbound` \| `inbound` |
| `first_clipboard_sync_succeeded` | 首次同步对端确认 | `direction`, `peer_os`, `transport_type`, `duration_ms` |
| `first_file_sync_succeeded` | 文件传输已支持时首次成功 | `peer_os`, `transport_type`, `payload_size_bucket` |

### 7.2 Reliability

`sync_attempted` / `sync_succeeded` / `sync_failed` 三件套，properties：

```rust
pub struct SyncEventProps {
    pub direction: Direction,           // outbound | inbound
    pub payload_type: PayloadType,      // text | image | file
    pub payload_size_bucket: PayloadSizeBucket,
    pub transport_type: TransportType,  // local | p2p_direct | relay | fallback_cloud
    pub peer_os: Option<Os>,            // 已知则填，不要因为缺失就丢事件
    pub sync_latency_ms: Option<u32>,   // 仅成功事件携带
    pub failure_reason: Option<FailureReason>,  // 仅失败事件携带
}
```

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
  "properties": { ...EventContext + event-specific 字段 },
  "timestamp": "2026-05-09T12:34:56+00:00"
}
```

US ingestion endpoint 是 §10 决策第 4 项的 2026-05-09 实际注册区域。
切 EU 或 self-host 实例只换 endpoint URL（`PosthogSink::with_endpoint`
是测试 / 后续迁移入口），事件 wire 形态零改动。

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
配置项；自写 client 通过更直接的方式实现等价契约——**永不主动 inject
IP-derived 字段**。PostHog 服务端的 geoip 是基于请求 IP 推断的，request
本身从客户端发出时不会附带 user IP（reqwest 默认行为），服务端推断的
geoip 字段会以 `$geoip_*` property 形式出现。若后续控制台发现这些字段
仍按请求 IP 落地理位置，可在 `properties` 显式置 `"$geoip_disable": true`
兜底。

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
- [ ] settings UI 拆分两个开关并补齐文案。
- [ ] dev 构建下事件 stdout 打印通路。
