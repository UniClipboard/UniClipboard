# Technology Stack: v0.7.0 LAN-only Mode（增量调研）

**项目:** UniClipboard Desktop
**里程碑:** v0.7.0 LAN-only Mode
**调研日期:** 2026-05-04
**关注点:** 新增/微调,不替换任何现有栈
**整体置信度:** HIGH（核心 API 与版本均经 docs.rs/0.98.x 与项目源码双重确认）

---

## 里程碑增量摘要

这次里程碑**只在五个明确点上做加法**,不引入新依赖、不替换任何既有 stack。所有目标技术全部已经在 lockfile 里:

| 维度 | 改动性质 | 一句话 |
| --- | --- | --- |
| **iroh relay state API** | 仅消费,无新依赖 | 用 0.98 既有 `Endpoint::remote_info(id) -> Option<RemoteInfo>` + `TransportAddrInfo::usage()/addr()` 判定通道类型,项目代码已经在用这条 API（`connect.rs:51`、`blobs.rs:135`） |
| **Settings `network` namespace** | 加结构体 + 加 migration | 跟随现有 `serde + serde_with::serde_as + #[serde(default)]` 模式,在 `uc_core::settings::model::Settings` 顶层加 `pub network: NetworkSettings`(注释里早就预留),`uc-infra` 的 `SettingsMigrator` 列表里追加 V1→V2 |
| **前端开关 UI** | **不增依赖** | `@radix-ui/react-switch ^1.2.6` 与 `lucide-react`(`Wifi` icon)都在 `package.json:62/77`,`SettingRow`/`SettingGroup`/`useSetting` 三件套现成,直接替换 `NetworkSection.tsx:11-23` 占位符 |
| **可观察性接入** | 不动 crate,改 span/字段命名 | 沿用 `uc-observability` 的 dotted-name span(参考 `stages.rs`),把"通道类型"作为字段(`network.channel = lan|relay|offline`)挂在已有的 connect/dispatch span 上;不新增 OTLP exporter,不动 `LogProfile` |
| **测试套接入** | 不增工具 | 跟随 `uc-bootstrap/tests/slice2_phase1_presence_e2e.rs` 既有 loopback 双 endpoint 模式,用 `RelayMode::Disabled` 验证"LAN-only=true ⇒ `addr().addrs` 不含 Relay 项",再用 `RelayMode::Default` 验证回归 |

**一条铁律:** 范围内禁止替换 iroh / sqlx / Diesel / React 任何一项。这个里程碑就是把 `IrohNodeConfig.disable_relays`(`node.rs:161`)从内部测试 hook 暴露成用户可控开关 + 加一个观察指示器,不是重写网络栈。

---

## 现状基线（不要再改、调研也不必复述全栈）

| 技术 | 版本 | 锁定位置 | 与本里程碑关系 |
| --- | --- | --- | --- |
| iroh | **0.98**（features = `address-lookup-mdns`） | `src-tauri/crates/uc-infra/Cargo.toml:79` | 关键依赖,所有 relay 状态 API 都从这里来 |
| iroh-blobs | 0.100 | 同上:80 | 不动 |
| iroh-tickets | 0.5 | 同上:81 | 不动 |
| noq-proto | 0.17（quinn 分叉,提供 BBR） | 同上:85 | 不动 |
| Rust + Diesel(SQLite) | edition 2021, diesel 2.3.5 | 同上:30 | 不动(settings 走 JSON 文件,非 SQLite) |
| serde / serde_with | 1.x / 任意（已用 `DurationSeconds`） | `uc-core/src/settings/model.rs:1-5` | 复用既有派生模式 |
| React | ^18.3.1 | `package.json:81` | 不动 |
| @tauri-apps/api | ^2.9.1 | 同上:67 | 不动 |
| @radix-ui/react-switch | ^1.2.6 | 同上:62 | 直接复用做 LAN-only toggle |
| lucide-react | ^0.577.0(已含 `Wifi`) | 同上:77 | `settings-config.ts:9` 已经引用 |
| @sentry/react + @tauri-apps/plugin-log + pino | 已装 | 不动 | 不需要新增 telemetry exporter |

> 现有 settings 持久化是 **JSON 文件 + atomic rename**(`FileSettingsRepository`,`uc-infra/src/settings/repository.rs:77-110`),不是 SQLite 表。`SettingsMigrator`(`uc-infra/src/settings/migration.rs:36-43`)的 `migrations` 列表当前**为空**,本里程碑会写第一条迁移。

---

## 维度 1: iroh relay 状态 API（HIGH 置信度）

### 1.1 推荐用什么

| Symbol | Crate / 路径 | 适用版本 | 用途 |
| --- | --- | --- | --- |
| `iroh::Endpoint::remote_info(id) -> Option<RemoteInfo>` | `iroh::Endpoint` | **iroh 0.98**（已在用） | 拿某个 peer 当前的对端连接快照(async,非 watcher) |
| `iroh::endpoint::RemoteInfo::addrs() -> impl Iterator<Item = &TransportAddrInfo>` | `iroh::endpoint::RemoteInfo` | iroh 0.98 | 遍历"已知所有传输地址",含 IP 与 Relay |
| `iroh::endpoint::TransportAddrInfo::usage() -> TransportAddrUsage` | `iroh::endpoint::TransportAddrInfo` | iroh 0.98 | 区分该地址是 `Active`(正在用)还是 `Inactive` |
| `iroh::endpoint::TransportAddrInfo::addr() -> &TransportAddr` | `iroh::endpoint::TransportAddrInfo` | iroh 0.98 | 拿到底层 `TransportAddr` |
| `iroh::TransportAddr::{Ip(SocketAddr), Relay(RelayUrl)}` + `is_ip()/is_relay()` | `iroh::TransportAddr` | iroh 0.98 | LAN 直连 vs 中继判定的真相源 |
| `iroh::endpoint::TransportAddrUsage::{Active, Inactive}` | `iroh::endpoint::TransportAddrUsage` | iroh 0.98 | 判定"当前是否在收发包",项目代码已用此 enum(`blobs.rs:139`、`connect.rs:56`) |
| `iroh::Endpoint::watch_addr() -> impl Watcher<Value = EndpointAddr>` | `iroh::Endpoint` | iroh 0.98 | **本端**地址变化的 watcher;不直接给"对端连接通道翻转"事件,但可作为路径重协商的间接信号 |
| `iroh::endpoint::Connection::closed()` | `iroh::endpoint::Connection` | iroh 0.98 | Offline 信号的主依据,项目 `IrohPresenceAdapter` 已用(`presence_adapter.rs:97`) |

### 1.2 为什么是这条不是别的

* **iroh 0.95 时代的 `Endpoint::conn_type(id) -> Watcher<ConnectionType>` 已经在 0.97/0.98 移除**,docs.rs/iroh/0.98.x 上 `RemoteInfo` 没有 `conn_type` 公开字段;项目源码注释也写明了这次迁移(`blobs.rs:122-128`、`connect.rs:45-50`、`tests/iroh_presence_probe.rs:5-11`)。**不要在新代码里再去找 `conn_type` 这条路径**——会冷不丁地编译过(因为内部 `node_state` 模块还有 enum)但拿不到调用句柄。
* **`remote_info` 是 snapshot,不是 watcher**——这正合适本里程碑:连接通道指示器是"按需呈现",每次 UI 拉取或事件触发时重读一次即可,不必维持订阅。docs.rs/0.98 明确写"a snapshot in time, i.e. it is not updating"。
* **`watch_addr()` 是本端地址变化**,不是对端通道翻转;不要为了"通道翻转推送"去用它,会拿到错误粒度的事件。如果未来要做事件驱动的"LAN→Relay 退化通知",更合适的钩子是 iroh tracing target `iroh::_events::conn_type::changed`(在文档检索结果里看到 iroh 内部以 `tracing::event!` 形式发出,但这是内部 target,不属于稳定公开 API,**v0.7.0 不依赖它**)。
* **判定算法(载入指南):**
  ```text
  IF endpoint.remote_info(id).await is None
      → channel = Offline
  ELSE iterate info.addrs():
      let active = addrs.filter(|a| matches!(a.usage(), Active)).collect()
      IF active is empty                                  → Offline
      ELSE IF active.iter().all(|a| a.addr().is_ip())     → LAN-direct
      ELSE IF active.iter().all(|a| a.addr().is_relay())  → Relay
      ELSE                                                → Mixed（建议归类为 LAN-direct,UI 显示直连优先）
  ```
  这个算法跟 `connect.rs:51-66` 已有的 `conn_type_str` 渲染共用一套字段,后续替换日志格式化为指标暴露时不会割裂。

### 1.3 不要做的事

* ❌ 不要 fork iroh、不要再加新的 iroh feature flag(本里程碑唯一新增的"通道感知"代码就是 ~30 行的 helper)
* ❌ 不要新增 iroh-relay/iroh-net 等子 crate
* ❌ 不要试图运行时切 `RelayMode`——`Endpoint::builder().relay_mode(...)` 是 bind 时定的,iroh 0.98 没有运行时切换 API(参考 `node.rs:368-372`),这就是本里程碑"重启生效"决策的根本原因

---

## 维度 2: Settings `network` 命名空间扩展（HIGH 置信度）

### 2.1 现有模式速览

`uc-core/src/settings/model.rs` 走的是**纯 serde 派生 + `#[serde(default)]` 兜底 + 手写 `Default` impl** 的模式:

* 顶层 `Settings`(`model.rs:177-203`)用 `#[derive(Debug, Clone, Serialize, Deserialize)]`,每个子结构都标了 `#[serde(default)]`,缺字段就走子结构的 `Default::default()`(`defaults.rs`)
* 时长字段统一 `#[serde_as(as = "DurationSeconds<u64>")]`(`model.rs:88` 等),u32/bool/枚举默认 derive
* enum 用 `#[serde(rename_all = "snake_case")]`(参考 `Theme`/`SyncFrequency`)
* `CURRENT_SCHEMA_VERSION: u32 = 1`(`model.rs:7`)、`SettingsVersion::V1`(`version.rs`)是 schema 版本号双源,改一处都要同步另一处
* **行号锚点**:`model.rs:201-202` 已有注释 `// pub network: NetworkSettings,` 占位

### 2.2 推荐扩展方式

| 步骤 | 文件 | 动作 |
| --- | --- | --- |
| 2.1 加结构体 | `uc-core/src/settings/model.rs` | 在 `Settings` 顶层加 `#[serde(default)] pub network: NetworkSettings,`,新增 `pub struct NetworkSettings { #[serde(default = "default_allow_relay_fallback")] pub allow_relay_fallback: bool }`,默认 `true` |
| 2.2 加默认实现 | `uc-core/src/settings/defaults.rs` | 加 `impl Default for NetworkSettings`,`allow_relay_fallback: true`;在 `Settings::default`(`defaults.rs:251-262`)的字段列表里追加 `network: NetworkSettings::default()` |
| 2.3 升 schema | `uc-core/src/settings/model.rs` + `version.rs` | `CURRENT_SCHEMA_VERSION` 升到 `2`,`SettingsVersion` 加 `V2` 变体(`version.rs:1-28`),`as_u32` 匹配 |
| 2.4 写 migration | `uc-infra/src/settings/migration.rs` | 在 `SettingsMigrator::new()`(`migration.rs:36-43`)的 `migrations` 列表里加 `Box::new(MigrationV1ToV2)`;新文件实现 `SettingsMigrationPort`,从 `from_version() -> 1`,`migrate(s)` 把 `s.schema_version = 2` 并保留默认 `network`(因为 `#[serde(default)]` 兜底,旧 JSON 解析时 `network` 字段就已经填默认值了——migration 主要是把 `schema_version` bump 到 2,触发后续 `if original_version < CURRENT_SCHEMA_VERSION { self.save(&migrated).await? }` 落盘) |
| 2.5 facade view/patch | `uc-application/src/facade/settings/models.rs` | 复刻 `FileSyncSettingsView/FileSyncSettingsPatch` 的形式新增 `NetworkSettingsView { allow_relay_fallback: bool }` 和 `NetworkSettingsPatch { allow_relay_fallback: Option<bool> }`,在 `SettingsView`(`models.rs:134-143`)、`SettingsPatch`(`models.rs:218-226`)、`apply_settings_patch`(`models.rs:446-566`)、`From<core::Settings>`(`models.rs:386-444`)四处补对应分支 |
| 2.6 facade re-export | `uc-application/src/facade/settings/mod.rs` | `pub use models::{...}` 列表加 `NetworkSettingsPatch, NetworkSettingsView`(参考现有 11 个 view/patch 的风格,`mod.rs:5-11`) |
| 2.7 webserver DTO | `uc-webserver/src/api/dto/settings.rs`(此文件目前在 `dto/` 目录下不存在,需要从 `uc_daemon_contract` 模块移植,参考 `crates/uc-webserver/src/api/settings.rs:14-20` 的 import) + `uc-webserver/src/api/settings.rs` 的 `settings_patch_from_dto` / `settings_view_to_dto`(`settings.rs:92-218`) | 加 `NetworkSettingsDto / NetworkSettingsPatchDto`,在 `SettingsDto` / `SettingsPatchDto` 里挂上,改两个映射函数 |
| 2.8 OpenAPI schemas | `uc-webserver/src/api/openapi.rs` | 在 `dto::settings::{...}` 列表加 `NetworkSettingsDto`(`openapi.rs:30-34` 是当前列表) |

### 2.3 为什么这条路径

* **现有架构是文件级 JSON + serde**,不是 SQLite 表——所以**没有 SQL DDL 迁移**,只有 `SettingsMigrator` 的 `from_version → migrate(Settings) → Settings` 单步函数。新增 `network` 子结构最干净的做法就是借助 `#[serde(default)]` 让旧 JSON 自动填默认值,V1→V2 migration 只负责 bump schema_version,不需要拷字段
* **反向命名 `allow_relay_fallback` 已经是产品决策**(`PROJECT.md:31`)——后端字段保留 infra 中性语义,UI 翻译为"LAN-only Mode"开关呈现(toggle 关闭 = `allow_relay_fallback = false`)
* **不要走表结构**——SettingsRepo 的 SQLite 在加密层做 keyslot,设置数据本身没用 Diesel ORM(`repository.rs:8-13` 显示只用 `serde_json` + tokio fs),引入表结构会偏离整个 settings 子系统的设计
* **不能用单一布尔放 `GeneralSettings`**——产品决策已明确 Network 是独立分类(`PROJECT.md:84`、`settings-config.ts:54-58`),将来还会塞自定义 OTLP endpoint、网络诊断等,structurally separate 现在就要立住

### 2.4 链路注入 `IrohNodeConfig`

`uc-bootstrap/src/space_setup.rs:178/180/210` 当前用的是 `IrohNodeConfig::default()`(`disable_relays = false`)。注入路径:

```text
启动时 in build_space_setup_assembly →
  load Settings via SettingsPort →
  let cfg = IrohNodeConfig {
      disable_relays: !settings.network.allow_relay_fallback,
      rendezvous_base_url: None,
  };
  → 传给 IrohNodeBuilder::bind(...)
```

**注意**:`build_space_setup_assembly` 当前签名直接接 `IrohNodeConfig`(`bootstrap/src/space_setup.rs:210`),要么把 `Settings` 也传下去构造 cfg、要么在调用端构造好再传 —— 后者改动小,推荐。

---

## 维度 3: 前端 UI（HIGH 置信度,无新依赖）

### 3.1 现成的零件

| 组件 / 库 | 锁定位置 | 用途 |
| --- | --- | --- |
| `Switch` from `@/components/ui` | `package.json:62`(`@radix-ui/react-switch ^1.2.6`) | LAN-only toggle |
| `Wifi` icon from `lucide-react` | `package.json:77`,已在 `settings-config.ts:9` 引用 | NetworkSection 侧栏图标(已挂) |
| `SettingGroup` / `SettingRow` / `useSetting` | `src/components/setting/SettingGroup.tsx` / `SettingRow.tsx` / `src/hooks/useSetting.ts` | 与 `SyncSection.tsx`、`GeneralSection.tsx` 完全同款的设置项 UI 套件 |
| `useTranslation` (i18next) | `package.json:75/84` | 标签/说明文案 |
| `sonner`(toast) | `package.json:92` | "重启生效"提示用 |
| `Badge` from `@/components/ui` | 已在 `SyncSection.tsx:5` 用 | "需要重启" 状态徽章 |
| 可选: `@radix-ui/react-tooltip` | `package.json:64` | 通道指示器的 hover 解释 |
| 可选: `@radix-ui/react-dialog` | `package.json:49` | 切换开关后的"重启确认" 模态(若不想只用 sonner toast) |

### 3.2 不需要新增的依赖

* **不需要** WebSocket 客户端 —— 现有 `daemon-ws-bootstrap` 已建立长连(`src/lib/daemon-ws-bootstrap.ts`)
* **不需要** 状态管理库 —— 已经在用 `@reduxjs/toolkit ^2.11.2`(`package.json:65`)和 React Context(`src/contexts/setting-context.ts`)
* **不需要** 新图标库 —— `lucide-react` 已含 `Wifi / WifiOff / Server / Cable` 等可表达通道状态的图标
* **不需要** 新 toast 库 —— `sonner` 已装

### 3.3 替换 NetworkSection 占位符

`src/components/setting/NetworkSection.tsx` 当前是 23 行的占位组件(`NetworkSection.tsx:11-23`)。本里程碑直接改成正文,沿用 `SyncSection.tsx:16-22` 的 hook+local state 模式:

```text
useSetting() → setting.network.allowRelayFallback →
  Switch checked={!setting.network.allowRelayFallback} →
  onCheckedChange → updateNetworkSetting({ allowRelayFallback: !checked }) →
    + sonner.toast.info("Restart required for the new mode to take effect")
```

`useSetting()` 的具体结构在 `src/contexts/setting-context.ts`(本研究未读全,Planner 阶段需补一个 `updateNetworkSetting(patch)` 方法,与 `updateSyncSetting` 同形)。

### 3.4 设备列表通道指示器

设备列表 UI 不在本研究文件范围内枚举,但**所需基础组件已经齐备**:`Badge` + lucide `Wifi`/`Server`/`WifiOff` 三态图标 + 已有的 PresenceEvent 广播(`uc_core::ports::PresenceEvent`,见 `presence_adapter.rs:48-49`)。后端再加一个"通道类型快照查询"的 facade 方法(基于 §1.2 算法),前端 polling 或订阅事件即可——**不需要新增前端依赖**。

---

## 维度 4: 可观察性（HIGH 置信度,不动 crate）

### 4.1 现状

* `uc-observability` 走 OTLP/HTTP-protobuf 单 exporter(`lib.rs:8`,Phase 87 retire 了 Seq/CLEF)
* span 命名采用 dotted 风格,见 `stages.rs`(如 `clipboard.outbound_send`)
* `LogProfile` 已配置 `iroh::socket=info`、`iroh::socket::remote_map::remote_state=error` 等噪音过滤(`profile.rs:53-54`)
* `telemetry_enabled` 字段已经在 `GeneralSettings`(`model.rs:25-26`)——**LAN-only 与 telemetry 是两件事**,产品决策(本调研外的 explore 阶段)是 LAN-only **不**自动关 telemetry,本里程碑保持现状

### 4.2 推荐做法(纯字段/span 增量,不增 crate)

| 工件 | 命名 | 规则 |
| --- | --- | --- |
| 新 span 名 | `network.channel_probe` | 包装 §1.2 那段判定算法的 helper,放 `uc-infra/src/network/iroh/`(新文件 `channel.rs` 或并入 `connect.rs`),attrs = `{ peer = %id.fmt_short(), channel = "lan|relay|offline" }` |
| 现有 span 加字段 | `connect.rs:68-74` 的 "iroh connect selected path" log 已经在用 `conn_type = %conn_type_str` —— 改为受控枚举值 `lan/relay/offline/mixed` 而非 raw addr 列表,前端拿到的指示器与日志面板字段同源 | |
| 设置变更 audit | `settings.network.allow_relay_fallback.changed` event | 在 `SettingsFacade::update`(`facade/settings/facade.rs:35-48`)的 `instrument` 已经覆盖,只需在 `apply_settings_patch` 命中 network 分支时 `tracing::info!(target: "settings.network", from = ?old, to = ?new, "LAN-only mode changed")` |
| `LogProfile` | **不动** | 不需要新 profile,新增字段自动随现有 profile 走 |

### 4.3 不要做的事

* ❌ 不要为 v0.7.0 加新的 OTLP attribute schema(语义约定层面),把 channel 当成 string field 即可
* ❌ 不要新增专门的 metrics(counter/gauge)—— `uc-observability` 当前不暴露 metrics 层,只有 logs+spans;v0.7.0 不是引入 metrics 的时机
* ❌ 不要用 iroh 内部 tracing target `iroh::_events::conn_type::changed` 作为外部接口的真相源——内部 target,会 break

---

## 维度 5: 测试栈（HIGH 置信度,无新工具）

### 5.1 现成测试设施

| 工具 | 锁定位置 | 用法参考 |
| --- | --- | --- |
| `tokio` test runtime | `uc-infra/Cargo.toml:99`(`features = ["full", "test-util"]`) | 见 `tests/iroh_presence_probe.rs` |
| `tempfile = "3"` | `uc-infra/Cargo.toml:100` | settings repo 文件系统测试 |
| `mockall = "0.13"` | `uc-infra/Cargo.toml:101` | port mock |
| `wiremock = "0.6"` | `uc-infra/Cargo.toml:102` | rendezvous HTTP mock(LAN-only 测试不需要,但已可用) |
| 双 endpoint loopback fixture | `uc-bootstrap/tests/slice2_phase1_presence_e2e.rs:354-356` | 已经在用 `IrohNodeConfig { disable_relays: true, .. }` |
| `iroh::RelayMode::Disabled` 直连 probe | `uc-infra/tests/iroh_presence_probe.rs:17-29` | 不依赖外网的 endpoint bind 模板 |

### 5.2 推荐的新测试文件

| 文件 | 关注点 | 模板 |
| --- | --- | --- |
| `uc-infra/tests/lan_only_relay_mode.rs`(新增) | bind 时 `disable_relays=true` ⇒ `Endpoint::addr().addrs` 不含 `TransportAddr::Relay`;`disable_relays=false` ⇒ 含 | 复制 `iroh_presence_probe.rs:22-29` 的 `bind_endpoint` 套路,断言 `endpoint.addr().addrs.iter().any(|a| matches!(a, TransportAddr::Relay(_)))` |
| `uc-application/src/facade/settings/facade.rs` mod tests(扩展) | `update(NetworkSettingsPatch)` round-trip;default `allow_relay_fallback = true` | 跟随 `facade.rs:51-129` 已有的 `InMemorySettings + SettingsFacade` fixture |
| `uc-infra/src/settings/migration.rs` mod tests(新增) | V1 JSON(没有 `network` 字段) → load → migrate → schema_version=2 + `network.allow_relay_fallback=true` | 没有现成 migration 测试样本,本里程碑**首次**立模板 |
| `uc-bootstrap/tests/space_setup_lan_only.rs`(新增,可选) | settings.allow_relay_fallback=false → `IrohNodeConfig.disable_relays=true` 注入;反向同理 | 复用 `slice2_phase1_presence_e2e.rs` 的 SpaceSetupAssembly 装配 |

### 5.3 不需要的工具

* **不需要** `testcontainers` —— 不引入容器化测试
* **不需要** Selenium/Playwright —— 前端已有 `vitest + @testing-library/react`(`package.json:103-105/136`)
* **不需要** 真 LAN 测试床 —— `RelayMode::Disabled` + loopback 完全足够覆盖核心契约;真"跨网段 fallback"行为在 v0.7.0 不需要 CI 验证(无可控 NAT 拓扑环境),归到手工冒烟

---

## 替换提议:**全部否决**

| 提议 | 否决理由 |
| --- | --- |
| ❌ 换 iroh 到 0.95 / 1.0.0-rc | 0.98 已锁,本里程碑不升级网络栈,跨版本 API 漂移会卡所有 phase |
| ❌ 用 SQLite 表存 settings.network | settings 是文件 JSON,Diesel 表与现有架构无关,新增表会引入 schema migration 复杂度 |
| ❌ 引入 zustand / jotai | Redux Toolkit + Context 已就位,不是 v0.7.0 该解决的问题 |
| ❌ 用 prom-client / metrics-rs | uc-observability 当前没有 metrics 层,本里程碑不引入新维度 |
| ❌ 新增独立 `uc-network-settings` crate | 一两个字段,放 `uc-core/src/settings/model.rs` 顶层 namespace 即可,新 crate 是过度设计 |

---

## 关键调用图（Planner 用）

```
[UI] NetworkSection (Switch)
   │
   ├─ useSetting().updateNetworkSetting({ allowRelayFallback })
   │      └─→ HTTP PUT /settings  body: SettingsPatchDto.network
   │
[Daemon HTTP] settings_patch_from_dto  ──▶
[Application] SettingsFacade::update   ──▶ apply_settings_patch
[Core]        Settings.network.allow_relay_fallback
[Infra]       FileSettingsRepository::save  (atomic write JSON)
   │
   └─[on next process start]
[Bootstrap]   build_space_setup_assembly (Settings → IrohNodeConfig)
[Infra]       IrohNodeBuilder::bind  (RelayMode = Disabled or Default)


[Channel indicator]
[UI] DeviceList badge
   │
   └─ 拉 channel snapshot：app_facade.devices.channel(device_id)
[Application] DeviceFacade::channel(id)   (新增 thin method)
[Infra]       endpoint.remote_info(id) → addrs().filter(Active) →
              { Ip→LanDirect, Relay→Relay, mixed→LanDirect, none/none→Offline }
```

---

## 来源

* iroh 0.98 RemoteInfo / TransportAddrInfo / TransportAddrUsage / TransportAddr — `https://docs.rs/iroh/0.98.0/` 与 `https://docs.rs/iroh/0.98.1/`(`RemoteInfo::addrs()` 返回 `impl Iterator<&TransportAddrInfo>` snapshot;`TransportAddrUsage::{Active, Inactive}`;`TransportAddr::{Ip(SocketAddr), Relay(RelayUrl)}` + `is_ip()/is_relay()`;`Endpoint::watch_addr()` 是本端地址 watcher)
* iroh 0.95→0.98 API 迁移记录(`conn_type` → `remote_info`)— `src-tauri/crates/uc-infra/tests/iroh_presence_probe.rs:5-11`、`src-tauri/crates/uc-infra/src/network/iroh/blobs.rs:122-128`、`connect.rs:45-50`
* 项目 iroh 版本锁 — `src-tauri/crates/uc-infra/Cargo.toml:79-80`
* `IrohNodeConfig` 与 `RelayMode` 决策点 — `src-tauri/crates/uc-infra/src/network/iroh/node.rs:152-162, 366-396`
* 已有 settings 序列化模式 — `src-tauri/crates/uc-core/src/settings/model.rs`、`defaults.rs`、`version.rs`
* 已有 settings 持久化 — `src-tauri/crates/uc-infra/src/settings/repository.rs`、`migration.rs`
* facade 视图/补丁模式 — `src-tauri/crates/uc-application/src/facade/settings/{facade.rs,models.rs,mod.rs}`
* webserver DTO 链路 — `src-tauri/crates/uc-webserver/src/api/settings.rs`、`openapi.rs:30-34`
* 前端依赖清单 — `package.json:38-94`
* 设置 UI 占位状态 — `src/components/setting/{NetworkSection.tsx,settings-config.ts,SyncSection.tsx}`
* 测试套样本 — `src-tauri/crates/uc-bootstrap/tests/slice2_phase1_presence_e2e.rs:354-356`、`src-tauri/crates/uc-infra/tests/iroh_presence_probe.rs`
* 探索阶段决策记录 — `.context/attachments/Summary of Explore LAN version need.md` 第 134-215 行(范围聚焦、UX 决策、MVP 拆分)
* 项目里程碑定义 — `.planning/PROJECT.md:11-33`(目标、P0/P1、范围外)
