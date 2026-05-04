# Roadmap: UniClipboard Desktop

## 当前里程碑：v0.7.0 LAN-only Mode

**目标：** 给"局域网洁癖"用户一个可观察、可控的开关 —— 禁用 iroh 公网中继回落，让流量真正只走局域网，并把"当前是直连还是中继"暴露成可见状态。

**Requirements：** 14 条 → `.planning/REQUIREMENTS.md`
**研究：** `.planning/research/SUMMARY.md`
**Phase 编号：** 94–97（沿用 v0.5.0 末尾，不重置）
**起草日期：** 2026-05-04
**Granularity：** fine

**核心约束（所有 phase 必须遵守）：**

- **反向命名规则：** UI = "LAN-only Mode = ON" ⇔ 后端 `network.allow_relay_fallback = false` ⇔ iroh `disable_relays = true`。**只允许在 `uc-bootstrap` 唯一一个 helper 函数 `relay_policy_to_iroh_config()` 里取反**；前后端 IPC 永远以 `allow_relay_fallback` 流动，不允许 `lan_only` 镜像穿过 IPC 边界。
- **不做运行时热切换：** iroh `RelayMode` 是 endpoint bind 时常量，本里程碑承担"重启生效"语义。
- **不联动遥测：** OTLP 由 `general.telemetry_enabled` 独立控制，禁止与 LAN-only 联动。
- **边界透明：** UI tooltip / `docs/lan-only.md` / changelog 三处必须一致披露仍走外网的 4 类请求（rendezvous / OTLP / pkarr DHT / auto-update）；i18n 禁止使用 "fully offline / 完全离线 / 绝对私有" 等绝对化用词。

---

## Phases 概览

- [ ] **Phase 94: 后端字段落地** — `uc-core::Settings.network` 新增、HTTP DTO + view/patch 镜像、`uc-bootstrap` 唯一取反 helper 注入 `IrohNodeConfig.disable_relays`
- [ ] **Phase 95: 前端 NetworkSection + 重启 UX** — 替换占位组件为真实开关、三态视觉、持久 inline 重启通知 + "立即重启"按钮、tooltip 披露 4 类外网请求
- [ ] **Phase 96: 连接通道指示器** — 新增 `ConnectionChannelPort` + `IrohConnectionChannelAdapter`、`PeerSnapshotDto.channel`、设备列表徽章、tray icon 状态徽章、"Out of LAN" 灰态
- [ ] **Phase 97: onboarding + 文档 + 跨平台 QA gate** — 配对成功 inline banner + dismiss 持久化、`docs/lan-only.md` / `docs/terminology.md` / changelog、跨平台 release-gate QA 矩阵

### 概览表

| #  | Phase 名                         | 目标（一句话）                                                    | Requirements                                | 依赖           | 标准条数 |
|----|----------------------------------|-------------------------------------------------------------------|---------------------------------------------|----------------|----------|
| 94 | 后端字段落地                     | 用户/客户端可以读写 `network.allow_relay_fallback` 并影响 iroh bind 行为 | NETSET-01, NETSET-02, NETSET-03             | 无             | 4        |
| 95 | 前端 NetworkSection + 重启 UX    | 用户可以在 Settings 切换 LAN-only Mode 并通过重启使其生效         | NETSET-04, NETSET-05, NETSET-06             | Phase 94       | 4        |
| 96 | 连接通道指示器                   | 用户可以肉眼验证设备走的是 LAN 直连还是 Relay 中继还是离线        | INDIC-01, INDIC-02, INDIC-03, INDIC-04      | Phase 94       | 5        |
| 97 | onboarding + 文档 + 跨平台 QA    | 用户/维护者获得一致透明的 LAN-only 边界披露与跨平台已验证 release | ONBORD-01, DOC-01, DOC-02, DOC-03           | Phase 95, 96   | 5        |

**覆盖：** 14/14 requirements 映射到唯一 phase ✓

---

## Phase 详细

### Phase 94: 后端字段落地

**Goal：** 用户/客户端可以通过持久化 settings 与 daemon HTTP `/settings` 读写 `network.allow_relay_fallback` 字段；重启 daemon 后字段值通过唯一取反 helper 注入 iroh endpoint bind，使 LAN-only 真生效。

**Requirements covered：** NETSET-01, NETSET-02, NETSET-03

**Dependencies：** 无（依赖图源头；schema 不定后续都飘）

**Success criteria（可观察的用户行为或可验证状态）：**

1. 在 `~/Library/Application Support/.../settings.json` 中手工添加 `"network": {"allow_relay_fallback": false}` 后重启 daemon，启动日志可见 `disable_relays = true` 且 iroh endpoint 以 `RelayMode::Disabled` bind（`Endpoint::addr().addrs` 不含 `TransportAddr::Relay` 项）；反之 `allow_relay_fallback: true` 或缺 `network` 段时 endpoint 仍可观察到 Relay 候选。
2. 通过 daemon HTTP `PUT /settings` 提交带 `network.allow_relay_fallback` 的 patch，写盘成功后再 `GET /settings` 返回字段值与提交一致；不带 `network` 段的旧客户端 PUT 仍 200，且不抹掉已存在的 `network` 字段。
3. 老 settings.json（缺 `network` 段）反序列化后字段断言 `== true`（手写 `impl Default { allow_relay_fallback: true }`，`#[serde(default)]` 覆盖向后兼容）；`schema_version` 数值不变（不触发 migration codepath）。
4. `uc-bootstrap::relay_policy_to_iroh_config()` truth-table 单测覆盖 `(allow=true → disable=false)` 与 `(allow=false → disable=true)` 两组；全工程 grep `disable_relays` / `allow_relay_fallback` 仅有这一处取反点（其它位置以原语义流动）。

**Key risks / pitfall 防御：**
- Pitfall 1（反向命名搞反方向）—— 强制集中转换点 + truth-table 单测 + DTO 字段名 `allow_relay_fallback` 不许重命名
- Pitfall 2（默认值倒置）—— 禁止 `#[derive(Default)]`，手写 `impl Default` 带 3 行警示注释 + 老 JSON 反序列化测试
- Pitfall 3（运行时热切换诱惑）—— `UpdateNetworkSettings` 入口禁止"立即生效"，必须返回 `restart_required: bool`；`IrohNodeBuilder::bind` 用 `OnceCell` 强制只能跑一次
- Pitfall 6（OTLP 联动）—— `network` 模块不允许引用 `general.telemetry_enabled`，PR review 必查
- Pitfall 8（测试覆盖）—— Tier A 单测 + Tier B 集成测试 `uc-infra/tests/lan_only_relay_mode.rs` 验证 bind 行为

**Estimated scope：** M

**Plans：** 6 plans

Plans:
- [ ] 094.01-PLAN.md — uc-core Settings.network 字段落地 + 手写 Default + 向后兼容反序列化测试
- [ ] 094.02-PLAN.md — uc-application View/Patch 镜像 + apply_settings_patch + facade pub use 白名单
- [ ] 094.03-PLAN.md — uc-daemon-contract DTO 镜像 + UpdateSettingsResponse.restart_required + OpenAPI schema
- [ ] 094.04-PLAN.md — uc-webserver DTO ↔ View 双向 mapping + handler 内联 restart_required + integration smoke
- [ ] 094.05-PLAN.md — uc-bootstrap network_policy 唯一取反 helper + 两处 bind 装配点改造 + tracing::info! 启动日志
- [ ] 094.06-PLAN.md — uc-infra IrohNodeBuilder::bind OnceCell 守护 + lan_only_relay_mode integration test

---

### Phase 95: 前端 NetworkSection + 重启 UX

**Goal：** 用户在 Settings → Network 分类下可以看到并切换 "LAN-only Mode" 开关，切换后看到持久化的 inline 重启通知与三态视觉，可点击"立即重启"触发 daemon 优雅 shutdown + relaunch；tooltip 披露开启后仍走外网的 4 类请求。

**Requirements covered：** NETSET-04, NETSET-05, NETSET-06

**Dependencies：** Phase 94（前端没法 PUT 不存在的字段；`Settings` interface 必须先有 `network.allowRelayFallback`）

**Success criteria（可观察的用户行为或可验证状态）：**

1. 打开 Settings → Network 分类可见 "LAN-only Mode" 开关，**默认 OFF**（关闭 = 允许 fallback，对应后端 `allow_relay_fallback: true`）；之前的 `'settings.sections.network.placeholder'` 占位 i18n key 与 `NetworkSection.tsx` 占位组件已被替换，无残留。
2. 切换开关后：UI 立即显示**持久 inline 通知**（非一秒 toast）"重启生效"，并切换到 "pending change" 三态视觉（区别于 applied OFF / applied ON 两个稳定态）；通知含"立即重启"按钮，点击后 daemon 走优雅 shutdown + relaunch 路径，重启完成后 UI 回到稳定态且新值已生效。
3. 开关附近显示 info icon，hover/点击展开 tooltip 列出开启后**仍会走外网**的 4 类请求：(a) 首次配对 rendezvous、(b) OTLP 遥测（独立由 General 控制）、(c) pkarr DHT NodeId 解析、(d) auto-update GitHub 检查；措辞与 `docs/lan-only.md` 完全一致，不含 "fully offline / 完全离线 / 绝对私有" 等绝对化用词。
4. 前端 store 内部状态名为 `allowRelayFallback`（驼峰），**不**维护 `lanOnly` 镜像；UI 组件用 `checked={!setting.network.allowRelayFallback}` 决定开关视觉；写入 debounce ≥ 500ms 防止反复切换爆 disk I/O。

**Key risks / pitfall 防御：**
- Pitfall 5（"LAN-only" 营销语 vs 边界透明）—— tooltip / `docs/lan-only.md` / changelog 三处一致；i18n 禁用绝对化词
- Pitfall 10（重启 UX 半生效）—— 持久 inline 通知（不是 toast）+ 三态视觉 + "立即重启"按钮 + debounce 写入
- Pitfall 11（占位组件残留）—— PR diff 必须删除 `'settings.sections.network.placeholder'` 与占位 JSX

**Estimated scope：** M

---

### Phase 96: 连接通道指示器

**Goal：** 用户在设备列表与 system tray 都能直观看到当前 LAN-only Mode 状态以及每台已配对设备走的是 LAN 直连 / Relay 中继 / Offline / Unknown / Out of LAN，从而**可肉眼验证**开关效果；通道判定来自 infra 层单一真相源，前后通过事件 + polling 双路径刷新。

**Requirements covered：** INDIC-01, INDIC-02, INDIC-03, INDIC-04

**Dependencies：** Phase 94（依赖 `network.allow_relay_fallback` 字段已落地，"Out of LAN" 灰态判定需要读它；与 Phase 95 技术解耦可并行）

**Success criteria（可观察的用户行为或可验证状态）：**

1. 设备列表中每台已配对设备显示连接通道徽章，至少 4 态可见：`LAN / Relay / Offline / Unknown`；徽章值来自 infra 层 `ConnectionChannelPort::channel_for(device)` 单点产出（grep application 层无 `if peer.ip.starts_with("192.168")` 之类 IP 段推断），UI 同时订阅既有 `peers.changed` 事件流 + 5–15s polling 双路径刷新；`Unknown` 态在 UI 显式可见，**不会**被默认渲染为 LAN/Relay。
2. hover 通道徽章可见 tooltip 解释当前通道含义，特别针对 Relay 显示 "加密中继，元数据可见" 之类就近论证（内容与 `docs/lan-only.md` 一致）。
3. 在 LAN-only Mode = ON 状态下断网或跨网段设备仍在 paired 列表，但显示为灰色 "Out of LAN" 态 + tooltip 说明（不是静默失联）；恢复同网段后徽态自动回到 LAN（事件或下一轮 polling 内）。
4. system tray icon 上可视化当前 LAN-only Mode 启用状态（差异图标 / 状态徽章），用户不打开主窗口也能确认；切换 + 重启后 tray icon 状态在 daemon 起来后随之更新。
5. `PeerSnapshotDto.channel: String` 取值范围严格限定 `"direct" | "relay" | "offline" | "unknown"`；DTO ↔ view 映射有双向单测；新增 `IrohConnectionChannelAdapter` 经 `endpoint.remote_info → 过滤 Active TransportAddrInfo → Ip⇒Direct / Relay⇒Relay / 空⇒Unknown` 推导，IPv6 ULA filter（`fc00::/7` + `fe80::/10`）顺手覆盖。

**Key risks / pitfall 防御：**
- Pitfall 4（通道指示器与真实状态偏差）—— 单一真相源 + Unknown 态显式 + 事件 + polling 双兜底；禁 IP 段推断
- Pitfall 7（IPv6 ULA / 跨平台边界）—— filter 顺手补，跨平台 QA 矩阵在 Phase 97 收尾验证

**Estimated scope：** L

---

### Phase 97: onboarding + 文档 + 跨平台 QA gate

**Goal：** 首次配对完成的用户能看到一次性 inline banner 发现 LAN-only Mode；维护者/贡献者 / reviewer / Release notes 读者从 `docs/lan-only.md` / `docs/terminology.md` / changelog 三处获得**完全一致**的边界披露；release 不发布除非跨平台 QA 矩阵全部通过。

**Requirements covered：** ONBORD-01, DOC-01, DOC-02, DOC-03

**Dependencies：** Phase 95（onboarding tip "了解更多"链接跳转 NetworkSection，要求 95 完成）+ Phase 96（changelog 必须完整描述徽章三态行为；QA 矩阵涉及通道指示器跨平台验证）

**Success criteria（可观察的用户行为或可验证状态）：**

1. 首次完成设备配对（pairing wizard 走完 "Done"）后，主界面顶部显示一次性 inline banner（**非 modal**），文案简述 LAN-only Mode 与"开启会让跨网段设备失联"边界，含"了解更多"按钮跳转 Settings → Network；banner 含 dismiss 按钮，dismiss 状态写入 settings `dismissed_tips: HashSet<String>`，再次启动不再显示；该 banner 不会自动开启 LAN-only。
2. `docs/lan-only.md` 存在，至少包含三段：(a) **LAN-only 会做什么**（disable iroh relay fallback、对应字段、重启生效语义）、(b) **LAN-only 不会做什么 / 仍走外网的 4 类请求**（rendezvous 时机与频次、OTLP 行为及如何关、pkarr DHT NodeId 解析、auto-update GitHub 检查）、(c) **明确不在范围内**（自托管 rendezvous、运行时热切换、跨网段静态地址簿、独立 LAN-only flavor、关闭遥测）；与 UI tooltip 措辞逐字一致。
3. `docs/terminology.md` 存在并提供 LAN-only 推荐用语 vs 禁止用语清单；明确禁止 "fully offline / 完全离线 / no internet / private mode / 绝对私有 / encrypted-and-local" 等绝对化措辞；reviewer checklist 在 PR 模板中引用本文档作为强制 gate。
4. v0.7.0 changelog / Release notes 包含一段 LAN-only Mode 章节，覆盖三块：开关行为（默认 OFF、重启生效、不联动遥测、不热切换）、连接通道徽章（4 态 + Out of LAN 灰态 + tray icon 徽章）、边界限制（4 类外网请求清单）；措辞与 `docs/lan-only.md` 与 UI tooltip 完全一致。
5. 跨平台 release-gate QA 矩阵全绿（**release blocker，非独立 phase 的子任务**）：macOS / Windows / Linux × 同 Wi-Fi 同子网 / 同 Wi-Fi 不同 VLAN / VPN 在线 / 企业 AP isolation 共 12 组合的开关切换 + 通道徽态 + tray icon + 重启 UX 验证；至少 1 组合执行 Tier C 抓包验证（开 LAN-only 后 Wireshark / tcpdump 确认无指向 `*.iroh.network` / `*.n0.computer` 流量）。

**Key risks / pitfall 防御：**
- Pitfall 5（边界文档 / 营销语 vs 现实）—— 4 类外网请求 4 surface（UI tooltip / `docs/lan-only.md` / changelog / banner 边界提示）逐字一致
- Pitfall 9（措辞一致性）—— `docs/terminology.md` + reviewer checklist + PR 模板强制 gate
- Pitfall 12（onboarding tip 时机 / 文案）—— 时机锁定 wizard "Done" 之后；文案带边界提示；dismiss 持久化
- Pitfall 7（跨平台 QA）—— 12 组合矩阵 + Tier C 抓包

**Estimated scope：** M

---

## 进度表

| Phase | Plans Complete | Status      | Completed |
|-------|----------------|-------------|-----------|
| 94. 后端字段落地                  | 0/6 | Not started | - |
| 95. 前端 NetworkSection + 重启 UX | 0/1 | Not started | - |
| 96. 连接通道指示器                | 0/1 | Not started | - |
| 97. onboarding + 文档 + 跨平台 QA | 0/1 | Not started | - |

---

## Milestones

- ✅ **v0.1.0 Daily Driver** — shipped 2026-03-06
- ✅ **v0.2.0 Architecture Remediation** — shipped 2026-03-09
- ✅ **v0.3.0 Log Observability & Feature Expansion** — shipped 2026-03-17
- ✅ **v0.4.0 Runtime Mode Separation** — archived 2026-04-09 with known gaps accepted
- ✅ **v0.5.0 Local Encrypted Search** — archived 2026-04-13
- 🚧 **v0.7.0 LAN-only Mode** — started 2026-05-04（当前里程碑）

## Archived Milestones

<details>
<summary>✅ v0.1.0 Daily Driver</summary>

See:

- `.planning/milestones/v0.1.0-ROADMAP.md`
- `.planning/milestones/v0.1.0-REQUIREMENTS.md`
- `.planning/milestones/v0.1-MILESTONE-AUDIT.md`

</details>

<details>
<summary>✅ v0.2.0 Architecture Remediation</summary>

See:

- `.planning/milestones/v0.2.0-ROADMAP.md`
- `.planning/milestones/v0.2.0-REQUIREMENTS.md`
- `.planning/milestones/v0.2.0-MILESTONE-AUDIT.md`

</details>

<details>
<summary>✅ v0.3.0 Log Observability & Feature Expansion</summary>

See:

- `.planning/milestones/v0.3.0-ROADMAP.md`
- `.planning/milestones/v0.3.0-REQUIREMENTS.md`
- `.planning/milestones/v0.3.0-MILESTONE-AUDIT.md`

</details>

<details>
<summary>✅ v0.4.0 Runtime Mode Separation</summary>

See:

- `.planning/milestones/v0.4.0-ROADMAP.md`
- `.planning/milestones/v0.4.0-REQUIREMENTS.md`
- `.planning/milestones/v0.4.0-MILESTONE-AUDIT.md`
- `.planning/milestones/v0.4.0-phases/`

Archive note:

- Archived on 2026-04-09
- Archived with known gaps accepted
- Main remaining gaps at archive time:
  - planning files and requirement bookkeeping still needed cleanup
  - GUI-launched daemon still did not inherit OTLP endpoint automatically
  - some verification files were still missing or stale

</details>

<details>
<summary>✅ v0.5.0 Local Encrypted Search</summary>

See:

- `.planning/milestones/v0.5.0-ROADMAP.md`
- `.planning/milestones/v0.5.0-REQUIREMENTS.md`
- `.planning/milestones/v0.5.0-MILESTONE-AUDIT.md` (backfilled 2026-05-04)

Archive note:

- Archived on 2026-04-13
- Phase 93 was completed manually and backfilled during milestone archive
- Audit file backfilled on 2026-05-04 — passed (23/23 requirements, planning gaps accepted as discarded debt)

</details>

## Next Step

当前里程碑 v0.7.0 处于规划阶段。下一步：`/gsd-plan-phase 94` 起草 Phase 94 计划。
