# Project Research Summary

**项目：** UniClipboard Desktop — v0.7.0 LAN-only Mode
**研究域：** 给已上线的「局域网 + 公网中继混合系统」加一个用户可控的 LAN-only 开关 + 连接通道（LAN / Relay / Offline）可观察指示器
**调研日期：** 2026-05-04
**整体置信度：** HIGH（4 路 researcher 独立验证后一致；关键 API、行号、版本、产品决策全部可追溯）

---

## Executive Summary

这是一次**范围窄、改动小、信任价值大**的里程碑。技术上不重写网络栈、不引入新依赖、不动六边形分层；产品上回答 B 站用户的一句话："有没有局域网专用版？我能不能确定流量没出局域网？我怎么验证？"。落地手段就两件：(1) 把 `IrohNodeConfig.disable_relays`（已存在的内部测试钩子，`uc-infra/src/network/iroh/node.rs:161`）暴露成用户可控的「LAN-only Mode」开关 + 持久化字段 `network.allow_relay_fallback`；(2) 给设备列表加「LAN / Relay / Offline」连接通道徽章，让开关效果**可肉眼验证**。两件事产品强耦合（必须同期发布，否则单发开关 = 没担保的承诺，单发徽章 = 没人 care 的信息），但技术解耦（可并行开发）。

**整个里程碑的所有失败模式都收敛到两条主因**，是后续 phase 设计与 review 必须贴脑门的红线：

- **主因 A：反向命名导致的「语义颠倒」。** UI 文案 "LAN-only Mode = ON"，后端字段 `network.allow_relay_fallback` = `false`，iroh 字段 `IrohNodeConfig.disable_relays` = `true` —— **三层语义两次反转**。任何一处取反搞反编译器都不报错，但用户层面是「开了开关流量还在走中继」或「关了开关跨网段设备突然失联」的口碑炸点。每一处涉及方向的代码必须强制集中在唯一一个翻译函数里，配 truth-table 单测覆盖。
- **主因 B：iroh `RelayMode` 是 bind 时常量，不是运行时旋钮（`node.rs:368-396`）。** `Endpoint::builder().relay_mode(...).bind()` 完成后 relay 行为就被冻结了，settings 改不动它，必须重启进程。任何「顺手做个运行时热切换」的尝试都会出现「endpoint 关了重 bind 但 ALPN handler 没重新挂」「`Arc<Endpoint>` 被多个 adapter 共享改不动」「UI 显示已生效但实际还在走 relay」三类半生效灾难。整个里程碑的 UX 必须诚实承担「重启生效」语义。

最高风险不是技术实现而是**产品诚信**："LAN-only" 是营销最优解，但实际行为比字面意思弱（首次配对仍走 `rendezvous.uniclipboard.app`、OTLP 遥测仍开、pkarr DHT 仍发包、auto-update 仍查 GitHub）。文档、UI tooltip、changelog 三处任一含糊措辞或者出现 "fully offline / 完全离线 / 绝对私有" 这类绝对化用词，就会从「信任锚点」变成「营销谎言」，且这种口碑伤害**不可逆**。本里程碑必须把「不属于 LAN-only 范围的外网请求」清单作为 Phase 5 的 release blocker。

---

## Key Findings

### 推荐 stack 增量（来自 STACK.md，HIGH 置信度，无新依赖）

整个里程碑**禁止替换任何既有技术、禁止新增任何 crate / npm 依赖**。所有目标 API 已在 lockfile：

| 维度 | 改动 | 关键引用 |
|---|---|---|
| iroh 通道判定 | 仅消费 0.98 既有 API：`Endpoint::remote_info(id) -> Option<RemoteInfo>` + `RemoteInfo::addrs()` + `TransportAddrInfo::usage()/addr()` + `TransportAddr::{Ip, Relay}` + `TransportAddrUsage::{Active, Inactive}` | 项目代码已用：`uc-infra/src/network/iroh/connect.rs:51-67`、`blobs.rs:135-145` |
| Settings `network` namespace | 加 `NetworkSettings { allow_relay_fallback: bool }` 子结构，跟随既有 `serde + #[serde(default)]` 模式；无 SQL DDL 迁移（settings 是 JSON 文件） | `uc-core/src/settings/model.rs:201-202` 早已注释占位 |
| 前端 UI | 不增依赖：`@radix-ui/react-switch ^1.2.6`、`lucide-react`（含 `Wifi`/`WifiOff`/`Server`/`Cable`）、`sonner`、`Badge`、`SettingGroup`/`SettingRow`/`useSetting` 全部就位 | `package.json:62/77/92`、`src/components/setting/SyncSection.tsx` |
| 可观察性 | 不动 `uc-observability` crate，仅新增 span/字段命名（dotted name `network.channel_probe`，attrs `{ peer, channel = "lan|relay|offline" }`），不引入 metrics 层 | `uc-observability/src/profile.rs:53-54` |
| 测试套 | 不增工具：`tokio test-util` + `tempfile` + `mockall` + `wiremock` + 既有双 endpoint loopback fixture（`slice2_phase1_presence_e2e.rs:354-356`），用 `RelayMode::Disabled` 验证「LAN-only=true ⇒ `addr().addrs` 不含 Relay 项」 | `uc-infra/tests/iroh_presence_probe.rs:17-29` |

**铁律：** 范围内禁止替换 iroh / Diesel / serde / React / Radix UI 任何一项。本里程碑**唯一**真正的"新增能力"是 `ConnectionChannelPort` + `IrohConnectionChannelAdapter`（约 30 行 helper），**其余全是把既有钩子接通**。

**关键陷阱：** 不要在新代码里再去找 iroh 0.95 时代的 `Endpoint::conn_type` —— 它在 0.97/0.98 已被 `remote_info` 替代（`tests/iroh_presence_probe.rs:5-11` 注释有明确迁移记录）；也不要试图运行时切 `RelayMode`（无公开 API）；也不要把 `iroh::_events::conn_type::changed` 这个内部 tracing target 当公开接口用。

### 必备体验项（Table Stakes）

最小集，每条 LOW 复杂度，缺任何一项「局域网洁癖」用户都不会信这是 LAN-only：

1. **Settings → Network 分类下出现 "LAN-only Mode" 开关**（替换 `NetworkSection.tsx:11-23` 占位组件）
2. **后端 `Settings.network.allow_relay_fallback: bool`，默认 `true`**（反向命名，UI ON ⇔ 后端 false）
3. **启动时 `Settings → IrohNodeConfig.disable_relays` 注入**（在 `builders.rs:178` / `non_gui_runtime.rs:280` 装配，`uc-infra` 内部不知道 `NetworkSettings`）
4. **切换后弹"重启生效"持久 inline 通知 + 三态视觉**（applied OFF / applied ON / pending change），附"立即重启"按钮，禁止只用一秒 toast
5. **设备列表 LAN / Relay / Offline 连接通道徽章**（信任锚点核心，与开关产品强耦合，必须同期 ship）
6. **配对成功后一次性 onboarding tip**（inline banner，非 modal；`dismissed_tips` 持久化；首次配对 wizard "Done" 之后展示，文案带"开启会让跨网段设备失联"边界提示）
7. **LAN-only 边界文档透明披露**（`docs/lan-only.md` + Settings tooltip + 配对页小字三处一致；列出仍走外网的 4 类请求：rendezvous / OTLP / pkarr DHT / auto-update）

### 差异化加分项（Differentiators，建议挤进 v0.7.0 的 LOW 复杂度项）

- **D1 通道徽章 hover tooltip**：解释 "Relay = 加密中继，元数据可见"，把"为什么开 LAN-only"的论证就近放给用户
- **D2 "Out of LAN" 远端设备灰色 + tooltip**：开了 LAN-only 后远程设备会变灰，不解释 = 困惑（PairDrop 模式参考）
- **D7 Tray icon 显示 LAN-only 状态徽章**：用户不打开主窗口也能确认状态，匹配「洁癖」用户的安全感诉求

**MEDIUM 推迟到 v0.7.x：** D3（实时连接诊断面板）、D4（OTLP `connection_path` tag）—— 等用户反馈再做。
**HIGH 推迟到 v0.8+：** D5（QR 码 / 离线配对）、D6（"测试 LAN-only" 按钮）—— 需要前置工作或评估真实需求量。

### 拒绝项（Anti-Features）

每条都有具体技术或产品理由（详见 FEATURES.md §Anti-Features 与 PROJECT.md §Out of Scope）：

- **A1 完全无联网首次配对** —— 已接受首次需联网；自带 P2P 配对协议是独立子项目（QR + 蓝牙 + NFC 都是侧信道），不在 v0.7.0
- **A2 自动检测 LAN/Relay 而无开关** —— Syncthing 论坛最常见投诉就是 "auto detection 在 NAT/IP 缓存边界经常判断错"；显式开关 + 默认 OFF 才是正确选项
- **A3 IP 段白名单/黑名单** —— 家用 DHCP 易把自己锁出；NodeId 才是更精确的身份层，IP 段是 IPv4 思维倒退
- **A4 运行时热切换** —— iroh `RelayMode` 是 bind 时确定，热切换需重建 endpoint + 重新挂 ALPN handler + 处理活跃 transfer，工程量翻倍且风险大
- **A5 独立 LAN-only 二进制 flavor** —— 双 binary 维护成本高；先做开关，flavor 看后续真实需求
- **A6 "Strict mode"（关 pkarr DHT）** —— 关掉后跨网段连接率从 ~90% 跌到接近 0；pkarr 性质类似 DNS 不算 relay
- **A7 自定义 rendezvous URL 输入框** —— rendezvous 服务暂未开源，先有部署文档再考虑用户暴露

### 架构变更摘要（来自 ARCHITECTURE.md）

**核心判断：这不是新架构，而是在既有钩子上挂一根线。** `uc-core::Settings` 已有 `// pub network: NetworkSettings,` 注释占位，`IrohNodeConfig.disable_relays` 已是 `pub`，`bind` 时 `RelayMode` 路径已通；本里程碑要补的是「把这根线接通」+ 一个全新的 `ConnectionChannelPort` 读出能力。

**新增（大头）**：`NetworkSettings` 值对象（`uc-core`）+ View / Patch 镜像（`uc-application`）+ DTO（`uc-webserver` + `uc-daemon-contract`）+ TS 类型 + NetworkSection 真实 UI + `ConnectionChannelPort` + `IrohConnectionChannelAdapter` + `PeerSnapshotDto` 加 `channel` 字段 + 前端 `ConnectionChannelBadge`。

**修改（中等）**：`apply_settings_patch` 加一段；`Settings::default` 加一行；`build_space_setup_assembly` 调用方（`builders.rs:178` / `non_gui_runtime.rs:280`）从 `IrohNodeConfig::default()` 改为「先读 settings 再造」；NetworkSection 替换占位；`peers.changed` 路径不变（增量字段而非新事件类型）。

**保持不变（大量）**：六边形分层、daemon-first 主权、HTTP `/settings` 与 WS `peers.changed` 协议骨架、Tauri commands（**继续没有 settings 命令**，前端走 daemon HTTP；`network.allow_relay_fallback` 没有 OS-level side effect，daemon HTTP 已覆盖）、iroh `RelayMode` bind-time 确定的事实、`disable_relays` 字段本身、settings JSON 文件原子写、SQLite migration 链（settings 不走 SQL migration）。

**常见误区纠正：** Settings 不是 SQLite 存储，是 `~/Library/Application Support/.../settings.json` 的 JSON 文件 + serde + atomic write（`uc-infra/src/settings/repository.rs:77`），migration 走 `SettingsMigrator` 基于 `schema_version` 数值递增。**且本里程碑不需要 bump `CURRENT_SCHEMA_VERSION`** —— 新字段全部带 `#[serde(default)]`，旧 settings.json 反序列化时缺字段直接走默认值，向前兼容；bumping schema version 反而触发不必要的 migration codepath。

**WS 协议不改动** —— `peers.changed` 仍然是全量快照，新 `channel` 字段跟着既有 mapping 走；不新增事件类型，不新增 HTTP endpoint。设备列表通道值由既有 `PresenceEvent` + 15s polling 双路径自然刷新。

### Top 5 critical pitfalls（来自 PITFALLS.md，标注 phase 归属）

1. **反向命名搞反方向**（Phase A schema） —— 三层语义两次反转，编译器无法捕捉。强制集中转换点 `relay_policy_to_iroh_config()` 在 `uc-bootstrap` 唯一一处取反；前后端 IPC 永远以 `allow_relay_fallback` 流动，不允许 `lan_only` 镜像穿过 IPC 边界；truth-table 单测覆盖 `(true, false), (false, true)` 两组。
2. **默认值倒置导致老用户跨网段设备突然离线**（Phase A） —— Rust `Default` for `bool` 默认 `false` 极度危险。`NetworkSettings` **禁止** `#[derive(Default)]`，必须手写 `impl Default { allow_relay_fallback: true }` + 三行注释 + 老 settings JSON 反序列化测试断言 `== true`；schema_version 不动（`#[serde(default)]` 已覆盖向后兼容）。
3. **运行时热切换的诱惑 → 半生效代码**（Phase A 注入路径 + Phase B 重启 UX） —— `endpoint.close() + 重新 bind` 会丢失 ALPN handler；`Arc<Endpoint>` 被多个 adapter 共享改不动；UI 假装已生效但流量还在走 relay。`UpdateNetworkSettings` use case 必须返回 `restart_required: bool`；进程内 `IrohNodeBuilder::bind` 强制 `OnceCell` 只能跑一次；PR 模板加 checkbox "[ ] 我没有尝试在运行时重建 iroh endpoint"。
4. **通道指示器与真实状态偏差**（Phase C 通道指示器） —— `tokio::broadcast` lagging receiver 会丢消息（`presence.rs:88` 说明）；iroh magicsock 路径切换无公开订阅；缓存陈旧让 LAN→Relay 退化在 UI 上不可见。**通道判定单一真相源由 infra 层 `ConnectionChannelPort` 单点产出**，禁止 application 层用 `if peer.ip.starts_with("192.168")` 推断（Tailscale / Clash / Docker bridge 全错）；UI 同时订阅事件流 + 5–10s polling 兜底；`ConnectionChannel::Unknown` 必须存在并显示，禁止默认显示为 LAN/Relay 之一。
5. **"LAN-only" 营销语 vs 配对仍需联网的现实边界**（Phase D 文档 + Phase B UX） —— 真实仍走外网的 4 类请求必须在 UI tooltip + `docs/lan-only.md` + changelog 三处同步披露：(a) 首次配对 `rendezvous.uniclipboard.app`、(b) OTLP 遥测（独立由 `general.telemetry_enabled` 控制，**禁止与 LAN-only 联动**）、(c) pkarr DHT NodeId 解析、(d) auto-update GitHub 检查。**i18n 禁止用词**：`fully offline` / `完全离线` / `no internet` / `private mode` / `绝对私有` / `encrypted-and-local`。

---

## Implications for Roadmap

### 综合三个 researcher 的 phase 建议

| 来源 | 建议 phase 数 | 排序 |
|---|---|---|
| ARCHITECTURE.md | 4（Phase A 后端 → Phase B 前端开关 → Phase C 通道徽章 → Phase D onboarding+文档） | 后端先行，前端开关与通道徽章可并行 |
| FEATURES.md | 暗含 4–5 phase（schema → wiring → UI → 通道徽章 → 文档/onboarding） | 同上结论 |
| PITFALLS.md | 6 phase（schema / 注入 / 通道指示器 / 重启 UX / 文档+onboarding / QA 验收） | 多分一个独立 QA phase |

**收敛建议：4 个核心 phase + 1 个跨平台 QA gate**。三份调研一致认为 Phase A（后端字段）必须最早完成（前端没有 schema 就 PUT 不进字段，DTO 422 或被忽略，没法验证开关真生效）；Phase B / Phase C 技术解耦可并行（Phase B 是纯 UI + HTTP wiring；Phase C 是新 port + adapter + DTO 字段，与开关行为无关）；Phase D（文档 + onboarding tip）可与 B/C 并行起步但必须在两者完成后整合验收；QA 跨平台手动验证矩阵作为 release gate（不是独立 phase 而是验收清单）。

### 推荐 phase 结构

#### Phase A · 后端字段落地（必须先做，1 个 phase）

**Rationale：** `uc-core` 是依赖图源头，schema 不定后续都飘。前端没 schema 就 PUT 不进字段；反过来后端先做不会 break 任何东西（旧 PUT 走 `serde(default)` 默认值）。

**Delivers：**
- `uc-core::Settings::network: NetworkSettings` + `Default` 实现（手写 `allow_relay_fallback: true`，三行注释禁止改 default）
- `uc-application` view/patch/apply_settings_patch 扩展 + `facade/settings/mod.rs` `pub use` 白名单
- `uc-webserver` + `uc-daemon-contract` DTO + dto ↔ view 双向映射
- `uc-bootstrap` `builders.rs:178` / `non_gui_runtime.rs:280` 读 settings → 唯一一处取反 `relay_policy_to_iroh_config()` → 构造 `IrohNodeConfig { disable_relays: !allow_relay_fallback, .. }`
- 单测：`apply_settings_patch` 处理 `network.allow_relay_fallback`；老 settings JSON（缺 `network` 字段）反序列化断言 `== true`；truth-table `(true→false, false→true)` 覆盖
- 集成测试：`uc-infra/tests/lan_only_relay_mode.rs` —— bind 时 `disable_relays=true` ⇒ `Endpoint::addr().addrs` 不含 `TransportAddr::Relay`；反向同理

**避免 pitfall：** 1（反向命名）、2（默认值倒置）、3（注入路径明确「只在 bind 时读一次」）、6（OTLP 不联动 — 决策固化进 PROJECT.md）、8（测试覆盖 Tier A/B 自动断言）

**验收：** 手工把 settings.json 加 `"network": {"allow_relay_fallback": false}`，重启 daemon → 日志看到 `disable_relays = true`、bind 时 `RelayMode = Disabled`；HTTP PUT `/settings` 带 `network` 段写盘成功，GET 返回一致。

#### Phase B · 前端 NetworkSection + 重启提示 UX（可与 Phase C 并行）

**Rationale：** Phase A 跑通后这一步纯前端工作。重启提示 UX 是 Pitfall 10 的主战场，必须三态视觉 + 持久 inline 通知 + debounce + "立即重启" 按钮。

**Delivers：**
- `src/api/daemon/settings.ts` Settings interface 加 `network: { allowRelayFallback: boolean }`
- `setting-context` 流转 `network.allowRelayFallback`（`updateNetworkSetting` 与 `updateSyncSetting` 同形）
- `NetworkSection.tsx` 替换占位组件 → 渲染 LAN-only Mode 开关（`Switch checked={!setting.network.allowRelayFallback}`）
- 三态视觉：applied OFF / applied ON / pending change（黄色 / 感叹号 / inline "重启生效" 标签）
- 持久 inline 通知 + "立即重启"按钮（调 daemon 优雅 shutdown + relaunch）；禁止只用一秒 toast
- Settings 写入 debounce 500ms 防止反复切换爆 disk I/O
- 删除占位 i18n key `'settings.sections.network.placeholder'`

**避免 pitfall：** 5（UI tooltip 必须有 info icon 列出 4 类仍走外网请求）、10（重启 UX 三态 + 持久通知 + debounce）、11（占位组件残留）

#### Phase C · 连接通道指示器（可与 Phase B 并行）

**Rationale：** 这是本里程碑**唯一真正的"新增能力"**，与开关行为技术解耦但产品同期发布（信任锚点核心）。新增 `ConnectionChannelPort` 抽象不让 application 层耦合 iroh API。

**Delivers：**
- `uc-core::ports::connection_channel::ConnectionChannelPort` + `ConnectionChannel { Direct, Relay, Offline, Unknown }` enum
- `uc-infra::IrohConnectionChannelAdapter`：包装 `Arc<Endpoint>` + `Arc<dyn PeerAddressRepositoryPort>`，`channel_for(device)` 流程 = `endpoint.remote_info(addr.id) → 过滤 Active TransportAddrInfo → Ip⇒Direct / Relay⇒Relay / 空⇒Unknown`
- `uc-application::MemberRosterDeps` 加 `Arc<dyn ConnectionChannelPort>`；`PeerSnapshotView` 加 `channel` 字段
- `uc-bootstrap::space_setup` 装配 adapter（推荐 `IrohNodeBuilder::spawn()` 顺带返回 `ConnectionChannelPort` 句柄，与既有 `install_*` 模式一致；不要把 `Arc<Endpoint>` 直接漏出去）
- `PeerSnapshotDto` 加 `channel: String`（`"direct"|"relay"|"offline"|"unknown"`）
- 前端 `src/components/device/ConnectionChannelBadge.tsx` 三态徽章 + `SpaceMembersPanel` 挂载（事件 + 15s polling 双路径）；UI 必须显示 Unknown 态
- D1 hover tooltip 解释 Relay 含义（建议挤进）
- D2 "Out of LAN" 远端设备灰色 + tooltip（建议挤进）
- D7 Tray icon LAN-only 状态徽章（建议挤进）

**避免 pitfall：** 4（通道判定单一真相源 + Unknown 态可见 + 事件+polling 双兜底；禁止 IP 段推断）、7（IPv6 ULA filter `fc00::/7` + `fe80::/10` 顺手补上 `node.rs:308-312`）

#### Phase D · onboarding tip + 文档边界披露（最后整合，两个 sub-phase）

**Rationale：** 文档比 UI 文案更危险（被搜索引擎抓取、reddit 引用、reviewer 截图）。一旦写错纠错难度远高于改 UI。必须 UI tooltip / `docs/lan-only.md` / README / changelog 四处一致，由 reviewer mandatory checklist gate。

**Delivers：**
- 配对成功后一次性 onboarding tip（inline banner，非 modal；`dismissed_tips: HashSet<String>` settings 持久化；wizard "Done" 之后展示；文案带"开启会让跨网段设备失联"边界提示；"了解更多"跳转 Settings 而非自动开启）
- `docs/lan-only.md` 详解：(a) 首次配对仍走 rendezvous、(b) OTLP 遥测仍开（独立设置）、(c) pkarr DHT NodeId 解析仍发包、(d) auto-update 仍查 GitHub
- `docs/terminology.md` 维护推荐用语 / 禁止用语
- 跨平台手动 QA 矩阵作为 release gate：macOS / Windows / Linux × 同 Wi-Fi 同子网 / 同 Wi-Fi 不同 VLAN / VPN 在线 / 企业 AP isolation 四场景
- Tier C 抓包验证：开 LAN-only 后 Wireshark / tcpdump 确认无指向 `*.iroh.network` / `*.n0.computer` 流量

**避免 pitfall：** 5（边界文档）、9（文档措辞 + 四 surface 一致性 + reviewer 必勾 checklist）、12（onboarding tip 时机 / 持久化 / 文案带边界）、7（跨平台 QA 矩阵）、8（Tier C 抓包）

### Phase 排序 rationale

- **Phase A 必先做** —— 编译器强制；后端单跑得通，前端先跑不通
- **Phase B 与 Phase C 技术解耦可并行** —— 两者依赖 Phase A，相互不依赖；Phase C 即使没 Phase B 也能独立 ship 让用户「看到」当前是 LAN 还是 Relay
- **Phase D 是整合 + 验收** —— 必须在 Phase B/C 完成后做 onboarding tip 接入与跨平台 QA；文档可以更早起草但 release gate 在最后

---

## Open Questions for Planner

每位 researcher 都留了开放问题，去重整理如下：

1. **`IrohNode.endpoint()` 暴露方式**（来自 ARCHITECTURE.md §1.5 + STACK.md §3.4） —— 给 `IrohConnectionChannelAdapter` 拿 endpoint 的方式：(a) 加 `pub fn endpoint(&self) -> Arc<Endpoint>` 访问器（小改、隐式合约），(b) `IrohNodeBuilder::spawn()` 顺带返回 `ConnectionChannelPort` 句柄（与现有 `install_*` 模式一致、合规但 spawn 签名变更）。**倾向 (b)**，由 planner 在 Phase C 启动前 1 小时确认。
2. **`SettingsMigrator::migrations` vec 当前是否真为空**（来自 STACK.md §2.2） —— 三份调研一致认为本里程碑不需要 V1→V2 migration（`#[serde(default)]` 兜底），但需要 grep 一次 `uc-infra/src/settings/migration.rs:36-43` 确认 `migrations` vec 现状真无内容（避免误判）。
3. **是否将 D1 / D2 / D7 三个 LOW 复杂度差异化项正式纳入 v0.7.0 Active**（来自 FEATURES.md MVP 章节） —— 三项均属 LOW 但累加后的 phase 工作量需要 roadmapper 拍板。**倾向纳入**：信任锚点贡献大、代码成本小、与既有组件复用度高。
4. **跨平台 QA「企业 AP isolation」环境如何模拟**（来自 PITFALLS.md §Pitfall 7） —— 是否需要采购一台测试 AP，或用 `iptables` 在虚拟机内模拟，或仅在文档列为 known limitation。**倾向后两者组合**：手动模拟 + 已知边界文档化。
5. **onboarding tip 文案的边界提示具体用语**（来自 PITFALLS.md §Pitfall 12 + FEATURES.md table stake #6） —— 在「鼓励发现」与「警告代价」之间找平衡的精确措辞，需要在 Phase D 启动时由产品 + 文档作者敲定。
6. **遥测 `general.telemetry_enabled` 邻近放置 vs 独立 General 分类**（来自 PITFALLS.md §Pitfall 6） —— 决策固化「LAN-only 不联动遥测」之后，UI 上是否把遥测开关放到 Network section 邻近以方便用户二次决策？倾向「不动现有 General 位置但在 Network tooltip 里链过去」。
7. **`docs/lan-only.md` 中关于 rendezvous 调用频次与时机的精确描述**（来自 Open Question 4） —— 写文档前需要 1 小时代码确认，避免「首次配对一次」与「每次启动 ping」表达不准。
8. **Tray icon 的 LAN-only badge 形态**（来自 FEATURES.md D7） —— 小锁 / 墙图标 / 字母标 LAN，跨平台 tray icon 格式约束需要在 Phase B 启动时由 UI 设计师快速给出参考稿。

---

## Confidence Assessment

| 维度 | 置信度 | 备注 |
|---|---|---|
| Stack | HIGH | iroh 0.98 API 路径项目代码已用例；Cargo.lock + package.json 全部锁定；无新依赖 |
| Features | HIGH | 6 个对标产品（Syncthing / Resilio / KDE Connect / LocalSend / Tailscale / AirDrop / PairDrop）一手资料覆盖；anti-features 每条有具体技术或产品理由 |
| Architecture | HIGH | 关键文件 + 行号全部 grep 验证；六边形分层规则核对；常见误区（SQLite vs JSON 文件 settings、Tauri command vs HTTP）已纠正 |
| Pitfalls | HIGH | 12 条全部锚定到具体文件行号或产品决策记录；recovery cost + warning signs + phase 归属完整 |

**整体置信度：HIGH**

### Gaps to Address（计划阶段需要关注）

- **`useSetting` 与 `setting-context` 完整 API**（Phase A 启动前 1 小时） —— STACK 调研未读全 `src/contexts/setting-context.ts`，需要确认 `updateNetworkSetting(patch)` 方法形态
- **`IrohNode` endpoint 访问器决策**（Phase C 启动前 1 小时） —— Open Question 1 详述
- **rendezvous 调用时机的精确披露**（Phase D 启动前） —— Open Question 7 详述
- **跨平台 QA 矩阵执行环境**（Phase D 准备期） —— Open Question 4 详述
- **遥测开关 UI 位置最终决策**（Phase B/D 之间） —— Open Question 6 详述

---

## Sources

| 文件 | 内容 | 置信度 |
|---|---|---|
| `.planning/research/STACK.md` | 5 维度增量栈调研（iroh API / Settings namespace / 前端零件 / 可观察性 / 测试套），全部 HIGH | HIGH |
| `.planning/research/FEATURES.md` | 6 对标产品 + table stakes / differentiators / anti-features / dependency graph / MVP / 信任锚点设计 | HIGH |
| `.planning/research/ARCHITECTURE.md` | 五层落点 + 集成点行号锁定 + 数据流前后对比 + 影响面分析 + 6 phase 构建顺序 | HIGH |
| `.planning/research/PITFALLS.md` | 12 个 critical pitfalls + 主因 A/B 收敛 + Tier A/B/C 测试分层 + recovery strategies + pitfall-to-phase mapping | HIGH |

---

*v0.7.0 LAN-only Mode 综合调研 — 2026-05-04*
*覆盖了原 v0.5.0 Local Encrypted Search 调研产物，里程碑切换至 LAN-only Mode*
*Ready for roadmap: yes*
