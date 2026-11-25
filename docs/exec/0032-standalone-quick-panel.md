# 执行计划：0032 独立快捷面板（Standalone Quick Panel）

- 状态：P0–P4 与活文档更新已实施并通过隔离进程/原生窗口端到端验收、竞态测试与三平台交叉编译；跨平台真实系统剪贴板与跨应用粘贴验收（N01–N04）保持 `NOT_RUN`（需显式授权的平台/一次性虚拟机环境），因此决策 `0032` 保留于 `docs/decisions/proposed/`。
- 日期：2026-11-25
- 决策：[0032 · Standalone Quick Panel](../decisions/proposed/0032-standalone-quick-panel.md)
- 代码基线：`58303fae954cf48e16d99099c4dd9b224c2b8c18`（分支 `wayland-quick-panel`，保留工作区已有的 `0032` 决策与台账登记）
- 落地目标：[无头服务与桌面集成架构](../architecture/headless.md)、[桌面运行时架构](../architecture/desktop-runtime.md)、[快捷面板系统契约](../contracts/desktop-quick-panel-contract.md)、[本机服务与桌面适配契约](../contracts/local-service-contract.md)
- 关联契约：[Engine 公共契约](../contracts/engine-contract.md)、[桌面端到端验收契约](../contracts/desktop-e2e-contract.md)
- 授权与证据边界：本文档仅记录源码调研结论与分阶段执行计划，不启动图形应用或虚拟机，也不代表任何运行时或系统剪贴板验收已完成。目录剪贴板（`testboard.Dir`）仅证明进程生命周期与本机协议顺序；真实系统剪贴板与跨应用粘贴须独立运行并单独记录结论。

## 1. 目标结果与不变量边界

在不引入新发行二进制、不向 `unic` / `unicd` 引入任何 UI 或 `mygo` 传递依赖的前提下，使已初始化的 profile 仅凭独立运行的 `unicd` 与公开入口 `unic-desktop --quick-panel` 即可在 Linux（Wayland 与 X11）、Windows、macOS 上完成快捷面板的打开/聚焦、历史搜索、内容预览、剪贴板复制（Copy），并在支持前台目标捕获与按键注入的会话（macOS、Windows、X11）中保持受检自动粘贴，在 Wayland 会话中提供可靠复制（含窗口关闭后的无剪贴板管理器选区保活）并明确报告自动粘贴不可用。

核心不变量：

1. **发行与依赖边界不变**：保留单一桌面程序 `unic-desktop` 与无头程序 `unic` / `unicd`。[Engine 边界测试](../../engine/boundary_test.go) 与无头二进制依赖检查保持零 UI 传递依赖；公开 `engine` Go SDK 签名无需破坏性变更。
2. **服务借用而非托管**：`unic-desktop --quick-panel` 仅连接同一 profile 下已就绪的非临时 `unicd`，借用其生命周期；服务缺失、不可达、profile 不匹配、处于 `retiring` 或 `cli-temporary` 模式时直接报错退出，**绝不隐式启动 `unicd`、`tray` 或 `gui`**。关闭面板窗口或退出面板进程均不停止所借用的 `unicd`。
3. **单一受控应用仲裁与单一观察基线**：每个 profile 同一时刻仅允许一个在途剪贴板应用票据（`Engine` 的 `e.ticket`）与一条连续演进的观察基线。无论是否存在后台观察者（`Tray` 或 macOS `unicd` 适配器），面板写入都必须经过 `unicd` 授权、原生写后回读校验、精确结果上报与基线推进，杜绝采集回环（capture echo）与同步回环（sync echo），并保留 `sync.on_restore` 与纯文本 Action（`builtin.text/plain` 不触发 restore-sync）既有语义。
4. **不确定结果零重放**：一旦原生剪贴板写入已发起而结果确认超时、断连或服务代次变化，面板必须向用户报告未确认错误，禁止注入粘贴按键，禁止自动重放请求；再次写入须来自新的显式用户操作。
5. **窗口资源释放与选区/文件租约保活分离**：面板隐藏/关闭时立即销毁主面板窗口与预览卫星 Flyout，取消视图上下文、事件流和解码缓存；原生剪贴板数据源（如 Wayland `wl_data_source` / GTK selection / Windows OLE 数据对象）以及已放到剪贴板上的托管文件租约继续保留在无窗口 `unic-panel` 进程与 `unicd` 会话中，直至剪贴板被外部内容替换或服务退出。

## 2. 现状源码审计与差距定位

基于 `58303fae9` 对照决策 `0032` 的逐层核查结论：

| 模块与路径 | 当前实现事实 | 与 0032 的差距及改造点 |
|---|---|---|
| 桌面命令入口 `app/desktop/cmd/unic-desktop/main.go` | 仅支持 `--mode`、`--background`、内部 `--role` 与私有 `--command`；无参入口固定启动 `tray` 并请求 `startup`/`background`。 | 新增公开布尔参数 `--quick-panel`；本地拒绝与 `--role`/`--command`/`--background`/`--mode`/位置参数的非法组合；实现服务探针、profile 校验、引导/迁移准入与共享面板激活调用。 |
| 角色协调与托盘生命周期 `app/desktop/cmd/unic-desktop/roles.go`, `roles_native.go` | 1) `trayRuntime.openPanelFiltered` 与 `ProcessPanel.Open` 将重复激活硬编码为隐藏切换，并发激活会把刚打开的面板关掉；2) 前台目标捕获与 `AllowForeground` 仅在 `trayRuntime` 中执行；3) `runPanel` 打开主窗口/设置硬编码调用 `Tray` 的 `open-gui`，无托盘时报错失败；4) `trayRuntime.close()` 在任意托盘退出路径均调用 `closeViews(GUI, QuickPanel)`。 | 1) 拆分为统一激活函数，显式区分 `open`（打开或聚焦，不覆盖已有目标，不误关窗口）、`toggle`（全局快捷键切换）与 `favorites`（收藏筛选）；2) 将激活前目标捕获与 `AllowForeground` 下沉到共享激活入口，使 `--quick-panel` 无托盘时同样生效；3) `runPanel` 的 `root` 回调在无托盘时通过标准桌面启动流拉起 `Tray`+`GUI` 并导航到对应页面；4) 区分整应用显式退出/轻量关闭/迁移/重启与借用独立 `unicd` 时的托盘单独退出，后者保留独立面板可用。 |
| 面板视图与窗口管理 `app/desktop/internal/desktopui/process_panel.go`, `quick_panel_window.go` | `ProcessPanel.Open` 若 `Visible()` 为真直接 `Hide()`；`release()` 销毁窗口并清空历史绑定与投影行，进程保持无窗口待命。 | 增加已可见窗口的显式聚焦分支（`Show` + `Focus`，保留当前会话 `target` 与搜索状态）；确保销毁窗口时不调用 `Clipboard.Clear()`，保留无窗口进程的原生选区源。 |
| macOS 守护进程剪贴板适配 `app/service/cmd/unicd/clipboard_darwin.go`, `roles_native.go` | `unicd` 内置 `nativeboard` 适配器，但仅 `Tray` 启动时调用 `POST /v1/desktop/clipboard/enable`；独立 `unicd` 未启用时 `/v1/desktop/clipboard/apply` 直接返回 `Failure: "clipboard"`。 | macOS 面板剪贴板客户端在执行 `Apply` 时按需请求 `EnableDesktopClipboard` 并等待初始基线就绪（`cli-temporary` 与 `retiring` 服务仍以 `409` 拒绝），无需在无头 `unicd` 启动阶段提前加载 AppKit。 |
| Windows / Linux 剪贴板写入与跟踪 `app/desktop/internal/board/board.go`, `internal/clipboardintegration/` | 非 macOS 平台由 `Tray` 运行 `RunRemote` 持有唯一适配器；面板通过 `Tray` 私有 IPC 请求 `clipboard-apply` / `clipboard-matches`。`mygo` `v0.9.2` 已原生支持 `ClipboardOptions{OnRelease}`、`Clipboard.OnChanged`（GTK `owner-change` / Win32 `WM_CLIPBOARDUPDATE`）及窗口关闭后选区持续服务。 | 移除面板对 `Tray` 剪贴板 IPC 的依赖；面板使用本进程 `board`（生产为 `board.MyGo`，隔离 E2E 为 `testboard.Dir`）执行经 `unicd` 授权的两阶段写入与回读校验，并利用 `OnRelease` + 租约期按需 `OnChanged` 在剪贴板被替换时释放托管文件租约。 |
| 本机服务适配器状态机 `app/service/internal/host/adapter.go`, `app/service/internal/server/adapter.go`, `internal/localapi/adapter.go` | `adapterState` 仅支持单一常驻 `session` 轮询协议；`BeginApplyHistory` 需要 `HostCapabilities.Apply == Available` 且 `ExpectedEpoch`/`ExpectedSequence` 与 `Engine` 严格相等；若外部写入推进 `e.obs.seq`，常驻观察者本地序号会失步冲突或产生回环。 | 扩展本机服务适配协议，新增绑定 `{profile, service_id, request_id, token}` 的有界显式面板应用接口（`begin` / `complete` / `release`）：无观察者时自动建立单次显式基线；有观察者时串行化写窗口并维护同 `epoch` 序号偏移映射，使观察者后续轮询稳定命中 `Engine` 的 `DispUnchanged`。 |
| 引导与迁移门控 `app/desktop/internal/setupflow/flow.go`, `internal/climigration/state.go` | `setupflow.Open(dir)` 在首次访问不存在 `desktop-setup.json` 的目录时会创建目录并落盘状态文件。 | 提供无副作用的只读准入检查函数（综合 `desktop-setup.json`、`cli-migration.json`、既有非空 `unic.db` 及可选在线 `tray` 迁移暂停态），避免被拒绝的 `--quick-panel` 调用污染未初始化目录。 |
| 自动化测试桩 `app/desktop/cmd/unic-desktop/automation_e2e.go`, `roles_fixture_e2e.go`, `app/desktop/e2e/` | `automation_e2e.go` 校验无 `--role` 进程 `MYGO_AUTOMATION == ""`，由 `roleEnvironment` 为 `quick-panel` 子进程注入 `automation-panel.sock`；`roles_fixture_e2e.go` 非视觉桩尚未模拟独立面板剪贴板协议。 | 扩展非视觉进程桩与原生 E2E 指令，支持通过公开 `--quick-panel` 启动/重入、独立 `unicd` 启停、托盘并存与退出切换、写中断注入及剪贴板替换后的文件租约核验。 |

## 3. P0 契约关口（生产代码改动前必须冻结）

在修改运行时代码前，须先在[本机服务契约](../contracts/local-service-contract.md)与[快捷面板系统契约](../contracts/desktop-quick-panel-contract.md)中落地以下精确规格：

### 3.1 公开命令行入口、准入与激活语义

1. **命令形式与参数互斥**：
   - 公开入口：`unic-desktop --quick-panel`。
   - 与 `--role`、`--command`、`--background`、`--mode` 及任何位置参数同时出现时，在本地参数解析阶段立即向 `stderr` 输出用法错误并以非零状态退出，不访问 socket、配置文件或锁文件。
   - `--version` 与 `--version --json` 继续在一切初始化之前返回构建身份。
2. **服务探针与 profile 绑定**：
   - 使用与 `unic` / `unic-desktop` 一致的路径解析规则（`UNIC_HOME`、`UNIC_SOCK`、`portable.dat`、默认目录与 `dev` 隔离目录）。
   - 向本机 socket 发起 `GET /v1/service`（探针超时 `1s`）：
     - 无监听、连接被拒、权限错误或非 `unicd` 响应：直接报告后台服务未运行/不可用并退出 `1`。
     - `service.Directory != canonical(dir)`：报告 profile 与 socket 服务不匹配并退出 `1`，绝不停止或解绑对方 socket。
     - `service.Retiring == true` 或 `service.Mode == "cli-temporary"`：报告当前服务不可用于桌面快捷面板并退出 `1`，绝不借用或替换 CLI 临时服务。
3. **只读引导与迁移准入**：
   - 激活前读取目标目录的 `desktop-setup.json`、`cli-migration.json` 与 `unic.db` 状态（不创建目录或写入标记文件），并在 `Tray` 在线时探测 `Tray` 是否处于迁移暂停态（`paused`）：
     - 若存在未完成的首次引导（`desktop-setup.json` 中 `complete: false`）、未完成的 v1 迁移（`cli-migration.json` 中 `status: "pending"`）、损坏的状态文件、在线 `Tray` 正在执行迁移暂停，或目录完全未初始化（既无已完成的 `desktop-setup.json` / `cli-migration.json`，也无既有合法非空 `unic.db`）：`--quick-panel` 直接拒绝激活并返回明确提示（引导用户运行 `unic-desktop` 完成引导或使用 `unic migrate` 初始化）。
     - 若由在线 `Tray` 的全局快捷键在 `setup_required` 期间触发，保持既有托盘行为（拉起 `GUI` 引导窗口）。
4. **统一激活路径与三种动作语义**：
   - 抽取共享激活入口供 `--quick-panel`、托盘菜单、主窗口按钮及全局快捷键共用，所有角色进程拉起通过 `desktopprocess` 的 profile 级内核文件锁（`quick-panel.lock`）与 OS 对端可执行文件身份校验（PID + 规范化路径 + SHA-256）串行收敛：
     - `open`（`--quick-panel`、托盘菜单、主窗口“打开快捷面板”按钮）：若面板当前已可见，聚焦现有面板窗口（`AllowForeground` + `Show` + `Focus`），**不重新捕获或覆盖原会话目标，也不隐藏窗口**；并发发起多个 `--quick-panel` 请求时，最终收敛到同一 profile 下的唯一可见面板进程与同一 `unicd` 代次。
     - `toggle`（全局切换快捷键 `TogglePanel`）：若面板已可见则执行显式关闭（若 `unic` 仍持有前台则恢复原目标焦点）；若不可见则按标准流程捕获目标并打开。
     - `favorites`（全局收藏快捷键 `ShowFavorites`）：若面板已可见则切换至收藏筛选且不重捕目标；若不可见则捕获目标并以收藏筛选打开。
   - **激活前目标捕获**：仅在面板处于不可见态、准备打开新会话时，在面板窗口显示并获取焦点**之前**调用 `Platform.Capture()`，并剔除当前 profile 下任何已知桌面角色（自身、`gui`、`quick-panel`、`tray`）的 PID。
5. **从独立面板打开主窗口与进程寿命**：
   - 面板内显式触发“打开主窗口”（`main`）或“快捷面板设置”（`settings`）时：若 `Tray` 在线则调用 `Tray` 的 `open-gui`；若 `Tray` 不在线则调用标准桌面启动流程拉起 `Tray` + `GUI` 并传递目标页面动作。
   - 寿命边界：
     - 独立面板借用已运行的 `unicd`；隐藏面板销毁窗口但保留待命进程与有效选区/文件租约；关闭或终止面板进程不影响 `unicd`。
     - 普通模式关闭 `GUI` 仅退出 `GUI`。
     - 当借用独立 `unicd` 的 `Tray` 单独退出（如收到 `SIGTERM` 或托盘退出而保留独立 `unicd`）时，`Tray` 释放自身快捷键与后台观察者会话，**不强杀独立运行的 `QuickPanel`**；面板后续复制直接走无观察者的服务协调路径。
     - 显式整应用退出（`Quit` / 更新器重拉起）、服务重启（`restart`）、离线迁移暂停（`pause-runtime`）及既有轻量模式主窗口关闭（`gui-closed`）维持既有角色回收契约；回收后只要独立 `unicd` 仍在运行，再次执行 `unic-desktop --quick-panel` 即可重新打开面板。

### 3.2 服务协调的显式面板剪贴板协议（Local Service Contract 扩展）

所有端点仅暴露在 owner-only 本机私有 Unix/AF_UNIX socket 上，协议版本保持 `1`，请求体严格开启 `DisallowUnknownFields` 并受大小限制保护：

1. **macOS 守护进程写入路径（复用既有端点并补齐按需启用）**：
   - `unicd` 启动时仍保持纯无头，不主动初始化 `nativeboard` / AppKit。
   - macOS 面板在触发剪贴板应用时，若服务侧适配器尚未就绪，先调用幂等的 `POST /v1/desktop/clipboard/enable`（`cli-temporary`、正在退出或非活跃图形控制台会话被明确拒绝；首次调用等待初始基线与 `ready` 快照完成后返回 `200`），再调用 `POST /v1/desktop/clipboard/apply` 与 `POST /v1/desktop/clipboard/match`。
2. **Windows 与 Linux 面板显式应用路径（新增三步有界协调端点）**：
   - **`POST /v1/desktop/panel-apply/begin`**（请求体上限 `96 MiB`，与 `import` 一致）：
     - 请求：`{profile, service_id?, request_id, history_id, plain, observed_before?}`。
     - 校验：`profile` 必须等于当前 `unicd` 的规范化状态目录；若传入 `service_id` 则必须匹配当前 `servicelifecycle` 代次 ID；`cli-temporary` 或 `retiring` 服务返回 `409 conflict` / `unavailable`；`request_id` 长度 `1..128` 字节。
     - 幂等与变换：
       - `plain == false`：复用宿主 `restores` 请求缓存（最多 `1024` 条，TTL `30m`；同 `request_id` 不同 `history_id` 报 `conflict`）；应用时标记 `UserRestore: true`。
       - `plain == true`：先校验 `history_id` 包含 `text/plain` 表示（缺失如纯 HTML、图片、文件则立即返回错误码 `plain`，不执行 Action、不写剪贴板）；随后以确定性派生请求键调用 `RunAction("builtin.text/plain")` 生成 `origin=action` 新历史记录，并对该新记录发起本地应用（`UserRestore: false`，后续绝不触发 `SyncRestoredHistory`）。
     - 观察基线与并发串行化：
       - 同一时刻仅允许一个在途剪贴板应用（与常驻观察者 `apply` / `apply-history` 共享同一互斥门控与 `Engine` 的单 `e.ticket` 槽位；超时未完成的残留面板事务超过 `8s` 自动按 `failed` 回收）。
       - **存在活跃后台观察者会话时**：关联当前活跃 `epoch` 与最新服务端观察序号 `lastEngineSeq`，并在面板写事务完成（或 `8s` 到期）前门控该观察者会话的新 `observe` / `apply` 请求，防止写中抢跑产生伪本地复制。
       - **不存在活跃后台观察者会话时**：将宿主 `HostCapabilities` 设为 `Apply: available`（`WriteFormats: [text/plain, text/html, image/png], WriteFiles: true, Observe: unavailable`），将 `observed_before`（若为空或不可读则为空租约）导入并立即提交 `ObserveClipboard(Mode: baseline, Sequence: 1)`，建立本次显式写入所需的观察基线。
     - 签发与物化：调用 `Engine.BeginApplyHistory` 取得 `ApplyTicket{Token, ContentID, LeaseID}`；读取表示字节或解析受控本地文件/目录路径（`LocalFilePath`、`FileReference`、`DirectoryPaths`、`DirectoryReferences`），返回 `{service_id, epoch, token, snapshot, has_files}`。
   - **`POST /v1/desktop/panel-apply/complete`**（请求体上限 `96 MiB`）：
     - 请求：`{profile, service_id, request_id, token, outcome, read_back?, error_code?}`（`outcome` 为 `"applied"` 或 `"failed"`）。
     - 代次与票据强校验：`profile`、`service_id`、`request_id`、`token` 任一不匹配或事务已过期/已结案时，拒绝为 `conflict` 或 `resource_expired`，不推进基线。
     - 当 `outcome == "applied"` 时：
       1. 导入面板回读快照 `read_back` 作为 `baseLease`（函数返回前释放该临时基线租约）。
       2. 计算下一序号 `nextSeq = lastEngineSeq + 1`，调用 `Engine.ReportApplyResult(Token, Applied, BaselineLeaseID: baseLease, Epoch: epoch, Sequence: nextSeq)`。若当前有活跃后台观察者会话，同步递增该 `epoch` 的服务端序号偏移量（`seqOffset++`），使后台观察者随后轮询同一剪贴板内容时自动映射至 `> nextSeq` 的合法序号并由 `Engine` 判定 `key == ob.baseline -> DispUnchanged`，从而零回环、不断流。
       3. 文件租约交接：先完成新快照导入与 `ReconcileClipboardFiles`，再释放此前挂在面板槽位上的旧文件租约（`panelHeldLease`）；若本次 `has_files == true`，保留本次 `ticket.LeaseID` 与规范化文件路径列表，否则立即释放 `ticket.LeaseID`。
       4. Restore 同步：若 `UserRestore == true`，使用独立持有的内容租约调用 `Engine.SyncRestoredHistory(ctx, token, contentLease)`，将完整结果写入 `restores[request_id]` 后返回 `{applied: true, restore}`。
     - 当 `outcome == "failed"` 时：调用 `Engine.ReportApplyResult(Token, Failed, ErrorCode)`，释放非持有租约，不推进基线。
   - **`POST /v1/desktop/panel-apply/release`**（请求体上限 `4 KiB`）：
     - 请求：`{profile, service_id, token, current_paths?}`。
     - 当 `service_id` 与当前挂起文件租约对应的 `token` 匹配且 `current_paths` 不再包含原持有文件时，调用 `Engine.ReconcileClipboardFiles(current_paths)` 并释放 `panelHeldLease`（幂等安全）。同时，后台观察者的常规 `reconcile` 与服务关闭流程也会自动回收已失效的 `panelHeldLease`。

### 3.3 面板端选区保活、文件租约监听与粘贴校验

1. **写入与回读校验**：面板在本地调用 `board.Write` 后立即调用 `board.Read()` 并执行 `sameWrittenSnapshot(want, back)` 比对（文件核对完整路径集合，表示核对各 MIME 字节），计算 `mark = clipboardMark(back)`。回读不一致或读取报错时按 `outcome: "failed", error_code: "conflict"` 上报 `complete` 并向用户报错。
2. **粘贴前二次核验（`Matches(mark)`）**：在 macOS 上调用 `unicd` 的 `/v1/desktop/clipboard/match`；在 Windows / Linux 上直接读取面板本进程 `board.Read()` 比对 `clipboardMark(s) == mark`，确认外部应用未在焦点切换间隙篡改剪贴板后才发送单次粘贴组合键。
3. **Wayland 选区服务与文件租约释放**：
   - 面板通过 `mygo.Clipboard` 写入时挂载 `mygo.ClipboardOptions{OnRelease}`。关闭面板窗口仅销毁 `mygo.Window` 而不清空剪贴板，无窗口 `unic-panel` 进程继续响应 Wayland 合成器/粘贴方的选区数据拉取直至失去选区所有权。
   - 仅当本次写入 `has_files == true` 时，面板激活基于事件的剪贴板变更监听（生产环境使用 `mygo.Clipboard.OnChanged` 配合 `OnRelease`，零后台轮询定时器；`desktop_e2e` 目录剪贴板使用轻量变更检测）：若剪贴板管理器接管选区但 `board.Read()` 仍返回相同文件路径（`sameHeldPaths`），继续保留文件租约；一旦路径变化、变为空/非文件或面板进程退出，立即调用 `/v1/desktop/panel-apply/release` 释放服务端文件租约并注销监听。

## 4. 核对与修改代码地图

| 层级 | 涉及路径 | 计划改动内容 |
|---|---|---|
| 契约与活文档 | `docs/contracts/local-service-contract.md`, `docs/contracts/desktop-quick-panel-contract.md`, `docs/architecture/headless.md`, `docs/architecture/desktop-runtime.md` | P0 先冻结契约条款；P5 按实测验证结论更新架构页并维护 `docscheck`。 |
| 共享本机 API DTO | `internal/localapi/adapter.go`（或独立 `panel_clipboard.go`） | 定义 `PanelApplyBeginRequest`、`PanelApplyGrant`、`PanelApplyCompleteRequest`、`PanelApplyCompleteReply`、`PanelApplyReleaseRequest` 及 `Client` 方法。保持无 UI 依赖。 |
| 服务侧协调实现 | `app/service/internal/host/adapter.go`, `app/service/internal/server/adapter.go`, `app/service/internal/server/server.go`, `app/service/cmd/unicd/daemon.go` | 在 `adapterState` 中实现显式面板应用状态机、8s 超时回收、无观察者基线建立、有观察者写窗口串行门控与同 `epoch` 序号偏移映射、面板文件租约持有与释放；绑定 `servicelifecycle` 代次校验。 |
| 剪贴板集成层 | `internal/clipboardintegration/panel.go`（新增）, `internal/clipboardintegration/clipboard.go`, `app/desktop/internal/board/board.go` | 封装 UI 无关的客户端两阶段面板写入执行器（复核 `sameWrittenSnapshot`、`clipboardMark`、不确定结果屏障与文件租约跟踪）；为 `board.MyGo` 扩展可选 `OnRelease` / 变更监听钩子接口而不影响既有 `Board` 实现。 |
| 引导只读预检 | `app/desktop/internal/setupflow/flow.go` | 新增无磁盘写入副作用的 `CheckAdmission(dir)`（联合 `climigration.Read(dir)` 与 `unic.db` 校验），供 `--quick-panel` 预检使用。 |
| 桌面入口与角色控制 | `app/desktop/cmd/unic-desktop/main.go`, `roles.go`, `roles_native.go`, `app/desktop/internal/desktopui/process_panel.go` | 增加 `--quick-panel` 入口与准入检查；抽取共享激活逻辑（`open` / `toggle` / `favorites`）与激活前目标捕获；面板在 macOS 按需 `EnableDesktopClipboard`、在 Windows/Linux 使用面板剪贴板执行器；修复无托盘时从面板打开主窗口/设置及借用独立服务时托盘单独退出的面板保活。 |
| 桌面端到端套件 | `app/desktop/cmd/unic-desktop/automation_e2e.go`, `roles_fixture_e2e.go`, `app/desktop/e2e/acceptance_processes_test.go`, `native_processes_test.go`, `system_clipboard_test.go`, `app/desktop/e2e/cases/*.txtar` | 先行编写失败用例，覆盖独立 `unicd` + `--quick-panel` 全流程、并发激活与异常准入、托盘并存与退出无回环、写中断零重放、剪贴板替换释放文件租约及各平台粘贴/Wayland 降级验证。 |

## 5. 实施前故障清单（Failure Inventory）

在编写生产运行时代码前，必须将以下失败模式转化为可观察的端到端断言：

| 编号 | 故障模式 | 必须观察到的防御结果 |
|---|---|---|
| F01 | `--quick-panel` 与 `--background`、`--mode`、`--role`、`--command` 或多余位置参数混用 | 本地解析直接返回非零退出码与清晰错误，不连接 socket、不创建进程或状态文件。 |
| F02 | `unicd` 未运行、socket 不可达、或 `UNIC_SOCK` 指向其他 profile 的 `unicd` | `--quick-panel` 直接报错退出，绝不自动启动 `unicd`、`tray` 或 `gui`，不污染目标目录。 |
| F03 | 当前活跃服务为 `unicd --cli-temporary` 或处于 `retiring` | `--quick-panel` 与面板剪贴板端点均明确拒绝，不借用、不阻塞也不杀死临时 CLI 服务。 |
| F04 | Profile 未初始化、`desktop-setup.json` 未完成、`cli-migration.json` 为 `pending` 或 `Tray` 正在离线迁移 | `--quick-panel` 拒绝激活并提示先完成引导/迁移，且不在空目录隐式写入已完成标记。 |
| F05 | 重复或并发执行 `unic-desktop --quick-panel` | 同一 profile 仅存在唯一 `quick-panel` 角色 PID 与同一 `unicd` 代次；已可见面板保持可见并获得焦点，**绝不被并发请求切换隐藏**，也不覆盖首次捕获的粘贴目标。 |
| F06 | 全局快捷键 `TogglePanel` / `ShowFavorites` 在面板可见时触发 | `TogglePanel` 正常关闭已可见面板；`ShowFavorites` 切换收藏筛选且保留原捕获目标，不退化。 |
| F07 | 独立面板（无 `Tray`）点击“打开主窗口”或“快捷面板设置” | 通过标准桌面启动入口成功拉起主窗口并导航至对应视图，而非因 `Tray` 不在而静默失败。 |
| F08 | macOS 仅运行独立 `unicd` 时通过 `--quick-panel` 复制 | 面板按需触发 `/v1/desktop/clipboard/enable` 等待基线就绪后完成写入，且不把启动前系统剪贴板已有内容当作新复制广播出去。 |
| F09 | Windows / Linux 仅运行独立 `unicd`（无后台观察者）时复制文本、图片、文件与纯文本 Action | `unicd` 建立显式基线并签发票据，面板写后回读校验通过并推进基线；`sync.on_restore=true` 时触发一次恢复同步；`Shift+Enter` 纯文本变换不触发恢复同步，HTML-only 条目明确报 `plain` 错误。 |
| F10 | `Tray` 后台观察者与独立面板同时运行时在面板内执行复制 | 单次操作仅写入系统剪贴板一次；`Tray` 轮询不会把面板写入识别为新复制（无新增重复历史、无同步回环广播），观察者会话不因序号冲突断流重连。 |
| F11 | 保留同一独立 `unicd` 并退出 `Tray` 后，继续使用面板执行复制 | 面板进程与窗口保持可用（或可由 `--quick-panel` 直接激活），后续复制无缝切换到无观察者基线路径成功完成，服务 PID 与代次不变。 |
| F12 | Wayland 无托盘、无全局快捷键、无剪贴板管理器会话中复制后关闭面板窗口 | 窗口、搜索与预览资源释放，无窗口 `unic-panel` 进程继续服务选区读取，外部应用仍可读取完整复制内容。 |
| F13 | 独立面板复制托管缓存文件后关闭窗口，外部应用随后写入新剪贴板内容 | 文件在剪贴板持有期间不被缓存清理删除；外部应用替换剪贴板后，旧文件租约及时释放，允许后续配额/过期清理回收底层文件。 |
| F14 | 写入过程中断、`complete` 响应丢失、超时或 `unicd` 重启变更代次 | 过期 `service_id` / `token` 被服务端拒绝；服务端超时自动回收悬空 `e.ticket`；面板报告未确认失败，**不注入粘贴按键、不自动重放写入**，下一次显式用户操作可正常恢复工作。 |
| F15 | Wayland 下按 `Enter` 请求自动粘贴 vs macOS/Windows/X11 目标校验失败 | Wayland 明确提示不支持自动粘贴且保持剪贴板不被误改，`Copy` 成功；支持平台在无权限、焦点转移、目标退出或剪贴板被篡改时绝不注入粘贴按键。 |

## 6. 分阶段实施顺序与准出关口

### P0：契约更新与先行失败用例（Contract Gate & Red Scenarios）

1. 将第 3 节规格写入 [`docs/contracts/local-service-contract.md`](../contracts/local-service-contract.md) 与 [`docs/contracts/desktop-quick-panel-contract.md`](../contracts/desktop-quick-panel-contract.md)。
2. 在 `app/desktop/e2e` 中注册先行验收用例（见第 7 节）并更新 `CASES.md` 目录，确认在未修改生产运行时前，新用例因缺少 `--quick-panel` 与独立面板剪贴板协调能力而按预期失败（Red），而非测试脚手架错误。

**准出条件**：契约 DTO、错误码、超时预算与生命周期表冻结；先行用例可编译并准确复现现状缺口；既有测试集不被破坏。

### P1：本机服务显式面板应用协调层（Service Coordination & macOS Opt-in）

1. 在 `internal/localapi` 定义面板显式应用 DTO 与客户端方法。
2. 在 `app/service/internal/host/adapter.go` 与 `app/service/internal/server` 实现 `panel-apply/begin`、`panel-apply/complete`、`panel-apply/release`：
   - 接入 `servicelifecycle` 代次与模式校验（拒绝 `cli-temporary` 与 `retiring`）。
   - 实现单应用槽位互斥、8 秒悬空票据自动失败回收、无观察者显式基线建立、有观察者写窗口门控与同 `epoch` 序号偏移映射（`seqOffset`）。
   - 接入 `plain` 纯文本 Action 派生与 `UserRestore` / `SyncRestoredHistory` 恢复同步及幂等缓存。
   - 实现面板托管文件租约持有与释放（先导入新快照再释放旧租约）。
3. 验证 `unic` 与 `unicd` 传递依赖仍不包含 `mygo` 或 `app/desktop`。

**准出条件**：针对真实 `unicd` 子进程的协议与竞态场景（无观察者写入、有观察者并发轮询零回环、过期代次/票据拒绝、写中断不重放、文件租约替换释放）全部通过，`-race` 零告警。

### P2：面板剪贴板执行器、选区保活与文件租约生命周期（Panel Clipboard Runtime）

1. 在 `internal/clipboardintegration` 实现面向面板的显式应用客户端（协调 `begin -> board.Write -> board.Read + sameWrittenSnapshot -> complete`，以及 `Matches(mark)` 核验与不确定结果阻断）。
2. 为 `app/desktop/internal/board` 接入可选的 `mygo.ClipboardOptions{OnRelease}` 与按需 `mygo.Clipboard.OnChanged` 回调封装，仅在面板持有文件租约时监听剪贴板替换并触发 `/v1/desktop/panel-apply/release`（待命无文件租约时零后台监听、零定时器）。
3. 为 macOS 面板剪贴板路径补齐按需 `EnableDesktopClipboard` 与就绪等待。

**准出条件**：面板在不启动 `Tray` 的情况下可在隔离目录剪贴板与 macOS 守护进程剪贴板上完成文本、富文本、纯文本变换、图片、文件及目录快照的精确复制与租约释放。

### P3：公开入口 `unic-desktop --quick-panel` 与角色生命周期整合（Public Entry & Lifecycle）

1. 在 `setupflow` 增加无写入副作用的只读引导/迁移准入检查。
2. 在 `app/desktop/cmd/unic-desktop` 实现 `--quick-panel` 参数解析、服务探针、profile 匹配检查、引导/迁移门控与共享激活器（区分 `open` / `toggle` / `favorites`，在窗口显示前完成外部目标捕获与 `AllowForeground`）。
3. 更新 `ProcessPanel.Open` 支持已可见窗口聚焦不误关、不丢原目标；更新 `runPanel` 的 `root` 回调支持无托盘时拉起主窗口/设置；收敛 `trayRuntime.close()` 使借用独立 `unicd` 的托盘单独退出时不强杀独立面板，同时完整保留 `quit`、`gui-closed`（轻量模式）、`restart` 与 `pause-runtime` 的既有角色回收行为。

**准出条件**：所有既有进程生命周期用例（`desktop-processes`、`desktop-lightweight-close`、`desktop-lightweight-invalid-config-close`、`first-install-setup`、`quick-panel-contract`）与新增独立面板用例同时通过。

### P4：全套规范化桌面 E2E 与跨平台编译验证（Canonical E2E & Cross-Build）

1. 通过 `make desktop-e2e` 运行新增与受影响的 `process` / `native` / `view` 用例，核查生成的 HTML 报告、连续采样录屏、截图、`manifest.json` 与 `SHA256SUMS`。
2. 核查单用例耗时与 6 分片均衡（`python3 app/desktop/e2e/run.py list --shard 1/6`），确保单用例远低于 2 分钟警戒线。
3. 执行 macOS、Linux、Windows 目标交叉编译检查、`go test -race ./...`、公开 `engine` 边界检查与代码静态检查。

**准出条件**：隔离 E2E 证据链完整且校验和一致，无残留进程或临时目录泄漏。

### P5：原生系统剪贴板/粘贴验收与活文档落地（Native Acceptance & Docs Landing）

1. 在获得授权的对应平台原生环境 / 一次性虚拟机（特别是无托盘宿主、无全局快捷键、无剪贴板管理器的 Linux Wayland 会话，以及 Linux X11、Windows、macOS）按第 7.2 节逐项执行真实系统剪贴板与目标应用粘贴验收，保留独立回执，严格区分目录剪贴板证据与原生系统证据。
2. 对照已验证代码更新活文档（`docs/architecture/headless.md`、`docs/architecture/desktop-runtime.md`、两份契约及 `app/desktop/README.md`），运行 `go run ./tools/docscheck`（提交后对架构页执行 `-bump`）；在跨平台原生验收全部闭环前保持 `0032` 位于 `proposed/`。

## 7. 端到端验收矩阵与可复现证据规范

严格遵循[桌面端到端契约](../contracts/desktop-e2e-contract.md)，将决策 `0032` 的四组验收场景拆分为**隔离自动化套件（CI 常驻运行，目录剪贴板）**与**原生系统环境套件（显式授权运行，真实系统剪贴板与真实目标应用）**两层，严禁混同结论。

### 7.1 隔离规范化用例（`app/desktop/e2e`）

| 用例 ID（规划） | 模式 | 覆盖 `0032` 场景与故障项 | 关键步骤与可观察断言 |
|---|---|---|---|
| `standalone-quick-panel-lifecycle` | `process` | 场景 1、2、3；F01–F07、F09–F11、F13、F14 | 1) 校验非法参数组合、无服务、跨 profile socket、`cli-temporary` 服务、未初始化/待迁移 profile 均被 `--quick-panel` 直接拒绝且零进程残留；2) 启动独立 `unicd`，并发发起多个 `unic-desktop --quick-panel`，断言收敛为唯一 `quick-panel` PID、无 `tray`/`gui` 进程、同一 `unicd` 代次；3) 执行文本、图片、托管缓存文件的面板复制与关闭，核对目录剪贴板精确字节及外部替换剪贴板后旧文件租约释放；4) 启动 `Tray` 与独立面板并存，验证面板复制仅产生一次写入、无重复历史与同步回环，随后仅退出 `Tray`（保留同一独立 `unicd`）并再次通过面板成功复制；5) 注入写完成中断，断言返回错误且零重放；6) 验证无托盘时面板内触发打开主窗口可正常拉起 `GUI`。 |
| `standalone-quick-panel-native` | `native` | 场景 1、2、4（Wayland/无目标降级）；F05–F06、F09、F12 | 仅启动独立 `unicd`，通过真实公开命令 `unic-desktop --quick-panel` 打开原生面板窗口并采集连续录屏：搜索并预览已存文本、图片、文件历史（含卫星预览窗渲染截图），执行复制，验证再次调用 `--quick-panel` 聚焦已开窗口而不误关，验证关闭窗口后 `unic-panel` 待命进程与 `unicd` 存活且目录剪贴板字节完整。 |
| `quick-panel-contract`（扩展既有） | `process` | 场景 3、4；F09、F14、F15 | 在既有目标会话/焦点/权限/修饰键/取消测试基础上，覆盖服务协调面板写入协议下的粘贴前 `Matches(mark)` 篡改拦截、写中断不发送按键、以及 Wayland `unsupportedPlatform` 报告自动粘贴不可用但 `Copy` 成功。 |

### 7.2 原生系统剪贴板与跨应用粘贴验收（显式授权环境）

| 编号 | 平台与会话环境 | 执行场景与独立验证方式 | 要求保留的证据 |
|---|---|---|---|
| N01 | Linux Wayland（含无托盘宿主、无全局快捷键绑定、无剪贴板管理器的纯净会话；可复用/扩展 `Environment: disposable-guest` 的 `system-wayland-panel` 与新增 `system-wayland-standalone-panel`） | 仅启动独立 `unicd`，运行 `unic-desktop --quick-panel`；检查进程表（仅 `unicd` + `unic-panel`）；搜索预览并依次复制文本、PNG 图片、托管文件；按 `Escape` 关闭面板窗口后，从独立外部应用/工具读取真实 Wayland 选区验证数据仍由无窗口 `unic-panel` 正常提供；按 `Enter` 验证面板明确报告不支持自动粘贴；外部应用写入新剪贴板后验证旧文件租约已释放。 | 平台与桌面会话标识（`WAYLAND_DISPLAY`、合成器、无剪贴板管理器证明）、源码与二进制 SHA-256、进程清单快照、有序操作时间线、外部独立剪贴板回读摘要、录屏与截图、`SHA256SUMS`。 |
| N02 | Linux X11 | 独立 `unicd` + `--quick-panel` 完成搜索、预览、文本/图片/文件复制及关闭后外部应用剪贴板回读；在真实外部编辑器焦点下触发 `--quick-panel` 捕获目标并按 `Enter` 验证单次粘贴插入；并存 `Tray` 后退出 `Tray` 再重复复制验证无回环。 | 会话标识（`DISPLAY`、窗口管理器）、源码与二进制哈希、进程清单、外部应用独立文本/文档回读回执。 |
| N03 | Windows（x64 / ARM64） | 独立 `unicd` + `--quick-panel` 首次冷启动与待命热重入（验证 `AllowForeground` 正常赋予焦点）；复制文本、图片、文件并在窗口关闭后由外部应用读取系统 OLE 剪贴板；验证外部非最小化窗口粘贴不改变原窗口尺寸且最小化窗口可恢复粘贴；外部复制替换后释放文件租约。 | OS 版本与架构、源码与二进制哈希、进程清单、操作顺序、系统剪贴板独立回读与接收窗口结果。 |
| N04 | macOS（arm64 / amd64） | 独立无头 `unicd`（初始未启用剪贴板适配）+ `--quick-panel`；验证首次复制自动按需启用 `unicd` 内 `nativeboard` 且不广播启动前剪贴板；完成文本、图片、文件复制并在面板关闭后从外部应用读取 `NSPasteboard`；验证辅助功能权限检查与外部应用自动粘贴插入。 | macOS 版本与架构、源码与二进制哈希、进程清单、操作时间线、独立 `NSPasteboard` 回读与粘贴回执。 |

## 8. 计划验证命令（待实施阶段执行）

以下命令为实施阶段的计划检查项，当前尚未运行：

```sh
# 1. 更新并校验 Desktop E2E 用例目录与分片均衡
python3 app/desktop/e2e/run.py catalog --update
python3 app/desktop/e2e/run.py list --shard 1/6

# 2. 运行新增及关联规范化桌面 E2E 用例
make desktop-e2e CASE=standalone-quick-panel-lifecycle
make desktop-e2e CASE=standalone-quick-panel-native
make desktop-e2e CASE=quick-panel-contract
make desktop-e2e CASE=desktop-processes
make desktop-e2e CASE=desktop-lightweight-close
make desktop-e2e CASE=production-boundary

# 3. 核心、服务与桌面竞态测试及无头依赖边界验证
go test -race ./...
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build ./app/service/cmd/unic ./app/service/cmd/unicd ./app/desktop/cmd/unic-desktop
GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build ./app/service/cmd/unic ./app/service/cmd/unicd ./app/desktop/cmd/unic-desktop
GOOS=darwin GOARCH=arm64 CGO_ENABLED=0 go build ./app/service/cmd/unic ./app/service/cmd/unicd ./app/desktop/cmd/unic-desktop

# 4. 文档链接、归档引用与空白格式检查
make docs-check
git diff --check
```

## 9. 完成度核对清单

- [x] 深入研究 `0032` 决策、关联契约及桌面/服务/剪贴板现状代码。
- [x] 撰写分阶段执行计划、故障清单与端到端验收矩阵（本文档）。
- [x] P0：完成 `local-service-contract.md` 与 `desktop-quick-panel-contract.md` 契约修订，注册先行失败用例并验证 Red 基线。
- [x] P1：完成 `unicd` 显式面板应用协调协议（含无观察者基线、有观察者序号偏移与写门控、8s 超时自动回收、文件租约管理与幂等恢复同步）。
- [x] P2：完成面板端原生剪贴板写入（`PanelClipboard`）、回读校验、macOS 按需 opt-in（`EnableDesktopClipboard`）、`OnRelease` + `OnChanged` 无轮询文件租约监听与释放。
- [x] P3：完成 `unic-desktop --quick-panel` 公开入口、只读引导/迁移准入（`setupflow.CheckAdmission`）、统一 `open`/`toggle`/`favorites` 激活语义、无托盘打开主窗口及托盘单独退出保活。
- [x] P4：隔离进程与原生窗口 Desktop E2E、全量竞态测试及三平台构建通过。
- [ ] P5：活文档与契约已同步更新并通过 `make docs-check`；已签入跨平台真机 E2E 套件（`app/desktop/e2e/testdata/standalone-quick-panel-real/`）；**N01a/N01b（Linux Wayland `kwin_wayland` + `niri`）、N02（Linux X11 `Xvfb +extension XTEST` + `Openbox`）、N03（Windows 11 ARM64）均已全部通过，N04（macOS 26.5 真实 `NSPasteboard` 10/10 项）已通过**；仅剩 KDE/GNOME `RemoteDesktop` 交互式授权弹窗与 macOS VM `Terminal.app` 辅助功能跨应用粘贴待人工桌面授权确认后再执行决策归档。

## 10. 实施与验证回执（2026-11-25）

- **执行主机**：macOS (`darwin/arm64`)，工作树分支 `wayland-quick-panel`（基线 `58303fae9`）；真机系统剪贴板与跨应用粘贴验证分别执行于：
  - UTM `Fedora` 44 (`linux/aarch64`, UUID `247DFAC1-4D83-4AE1-93B1-47642E7A7939`) — **N01a / N01b（Wayland）** 与 **N02（X11）**；
  - Parallels `Windows 11` (`windows/arm64`, UUID `{32affa85-5174-4d5b-ab6b-903a06d7fa9b}`, `Shared clipboard mode: off`) — **N03**；
  - Parallels `macOS` 26.5 (`darwin/arm64`, UUID `{fcde9fbb-9a92-4d5e-a19c-33cd0e68affe}`, `Shared clipboard mode: off`) — **N04**。
- **证据边界声明**：第 10.1–10.2 节自动化运行使用隔离目录剪贴板（`testboard.Dir`）与独立私有 profile；第 10.4 节 N01–N04 全部运行于显式授权且禁用剪贴板共享的独立虚拟机内，本机宿主机 macOS 真实剪贴板与用户 profile 全程未被触碰。四套可复跑真机驱动已签入 `app/desktop/e2e/testdata/standalone-quick-panel-real/`。

### 10.1 单元测试与竞态检查

- `go test -race -v ./app/service/internal/host -run TestPanelApply`：**PASS**（无观察者多格式写入、HTML-only `plain` 拒绝、`cli-temporary` / `retiring` 拒绝、幂等重放与冲突、8s 悬空事务超时回收、文件租约保留/释放、活跃后台观察者并发零回环与同 epoch 序号连续性）。
- `go test -race -v ./internal/clipboardintegration ./internal/testboard`：**PASS**（`PanelClipboard` 写后回读篡改失败上报、不确定完成零重放、`Matches(mark)` 核验、剪贴板管理器接管同路径保留租约、外部替换触发 `/v1/desktop/panel-apply/release`）。
- `go test -race -v ./app/service/cmd/unicd`：**PASS**（`TestDarwinOnDemandClipboardEnableNoPreexistingBroadcast`：F08 验证 macOS 独立 `unicd` 按需 `EnableDesktopClipboard` 等待基线就绪、不广播启动前剪贴板内容、随后精确写入与 `MatchDesktopClipboard` 校验）。
- `go test -race ./app/desktop/internal/setupflow ./app/desktop/cmd/unic-desktop`：**PASS**（只读 `CheckAdmission` 零目录/标记副作用验证）。
- `TMPDIR=/private/tmp go test -race ./...`：所有改动及相关包全部 **PASS**。注：并行重负载执行全量套件时，未改动的 `engine/TestNewerContentSupersedesAWaitingOne`（`engine/send_test.go:336`）出现 1 次偶发超时失败（`Target State: failed / unavailable: the transfer failed`）；随后以 `TMPDIR=/private/tmp go test -race -count=3 -v ./engine -run TestNewerContentSupersedesAWaitingOne` 独立重试 3/3 全部 **PASS**（0.23s / 0.33s / 0.27s），确认与本次 `0032` 改动无关，根因记为未解决的并行负载时序问题并单独追踪。

### 10.2 规范化桌面 E2E（`make desktop-e2e`，隔离目录剪贴板）

| 用例 ID | 模式 | 耗时 | 结果 | 报告与证据核查 |
|---|---|---|---|---|
| `standalone-quick-panel-native` | `native` | 23.7s | **PASS** | `.build/desktop-e2e/20261125T101531Z-7424fbda/index.html`；`SHA256SUMS` 9/9 文件校验 OK（含 `01-satellite.png`、`02-standalone-panel-open.png`、`03-standalone-panel-refocused.png` 与连续采样 `recording.mp4`）；仅 `unicd` 与 `quick-panel` 运行，无 `tray`/`gui` |
| `standalone-quick-panel-lifecycle` | `process` | 118.8s | **PASS** | `.build/desktop-e2e/20261125T101651Z-9b6a4ad3/index.html`；覆盖 F01–F07、F09–F11、F13、F14 |
| `quick-panel-contract` | `process` | ~31s | **PASS** | `.build/desktop-e2e/20261125T101859Z-6842b335/index.html`；含 `PanelClipboard` 篡改拦截、写中断零按键/零重放、Wayland `ErrUnsupported` 提示自动粘贴不可用且 `Copy` 成功 |
| `desktop-processes` | `process` | PASS | **PASS** | `.build/desktop-e2e/20261125T101937Z-67f6f99a/index.html` |
| `desktop-lightweight-close` | `process` | PASS | **PASS** | `.build/desktop-e2e/20261125T102227Z-bba79b52/index.html` |
| `desktop-lightweight-invalid-config-close` | `process` | PASS | **PASS** | `.build/desktop-e2e/20261125T102443Z-447bbf65/index.html` |
| `desktop-native-processes` | `native` | PASS | **PASS** | `.build/desktop-e2e/20261125T102529Z-7d29bf84/index.html` |
| `first-install-setup` | `process` | PASS | **PASS** | `.build/desktop-e2e/20261125T102642Z-2e3495f8/index.html` |
| `production-boundary` | `process` | PASS | **PASS** | `.build/desktop-e2e/20261125T102709Z-4bc92503/index.html` |

### 10.3 三平台交叉编译与依赖边界

- `GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build ./app/service/cmd/unic ./app/service/cmd/unicd ./app/desktop/cmd/unic-desktop`：**PASS**
- `GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build ./app/service/cmd/unic ./app/service/cmd/unicd ./app/desktop/cmd/unic-desktop`：**PASS**
- `GOOS=darwin GOARCH=arm64 CGO_ENABLED=0 go build ./app/service/cmd/unic ./app/service/cmd/unicd ./app/desktop/cmd/unic-desktop`：**PASS**
- `go list -f '{{join .Deps "\n"}}' ./app/service/cmd/unic ./app/service/cmd/unicd | grep -E 'mygo|app/desktop'`：**零匹配（PASS）**
- `python3 app/desktop/e2e/run.py catalog --check` 与 `python3 app/desktop/e2e/run.py list --shard 1/6`：**PASS**

### 10.4 原生系统剪贴板与跨应用粘贴验收（显式授权 UTM 虚拟机）

- **N01a（Linux Wayland 无托盘/无全局快捷键/无剪贴板管理器真实选区保活、Copy 降级与租约释放 — Fedora 44 `kwin_wayland` 6.8.2）**：**PASS**（证据目录 `.build/0032-vm-acceptance/n01-fedora-wayland/`，`SHA256SUMS` 10/10 文件校验 OK）
  - **虚拟机与纯净会话标识**：UTM Fedora 44 Workstation (`linux/aarch64`，内核 `6.19.10-300.fc44.aarch64`，VM UUID `247DFAC1-4D83-4AE1-93B1-47642E7A7939`，`systemd-detect-virt --vm` = `qemu`)；独立 `dbus-run-session` + `kwin_wayland --virtual --socket wayland-n01`（`kwin 6.8.2`），`WAYLAND_DISPLAY=wayland-n01`、`DISPLAY` 显式置空；测试用户进程表中零 `spice-vdagent`、零剪贴板管理器、零托盘宿主。
  - **已验证项（15/15 全部通过，含加入 Wayland 自动粘贴实现后的回归复测）**：
    1. 无测试标签生产二进制 `unic-desktop --quick-panel` 连接独立 `unicd`，进程表严格仅含 `unicd` 与 `unic-panel`；重复调用 `--quick-panel` 聚焦而不隐藏、不替换 PID。
    2. 无 `zwp_virtual_keyboard_manager_v1` 且无活跃 `RemoteDesktop` portal 后端时平滑降级为 Copy-only，按 `Enter` 不篡改真实 Wayland 剪贴板。
    3. 窗口关闭至 0 可见窗口后（无剪贴板管理器），独立 `wl-paste` 精确读回 UTF-8 中文文本（`1023e2f5...`）、PNG 图片（12,069 B，`278fd13f...`）及配对对端接收的默认托管缓存文件 `text/uri-list`（`ca3bb383...`）。
    4. F13 租约闭环：持有文件选区期间删除历史记录，磁盘缓存文件保持存在且可读；外部 `wl-copy` 替换后，无窗口 `unic-panel` 触发 `OnRelease` -> `/v1/desktop/panel-apply/release`，磁盘托管缓存文件立即被回收（`stored_bytes: 0, files: []`）。
    5. `Escape` 关闭不改写剪贴板；与 `tray` 并存复制零历史回环；`SIGTERM` 停止 `tray` 后独立 `unicd` 与 `unic-panel` 存活且再次复制 PNG 校验一致。
  - 另在 `kwin_wayland` + `XDG_CURRENT_DESKTOP=KDE` 激活 `xdg-desktop-portal-kde` 后验证了 `org.freedesktop.portal.RemoteDesktop` 属性探测（`version = 2`, `AvailableDeviceTypes = 7` 含键盘位）；KDE/GNOME 交互式远程控制授权弹窗真机点击留待人工桌面会话验收。
- **N01b（Linux Wayland 真实跨应用自动粘贴 — Fedora 44 `niri 25.11`，`zwp_virtual_keyboard_v1`）**：**PASS**（证据目录 `.build/0032-vm-acceptance/n01b-fedora-niri-autopaste/`，`SHA256SUMS` 5/5 文件校验 OK）
  - **环境**：UTM Fedora 44 (`linux/aarch64`, VM UUID `247DFAC1-4D83-4AE1-93B1-47642E7A7939`) 内真实 `niri 25.11` 纯 Wayland 会话（`WAYLAND_DISPLAY=wayland-1`, `DISPLAY` 置空），无托盘、无剪贴板管理器；独立外部 Wayland GTK3 `Gtk.TextView` 编辑器作为接收目标。
  - **已验证项**：
    1. Linux `aarch64` 原生单元测试 `TestWaylandPlatformLifecycle`、`TestWaylandPortalTokenPersistence`、`TestWaylandVirtualKeyboardWireProtocol`、`TestControllerPastePreparerConsentAndAbort`、`TestWaylandVirtualKeyboardLive` 全部 **PASS**。
    2. 外部 Wayland 编辑器聚焦时通过 `unic-desktop-e2e --quick-panel` 打开独立面板，底部正确显示 `粘贴到 原应用 ⏎`（`01-niri-wayland-paste-ready.png`）。
    3. 按 `Enter` 后：面板写入真实 Wayland 剪贴板、隐藏窗口并让 `niri` 将焦点归还给外部编辑器、确认窗口失焦且剪贴板未篡改后，经纯 Go `zwp_virtual_keyboard_v1`（`libxkbcommon` 标准 XKB 键表）发送单次 `Ctrl+V`；外部 Wayland 编辑器精确单次插入 `"Wayland Niri 自动粘贴验证 🚀"`（SHA-256 `e422505f...`），独立 `wl-paste` 同步一致。
    4. 按 `Ctrl+Enter`（粘贴并保持打开）后：外部 Wayland 编辑器精确追加插入第二条 `"Wayland 第二条保持打开粘贴"`（累计文本 SHA-256 `ee538ea0...`），面板在 150ms 后自动重新打开并保持焦点（`02-niri-wayland-reopened-after-ctrl-enter.png`）。
  - **收尾清理**：已清除 Fedora 虚拟机内 `/tmp/unic-0032-vm` 临时目录，并正常停止虚拟机。
- **N02（Linux 原生 X11 真实系统剪贴板与跨应用自动粘贴 — Fedora 44 `Xvfb +extension XTEST` + 非 root `Openbox 3.6.1`）**：**PASS**（9/9 项全过；证据目录 `.build/0032-vm-acceptance/n02-fedora-x11/`，`SHA256SUMS` 6/6 文件校验 OK；签入驱动 `app/desktop/e2e/testdata/standalone-quick-panel-real/linux_x11_e2e.py`）
  - **环境**：UTM Fedora 44 (`linux/aarch64`, VM UUID `247DFAC1-4D83-4AE1-93B1-47642E7A7939`) 内独立 `dbus-run-session` + `Xvfb :97 -screen 0 1280x800x24 +extension XTEST` + 非特权解包 `Openbox 3.6.1`（`WAYLAND_DISPLAY` 显式置空、`XDG_SESSION_TYPE=x11`、`GDK_BACKEND=x11`，无 `spice-vdagent`）。
  - **已验证项（9/9）**：生产 `unic-desktop --quick-panel` 独立运行（仅 `unicd` + `quick-panel`）、重复 `--quick-panel` 保活聚焦不换 PID、物理 `XTest` `Return` 单次跨应用粘贴进外部 PyGTK3 `Gtk.TextView`（`"N02 X11 独立快捷面板自动粘贴验证 🚀"`，SHA-256 `3c01a6be...`）、关窗后独立 X11 读回 PNG（12,809 B，`507cb955...`）与托管缓存文件 `text/uri-list`（`9a6f90d1...`）、F13 删历史后外部 `xsel -ib` 替换释放文件租约并回收缓存（`stored_bytes: 0`）、托盘共存零回环与 `SIGTERM` 退出托盘保活。已清理临时目录并停止 Fedora 虚拟机。
- **N03（Windows 11 ARM64 真实 OLE 剪贴板、`AllowForeground` 与跨应用粘贴 — Parallels `Windows 11`）**：**PASS**（10/10 项全过；证据目录 `.build/0032-vm-acceptance/n03-parallels-win11/`，`SHA256SUMS` 6/6 文件校验 OK；签入驱动 `app/desktop/e2e/testdata/standalone-quick-panel-real/windows_e2e.ps1`）
  - **环境**：Parallels `Windows 11` (`windows/arm64`, UUID `{32affa85-5174-4d5b-ab6b-903a06d7fa9b}`, `Shared clipboard mode: off`，交互式控制台会话 Session 1），交叉编译未加测试标签的生产 `unic.exe` / `unicd.exe` / `unic-desktop.exe`。
  - **已验证项（10/10）**：冷启动独立 `unic-desktop.exe --quick-panel`（仅 2 进程）通过 `AllowForeground` 获 Win32 前台焦点、重复调用保活不换 PID；物理 `SendInput` `Enter` 向外部最大化 WinForms `TextBox` 单次精确插入 `"Windows 11 快速面板自动粘贴验证 #N03"`（SHA-256 `ecba1566...`）且 **`IsZoomed` 保持 `true` 不退化**；待命热重入复用同 PID 且获前台焦点；将外部目标最小化（`SW_MINIMIZE`）后按 `Enter` 自动 `SW_RESTORE` 并精确粘贴；无窗 OLE 剪贴板独立 STA 读回 PNG（`b8f642bc...`）与托管文件 `FileDropList`（`185f2e09...`）；F13 删历史后独立进程外部 OLE 替换触发 `WM_CLIPBOARDUPDATE` 释放文件租约并回收缓存（`stored_bytes: 0`）；托盘共存零回环与托盘单独退出保活并继续正常复制 PNG。已清理 `C:\ut0032` 并停止 Windows 11 虚拟机。
- **N04（macOS 26.5 ARM64 真实 `NSPasteboard` 按需启用与租约释放 — Parallels `macOS`）**：**PARTIAL（真实 `NSPasteboard` 10/10 项 PASS；辅助功能跨应用 `Cmd+V` 粘贴待虚拟机 `Terminal.app` 授权）**（证据目录 `.build/0032-vm-acceptance/n04-parallels-macos/`，`SHA256SUMS` 6/6 文件校验 OK；签入零依赖 Go 驱动 `app/desktop/e2e/testdata/standalone-quick-panel-real/macos_e2e.go`）
  - **环境**：Parallels `macOS` 26.5 (`Build 25F71`, `darwin/arm64`, UUID `{fcde9fbb-9a92-4d5e-a19c-33cd0e68affe}`, `Shared clipboard mode: off`)，生产 `unic` / `unicd` / `unic-desktop`。
  - **已验证项（10/10）**：启动前预置 `NSPasteboard` 基线不被独立无头 `unicd` 捕获（历史 0 条）；生产 `unic-desktop --quick-panel` 独立运行获前台焦点且重复调用保活不换 PID；按需调用 `EnableDesktopClipboard` 开启真实 `nativeboard` 且不广播启动前基线；真实 `NSPasteboard` 写入及独立 `pbpaste`/`AppKit` 回读 UTF-8 中文文本（`9b12d682...`，含 `/v1/desktop/clipboard/match` 校验）、`public.png`（`9adace0d...`）与托管文件 `public.file-url`（`6cb00eb2...`）；F13 持有期间删历史不删磁盘文件，外部 `pbcopy` 替换后释放租约并回收缓存（`stored_bytes: 0`）；托盘共存零回环与 `SIGTERM` 退出托盘保活；另实测在给 `macos-e2e` 单独授权后，通过生产 `unic-desktop --quick-panel` 按物理 `Cmd+C` 可正常触发按需 `NSPasteboard` 写入。
  - **未验证边界**：macOS 26.5 TCC 系统策略硬拒绝 SSH 会话持有辅助功能权限（`responsible=/usr/libexec/sshd-keygen-wrapper`, `Auth Right: Denied (System Set)`），完整 `TextEdit.app` 跨应用 `Cmd+V` 自动粘贴（`macos-e2e --require-accessibility`）待在虚拟机内一次性授权 `Terminal.app` 后经 `open -a Terminal` 补跑。已清理 `/tmp/unic-0032-mac` 并停止 macOS 虚拟机。
