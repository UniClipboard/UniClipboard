# Pitfalls Research

**Domain:** 给已上线的"局域网 + 公网中继混合系统"加用户可见的 LAN-only 开关（v0.7.0）
**Researched:** 2026-05-04
**Confidence:** HIGH —— 基于现有 `IrohNodeConfig.disable_relays` 实测路径、`uc-core` settings schema、`PresenceEvent` 事件链、explore 阶段已固化的产品决策

---

## 里程碑特有 pitfalls 摘要

本里程碑的所有失败模式可以归纳为 **两条主因**，建议在 review 时把这两条贴在脑门上：

### 主因 A：反向命名导致的「语义颠倒」

UI 显示 **"LAN-only Mode"**（开 = 限制为局域网），后端字段 `network.allow_relay_fallback`（true = 允许中继），iroh 内部字段 `IrohNodeConfig.disable_relays`（true = 禁用中继）。**三层语义两次反转**，任何一处脑子没转过来就会让默认值翻车、迁移翻车、测试断言翻车。

落地建议：

- 每一处涉及"开关方向"的代码都要写一行注释说明 toggle 与字段的语义关系
- frontend 与 backend 之间永远以 `allow_relay_fallback` 流动，不允许 `lan_only` 这种字段穿过 IPC 边界
- 单元测试至少有一组「向上转换」断言：`UI(toggle on) → repo(allow=false) → infra(disable=true)`

### 主因 B：iroh `RelayMode` 是 bind 时常量，不是运行时旋钮

`uc-infra/src/network/iroh/node.rs:368-372` 把 `disable_relays` 转换成 `RelayMode::Disabled | RelayMode::Default`，这个值喂给 `Endpoint::builder().relay_mode(...).bind()`。**bind 完成后 endpoint 的 relay 行为就被冻结了**，settings 改不动它，必须重启进程才能生效。

落地建议：

- 后端 `network` 模块 **禁止** 对外暴露任何"立即生效"的 API；任何调用方传"是否需要重启"应统一返回 `RestartRequired`
- 设置写入路径在 commit 后必须立刻发出 `NetworkRestartRequired` 事件，UI 据此显示"待重启生效"视觉锁定（见 Pitfall 10）
- 任何尝试调 `endpoint.set_relay_mode(...)` / 重建 endpoint 的 PR 应该被拒绝（除非以独立 phase 立项做"运行时热切换"，而本里程碑明确不做）

---

## Critical Pitfalls

### Pitfall 1: 反向命名在 toggle 转换处搞反方向

**What goes wrong:**
前端 UI 写"LAN-only Mode"开关（用户视角：on = 我要限制在局域网），后端 schema 是 `allow_relay_fallback: bool`（默认 true），最底层 `IrohNodeConfig.disable_relays: bool`（默认 false）。任意一层把方向写错，都会出现"用户开了 LAN-only，但流量还在走中继"或者"用户关了 LAN-only，但跨网段设备突然连不上"的灾难。`uc-application` facade 层、`uc-daemon-contract` DTO 层、`uc-bootstrap` 注入 `IrohNodeConfig` 的位置——三处都在转换语义，任一处搞反都不会编译报错。

**Why it happens:**
布尔字段反向转换在静态类型系统里是不可见的，编译器不会区分 `disable_relays = !allow_relay_fallback` 和 `disable_relays = allow_relay_fallback`。开发者读 settings 字段名、看 UI 文案、再瞄底层 iroh 字段，三个语境快速切换，认知负担高于普通字段映射。代码 review 时 reviewer 也只看到 `not !` 这种符号，难以判断该不该有这个 not。

**How to avoid:**

- **强制集中转换点**：`uc-bootstrap` 中只在 **唯一一处** 做 `allow_relay_fallback → disable_relays` 的取反，这个函数必须命名为 `relay_policy_to_iroh_config()`（带语义）而不是 `to_config()`。任何其他地方碰这两个字段都视为越界。
- **双向断言测试**：`uc-bootstrap` 必须有形如 `assert_eq!(false, to_config(true).disable_relays)` 的 truth-table 测试，覆盖 `(true, false), (false, true)` 两组组合。
- **DTO 层不允许重命名**：`uc-daemon-contract` 中 settings DTO 的字段名必须 **完全等于** core 字段 `allow_relay_fallback`，前端用 `allowRelayFallback`（自动驼峰），UI 文案的"LAN-only" 只在 i18n key 中出现。
- **IPC 不暴露反向变体**：禁止前端 store 内部维护一个 `lan_only` 镜像状态再来回反转。状态应统一是 `allowRelayFallback`，UI 组件读这个值决定 toggle 视觉态。

**Warning signs:**

- code review 中出现 `!settings.network.allow_relay_fallback`（除了在那唯一一处转换函数）
- 单测断言中出现 `disable_relays = true` 但用户故事是"允许中继"
- 集成测试启动日志 `disable_relays = X` 与用户配置不一致
- 前端 TypeScript 中出现 `lanOnly: !allowRelayFallback` 这种局部反转

**Phase to address:**
**Phase 1（后端 schema + 取反函数）** —— 必须把转换点和断言测试一起落地，后续 phase 才能安全引用。

---

### Pitfall 2: 默认值倒置，老用户升级后跨网段设备突然离线

**What goes wrong:**
迁移时 `network` 字段是新增字段，`#[serde(default)]` 会用 `NetworkSettings::default()` 填充。如果这个 default 写成 `allow_relay_fallback: false`（"既然字段叫 allow，新功能就严格点"），老用户升级到 v0.7.0 当天起就会发现：跨网段同步的设备全部离线、远程办公场景的剪贴板同步整个停摆。这是不可逆的口碑事件——用户不会去看 changelog，只会发问"为什么之前能用现在不行"。

**Why it happens:**

- Rust 的 `Default` 对 `bool` 默认是 `false`，导致 `#[derive(Default)]` 的字段悄悄变成 `false`
- `uc-core/src/settings/defaults.rs` 现存模式是显式 `impl Default`，但开发者新增字段时常常忘记看现有 sub-section 的默认值约定
- 测试用例里 default 用什么值都跑得通，迁移行为只在真实老用户数据上才会暴露
- 反向命名加剧了这个 trap：开发者直觉"LAN-only 默认应该 off"，但翻译过来后端字段应该是 `allow_relay_fallback = true`，方向容易倒

**How to avoid:**

- `NetworkSettings` **不允许** `#[derive(Default)]`，必须 **手写** `impl Default for NetworkSettings { fn default() -> Self { Self { allow_relay_fallback: true } } }`，并且字面量 `true` 旁边附三行注释说明：`// 默认 true = 允许 fallback。改成 false 会让所有跨网段老用户突然离线，属于 breaking change，禁止改动。`
- **migration 测试**：`uc-core/tests/settings_migration_v0_6_to_v0_7.rs`（或对应位置）必须有一个老 settings JSON（缺 `network` 字段）反序列化后断言 `settings.network.allow_relay_fallback == true`。
- **schema_version 不动**：本里程碑加字段但 **不** 升 `CURRENT_SCHEMA_VERSION` —— `serde(default)` 已经覆盖向后兼容，bumping schema version 反而会触发不必要的 migration codepath。如果有人提议升 version，让他先解释为什么 default 不够。
- **changelog 双语条目**：里程碑发布说明里中英文都要明确"默认值不变，老用户行为完全一致"，给 support 同学留一个挡箭牌。

**Warning signs:**

- 迁移单测里出现 `assert!(!settings.network.allow_relay_fallback)`
- `NetworkSettings` 上面是 `#[derive(Default)]` 而没有 `impl Default`
- PR 的 schema migration 章节缺失或一句话带过
- 老 settings JSON 测试 fixture 不存在

**Phase to address:**
**Phase 1（后端 schema）** —— 默认值与 migration 测试必须同 PR 落地，否则后续 phase 引用 `network.allow_relay_fallback` 就已经基于错误地基。

---

### Pitfall 3: 运行时热切换的诱惑——半生效代码

**What goes wrong:**
开发者读到"切换 LAN-only Mode 后弹重启提示"会觉得这是低端的妥协方案，就尝试"小成本支持立即生效"。最常见的几种半成品：

1. settings 写入后 **只** 调 `endpoint.close()` + 重新 bind，但忘了 `Router` 上 `install_pairing` / `install_clipboard` 等所有 ALPN handler 都已与旧 endpoint 绑定，新 endpoint 上没有任何接收方
2. 把 `IrohNodeConfig` clone 一份改字段，再去 set 进 `IrohNode`，但 `IrohNode` 内部的 `Arc<Endpoint>` 已经被 dispatch / receiver / blob adapter 拷了多份，改不动了
3. 写入 settings、刷新设备列表 UI 上的 "channel" 指示器，但 **没** 重建 endpoint —— 看上去 LAN-only "立刻生效"了，实际流量仍走 relay，UI 完全说谎

**Why it happens:**

- iroh `RelayMode` 是 endpoint bind 时确定（`uc-infra/src/network/iroh/node.rs:373-396`），但这个事实没有在编译期被表达出来——开发者读 `IrohNodeConfig.disable_relays` 字段会自然以为它是个"运行时配置"
- 设置类的 UX 心智模型默认是"切换即生效"，"重启生效"是用户和开发者都讨厌的妥协
- explore 阶段已经明确"运行时热切换不在本里程碑范围"（PROJECT.md:111），但 phase 实施过程中很容易"反正都改到这了顺手做了"
- presence / clipboard / blobs 三个 adapter 都共享同一个 `Arc<Endpoint>`，重建 endpoint 不是简单的 swap，而是要重置整条 `IrohNodeBuilder` 链路

**How to avoid:**

- **写入路径返回 `RestartRequired` 信号**：`UpdateNetworkSettings` use case 的返回值必须包含一个 `restart_required: bool` 字段（即便目前永远是 true），让调用方显式承担"还没真正生效"的事实
- **禁止 endpoint 重建代码**：本里程碑严禁出现 `endpoint.close().await` + `IrohNodeBuilder::bind` 在同一个进程里跑第二次。`IrohNode` 的 lifecycle 仍由 `uc-bootstrap` 单点拥有，进程内重启路径 **不存在**
- **运行时 invariant 测试**：在 `uc-bootstrap` 增加 `assert!()`：进程启动后只能 `bind` 一次（用 `OnceCell` 强制）
- **UI 视觉锁定**：toggle 切换后立即把开关变成"已切换、待重启生效"的灰态（见 Pitfall 10），物理上消除"用户怀疑没生效再切回去"的循环
- **PR 模板里加复选框**：`[ ] 我没有尝试在运行时重建 iroh endpoint`

**Warning signs:**

- diff 中出现 `Endpoint::builder()` 在 bootstrap 之外的位置
- UI 切换后没有重启提示但开关变化完成
- `IrohNode::shutdown` 后又有重新构造
- 集成测试覆盖"切换 LAN-only 后立刻断开 relay 连接"（这种行为本里程碑不应实现，写了说明走偏了）

**Phase to address:**
**Phase 2（settings 注入到 IrohNodeConfig）+ Phase 4（重启提示 UX）** —— 两个 phase 必须呼应：注入路径明确"只在 bind 时读一次"，UX 路径承担"未生效"心智。

---

### Pitfall 4: 通道指示器（LAN/Relay/Offline）与真实 endpoint 状态偏差

**What goes wrong:**
设备列表上加"连接通道指示器"是这次里程碑的核心可观察性卖点，但极易出现下列偏差：

1. **缓存陈旧**：第一次连接走 LAN 直连后通道显示"LAN"，后续 NAT 路径切换、iroh 内部 magicsock 重新选路换到 relay，UI 不知道，仍显示 LAN
2. **事件丢失**：`PresenceEvent` 是 `tokio::sync::broadcast`（`presence.rs:411`），lagging receiver 会丢消息（"Lagging receivers drop messages per `broadcast` contract"，见 `uc-core/src/ports/presence.rs:88`），UI 重连或者 dashboard 切回前台时刚好错过通道切换事件
3. **状态机分叉**：通道判定如果同时基于 `PresenceEvent.state`（online/offline/unknown）和某个新 `ChannelKind` 字段，两路独立更新就会出现"online 但 channel = unknown" / "offline 但 channel = lan" 的不一致状态
4. **iroh 路径选择透明性**：iroh 自己在 LAN 直连和 relay 之间动态切换 magicsock path（见 `node.rs:283 max_concurrent_multipath_paths`），UI 想准确反映需要订阅 iroh 内部 path 事件——但 `uc-core` 不应耦合 iroh API，这里需要新 port 抽象
5. **"LAN-only" 开启后 relay 通道仍存在的歧义**：用户开了 LAN-only Mode，但 `RelayMode::Disabled` 只是 endpoint **不再使用** relay；如果在切换前已经有一条经 relay 建立的 QUIC 连接，连接对象本身还在，通道指示器是显示"还在用 relay"还是"已经断"？

**Why it happens:**

- `PresenceEvent` 当前只有 online/offline/unknown 三态（presence.rs:24-28），明确说没有 `Connecting` / `Degraded`，新增 channel 维度需要扩展事件 schema 或新增 port
- iroh 的 magicsock 路径选择是底层动作，从 `Endpoint` API 上 **没有现成的"当前用的哪条 path"查询**（`node.rs:172-199` 的 `log_publish_addrs` 是输出端候选地址，不是入站对端实际使用路径）
- `tokio::broadcast` 的 lag-drop 行为在 happy path 测试里看不到，只有在 UI 长时间后台切回时才暴露
- 开发者在 UI 上加一个"channel" 字段最简单的实现是"只在第一次连接时记一次"，这就是缓存陈旧 trap 的成因

**How to avoid:**

- **通道判定单一真相源**：通道值由 **infra 层的 `ConnectionChannelPort`**（新增 port）单点产出，不允许 application 层根据 `peer_addr` 自己推断"是不是 LAN IP"（IP 段判断会被 Tailscale / Clash TUN 误导，参考 `node.rs:300-313` 已有的 `is_virtual_nic_ip` filter）。
- **事件兜底为 polling**：UI 同时订阅事件流和 **间隔轮询**（例如 5s 一次 `current_channel(device_id)`），事件用于即时更新，轮询用于消除 lagging drop。这个兼容策略已经在 explore 阶段被认可（参考 deferred 列表里"事件驱动"是 NEXT，意味着当前还在 polling baseline）。
- **"未知" 是合法状态**：`ChannelKind::Unknown` 必须存在并且 UI 必须展示（例如灰色"-"或 spinner）。任何代码不允许把 `Unknown` 默认显示为 "LAN" 或 "Relay" 的某一种。
- **整数事件序号**：`ChannelEvent { device_id, kind, seq }` 加单调递增 seq，UI 收到旧 seq 直接丢弃，避免事件乱序导致通道跳变。
- **明确语义边界文档**：在 i18n string 里清楚定义：
  - "LAN" = 当前活跃 QUIC path 是局域网直连
  - "Relay" = 当前活跃 QUIC path 经过公网中继
  - "Offline" = 没有活跃 connection
  - "Unknown" = 还在握手或路径切换中
- **集成测试场景矩阵**：起码覆盖（a）loopback bind 直连、（b）loopback bind + 强制 relay 模拟、（c）连接成功后中断网络、（d）切换 LAN-only 后已建立的 relay 连接如何在 UI 上呈现

**Warning signs:**

- 任何位置出现 `if peer.ip.starts_with("192.168")` 类的"局域网 IP 推断"
- UI 中 channel 字段从不显示 "Unknown"
- `PresenceEvent` 被强行复用承载 channel 信息（应当是独立事件流）
- 测试用例跑完后通道值不再变化（说明缓存过期判定没接事件）
- iroh 内部 path-validation churn 日志（参考 `node.rs:227-232` 的"Congestion controller state reset 3× per connection"）出现时 UI 仍稳定显示同一通道

**Phase to address:**
**Phase 3（通道指示器）** —— 这个 phase 必须独立设计 `ConnectionChannelPort` 抽象、事件 + polling 双兜底、UI Unknown 态处理。如果 phase 范围里没有 polling fallback，roadmap 应当扩 phase。

---

### Pitfall 5: "LAN-only" 营销语 vs. 配对仍需联网的现实边界

**What goes wrong:**
用户在 UI 上看到"LAN-only Mode"，开启后产生的合理预期是"我所有的网络流量都不出局域网"。但实际上：

1. **首次配对必须经 `rendezvous.uniclipboard.app`**（公网 HTTP），这是 explore 阶段已经确认接受的限制（PROJECT.md:30）
2. **OTLP 遥测** 默认开启 + 默认指向外网 endpoint（`uc-core/src/settings/model.rs:22-26 default_telemetry_enabled = true`）
3. **iroh `presets::N0`** 内部仍会去 pkarr DHT 做 NodeId 解析，即便 relay 关闭，pkarr lookup 包仍走 UDP 到 n0 的服务器
4. **更新检查**（`auto_check_update`）默认开启，定期请求 GitHub releases API

如果 UI 把"LAN-only"暗示成"完全离线"或"流量不出局域网"，那就是虚假宣传——一旦被技术博客抓包做对比，口碑炸。这种炸法是 **不可挽回** 的：用户对开源加密产品的信任锚点就是"代码不撒谎"，一次违背就永久失去这部分用户。

**Why it happens:**

- "LAN-only Mode" 是营销最优解（直接回应 B 站用户原话），但它的字面含义比真实行为强
- 开发者实现"流量不走 relay" 的功能时，往往不会主动审计 **其他** 外网请求（pkarr / rendezvous / OTLP / update check），因为这些不在 iroh 配置范围内
- explore 决策接受"首次配对仍需联网"，但这个决策的传达只在 `.context/attachments/Summary` 里，前端 UI 文案设计阶段未必看到

**How to avoid:**

- **UI 必须有边界说明**：toggle 旁边必须有 info icon，点开展示"这并不是完全离线模式"四件事的清单：
  1. 首次配对仍需联网（去 `rendezvous.uniclipboard.app` 短期换 NodeId）
  2. 已配对设备的同步流量不走中继
  3. 局域网发现使用本地 mDNS（不出网）
  4. （如果遥测仍开）应用诊断遥测仍可能上报，要单独关
- **文档独立 section**：用户文档加 "What 'LAN-only Mode' means and doesn't mean"，逐条列出 **仍然会走外网的请求** 和 **它们的目的**。这是产品诚信的绝对底线。
- **审计清单作为 phase deliverable**：Phase 5（文档）必须以"已确认本里程碑边界文档列出了所有外网请求"作为 done criteria。审计内容包括 `pkarr`、`rendezvous`、OTLP、auto-update、telemetry。
- **i18n key 命名约束**：英文文案统一用 "LAN-only Mode (Limit Sync to Local Network)"，避免裸用 "LAN-only"——副标题承担降噪责任。中文："限制同步流量在局域网内"。**禁止** 使用"完全离线"、"绝对私有"、"不联网"这种绝对化措辞。
- **changelog 同样诚实**：发布说明的功能点必须包含一句"本功能不影响首次配对、应用更新检查、遥测的网络行为"。

**Warning signs:**

- 任何 i18n string 含 "fully offline" / "完全离线" / "no internet" / "绝对私有"
- toggle 旁边没有 info icon 或长按 tooltip 解释
- 用户文档里没有"什么外网请求仍然存在"的清单
- changelog 用语夸张
- 配对流程在 LAN-only 开启状态下仍能跑（这是预期行为）但用户没看到任何"现在去访问 rendezvous"的提示

**Phase to address:**
**Phase 4（重启提示 UX）+ Phase 5（onboarding tip / 文档）** —— UX phase 负责 toggle 旁的 info；文档 phase 负责长版边界说明。两者必须同步发版，缺一即口碑炸。

**2026-05-10 update — LAN-only 收紧到稳态全程仅 mDNS：**
原文第 3 条"`presets::N0` 内部仍会去 pkarr DHT" 已不再准确。`uc-infra/src/network/iroh/node.rs::IrohNodeBuilder::bind` 在 `disable_relays = true` 路径下追加：
1. `clear_address_lookup()` 清掉 N0 默认注入的 `PkarrPublisher` + `DnsAddressLookup`（不再向 `dns.iroh.link` 发布/查询）；
2. `runtime_consts::install_lan_only(true)` 把 LAN-only 固化为进程常量，`connect.rs::strip_relay_if_lan_only` 在每次 dial 前从对端 `EndpointAddr` 中剥掉 `TransportAddr::Relay` —— 否则即便本端 `RelayMode::Disabled`，iroh 仍会用对端发布的 relay url 走中转（已在 dev 日志中观测到）。
取舍：跨网段已配对设备不再可达（mDNS 同子网兜底失败 → 直接失败），这是 LAN-only 的设计意图。**首次配对仍走 rendezvous 公网 HTTP（本次未动）**，对应原文第 1 条仍然成立。

---

### Pitfall 6: OTLP 遥测在 LAN-only 下默认行为模糊

**What goes wrong:**
现有 settings：`general.telemetry_enabled` 默认 true（`model.rs:25, defaults.rs`），OTLP exporter 通过环境变量 / 编译期 baked endpoint 决定上报地址（`uc-observability/src/otlp/config.rs`）。LAN-only Mode 用户的核心心智就是"网络洁癖"——他们开了 LAN-only 然后 **仍然** 看到应用对外发 OTLP 请求，会立刻情绪炸裂；但如果实现里"自动跟着 LAN-only 关掉遥测"，又会让产品质量监控数据出现选择性偏差（恰好是那些喜欢 LAN-only 的用户从此不上报问题）。

更糟的是：本里程碑 **没有明确说明** 遥测是否归属本次范围。如果 phase 实施时不主动决定，就会出现"开发者偷偷把 telemetry_enabled 跟 allow_relay_fallback 联动"的隐式行为，没人 review，文档也没说，等用户发现时已经是事故。

**Why it happens:**

- explore 阶段对话提了"是否在 LAN-only 下默认关闭遥测"，但 PROJECT.md 没有把这个决策固化
- OTLP 配置散在三处：`general.telemetry_enabled`、环境变量、baked endpoint —— 任何一处偷偷加 `if allow_relay_fallback == false { disable }` 都会改变行为
- 反向命名再一次咬人：`telemetry_enabled` 是正向、`allow_relay_fallback` 是正向但语义反，开发者写联动条件时容易写错

**How to avoid:**

- **明确决策并写入 PROJECT.md**：本里程碑 **不** 联动遥测开关。LAN-only Mode 只控制 relay fallback，遥测仍然由独立的 `telemetry_enabled` 控制。在 UI 上，**遥测设置** 应该和 LAN-only 开关 **邻近放置**（同 Network section 或临近 section），让用户主动二次决策。
- **info tooltip 说明**：LAN-only toggle 的解释 tooltip 必须包含一行："This setting does not affect anonymous diagnostic telemetry. To disable telemetry, see the Telemetry option in [General / Privacy] settings."
- **代码层面 invariant**：`uc-bootstrap` 装配 OTLP 时禁止读 `network.allow_relay_fallback`。整段代码搜索一次，确认两个字段语义独立。
- **测试用例**：写一个 `bootstrap_lan_only_does_not_affect_telemetry` 测试，断言 settings (`allow_relay_fallback = false, telemetry_enabled = true`) 启动后 OTLP exporter 仍 active。
- **验收清单条目**：里程碑 acceptance criteria 必须有一条"遥测行为在切换 LAN-only 前后未改变"。

**Warning signs:**

- diff 里出现 `if !settings.network.allow_relay_fallback && settings.general.telemetry_enabled` 这种联动
- OTLP `tracing.rs` 中读 `network` namespace 字段
- 集成测试中 LAN-only 开启后 OTLP exporter 被强制 disable
- changelog 里写"LAN-only 模式自动关闭遥测"

**Phase to address:**
**Phase 1（schema）** —— 决策固化进 PROJECT.md key decisions；**Phase 5（文档/onboarding tip）** —— info tooltip 文案明确边界。

---

### Pitfall 7: 跨平台 `RelayMode::Disabled` 行为差异（特别是 IPv6 mDNS）

**What goes wrong:**
`disable_relays = true` 在 macOS / Windows / Linux 上的网络栈表现并不完全一致：

1. **macOS Wi-Fi + AWDL**：iroh 已经踩过 AWDL 抢占 Wi-Fi 导致 RTT 抖动的坑（`node.rs:202-211`）。`RelayMode::Disabled` 后没有 relay 兜底，AWDL 抢占期间整个连接就直接断
2. **Windows mDNS**：Windows 的 mDNS 实现历史上一直问题多（特别在 IPv6 link-local、多网卡环境）。本项目用的是 iroh 自带的 mDNS（`node.rs:393 .address_lookup(MdnsAddressLookup::builder())`），但是否在 Windows 上稳定上报需要实测
3. **Linux 防火墙**：很多发行版默认 firewalld / nftables 会过滤入站 5353（mDNS）端口，在公司环境下尤为常见。无 relay 兜底就完全断
4. **Tailscale / 公司 VPN 环境**：`AddrFilter` 已经过滤了 100.64/10、198.18/15、169.254/16（`node.rs:300-313`），但只过滤 IPv4。开了 LAN-only + 只有 IPv6 链路的小众场景没兜底
5. **企业网 client isolation**：公司 / 校园 Wi-Fi 经常开启 AP isolation（同 SSID 设备不能互通），用户开了 LAN-only 直接两台设备完全连不上，且无任何错误提示——只看到对方"离线"

**Why it happens:**

- 本地开发环境（同 Wi-Fi、家用路由器）和实际用户环境（企业 / 学校 / 多网卡 / VPN）差异巨大
- iroh 自身在 LAN 直连场景的覆盖测试不如 relay-fallback 场景完整（公网 relay 行为更受关注）
- `IrohNodeConfig::default()` 的 `disable_relays = false`（`node.rs:152, 161`）意味着所有现存集成测试都隐式带着 relay 兜底，纯 LAN 路径很少被测
- 开发者把 LAN-only 当作"小变量"，但实际它会暴露所有"原本被 relay 掩盖"的网络环境问题

**How to avoid:**

- **三平台手动验证清单**：里程碑 acceptance criteria 必须包含手动 QA 矩阵。最少四个场景 × 三平台：
  - 同 Wi-Fi 同子网（baseline）
  - 同 Wi-Fi 不同 VLAN / IP 段
  - VPN 在线（Tailscale / Cisco / WireGuard）
  - 企业 Wi-Fi（AP isolation 模拟）
- **AddrFilter IPv6 ULA 扩展**：`node.rs:308-312` 注释里说 "v1 only filters IPv4. IPv6 ULA / link-local can be added once we have telemetry showing iroh actually publishing them"——本里程碑提供了让 IPv6 表现暴露出来的契机，应该顺手补上 IPv6 ULA / link-local 过滤（`fc00::/7`、`fe80::/10`）
- **错误反馈**：通道指示器（Pitfall 4）显示 "Offline" 时，UI 必须有 "Why? Diagnose connection..." 入口，给用户至少一句"对方设备最近一次 candidate set 包括 [list of IPs]，本机 LAN 看不到"这种线索（不是 stack trace，是用户能理解的诊断）
- **explicit "won't work" scenarios 文档**：用户文档列出"LAN-only 已知不工作的场景"——AP isolation、Cisco AnyConnect with split-tunnel、Hyper-V 默认 NAT switch
- **OTLP 监控**：保留 LAN-only 切换事件 + 切换后 N 分钟内 device 是否成功收到一次 clipboard sync 的 funnel 指标，作为里程碑发布后的核心健康度
- **rollback 路径**：在 onboarding tip 里告诉用户"如果开启后跨网段设备连不上，可以从 Settings 关掉重启即可恢复"——降低误开成本

**Warning signs:**

- 集成测试只在 macOS 一个平台跑
- 没有手动 QA 矩阵
- 通道指示器显示 Offline 但没有诊断入口
- IPv6 link-local 仍能从 mDNS 进入候选地址列表

**Phase to address:**
**Phase 6（QA / 验收）** —— 必须有跨平台手动验证 phase；**Phase 3（通道指示器）** —— 顺手把诊断入口和 IPv6 filter 处理掉。

---

### Pitfall 8: 测试覆盖陷阱——"relay 是否被实际使用"在单元测试里测不到

**What goes wrong:**
本里程碑想验证"开 LAN-only 后流量真的不走 relay"，但单元测试和集成测试都覆盖不到这个断言：

1. **现有 e2e 测试模式**：`uc-bootstrap/tests/slice*_e2e.rs` 全部用 `disable_relays: true`（loopback only），意味着所有现存集成测试都已经在跑 LAN-only 等价配置。这些测试 **永远不会** 触发 relay 路径
2. **iroh API 没有 "is this connection over relay"  query**：要从 endpoint API 直接拿"当前活跃 path 是 LAN 还是 relay"，需要订阅内部事件或 inspect connection metadata，没有简单的同步 query
3. **mock relay 难度高**：要写"开 LAN-only 后 relay 不发流量"的负向测试，需要一个 mock relay server 计数器，验证计数为 0。但 mock relay 部署成本极高
4. **行为正确 vs. 配置正确**：测试只能断言 `IrohNodeConfig.disable_relays == expected_value`（配置层），无法断言 endpoint 真的没有 relay path（行为层）
5. **集成测试不会捕捉跨平台/跨网段差异**：CI 上跑的是 loopback，永远不会暴露 Pitfall 7 的真实环境问题

**Why it happens:**

- iroh 的 relay 行为是"NAT 阻断时才用"，所以即使开了 relay fallback，本地 happy-path 测试也直接走 LAN，relay 路径在 CI 中 **几乎从不被走到**
- "我跑了集成测试都过了" 给出的安全感是假的
- 单元测试天然只能测"配置传递正确"，验证不了"runtime endpoint 行为"

**How to avoid:**

- **分两层测试明确**：
  - **Tier A（自动化）：配置传递断言** —— `bootstrap_propagates_allow_relay_fallback`：写 `(true, false)` 两组 settings，断言 `IrohNodeConfig.disable_relays` 取值正确。这是 cheap 的，必须有。
  - **Tier B（自动化）：endpoint 状态断言** —— bind 后用 `endpoint.addr()` 检查 `addrs` 中是否有 `TransportAddr::Relay(_)` 项。`disable_relays = true` 时不应该有 relay URL 出现（可参考 `node.rs:172-199` 的 `log_publish_addrs` 已有逻辑）。
  - **Tier C（手动）：抓包 + 网络观测** —— 验收清单要求 reviewer 在三平台用 Wireshark / Console 抓包，确认开 LAN-only 后无指向 `*.iroh.network` 或 `*.n0.computer` 的流量。
- **集成测试不能默认 `disable_relays: true`**：本里程碑新增的测试用例 **必须显式说明** relay 配置选择，禁止照抄 slice2 测试的 `IrohNodeConfig { disable_relays: true, .. }` 模式（否则测的是空气）
- **手动验证清单作为 PR mandatory**：PR 模板加 checkbox "[ ] 已在三平台手动抓包验证 relay 流量为零"
- **回归 fence**：在 `uc-bootstrap/src/main_wiring.rs` 等装配点放一行 `debug_assert!()` 之类，验证 `IrohNodeConfig::disable_relays == !settings.network.allow_relay_fallback`，这是 unit-test 边界以外的运行时断言

**Warning signs:**

- 新增测试 fixture 重复用 `disable_relays: true`
- PR 描述里只说"集成测试都过了"，没有提到手动 / 抓包验证
- 没有任何测试断言 `endpoint.addr().addrs` 的内容
- "在我机器上 work" 是 PR 的主要佐证

**Phase to address:**
**Phase 2（注入路径）** —— Tier A/B 测试；**Phase 6（QA / 验收）** —— Tier C 手动验证清单。

---

### Pitfall 9: 文档措辞陷阱——把"LAN-only"过度承诺

**What goes wrong:**
（与 Pitfall 5 互补，专注 **文档** 而非 UI 文案）

文档里如果出现下列任一句子，就埋了口碑雷：

- "LAN-only Mode ensures all traffic stays on your local network" —— 错，配对走外网
- "完全离线模式" —— 错，遥测/更新仍走外网
- "Disable all internet connections" —— 错，pkarr lookup 仍发包
- "Your data never leaves your network" —— 措辞过强，如果用户绝对相信这句话，配对失败时会无法理解

文档比 UI 文案更危险——文档会被搜索引擎抓取、被 reddit 引用、被 reviewer 截图。一旦写错，纠错难度远高于改 UI 文案。

**Why it happens:**

- 文档作者通常是开发者本人，写完代码顺手写文档时容易直接搬用 commit message 里的口语化表达
- "LAN-only" 是英文短词，没有空间一句话讲清边界，作者容易省略 caveat
- README / changelog / 设置页文案是三个独立 surface，写文档时容易只更新其中一个

**How to avoid:**

- **统一术语清单**：文档项目里维护一个 `terminology.md`，规定：
  - 推荐用语："Limit clipboard sync traffic to LAN" / "Disable public-relay fallback for clipboard sync" / "限制剪贴板同步流量在局域网内"
  - 禁止用语：完全离线 / fully offline / no internet / private mode / encrypted-and-local（这些都会引起更广泛的隐私承诺联想）
- **文档审核 checklist**：文档 PR 模板必须勾选"LAN-only 文档已经包含'首次配对仍需联网' caveat"
- **同步发布的三个 surface**：UI 文案 / README / docs/lan-only.md 必须在同一 PR 中更新，用 `git grep -i "lan-only"` 确认没有遗漏 surface
- **截屏对照**：文档里如果有截图，截图必须包含 toggle 旁的 info icon / tooltip。截不到 tooltip 的截图视为不合格
- **第三方引用准备**：准备一份 "How to describe LAN-only Mode accurately"（README 中）的官方解释，方便博客作者引用，避免他们二次创作误解

**Warning signs:**

- README diff 里出现 "fully offline" / "no internet" / "完全离线"
- 设置页文案、changelog、docs/lan-only.md 三处描述不一致
- changelog 里写"P2P encrypted, fully local"
- 文档 PR 不需要 reviewer

**Phase to address:**
**Phase 5（文档）** —— 必须有专门的"边界文档"deliverable，且作为 release blocker。

---

### Pitfall 10: 重启提示 UX——用户开关后看不到效果反复切换

**What goes wrong:**
因为 iroh `RelayMode` 是 bind-time（Pitfall 3），切换 LAN-only Mode 不会立即生效。如果重启提示做得弱（只是 toast 一闪），用户的体验就是：

1. 切 toggle → 看到 toast 一秒钟 → toast 消失
2. 看设备列表通道指示器没变化 → 怀疑没生效
3. 切回去 → toast 又一闪
4. 反复几次 → settings 文件被反复写、用户陷入"开关好像没用"的负面体验
5. 用户卸载，跑去 Reddit 写"开了开关没用，骗子"

更糟糕的是：开关切换可能触发其他副作用（比如写设置触发了某个 effect handler 重新订阅事件），切多了可能把后端搞到不一致状态。

**Why it happens:**

- 一般 toggle 的设计 metaphor 都是"切了就生效"
- "重启生效"是开发者不喜欢的妥协，所以容易做得马虎，希望"反正用户会重启"
- toast 通知是最便宜的实现，但稍纵即逝
- 没有视觉锁定：切换后 toggle 视觉态没变化，用户不知道"已经切了，但还没生效"

**How to avoid:**

- **三态视觉**：toggle 必须有三种视觉态：
  - **applied OFF**（白色滑块在左，绝对不打扰）
  - **applied ON**（蓝色滑块在右，已生效）
  - **pending change**（黄色 / 带感叹号 / 带"重启生效" inline 标签）—— 用户切换后立刻进入这一态，settings 内部已写入但 UI 表达"尚未生效"
- **持久化通知**：切换后 **不要** 用 toast，要在 toggle 下方插入一行 **持久** 的"应用需重启以应用 LAN-only 设置"提示，附"立即重启"按钮（调 daemon 的优雅 shutdown + relaunch）
- **再次切换的反应**：在 pending change 状态下用户再切回原值，toggle 应该回到 applied 态（取消修改），而不是叠加新的 pending。这要求 UI store 区分 "current persisted value" 和 "pending edit value"
- **重启流程的诚实**：如果用户点"立即重启"，UI 必须在重启前后都明确——重启进行中的 loading 态、重启完成后的"已生效" toast 持续 3 秒。让"切了 → 重启 → 看到效果" 这个循环肉眼可见
- **避免连续写入**：settings repo 在 pending change 期间不能反复落盘，应该 debounce（500ms）或者只在用户明确点"应用"时落盘——避免 4 次切换写 4 次 disk
- **接受度回退**：onboarding tip 里告诉用户"如果不想用，再次切换并重启即可恢复"，降低误试的心理成本

**Warning signs:**

- 切换 toggle 后只有 toast，没有持久提示
- 切换 toggle 后 toggle 视觉态立即变成 applied（绿/蓝）
- 没有"重启"按钮，要求用户手动从菜单 Quit
- 反复切换会重复触发 settings 写入和事件
- 用户切换后通道指示器无任何变化（包括"待重启"标识）

**Phase to address:**
**Phase 4（重启提示 UX）** —— 三态视觉 + 持久通知 + debounce 写入是这个 phase 的最小集合，缺一个都不算 done。

---

### Pitfall 11: NetworkSection 旧占位组件残留 / 复用风险

**What goes wrong:**
当前 `src/components/setting/NetworkSection.tsx` 是占位实现（直接引用源代码注释"网络设置功能在新架构中尚未实现"）。本里程碑会把它替换成实际功能。如果替换不彻底——比如保留旧的 i18n key `'settings.sections.network.placeholder'`，或者保留旧组件作为 `NetworkSectionLegacy` 备份——就会出现：

- i18n 文件里出现孤儿 key 持续 lint warning
- 测试 import 旧组件
- 未来的 NetworkSection 扩展（自托管 rendezvous、网络诊断）参考旧占位写法，复制粘贴 `placeholder` 文案

**Why it happens:**

- 占位组件是"无害"的，开发者倾向"留着以防万一"
- i18n key 删除会担心遗漏引用导致运行时缺 key 报错

**How to avoid:**

- **彻底删除占位 div + i18n key**：本里程碑必须把 `'settings.sections.network.placeholder'` key 从所有语言文件删除，组件内部不允许保留 `placeholder` 文案分支
- **lint 规则**：`tsc` / eslint 要能在新代码引入孤儿 i18n key 时报错（项目应该已有，verify 一下）
- **PR 描述要点列表**：明确"删除了 placeholder i18n key"

**Warning signs:**

- diff 里出现 `// TODO: keep placeholder fallback`
- i18n 文件中 `placeholder` key 还在
- 新组件 import 了 `SettingGroup` 但同时 fallback 渲染 placeholder div

**Phase to address:**
**Phase 3 或 Phase 4（前端实现）** —— 替换占位是该 phase 的首要 deliverable。

---

### Pitfall 12: 配对成功后 onboarding tip 时机错位

**What goes wrong:**
explore 阶段定了 "配对成功后弹一次性提示" 引导用户发现 LAN-only 开关。这个 tip 的实现时机有几种错法：

1. **太早**：配对中途（用户还在等对方确认）就弹，破坏关键流程，用户错点关闭，错过整个发现机会
2. **太晚**：配对完成后 N 秒、用户已经离开 setup wizard 进入 Dashboard，tip 浮起在 Dashboard 上无上下文
3. **每次配对都弹**：用户配多台设备的体验里反复弹，从"发现"变成"骚扰"
4. **没有持久化**：tip 关闭后没有记一个 `discovered_lan_only_tip = true` 的本地 flag，下次配对又弹
5. **跨平台 modal 行为差异**：macOS / Windows toast 行为不同，tip 在某个平台直接看不见
6. **跟 LAN-only 开关切换实际行为脱节**：tip 文案只说"试试 LAN-only Mode"，没说"这意味着跨网段设备会断"，用户开了之后才发现副作用

**Why it happens:**

- onboarding tip 通常被当作 "P1" 不重要功能，实现仓促
- 没有 onboarding tip 框架——这是项目首个 onboarding tip，没有可复用的 dismissed-state 持久化机制
- 文案设计偏重"发现感"，不重提示"代价"

**How to avoid:**

- **明确触发条件**：tip 仅在 **首次配对成功** 那一次显示，非首次跳过。"首次"判定基于 `members.count() == 2 && !settings.dismissed_tips.contains("lan_only_v0_7_intro")`
- **持久化 dismissed flag**：在 settings 里加 `dismissed_tips: HashSet<String>`，tip 显示后无论用户怎么操作（关闭 / 跳转 / 不理）都标记为 dismissed
- **延迟时机**：tip 在 setup wizard "Done" / "Next" 按钮被点之后展示，让用户清楚"配对已完成"，进入下一阶段才看到 tip
- **inline 显示，非 modal**：用 Dashboard 顶部 inline banner（带 dismiss X），不要打断式 modal。modal 跨平台行为差异大且打扰强
- **文案带边界**：tip 不能只说"试试 LAN-only Mode"，要包含一句"它会让跨网段（比如家里和公司）的设备同步失效，开启前请评估"
- **跳转链接而非自动开启**：tip 中"了解更多"按钮跳转到 Settings 的 Network 区，**绝不** 直接帮用户开启开关

**Warning signs:**

- onboarding tip 在配对 **进行中** 而非完成后展示
- 没有 dismissed_tips persistence
- tip 用 modal 而非 banner
- 文案只描述好处，不描述代价
- "了解更多"按钮直接帮用户切了开关

**Phase to address:**
**Phase 5（onboarding tip / 文档）** —— banner 实现 + dismissed_tips persistence + 文案 review 是该 phase 三块 deliverable。

---

## Technical Debt Patterns

针对本里程碑可能出现的"看似合理实则负债"的捷径。

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| 在 frontend store 临时维护 `lanOnly = !allowRelayFallback` 镜像 | UI 组件读取直观 | 双源 truth，下次有人改一边忘改另一边 | 永不接受 |
| onboarding tip 用 in-memory flag 而非 settings persistence | 实现快 | 重启后用户被反复提示 | 永不接受 |
| 通道指示器只用 `PresenceEvent` 不加 polling 兜底 | 一个事件流足够 | 长时间后台后状态错位 | 永不接受 |
| 把 `disable_relays` 直接当 settings 字段名（不做反向命名） | 少一层取反 | i18n 文案被锁死成"disable relays"，营销难讲 | 永不接受 |
| toggle 切换后立刻视觉态变成 applied | 看起来响应快 | 用户以为生效了，反复切换 | 永不接受 |
| 用 `if peer.ip.starts_with("192.168")` 推断通道是 LAN | 不需要新 port | Tailscale / Clash / Docker 桥接全错 | 永不接受 |
| 集成测试照搬 slice2 的 `disable_relays: true` 配置 | 测试很快过 | 测的是空气，relay-fallback 路径无人测 | 永不接受 |
| schema migration 直接升 `CURRENT_SCHEMA_VERSION` | "看起来更严谨" | 触发不必要的 migration codepath | 仅在字段存在歧义时；本里程碑加新字段不需要 |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| iroh `Endpoint` | 想运行时切换 `RelayMode` | 关闭 + 重启进程，本里程碑明确不做 in-process 重建 |
| iroh `Endpoint::addr()` | 用它判定"当前是否在用 relay" | 这个 API 返回的是 **对外发布的候选地址**，不是当前活跃 path。需要新 port 抽象 |
| `tokio::sync::broadcast` | 假设 receiver 不会丢消息 | lagging receiver 必丢；UI 必须 polling 兜底 |
| settings serde | 给新字段 `#[derive(Default)]` | 必须显式 `impl Default`，bool 默认 false 极度危险 |
| OTLP exporter | 跟 LAN-only 联动 | 两个 settings 字段语义独立，禁止联动 |
| pkarr discovery | 当作 relay 一部分一并禁用 | pkarr 不归 `RelayMode` 管，本里程碑无法关闭 pkarr lookup |
| mDNS | 默认在 Windows / Linux 跨网卡环境工作 | 实际经常被防火墙挡，需要文档标注 |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| settings 写入未 debounce | 用户快速切换 toggle 时 disk I/O 爆 | UI 层 debounce 500ms，或显式"应用"按钮 | 用户连续切换 5+ 次 |
| 通道指示器轮询频率过高 | UI 帧率下降、daemon CPU 占用上涨 | 5–10s 间隔，inactive 视图（设备列表未展示）时停止 | 设备数 > 10 |
| `PresenceEvent` 累积未消费 | broadcast buffer 满，旧事件丢失 | 订阅端及时消费 + lag-aware 处理 | UI 后台超过 1 分钟 |
| 频繁重启 daemon 验证 LAN-only 行为 | 启动慢、用户体验割裂 | 重启 UX 流畅；考虑 progress 进度条 | 每次切换都要重启 |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| LAN-only Mode 开启状态被宣传为"完全私密" | 用户基于错误前提信任产品，关键场景误用（如分享商业机密） | 文档严格列出仍走外网的请求；UI tooltip 同步 |
| `dismissed_tips` 字段在 settings JSON 落盘明文 | 泄漏用户使用模式（哪些 tip 被看过） | 使用纯本地 key，无敏感语义即可，但避免暴露用户 ID 关联 |
| 通道指示器把对端 IP 显示给用户（ "Connected to 192.168.1.5"） | 公开内网 IP，且 tooltip 截屏时可泄漏 | 仅显示通道类型（LAN/Relay/Offline），不显示 IP |
| 文档使用绝对化措辞引导用户关掉防火墙以"让 LAN-only 工作" | 用户安全态势下降 | 文档明确"如果防火墙阻止 mDNS，请单独评估开放 5353/UDP 的安全影响" |
| 不在 `RelayMode::Disabled` 时清理已建立的 relay-based connection | 用户开了 LAN-only 但 relay 连接仍持续到 idle timeout | 切换后强制 close 现有 connection，让重启接管（已通过"重启生效"机制 implicit 解决） |

---

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| toggle 切换后只有一秒 toast | 用户怀疑没生效反复切换 | 持久 inline 通知 + 三态视觉 |
| 通道指示器永远不显示 "Unknown" | 用户看到空白或错误的"LAN" | "Unknown" 是合法初始 / 过渡态 |
| onboarding tip 是 modal 弹窗 | 强中断、跨平台行为不一 | inline banner with dismiss |
| 设置项藏在三层菜单后面 | 用户根本找不到，等于没做 | Settings → Network 是 explore 决策，且有 onboarding tip 引流 |
| LAN-only 开启后远程设备显示 "Offline" 无解释 | 用户以为程序坏了 | "Out of LAN" 状态 + tooltip 解释 LAN-only 已开启 |
| 切换需重启但没"立即重启"按钮 | 用户要去找 Quit 菜单或 Activity Monitor | inline 通知附"立即重启"按钮 |
| 文案使用 "fully offline" / "完全离线" | 与现实不符引发投诉 | "Limit sync traffic to LAN" / "限制同步流量在局域网内" |

---

## "Looks Done But Isn't" Checklist

- [ ] **后端 `network` namespace**：检查 `Settings::default()` 的 `network.allow_relay_fallback == true`；检查老 settings JSON 反序列化也是 true
- [ ] **取反转换函数**：搜索 `disable_relays =` 全部定位，确认只在 `relay_policy_to_iroh_config()` 一处，且参数名是 `allow_relay_fallback`
- [ ] **端到端 IPC**：从 frontend toggle event → daemon settings update → 重启后 IrohNodeConfig 读取的全链路真有用 `false` 走通过
- [ ] **通道指示器**：手动断网 / 切 Wi-Fi 测试是否能正确反映 LAN → Offline → Relay 切换
- [ ] **通道指示器 Unknown 态**：刚启动还没拨号时是否显示 Unknown
- [ ] **重启提示**：切换 toggle 后查 settings 文件落盘了，但运行时行为未变（直到重启）
- [ ] **onboarding tip 持久化**：tip dismiss 后重启 daemon 不应再次出现
- [ ] **OTLP 不被影响**：开 LAN-only 重启后 OTLP exporter 仍 active（除非用户单独关了 telemetry_enabled）
- [ ] **删除占位组件**：grep `placeholder` 在 NetworkSection 相关 i18n / TS 文件中无残留
- [ ] **三平台**：macOS / Windows / Linux 三个手动验证清单跑过
- [ ] **抓包**：开 LAN-only 后 Wireshark / tcpdump 验证无 `*.iroh.network` 流量
- [ ] **文档边界**：README / docs / changelog / UI tooltip 全部包含"首次配对仍需联网"caveat
- [ ] **i18n 完整**：中文 / 英文（至少这两个）的 toggle 文案 + tooltip + 重启提示 + onboarding tip 都翻译到位
- [ ] **rollback 路径**：用户开了 LAN-only 后再关，是否能完整恢复到 v0.6.0 等价行为

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| 默认值倒置发版 | HIGH | hotfix 版本紧急发布；对所有已升级用户的 settings 文件做 patch（强制写 `allow_relay_fallback = true`）；公告 + 道歉 |
| LAN-only 文案被理解为"完全离线" | HIGH | 撤回该版本宣传文案；docs / README / 设置 tooltip 三处同步修订；配套发布"What LAN-only does and doesn't" 解释帖 |
| 通道指示器持续显示错误状态 | MEDIUM | hotfix 中改为只显示 Online/Offline 两态，把不可靠的 LAN/Relay 区分降级为 hidden behind "Show advanced status" |
| 切换 toggle 触发反复重启崩溃 | MEDIUM | hotfix 加 settings 写入 debounce + 切换冷却 |
| LAN-only 在 Windows mDNS 不工作 | MEDIUM | 文档加 known issue，UI 提供"诊断"入口，下个里程碑评估替代发现机制 |
| 反向命名搞反方向（开关与流量行为相反） | LOW | hotfix 修一行；可在转换函数加 invariant 检查长期防御 |
| onboarding tip 反复出现 | LOW | hotfix 加 dismissed_tips persistence |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| 1. 反向命名搞反方向 | Phase 1（schema + 取反函数） | truth-table 单测；review 中只允许一处取反 |
| 2. 默认值倒置 | Phase 1（schema） | 老 settings JSON 反序列化测试；显式 `impl Default` |
| 3. 运行时热切换的诱惑 | Phase 2（注入到 IrohNodeConfig）+ Phase 4（UX） | 禁止 endpoint 重建；UpdateNetworkSettings 返回 `restart_required` |
| 4. 通道指示器与真实状态偏差 | Phase 3（指示器） | 新增 `ConnectionChannelPort`；事件 + polling 双兜底；Unknown 态可见 |
| 5. "LAN-only" 营销语 vs 现实边界 | Phase 4（UX） + Phase 5（文档） | UI tooltip + docs/lan-only.md 列出仍走外网的请求 |
| 6. OTLP 联动模糊 | Phase 1（决策固化） + Phase 5（文档） | bootstrap 中无 `network.allow_relay_fallback` 读取；文案明示独立性 |
| 7. 跨平台差异 | Phase 6（QA / 验收） + Phase 3（指示器诊断） | 三平台 × 四场景手动 QA 矩阵；IPv6 ULA filter 补丁 |
| 8. 测试覆盖陷阱 | Phase 2（注入） + Phase 6（QA） | Tier A/B 自动断言 + Tier C 抓包验证；新测试不允许照抄 `disable_relays: true` 配置 |
| 9. 文档措辞陷阱 | Phase 5（文档） | terminology.md 维护；UI / README / docs / changelog 四处一致性 review |
| 10. 重启提示 UX | Phase 4（UX） | 三态视觉 + 持久 inline 通知 + debounce + "立即重启"按钮 |
| 11. NetworkSection 占位残留 | Phase 3 或 Phase 4（前端实现） | grep `placeholder` 全部清理；i18n key 删除 |
| 12. onboarding tip 时机错位 | Phase 5（onboarding tip） | banner 而非 modal；dismissed_tips 持久化；文案带边界说明 |

---

## Sources

- `src-tauri/crates/uc-infra/src/network/iroh/node.rs` — `IrohNodeConfig.disable_relays` 当前定义；`RelayMode` bind-time 决策（line 368-396）；`AddrFilter` 已有 IPv4 虚拟 NIC 过滤；`log_publish_addrs` 候选地址快照；`max_concurrent_multipath_paths` 与 path 选路相关
- `src-tauri/crates/uc-core/src/settings/model.rs` — `Settings` schema、`telemetry_enabled` 默认 true、`schema_version = 1`、`#[serde(default)]` 字段约定
- `src-tauri/crates/uc-core/src/settings/defaults.rs` — `impl Default for Settings` 模式
- `src-tauri/crates/uc-core/src/ports/presence.rs` — `PresenceEvent` 三态 + broadcast lag drop 行为说明
- `src-tauri/crates/uc-observability/src/otlp/config.rs` — OTLP 配置散布在 env / baked / settings 三处
- `src/components/setting/NetworkSection.tsx` — 当前占位实现，本里程碑替换目标
- `.context/attachments/Summary of Explore LAN version need.md` — explore 阶段对话与决策记录
- `.planning/PROJECT.md` — 当前里程碑范围（PROJECT.md:11-33）、Out of Scope（PROJECT.md:103-112）
- `src-tauri/crates/uc-bootstrap/tests/slice*_e2e.rs` — 现有集成测试的 `disable_relays: true` 模式（即测试覆盖陷阱来源）
- iroh upstream — `RelayMode` 在 `Endpoint::builder().relay_mode(...).bind()` 处确定，无 runtime API（参考 iroh 0.97/0.98 文档）

---
*Pitfalls research for: v0.7.0 LAN-only Mode toggle*
*Researched: 2026-05-04*
