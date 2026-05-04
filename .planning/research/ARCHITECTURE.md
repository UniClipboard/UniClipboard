# 架构集成研究 · v0.7.0「LAN-only Mode」

**里程碑：** v0.7.0 LAN-only Mode
**研究域：** 既有六边形架构下的字段落点、装配链注入点、设备列表"连接通道"指示器来源
**研究时间：** 2026-05-04
**整体置信度：** HIGH（关键文件全部 grep 验证，集成点行号锁定）

---

## 0. 本里程碑架构变更摘要

> **核心判断：** 这不是新架构，而是**在既有钩子上挂一根线**。`uc-core::Settings` 已有 `// pub network: NetworkSettings,` 注释占位（`uc-core/src/settings/model.rs:201-202`），`IrohNodeConfig.disable_relays` 已是 `pub`（`uc-infra/src/network/iroh/node.rs:161`），`bind` 时 `RelayMode` 路径已通（`node.rs:368-372`）。本里程碑要补的是"把这根线接通"，外加一个全新的"连接通道"读出能力。

| 类别 | 涉及组件 | 备注 |
|------|---------|------|
| **新增** | `NetworkSettings` 值对象（`uc-core`）+ View / Patch 镜像（`uc-application`）+ DTO（`uc-webserver` + `uc-daemon-contract`）+ TS 类型（`src/api/daemon/settings.ts`）+ NetworkSection 真实 UI + "连接通道"读取链路（新增 port `ConnectionChannelPort` + iroh 适配 + DTO 字段 + 前端组件） | 大头 |
| **修改** | `apply_settings_patch`（多挂一段）+ `Settings::default`（多一行）+ `build_space_setup_assembly` 调用方（`builders.rs:178` / `non_gui_runtime.rs:280`）从 `IrohNodeConfig::default()` 改成"先读 settings 再造"+ NetworkSection 替换占位 + `PeerSnapshotDto` 加字段 + `peers.changed` 路径不变（增量字段而非新事件类型） | 中等 |
| **保持不变** | 六边形分层、daemon-first 主权、HTTP `/settings` 与 WS `peers.changed` 协议骨架、Tauri commands（**继续没有 settings 命令**，前端走 daemon HTTP）、iroh `RelayMode` 在 bind 时确定的事实、`disable_relays` 字段本身、settings JSON 文件原子写策略、SQLite migration 链（settings 是 JSON 文件，不走 SQL migration） | 大量 |

> **常见误区纠正：** Settings 不是 SQLite 存储。它是 `~/Library/Application Support/app.uniclipboard.desktop/.../settings.json` 的 JSON 文件 + serde（`uc-infra/src/settings/repository.rs:77 atomic_write`），migration 走 `SettingsMigrator`（基于 `schema_version` 数值递增）而非 SQL。原 question 里"SQLite 存储/migration 怎么走"的提法本身假设错了。

---

## 1. 推荐架构（按层）

### 1.1 Domain 层（`uc-core`）

**新增值对象 `NetworkSettings`** —— 放进既有 settings model。

文件：`src-tauri/crates/uc-core/src/settings/model.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkSettings {
    /// 是否允许 iroh 在直连失败时回落到公网中继。
    /// 默认 `true`（保持现状，存量用户不受影响）。
    /// UI 上的"LAN-only Mode"开关呈现为它的反向：toggle 关闭 = `false`。
    #[serde(default = "default_allow_relay_fallback")]
    pub allow_relay_fallback: bool,
}

fn default_allow_relay_fallback() -> bool { true }
```

挂到 `Settings` 上（取消 `model.rs:201-202` 注释，正式启用）：

```rust
pub struct Settings {
    // ...既有字段
    #[serde(default)]
    pub network: NetworkSettings,
}
```

`Default for Settings` 在 `uc-core/src/settings/defaults.rs:251-262` 增一行 `network: NetworkSettings::default()`，并补 `impl Default for NetworkSettings { fn default() -> Self { Self { allow_relay_fallback: true } } }`。

> **为什么放 `uc-core` 而不是 `uc-infra`：** `uc-core/AGENTS.md` §8 明确说"业务设置（如 `SyncSettings`）属于 core，配置加载属于 infra"。`network.allow_relay_fallback` 是用户业务偏好（"我要不要走公网中继"），不是 infra 的配置加载逻辑，归 core 是直接对应的。注意值对象**不持有**任何 iroh / libp2p 类型——`disable_relays` 这种命名留给 `uc-infra`。

**`schema_version` 是否要 bump？** 不需要。新增字段全部带 `#[serde(default = ...)]`，旧 settings.json 反序列化时缺字段直接走默认值，向前兼容。`CURRENT_SCHEMA_VERSION` 保持 `1`（`uc-core/src/settings/model.rs:7`），`SettingsMigrator` 不需要新条目（`uc-infra/src/settings/migration.rs:38-41` 当前是空 vec）。

**领域端口要加吗？** `SettingsPort`（`uc-core/src/ports/mod.rs`）签名 `load(&self) -> Settings` / `save(&self, &Settings)` 是字段无关的，新字段自动跟着流过去，**不动**。**但** §1.5 会建议**新增一个独立 port** `ConnectionChannelPort` 给"连接通道"指示器，原因见那一节。

---

### 1.2 Application 层（`uc-application`）

**改 `models.rs` —— 加 namespace，不破坏既有签名。**

文件：`src-tauri/crates/uc-application/src/facade/settings/models.rs`

新增（在 `FileSyncSettingsView` 之后）：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSettingsView {
    pub allow_relay_fallback: bool,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkSettingsPatch {
    pub allow_relay_fallback: Option<bool>,
}
```

`SettingsView` 新增字段 `pub network: NetworkSettingsView`（`models.rs:134-143`），`SettingsPatch` 新增字段 `pub network: Option<NetworkSettingsPatch>`（`models.rs:217-226`）。

`From<core::Settings> for SettingsView`（`models.rs:386-444`）末尾补 `network: NetworkSettingsView { allow_relay_fallback: value.network.allow_relay_fallback }`。

`apply_settings_patch`（`models.rs:446-566`）末尾追加：

```rust
if let Some(network) = patch.network {
    if let Some(v) = network.allow_relay_fallback {
        existing.network.allow_relay_fallback = v;
    }
}
```

**`facade.rs` 不动。** `SettingsFacade::get` / `update`（`facade.rs:27-48`）签名都是 `SettingsView` / `SettingsPatch`，新增字段透明流过。这是**纯 additive 改动，不是 namespace 重构**。`uc-application/AGENTS.md` §11.4 要求外部只通过 `facade/` 目录访问；`mod.rs:5-12` 的 `pub use` 白名单需要新增一行 `NetworkSettingsView, NetworkSettingsPatch`。

**为什么是 additive 而不是 namespace 隔离：** `SettingsView` 已经是 namespace 化的（`general` / `sync` / `security` / `pairing` / `file_sync`），新增 `network` 是**沿用既有结构**，不是重命名/迁移。其他 namespace 现状不动。

---

### 1.3 Infra 层（`uc-infra`）

**Settings 存储/migration 不动。** 已经是 JSON 文件 + `serde(default)` + 显式 `SettingsMigrator`（基于 `schema_version`）。新字段加 `#[serde(default)]` 自然兼容；`SettingsMigrator` migrations vec（`migration.rs:38-41`）保持空。

**iroh node 启动注入 `disable_relays`** —— 装配点是 `uc-bootstrap::space_setup`，不是 `uc-infra` 内部。

文件：`src-tauri/crates/uc-bootstrap/src/space_setup.rs:208-228`

当前签名（`build_space_setup_assembly` 接 `IrohNodeConfig` 参数）已经允许调用方传入定制配置。**调用方**才是真正的注入点：

| 调用方文件 | 行号 | 当前 | 修改后 |
|---|---|---|---|
| `uc-bootstrap/src/builders.rs` | 178 | `build_space_setup_assembly(&wired, IrohNodeConfig::default())` | 先读 `wired.deps.settings.load().await`，根据 `network.allow_relay_fallback` 构造 `IrohNodeConfig { disable_relays: !allow_relay_fallback, rendezvous_base_url: None }` |
| `uc-bootstrap/src/non_gui_runtime.rs` | 280 | 同上 | 同上 |
| `uc-bootstrap/src/space_setup.rs` 测试 | n/a | 测试用 `IrohNodeConfig { disable_relays: true, .. }` 直传 | 不动，集成测试保持显式控制 |

**为什么不在 `space_setup.rs` 内部读 settings：** `space_setup.rs` 已经接 `IrohNodeConfig` 参数，是装配体的入参；它不应该再反向调用 `SettingsPort::load`，否则**装配体既是消费者又是配置读取者**，违反 `uc-bootstrap` 的"只装配，不决策"职责。读 settings 的责任应该留在 builders（已经持有 `wired.deps.settings`）。

**新增 `IrohNodeConfig::from_network_settings(&NetworkSettings)`？** 不要做。`uc-infra/AGENTS.md` §4.1 明确"不让 core 适配 infra"——`NetworkSettings` 不应该被 `uc-infra` 直接知道。装配代码（`uc-bootstrap`）做翻译是正确的边界。

**`uc-infra/src/network/iroh/node.rs:153-162` 的 `IrohNodeConfig` 不动。** `disable_relays: bool` 已经满足需要。

**新增 infra adapter `IrohConnectionChannelAdapter`** —— 见 §1.5。

---

### 1.4 Webserver / Tauri commands

**HTTP `/settings`（webserver）只动 DTO 层，不加新命令。**

文件：`src-tauri/crates/uc-webserver/src/api/settings.rs:23-27`

`router()` 不动。`get_settings_handler` / `update_settings_handler` 都是泛 patch，新字段自动流过。**改的是 DTO 转换：**

* `src-tauri/crates/uc-webserver/src/api/dto/settings.rs`：新增 `NetworkSettingsDto { allow_relay_fallback: bool }` + `NetworkSettingsPatchDto { allow_relay_fallback: Option<bool> }`，挂到 `SettingsDto` 与 `SettingsPatchDto`
* `settings.rs:92-160` 的 `settings_patch_from_dto`：在末尾补一段（与 `file_sync` 同级）：

  ```rust
  network: patch.network.map(|n| app_settings::NetworkSettingsPatch {
      allow_relay_fallback: n.allow_relay_fallback,
  }),
  ```

* `settings.rs:162-218` 的 `settings_view_to_dto`：末尾补 `network: NetworkSettingsDto { allow_relay_fallback: value.network.allow_relay_fallback }`

**Tauri commands：不需要新命令。** `uc-tauri/src/commands/mod.rs` 里**根本没有 settings 命令**（确认：grep `fn get_settings|fn update_settings` in `uc-tauri` 无结果）。前端的 settings 走 daemon HTTP 客户端 `daemonClient.request('/settings')`（`src/api/daemon/settings.ts:185-207`）。这是既有架构的明确选择（webserver `settings.rs:5-7` 有注释说明 "Unlike the Tauri command（which applies OS-level side effects），these handlers only update the settings domain model"——历史上**曾经**有 Tauri command；当前代码里 Tauri commands 已经下线 settings，全部走 HTTP）。

**为什么不重新加 Tauri command：** `network.allow_relay_fallback` **没有 OS-level side effect**（不像 `auto_start` 要写注册表，或 `keyboard_shortcuts` 要 register global shortcut）。它只影响 daemon 进程下次启动时的 iroh bind 行为，是纯 daemon-domain 的字段。daemon HTTP 路径已经覆盖。

---

### 1.5 设备列表"连接通道"指示器

这是本里程碑**唯一真正的"新增能力"**——前面所有改动都是"接通既有钩子"。

#### 现状（无指示器）

* `PeerSnapshotDto`（`uc-daemon-contract/src/api/types.rs:33-40`）目前字段：`peer_id` / `device_name` / `addresses` / `is_paired` / `connected: bool` / `pairing_state: String`。**没有"通道类型"字段**。
* `connected: bool` 来自 `PresencePort.current_state()`（即 `Online | Offline | Unknown`，`uc-core/src/ports/presence.rs:23-28`）二值化映射，与"通道"是两件事。
* iroh **有**读 connection type 的 API：`Endpoint::remote_info(addr_id) -> Option<RemoteInfo>`，过滤 `TransportAddrInfo` 是 `Active` 的 entries（`uc-infra/src/network/iroh/connect.rs:51-67` 已经有用例，目前只用作日志）。
* 现成的事件路径：`PresenceMonitor`（`uc-desktop/src/daemon/peers/presence_monitor.rs:1-50`）在 `PresenceEvent` 触发时拉一遍 `app_facade.list_peer_snapshots()` 然后 broadcast `peers.changed` 全量快照；前端 `SpaceMembersPanel.tsx:50-58` 收到事件后重拉 `/paired-devices`。**新增字段沿这条路就行，不需要新事件类型。**

#### 推荐设计

新增一个**领域 port** + 一个**iroh adapter** + 给 `PeerSnapshotDto` 加字段。

**Port 定义**（`uc-core/src/ports/connection_channel.rs`，新文件）：

```rust
/// 当前连接所走通道。"我此刻的流量是 LAN 直连还是公网 relay 中继？"
/// 由 application 层喂给 roster facade，最终回流到 PeerSnapshotDto。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionChannel {
    /// 直连（包含 LAN mDNS 与 hole-punched WAN）
    Direct,
    /// 公网中继中转
    Relay,
    /// 当前不在线，无通道
    Offline,
    /// 未拨过号或无法判定
    Unknown,
}

#[async_trait]
pub trait ConnectionChannelPort: Send + Sync {
    async fn channel_for(&self, device: &DeviceId) -> ConnectionChannel;
}
```

> 注：`uc-core/AGENTS.md` §6 允许 core 出现"设备之间的关系"类型；`ConnectionChannel` 是关系层而不是协议层（不出现 "iroh" / "transport" 字眼），合规。

**Adapter 实现**（`uc-infra/src/network/iroh/connection_channel_adapter.rs`，新文件）：

包装 `Arc<Endpoint>` + `Arc<dyn PeerAddressRepositoryPort>`。`channel_for(device)` 流程：

1. 从 `peer_addr_repo.get(device)` 拿到 `EndpointAddr`，没有 → `Offline`
2. `endpoint.remote_info(addr.id).await` → 找 `Active` 的 `TransportAddrInfo`：
   * 若有 `TransportAddr::Ip` → `Direct`（LAN/WAN 都算直连，UI 文案上统一显示"LAN/Direct"；如需进一步细分 LAN vs hole-punched WAN，在 adapter 里检查 IP 是否私有地址范围即可，不影响 port 契约）
   * 若只有 `TransportAddr::Relay` → `Relay`
   * 若 `Active` 集合为空 → `Unknown`（拨过号但路径没建立）
3. 没 `remote_info` → `Unknown`

> 注：iroh 0.98 把 `Endpoint::conn_type` 换成了 snapshot 风格的 `remote_info`（连 `connect.rs:45-67` 的注释都明确写了），这是当前 API。无需 watcher。

**Application 层接入**：

* `uc-application` 把 `Arc<dyn ConnectionChannelPort>` 加进 `MemberRosterDeps`（与 `PresencePort` 并列），`MemberRosterFacade::list_with_presence` 内部聚合时多读一次 `channel_for`。
* `app_facade.list_peer_snapshots()`（`uc-webserver/src/api/server.rs:99-112` 调它）的 `PeerSnapshotView` 增字段 `channel: ConnectionChannel`。

**装配链**：

文件：`src-tauri/crates/uc-bootstrap/src/space_setup.rs`

`build_space_setup_assembly` 在 `let iroh_node = builder.spawn();`（行 273 前后）拿到 `endpoint` 之后构造 `Arc::new(IrohConnectionChannelAdapter::new(endpoint.clone(), wired.peer_addr_repo.clone()))`，喂进 `MemberRosterFacade::new(...)`。

> 注：`IrohNode` 当前**私有持有** `endpoint`（`node.rs:99-102`，没有 `pub fn endpoint()`）。需要新增一个 `pub fn endpoint(&self) -> Arc<Endpoint>` 访问器，**或**让 `IrohNodeBuilder::spawn()` 同时返回一个 `ConnectionChannelPort` 句柄（更干净，与现有 `install_*` 模式一致）。后者更符合 `uc-infra/AGENTS.md` §4.3 的可替换性原则——把 `Endpoint` 当 `Arc` 漏出去，等于把"拿到 iroh endpoint"作为隐式合约。

**DTO/事件传递**：

* `uc-daemon-contract/src/api/types.rs:33-40` 给 `PeerSnapshotDto` 加 `pub channel: String`（值 `"direct" | "relay" | "offline" | "unknown"`）
* `uc-webserver/src/api/server.rs:99-112` 的 mapping 多一行 `channel: peer.channel.into()`（或 `channel_to_dto(...)` helper）
* WS 协议**不改动**——`peers.changed` 仍然是全量快照（`PeersChangedFullPayload { peers: Vec<PeerSnapshotDto> }`，`types.rs:115-118`），新字段跟着走

**前端**：

* `src/api/daemon/members.ts` 的 `SpaceMember` 接口加 `channel?: 'direct' | 'relay' | 'offline' | 'unknown'`
* 在 `src/components/device/` 下新建 `ConnectionChannelBadge.tsx`，根据 channel 显示 LAN / Relay / Offline 三态徽章
* `SpaceMembersPanel.tsx` 渲染每个成员时挂上这个徽章；保留既有 `peers.changed` 订阅（行 50-58）即可，无需新事件

#### 何时通道会变化？

iroh 不主动 broadcast "通道切换"事件。当前逻辑是：每次 `PresenceEvent`（拨号成功 / connection.closed）会触发 `peers.changed` 全量快照（`presence_monitor.rs:13-27`）；前端 `DevicesPage.tsx:11-16` 还会每 15s 主动 `refreshPresence()` 一次。这两条路径都会自然带回新的 `channel` 值。**无需新订阅 / 新事件**——通道由 `peer_snapshots()` 自然刷新。

如果用户感觉刷新太慢（直连失败回落到 relay 但 UI 没更新），可以在 §6 留一个增强项："连接路径切换专用事件"，本里程碑不做。

---

## 2. 数据流（当前 vs 之后）

### 2.1 启动期（settings → iroh bind）

**当前：**

```
进程启动 → builders::build_slice1_cli_context (or non_gui)
        → build_space_setup_assembly(&wired, IrohNodeConfig::default())  // disable_relays = false
        → IrohNodeBuilder::bind → Endpoint(relay_mode = Default)
```

**之后：**

```
进程启动 → builders / non_gui_runtime
        → wired.deps.settings.load().await        // 新增
        → let cfg = IrohNodeConfig {
              disable_relays: !settings.network.allow_relay_fallback,
              rendezvous_base_url: None,
          }                                       // 新增翻译
        → build_space_setup_assembly(&wired, cfg)
        → IrohNodeBuilder::bind → Endpoint(relay_mode = Default | Disabled)
```

**关键约束：** iroh `RelayMode` 在 `Endpoint::builder()...bind()` 时确定（`node.rs:368-396`），运行时改不了。本里程碑**接受这个限制**，UI 切换时弹"重启生效"（`node.rs:380` 的注释明确"Slice 1 always has pairing. A future slice ... would add a separate `bind_bare` constructor"——用户决策也明确不做热切换）。

### 2.2 用户切换 LAN-only Mode

```
用户在 NetworkSection 拖动开关
  → updateSettings({ network: { allowRelayFallback: false } })   // src/api/daemon/settings.ts
  → PUT /settings (HTTP, daemon webserver)
  → settings_patch_from_dto → SettingsFacade::update
  → SettingsPort::save (写 settings.json，原子写)
  → 前端收到响应 → 弹 RestartHint dialog "重启后生效"
  → 用户手动重启 daemon
```

**daemon 内部不订阅 settings 变更**（确认：`uc-core/src/ports/mod.rs` 的 `SettingsPort` 只有 `load` / `save`，无 subscribe）。所以"切换 → 立即注入 IrohNodeConfig"的链路本里程碑不存在；**重启路径的注入点就是 §2.1 描述的 builders 那条**。

### 2.3 设备列表渲染（含连接通道）

**当前：**

```
DevicesPage 挂载
  → fetchSpaceMembers → GET /paired-devices → SpaceMember[]
  → DevicesPage 每 15s POST /presence/refresh
  → daemon ensure_reachable_all → PresenceEvent → PresenceMonitor 收事件
  → PresenceMonitor 调 list_peer_snapshots → 推 peers.changed (WS)
  → 前端 SpaceMembersPanel 收到 → 重新拉 fetchSpaceMembers
  → UI 用 connected: bool 显示在线/离线
```

**之后（增量）：**

```
（链路完全一致，差异在数据字段）
  → list_peer_snapshots 内部多读一次 ConnectionChannelPort.channel_for(device)
  → PeerSnapshotDto 多一个 channel 字段
  → 前端 SpaceMember 多 channel 字段
  → 渲染时挂 ConnectionChannelBadge 显示 LAN / Relay / Offline
```

> **重点：不新增 WS 事件类型，不新增 HTTP endpoint。** 复用既有 `peers.changed` 全量快照与 `GET /paired-devices`。

---

## 3. 集成点（已 grep 验证）

| 位置 | 文件:行号 | 作用 | 本里程碑动作 |
|------|---------|------|--------|
| `IrohNodeConfig::default()` 调用 | `uc-bootstrap/src/builders.rs:178` | GUI 启动装配 | **改**：先读 settings 再造 cfg |
| 同上 | `uc-bootstrap/src/non_gui_runtime.rs:280` | CLI/daemon 启动装配 | **改**：同上 |
| `IrohNodeBuilder::bind` | `uc-infra/src/network/iroh/node.rs:363-411` | bind endpoint，应用 `relay_mode` | **不动** |
| `disable_relays` 字段 | `uc-infra/src/network/iroh/node.rs:161` | `IrohNodeConfig` 字段 | **不动**（已 pub） |
| Settings 字段挂载点 | `uc-core/src/settings/model.rs:201-202` | 已有 `// pub network: NetworkSettings,` 注释 | **改**：取消注释、新增类型定义 |
| `Settings::default()` | `uc-core/src/settings/defaults.rs:251-262` | 默认值 | **改**：加 `network: NetworkSettings::default()` |
| `SettingsView` / `SettingsPatch` | `uc-application/.../settings/models.rs:134-226` | App 层 view / patch | **改**：加 `network` 字段 |
| `apply_settings_patch` | `uc-application/.../settings/models.rs:446-566` | patch 合并 | **改**：末尾追加 `network` 处理 |
| `SettingsFacade` | `uc-application/.../settings/facade.rs:17-49` | 读写 façade | **不动**（字段无关） |
| Facade `pub use` 白名单 | `uc-application/.../settings/mod.rs:5-12` | 对外类型暴露 | **改**：加 `NetworkSettingsView, NetworkSettingsPatch` |
| HTTP `/settings` router | `uc-webserver/src/api/settings.rs:23-27` | GET/PUT 路由 | **不动** |
| `settings_patch_from_dto` / `settings_view_to_dto` | `uc-webserver/src/api/settings.rs:92-218` | DTO ↔ App view 映射 | **改**：补 `network` 段 |
| `PeerSnapshotDto` | `uc-daemon-contract/src/api/types.rs:33-40` | 节点快照 DTO | **改**：加 `channel` 字段 |
| `peer_snapshots()` mapping | `uc-webserver/src/api/server.rs:99-112` | App view → DTO | **改**：补 `channel` 字段映射 |
| `PresenceMonitor` | `uc-desktop/src/daemon/peers/presence_monitor.rs` | WS broadcast | **不动**（透明） |
| `peers.changed` 事件订阅 | `src/components/device/SpaceMembersPanel.tsx:50-58` | 前端 WS 订阅 | **不动**（透明） |
| 前端 settings 入口 | `src/api/daemon/settings.ts:129-138` `Settings` interface | TS 类型 | **改**：加 `network: { allowRelayFallback: boolean }` |
| 前端 settings context | `src/contexts/setting-context.ts`（`useSetting` 来源） | Settings 上下文 | **改**：补 `network.allowRelayFallback` 字段流转 |
| `NetworkSection.tsx` | `src/components/setting/NetworkSection.tsx` | 占位组件 | **替换**：实现真实开关 + RestartHint |
| `settings-config.ts` | `src/components/setting/settings-config.ts:54-58` | 分类挂载 | **不动**（Wifi 图标已挂） |
| `SpaceMembersPanel` 单成员渲染 | `src/components/device/SpaceMembersPanel.tsx` | 设备卡片 | **改**：加 `<ConnectionChannelBadge channel={...} />` |

---

## 4. 影响面分析

### 4.1 编译期边界

| 边界规则 | 是否触动 | 说明 |
|---------|---------|------|
| `uc-core` 不依赖具体 infra 类型 | 不破 | `NetworkSettings` 是值对象，不引入 iroh 类型 |
| `uc-application` 只通过 port 访问 infra | 不破 | `ConnectionChannelPort` 在 core 定义，infra 实现 |
| `uc-application` 只通过 `src/facade/` 对外暴露 | 不破 | 新增类型走 `facade/settings/mod.rs:5-12` 白名单 |
| `uc-infra` 不上浮第三方类型 | 不破 | `IrohConnectionChannelAdapter` 内部消化 `iroh::Endpoint` |
| `uc-desktop` GUI-framework agnostic | 不破 | 不涉及 GUI 框架 |

### 4.2 历史欠账与命名清理

* `uc-application/AGENTS.md` §11.4.7 提到"部分外部消费者仍直接从 `uc_application::<业务子模块>` 导入"。本里程碑**不解决**这个欠账，但**不能再引入**新的越界 import。新增 `NetworkSettingsView` / `NetworkSettingsPatch` 类型必须只通过 `facade/settings/mod.rs` 暴露。
* `IrohNodeConfig.disable_relays` 命名是 infra 内部细节（反向语义），不冒泡到 core。core 用 `allow_relay_fallback`（业务语义）；翻译在 builders 完成。这与之前 explore 阶段决策一致（见 `.context/attachments/Summary of Explore LAN version need.md` 用户对话末段）。

### 4.3 可观测性

* `node.rs:399-404` 的 bind 后 `debug!(... disable_relays = config.disable_relays, ...)` 日志已经覆盖关键事实，建议**新增** `tracing::info!` 在 builders 翻译那一步打印 "applying network.allow_relay_fallback={} → disable_relays={}"，方便用户支持和排障。
* `IrohConnectionChannelAdapter` 实现里建议复用 `connect.rs:50-66` 的 `Active` 路径解析逻辑（提取成 free function），避免两份地方各自演化。

### 4.4 测试影响

* `uc-bootstrap/tests/slice*_*.rs` 三个集成测试都用 `IrohNodeConfig { disable_relays: true, .. }`（`slice1_handshake_e2e.rs:344` / `slice2_phase1_presence_e2e.rs:354` / `slice2_phase2_clipboard_e2e.rs:373`）。这些测试**不动**——它们是 loopback-only 自动化，本来就要禁 relay，与 LAN-only Mode 业务无关。
* 新增单元测试：
  * `apply_settings_patch` 处理 `network.allow_relay_fallback`
  * `IrohConnectionChannelAdapter.channel_for` 三态映射（`Direct` / `Relay` / `Offline`）—— 用 fake `PeerAddressRepo` 与 mock endpoint 行为，或写 doc-test 验证私有 IP 判定
  * `MemberRosterFacade.list_with_presence` 输出含 `channel`

---

## 5. 建议构建顺序

> **建议：后端字段在前，前端开关在后；"连接通道"指示器和 LAN-only 开关解耦推进。**

### Phase A · 后端字段落地（必须先做）

1. `uc-core::Settings::network` 字段 + `NetworkSettings` 类型 + `Default` 实现
2. `uc-application` view/patch/apply_patch 扩展 + `mod.rs` 白名单
3. `uc-webserver` DTO + dto ↔ view 映射
4. `uc-bootstrap` `builders.rs` / `non_gui_runtime.rs` 读 settings → 构造 `IrohNodeConfig`

**为什么先做后端：**
* 前端没有"NetworkSection 真实化"之前，PUT 请求里不带 `network` 字段，DTO 会接受空 → 走 `serde(default)` 默认值，**不会 break 任何东西**。后端单独跑得通。
* 反过来不行：前端先发 `network.allowRelayFallback`，后端没字段，要么 422，要么字段被忽略，没法验证开关真生效。

**验收标准：**
* 手工把 settings.json 里加 `"network": { "allow_relay_fallback": false }`，重启 daemon → 日志看到 `disable_relays = true`、bind 时 RelayMode = Disabled
* HTTP PUT `/settings` 带 `network` 段 → 写盘成功，`GET /settings` 返回字段一致

### Phase B · 前端 NetworkSection + RestartHint

5. `src/api/daemon/settings.ts` Settings interface 加 `network` 字段
6. `setting-context` 流转 `network.allowRelayFallback`
7. `NetworkSection.tsx` 替换占位，渲染 LAN-only Mode 开关（toggle 显示状态 = `!allowRelayFallback`）
8. RestartHint 模态：切换后弹"重启 daemon 生效"

**为什么后做：** 这一步 user-facing，依赖 Phase A 的 HTTP 通道。Phase A 跑通后这一步纯前端工作。

### Phase C · 连接通道指示器（独立 phase，可与 B 并行）

9. `uc-core::ConnectionChannelPort` + `ConnectionChannel` enum
10. `uc-infra::IrohConnectionChannelAdapter` + `IrohNode` 暴露访问器（或 `install_*` 风格扩展）
11. `uc-application::MemberRosterDeps` / `PeerSnapshotView` 加 `channel` 字段
12. `uc-bootstrap::space_setup` 装配 adapter
13. DTO 加 `channel` 字段
14. 前端 `ConnectionChannelBadge` 组件 + `SpaceMembersPanel` 挂载

**为什么独立：** 这是新增能力，与 LAN-only Mode 字段解耦。即使 Phase B 的开关没做，Phase C 也能独立 ship 让用户"看到"当前是 LAN 还是 Relay（仅观察）。两者最终在 onboarding tip 时合一（"开 LAN-only → 看徽章变化"）。

### Phase D · onboarding tip + 文档（最后）

15. 配对成功后一次性 tip
16. 文档更新（"LAN-only" 边界，配对仍走 rendezvous）

---

## 6. 留作后续（不在本里程碑）

* **运行时热切换** —— 需要重建 endpoint。技术上需要 `IrohNodeBuilder::bind_bare` 风格的二次 bind，并处理 ALPN handler 重新注册、活跃 connection 重连、session 状态保持。已被用户决策推到下一里程碑。
* **通道切换专用 WS 事件** —— 当前依赖 `peers.changed` 全量快照刷新，在"直连断了回落 relay"场景下可能慢一拍。如果用户反馈强烈再做。
* **自托管 rendezvous 自动化部署** —— `IrohNodeConfig.rendezvous_base_url` 字段已 pub，本里程碑不暴露给用户；只在文档里点一下"是已有钩子，未来会做"。
* **Anti-pattern 警惕：** 不要把 `network.allow_relay_fallback` 落到 `daemon` 进程的进程变量里再让 daemon 内部依赖。settings 真相源是 settings.json，daemon 启动时读一次，运行期不变；任何业务模块如果想"动态读 LAN-only 状态"都是错的——本里程碑**不存在**这种需求（开关只影响 bind 时刻）。

---

## 7. Sources

| 来源 | 类型 | 置信度 |
|------|-----|------|
| `src-tauri/crates/uc-infra/src/network/iroh/node.rs:153-411` | 直接 grep | HIGH |
| `src-tauri/crates/uc-infra/src/settings/repository.rs:77-111` | 直接 grep | HIGH |
| `src-tauri/crates/uc-infra/src/settings/migration.rs:38-101` | 直接 grep | HIGH |
| `src-tauri/crates/uc-application/src/facade/settings/{facade,models,mod}.rs` | 直接 grep | HIGH |
| `src-tauri/crates/uc-core/src/settings/{model,defaults}.rs` | 直接 grep | HIGH |
| `src-tauri/crates/uc-core/src/ports/presence.rs` | 直接 grep | HIGH |
| `src-tauri/crates/uc-bootstrap/src/{builders,non_gui_runtime,space_setup}.rs` | 直接 grep | HIGH |
| `src-tauri/crates/uc-webserver/src/api/{settings,server}.rs` | 直接 grep | HIGH |
| `src-tauri/crates/uc-daemon-contract/src/api/types.rs:33-118` | 直接 grep | HIGH |
| `src-tauri/crates/uc-desktop/src/daemon/peers/presence_monitor.rs:1-120` | 直接 grep | HIGH |
| `src-tauri/crates/uc-tauri/src/commands/mod.rs` | 直接 grep（确认无 settings command） | HIGH |
| `src/components/setting/{NetworkSection,settings-config}.tsx` | 直接 grep | HIGH |
| `src/components/device/SpaceMembersPanel.tsx`, `src/pages/DevicesPage.tsx`, `src/api/daemon/settings.ts`, `src/store/slices/devicesSlice.ts` | 直接 grep | HIGH |
| `.planning/PROJECT.md` 与 `.context/attachments/Summary of Explore LAN version need.md` | 用户决策来源 | HIGH |
| iroh 0.98 `Endpoint::remote_info` API（替代 `conn_type`） | 通过 `connect.rs:45-67` 已有用例验证 | HIGH |
