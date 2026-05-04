# Feature Research

**Domain:** LAN-only Mode 开关 + 连接通道可观察性（v0.7.0 里程碑增量）
**Researched:** 2026-05-04
**Confidence:** HIGH（Syncthing / Resilio Sync / KDE Connect / LocalSend / Tailscale / Magic Wormhole 均已查证一手资料）

---

## 里程碑功能增量摘要

**这是一个范围窄、非常聚焦的里程碑**。不是要给 UniClipboard 加全套网络功能，而是只回答一个用户场景下的一对问题：

> "我能不能确定流量没出局域网？我怎么验证？"

因此功能增量只覆盖**两块**：

1. **开关本身**（LAN-only Mode 切换 + 重启提示 + 后端 `network` 命名空间）
2. **可观察性**（设备列表的"连接通道"指示器，让"局域网专用"可被肉眼验证）

外加少量**贴边的辅助项**：onboarding tip + 文档透明披露。

**不在本里程碑范围**（已在 PROJECT.md 锁死）：
- 自托管 rendezvous
- 跨网段静态地址簿 / 手动 NodeId 输入
- 运行时热切换
- 独立 LAN-only 二进制 flavor
- 完全无联网首次配对

**信任锚点的核心定义**：用户能 (1) 在 UI 看见开关状态、(2) 在设备列表看见"LAN / Relay / Offline"标签、(3) 配对成功被一次性 tip 引导发现该开关、(4) 在文档里读到边界。这四件事拼起来才构成"可信的 LAN-only"。任何一条缺失，"开关亮了"就不等于"可验证"。

---

## 类似产品如何做（对标参考）

| 产品 | 开关位置 | 默认值 | 切换是否需重启 | 设备状态显示 | 关键经验 |
|------|----------|--------|----------------|--------------|----------|
| **Syncthing** | 用户最初提议放在 Connections 菜单的复选框；社区争论后核心维护者推翻"简单 toggle"方案，建议改成带确认弹窗的"Disable external requests"按钮 | OFF（默认开放 global discovery + relay） | 维护者明确说 LAN-only "不能从根本上保证不出网" → 必须用 explicit 警告 | 远端设备列表里有专门的 "Connection Type" 字段，值为 `TCP LAN` / `TCP WAN` / `QUIC LAN` / `QUIC WAN` / `Relay WAN` | "LAN-only" 是营销语，技术上必须配套**可观察的连接通道**才能让用户验证，否则只是"安慰开关" |
| **Resilio Sync** | Share Preferences + Power User Settings 双层；同时也支持配置文件 `sync.conf`（`use_relay_server`, `use_tracker`, `search_lan`） | 默认开启 relay + tracker | 改后需要重启才彻底生效，且坑点：之前用过 WAN 连过的 peer 会缓存 IP，需要把 "Peer expiration" 调成 0 重启再调回 | 状态简单（Direct / Relay），没有专门 channel UI badge | 配置散在多个层会让用户困惑，**单一开关 + 单一文档说明**比"很多旋钮"对 LAN-only 用户更友好 |
| **KDE Connect** | 没有 LAN-only 开关 —— 它**本身就只跑 LAN**（UDP broadcast 发现 + TCP 直连） | N/A | N/A | 没有"是否经过中继"概念，因为它根本不做中继 | 极简心智模型：要 LAN-only 就什么花活都不要给 = 用户不用理解任何拓扑 |
| **LocalSend** | 没有开关 —— 与 KDE Connect 类似，**架构上就只跑 LAN**（UDP multicast 广播 + TCP 响应 + 端口 53317） | N/A | N/A | 设备发现仅显示同 LAN 设备；跨子网失败时引导用户手动输 IP | 验证方式 = 用户根本看不到 WAN 上的设备，"看不到"本身就是承诺 |
| **Magic Wormhole** | 没有开关 —— 客户端总是**先尝试直连 LAN，失败才回落到 transit relay**；但用户控制的是"等几秒再回落"参数 | 自动 | N/A | 协议层有"connection hints"概念但 CLI 不显示，用户感知是"快=直连，慢=relay" | 默认偏向 LAN 但不堵死 relay，对追求确定性的用户**不够**，需要 explicit kill-switch |
| **Tailscale** | `tailscale up --advertise-exit-node` 等 CLI flags + admin console；DERP relay 关不了，只能"尽量直连"，没有真·LAN-only | 自动（首选直连，DERP 兜底） | bind 时确定 | `tailscale status` 输出明确写 `direct 140.82.x.x:port` 或 `relay "tor"` 或 `peer-relay`；`tailscale ping` 显示每包路径 `via DERP(xxx)` 或 `via <ip>:<port>` | **CLI 层把通道暴露成一等公民**——这正是 UniClipboard 应该在 GUI 里做的事 |
| **AirDrop** | 没有开关 —— P2P Wi-Fi Direct + BLE 发现，**架构上完全离线**（飞行模式可用，无需 SSID） | N/A | N/A | 验证设备显示头像 + 名字，未验证设备进 "Other People" 区域 | 把"已识别 / 未识别"做成视觉差异，是另一种"信任锚点"的 UI 表达 |
| **PairDrop** | 自托管/远程模式可选；6 位码 / QR Code 配对 | 默认本地优先 | N/A | 区分"本地 LAN 设备" vs "通过 6-digit code 配对的远程设备"两个区域 | UniClipboard 已决定"首次配对仍需联网"——即使如此，**把 LAN 设备和远程设备视觉区分**仍然是必要的（属于本里程碑的"连接通道"指示器） |

**关键 takeaway 给 UniClipboard 的**：

1. Syncthing 维护者拒绝纯 toggle 的理由是核心警示：**"LAN-only 不是技术保证，必须配可观察通道才不变成营销谎言"** —— 我们把"连接通道指示器"列入 P0 与开关同级，正好直面这个批评。
2. Tailscale `status` 输出是连接通道展示的黄金范本：`direct ip:port` / `relay "name"` / `peer-relay` 三态清晰。UniClipboard 做 GUI 版即可。
3. Resilio Sync 的"配置散在多层 + IP 缓存坑"是反面教材，UniClipboard 的开关必须**单一入口、重启生效语义清楚**。
4. KDE Connect / LocalSend / AirDrop 这一档"架构上就只跑 LAN"虽然信任最强，但需要重写发现层，**当前里程碑做不到也不应做**——所以"开关 + 可观察"是务实折中。

---

## Feature Landscape

### Table Stakes (Users Expect These)

最小集，缺任何一项，"局域网洁癖"用户都不会信你这是 LAN-only。控制在 ≤8 条。

| # | Feature | Why Expected | Complexity | Notes |
|---|---------|--------------|------------|-------|
| 1 | **Settings → Network 分类下出现 "LAN-only Mode" 开关** | 用户的第一次寻找路径就是"设置里有没有"，没找到 = 不存在 | LOW | `NetworkSection.tsx` 当前是占位，本里程碑替换为真实开关。归在 Network 而不是 Sync/Security 已在 explore 阶段定（"流量怎么走" = 网络拓扑层） |
| 2 | **后端 `Settings` 新增 `network` 命名空间，字段 `network.allow_relay_fallback: bool`（默认 true）** | 没有持久化字段就没有真"开关"，每次启动都重置不可接受 | LOW | 反向命名：UI "LAN-only Mode = ON" 对应字段 `allow_relay_fallback = false`。语义稳定，未来加"自定义 relay 白名单"等中间态字段不会冲突 |
| 3 | **启动时把字段读入 `IrohNodeConfig.disable_relays`** | 字段存了不读 = 摆设 | LOW | 现有代码 `node.rs:147-150` 已经支持 `disable_relays`，仅缺 wiring |
| 4 | **切换开关后弹"重启生效"提示（modal 或 toast）** | iroh `RelayMode` 是 bind 时确定，运行时改不了；不提示用户会以为切完就生效，过一会儿仍看见 relay → 信任崩 | LOW | 提示 UX：toast 比 modal 轻；按钮可选"Restart Now" 或"Later"。首次提示后的 grace 内不重复弹（避免 toggle 抖动骚扰） |
| 5 | **设备列表显示连接通道徽章：LAN / Relay / Offline** | "LAN-only ON 但设备显示 Relay"才能让用户立刻发现配置没生效；这是验证回路 | MEDIUM | 直接对标 Syncthing `Connection Type` 字段 + Tailscale `status` 输出。UniClipboard 内部有 iroh ConnectionType（direct vs relay）已知，仅缺前端展示 |
| 6 | **配对成功后的一次性 onboarding tip** | 开关藏在 Settings 深处，不引导没人会发现 = 等于没做。"一次性" = 不打扰已经习惯的老用户 | LOW | localStorage 存 `lan_only_onboarding_seen: true`；用户点关闭或点 "Don't show again" 都视为已看过 |
| 7 | **文档透明披露 "LAN-only 边界"** | "首次配对仍需联网经 rendezvous" —— 这是硬伤，不主动说 = 用户上手发现就觉得被骗 | LOW | 至少有：(a) Settings 里的开关说明文案、(b) 项目 docs/lan-only.md 一页详解、(c) 配对页面有一行小字 "首次配对会联系 rendezvous.uniclipboard.app"。三处任一缺失都失分 |
| 8 | **"重启生效"语义清楚，不暴露半生效状态** | 用户切完没重启时，UI 不应该撒谎说 "LAN-only ON" 但实际还在用 relay | LOW | 方案：开关切完显示 "ON (pending restart)" 视觉变体（如黄色边框）；重启后转为正式 ON 状态。或者切完直接禁用本身，文案明确 "Will take effect on next restart" |

### Differentiators (Competitive Advantage)

可加可不加，**本里程碑不强求**，但都标了复杂度，让 roadmapper 决定哪些挤进 v0.7.0、哪些丢到 v0.7.x / v0.8.0。

| # | Feature | Value Proposition | Complexity | Notes |
|---|---------|-------------------|------------|-------|
| D1 | **设备列表 channel 徽章带 tooltip 解释**（如 hover Relay 显示 "Encrypted relay via iroh; metadata visible to relay node"） | 把"为什么开 LAN-only" 的论证就近放给 hover 用户，教育成本低 | LOW | 文案需要简短中立，避免吓到普通用户。可参考 iroh FAQ 里 "relays know NodeID X talks to NodeID Y but cannot decrypt" |
| D2 | **设备列表为远端"Out of LAN"设备显示灰色/禁用态 + 解释 tooltip** | 用户开了 LAN-only 后，原本远程的设备会变灰，不解释 = 困惑 | LOW | 直接对标 PairDrop 把"本地 LAN 设备" vs "远程设备"分区。tooltip: "This device is outside your LAN. Disable LAN-only Mode in Settings to reach it." |
| D3 | **诊断面板：实时显示当前节点的对外连接**（每个 peer 一行，列出 channel + 数据量 + 最后活跃时间） | 高级用户验证的最强武器；Tailscale 用 `tailscale status` 实现，UniClipboard GUI 化 | MEDIUM | 可以放在 Network 分类下，作为开关下方的折叠展开区。需要后端暴露 `list_active_connections` 命令；很多基础设施已经在 telemetry 里，主要是 UI 工作 |
| D4 | **流量审计日志：以 OTLP span 暴露所有 relay 经过的 byte 总量** | 用 Seq 看时序，可证明"我开 LAN-only 之后 relay byte = 0" | MEDIUM | 现有 OTLP pipeline 已有 flow_id span 基础设施（v0.4.0 验证过）。新增 span tag `connection_path: lan/relay`。给开发者/资深用户的功能 |
| D5 | **首次配对 QR 码引导**（屏幕显示二维码，对方扫码 = 拿到 NodeId） | 走向"完全无联网首次配对"的过渡形态；当前里程碑不做完整 P2P 配对，但 QR 码可作为 rendezvous 的视觉替代体验 | HIGH | 不是把 rendezvous 删掉，而是把 6 字符 / NodeId 展示成 QR。**注意：本里程碑用户已敲定"接受首次配对需联网"，所以这是 v0.8+ 的事**，列在这里只是为了承上启下。**不应进入 v0.7.0** |
| D6 | **"测试 LAN-only" 按钮**：一键尝试连一个非局域网 peer，预期失败，UI 显示"已确认流量不会经由 relay" | 给用户一个"我能自己跑测试"的强信任锚点 | HIGH | 实现复杂（需要构造一个明显在 LAN 之外的 NodeId、避免误伤真实 peer），收益不一定值得。**不应进入 v0.7.0**，留作 v0.8+ |
| D7 | **状态栏（tray）显示"LAN-only ON"小图标** | 用户不打开主窗口也能确认状态，匹配"洁癖"用户的安全感诉求 | LOW | 现有 tray icon 系统可加 badge / 不同 icon 变体。文案极简（一个小锁或墙图标），hover 显示完整状态 |

复杂度回顾约束：D5 / D6 是 HIGH，**应该排除在本里程碑外**。D3 / D4 MEDIUM 可作为 stretch goal，roadmapper 决定。D1 / D2 / D7 LOW，**强烈建议挤进 v0.7.0**——它们对"信任锚点"贡献大但代码成本小。

### Anti-Features (Reject These — 必须给"为什么不做"理由)

诱人但不该做。每条都给具体技术或产品理由。

| # | Feature | Why Requested | Why Problematic | Alternative |
|---|---------|---------------|-----------------|-------------|
| A1 | **"完全无联网首次配对"**（自带 P2P 配对协议，跳过 rendezvous） | 用户字面理解的"局域网专用 = 任何时候都不联网"；强洁癖用户的终极诉求 | (a) iroh 的 NodeId 配对必须双向交换公钥；没有 rendezvous 就需要侧信道（QR / 蓝牙 / NFC / 手抄）。每条都是单独子项目（QR 实现需要 D5 + 摄像头权限 + 跨平台扫码 SDK；蓝牙需要 BLE 栈整合）。(b) 用户已在 explore 阶段拍板**接受首次配对需联网**。(c) 配对是低频事件，每次新设备一次性，对"流量绝对不出网"的边际损害很小 | **文档透明披露**这是已知边界（table stake #7）；将完整无联网配对延后到 v0.8+，并标注为 D5 |
| A2 | **"自动检测应该用 LAN 还是 Relay"**（无需用户开关，自适应） | 看起来更智能、更省心 | (a) Syncthing 论坛里**最常见的投诉之一**就是"明明在 LAN 却走了 relay"——auto detection 在 NAT/IP 缓存/广播延迟边界场景**经常判断错**，且用户没有 escape hatch。(b) 自动模式下用户**看不到行为契约**，也就**没法验证**——而验证是这个里程碑的核心需求。(c) 默认行为（开关 OFF）就是"自动模式 = 允许 relay 兜底"，已经覆盖普通用户；开关只服务于明确表态"我不要 relay"的用户 | 当前的"显式开关 + 默认 OFF"已经是正确选项；显式开关 + 可观察徽章给"自动模式"加了人类裁判 |
| A3 | **基于 IP 段的白名单 / 黑名单**（"只允许 192.168.0.0/16 内的 peer"） | 看起来比简单 toggle 更精细，企业 IT / NAS 用户会喜欢 | (a) 家用路由器普遍 dynamic IP 分配（DHCP lease），用户切 SSID / 重启路由器后 IP 段会变；白名单容易把自己锁出。(b) 跨网段（VLAN、有线+无线、Tailscale 内网）的"局域网"心智模型不是"同子网"，IP 段并不能精确表达"我的家"。(c) iroh 的 NodeId 已经是更精确的身份层，用 IP 段做白名单是把现代 P2P 倒退回 IPv4 思维。(d) 实现复杂：需要 IP 段输入 UI + 验证 + 跨平台获取 LAN 接口列表 + 错误恢复 | 信任锚点应建在 NodeId / 设备身份上（已加密配对），不是 IP；如果跨网段是问题，未来加"静态地址簿"（仍按 NodeId 索引）即可 |
| A4 | **运行时热切换 LAN-only 开关**（不重启就生效） | 切完不重启太"重"了，UX 不顺滑 | (a) iroh `Endpoint` 的 `RelayMode` 是 bind 时确定，运行时切要重建 endpoint。(b) 重建 endpoint 涉及关闭所有连接、重新发起配对会话、重传未完成的 transfer——状态管理复杂，本里程碑工程量翻倍。(c) "LAN-only" 是低频切换设置（用户开了一般不会频繁关），重启提示的成本可接受。(d) **风险**：热切换实现不到位会让用户在"切完没生效但 UI 显示已生效"的窗口期被骗 | 显式"重启生效"提示（table stake #4 + #8），把语义讲清楚比假装支持热切换更诚实；放到 v0.8+ 再考虑 |
| A5 | **独立 LAN-only 二进制 flavor**（编译期剔除所有 relay/rendezvous 代码） | 用户最强的信任承诺：可审计的二进制，无 relay 代码就 = 100% 不可能走 relay | (a) 双 binary 维护成本高（CI / 发布 / 文档 / 用户分发都要分叉）。(b) 主程序 + 开关已经覆盖 95% 的用户场景，剩下 5% "强洁癖"用户可以自己 cargo build with feature flag。(c) "可审计"承诺不需要独立 binary，可以让用户自行 build 和验证；本里程碑先做开关，看真实需求量再决定 flavor | PROJECT.md 已明确不在本里程碑范围；如果未来需求强烈，再加 cargo feature `--no-default-features --features lan-only` |
| A6 | **"严格 LAN-only 模式"，连 mDNS 失败都不允许 fallback 到 pkarr DHT**（更狠的版本） | 极少数硬核用户怀疑 pkarr DHT 也算"出网" | (a) 关掉 pkarr 之后跨网段连接率会从 ~90% 跌到接近 0；普通用户跨 VLAN / 有线-无线 都会失败。(b) pkarr DHT 是 NodeId 解析机制，不传输用户数据，性质和 DNS 类似，不应等同于 relay。(c) 本里程碑的"LAN-only" 定义已经在 PROJECT.md 锁定 = 关 relay fallback，不是关一切 P2P 网络。混淆这两件事会让 milestone scope 爆炸 | 文档里**明确定义"LAN-only" 的范围**：仅关闭公网 relay 兜底；mDNS + pkarr 解析 NodeId 仍然工作。强洁癖用户可以期待未来"strict mode"，但不在 v0.7.0 |
| A7 | **设置面板加入"自定义 rendezvous URL"输入框** | 既然首次配对需联网，让我自托管 rendezvous 不就是"局域网专用"了吗？ | (a) PROJECT.md 明确"自托管 rendezvous（v0.7.0）—— 配对仍走 rendezvous.uniclipboard.app，自建服务延后评估"。(b) 部署 rendezvous 需要服务端组件 + DNS + TLS，对最终用户是高门槛；加 UI 输入框等于鼓励一个还没开源/没文档的能力。(c) 现有代码 `IrohNodeConfig::rendezvous_base_url` 已是测试钩子，但生产暴露需要先把 rendezvous 服务开源化 | 当前里程碑透明告知"首次配对走官方 rendezvous"；把自托管延后到独立里程碑（很可能 v0.8+），先做服务端开源 + 部署文档 |

---

## Feature Dependencies

```
[1] LAN-only Toggle (UI)
    └──requires──> [2] Backend `network.allow_relay_fallback` field
                      └──requires──> [3] Settings → IrohNodeConfig wiring at startup
                                        └──requires──> 已有的 IrohNodeConfig.disable_relays
                                                       (uc-infra/.../node.rs:147-150 — 钩子已就绪)

[4] Restart-required Toast
    └──requires──> [1] Toggle exists to react to onChange
    └──independent of──> [3] backend wiring (toast 触发只依赖前端 onChange，与生效路径解耦)

[5] Connection Channel Badge (LAN/Relay/Offline)
    ├──requires──> 设备列表组件存在（已存在）
    ├──requires──> 后端暴露 `connection_type` 给前端（iroh ConnectionType → command DTO 新字段）
    └──INDEPENDENT of──> [1] / [2] / [3] / [4]
       【关键洞察】: 即使 LAN-only 开关 = OFF（默认），徽章也应该显示当前是 LAN 还是 Relay。
       这样徽章就成了"开关效果的反馈机制"。两件事必须同期发布：
         - 单发开关：用户切完没法验证 → 失信
         - 单发徽章：默认 OFF 时显示一堆 Relay 也没人 care → 信息浪费

[6] Onboarding Tip (一次性)
    └──requires──> [1] 开关存在
    └──requires──> 配对成功事件已知（已在事件总线中）
    └──requires──> localStorage flag `lan_only_onboarding_seen` (前端独立)

[7] LAN-only Boundary Documentation
    └──INDEPENDENT of──> 所有功能项（纯文档工作）
    └──coordinates_with──> Settings 中开关下方的 helper 文案 + 配对页面小字

[8] "ON (pending restart)" 视觉态
    └──requires──> [1] + [4] 共同实现
    └──状态机──> OFF → ON-pending → (用户重启) → ON-active
                              └─ OR (用户切回 OFF) → OFF（无重启需求）

依赖关系总结（回答用户提问"6 个 Active 项之间是否独立"）:
─────────────────────────────────────────────────────
✅ 强依赖：[2] → [1]（开关 UI 没有持久化字段就是个摆设）
✅ 强依赖：[3] → [2]（持久化字段没注入到 iroh 就没生效）
✅ 强依赖：[4] → [1]（toast 触发依赖 toggle onChange）
🔶 协同关系：[5] 与 [1]-[4] 技术上独立，但产品上必须同期 ship
   ↳ 两件事拼在一起才构成完整"LAN-only 信任锚点"
🔶 协同关系：[6] 依赖 [1] 存在但不依赖 [3] 已生效
🔶 协同关系：[7] 文档与 [1]-[6] 任何项都不耦合，可以提前 / 并行
─────────────────────────────────────────────────────
```

### Dependency Notes

- **关键发现：连接通道指示器（[5]）与开关（[1]-[4]）技术独立但产品强耦合。**
  - 技术独立是因为：徽章读的是 iroh runtime 实际连接状态，无论开关是 ON 还是 OFF 都应该显示。
  - 产品强耦合是因为：单发开关，用户没法验证，等于"信任承诺没有担保"；单发徽章，默认场景下徽章一堆 Relay，用户没有 actionable 路径。
  - **结论**：两个 issue 可以分别开发（可以并行），但必须同一个 release 发布。如果时间紧张要砍，**砍其中任何一个都比砍掉两个之一有意义**（尤其不能只砍徽章，那是单纯的"开关效果验证"基础设施）。

- **`network` 命名空间的预算意义**：本里程碑只新增一个字段 `network.allow_relay_fallback`，但建立的是一个空间——未来"自定义 rendezvous URL"、"自托管 OTLP endpoint"、"网络诊断面板"都能往这里扩展。命名空间一次定义，避免每次新功能都纠结放哪。

- **反向命名（`allow_relay_fallback` 而非 `lan_only`）的依赖含义**：UI 显示的 "LAN-only Mode = ON" 是 `allow_relay_fallback = false` 的视觉投影。这避免了"未来加自定义 relay 白名单时该字段语义崩塌"的尴尬——后端语义稳定，前端文案可调。

- **"重启生效" 提示与生效路径解耦**：toast 是前端事件，与后端是否真的重新初始化 endpoint 无关。这意味着即使 [3] 的 wiring 出 bug，[4] 的 UX 也不受影响——便于分阶段开发。

---

## MVP Definition

### Launch With (v0.7.0 — 这一里程碑)

来自 PROJECT.md "Active" 6 条，全部对应本节 P1：

- [ ] **[1+2+3]** Settings → Network 下加 "LAN-only Mode" 开关，默认 OFF；后端 `network.allow_relay_fallback` 字段（默认 true）；启动时 wiring 到 `IrohNodeConfig.disable_relays`
- [ ] **[4+8]** 切换开关后弹"重启生效"提示；ON 状态分 `pending restart` / `active` 两态视觉区分
- [ ] **[5]** 设备列表显示连接通道徽章（LAN / Relay / Offline）
- [ ] **[6]** 配对成功后的一次性 onboarding tip
- [ ] **[7]** 文档：`docs/lan-only.md` 详解 LAN-only 边界（首次配对仍需联网透明披露）+ Settings 开关下 helper 文案 + 配对页面小字

**强烈建议同期挤进的 LOW 复杂度差异化（保留决定权给 roadmapper）**：

- [ ] **[D1]** 徽章 tooltip 解释（hover Relay 显示"加密中继，元数据可见"）
- [ ] **[D2]** "Out of LAN" 设备灰色 + tooltip 引导
- [ ] **[D7]** Tray icon 显示 LAN-only 状态徽章

### Add After Validation (v0.7.x / v0.8.0)

数据驱动的下一步——先看 v0.7.0 用户反馈：

- [ ] **[D3]** 网络诊断面板（实时连接列表 + 数据量）—— 等"高级用户想要更多信息"反馈出现再做
- [ ] **[D4]** OTLP span 增加 `connection_path` tag —— observability 自然演进
- [ ] **[A4]** 运行时热切换 —— 等用户抱怨"重启太烦" 反馈出现再做
- [ ] **跨网段静态地址簿** —— 等用户抱怨"换 Wi-Fi 就连不上" 反馈出现再做

### Future Consideration (v0.8+ 或更后)

需要先解决前置问题或商业判断：

- [ ] **[D5]** QR 码 / 离线配对 —— 需要先评估 rendezvous 自托管路径，且涉及跨平台二维码 SDK
- [ ] **[D6]** "测试 LAN-only" 按钮 —— 实现复杂、收益不确定
- [ ] **[A5]** 独立 LAN-only 二进制 flavor —— 看 v0.7.0 + v0.8.0 的真实需求量
- [ ] **[A7]** 自托管 rendezvous URL 配置 —— 需要先开源 rendezvous 服务 + 提供 Docker compose 部署模板
- [ ] **[A6]** "Strict mode"（关 pkarr DHT）—— 极小用户群，先观察

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| [1] Settings UI 开关 | HIGH | LOW | P1 |
| [2] 后端 `network` 命名空间 + 字段 | HIGH | LOW | P1 |
| [3] 启动时注入 `IrohNodeConfig` | HIGH | LOW | P1 |
| [4] 重启生效 toast | HIGH | LOW | P1 |
| [5] 连接通道徽章（LAN/Relay/Offline） | HIGH（信任锚点核心） | MEDIUM | P1 |
| [6] 配对后 onboarding tip | MEDIUM | LOW | P1 |
| [7] LAN-only 边界文档 | HIGH | LOW | P1 |
| [8] "ON (pending restart)" 视觉态 | MEDIUM | LOW | P1 |
| [D1] 徽章 hover tooltip | MEDIUM | LOW | P2（建议挤进 v0.7.0） |
| [D2] "Out of LAN" 灰色 + tooltip | MEDIUM | LOW | P2（建议挤进 v0.7.0） |
| [D7] Tray icon 状态徽章 | MEDIUM | LOW | P2（建议挤进 v0.7.0） |
| [D3] 网络诊断面板 | MEDIUM | MEDIUM | P3（v0.7.x） |
| [D4] OTLP `connection_path` tag | LOW（开发者向） | MEDIUM | P3（v0.7.x） |
| [D5] QR 码配对 | HIGH | HIGH | P4（v0.8+） |
| [D6] "测试 LAN-only"按钮 | LOW（噱头） | HIGH | P4（v0.8+） |

**Priority key:**
- **P1**: v0.7.0 必须发布（对应 PROJECT.md Active 6 项）
- **P2**: 建议挤进 v0.7.0（LOW 成本 / 信任锚点加分）
- **P3**: 推迟到 v0.7.x（MEDIUM 成本 / 等数据反馈）
- **P4**: 推迟到 v0.8+（HIGH 成本 / 需前置工作）

---

## "用户怎么验证 LAN-only 真的生效了" — 信任锚点设计

这是用户提问的核心。把"信任"拆解成可观察的子契约：

| 信任契约 | 用户怎么观察 | 本里程碑覆盖 |
|----------|--------------|--------------|
| 我能找到这个开关 | Settings → Network 分类下有醒目"LAN-only Mode" 开关 | ✅ table stake #1 |
| 切完真的会生效（不是 placebo） | 切完弹"重启生效"，重启后 UI 显示 "ON (active)"；徽章里所有设备从 Relay 变 LAN（或变 Offline） | ✅ table stakes #4 + #8 + #5 |
| 我能看到当前流量是直连还是中继 | 设备列表每行有 LAN / Relay / Offline 徽章，反映 iroh runtime 实时状态 | ✅ table stake #5 |
| 切回 OFF 也会立刻 reflect | 切完同样提示重启；重启后徽章可恢复 Relay | ✅ table stakes #4 + #5 |
| 我能理解什么"算 LAN-only"什么"不算" | 文档明确披露："关闭 relay 兜底，但 mDNS + pkarr DHT 仍工作；首次配对仍走 rendezvous" | ✅ table stake #7 |
| 我能理解关了 LAN-only 之后远程设备为什么连不上 | "Out of LAN" 设备灰色 + tooltip 解释 | ✅ D2（建议挤进） |
| 我能在不打开主窗口的情况下确认状态 | Tray icon 加 badge / 不同图标变体 | ✅ D7（建议挤进） |

**故意没做但应当被理解的契约**（属于 anti-features 的诚实交代）：

| 用户可能期望 | 当前里程碑诚实回答 | 解决路径 |
|--------------|-------------------|---------|
| "我希望 100% 离线，包括首次配对" | 当前不行；首次配对走 rendezvous，已透明披露 | v0.8+ QR 码配对（D5） |
| "我希望审计二进制确认无 relay 代码" | 当前不行；relay 代码仍在 binary，靠 runtime 配置关掉 | v0.8+ 独立 flavor（A5） |
| "我希望切换不需要重启" | 当前不行；iroh `RelayMode` 是 bind-time 决定 | v0.8+ 运行时热切换（A4） |

**关键决定**：信任锚点不是"让所有契约都覆盖"，而是"让覆盖的契约清楚标记 ✅、未覆盖的契约清楚标记理由"。第二种 "诚实的不覆盖" 比"虚假的覆盖"更建立信任。这就是为什么 [7] 文档透明披露和 anti-features 必须同等重要。

---

## Competitor Feature Comparison Matrix

| 维度 | Syncthing | Resilio Sync | KDE Connect | LocalSend | Tailscale | UniClipboard v0.7.0 |
|------|-----------|--------------|-------------|-----------|-----------|---------------------|
| 提供 LAN-only 开关 | 提案 PR #10226 被关闭，未发布；建议改用"action button" | ✅ 配置文件 + Power User Settings | N/A（架构上就只跑 LAN） | N/A（架构上就只跑 LAN） | ❌（DERP 关不掉） | ✅ 单一 toggle，反向字段 `allow_relay_fallback` |
| 默认值 | OFF | 默认开 relay | N/A | N/A | DERP 总是兜底 | OFF（不打扰存量） |
| 切换需重启 | 未实现 | 是（且有 IP 缓存坑） | N/A | N/A | bind 时确定 | ✅ "重启生效"提示 |
| 连接通道指示器 | ✅ TCP LAN / TCP WAN / QUIC LAN / QUIC WAN / Relay WAN | 较弱（Direct / Relay） | N/A | 仅显示 LAN 设备 | ✅ CLI: `direct ip:port` / `relay "name"` / `peer-relay` | ✅ LAN / Relay / Offline 徽章（GUI 化） |
| 一次性 onboarding | 无 | 无 | 无 | 无 | 无 | ✅ 配对成功后引导 |
| 文档透明边界 | 一般 | 一般 | 极简（架构透明） | 一般 | 优秀（详细 KB） | ✅ docs/lan-only.md 计划 |
| Tray / 状态栏指示 | 无 | 无 | 无 | 无 | ✅ Mac/Windows 状态栏图标显示连接状态 | D7（建议挤进） |

**对比结论**：UniClipboard v0.7.0 在"开关 + 可观察"两件事上**与 Syncthing 持平甚至略优**（我们做了 Syncthing 暂未发布的事）；与 Tailscale CLI 透明度持平但用 GUI 表达；不与 KDE Connect / LocalSend 直接竞争（它们是不同的产品形态）。差异化优势来自"端到端加密 + LAN-only + 可观察"三件事的**组合**——单一 LAN-only 不稀奇，但配上 E2E 加密和实时通道徽章，足够回应 B 站用户的"信任锚点"诉求。

---

## Sources

### 一手 / 高置信度
- Syncthing LAN-only PR #10226（未合并，社区争论关键）: https://github.com/syncthing/syncthing/pull/10226
- Syncthing LAN-only 原始 issue #9377: https://github.com/syncthing/syncthing/issues/9377
- Syncthing 连接通道 issue #8244 + commit 8f2db99: https://github.com/syncthing/syncthing/issues/8244 / https://github.com/syncthing/syncthing/commit/8f2db99c86f624a922dd8280f70681f0c6f7904c
- Syncthing 连接类型论坛讨论: https://forum.syncthing.net/t/connection-types/24468
- Syncthing v1.30.0 (2025-07-01) 发布日志（确认 LAN-only 未在 1.30 发布）: https://forum.syncthing.net/t/syncthing-v1-30-0-2025-07-01/24574
- Syncthing relay 误用论坛贴（auto-detection 反例）: https://forum.syncthing.net/t/syncing-being-done-via-relay-and-not-lan/13831
- Resilio Sync 官方 LAN-only 配置: https://help.resilio.com/hc/en-us/articles/204754349-Can-I-force-Sync-to-do-local-network-LAN-syncing-only-and-not-sync-via-the-Internet
- Resilio Sync 配置参数: https://help.resilio.com/hc/en-us/articles/206178884-Running-Sync-in-configuration-mode
- KDE Connect 离线配置参考: https://ivonblog.com/en-us/posts/use-kde-connect-without-wifi/
- LocalSend 网络发现技术细节（DeepWiki）: https://deepwiki.com/localsend/localsend/2.6-network-discovery
- LocalSend 跨子网讨论: https://github.com/localsend/localsend/discussions/1254
- Magic Wormhole Transit Protocol: https://magic-wormhole.readthedocs.io/en/latest/transit.html
- Tailscale 连接类型 KB: https://tailscale.com/kb/1257/connection-types
- Tailscale ping types KB: https://tailscale.com/kb/1465/ping-types
- AirDrop 安全文档（peer-to-peer Wi-Fi 验证机制）: https://support.apple.com/guide/security/airdrop-security-sec2261183f4/web
- iroh 项目 FAQ（relay metadata 范围）: https://www.iroh.computer/docs/faq
- PairDrop（QR 码 + 6-digit code 配对参考）: https://github.com/schlagmichdoch/PairDrop

### 项目内部
- `.context/attachments/Summary of Explore LAN version need.md` — 用户决策已锁定的范围与 UX 风险清单
- `.planning/PROJECT.md` — Active / Out of Scope 范围声明
- `src/components/setting/NetworkSection.tsx` — 当前占位组件
- `src/components/setting/settings-config.ts` — Network 分类已挂在侧边栏（Wifi 图标）
- `uc-infra/src/network/iroh/node.rs:147-150` — `disable_relays` 钩子已就绪（仅缺 wiring 与 UI）

---

*Feature research for: v0.7.0 LAN-only Mode（开关 + 可观察性增量）*
*Researched: 2026-05-04*
