# ADR-005：抽取 `uc-engine` 抽象层以统一支持 CLI / Web / Desktop / iOS / Android / HarmonyOS

- **状态**：Accepted（已接受）
- **日期**：2026-05-20
- **修订**：2026-07-19（四平台统一完整 P2P 节点）
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
- **iOS / Android / HarmonyOS（新增）**

### 1.3 移动端关键约束（已决策）

在与产品讨论后已确认（详见 §2 决策记录前的对话纪要）：

1. **mobile 不存在桌面式常驻 daemon**：宿主按系统授予的运行窗口启动节点；被暂停时正常离线
2. **运行时 mobile = 一个完整 node**：身份、配对、加密、P2P 传输、内容与文件能力和桌面端一致
3. **四平台彼此对等**：desktop、iOS、Android、HarmonyOS 之间不按平台限制连接组合
4. **恢复保持身份**：移动宿主恢复后重新创建短生命 endpoint，但必须加载原有持久身份并回到同一 Space
5. **离线语义不变**：离线不重发、不排队、不最终一致；失败即报告，用户需要时主动重发
6. **iOS share extension 拓扑**：尚未确认（独立进程运行核心，或只把用户操作交给主 app），但不得另建 LAN 协议路径

约束 2 是核心架构主张：**mobile 不需要 engine 的精简子集**，它跑同一份 engine，差异仅在生命周期与平台适配器。

## 2. 决策

### 2.1 新建 `uc-engine` crate

`uc-engine` 是 **唯一** 一个对 host 暴露的统一入口，封装：

- 依赖装配（替代当前 `uc-bootstrap::assembly`）
- 生命周期管理（`start` / `quiesce(deadline)` / `suspend` / `resume` / `shutdown(deadline)`）
- 稳定操作入口（`engine.execute(operation)`）
- 事件订阅（由 `start` 返回同生命周期的事件流）
- 显式 resend 操作（`Operation::ResendEntry`，只能由用户动作触发，详见 §2.5）

```rust
// Design sketch, not the final signature.
pub struct Engine { /* owns assembly, tasks, and lifecycle state */ }

impl Engine {
    pub async fn start(
        config: EngineConfig,
        host: HostCapabilities,
    ) -> Result<(Self, EventStream), EngineError>;
    pub async fn execute(&self, operation: Operation) -> Result<OperationResult, EngineError>;
    pub async fn quiesce(&self, deadline: Duration) -> Result<(), EngineError>;
    pub async fn suspend(&self) -> Result<(), EngineError>;
    pub async fn resume(&self) -> Result<(), EngineError>;
    pub async fn shutdown(self, deadline: Duration) -> Result<(), EngineError>;
}
```

`EngineConfig`、`HostCapabilities`、`Operation`、`OperationResult`、`EventStream` 和 `EngineError` 是 `uc-engine` 自己拥有的稳定边界类型。`HostCapabilities` 只表达目录、安全存储、系统剪贴板、文件句柄和生命周期通知；不得公开 `uc-core` port、`AppFacade`、`UseCases`、tokio handle 或 `Arc<dyn ...>`。平台 host 或绑定负责把系统能力组装成 `HostCapabilities`。

设备身份由核心生成、解释并恢复。宿主只提供系统安全存储的读写能力，不能自行指定身份格式、派生节点 ID 或把密钥回退到普通文件。核心持有所有数据库、索引、blob 与临时传输格式的所有权。

**`Engine: Send + Sync + 'static`** 是硬要求；不能在公共 API 上漏出内部并发或基础设施类型。

#### 2.1.1 生命周期状态机

| 调用 | 合法起点 | 结果 | 在途操作规则 |
|---|---|---|---|
| `start` | 尚无实例 | `Running` | 加载或首次生成持久身份，创建新 endpoint，返回与实例同生命周期的事件流 |
| `quiesce(deadline)` | `Running` | `Quiesced` | 停止接收新操作；在 deadline 内等待在途操作结束，到期后取消剩余操作并为每项报告明确失败 |
| `suspend` | `Running` 或 `Quiesced` | `Suspended` | 从 `Running` 调用时先执行零等待 quiesce；释放 endpoint 和运行任务，不保留待发送队列 |
| `resume` | `Suspended` | `Running` | 在同一实例内重建 endpoint，恢复原身份；原事件流继续有效并收到状态变化 |
| `shutdown(deadline)` | `Running`、`Quiesced` 或 `Suspended` | `Stopped` | 完成有 deadline 的终止清理；实例与事件流随后失效 |

不合法的状态转换返回稳定的状态错误，不能静默忽略。若进程被系统终止，旧实例已经不存在；宿主下次获得运行机会时调用 `start`，核心从安全存储恢复原身份，而不是调用 `resume`。

quiesce 或 suspend 取消的文本、图片、文件收发必须进入明确的失败或取消终态。恢复后不得自动续投、自动重发或把未完成操作重新排队。未完成文件留下的密文临时片段只能等待清理，不能被当成待执行任务；用户需要时必须主动重发。

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
| `uc-host-ohos`（新） | 新建 | 绑 HarmonyOS lifecycle，注入 Pasteboard / HUKS 或 Asset Store |
| `uc-mobile-ffi`（新） | 新建 | UniFFI 暴露 `Engine` 稳定操作给 Kotlin / Swift |
| `uc-ohos-napi`（新） | 新建 | N-API 暴露同一 `Engine` 稳定操作给 ArkTS |

### 2.4 Platform 适配器拆分

| 当前 | 演化后 |
|---|---|
| `uc-platform` | 改名为 `uc-platform-desktop`（保留全部内容） |
| 无 | 新建 `uc-platform-ios`（Pasteboard / Keychain / 网络接口探测） |
| 无 | 新建 `uc-platform-android`（JNI Clipboard / Keystore） |
| 无 | 新建 `uc-platform-ohos`（Pasteboard / HUKS 或 Asset Store / 网络接口探测） |

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
| `uc-application` | **新增 `ResendEntryUseCase`** | 待新增；由公开操作层调用，不直接暴露给宿主 |
| `uc-application/facade` | `ClipboardOutboundFacade::resend_entry(cmd)` thin method | 待新增 |
| `uc-engine` | `Operation::ResendEntry` | 抽 engine 时作为统一操作的一种，不增加旁路方法 |
| UI | desktop 详情视图加"重发"入口（按 entry / 按 peer）；mobile 同 | 待新增（前端工作） |

#### 2.5.5 触发完全交给用户，与 host 无关

| Host | 触发方式 |
|---|---|
| desktop | 用户在详情视图点"重发"（按 entry 整体 / 按某个 peer 行）|
| mobile (iOS / Android / HarmonyOS) | 同 desktop，UI 上点"重发" |
| CLI | `uniclip send --resend <entry-id> [--peer <device-id>]` 子命令 |
| web server | 不暴露（只读视图） |

**不存在自动触发器**，因此也不存在"BGTask 周期"、"presence 上线钩子"、"`WorkManager` 调度"这些跨平台差异。mobile 与 desktop 在重发能力上 **行为完全对称**——跟 §1.3 的核心约束"前台时 mobile = 一个完整 node"自洽。

### 2.6 Engine 必须满足的工程约束（mobile 反向施加，desktop 同样受益）

| 约束 | 来自 | 要求 |
|---|---|---|
| 启动预算可测且受控 | 移动宿主运行窗口有限 | 数据库连接、iroh node bind 分阶段初始化，并记录各阶段耗时 |
| `quiesce` / `shutdown` 支持硬 deadline | 移动系统可能随时暂停宿主 | 所有 spawned task 接入统一取消机制；到期后的操作进入失败或取消终态，不得在 resume 后自动继续 |
| 同进程可多次 `start` / `shutdown` | mobile 反复 fg/bg | 同一时刻只允许一个节点；完成 shutdown 后允许重建，不使用进程终身 `OnceCell` bind 守卫 |
| iroh node：endpoint 短生命、identity 长生命 | mobile 短会话 | 核心拥有身份格式和恢复规则，并通过宿主提供的系统安全存储持久化密钥；endpoint 每次 start 或 resume 重建 |
| 协议与内容能力不按平台裁剪 | 四平台对等 | 不用 platform feature 关闭配对、relay、图片或文件传输；差异只在宿主能力 |

这些约束不视为 mobile-specific 特性，而是 engine 的卫生基线——desktop 上做到这些只会让 daemon 重启更平滑，没有副作用。

### 2.7 明确不做的事

- ❌ 不引入 APNs / FCM 等推送基础设施
- ❌ 不引入中央 relay blob 暂存
- ❌ 不按 platform 用 cargo feature 拆 engine profile（同一份代码喂所有 host）
- ❌ 不在 engine 公开 API 上漏出 tokio future / handle / `Arc<…>` 内部类型
- ❌ 不把 LAN HTTP 兼容路径作为新移动客户端的目标架构

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
- mobile target 引入了 toolchain / CI 复杂度（iOS / Android cross-compile、HarmonyOS 构建、UniFFI 与 N-API codegen）

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
- 否决理由：完整 P2P 节点已能直接连接，不需要中心服务器保存业务内容；APNs / FCM 即使将来用于唤醒，也不能成为数据通道或存储层

### 方案 D：mobile 只作为 LAN HTTP client

- 优点：实现最简单
- 否决理由：失去跨网络能力、丢弃 iroh 的关键投资；mobile 与 desktop 行为不对称，违背"mobile 前台时 = 一个 node"

## 5. 实施路径

2026-07-19 起，唯一执行顺序由 `plans/README.md` 维护：

1. 先固化四平台完整 P2P 决策。
2. 在现有代码形态上证明四平台完整依赖、真机互通和反复启停，不先造临时公开入口。
3. 可行性通过后，按用例驱动建立唯一 `uc-engine` 入口并让桌面先切换。
4. 三种移动宿主只通过该入口接入并完成一致性矩阵。
5. 最后迁入单一核心仓库，发布统一版本并移除旧 LAN 主路径。

计划 002 未通过时不得开始计划 003；计划 004 未通过时不得拆仓。该顺序取代本 ADR 初稿中的“先做桌面 EngineHandle 雏形，再验证移动端”安排，避免制造临时入口和二次迁移。

以下旧 Stage 1 / Stage 2 仅保留为修订前的背景记录，不再作为执行清单。具体任务、完成标准和停止条件全部以 `plans/001-005` 为准。

### 历史 Stage 1 — 前置准备（已被取代）

原方案把桌面 resend、生命周期和临时 `EngineHandle` 放在移动可行性前。可复用的需求保留，但重新归入计划 002 与计划 003，并继续遵守 `docs/agent/architecture-rules.md` 的原子提交规则。

---

#### 历史 1a. 补齐当前缺失的 resend feature

> 本节只保留需求背景；实际入口必须是 `Operation::ResendEntry`，不得重新公开 facade 或专用 engine 方法。

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

#### 历史 1b. 生命周期卫生 —— CancellationToken 化 + lazy init

**桌面端今日收益**：daemon 重启 / dev profile 切换 / WSL hot-reload 更平滑；Sentry 崩溃后能更可靠恢复。

- 在 `uc-application::deps` 引入 root `CancellationToken`
- 给 `uc-bootstrap::task_registry` 内所有 `tokio::spawn` 接入 child token（select on `token.cancelled()`）
- 在 `uc-bootstrap::non_gui_runtime` / `runtime` 暴露 `shutdown(deadline: Duration) -> Result<()>`，保证 deadline 内全部 task drain
- 把数据库连接 / iroh node bind / tracing sink 改为 lazy（on-demand 初始化）
- 去除任何 `static` / `OnceCell` 形式的单进程守卫
- **验收**：desktop 集成测试中加一个"同进程内反复 start → shutdown(5s) → start"循环 10 次，资源不泄漏（fd / mem / port）

#### 历史 1c. iroh node：identity 持久化、endpoint 短生命

**桌面端今日收益**：daemon 重启不丢身份；vendor fork 中关于 `BaoFileStorage` poisoned 的修复进一步收尾。

- 审计 `uc-platform/src/adapters/libp2p_network.rs` 与 iroh node 构造路径
- 核心生成并解释 iroh secret key，宿主只提供系统安全存储能力；正式移动构建禁止普通文件回退
- endpoint 在 `Engine::start` 或 `resume` 时绑定，在 `suspend` 或 `shutdown` 时释放，跨 start 不复用
- **验收**：desktop daemon kill -9 → 重启后 device_id / iroh node id **保持不变**；同进程多次 start/shutdown 不报"port in use"

#### 历史 1d. Engine handle 雏形（已取消）

**桌面端今日收益**：现在 desktop / tauri / cli 三处各自捡部分 `AppDeps` 字段，重构脆弱；统一 handle 减少耦合。

- 不再在 `uc-bootstrap` 建立临时公开 handle。计划 003 直接建立最终 `uc-engine` 入口并迁移桌面，避免两次切换。

---

### 历史 Stage 2 — Engine 抽象 + mobile 接入（已被取代）

本节任务已分别迁入计划 002、003、004，不得按旧顺序执行。

#### 2a. 移动端可行性验证

- 分别对 iOS、Android、HarmonyOS 交叉编译同一完整 `uc-engine` 依赖图，不关闭 pairing、relay、clipboard wire 或 blob/file transfer
- 验证 Diesel + `libsqlite3-sys`、定制 `iroh-blobs` 和加密依赖在三个移动目标可重复构建与链接
- 为三个移动平台各写一个最小宿主，与桌面节点完成配对、文本、图片、文件、relay 和换网验证
- 测量系统安全存储加载身份、首次启动、暂停、恢复与 shutdown deadline
- 任一平台失败时停止 Stage 2 并报告阻碍；LAN HTTP 只能继续服务旧客户端，不能替代本阶段完成

#### 2b. 抽出 `uc-engine` crate

- 新建 `crates/uc-engine`
- 直接建立最终 `Engine`，不从 `uc-bootstrap` 迁移临时公开 handle
- 同步迁出：`assembly.rs` / `builders.rs` / `non_gui_runtime.rs` / `task_registry.rs` / `file_transfer_lifecycle.rs`
- `uc-bootstrap` 退化为 desktop 专属装配（Sentry / 文件 tracing / autostart / analytics 默认值）

#### 2c. Platform 拆分

- `uc-platform` → `uc-platform-desktop`（**纯改名 commit**，与功能改动隔离）
- 新建 `uc-platform-ios` / `uc-platform-android` / `uc-platform-ohos`（最小实现：Clipboard + SecureStorage + 网络接口探测）

#### 2d. FFI 与 Mobile Host

- 新建 `uc-mobile-ffi`（UniFFI 暴露 `Engine` 稳定操作）
- 新建 `uc-host-ios` / `uc-host-android`，绑 lifecycle + 注入 platform ports
- 新建 `uc-ohos-napi` / `uc-host-ohos`，用同一 `Engine` 接入 ArkTS lifecycle + platform capabilities
- mobile UI 加"重发"入口（与 desktop 行为完全对称，复用 Stage 1a 的用例，零额外代码）
- Kotlin / Swift / ArkTS sample app 接入

---

### 历史 Stage 1 退出标准（不再使用）

以下条目已分配到计划 002 和计划 003，不再构成独立阶段门槛：

- [ ] `ResendEntryUseCase` + facade 入口上线，desktop 集成测试覆盖 happy path / 离线持有失败 / 本机无 plaintext 三条路径；desktop UI 重发按钮可用
- [ ] desktop daemon 在同进程内反复 start / shutdown(deadline) 10 次资源不泄漏
- [ ] kill -9 desktop daemon 后重启，device_id / iroh node id 保持一致
- [ ] 所有 host crate 通过最终 `Engine` 访问稳定操作，无散落的 `AppDeps` 字段引用

## 6. 风险与未知

| 风险 | 影响 | 缓解 |
|---|---|---|
| 完整依赖图在任一移动目标无法构建或运行 | 计划 002 阻塞 | 先验证并修复平台假设；无法解决时停止并报告，不得以 LAN HTTP 冒充完成 |
| iOS share extension 进程模型未定（独立 process vs 主 app） | 影响 `Engine` 的宿主方式 | 在计划 002 真机实验完成前必须由产品与工程共同决定 |
| 系统安全存储加载身份超过移动宿主的实测启动预算 | mobile 启动卡顿或错过运行窗口 | Stage 2a 分平台实测；必要时分阶段初始化，但不得缓存明文密钥到普通文件 |
| 统一取消机制改造涉及面广 | 计划 002/003 工时低估 | 优先改造高频任务，并用生命周期状态机测试守住行为 |
| `ResendEntryUseCase` 上线后用户无法识别"哪些 entry 该 resend" | 功能形同虚设 | desktop / mobile UI 必须在 entry 详情视图清晰暴露每个 peer 的投递状态（视图层 `GetEntryDeliveryViewUseCase` 已就绪，前端需要把 `Failed { Offline }` 状态做明显视觉提示） |
| `uc-platform` 改名导致全 workspace import 大范围变更 | 一次性 diff 过大 | 通过 cargo `[package].rename` + 一个迁移 commit 完成，不与功能改动混合 |

## 7. 待决问题（Open Questions）

1. **iOS share extension 拓扑**：v1 选 (A) extension 内 in-process 跑完整 engine，还是 (B) extension 只把用户操作安全交给主 app，由主 app 启动完整 engine？不得引入第三套精简协议实现。
2. **UniFFI 的 async 标注 vs callback bridge**：哪种风格更适合 `Engine::start` 这种长 init 操作？
3. **CLI 的 in-process 路径是否仍保留**？还是统一改走 `uc-daemon-client`（即便在同机上也跨进程）？这影响 `Engine` 是否要支持"无 daemon 模式"。
   - **部分解答（[ADR-007](./adr-007-headless-server-node-deployment.md) §2.2）**：本期保留单二进制自启（`uniclip start` detached-spawn `uniclip daemon`），RunMode 解析下沉 `uc-desktop`（Scope A）；拆独立 `uniclipd` 二进制（Scope B）暂缓，须单独 ADR。完整的"是否统一走 daemon-client"仍待定。
   - **后续立项（[ADR-008](./adr-008-uniclipd-split-gui-as-client.md)）**：Scope B 正式立项——拆独立 `uniclipd` 二进制、GUI 删除 `GuiInProcess` 永久转 client、轻量模式（GUI 退出后 daemon detach 留守）。即对本 OQ "统一走 daemon-client（即便同机也跨进程）" 给出肯定回答（GUI 侧；CLI 的一次性业务命令仍保留 in-process `uc-bootstrap` 路径）。
4. **移动端 share intent 如何进入统一操作入口**？它必须转换为与桌面相同的捕获/发送用例并产生一致的投递记录，不得调用旧 `mobile_sync` facade 或新建旁路协议。

## 8. 决策记录

本 ADR 由 §1.3 中列出的产品决策推导。2026-07-19 的当前决定是：

- 桌面、iOS、Android、HarmonyOS 都运行同一完整 P2P 核心
- 对等节点共享身份、配对、加密、传输和内容能力，不按平台限制连接组合
- 对等身份不等于永久后台在线；移动宿主暂停时正常离线，恢复后以原身份重连
- 离线不重发、不排队、不最终一致，仍由用户主动重发
- 旧 LAN HTTP 仅作为限期兼容路径，不再承载新能力

任何对上述决定的修订必须更新本节，并通过后续 ADR 显式取代。
