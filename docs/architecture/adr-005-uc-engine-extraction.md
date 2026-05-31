# ADR-005：抽取 `uc-engine` 抽象层以统一支持 CLI / Web / Desktop / iOS / Android

- **状态**：Draft（提案中）
- **日期**：2026-05-20
- **相关文档**：`docs/architecture/ports.md`、`docs/agent/architecture-rules.md`、`docs/architecture/module-boundaries.md`、`docs/architecture/bootstrap.md`、[`adr-007-headless-server-node-deployment.md`](./adr-007-headless-server-node-deployment.md)（无头 server 节点部署——本 ADR host 模型的一个运行模式）

## 1. 背景

### 1.1 现状

`src-tauri/` 下当前的 workspace 已经按六边形架构落到 14 个 crate：

| Crate | 职责 |
|---|---|
| `uc-core` | 纯领域模型 + Port traits |
| `uc-application` | Use cases / orchestrators |
| `uc-infra` | Diesel / SQLite / FS / iroh-blobs 等基础设施适配器 |
| `uc-platform` | OS 适配器（clipboard、keyring、libp2p） |
| `uc-observability` | tracing + PostHog sink |
| `uc-bootstrap` | 组合根 + Sentry / autostart / 文件式 tracing 初始化 |
| `uc-desktop` | 桌面进程内 host：daemon 模式、本地 API、桌面事件源 |
| `uc-tauri` | Tauri builder / commands / plugin 装配 |
| `uc-webserver` | 进程内 axum HTTP/WS server |
| `uc-daemon-{contract,local,client}` | 进程间协议与客户端 |
| `uc-cli` | `uniclip` 二进制 |

**实质上 engine = `uc-core` + `uc-application` + 一个"组合 + 生命周期"门面**。
当前这个门面被埋在 `uc-bootstrap` 与 `uc-desktop` 里，并且夹杂了不少桌面假设。

### 1.2 触发需求

要把同一套业务能力推广到：

- CLI（已存在，目前同时挂 `uc-bootstrap` in-process 与 `uc-daemon-client` 两条路径）
- Web Server（已存在，仍嵌在 desktop daemon 内）
- Desktop（已存在，Tauri 壳）
- **iOS / Android（新增）**

### 1.3 移动端关键约束（已决策）

在与产品讨论后已确认（详见 §2 决策记录前的对话纪要）：

1. **mobile 不存在真正的 daemon**：app 前台时可工作，进入后台后被 OS 暂停
2. **前台时 mobile = 一个完整 node**：核心功能（传输 / 同步）与桌面端无差别
3. **接收方 offline 时数据存在发送方本地 pending**（不引入中央 relay blob 暂存）
4. **不承诺 mobile ⇄ mobile 跨网络**：v1 仅做 mobile ⇄ desktop 互通
5. **iOS share extension 拓扑**：尚未确认（独立进程 vs 写入持久队列由主 app 处理），待 §6 风险一节后续定夺

约束 2 是核心架构主张：**mobile 不需要 engine 的精简子集**，它跑同一份 engine，差异仅在生命周期与平台适配器。

## 2. 决策

### 2.1 新建 `uc-engine` crate

`uc-engine` 是 **唯一** 一个对 host 暴露的统一入口，封装：

- 依赖装配（替代当前 `uc-bootstrap::assembly`）
- 生命周期管理（`start` / `shutdown(deadline)`）
- Use case 全集访问（`engine.use_cases()`）
- 事件订阅（`engine.events()`）
- 显式 resend 触发（`engine.resend_entry(...)`，由用户在 UI 主动调用，详见 §2.5）

```rust
// 设计示意，非最终签名
pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub identity_provider: Arc<dyn IdentityProvider>,
    pub clipboard_port: Arc<dyn SystemClipboardPort>,
    pub secure_storage_port: Arc<dyn SecureStoragePort>,
    pub network_probe_port: Arc<dyn NetworkInterfaceProbePort>,
    pub clock: Arc<dyn ClockPort>,
    // ...
}

pub struct EngineHandle { /* 内部持有 AppDeps + 运行 token */ }

impl EngineHandle {
    pub async fn start(config: EngineConfig) -> Result<Self, EngineError>;
    pub async fn shutdown(self, deadline: Duration) -> Result<(), EngineError>;
    pub fn use_cases(&self) -> &UseCases;
    pub fn events(&self) -> EventSubscription;
    pub async fn resend_entry(&self, cmd: ResendEntryCommand) -> Result<ResendReport, EngineError>;
}
```

**`EngineHandle: Send + Sync + 'static`** 是硬要求；不能在公共 API 上漏出 tokio task handle。

### 2.2 依赖关系

```text
uc-engine  →  uc-core, uc-application, uc-infra
uc-engine  ✗→ 任何 uc-platform-*  ← 由 host 注入
uc-engine  ✗→ uc-webserver / uc-daemon-*  ← host 决定是否启动的外壳
```

**`uc-engine` 不允许直接 import 任何具体 platform adapter**。这是边界铁律。

### 2.3 Host 层分布

| Host crate | 状态 | 职责 |
|---|---|---|
| `uc-host-desktop` | 由现 `uc-desktop` + `uc-bootstrap` 桌面部分演化而来 | Sentry / 文件 tracing / autostart / daemon HTTP 起停 |
| `uc-tauri` | 不动 | 调 `uc-host-desktop` |
| `uc-cli` | 不动 | in-process 时调 `uc-host-desktop`；远程调 `uc-daemon-client` |
| `uc-host-ios`（新） | 新建 | 绑 iOS lifecycle，注入 Pasteboard / Keychain |
| `uc-host-android`（新） | 新建 | 绑 Android lifecycle，注入 JNI ClipboardManager / Keystore |
| `uc-mobile-ffi`（新） | 新建 | UniFFI 暴露 `EngineHandle` 子集给 Kotlin / Swift |

### 2.4 Platform 适配器拆分

| 当前 | 演化后 |
|---|---|
| `uc-platform` | 改名为 `uc-platform-desktop`（保留全部内容） |
| 无 | 新建 `uc-platform-ios`（Pasteboard / Keychain / 网络接口探测） |
| 无 | 新建 `uc-platform-android`（JNI Clipboard / Keystore） |

所有 port trait 仍归 `uc-core`，platform crate 只是同一 trait 的不同 impl。

### 2.5 用户主动 resend（复用 `EntryDeliveryRecord`，**不引入新表 / 新 Port / 不自动触发**）

#### 2.5.1 项目定位决定了语义

UniClipboard 的定位是"**多台设备服务一个人**"，不是协作工具，不是消息队列。这意味着：

- **"对端离线"不是失败**，是预期。用户清楚自己关上了 Mac mini。
- **剪贴板默认是 ephemeral 的**。系统剪贴板关机即失。本项目把它持久化已经超出 OS 默认；如果再加自动补投，等于"用户不在场时替他做了同步决定"——开机后桌上突然多出几小时前在公司复制的临时 OTP / token，违反 ephemeral 语义。
- 自动恢复是协作工具语义（Slack 离线消息、邮件队列），与本项目定位冲突。

#### 2.5.2 真正缺失的功能是 resend

当前桌面端 **没有 resend feature**。用户能在视图层看到某条 entry 对某 peer 是 `Failed { Offline }`，但 **没有"重发"按钮**。这才是要补的能力。

#### 2.5.3 现有真相来源

桌面端已经把"投递事实"建模为 `EntryDeliveryRecord`，由 `EntryDeliveryRepositoryPort` 持久化。其领域宪法已经在 `crates/uc-core/src/clipboard/delivery.rs` 模块开头明确：

> 本模块只关心 **已发生** 的投递尝试。`Pending`（还没尝试）不是一个会被持久化的事实，而是"已知 trusted peer 集合减去已尝试过的目标集合"的差集，由应用层在拼装视图时合成，不在本模块定义。

视图层用例 `GetEntryDeliveryViewUseCase`（`crates/uc-application/src/usecases/clipboard_sync/get_entry_delivery_view.rs`）已落实这条规则——这是 resend 的 **读取侧** 基础，已就绪。

#### 2.5.4 决策

**不引入任何新表、不新增 Port、不自动触发**。仅补 **写入侧**——一个由用户主动调用的 resend 用例：

```rust
pub struct ResendEntryCommand {
    pub entry_id: EntryId,
    /// None = 对该 entry 上所有"非 Delivered / Duplicate"的 peer 重发
    /// Some = 仅对指定 peer 集合重发
    pub target_filter: Option<Vec<DeviceId>>,
}
```

实现流程：

1. 由用户在 UI 上看到 `GetEntryDeliveryViewUseCase` 渲染的投递状态，主动点"重发"
2. UI 层（Tauri command / mobile native bridge）调 `ClipboardOutboundFacade::resend_entry(cmd)`
3. 用例根据 `target_filter` 派生目标集合（无 filter 时从差集派生），过滤掉本机已不持有 plaintext / blob 的目标
4. 对每个目标调既有 `DispatchClipboardEntryUseCase`，走原 fan-out 路径
5. 结果落新 `EntryDeliveryRecord`，UI 通过既有视图刷新

| 层 | 物件 | 状态 |
|---|---|---|
| `uc-core/ports` | `EntryDeliveryRepositoryPort` / `TrustedPeerRepositoryPort` / `MemberRepositoryPort` | ✅ 已有 |
| `uc-infra` | `DieselEntryDeliveryRepository` | ✅ 已有 |
| `uc-application` | **新增 `ResendEntryUseCase`** | 待新增（双端共同收益，详见 §5 Stage 1a） |
| `uc-application/facade` | `ClipboardOutboundFacade::resend_entry(cmd)` thin method | 待新增 |
| `uc-engine` | `EngineHandle::resend_entry(...)` 转发 | 抽 engine 时一并暴露 |
| UI | desktop 详情视图加"重发"入口（按 entry / 按 peer）；mobile 同 | 待新增（前端工作） |

#### 2.5.5 触发完全交给用户，与 host 无关

| Host | 触发方式 |
|---|---|
| desktop | 用户在详情视图点"重发"（按 entry 整体 / 按某个 peer 行）|
| mobile (iOS / Android) | 同 desktop，UI 上点"重发" |
| CLI | `uniclip send --resend <entry-id> [--peer <device-id>]` 子命令 |
| web server | 不暴露（只读视图） |

**不存在自动触发器**，因此也不存在"BGTask 周期"、"presence 上线钩子"、"`WorkManager` 调度"这些跨平台差异。mobile 与 desktop 在重发能力上 **行为完全对称**——跟 §1.3 的核心约束"前台时 mobile = 一个完整 node"自洽。

### 2.6 Engine 必须满足的工程约束（mobile 反向施加，desktop 同样受益）

| 约束 | 来自 | 要求 |
|---|---|---|
| 启动预算 < 300ms | iOS share extension 30s 硬窗口 | 数据库连接、iroh node bind **lazy 化** |
| Shutdown 支持硬 deadline | iOS background suspend | 所有 spawned task 接入 `tokio_util::sync::CancellationToken`，cancel-safe |
| 同进程多次 `start` / `shutdown` | mobile 反复 fg/bg | engine 不持有 `static` 全局；不依赖 `OnceCell` 单进程 bind 守卫 |
| iroh node：endpoint 短生命、identity 长生命 | mobile 短会话 | iroh secret key 持久化（mobile 走 Keychain）；endpoint 每次 start 现绑 |

这四条不视为 mobile-specific 特性，而是 engine 的卫生基线——desktop 上做到这些只会让 daemon 重启更平滑，没有副作用。

### 2.7 明确不做的事

- ❌ 不引入 APNs / FCM 等推送基础设施
- ❌ 不引入中央 relay blob 暂存
- ❌ 不支持 mobile ⇄ mobile 跨网络（v1 范围）
- ❌ 不按 platform 用 cargo feature 拆 engine profile（同一份代码喂所有 host）
- ❌ 不在 engine 公开 API 上漏出 tokio future / handle / `Arc<…>` 内部类型

## 3. 后果

### 3.1 正向

- **桌面端立即受益**：§2.5 的 `ResendEntryUseCase` 补齐当前 **缺失的 resend feature**——用户能看到投递状态视图却没有"重发"按钮，这是已知功能缺口。本项目定位"多台设备服务一个人"，"对端离线"是预期而非失败，因此选择交给用户主动触发，而非自动恢复
- **桌面端立即受益**：§2.6 的 cancel-safe 改造令 desktop daemon 重启行为更可预测，dev profile 切换 / WSL hot-reload / Sentry 崩溃恢复都更稳
- mobile / desktop / cli / web 调用 use case 的路径完全一致，新增能力只需写一次
- platform adapter 拆分让 mobile 上的 attack surface 与编译产物显著缩小
- ADR-005 一旦落地，后续 `docs/architecture/ports.md` §13 "添加方法前先问的问题"的执行范围有了清晰锚点

### 3.2 反向 / 成本

- `uc-bootstrap` 需要被拆解、降级，多处 import 路径会变化
- `uc-platform` → `uc-platform-desktop` 的改名涉及全 workspace import 更新
- `ResendEntryUseCase` 需要新写并补集成测试（不新增 port / 表，但需覆盖 `target_filter` 的两个分支与"本机已不持有 plaintext"的过滤）
- desktop / mobile UI 需要补"重发"按钮入口（前端工作）
- mobile target 引入了 toolchain / CI 复杂度（iOS / Android cross-compile、UniFFI codegen）

### 3.3 边界铁律

提案落地后，以下行为属于违反 ADR-005：

1. `uc-engine` 直接 `use uc_platform_*::…`
2. `uc-engine` 公开 API 暴露 `tokio::task::JoinHandle` / 内部 `Arc<...>` 类型
3. 在 engine 内部 spawn 不接入 cancellation token 的 task
4. 在 host 外的任何 crate 调用 platform-specific 函数（如 iOS Keychain）
5. 为支持 mobile / desktop 区别而在 `uc-engine` 引入 `#[cfg(target_os = "...")]`
6. 新建任何"未投递任务"的持久化表 / 新 Port —— `EntryDeliveryRecord` 是唯一真相源，重投候选必须由差集派生（§2.5）

## 4. 已考虑但被否决的替代方案

### 方案 A：不抽 `uc-engine`，每个 host 各自 wire `uc-bootstrap`

- 优点：动静最小
- 否决理由：`uc-bootstrap` 已经混杂 Sentry / autostart / 文件 tracing 等桌面假设，mobile 无法直接复用；CLI 已经因为同时支持 in-process 与 daemon-client 两条路径而显得复杂

### 方案 B：按 platform 用 cargo feature 拆 engine（如 `feature = "mobile"`）

- 优点：编译产物体积最小
- 否决理由：违反"mobile 前台时 = 一个 node"的对称性主张；feature flag 会扩散到 use case 层，导致两套测试矩阵；与 `docs/agent/architecture-rules.md` 中"不保留平行新旧逻辑"的原则冲突

### 方案 C：mobile ⇄ mobile 通过 APNs + 自有 relay 暂存

- 优点：用户体验完整
- 否决理由：超出 v1 范围；引入中央服务和审核负担；与已决策的"选项 a + 不承诺 mobile⇄mobile"冲突

### 方案 D：mobile 只作为 LAN HTTP client

- 优点：实现最简单
- 否决理由：失去跨网络能力、丢弃 iroh 的关键投资；mobile 与 desktop 行为不对称，违背"mobile 前台时 = 一个 node"

## 5. 实施路径

分两个大阶段：

- **Stage 1 — 前置准备**：每一步都对 **桌面端立即有可观察的收益**，同时是 mobile 路径的硬前提。
  这一阶段 **不依赖** 移动端可行性结论，可以独立交付；即便后续 mobile 路径被中止，Stage 1 的产出对 desktop 仍然是净正收益。
- **Stage 2 — Engine 抽象与 mobile 接入**：Stage 1 完成后再启动。

每个 Stage 内每个步骤必须满足 `docs/agent/architecture-rules.md` 的原子提交规则；port 定义与 adapter 实现不得在同一 commit。

---

### Stage 1 — 前置准备（双端共同收益）

按"desktop 立即可见收益度 + 后续阶段依赖度"排序，建议四步并行/顺序按团队节奏推进。

#### 1a. 补齐当前缺失的 resend feature —— `ResendEntryUseCase`（最高优先级）

**桌面端今日收益**：当前桌面端 **没有 resend 按钮**——用户能看到某条 entry 对某 peer 是 `Failed { Offline }`，却无法主动重发。补齐后这条缺口立刻关上，desktop 用户可见。

**项目定位约束**：UniClipboard 是"多台设备服务一个人"的工具，"对端离线"是预期而非失败（详见 §2.5.1）。因此本步骤 **只做用户主动 resend**，不做任何自动触发。

实现步骤：

- 在 `uc-application/src/usecases/clipboard_sync/` 新增 `resend_entry.rs`，用例输入：

  ```rust
  pub struct ResendEntryCommand {
      pub entry_id: EntryId,
      pub target_filter: Option<Vec<DeviceId>>,
  }
  ```

- 用例步骤：
  1. 加载 entry，确认本机仍持有 plaintext / 必要 blob；否则返回明确的 `EntryNotResendable` 错误（不静默 skip）
  2. 派生目标集合：
     - 有 `target_filter` → 直接用，但用 `TrustedPeerRepository` 验证目标仍在可信集合内
     - 无 filter → 用 `EntryDeliveryRecord` 差集（非 `Delivered` 且非 `Duplicate` 的 trusted peer）
  3. 对每个目标调既有 `DispatchClipboardEntryUseCase`，走原 fan-out 路径
  4. 结果落新 `EntryDeliveryRecord`
- 在 `ClipboardOutboundFacade` 上加 thin method `resend_entry(cmd)`（遵 §11.4 facade 唯一对外纪律）
- 通过 `AppFacade` 暴露给 `uc-tauri` / `uc-cli` 等 host
- desktop UI 在 entry 详情视图加"重发"入口（按 entry 整体 / 按某个 peer 行）
- CLI 加 `uniclip send --resend <entry-id> [--peer <device-id>]` 子命令
- **验收**：
  - desktop 上对一条已存在的、对某 peer 状态为 `Failed { Offline }` 的 entry，点"重发"→ 若 peer 已在线则该 peer 收到内容，`EntryDeliveryRecord` 翻为 `Delivered`
  - peer 仍离线时，点"重发"→ 落新 `Failed { Offline }` 记录，UI 状态保持但 `updated_at_ms` 更新
  - 本机已不持有 plaintext 的 entry 上重发 → 用例返回 `EntryNotResendable`，UI 给出明确反馈

#### 1b. 生命周期卫生 —— CancellationToken 化 + lazy init

**桌面端今日收益**：daemon 重启 / dev profile 切换 / WSL hot-reload 更平滑；Sentry 崩溃后能更可靠恢复。

- 在 `uc-application::deps` 引入 root `CancellationToken`
- 给 `uc-bootstrap::task_registry` 内所有 `tokio::spawn` 接入 child token（select on `token.cancelled()`）
- 在 `uc-bootstrap::non_gui_runtime` / `runtime` 暴露 `shutdown(deadline: Duration) -> Result<()>`，保证 deadline 内全部 task drain
- 把数据库连接 / iroh node bind / tracing sink 改为 lazy（on-demand 初始化）
- 去除任何 `static` / `OnceCell` 形式的单进程守卫
- **验收**：desktop 集成测试中加一个"同进程内反复 start → shutdown(5s) → start"循环 10 次，资源不泄漏（fd / mem / port）

#### 1c. iroh node：identity 持久化、endpoint 短生命

**桌面端今日收益**：daemon 重启不丢身份；vendor fork 中关于 `BaoFileStorage` poisoned 的修复进一步收尾。

- 审计 `uc-platform/src/adapters/libp2p_network.rs` 与 iroh node 构造路径
- 把 iroh secret key 加载从"启动单次"改为"由 `SecureStoragePort` 提供"，桌面 host 注入文件 / Keychain 实现
- endpoint 改为 `EngineHandle::start` 时绑、`shutdown` 时释放，跨 start 不复用
- **验收**：desktop daemon kill -9 → 重启后 device_id / iroh node id **保持不变**；同进程多次 start/shutdown 不报"port in use"

#### 1d. Engine handle 雏形（不抽 crate，先在 `uc-bootstrap` 内做）

**桌面端今日收益**：现在 desktop / tauri / cli 三处各自捡部分 `AppDeps` 字段，重构脆弱；统一 handle 减少耦合。

- 在 `uc-bootstrap` 内引入 `EngineHandle` 结构体，封装 `AppDeps` + 生命周期 + facade 入口
- 现有外部消费者（`uc-desktop` / `uc-tauri` / `uc-cli` / `uc-webserver`）改为只持有 `Arc<EngineHandle>`
- **不** 做 crate 抽离，只在 `uc-bootstrap` 内 reshape
- **验收**：grep `pub.*AppDeps` 在 host 层无引用；所有 host 都通过 `EngineHandle` 访问 facade

---

### Stage 2 — Engine 抽象 + mobile 接入

Stage 1 全部 land 后才启动。

#### 2a. 移动端可行性验证

- `cargo check --target aarch64-apple-ios-sim -p uc-infra` —— **vendor 的 iroh-blobs 是最大未知，必须先验**
- 验证 Diesel + `libsqlite3-sys` bundled 在 iOS 模拟器能 link
- 写一个最小 demo：iOS app 内 attach iroh endpoint、与本机 desktop 节点完成一次握手 + blob 传输
- 测 iOS Keychain 加载 secret key 的实际延迟，确认是否在 §2.6 启动预算（< 300ms）内

#### 2b. 抽出 `uc-engine` crate

- 新建 `crates/uc-engine`
- 把 Stage 1d 中在 `uc-bootstrap` 内做的 `EngineHandle` 迁出
- 同步迁出：`assembly.rs` / `builders.rs` / `non_gui_runtime.rs` / `task_registry.rs` / `file_transfer_lifecycle.rs`
- `uc-bootstrap` 退化为 desktop 专属装配（Sentry / 文件 tracing / autostart / analytics 默认值）

#### 2c. Platform 拆分

- `uc-platform` → `uc-platform-desktop`（**纯改名 commit**，与功能改动隔离）
- 新建 `uc-platform-ios` / `uc-platform-android`（最小实现：Clipboard + SecureStorage + 网络接口探测）

#### 2d. FFI 与 Mobile Host

- 新建 `uc-mobile-ffi`（UniFFI 暴露 `EngineHandle` 子集）
- 新建 `uc-host-ios` / `uc-host-android`，绑 lifecycle + 注入 platform ports
- mobile UI 加"重发"入口（与 desktop 行为完全对称，复用 Stage 1a 的用例，零额外代码）
- Kotlin / Swift sample app 接入

---

### Stage 1 退出标准（进入 Stage 2 的前提）

全部满足才允许进入 Stage 2：

- [ ] `ResendEntryUseCase` + facade 入口上线，desktop 集成测试覆盖 happy path / 离线持有失败 / 本机无 plaintext 三条路径；desktop UI 重发按钮可用
- [ ] desktop daemon 在同进程内反复 start / shutdown(deadline) 10 次资源不泄漏
- [ ] kill -9 desktop daemon 后重启，device_id / iroh node id 保持一致
- [ ] 所有 host crate 通过 `EngineHandle` 访问 facade，无散落的 `AppDeps` 字段引用

## 6. 风险与未知

| 风险 | 影响 | 缓解 |
|---|---|---|
| `uc-infra` 在 iOS target 上无法编译（vendor iroh-blobs 等） | Stage 2 不可行（Stage 1 不受影响） | Stage 2a 优先验证；若失败则 mobile 路径退回 LAN HTTP-only 子集 |
| iOS share extension 进程模型未定（独立 process vs 持久队列） | 影响 `EngineHandle` 是否需多进程并发 | 在 Stage 2a demo 完成前必须由产品 + 工程联合决策 |
| iroh secret key 在 Keychain 的存储 / 加载延迟超出 300ms 启动预算 | mobile 启动卡顿 | Stage 2a 实测；必要时引入异步加载 + 守卫 |
| `tokio_util::sync::CancellationToken` 化改造涉及面广 | Stage 1b 工时低估 | 优先改造高频 spawn 点（pairing / file-transfer / clipboard-capture） |
| `ResendEntryUseCase` 上线后用户无法识别"哪些 entry 该 resend" | 功能形同虚设 | desktop / mobile UI 必须在 entry 详情视图清晰暴露每个 peer 的投递状态（视图层 `GetEntryDeliveryViewUseCase` 已就绪，前端需要把 `Failed { Offline }` 状态做明显视觉提示） |
| `uc-platform` 改名导致全 workspace import 大范围变更 | 一次性 diff 过大 | 通过 cargo `[package].rename` + 一个迁移 commit 完成，不与功能改动混合 |

## 7. 待决问题（Open Questions）

1. **iOS share extension 拓扑**：v1 选 (A) extension 内 in-process 跑 engine 子集，还是 (B) extension 仅写持久队列、主 app 处理？
2. **UniFFI 的 async 标注 vs callback bridge**：哪种风格更适合 `EngineHandle::start` 这种长 init 操作？
3. **CLI 的 in-process 路径是否仍保留**？还是统一改走 `uc-daemon-client`（即便在同机上也跨进程）？这影响 `EngineHandle` 是否要支持"无 daemon 模式"。
   - **部分解答（[ADR-007](./adr-007-headless-server-node-deployment.md) §2.2）**：本期保留单二进制自启（`uniclip start` detached-spawn `uniclip daemon`），RunMode 解析下沉 `uc-desktop`（Scope A）；拆独立 `uniclipd` 二进制（Scope B）暂缓，须单独 ADR。完整的"是否统一走 daemon-client"仍待定。
   - **后续立项（[ADR-008](./adr-008-uniclipd-split-gui-as-client.md)）**：Scope B 正式立项——拆独立 `uniclipd` 二进制、GUI 删除 `GuiInProcess` 永久转 client、轻量模式（GUI 退出后 daemon detach 留守）。即对本 OQ "统一走 daemon-client（即便同机也跨进程）" 给出肯定回答（GUI 侧；CLI 的一次性业务命令仍保留 in-process `uc-bootstrap` 路径）。
4. **mobile share intent 触发的 entry 是否走 `clipboard_capture → dispatch_entry → EntryDeliveryRecord`**？需要核 `crates/uc-application/src/facade/mobile_sync/` 的内部路径；若该路径不落 delivery record，mobile 上的 entry 将无法被 resend，需要补一条路径桥接。

## 8. 决策记录

本 ADR 由 §1.3 中列出的产品决策推导。原始对话摘要：

- 移动端不存在 daemon，仅前台可工作
- 前台 mobile = 一个完整 node（与 desktop 对称）
- 接收方 offline 时数据落发送方本地（选项 a）
- v1 不承诺 mobile ⇄ mobile 跨网络
- 优先级：mobile ⇄ desktop 互通 > mobile ⇄ mobile

任何对上述决策的修订需要更新本节并新建后续 ADR。
