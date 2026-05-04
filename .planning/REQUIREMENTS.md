# Requirements: v0.7.0 LAN-only Mode

**里程碑：** v0.7.0 LAN-only Mode
**目标：** 给"局域网洁癖"用户一个可观察、可控的开关 —— 禁用 iroh 公网中继回落，让流量真正只走局域网，并把"当前是直连还是中继"暴露成可见状态。
**起源：** B 站用户反馈 → `.context/attachments/Summary of Explore LAN version need.md`
**研究：** `.planning/research/SUMMARY.md`（基于 STACK / FEATURES / ARCHITECTURE / PITFALLS 综合）
**起草日期：** 2026-05-04

---

## 范围摘要

把已存在但仅供测试用的 `IrohNodeConfig.disable_relays` 钩子，**暴露成用户可控的 "LAN-only Mode" 开关 + 设备列表"连接通道"指示器**。两件事产品强耦合（必须同期发布），技术解耦（可并行开发）。

**核心约束（所有 phase 必须遵守）：**

- **反向命名规则：** UI = "LAN-only Mode = ON" ⇔ 后端 `network.allow_relay_fallback = false` ⇔ iroh `disable_relays = true`。**只允许在 `uc-bootstrap` 唯一一个 helper 函数里取反**，前后端 IPC 永远以 `allow_relay_fallback` 流动。
- **不做运行时热切换：** iroh `RelayMode` 是 endpoint bind 时常量，本里程碑承担"重启生效"语义。
- **不联动遥测：** OTLP 由 `general.telemetry_enabled` 独立控制，禁止与 LAN-only 联动。
- **边界透明：** UI tooltip / `docs/lan-only.md` / changelog 三处必须一致披露仍走外网的 4 类请求（rendezvous / OTLP / pkarr DHT / auto-update），i18n 禁止使用 "fully offline / 完全离线 / 绝对私有" 等绝对化用词。

---

## v0.7.0 Requirements

### NETSET — 网络设置开关与字段（6 条）

- [ ] **NETSET-01**：用户可以在持久化的设备 settings 中通过 `network.allow_relay_fallback: bool` 字段控制是否允许公网中继回落，新装/未配置设备默认 `true`（沿用现状，不打扰存量用户）
- [ ] **NETSET-02**：用户/客户端可以通过 daemon HTTP `GET/PUT /settings` 读写 `network.allow_relay_fallback` 字段；老 settings.json 缺失 `network` 段时反序列化必须回填默认值 `true`（向前兼容）
- [ ] **NETSET-03**：用户在 settings 中关闭 "允许中继回落" 后重启 daemon，启动路径会把字段值通过唯一的取反 helper 注入 `IrohNodeConfig.disable_relays`，使 iroh endpoint 以 `RelayMode::Disabled` 模式 bind，且 `Endpoint::addr().addrs` 中不含 `TransportAddr::Relay` 项
- [ ] **NETSET-04**：用户在 Settings → Network 分类下能看到 "LAN-only Mode" 开关，默认 OFF（关闭=允许 fallback），切换不立即生效
- [ ] **NETSET-05**：用户切换 LAN-only 开关后，UI 显示持久化的 inline "重启生效" 通知（不是一秒 toast），且开关呈现三态视觉（applied OFF / applied ON / pending change），通知内含"立即重启"按钮可触发 daemon 优雅 shutdown + relaunch
- [ ] **NETSET-06**：用户在 LAN-only 开关附近能看到 info icon / tooltip，明确披露开启后**仍会走外网**的 4 类请求（首次配对 rendezvous、OTLP 遥测、pkarr DHT NodeId 解析、auto-update GitHub 检查）

### INDIC — 连接通道指示器（4 条）

- [ ] **INDIC-01**：用户在设备列表中可以看到每台已配对设备的"连接通道"徽章，至少 4 态：`LAN / Relay / Offline / Unknown`；通道值来自 infra 层 `ConnectionChannelPort` 单点产出（禁止 application 层基于 IP 段推断），通过事件流 + 5–15s polling 双路径刷新
- [ ] **INDIC-02**：用户 hover 在通道徽章上能看到 tooltip 解释当前通道含义（特别是 "Relay = 加密中继，元数据可见"，给用户为什么开 LAN-only 的就近论证）
- [ ] **INDIC-03**：用户在开启 LAN-only Mode 后，跨网段无法直连的远端设备在设备列表中显示为灰色 "Out of LAN" 态 + tooltip 说明（避免用户看到设备静默失联而困惑）
- [ ] **INDIC-04**：用户从 system tray icon 上能直接看到当前 LAN-only Mode 是否启用（通过 tray icon 上的状态徽章/差异图标），不需打开主窗口确认

### ONBORD — 配对引导（1 条）

- [ ] **ONBORD-01**：用户首次完成设备配对后，看到一次性 inline banner（非 modal），简述 LAN-only Mode 的存在与代价（开启会让跨网段设备失联），含"了解更多"跳转 Settings 链接；banner 可永久 dismiss，dismiss 状态持久化到 settings

### DOC — 文档与边界披露（3 条）

- [ ] **DOC-01**：用户在仓库 `docs/lan-only.md` 能读到完整的 LAN-only 边界文档：详细说明仍走外网的 4 类请求（rendezvous 时机/频次、OTLP 行为、pkarr DHT、auto-update），并明确 LAN-only 不会做的事（首次配对仍需联网、不自托管 rendezvous、不关闭遥测、不影响 auto-update）
- [ ] **DOC-02**：维护者/贡献者在 `docs/terminology.md` 能查到 LAN-only 相关推荐用语 vs 禁止用语清单（禁止 "fully offline / 完全离线 / 绝对私有" 等绝对化措辞），用于 PR review 与文档校对
- [ ] **DOC-03**：用户在 v0.7.0 changelog / Release notes 中能看到清晰的 LAN-only Mode 范围说明，包含开关行为、连接通道徽章、边界限制三块；措辞与 `docs/lan-only.md` 和 UI tooltip 一致

---

## Future Requirements（v0.7.x 或更晚）

- **D3 实时连接诊断面板** —— Settings 中加可展开的"诊断"折叠块，实时显示当前 endpoint 状态、远端设备路径、握手耗时；推到 v0.7.x，看用户反馈再做
- **D4 OTLP `connection_path` 标签** —— 给已发出的 sync span 打上 `connection_path = lan|relay` 标签，便于在 OTLP 后端做长尾分析；v0.7.x
- **D5 QR 码 / 离线配对** —— 真正的"无外网首次配对"需要侧信道（QR + 蓝牙 + NFC），是独立子项目；v0.8+
- **D6 "测试 LAN-only" 按钮** —— Settings 加一键"诊断我的 LAN-only 是否真的生效"，包含一次主动 endpoint state 抓取 + 流量检测；v0.8+
- **运行时热切换 LAN-only** —— 不重启即生效，需要重建 endpoint + 重挂 ALPN handler，工程量 + 风险大；待真有需求再考虑
- **跨网段静态地址簿 / 手动 NodeId 输入** —— 边缘场景，先看 v0.7.0 LAN-only 是否覆盖大多数诉求

---

## Out of Scope（v0.7.0 显式排除，理由记录）

- **自托管 rendezvous** —— rendezvous 服务暂未开源，需要先给出部署文档/容器化才能让用户暴露；attachment 中用户已确认"暂时不可自建"
- **运行时热切换 LAN-only Mode** —— iroh `RelayMode` 是 bind 时常量，热切换需重建 endpoint + 重挂 ALPN handler + 处理活跃 transfer，本里程碑用"重启生效"提示替代
- **独立 LAN-only 二进制 flavor（uniclip-lan）** —— 双 binary 维护成本高；先做开关，flavor 看后续真实需求
- **完全无联网首次配对** —— 接受首次需联网经 rendezvous；侧信道配对（QR+蓝牙+NFC）是独立子项目
- **自动检测 LAN/Relay 而省略开关** —— Syncthing 论坛已证实 auto detection 在 NAT/IP 缓存边界容易误判
- **基于 IP 段的白名单/黑名单** —— 家用 DHCP 易把自己锁出，NodeId 才是更精确的身份层
- **关闭 pkarr DHT 的 "Strict mode"** —— 关掉后跨网段连接率会从 ~90% 跌到接近 0；pkarr 性质类似 DNS，不算 relay
- **自定义 rendezvous URL 输入框** —— 与"自托管 rendezvous" 一致，先有部署文档再考虑用户暴露
- **LAN-only 联动遥测开关** —— OTLP 遥测由 `general.telemetry_enabled` 独立控制，本里程碑**禁止**联动；如有需求请由用户在 General 分类显式关闭遥测

---

## Traceability（roadmapper 填写）

每条 v0.7.0 requirement 必须映射到唯一的 phase。下表由 `gsd-roadmapper` 在创建 ROADMAP.md 时回填。

| REQ-ID | Phase # | Phase Name | Notes |
|--------|---------|------------|-------|
| NETSET-01 | TBD | TBD | |
| NETSET-02 | TBD | TBD | |
| NETSET-03 | TBD | TBD | |
| NETSET-04 | TBD | TBD | |
| NETSET-05 | TBD | TBD | |
| NETSET-06 | TBD | TBD | |
| INDIC-01 | TBD | TBD | |
| INDIC-02 | TBD | TBD | |
| INDIC-03 | TBD | TBD | |
| INDIC-04 | TBD | TBD | |
| ONBORD-01 | TBD | TBD | |
| DOC-01 | TBD | TBD | |
| DOC-02 | TBD | TBD | |
| DOC-03 | TBD | TBD | |

---

*Last updated: 2026-05-04 — initial v0.7.0 requirements drafted from explore decisions + 4-way research synthesis*
