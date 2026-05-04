# Phase 95: 前端 NetworkSection + 重启 UX - Context

**Gathered:** 2026-05-04
**Status:** Ready for planning

<domain>
## Phase Boundary

用户在 Settings → Network 分类下能看到并切换 "LAN-only Mode" 开关；切换后立即看到**持久 inline 重启通知**（专用 RestartBanner 组件，贴在 Switch 上方），含「立即重启」按钮触发 `app.restart()`（Tauri 整个 GUI 进程退出 + relaunch，daemon 子系统随之重启）；info icon 旁 Popover 披露开启后仍走外网的 4 类请求；前端 store 内部状态名 `allowRelayFallback`（驼峰），`NetworkSection.tsx` 占位组件 + `settings.sections.network.placeholder` i18n key 已替换、无残留。

**交付范围：**
- TypeScript Settings 类型 + daemon API 客户端补 `network.allowRelayFallback`（与 Phase 94 wire 对齐）
- `NetworkSection.tsx` 占位组件实质化 + 删除占位 i18n key
- 专用 `RestartBanner` 组件 + Section 内部嵌入
- info icon → Popover（点击展开），含 4 类外网请求清单文案
- pending 跨 session 识别：Tauri command `get_restart_state` 暴露 `process_started_at` + `settings_mtime`
- 「立即重启」按钮：Tauri command 调 `app.restart()`，复用 updater.rs:301 同模式
- debounce ≥ 500ms 写入（用 `useDebounce` hook）
- i18n 文案（zh-CN + en-US）含 4 类外网请求清单 + Banner 文案

**不在本 phase 范围：**
- daemon-only CLI 模式（`uniclip daemon`）的「立即重启」UX —— 整里程碑显式排除（用户自己 systemctl/launchd），daemon 不暴露 admin/restart HTTP
- system tray icon LAN-only 状态 —— Phase 96
- 设备列表「连接通道」徽章 —— Phase 96
- onboarding tip + `docs/lan-only.md` 文档化 + changelog —— Phase 97
- 旧 i18n 残留 `network.syncMethod / webserverPort / customPeerDevice / cloudServer` 块清理 —— Claude Discretion，planner 顺手做或延后

</domain>

<decisions>
## Implementation Decisions

### A. 重启 UX 形态

- **D-A1：** 持久 inline 重启通知用**专用 RestartBanner 组件**（`src/components/setting/RestartBanner.tsx`）。
  - 不复用 shadcn `Alert`：RestartBanner 需独立样式控制（pending 强调色、"立即重启" 主按钮、loading 状态）；后续 Phase 96/97 若需重启提示可复用本组件。
  - 不用 sonner toast：即便 `duration:Infinity` 在用户心智里仍是"右上角浮动短提示"，违反 ROADMAP「持久 inline」语义。
- **D-A2：** RestartBanner 贴在 **NetworkSection 内部、LAN-only Switch SettingRow 上方**（不在 Settings 页全局顶部，不在 App layout 全局）。
  - 用户切到其它 Settings 分类时 Banner 不可见 —— 接受这一代价；v0.7.0 的全局可见性由 Phase 96 system tray icon 状态徽章兜底。
  - 不动 Settings 页 layout，Phase 95 改动局限在 NetworkSection 文件树。
- **D-A3：** 三态视觉**只靠 RestartBanner 出现/消失**表达，Switch 本身不动样式。
  - applied OFF: Switch=OFF + 无 Banner
  - applied ON: Switch=ON + 无 Banner
  - pending change: Switch=用户的选择 + Banner 出现
  - **不**在 Switch 右侧加 "Pending" Badge —— Banner 已表达完整 pending 信号；额外 Badge 增加 SettingRow 视觉密度且与 Phase 96 通道徽章混淆。

### B. 「立即重启」工程范围 + 实现

- **D-B1：** Phase 95 **只 cover Tauri GUI 模式**。`uniclip daemon` CLI 模式不在 Phase 95 范围。
  - 物理意义：GUI 模式下 daemon 是 in-process（`uc-desktop/src/daemon/handle.rs::start_in_process`），所以"daemon graceful shutdown + relaunch" ≡ Tauri **整个 GUI 进程**退出 + 重新拉起。daemon 子系统随 Tauri 进程一起重启 → 新进程读 settings.json → `IrohNodeBuilder::bind` 用新 `disable_relays` 值（OnceCell 守护在新进程下重置）。
  - 这是唯一干净路径：iroh `RelayMode` 是 endpoint bind 时常量；Phase 94 plan 06 已用 `OnceCell`（`#[cfg(not(test))]`）阻断进程内二次 bind（Pitfall 3 防御）。
- **D-B2：** Tauri command 复用 `app.restart()`（与 `uc-tauri/src/commands/updater.rs:301` 同一调用模式）。
  - 新增 Tauri command（命名由 planner 定，建议 `restart_app` 或 `restart_for_settings`，含 trace metadata 走 `record_trace_fields` helper）。
  - 调用前**不**显式做 daemon graceful shutdown：Tauri 进程退出会触发 task cancel cascade（`task_registry.rs::shutdown`），daemon 子系统随之 graceful 关闭 —— 复用现有生命周期治理。
- **D-B3：** 重启失败兜底：`app.restart()` 失败时显示 inline error（在 RestartBanner 内 inline 渲染错误信息 + 提示用户手动重启），不抛 toast。

### C. 4 类外网请求披露形态

- **D-C1：** info icon 用 **Popover（点击展开）**，不是 hover Tooltip。
  - 4 条 + 描述（首次配对 rendezvous / OTLP 遥测 / pkarr DHT NodeId 解析 / auto-update GitHub 检查）单条 ~30 中文字 + 标题，hover Tooltip 容纳量不够、不可选不可复制。
  - 用 `src/components/ui/popover.tsx`（已有）。
  - info icon 选 lucide-react `Info` 图标（与 settings-config.ts 的 about 分类一致）—— 由 planner 最终敲定。
- **D-C2：** Phase 95 文案 **必须最终敲定**，因为 Phase 97 `docs/lan-only.md`（DOC-01）/ changelog（DOC-03）反向以 Phase 95 文案为基准 —— ROADMAP 验收 #3 要求"措辞与 docs/lan-only.md 完全一致"。Phase 95 planner 应基于 PROJECT.md "关键决策" + REQUIREMENTS.md NETSET-06 + Future Phase 边界 草拟最终文案；Phase 97 复制粘贴 + 扩写。
  - i18n key 命名建议：`settings.sections.network.lanOnly.disclosure.{rendezvous,otlp,pkarr,autoUpdate}.{title,description}`。
  - **禁词清单**（与 Phase 97 DOC-02 `docs/terminology.md` 对齐）：禁用 "fully offline / 完全离线 / 绝对私有 / no internet / private mode / encrypted-and-local"。

### D. pending 跨 session 持久性

- **D-D1：** pending 识别走 **Tauri command 路径**（不动 Phase 94 已锁 daemon HTTP 契约）。
  - 新增 Tauri command（命名建议 `get_restart_state`），返回 `{ process_started_at: i64, settings_mtime: i64 }`：
    - `process_started_at`: Tauri `OnceCell<SystemTime>` 在 `uc-tauri::run` 启动早期赋值（millis since epoch）。
    - `settings_mtime`: `std::fs::metadata(settings_path).modified()` 实时读 millis since epoch；settings_path 通过 `TauriAppRuntime` 暴露的 settings dir helper 拿（沿用现有 path helper，由 planner 找）。
  - 前端 NetworkSection mount 时调一次 `get_restart_state`：若 `settings_mtime > process_started_at` 且 `settings.network.allowRelayFallback ≠ daemon 当前 bind 值` ⇒ 显示 RestartBanner。
  - **简化**：Phase 95 不需要"daemon 当前 bind 值"反查 —— 只要 `settings_mtime > process_started_at`（说明本进程启动后 settings.json 改过）即可推断 pending。bind 值反查留给 Phase 96「连接通道指示器」（INDIC-01 那边的 `ConnectionChannelPort` 已经会反映真实 bind 状态）。
- **D-D2：** 切换开关后**乐观 pending**：Switch onCheckedChange 立即把 in-memory `setting.network.allowRelayFallback` 改掉 + Banner 显示，不等 PUT 返回；PUT 完成后再决定是否要刷新 banner 状态（如果 PUT 返回 `restart_required: true` 且 mtime 已更新，banner 维持）。
- **D-D3：** debounce 用现有 `useDebounce(value, 500)` hook（`src/hooks/useDebounce.ts`）。语义：用户连击切换时，UI 即时反映最后一次状态 + Banner 即时出现，但 PUT /settings 仅在停止切换 500ms 后发一次（最后值）。**注意**：Banner 显示和 PUT 写盘解耦 —— Banner 在用户切的瞬间出现，PUT 落盘是后续事件。

### Claude's Discretion

- **RestartBanner 视觉细节** —— 配色（warn/info）、图标（lucide-react `RefreshCw` / `AlertCircle`）、按钮 variant、内边距 —— 由 planner / UI design 决策。但语义角色必须传达"等待重启"（不是 error，不是 success）。
- **Tauri command 命名** —— `restart_app` vs `request_restart` vs `restart_for_settings`；`get_restart_state` vs `query_restart_state` —— planner 决策，沿用现有 commands/ 命名风格。
- **info icon 选哪一个 lucide-react 图标** —— `Info` / `HelpCircle` / `CircleHelp` —— planner 决策，与现有 settings 风格一致即可。
- **i18n key 命名层级** —— 上面 D-C2 给了建议但非锁定；planner 可根据 zh-CN.json 现有结构调整。
- **Popover 触发器是 button 还是 icon** —— 无障碍上 button 更优（aria-haspopup），但视觉上 icon-only 更紧凑 —— planner 决策。
- **旧 i18n 残留清理范围** —— `network.syncMethod / webserverPort / customPeerDevice / cloudServer` 几块都是过期实现的残留 —— 是只清 ROADMAP 验收 #1 提到的 `placeholder` key 还是顺手清整个块 —— planner 决策。建议顺手清以避免未来误用，但若 grep 发现其它地方仍在引用，按引用情况裁剪。
- **重启失败具体文案 + retry/dismiss 按钮配置** —— 由 planner 决策。
- **PUT /settings 失败回退** —— PUT 失败时 Switch UI 是否回滚到旧值、Banner 怎么处理 —— planner 决策（建议：Switch 回滚 + RestartBanner 不出现 + 显示 inline error）。

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 里程碑 / 需求 / 项目层

- `.planning/ROADMAP.md` §Phase 95 — 4 条 success criteria + 3 条 Pitfall 防御（5 / 10 / 11）
- `.planning/REQUIREMENTS.md` NETSET-04 / NETSET-05 / NETSET-06 — 三条用户级需求 + traceability 注释
- `.planning/PROJECT.md` §Current Milestone v0.7.0 LAN-only Mode + §Out of Scope — 反向命名规则、不联动遥测、不做运行时热切换
- `.planning/research/PITFALLS.md` Pitfall 5 / 10 / 11 — 营销语 vs 边界透明 / 重启 UX 半生效 / 占位组件残留
- `.context/attachments/Summary of Explore LAN version need.md` — 反向命名决策来源（如仍存）

### 上游 Phase 锁定层（必读）

- `.planning/phases/094-backend-network-allow-relay-fallback/094-CONTEXT.md` — Phase 94 决策树（D-A1 取反 helper 位置 / D-D1-3 restart_required 信号契约 / Pitfall 3 OnceCell 守护）
- `.planning/phases/094-backend-network-allow-relay-fallback/094-VERIFICATION.md` — Phase 94 验收，含 wire 契约最终形态
- `.planning/research/STACK.md` §1 + §2 — iroh 0.98 RelayMode bind-time 常量、settings `network` namespace 模式
- `.planning/research/ARCHITECTURE.md` §0 + §1.1–1.4 — 五层落点 + 行号锚定（前端 src/ 在第 5 层）

### 关键代码锚点（前端）

- `src/components/setting/NetworkSection.tsx:1-25` — 当前占位组件（Phase 95 实质化目标）
- `src/components/setting/SyncSection.tsx:1-362` — 最近的 Section 完整实现参考（Switch + 多字段 + i18n + useSetting hook 集成模式）
- `src/components/setting/settings-config.ts:54-58` — `SETTINGS_CATEGORIES` 中 network 分类注册（已在；不动）
- `src/components/setting/SettingGroup.tsx` / `SettingRow.tsx` — Section 容器 + 行布局（必读以保持视觉一致）
- `src/contexts/SettingContext.tsx:46-64, 66-82` — `saveSetting` / `updateGeneralSetting` 等模式（NetworkSection 需要新增 `updateNetworkSetting` 同形 helper + 处理 `restart_required` 响应字段）
- `src/types/setting.ts:124-133` — `Settings` interface（需补 `network: NetworkSettings`）
- `src/api/daemon/settings.ts:120-138` — `Settings` API 类型（需补 `NetworkSettings` + `restart_required`）+ `:209-301` `toSettingsPatchRequest`（需补 `network` 段）
- `src/hooks/useDebounce.ts:1-17` — 默认 500ms，`useDebounce(value, 500)`，复用即可
- `src/i18n/locales/zh-CN.json:50` `categories.network`（保留）+ `:192-238` 旧 `network.*` 块（清理 + 替换）
- `src/i18n/locales/en-US.json` — 同步翻译

### 关键代码锚点（后端 / Tauri 层）

- `src-tauri/crates/uc-daemon-contract/src/api/dto/settings.rs:208-209` `NetworkSettingsDto`（wire 形态：`allowRelayFallback: bool`）
- `src-tauri/crates/uc-daemon-contract/src/api/dto/settings.rs:318-319` `NetworkSettingsPatchDto`
- `src-tauri/crates/uc-webserver/src/api/settings.rs:69-93` `update_settings_handler` —— `UpdateSettingsResponse` 含 `restart_required: bool`，前端 PUT 后必须读这字段
- `src-tauri/crates/uc-tauri/src/commands/updater.rs:300-301` — `app.restart()` 现有调用模式（Phase 95 「立即重启」复用）
- `src-tauri/crates/uc-tauri/src/commands/mod.rs:1-44` — Tauri commands 注册 + `record_trace_fields` helper（Phase 95 新增 commands 注册到此）
- `src-tauri/crates/uc-desktop/src/daemon/handle.rs:1-30` — daemon in-process 启动 / `DaemonHandle::shutdown` 语义（理解"为什么 GUI 模式 = 整进程重启"）
- `src-tauri/crates/uc-bootstrap/src/task_registry.rs:74` — `TaskRegistry::shutdown` cancel cascade（进程退出时 daemon 子系统 graceful 关闭依赖）
- `src-tauri/crates/uc-infra/src/network/iroh/node.rs` `IrohNodeBuilder::bind` — Phase 94 plan 06 加的 `OnceCell` 守护（解释为什么必须重启进程）

### 下游 Phase 反向依赖（Phase 95 必须考虑的边界）

- `.planning/ROADMAP.md` §Phase 96 — system tray icon LAN-only 状态徽章（INDIC-04）依赖 Phase 95 settings 落地，但 Phase 95 不实现 tray 部分
- `.planning/ROADMAP.md` §Phase 97 — `docs/lan-only.md`（DOC-01）/ changelog（DOC-03）/ onboarding banner（ONBORD-01）反向引用 Phase 95 文案 → **Phase 95 文案必须最终敲定，可被 Phase 97 复制粘贴**

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- **`useDebounce(value, 500)`**（`src/hooks/useDebounce.ts`）—— 现成，500ms 默认与 ROADMAP 锁定值一致；直接复用。
- **`Switch` / `Badge` / `Popover` / `Alert` / `Button` / `Tooltip`**（`src/components/ui/`）—— shadcn 完整套件已就位。Phase 95 用 `Switch` + `Popover` + 自写 `RestartBanner`。
- **`SettingGroup` / `SettingRow`**（`src/components/setting/`）—— Section 容器 + 行布局 primitive，所有现有 Section 都用；NetworkSection 跟随同模式。
- **`useSetting` hook + SettingContext**（`src/contexts/SettingContext.tsx`）—— 现有 `updateGeneralSetting` / `updateSyncSetting` / `updateFileSyncSetting` 同形 helper 模式；新增 `updateNetworkSetting` 完全可镜像。
- **`saveSetting` 函数**（`SettingContext.tsx:46-64`）—— 当前实现把 PUT 响应**忽略**了；Phase 95 需扩展使其捕获 `restart_required` 信号 + 暴露给 NetworkSection（建议 saveSetting 改为返回 `Promise<{ restartRequired: boolean }>`，所有调用方继续 ignore 也无妨；NetworkSection 的 updateNetworkSetting 读这个字段）。
- **`updateSettings` API**（`src/api/daemon/settings.ts:201-207`）—— 当前丢弃响应 body；Phase 95 改返回 `{ success, restartRequired }`，向上传递。
- **lucide-react `Wifi` icon** —— `settings-config.ts:9` 已 import，作为 network 分类图标，不动。
- **`app.restart()` 调用模式** —— `uc-tauri/src/commands/updater.rs:301` 是现成参考；Phase 95 新 command 复用同 API。
- **i18n hook + 模式** —— `useTranslation()` + `t('settings.sections.network.xxx')` —— 与现有所有 Section 一致。

### Established Patterns

- **camelCase wire 字段** —— `src/types/setting.ts` + `src/api/daemon/settings.ts` 全部 camelCase，与 daemon-contract `#[serde(rename_all = "camelCase")]` 对齐。`network.allowRelayFallback` 跟随。
- **Section 组件签名** —— 所有 Section 都是 `const FooSection: React.FC = () => {...}` + `export default`，无 props。NetworkSection 跟随。
- **Switch + 即时 PUT 模式** —— `SyncSection.tsx:67-70` `handleAutoSyncChange` 是直接 `updateSyncSetting({ autoSync: checked })` 模式（**没有** debounce）。Phase 95 LAN-only Switch **必须** 加 `useDebounce(switchValue, 500)`，写盘是 debounce 后的值。这与 SyncSection 当前模式不同，是 Phase 95 显式偏离（ROADMAP Pitfall 10 防 disk I/O 爆）。
- **Tauri command + invoke 模式** —— 现有 `invokeWithTrace`（`src/lib/tauri-command.ts`）已是统一入口；新增 commands 走相同链路。
- **错误处理：try/catch + log.error + setState** —— `SettingContext.tsx:36-43, 56-60` 模式，Phase 95 沿用。
- **i18n 文案占位 fallback** —— `t('xxx') || 'fallback text'`（NetworkSection.tsx:18 现有）—— 可保留作为 dev 兜底，但生产 i18n key 必须存在。
- **`'settings.sections.network.placeholder'` 占位 key 当前**只在 NetworkSection.tsx 引用**（grep 已确认）—— 删除占位组件 + 删除 i18n key 同步进行，无外部引用阻塞。

### Integration Points

- **`src/types/setting.ts` Settings interface** —— 加 `network: NetworkSettings`（必填，与后端 `Settings` 一致），`NetworkSettings` 形状 `{ allowRelayFallback: boolean }`。
- **`src/api/daemon/settings.ts`** ——
  1. 加 `NetworkSettings` interface + `Settings.network`（必填）
  2. `SettingsPatchRequest` 加 `network?: Partial<NetworkSettings>`
  3. `toSettingsPatchRequest` 加 `if (settings.network) { patch.network = { allowRelayFallback: settings.network.allowRelayFallback } }`
  4. `updateSettings` 改签名 `Promise<{ success: boolean; restartRequired: boolean }>`，返回 `res.data.success` + `res.data.restart_required`（注意 daemon 用 camelCase serde，wire 上是 `restartRequired`，校验一下 Phase 94 实际 wire 字段名）。
- **`src/contexts/SettingContext.tsx`** ——
  1. `saveSetting` 改签名返回 `{ restartRequired }`
  2. 加 `updateNetworkSetting(newNetworkSetting: Partial<NetworkSettings>)` helper，与 `updateGeneralSetting` 同形
  3. `SettingContextType` 加 `updateNetworkSetting`
- **`src/types/setting.ts` SettingContextType** —— 加 `updateNetworkSetting`。
- **新建 `src/components/setting/RestartBanner.tsx`** —— 接受 props `{ visible: boolean; onRestart: () => Promise<void>; onDismiss?: () => void; loading?: boolean; error?: string | null }`（具体签名由 planner 定）。
- **新建 `src/components/setting/NetworkLanOnlyDisclosure.tsx`**（命名由 planner 定）—— Popover trigger + content，info icon + 4 类外网请求 + 标题/描述。
- **`src/components/setting/NetworkSection.tsx`** —— 完全重写。组成：
  - `useSetting()` 拿 `setting.network.allowRelayFallback`
  - `useDebounce` 包用户输入
  - LAN-only Switch SettingRow（label + description + info-icon Popover trigger）
  - RestartBanner（visible 由 `pending` 状态驱动）
  - 「立即重启」按钮通过 invokeWithTrace 调新 Tauri command
- **新增 Tauri commands**（`src-tauri/crates/uc-tauri/src/commands/`）——
  - `restart_app(...)`（命名待定）—— 调 `app.restart()`，加 `record_trace_fields`，与 updater.rs:301 同模式
  - `get_restart_state(...)`（命名待定）—— 返回 `{ process_started_at: i64, settings_mtime: i64 }`（millis）
  - `mod.rs` 注册 + `pub use`
  - `commands/restart.rs` 新文件（建议）
- **`uc_tauri::run` 启动早期初始化 `PROCESS_STARTED_AT: OnceCell<SystemTime>`** —— Tauri builder setup 阶段 set。
- **i18n key 增删** ——
  - 新增：`settings.sections.network.lanOnly.label/description`、`settings.sections.network.lanOnly.disclosure.{rendezvous,otlp,pkarr,autoUpdate}.{title,description}`、`settings.sections.network.restartBanner.{message,restartButton,errorMessage}`、`settings.sections.network.lanOnly.infoIconAriaLabel`
  - 删除：`settings.sections.network.placeholder`（grep 仅 NetworkSection.tsx 一处引用）
  - 旧 `network.{syncMethod,webserverPort,customPeerDevice,cloudServer}` 块清理由 Claude Discretion 决定（顺手或延后）
- **OpenAPI 客户端** —— Phase 94 已经在 `UpdateSettingsResponse` schema 加了 `restart_required`；如果前端用代码生成的 client，需要重新生成（确认仓库是否有此流程，未看到则跳过）。

</code_context>

<specifics>
## Specific Ideas

- **「立即重启」按钮文案**（zh / en 双语，最终由 planner 起草，建议方向）：
  - zh-CN: 「立即重启以应用」/「立即重启」
  - en-US: "Restart now to apply" / "Restart now"
- **RestartBanner 主信息文案** 建议：
  - zh-CN: 「需要重启应用以使 LAN-only Mode 生效」
  - en-US: "Restart the app to apply the LAN-only Mode change"
- **Popover 4 类外网请求清单**（最终敲定文案、Phase 97 复制粘贴）：
  1. **首次配对 rendezvous** —— 配对新设备时仍需联网经 `rendezvous.uniclipboard.app` 完成 NodeId 交换；已配对设备日常同步不再用
  2. **OTLP 遥测** —— 由 General → 遥测开关独立控制，与 LAN-only 无关；如需关闭请到 General 分类
  3. **pkarr DHT NodeId 解析** —— 跨网段连接通过 pkarr 公网 DHT 解析对端 NodeId，性质类似 DNS，关闭会导致跨网段连接率从 ~90% 跌到接近 0
  4. **auto-update GitHub 检查** —— 由 General → 自动更新开关独立控制，访问 GitHub release API 检查新版本
  - 措辞**禁用**："fully offline / 完全离线 / 绝对私有 / no internet / private mode / encrypted-and-local"
- **info icon 选 lucide-react `Info`** —— 与 settings-config.ts about 分类一致；planner 可换 `HelpCircle`，但同一文件内保持单选。
- **三态 → 实际只是二态布尔** —— 提醒 planner：实现时 `pending: boolean` 即可，不需要 `RestartState = 'applied-off' | 'applied-on' | 'pending'` 这种 enum；状态来自「当前 setting 值 + mtime > process_started_at」推导。
- **PUT /settings 调用顺序与 banner 显示**：
  1. 用户切 Switch → `setting.network.allowRelayFallback` 局部更新（乐观）→ Banner 立即显示
  2. `useDebounce` 500ms 后触发 `updateNetworkSetting({ allowRelayFallback })`
  3. PUT 到 daemon，daemon 返回 `restart_required: true`
  4. SettingContext refresh 持久值；Banner 维持显示
  5. 用户点「立即重启」→ `invokeWithTrace('restart_app')` → Tauri 进程退出
  6. 新进程启动 → `process_started_at = now` → settings_mtime < process_started_at → Banner 不显示

</specifics>

<deferred>
## Deferred Ideas

- **daemon-only CLI 模式（`uniclip daemon`）的「立即重启」UX** —— 整里程碑显式排除（PROJECT.md §Out of Scope）。CLI 用户走 systemctl/launchd/手动 kill+restart。如果 v0.7.x 用户反馈 daemon 模式 pending 提示缺失，再考虑暴露 `POST /admin/restart` HTTP 端点 + tracing::warn! 提示。
- **`bind_started_at` 通过 daemon HTTP 暴露** —— 仅限 v0.7.x daemon-only 模式 pending 提示需要时再做；Phase 95 不动 daemon HTTP 契约。
- **Phase 96 system tray icon LAN-only 状态徽章** —— INDIC-04，独立 phase。Phase 95 不做 tray 形态。
- **Phase 97 `docs/lan-only.md` / `docs/terminology.md` / changelog** —— DOC-01/02/03，独立 phase。Phase 95 把 4 类请求文案最终敲定供 Phase 97 复制粘贴；reviewer checklist + PR 模板 Pitfall 5 守护放 Phase 97。
- **运行时热切换 LAN-only Mode** —— 整里程碑显式排除，独立 phase + 重建 endpoint + ALPN handler 重挂；当前由 OnceCell 主动阻断（Phase 94 plan 06）。
- **OTLP `connection_path` 标签** —— Future Requirements D4，v0.7.x 范围。
- **D6「测试 LAN-only」诊断按钮** —— Future Requirements，v0.8+ 范围。
- **旧 i18n 残留 `network.{syncMethod,webserverPort,customPeerDevice,cloudServer}` 整块清理** —— Claude Discretion；planner 决定 Phase 95 顺手清还是单独 cleanup phase。建议顺手清：
  - 优点：Phase 95 改完整个 NetworkSection 区，i18n 同步更新更自然
  - 风险：grep 全仓确认无引用即可（已知 placeholder 仅 NetworkSection.tsx 一处）

### Reviewed Todos (not folded)

无 — `gsd-tools todo match-phase 95` 返回 5 个匹配但都不在本 phase 范围：
- `2026-03-21-fix-setup-pairing-confirmation-toast-missing.md`（score 0.7）—— setup 配对 toast，不是 NetworkSection
- `2026-04-26-daemon-clipboard-workers.md`（0.6）—— daemon clipboard 范围，不相关
- `2026-04-26-route-daemon-composition-through-application.md`（0.6）—— architecture，不相关
- `2026-04-27-hybrid-daemon-connection-info.md`（0.6）—— hybrid daemon，不相关
- `2026-04-17-wire-real-filetransferevent-cancelled-emitter.md`（0.2）—— file transfer，不相关

</deferred>

---

*Phase: 95-前端 NetworkSection + 重启 UX*
*Context gathered: 2026-05-04*
