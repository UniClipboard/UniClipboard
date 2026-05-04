# Phase 95: 前端 NetworkSection + 重启 UX - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-04
**Phase:** 95-前端 NetworkSection + 重启 UX
**Areas discussed:** 重启 UX 形态 (A1/A2/A3), 立即重启工程范围 (B), 4 类外网请求披露形态 (C), pending 跨 session 持久性 (D)

---

## 范围筛选（Initial gray area selection）

| Option | Description | Selected |
|--------|-------------|----------|
| 重启 UX 形态 | 持久 inline 通知放哪里 + 三态视觉怎么呈现 | ✓ |
| 立即重启的工程范围 | GUI Tauri vs daemon-only CLI 是否覆盖 | ✓ |
| 4 类外网请求披露形态 | Tooltip vs Popover vs inline | ✓ |
| pending 跨 session 持久性 | 关掉应用再打开还显示吗 | ✓ |

**User's choice:** 全部 4 个展开
**Notes:** 用户对所有 4 个领域都想拍板，没有交给 planner 决定。

---

## A1. 通知载体（持久 inline 重启通知组件选型）

| Option | Description | Selected |
|--------|-------------|----------|
| 复用 shadcn Alert | 现有 src/components/ui/alert.tsx；样式与其它 Settings 区块一致；成本最低、跳动最少 | |
| 写专用 RestartBanner | src/components/setting/RestartBanner.tsx 独立组件，样式定制；适合后续复用（Phase 96/97 可能也需要重启提示），但要多建一个组件 | ✓ |
| Sonner toast 带 duration:Infinity | 与 ROADMAP 「非 toast」要求字面冲突 —— 用户心智里「toast」= 浮在右上角可被点掉的短提示；不推荐但技术上可行 | |

**User's choice:** 写专用 RestartBanner
**Notes:** 选定独立组件以便后续 Phase 96/97 复用。

---

## A2. 通知位置（贴在哪里）

| Option | Description | Selected |
|--------|-------------|----------|
| Section 内部（近按钮） | 贴在 NetworkSection 顶部、LAN-only Switch 上方；切到其它 Settings 分类看不到，但 Phase 96 system tray icon 可补全局提醒 | ✓ |
| Settings 页顶部（跨 section） | 改 SettingsSidebar / Settings 布局，顶部预留区列出全局提示；要修改 Settings 布局、几个文件 — 独立于 Phase 95 范围。收益：用户在其它分类也能看到 | |
| 应用全局顶部（跨页面） | 放在 App layout 级别，Dashboard / Devices / Settings 都能看到。合适重启这种「全局调件」提示，但需动到 layout 层、与 Phase 97 onboarding banner 位置冲突（均顶部） | |

**User's choice:** Section 内部（近按钮）
**Notes:** 接受切到其它 Settings 分类时 Banner 不可见的代价；v0.7.0 全局可见性由 Phase 96 system tray 兜底。

---

## A3. 三态视觉呈现

| Option | Description | Selected |
|--------|-------------|----------|
| Switch + 旁 Badge "Pending" | Switch 选中状态不变（跟随用户的选择），同时右侧出现一个 Badge（如 "重启后生效"）。Badge 组件已有；表达明显。需 Settings 设值 vs daemon 实际 bind 状态差异判断 | |
| Switch 加虚线/饣色状 | pending 时 Switch 以虚线边框或饣色状态表达「还未生效」。需要改 Switch 样式变体，技术上能做但与现有 Switch 样式远。可读性不如 Badge | |
| 只靠 RestartBanner（推荐） | 不动 Switch 本身。applied = 无 banner；pending = 有 banner。最轻量。但「三态视觉」严格说只能区分二态（applied vs pending）— 如果说 ROADMAP 的 「applied OFF / applied ON」 由 Switch 本身表达已足够，这个选项最净 | ✓ |

**User's choice:**（重问后）只靠 RestartBanner（推荐）
**Notes:** 用户最初问"为什么是三态，我理解只有两个状态"。Claude 解释清楚 ROADMAP「三态」= applied OFF / applied ON / pending change，pending 是过渡态（settings 已写盘但 daemon 没重启 / iroh 还在跑旧 bind）。重新问后用户选择最干净方案——RestartBanner 出现/消失独立表达 pending 信号，不需要 Switch 上额外 Badge。

---

## B. 重启范围（"立即重启" 工程范围）

| Option | Description | Selected |
|--------|-------------|----------|
| 只 cover Tauri GUI | Tauri 命令 + app.restart()（与 updater 一致的现有模式 src-tauri/.../updater.rs:301）。daemon-only 用户（uniclip daemon CLI）几乎都是后台运营，手动 systemctl restart / supervisor 重启。Phase 95 仅 GUI | ✓ |
| GUI + daemon 两路径 | Tauri GUI 走 app.restart()。daemon-only 模式提供 POST /admin/restart 之类 HTTP 端点调 std::process::exit + supervisor 重拉 — 但需 supervisor 在位。工作量大，且 daemon-only 模式本来没 GUI 交互 | |
| GUI + daemon 提示手动重启 | GUI 走 app.restart()。daemon-only 模式不提供「重启」按钮，只在 PUT 响应 restart_required:true 时 daemon 打一个 tracing::warn! 提醒 "network settings changed; restart daemon to apply"。Phase 95 不包括 daemon-only 重启交互 UI | |

**User's choice:** 只 cover Tauri GUI
**Notes:** 用户进一步追问"重启什么，需要重启整个 GUI 吗"。Claude 解释：iroh RelayMode 是 endpoint bind 时常量，Phase 94 plan 06 加了 OnceCell 守护进程内只能 bind 一次（Pitfall 3）。GUI 模式下 daemon 是 in-process（uc-desktop/src/daemon/handle.rs::start_in_process），所以"daemon graceful shutdown + relaunch" ≡ Tauri 整个 GUI 进程退出 + 重新拉起。daemon 子系统随进程一起重启 → 新进程读新 settings.json → IrohNodeBuilder::bind 用新 disable_relays 值。这是唯一干净路径。

---

## C. 4 类外网请求披露形态

| Option | Description | Selected |
|--------|-------------|----------|
| Popover（点击展开） | src/components/ui/popover.tsx 已有。点 info icon 弹出面板，列出 4 类 + 描述，文字可选可复制，面板外点击自动关闭。适合详细内容、响应式位置算法与 Tooltip 一致。**推荐** | ✓ |
| Tooltip（hover-only） | 与 ROADMAP 字面一致。src/components/ui/tooltip.tsx 已有。但 Tooltip 设计是短提示：hover 离开即消失、文字不可选、移动到 Tooltip 里点超链接也难。与"4 类详细披露"有点错位 | |
| Inline Collapsible 展开 | src/components/ui/collapsible.tsx 已有。Switch 下方一个「为什么还是会联网？」折叠区，点一下展开列表。占用纵向空间但不需额外面板。适合「总是可见 + 需要才展开」 | |

**User's choice:** Popover（点击展开）
**Notes:** 4 条详细描述 hover Tooltip 容纳量不够，且不可复制。Popover 适合有标题 + 多条描述的可读披露。

---

## D. pending 跨 session 持久性

| Option | Description | Selected |
|--------|-------------|----------|
| 用 settings_loaded_at 时间戳判断（推荐）| Tauri Rust 端在进程启动时记下 process_started_at。响应 GET /settings 时同时返回 settings_persisted_at（settings.json 最后改动时间）。**settings_persisted_at > process_started_at 且所在改动是 network 段 ⇒ pending**。需后端增加此信号（跨 phase 95 范围，影响 Phase 94 DTO） | ✓ |
| 前端 localStorage 记录「pending」位 | 在前端 PUT 响应 restart_required:true 后写 localStorage；localStorage 有 ⇒ 渲染 Banner。点「立即重启」后清除。主要问题：进程重启后 localStorage 仍在，需一个「启动后清除 pending 标记」的机制 — 但这与 "重启后 bind 应用新值" 是同一件事，由启动路径 clear 即可 | |
| 不持久化 (in-memory 只) | 只在本次会话记住。关掏重开不再提示。代价：用户下次看到 Switch=ON 但实际 daemon 还在跑旧值—沉默失同步，Pitfall 10 破微防。**不推荐** | |

**User's choice:** 用 settings_loaded_at 时间戳判断（推荐）

### D 细化（实现路径分支）

| Option | Description | Selected |
|--------|-------------|----------|
| Tauri command（推荐） | 与 Phase 95 GUI 范围一致，不动 Phase 94 已锁 daemon HTTP 契约。Tauri 端加一个 get_restart_state command 返回 { process_started_at, settings_mtime }。前端比对 settings_mtime > process_started_at 且 network 段变动 ⇒ pending | ✓ |
| 扩 daemon HTTP 契约 | GET /settings 响应加 settings_persisted_at，同时 daemon 启动时记下 bind_started_at 也由 HTTP 返回。后续 CLI daemon 模式也能读。代价：Phase 94 已锁定的 GetSettingsResponse / OpenAPI 动了，跨越 Phase 95 范围 | |

**User's choice:** Tauri command（推荐）
**Notes:** 选定 Tauri command 路径以与 Phase 95 GUI 范围一致；不动 Phase 94 已锁 daemon HTTP 契约。

---

## Claude's Discretion

由 planner / 实施者决策，CONTEXT.md `<decisions>` 段「Claude's Discretion」已列出：

- RestartBanner 视觉细节（配色、图标、按钮 variant、内边距）
- Tauri command 命名（`restart_app` / `request_restart` / `restart_for_settings` 等）
- info icon lucide-react 图标选型（`Info` / `HelpCircle` / `CircleHelp`）
- i18n key 命名层级
- Popover 触发器是 button 还是 icon-only
- 旧 i18n 残留 `network.{syncMethod,webserverPort,customPeerDevice,cloudServer}` 块清理范围
- 重启失败 retry/dismiss 按钮文案配置
- PUT /settings 失败 Switch 回滚 + Banner 处理细节

## Deferred Ideas

详见 CONTEXT.md `<deferred>` 段。要点：

- daemon-only CLI 模式 「立即重启」 UX —— 整里程碑显式排除
- `bind_started_at` 通过 daemon HTTP 暴露 —— v0.7.x 后续按需
- Phase 96 system tray icon LAN-only 状态徽章
- Phase 97 `docs/lan-only.md` / `docs/terminology.md` / changelog（Phase 95 文案最终敲定供 Phase 97 复制）
- 运行时热切换 LAN-only Mode（整里程碑显式排除）
- OTLP `connection_path` 标签 (Future Requirement D4)
- D6 "测试 LAN-only" 诊断按钮 (v0.8+)
- 旧 i18n 残留整块清理（Claude Discretion）

### Reviewed Todos (not folded)

5 个 todos 与 Phase 95 不相关：
- 2026-03-21-fix-setup-pairing-confirmation-toast-missing.md（setup 配对 toast，不是 NetworkSection）
- 2026-04-26-daemon-clipboard-workers.md（daemon clipboard，不相关）
- 2026-04-26-route-daemon-composition-through-application.md（architecture，不相关）
- 2026-04-27-hybrid-daemon-connection-info.md（hybrid daemon，不相关）
- 2026-04-17-wire-real-filetransferevent-cancelled-emitter.md（file transfer，不相关）
