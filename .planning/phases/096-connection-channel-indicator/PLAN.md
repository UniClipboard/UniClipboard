# Phase 96 — 连接通道指示器

**里程碑：** v0.7.0 LAN-only Mode
**Requirements：** INDIC-01 / INDIC-02 / INDIC-03 / INDIC-04
**依赖：** Phase 94（`network.allow_relay_fallback` 已落地）
**起草：** 2026-05-05
**实施方式：** 直接动手实施（用户明确 "不走 gsd skill 流程"），所有改动一次性放进 `mkdir700/phase-96` 分支。

---

## Goal（一句话）

让用户在设备列表与 system tray 都能直观看到当前 LAN-only Mode 状态以及每台已配对设备走的是 LAN 直连 / Relay 中继 / Offline / Unknown / Out of LAN，从而可肉眼验证 Phase 95 的开关效果；通道判定来自 infra 层单一真相源，前后通过事件 + polling 双路径刷新。

---

## Success Criteria 映射（来自 ROADMAP §Phase 96）

| # | 描述 | 落地点 |
|---|---|---|
| 1 | 设备列表至少 4 态可见（LAN/Relay/Offline/Unknown），通道值来自 infra 层 `ConnectionChannelPort` 单点产出，UI 同时订阅 `peers.changed` + 5–15s polling 双路径 | `IrohConnectionChannelAdapter` + `ConnectionChannelBadge` + `SpaceMembersPanel` 既有 WS 订阅 + `DevicesPage.tsx` 既有 `PRESENCE_REFRESH_INTERVAL_MS = 15_000` polling |
| 2 | hover 通道徽章可见 tooltip，特别 "Relay = 加密中继，元数据可见" | `ConnectionChannelBadge` + i18n `devices.list.channel.tooltip.*` |
| 3 | LAN-only ON 状态下跨网段设备显示为灰 "Out of LAN" + tooltip；恢复同网段后徽态自动回到 LAN | `deriveBadgeKind(channel, lanOnlyActive)` 纯函数合成；恢复路径由既有 `peers.changed` + polling 自然触发 |
| 4 | system tray icon 上可视化当前 LAN-only Mode 启用状态 | `TrayState::init(app, lang, lan_only_active)` + tray tooltip + 不可交互状态菜单行 |
| 5 | `PeerSnapshotDto.channel: String` 取值严格 `"direct"\|"relay"\|"offline"\|"unknown"`；DTO ↔ view 映射有双向单测；`IrohConnectionChannelAdapter` 经 `endpoint.remote_info → 过滤 Active TransportAddrInfo → Ip⇒Direct/Relay⇒Relay/空⇒Unknown` 推导，IPv6 ULA filter 顺手覆盖 | `connection_channel_to_wire` + `wire_strings_are_locked` 单测 + `derive_channel_from_addrs` + `is_filtered_ip` truth-table 单测 |

---

## Pitfall 防御

- **Pitfall 4（通道偏差）**：`ConnectionChannelPort` 在 `uc-core` 单点定义，唯一实现 `IrohConnectionChannelAdapter` 在 `uc-infra`；application 层透传不解释；`ConnectionChannel::default() == Unknown` 强制显式可见；不缓存（每次 `channel_for` 都跑 `endpoint.remote_info` snapshot）。
- **Pitfall 7（IPv6 ULA / 跨平台）**：`is_filtered_ip` 在 channel 推导处过 `fc00::/7` ULA + `fe80::/10` link-local，避免 iroh 偶发把它们当 Active path 上报时被 UI 误判 LAN 直连。**节点级 `AddrFilter` 不动**——那个影响 outbound dial 候选，调它会改变连接行为。
- **Pitfall 1（反向命名）传染防御**：`connection_channel_to_wire` 单点产出 wire 字符串，`SpaceMembersPanel` 中 `lanOnlyActive = setting?.network?.allowRelayFallback === false`（与 NetworkSection 同源唯一翻译点）。

---

## 实施路径（已落地）

### 1. uc-core — `ConnectionChannelPort` + `ConnectionChannel` enum
- `crates/uc-core/src/ports/connection_channel.rs`：4 态枚举 + `channel_for(&DeviceId) -> ConnectionChannel` async trait + 显式 Default=Unknown + 单测
- `crates/uc-core/src/ports/mod.rs`：`pub mod connection_channel;` + 重新导出

### 2. uc-infra — `IrohConnectionChannelAdapter`
- `crates/uc-infra/src/network/iroh/connection_channel_adapter.rs`：基于 `endpoint.remote_info()` snapshot 推导，Direct > Relay 优先级；IPv4 假段（同 `node.rs::is_virtual_nic_ip`）+ IPv6 ULA / 链路本地过滤；`derive_channel_from_addrs` + `is_filtered_ip` 纯函数单测覆盖 truth-table
- `crates/uc-infra/src/network/iroh/node.rs`：新增 `IrohNodeBuilder::install_connection_channel(peer_addr_repo)` 方法，复用同一 endpoint，不装 ALPN handler
- `mod.rs` 重新导出 `IrohConnectionChannelAdapter`

### 3. uc-application — 贯穿 view 层
- `PeerSnapshotView.channel: ConnectionChannel`
- `MemberRosterFacade::list_peer_snapshots` 在 entry 拼装时调 `connection_channel.channel_for`，缺省时 `Unknown`
- `MemberRosterDeps.connection_channel: Option<Arc<dyn ConnectionChannelPort>>`
- `roster/mod.rs::connection_channel_to_wire`：`Direct/Relay/Offline/Unknown` → `"direct"/"relay"/"offline"/"unknown"`，`wire_strings_are_locked` 单测锁字面值
- `facade/mod.rs` 重新导出 `ConnectionChannel` + `connection_channel_to_wire`

### 4. uc-daemon-contract / uc-webserver / uc-desktop — DTO 映射
- `PeerSnapshotDto.channel: String`、`SpaceMemberDto.channel: String`
- `presence_monitor.rs::peer_snapshot_to_dto` + `server.rs::peer_snapshots` + `server.rs::paired_devices` 三处映射点都补 channel 字段，统一调 `connection_channel_to_wire`

### 5. uc-bootstrap — 装配
- `space_setup.rs`：`builder.install_connection_channel(peer_addr_repo)` 紧跟 `install_presence` 之后；`MemberRosterDeps.connection_channel = Some(...)`
- 既有两个 e2e tests 显式 `connection_channel: None`（不验证 channel 字段）

### 6. 前端 — 类型 + Badge + 集成
- `src/api/daemon/members.ts`：`ConnectionChannel` 类型 + `SpaceMember.channel`
- `src/hooks/useDaemonEvents.ts`：`PeersChangedPayload.peers[].channel?: ...`
- `src/components/device/ConnectionChannelBadge.tsx`：5 态视觉（lan/relay/offline/unknown/outOfLan）；`deriveBadgeKind(channel, lanOnlyActive)` 纯函数；hover tooltip
- `src/components/device/SpaceMembersPanel.tsx`：每个 device row 增加 `<ConnectionChannelBadge channel={device.channel ?? 'unknown'} lanOnlyActive={...} />`；`lanOnlyActive = setting?.network?.allowRelayFallback === false`
- `i18n/locales/en-US.json` + `zh-CN.json`：新增 `devices.list.channel.{lan,relay,offline,unknown,outOfLan}` + `tooltip.*` 共 10 条 key
- `src/components/device/__tests__/ConnectionChannelBadge.test.tsx`：truth-table 10 用例覆盖 5 态合成 + 渲染断言

### 7. uc-tauri — Tray
- `tray.rs::TrayState::init(app, lang, lan_only_active)`：新增 status 状态行（不可交互）+ tooltip 后缀（"UniClipboard — LAN-only Mode is ON"）；中英双语；`set_language` 同步刷新状态文案 + tooltip
- `run.rs`：startup 加载 settings 时附带读 `network.allow_relay_fallback`，反向命名翻译 `lan_only_active = !allow_relay_fallback`，喂给 tray init

---

## 测试清单

| 层 | 测试 | 命令 |
|---|---|---|
| uc-core | `default_is_unknown` | `cargo test -p uc-core ports::connection_channel` |
| uc-infra | `ipv4_filter_truth_table` + `ipv6_filter_covers_ula_and_link_local` | `cargo test -p uc-infra connection_channel_adapter` |
| uc-application | `wire_strings_are_locked` | `cargo test -p uc-application roster::wire_tests` |
| uc-bootstrap | 既有 slice2 e2e tests 仍通过（`connection_channel: None`） | `cargo test -p uc-bootstrap` |
| 前端 | Badge 5 态 truth-table + i18n 渲染 10 用例 | `bun run test src/components/device/__tests__/` |

---

## UAT（用户人工验收）

P96-UAT-1 设备列表 hover Relay 徽章看到 "加密中继，元数据可见" 文案；
P96-UAT-2 切换 LAN-only Mode = ON + 重启后,跨网段设备徽章变灰 "Out of LAN",同网段设备保持 "局域网"；
P96-UAT-3 system tray hover 看到 "UniClipboard — LAN-only Mode is ON" tooltip,展开菜单看到状态行 "LAN-only Mode: ON"；
P96-UAT-4 关闭 LAN-only + 重启,徽章 / tray 状态恢复未开启文案。

---

## 不在范围内

- 运行时热切换：iroh `RelayMode` bind-time 常量，本里程碑承担"重启生效"语义；tray 状态在进程内不随设置变化更新（避免给用户 "切完即生效" 的错觉）。
- 差异图标资产：success criteria #4 给 "差异图标 OR 状态徽章" 二选一，本实现选状态文案 + tooltip 双重披露，不引入新 PNG/ICNS 资产，避免跨平台 icon 渲染差异。
- onboarding banner / `docs/lan-only.md` / changelog：归 Phase 97。

---

## 提交计划

- 分支：`mkdir700/phase-96`（已切好）
- 后端 commit `53dbf76d`：`feat(phase-96): backend connection channel port + iroh adapter wired through DTO`
- 前端 + tray + 文档 commit（即将）：`feat(phase-96): frontend channel badge + tray LAN-only status`
