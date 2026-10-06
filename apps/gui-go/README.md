# Go GUI 原型

Wails v3 外壳的最小端到端原型，保留 Rust daemon/Engine，复用现有 React 与 HTTP/WS 客户端。
固定版本为 `v3.0.0-beta.28`，仅本地评估，不加入生产打包或替换 Tauri。

## 编码前验证设计

| 失败方式 | 端到端检查 | 工件 |
| --- | --- | --- |
| 错连真实 profile、读写真实剪贴板或钥匙串 | 启动必须显式设置独立 profile、临时 HOME、禁用系统剪贴板；仅测试文件密钥设施 | 环境安全断言、进程与路径摘要 |
| daemon 短暂未就绪被误当不存在、重复启动 | 复用 CLI 已有发现/PID 验证；等待真实 daemon 健康；启动第二 GUI 复用同一 PID | PID/版本/健康断言 |
| 认证无法跨 Wails binding 或 bearer 泄露 | native 交换 JWT，React 直接 HTTP/WS；前端只获取短期 session；不输出凭据 | 真实认证请求成功、前端状态、脱敏日志 |
| React 仅在浏览器能运行、native binding/WS 失效 | 在真实 Wails WebView 中读取 daemon HTTP 与 WS 快照 | 原生窗口截图、前端就绪/HTTP/WS 断言 |
| 多窗口不同步、关窗误停后台 | 从主窗口打开第二窗口、读取相同服务状态；退出 GUI 后检查 daemon 仍在 | 多窗口与 daemon 存活断言 |
| Go 公共代码提取导致 CLI 回归 | 编译原 CLI，运行隔离环境下已有帮助/daemon E2E；不新增单元测试 | CLI E2E 结果与构建来源 |
| 原型无法复跑 | 一条命令构建与执行，记录源码/依赖/产物哈希 | manifest、assertions、SHA256SUMS |

原型只绑定最小宿主服务，不把业务逻辑搬进 Go，不复制 Engine 持久化/加密规则。
共享代码从 Go CLI 移至 `packages/desktop-host-go`，CLI/GUI 使用同一实现。
PoC 结束后只能选择继续达到完整功能验收或删除该入口；不建立长期 Tauri/Wails 双实现切换层。

## 当前范围

已验证：真实 daemon 启动与复用、认证、HTTP/WS、共享 React 主界面（设置、解锁、历史、设备、设置页）、
主窗口关闭隐藏与重开。第二窗口：真实 updater（dev 预览）与 quick panel 页面经多页构建加载，Go 宿主负责窗口创建、两阶段显示、失焦隐藏与尺寸；WebView 面板由 `app.GlobalShortcut` 全局快捷键切换（Windows、无原生面板辅助进程时；见“Wails 能力审计”）。托盘与退出语义：托盘菜单（同步开关、打开、设置、检查更新、重启、轻量模式、退出，六种语言标签）；普通退出（托盘退出、Cmd-Q）停止 daemon，轻量模式与重启保留 daemon。更新服务（`internal/update`）：同一份 Tauri 更新清单格式、minisign 签名校验（含 trusted comment）、下载进度与取消、macOS 原位安装并重启；公钥构建时从 Tauri 更新配置注入，E2E 构建才允许用本地清单与临时密钥覆盖。后台更新调度已实现（含系统唤醒补检查与 macOS App Nap 补检查，见“Wails 能力审计”）。Windows 生产形态（第 17b 片，仅编译与离线核对，无 Windows 运行证据）：生产入口、`TerminateProcess` 停止 daemon、NSIS 原位更新调用、旧 Tauri `Run` 项清理、双击修饰键监视器、NSIS 安装包与便携包脚本，见“Windows 生产形态（17b）”。尚未实现：Linux 原位安装、设备同步子菜单、轻量模式通知、更新、通知、文件预览协议、
原生粘贴与 GPUI 宿主、Windows/Linux 原生验收。

## 开发运行（对应 `bun tauri:dev`）

```sh
bun wails:dev                    # profile 为 dev，等价于 bun tauri:dev
bun wails:dev:profile <profile>  # 其他开发 profile，等价于 bun tauri:dev:profile
```

脚本 `scripts/wails-dev.mjs` 依次构建 daemon 与 Go 宿主（不带 `production` 标签），启动 Vite 开发服务器
（按 profile 取固定端口，被占用则换端口）并让应用通过 `FRONTEND_DEVSERVER_URL` 加载它，前端修改即时热更新；
Go 或 daemon 改动需重启命令。使用你自己的 HOME 与指定 profile，数据、钥匙串条目和 daemon 与正式版隔离。
宿主只在 `UNICLIPBOARD_ENV=development` 且设置了合法 `UC_PROFILE` 时启动；测试用的隔离模式
（`UC_GUI_GO_ISOLATED=1`，临时 HOME、`gui-go-*` profile、禁用系统剪贴板）仍由 E2E 强制。

## 运行与复跑

在仓库根目录运行（本轮仅验证 macOS）：

```sh
bun install --frozen-lockfile
apps/gui-go/run.sh
```

启动脚本会构建原生应用包、创建 `uc-gui-go-*` 临时 HOME 与 `gui-go-*` profile。
可手工刷新会话、打开第二窗口。GUI 本身退出不会停止 daemon；启动脚本在演示结束后
通过同一隔离环境下的 CLI 停止测试 daemon，并保留临时 HOME 供排查。
禁止直接打开应用包连接生产 profile；启动守卫会拒绝非隔离配置。

```sh
apps/gui-go/e2e/run.sh target/gui-go/evidence
```

这条命令构建带 `e2e` 标签的独立应用包，在真实 WebView 内由 `frontend/src/e2e-driver.ts`
操作共享 React 页面并逐步上报断言：共享应用挂载、WS 状态快照、首次启动在 GUI 内创建空间、
设备/设置导航与返回、主窗口关闭后隐藏并重开且 WebView 存活；第二次启动经钥匙环解锁并重复。
两次启动必须复用同一 daemon PID，GUI 退出后 daemon 仍健康，最后只停止本次 PID 并验证退出。
`assertions.json`、`native.jsonl`、窗口截图与 `build-hashes.json` 形成工件。
测试控制面（`EvidenceService`）与驱动脚本只存在于 `e2e` 构建；普通构建产物不含二者。

### 剩余 E2E 缺口（第 14 片）

均为 quiet 模式、隔离 HOME 与唯一 profile、`UC_DISABLE_SYSTEM_CLIPBOARD=1`，只停止本次启动的 PID，工件写入 `--out` 目录：

| 场景 | 命令 | 断言 |
| --- | --- | --- |
| 错误口令解锁（密钥丢失） | `e2e/unlock_run.py --mode key-deleted` | 删除测试 HOME 内的文件钥匙环条目后，GUI 进入“恢复本机资料”页；错误口令得到本地化的“口令错误”提示且内容仍锁定，正确口令解锁；再次启动经钥匙环解锁，恢复前写入的历史仍在，钥匙环已被写回 |
| 错误口令解锁（钥匙串被拒） | `e2e/unlock_run.py --mode keychain-denied` | 解锁页的口令表单：错误口令被拒、仍锁定，正确口令解锁。钥匙串拒绝由 e2e 构建的宿主边界注入（`UC_GUI_GO_E2E_KEYRING_UNLOCK=denied`），口令校验仍走真实 daemon |
| 实时历史更新 | `e2e/history_live_run.py`（需网络） | 对端经生产 rendezvous 配对并发送文本；打开中的历史页在同一文档、同一路由内出现新卡片，并记录布防后收到的 WebSocket 帧（`incoming_pending`、`new_content`、`inbound_notice`）。本机 `uniclip send` 也会让列表出现新卡片，但未观察到稳定的 WS 帧，故不作为证据 |
| 收到的单图 | `e2e/single_image_ui_run.py`（需网络） | 真实 DOM 中单图从 `blob:`（daemon 字节）解码，且没有 `/host-file` 请求 |
| 生产更新公钥路径 | `e2e/updater_key_run.py` | 生产构建（`build.sh manual`）链接的公钥等于 Tauri 配置、无 e2e 控制面与环境覆盖字符串；Go 默认 feed 与 Rust 外壳一致；以本地 feed 验证：生产公钥拒绝其他密钥的签名，空公钥失败关闭且零请求，对照组接受自己的签名并拒绝别人的 |

`/host-file` 的 UI 消费者只有“单条目多图”的缩略图网格；daemon 一次只派发一个文件、目录发送被拒绝，多文件条目只能来自系统剪贴板的多文件复制，而隔离运行禁用系统剪贴板。因此该 UI 路径 **未经端到端验证**；路由本身由 `file_preview_run.py` 覆盖（直接请求与拒绝路径）。生产公钥只证明“注入与使用路径正确”，没有生产私钥与发布源访问，**未验证它能校验真实发布签名**。

## 前端源码共享

`vite.config.ts` 的 `@` 直接指向 `apps/gui/src`，整个 React 应用（页面、状态、HTTP/WS 客户端、
生成的 SDK、样式）原地复用，没有第二份源码。仅宿主边界被替换：8 个 `@tauri-apps/*` 模块别名到
`frontend/src/host/` 的 Wails 适配（`invoke`、`listen/emit`、窗口、打开链接等）。
命令统一经 Go `HostService.Invoke` 与显式命令表 `commands` 路由；未实现命令立即返回结构化错误。
`apps/gui/src` 不含任何平台分支，`ipc.ts` 与生成绑定保持原样。
`frontend/index.html` 仅是入口壳。路径别名无需 Git 软链接，普通 Windows 检出不受软链接权限影响；
Go 只嵌入构建产物 `frontend/dist`，不嵌入源码。

未实现命令清单：`apps/gui-go/e2e/command-coverage.sh`。

更新 E2E：`apps/gui-go/e2e/update_run.py --out <dir>`（需先 `build.sh e2e`）在安装副本上运行：
不可信签名必须被拒绝且包不变；可信签名则下载、校验、停止旧 daemon、替换自身包、重启，
重启后的进程发现更新标记；旧进程与旧 daemon 均退出。

文件预览（对应 Tauri 的 `asset://` 协议）：共享前端用 `convertFileSrc(路径)` 显示文件条目的本机图片，宿主适配把它
映射为 `/host-file?path=…`，由 Go 资源中间件提供。只服务同时满足：绝对且已清理的路径、图片扩展名、且该路径出现在
daemon 历史条目（`GET /clipboard/entries` 的 `file://` 预览）里；历史被内容锁封住时不服务；响应带 `nosniff` 与沙箱 CSP，
单文件上限 64 MB。路径集合有 5 秒缓存，未命中最多每秒重建一次。收到的图片文件目前由前端回退到 daemon 字节（blob URL）
显示；本路由主要覆盖本机原文件与已落盘的缓存路径。双端 E2E：`apps/gui-go/e2e/file_preview_run.py --out <dir>`
（需网络访问配对服务）。

### 启动模式与 quiet E2E

`startup.go` 复刻 `crates/uc-desktop/src/startup/actions.rs` 的冷启动序列：补全设备名；仅当本次启动拉起了 daemon
（冷启动）时才做加密会话恢复与“恢复最近一条剪贴板”；复用已有 daemon（重开）时跳过两者。静默（Silent）与轻量
（Lightweight）模式启动时不创建主窗口（隐藏的 Wails 窗口从未运行过时 `Show()` 不生效，因此主窗口改为首次需要时
懒创建）；静默模式发送一条原生通知；轻量模式冷启动进入后台运行，轻量重开则显示窗口。

E2E 默认是 quiet 模式：Accessory 激活策略、窗口停放在屏幕外、指针位置由测试注入、窗口摆放只记录不应用、不抢焦点、
不移动真实鼠标；`UC_GUI_GO_E2E_VISIBLE=1` 恢复可见模式。quiet 模式下的截图来自窗口自身内容，不能证明视觉位置、
真实焦点或系统快捷键；这些仍需可见模式的人工/专用主机验收。复跑：

```sh
apps/gui-go/e2e/run.sh <dir>                              # 主流程
python3 apps/gui-go/e2e/startup_run.py --out <dir>        # 启动模式
python3 apps/gui-go/e2e/quick_panel_settings_run.py --out <dir>
python3 apps/gui-go/e2e/scheduler_run.py --out <dir>
python3 apps/gui-go/e2e/update_wake_run.py --out <dir>    # 唤醒守卫 + 更新 analytics（约 4 分钟）
python3 apps/gui-go/e2e/single_instance_run.py --out <dir> # 单实例：第二次启动、scope 隔离、重启与接管（约 2 分钟）
```

宿主的 daemon 客户端（`packages/desktop-host-go/daemonclient`）现在缓存会话令牌至刷新时间，401 时重新交换一次。
此前每个请求都交换令牌，会触发 daemon 对 `/auth/connect` 的每 IP 每分钟 100 次限流（429），表现为主流程间歇失败。

## 更新唤醒与 analytics

调度行为对照 Tauri（`crates/uc-tauri/src/update_scheduler`）：启动后等 setup 完成再立即检查一次，之后成功 6h ± 15min、失败 30min；系统唤醒（`Common.SystemDidWake`）仅在距上次任意来源的检查不少于 1 小时时补一次检查，被跳过的唤醒不改变周期计时器，连续多次唤醒合并为一次；手动、托盘与调度的检查都会刷新 `lastCheckAt`；`autoCheckUpdate` 关闭时是空闲成功（无请求、无 analytics）。

analytics 沿用 daemon 的 `POST /analytics/capture`（daemon 是唯一发送方并执行“使用情况统计”同意开关，GUI 不连接任何分析后端，也不判断同意）。共享前端已上报 `dialog_opened`、`dismissed` 与 UI 侧 `action_invoked`；Go 原生侧补齐 Tauri 原生侧发送的三类，名称、字段和顺序与 Tauri 一致：

| 事件 | 触发 | 字段 |
| --- | --- | --- |
| `check_performed` | `check_for_update` 命令与托盘检查（`manual`）、调度（`scheduled`）；调度在副作用（通知、自动下载）之后发送；`autoCheckUpdate` 关闭或读取设置失败时不发 | `source`、`outcome`（`available`/`up_to_date`/`failed`）、失败时的 `failure_kind`、`install_kind` |
| `notification_shown` | 更新窗口因新版本打开且已去重之后 | `version`、`delivery_status`（Wails 创建窗口不报告失败，恒为 `sent`）、`install_kind` |
| `action_invoked`（`download_bg`） | `download_update` 命令与自动下载：开始、终态各一条；前置条件拒绝（含已下载完成、正在下载）不发 | `action`、`outcome`、`error_kind`（仅失败时为 `download_failed`） |

`failure_kind` 与 Tauri 不同：Tauri 对错误文本做子串猜测（例如端点 URL 含 `.json` 会被误判为解析错误），Go 由 `internal/update` 的类型化错误直接给出 `network` / `http_error` / `parse_error` / `other`（签名无效归 `parse_error`，与 Tauri 一致）。

托盘检查与 Tauri 对齐：发现新版本时走“按版本去重、不受冷却限制”的通知路径（会上报 `notification_shown` 并记录已通知版本）；用户主动点击时，已通知或已跳过的版本仍会静默打开窗口（这是 Go 既有的显式行为，Tauri 在该情形下不打开）。

隔离收集端：E2E 使用 debug 构建的 daemon，其分析汇是 `StdoutSink`（写入 daemon JSON 日志，target `uc_observability::analytics`），没有 PostHog 汇，因此测试事件不会离开本机；`update_diag` 行不受同意开关影响，证明 GUI 已发出，`StdoutSink` 行只在通过同意开关后出现。E2E 不设置 `POSTHOG_PROJECT_KEY`。

## Wails 能力审计

设计原则：**Wails 已有的能力就集成，不重复造轮子**。每个宿主能力先查固定版本（`github.com/wailsapp/wails/v3 v3.0.0-beta.28`）的源码，确认存在再用；官方文档描述的是当前主线，不代表固定版本已有该 API，必须以模块源码为准，需要更高版本时先评估受控升级，不直接跳到 main。

下表证据均来自 beta.28 模块源码 `pkg/application`、`pkg/events`、`pkg/services`。

| 能力 | Wails beta.28 API（源码证据） | 当前实现 | 适配缺口 | 替换切片 / 验收 |
| --- | --- | --- | --- | --- |
| 开机自启 | `app.Autostart`：`Enable` / `EnableWithOptions` / `Disable` / `IsEnabled` / `Status`；选项 `Identifier`、`Arguments`；macOS 打包且 ≥13 用 `SMAppService`，否则（含裸二进制）写 LaunchAgent，Windows 写 `HKCU\...\Run`，Linux 写 XDG `.desktop` | **已替换（slice 14b）**：`autostart.go` 只保留 Uni 适配层，直接调用 `app.Autostart`；已删除 `packages/desktop-host-go/autostart`（无其他消费者，CLI service 的 systemd/launchd 不受影响） | 适配层保留：偏好持久化优先与失败回滚、启动对账（已注册则不重复注册）、`Disable` 无条件调用、`--autostart` 标记（仅 LaunchAgent 路径生效）、旧 Tauri 登录项清理（只删本登录项同名且指向其他可执行文件的 plist，不 bootout）。**Wails 能力缺口（beta.28 源码）**：① `SMAppService` 路径 `Identifier` 只校验、`Arguments` 被丢弃，登录项归整个 bundle，`Identifier` 无法隔离 profile；② LaunchAgent 的 `Status`/`Disable` 按可执行路径而非 label 匹配；③ LaunchAgent 启用会 `launchctl bootstrap` 立刻拉起一个实例（不受 Wails 选项控制）。对策：具名 profile 在 bundle 内运行时拒绝改动系统登录项（仅隔离测试构建可放行）；同一二进制的多个 profile 共享 LaunchAgent 仅靠路径匹配，不声称相互隔离 | 证据 `library/e2e-autostart-wails/`：重复启停、`Status.Strategy`（`launchagent` / `smappservice`）、plist 记录、启动对账、旧项清理、回滚、profile 守卫、测试 bundle 的 `SMAppService` 注册与清理。**未验证**：真实注销/登录后自动启动；`SMAppService` 的独立（BTM）核对；Windows/Linux 机制（slice 17） |
| 通知 | `services/notifications`（`NotificationService`） | 已集成（`host_notifications.go`） | 仅 Tauri 通知插件语义适配（权限、点击回调事件） | 已覆盖，无替换 |
| 全局快捷键 | `app.GlobalShortcut`（`Register`/`Unregister`/`UnregisterAll`/`IsRegistered`/`GetAll`；Windows 为 `RegisterHotKey`，注册在应用的主线程窗口，被占用时 `Register` 返回错误；Wails 在 shutdown 钩子之后自动 `UnregisterAll`；其修饰键只认 `cmdorctrl/cmd/command/ctrl/optionoralt/alt/option/shift/super`，不含 `meta`/`control`；不提供两步 chord，也不提供双击修饰键） | **第 17a 片**：`global_shortcuts.go`。无原生面板辅助进程（Windows，及 `UC_GPUI_QUICK_PANEL=0`）时由宿主注册 `global.toggleQuickPanel`（默认 Windows/Linux `ctrl+alt+v`，macOS `super+ctrl+v`）；macOS 默认仍由 GPUI 辅助进程持有快捷键，路径不变 | 只保留 Uni 契约：设置值（字符串或列表）与默认值、物理键归一（`meta`→`super`、`mod`/`cmd`→平台主修饰键、`control`→`ctrl`）、两步 chord 与同键双击（1 s 窗口，对应 `shortcut_registry.rs`）、卸旧→防御性卸新→装新→失败回滚（对应 `update_shortcuts`）、OS 拒绝时返回 `Conflict`；`update_keyboard_shortcuts`/`set_quick_panel_enabled` 先改 OS 绑定、再保存设置、保存失败再回滚；`--quick-panel` 第二次启动与快捷键共用 `QuickPanelToggleController` 等价物（就绪前的请求按奇偶挂起）。Linux Wayland 的合成器托管归 17c；双击修饰键见下一行（17b，Wails 不提供，Windows 靠 `GetAsyncKeyState` 轮询而非键盘钩子） | 证据 `library/e2e-windows-17a/mac-shortcut/`（`webview_panel_shortcut_run.py`，macOS quiet、`UC_GPUI_QUICK_PANEL=0`）：**控制器断言 17 项通过**（真实 Wails/Carbon 注册、换绑、无法解析的键被拒绝并保持旧绑定与设置、chord 与双击时序、禁用/重新启用、真实第二进程 `--quick-panel`、退出）。**注入**：未产生任何键盘事件，`shortcut-press` 直接调用处理函数；macOS Carbon 不报告被其他进程占用的热键（实验：两个进程都得到 0），所以此处不证明冲突。**原生可见性断言 3 项未验证**（主显示器休眠，仅相关性；`lastShown` 只证明控制器发起了显示，不替代窗口显示契约）。第一次运行（未带 `lastShown` 时）4 项可见性相关检查失败，原始文件已被重跑覆盖，仅留文字记录。Windows 的真实按键、冲突、退出后释放：`e2e/windows_quick_panel_run.py` 已编写、**未执行**（无主机） |
| 粘贴到前一个应用 | Wails beta.28 `pkg/w32` 有 `SetForegroundWindow`/`SetFocus`/`GetForegroundWindow`/`GetAsyncKeyState`；`x/sys/windows` 有 `GetGUIThreadInfo`/`GetWindowThreadProcessId`/`IsWindow`；**缺口**：`AttachThreadInput`，以及 `SendInput`（`pkg/w32/user32.go` 中仅为注释掉的 cgo 残留）。没有 Wails 的“向其他应用发送按键/记录前台窗口”API | **第 17a 片**：`previous_app_windows.go` 只桥接 `AttachThreadInput` 与 `SendInput`（`x/sys` 的 `NewLazySystemDLL`，无 cgo），其余用上述已有 API。语义逐项对应 `crates/uc-tauri/src/quick_panel/windows.rs` 与 `mod.rs`：显示面板前记录前台窗口及其内层焦点子窗口；恢复时（主线程）还原最小化、`AttachThreadInput`+`SetForegroundWindow`+`SetFocus`、再下沉到内层子窗口，记录用后即清；`paste_to_previous_app` 等 40 ms、最多 120 ms 等 Alt 释放（仍按下则前后中和）、发 Ctrl+V，不写剪贴板；`type_file_paths_to_previous_app` 校验非空，用 `\n` 连接，逐 UTF-16 单元发 Unicode 键事件，不经剪贴板；失败（含无记录窗口）重新显示面板并返回字符串错误。非 Windows 一律返回“not yet supported”，**绝不空成功**（macOS 由原生面板辅助进程粘贴，其行为不变） | 未调研 Go 生态中的其他按键模拟库（`robotgo` 需要 cgo 工具链，`keybd_event` 无 Unicode 输入）；仅按“最小桥接 + 已有依赖”选择。Windows 真实运行未验证；macOS 上仅验证了拒绝语义（`mac-shortcut` 工件） | 第 17a 片：编译为 Windows（见下）；`windows_quick_panel_run.py` 覆盖前台还原、路径输入、粘贴与失败重显示，**未执行** |
| 原生对话框 | `app.Dialog`（OpenFile / SaveFile / Info / Error） | 已集成 | 无 | 已覆盖 |
| 托盘与菜单 | `app.SystemTray`、`app.NewMenu` | 已集成（`tray.go`、`tray_devices.go`） | 仅 Uni 业务菜单内容 | 已覆盖 |
| 单实例 | `application.Options.SingleInstance`（`single_instance_*.go`；darwin 为 `flock` 锁文件 + `NSDistributedNotificationCenter`，Linux 为 D-Bus，Windows 为命名互斥体 + 窗口消息） | **已启用（单实例切片）**：`single_instance.go` 只保留 Uni 语义适配，锁与激活消息全部由 Wails 完成。生命周期：`application.New`（取锁；第二个同 scope 进程在此交出参数并以 0 退出）→ `app.Run` 启动事件循环（Wails 此时才注册激活观察者）→ `ApplicationStarted` 后才做 daemon 仲裁/启动、窗口、托盘、GPUI helper、登录项对账、冷启动序列、更新调度。所以第二个进程不接触 daemon/数据/登录项，且数秒的冷启动 daemon 拉起位于观察者之后，期间的激活不会丢失。`UniqueID` = bundle id + `UNICLIPBOARD_ENV` + profile + 数据根哈希（darwin 锁文件在 `NSTemporaryDirectory()` 而不在 HOME，临时 HOME 不隔离锁，隔离完全靠 ID）；E2E 构建的 bundle id 带 `.e2e`，与正式应用互不串扰。回调语义：普通第二次启动 → 首实例 `showMainWindow`（Tauri 契约；启动尚未完成时先挂起，冷启动序列结束后执行一次，轻量模式冷启动遇到挂起的请求则改为显示窗口而不退到后台）；`--autostart` → 仅记录、不弹窗（登录自启不强制弹主窗口，Tauri 未区分，属显式决策）；`--quick-panel` → 切换 WebView 面板（第 17a 片；启动尚未就绪时按奇偶挂起；有原生面板辅助进程时无事可做）；首实例自身带 `--quick-panel` 启动 → 退出码 1（对应 `validate_primary_launch`）。silent/lightweight/autostart 的窗口行为不变。重启：新进程带 `UC_GUI_RESTART_PARENT_PID`，在 `application.New` 之前等旧进程退出再取锁（旧进程退出前仍持锁）。 | 适配层保留：① 回调只做 `go handle…` 转交——**Wails 缺陷（beta.28）**：回调与 darwin 主线程通知处理共用容量为 1 的 `secondInstanceBuffer`，回调内同步做 UI 工作会令突发启动把主线程阻塞在满通道上、同时回调 goroutine 等主线程而死锁（实测：未转交时 8 次突发只送达 2 条，之后所有启动都丢失；转交后 8/8）；② 重启等待旧进程；③ 范围 ID 与参数白名单；④ 启动期挂起并重放窗口请求。**仍存在的 Wails 限制（不手写补丁）**：观察者在 `app.Run` 才注册，`application.New` 到 `Run` 之间（只剩窗口之外的构造，毫秒级，无 daemon）仍无法接收；投递尽力而为（无队列，背靠背的相同消息可能合并）；消息不加密（`EncryptionKey` 为零，同用户任何进程可向该名字发消息，故参数只按白名单解释）。**已观察的原生崩溃**：把窗口重放放在 daemon 引导后立即执行（与托盘、通知、冷启动并发创建窗口）时 `SIGSEGV`（`diagnostic-sigsegv-replay/`），根因未定位；现在重放排在冷启动序列之后，两次完整运行无崩溃，但并发创建窗口的根因仍属未证实。不提供 Tauri 的 `UC_DISABLE_SINGLE_INSTANCE`（生产二进制不带测试开关）。 | 证据 `library/e2e-single-instance/`（`single_instance_run.py`，两次完整通过）：真实第二次启动，首实例 PID / daemon PID / GPUI helper PID / 进程表不变，第二进程约 0.4s 退出码 0 且不写 `launch` 步骤；`--autostart`/`--quick-panel`/普通/8 次突发（8/8）/无首实例的 `--quick-panel`（退出码 1，无 daemon）；冷启动中的早到启动（`--autostart` 静默，普通启动由首实例在启动完成后执行：`bootstrapped.replayedHeldShow=true`、`mainExists=true`，仅 1 个 daemon）；不同 profile、同 profile 名不同 HOME、另一 bundle id 的实例各自独立（4 个不同 `UniqueID`，4 个锁文件，激活只到自己的 scope）；重启后新进程仍是首实例；首实例退出后可接管；清理后无遗留进程，生存时间在 assertions JSON。**注入与未验证**：`--autostart` 由手工传入代替 `launchctl bootstrap`（真实 bootstrap 会在真实 HOME 下起 job，未执行）；“另一应用”是同一源码换 bundle id，不是正式生产二进制（生产入口仍未实现）；Windows/Linux 单实例未验证（slice 17）；未经 sandbox 容器、真实 Dock/Finder 再次点击（`Reopen`）验证；`application.New`→`Run` 毫秒窗口未覆盖 |
| 窗口事件 | `events.Common.WindowClosing`、`WindowLostFocus` 等窗口事件与 hook | 已集成 | 无 | 既有主流程 / 面板 E2E |
| 系统睡眠/恢复（sleep/resume） | `events.Common.SystemDidWake` / `SystemWillSleep`。固定源码的派发路径：macOS `application_darwin.go` 在 `NSWorkspace` 通知中心注册 `NSWorkspaceDidWakeNotification` → `workspaceDidWake:` → `Mac.ApplicationDidWake` → `events_common_darwin.go` 映射为 `Common.SystemDidWake`；Windows `application_windows.go` 的 `WM_POWERBROADCAST` → `Windows.APMResumeAutomatic`（每次恢复都发）→ `events_common_windows.go` 映射为 `Common.SystemDidWake`，`APMResumeSuspend`（仅用户输入触发的恢复后补发）**不** 映射到 Common；Linux `application_linux_dbus.go` 订阅 logind `PrepareForSleep` → `Linux.SystemDidWake` → `Common.SystemDidWake`，无 logind/elogind 时只记 warning 不触发 | **已集成（slice 15）**：`main.go` 只订阅 `Common.SystemDidWake` 一个（平台事件已被重发为 Common 事件，再订阅会重复），回调只向容量为 1 的通道做非阻塞发送；`update_scheduler.go` 的循环用 `lastCheckAt`（墙钟，初值为启动时刻）判断，距上次任意来源的检查不足 1 小时则跳过，且不改动周期计时器；退出时 `shutdown` 先取消订阅再停调度器 | Tauri 契约（`crates/uc-tauri/src/update_scheduler/scheduler.rs` 的 `WAKE_MIN_RECHECK_SECS`）：Windows 监听 `PBT_APMRESUMEAUTOMATIC` 与 `PBT_APMRESUMESUSPEND`，Wails 的 Common 事件只映射前者，而前者每次恢复都会发送，覆盖范围等价。**Linux 在 Tauri 中没有唤醒源**，Go 侧新增，行为未在本机验证。Go 的单调时钟在 macOS 睡眠期间不前进，故必须用墙钟守卫 | 证据 `e2e/update_wake_run.py`：经 Wails 自己的观察者分发链注入唤醒（e2e 构建向 `NSWorkspace` 通知中心发布 `NSWorkspaceDidWakeNotification`，**机器并未睡眠**），listener 被调用的次数记在 GUI 日志中。**未验证**：真实 macOS 睡眠恢复；Windows/Linux 的原生事件（无主机，保持未验证） |
| macOS App Nap | Tauri 契约：`background_activity_macos.rs` 用 `NSBackgroundActivityScheduler`（标识 `app.uniclipboard.update-check`，间隔 6h，容差 10%），在 App Nap 挂起定时器时仍会触发并经同一 Wake 守卫补一次检查。固定源码核查（slice 15c 重跑）：`grep -rIl "NSBackgroundActivity\|beginActivity\|NSActivity\|AppNap\|App Nap" $(go list -m -f '{{.Dir}}' github.com/wailsapp/wails/v3)` 均为 0 个文件，**Wails 不覆盖**；对现有项目与模块缓存的同样检索也没有找到可复用的桥接（这只说明本仓库与已下载模块里没有，不代表整个 Go 生态没有此类库，未做生态调研）；`SystemDidWake` 对应的是系统睡眠恢复，不是 App Nap 退出；本仓库无现成依赖，沿用既有的 cgo 桥接模式（`cursor_darwin.go`、`accessibility_darwin.go`），`Info.plist` 无 `NSAppSleepDisabled` | **已实现（slice 15c）**：`app_nap_darwin.go` 是最小适配，cgo 调 `NSBackgroundActivityScheduler`（标识、重复、间隔取 `defaultSchedulerTiming.activityInterval`=6h、容差 10%，与 Tauri 一致）；回调在系统的 XPC 队列上（非主线程）立即完成，只向与 `SystemDidWake` 相同的 `h.wake` 通道投递来源 `background-activity`，不做任何检查；系统要求延后（`shouldDefer`）时按 Apple 约定回 `Deferred` 且不唤醒调度器。调度循环、`lastCheckAt` 墙钟守卫、`autoCheckUpdate` 开关、setup 门禁、去重与忙碌行为全部复用，没有第二条检查路径。启动在 `bootstrap` 中与调度器一起；`shutdown` 在停止调度循环前 `invalidate`。非 macOS 为空实现（`app_nap_other.go`）：Windows/Linux 没有 App Nap，其恢复事件已由 `SystemDidWake` 覆盖 | 观察：系统在 `scheduleWithBlock` 之后会立即回调一次（与 Tauri 相同），此时守卫正确跳过；现有项目与模块缓存中没有找到可替代的现成依赖。无法证明进程“确实处于 App Nap”（无非特权接口），E2E 的定时器偏差探针只证明定时器延迟这一效应，不证明状态 | `app_nap_run.py`（见 `library/e2e-app-nap/`）：Accessory 离屏进程，空闲下真实系统回调，不注入任何唤醒；生产 6h 间隔不实测，E2E 构建用 `UC_UPDATE_BACKGROUND_ACTIVITY_INTERVAL` 缩短 |
| 应用更新 | beta.28 的 `pkg/services` 仅有 `dock`、`fileserver`、`kvstore`、`log`、`notifications`、`sqlite`，**没有更新服务** | 自研 `internal/update`（Tauri 清单格式、minisign 签名含 trusted comment、macOS 原位替换） | Wails 不覆盖；发布格式与签名契约由现有 Tauri 发布流程决定，故保留自研 | 无替换；第 14 片验证生产公钥注入路径 |

审计规则：以后每个新宿主能力在实现前，先在本表补一行并写出源码证据；已完成的切片按此表回头审计，发现重复实现就列为替换切片，不因“已经写过”而保留。

## Windows（第 17 片，分段交付）

第 17 片按可独立验收的段交付，整体目标不缩减：**17a**（本片）Windows 开发 profile 的 GUI 最小路径、WebView 快捷面板、全局快捷键、粘贴到前一个应用的代码与可复跑工件；**17b** Windows 生产形态（NSIS 安装与原位更新、`--autostart` 与旧 Tauri 登录项迁移、重启/单实例的 Windows 实测、daemon 在 Windows 上的停止方式、双击修饰键需要低层键盘钩子、生产入口与签名）；**17c** Linux（AppImage/deb/rpm/便携包、X11/Wayland 快捷键、托盘与面板）。三段之后仍需全目标原生审计。

### 17a 范围与状态

| 项 | 状态 |
| --- | --- |
| 入口与环境 | `validateEnvironment` 允许 macOS 与 Windows，仍只接受开发 profile。`daemonproc.IsPidAlive`（原 `isPidAlive`）导出，替换了 `single_instance.go` 里仅 Unix 才有的 `syscall.Kill`，Windows 才能链接 |
| 隔离（Windows） | Go 宿主与 Rust daemon 都用 Windows 已知文件夹解析数据根，**不读 HOME 或 LOCALAPPDATA**，临时 HOME 不会改变任何路径；而 daemon 在 Windows 总是用 Credential Manager（`UNICLIPBOARD_ENV=development` 只在 macOS 强制文件密钥）。所以隔离模式是便携沙箱：把可执行文件复制到 `uc-gui-go-*` 临时目录，`UC_PORTABLE=1`，数据根、缓存和文件密钥都在该目录内；`environment_windows.go` 校验这一点。非隔离的开发 profile 会写 Credential Manager 条目 `UniClipboard-<profile>` |
| 构建 | `e2e/build_windows.py --mode e2e\|production`（Windows 主机上构建 daemon、CLI、前端与 `gui-go.exe`）；`--cross-check-only` 在任何有 Go 的主机上为 windows/amd64 编译 GUI。`apps/gui-go/e2e_bundle_*.go` 把 e2e 构建里的 macOS-only `mac.GetBundleID` 隔离到 darwin。**交叉编译只证明能编译链接，不证明能运行** |
| 面板与快捷键 | 无原生面板辅助进程时用 WebView 面板，默认即 Windows；`app.GlobalShortcut`、前台窗口记录/恢复、失焦后 100 ms 复核再隐藏（对应 Tauri 的 `BLUR_VERIFY_DELAY`，过滤 `AttachThreadInput` 与输入法引起的伪失焦）、面板显示后 `forceForeground`。失焦复核对所有走 WebView 面板的平台生效，包括 macOS 的 `UC_GPUI_QUICK_PANEL=0` 回退路径（默认的 GPUI 路径不受影响），该回退路径的 `native-quick-panel-dismissed` 尚未在显示器唤醒时重跑。详见“Wails 能力审计”两行 |
| 自启（Windows） | Wails 的 Windows `Disable`/`Status` 按 **可执行文件路径** 匹配 `HKCU\...\Run`，所以沙箱里的启动对账不会碰到已安装应用的条目；旧 Tauri 登录项迁移见 17b 一节 |
| 通知 | `notifications` 在 Windows 用 `go-toast`（`go.sum` 本片补上缺失条目，`go.mod` 把 `minisign` 与 `x/mod` 校正为直接依赖）；Windows 上的权限与点击回调行为未验证 |
| daemon 停止 | 17a 时 `daemonproc.Terminate` 在 Windows 调用不带 `/F` 的 `taskkill /PID`；对无窗口、`DETACHED_PROCESS` 的 daemon 它无法结束进程，且从 windowsgui 宿主 shell-out 会闪控制台窗口（Rust 侧早已因此改成 Win32）。**17b 已修**：见下一节 |
| 不在 17a | 双击修饰键（17a 借用 `unsupported_display_session` 占位，17b 已替换为真实监视器）、生产入口、NSIS、原位更新、旧登录项迁移（均在 17b）、Linux（17c） |

## Windows 生产形态（17b）

分支 `hp/uni/t-0188-go-gui-windows-production`，基于 17a。**没有任何 Windows 运行证据**（runner `uniclipboard-windows-x64-vm-01` 2026-10-06 14:39Z 只读查询为 `offline`）。本节“已实现”只表示代码存在、可为 windows/amd64 与 arm64 编译、安装器脚本可编译；下表“证据”只列 macOS 侧或离线能证明的部分。

| 需求 | Tauri 契约 / 固定版 Wails 核查 | 实现 | 证据与未验证 |
| --- | --- | --- | --- |
| 生产入口（R1） | 产品为单 profile（`run.rs` 的 P4-7 决策），便携模式靠 `portable.dat`。Wails 无此概念 | 新构建标签 `release`（`environment_release.go`）：不要求 `UNICLIPBOARD_ENV=development`、不接受 `UC_PROFILE`/daemon 覆盖/隔离测试开关、允许便携；**非 Windows 直接拒绝启动**。`production` 标签保持原意（macOS 手动构建仍是开发构建）。空 profile 的消费者已核对：单实例 ID、登录项名（= `UniClipboard`，与 Tauri 的 Run 值名一致）、`Connection.Profile`、更新测试覆盖（e2e 标签才可达） | macOS 上带 `release` 标签的二进制启动即以 1 退出（`library/e2e-windows-17b/release-tag-refusal/`）；**从未在 Windows 运行**，真实数据根/身份/daemon 路径未验证 |
| daemon 停止（R2） | Rust：`TerminateProcess` + 等待进程句柄（`win_process.rs`）；更新前 Windows 上停不掉就中止安装 | `daemonproc.Terminate` 改为 Win32 `TerminateProcess`；新增 `TerminateAndWait`（Windows 等句柄，Unix 仅发 SIGTERM，与 Rust 一致）；`stopDaemon` 读 `.daemon-pid`，陈旧或 in-process 不动，存活则返回错误 | **这是强制终止，不是优雅关闭**：daemon 无机会自行收尾，与 Tauri 行为相同；终止瞬间的写入一致性依赖 daemon 自身的持久化事务，本片未做数据一致性验证。仅编译与 `go vet`；未运行 |
| 原位更新（R3） | `tauri-plugin-updater` 2.10.1：默认 passive，`ShellExecuteW(setup.exe, "/P /R /UPDATE /ARGS <转义参数>")` 后退出；载荷可为 `-setup.exe` 或 `.nsis.zip`；便携版不自更新 | `internal/update/nsis.go`（参数与转义、载荷提取）、`install_windows.go`（临时目录 + `ShellExecute`）、`host_install_*.go`（Windows：便携拒绝 → 停 daemon（失败则中止）→ 安装 → 仅退出，安装器 `/R` 负责重启；macOS 行为不变） | `e2e/installer_contract`：Tauri 自带转义用例表 + 参数串 + exe/zip/非法载荷共 21 项离线通过（`library/e2e-windows-17b/installer-contract-assertions.json`）。真实 `ShellExecute`、安装器重启应用、单实例交接均未运行 |
| 旧登录项（R4） | Tauri（auto-launch）写 `HKCU\…\Run\UniClipboard`；Wails 的 `find` 按可执行路径匹配，值名取 `Identifier` | 同路径原位升级：Wails 识别旧项，无需处理。路径不同：`legacy_run_windows.go` 在对账时只删 **本登录项同名** 且指向其他 exe 的 Run 值（具名 profile 不碰主项） | 仅编译；`StartupApproved\Run`（任务管理器禁用状态）Wails 与本实现都不处理，是已知限制；多 profile 策略与注销登录后的真实启动仍 OPEN |
| 双击修饰键（R5） | Rust：快照检测器（400 ms 窗口，其他键使 tap 失效）+ 20 ms `GetAsyncKeyState` 轮询；Wails 无此能力，但 `pkg/w32.GetAsyncKeyState` 已有 | `modifier_double_tap.go`（检测器与 worker，同常量）、`modifier_keys_windows.go`（用 Wails 的 `w32.GetAsyncKeyState`，选中键 + 0x07..0xFE，跳过鼠标键）；启动按设置、随“面板启用”开关、变更时立即生效并在保存失败时回滚、退出释放；可用性在 Windows 为 `supported` | `e2e/modifier_double_tap_run.py`（macOS，scripted keyboard，21 项通过，`modifier-mac-run2/`；run1 三项失败是脚本序列错误，原始工件与归因保留）。**scripted keyboard 只替换“读取快照”这一层**，没有验证 Windows 真实 `GetAsyncKeyState`、可见性与焦点 |
| 打包（R6、R7） | Wails 的 `project.nsi.tmpl` 需 wails3 CLI 生成 `wails_tools.nsh`、卸载键与安装目录不同于 Tauri、无 `/UPDATE`/`/ARGS`、不停进程、无版本比较，**不能原位升级 Tauri 安装**；`tauri bundle` 只认 Tauri 工程的 cargo 产物 | `windows/installer.nsi`（小型实现同一契约与注册表身份，复用 `installer-hooks.nsh`）；`e2e/package_windows.py`：固定版 `wails3 generate syso` 生成资源、`-H windowsgui` 的 release 构建、makensis、便携 zip（与 `build.yml` 内容一致）、manifest（HEAD、dirty、diff 哈希） | 脚本用标准库 `debug/pe`（`e2e/pecheck`）要求 daemon 文件是目标架构的结构有效 PE 可执行文件，损坏或架构不符一律拒绝；这 **不证明** 它是 Rust daemon（来源、身份、能否运行都未验证，manifest 记为 `daemon.kind=supplied-unverified-origin`、`identityVerified=false`）。`--packaging-check-fixture` 永远表示 fixture（即使文件是有效 PE）：输出带 `FIXTURE-` 前缀，manifest 标 `purpose=packaging-check`、`daemon.kind=fixture`；`productionUsable` 恒为 `false`。`library/e2e-windows-17b/package-amd64-fixture/` 只证明 exe 可构建、安装器脚本可编译。**未签名；真实 Rust daemon 的包、安装、卸载、原位更新均未运行**。与 Tauri 模板相比未实现：版本比较/降级保护、卸载时删除应用数据选项、WiX 迁移、语言选择；制造商注册表键用 `UniClipboard`，Tauri 实际计算的发布者字符串未能离线核实 |
| 重启 / 单实例交接（R8） | 更新安装不得再 `restartGUI` | Windows 更新路径只 `quit(true)`；重启等待旧进程用 Win32 `IsPidAlive` | 未运行 |

复跑：`e2e/installer_contract`（`go run ./e2e/installer_contract <dir>`，离线）、`e2e/modifier_double_tap_run.py`（macOS，需 `build.sh e2e`）、`e2e/package_windows.py`（需 Go、bun、makensis、网络安装固定版 wails3）、`e2e/windows_production_run.py`（专用 Windows 主机，**已编写、从未执行**：daemon 停止、真实 Alt 轻击、Run 值清理、release 便携包的生产数据根与单实例、静默安装/覆盖更新/卸载）。

### Windows 主机核查（只读）

项目记录的 Windows 测试环境：自托管 runner `uniclipboard-windows-x64-vm-01`（标签 `uniclipboard-desktop-windows-x64`、`self-hosted`、`Windows`、`X64`，见仓库 Actions runners）。2026-10-06 用 `gh api repos/UniClipboard/UniClipboard/actions/runners` 只读查询，状态 `offline`、未忙。未连接任何主机，未猜测 SSH 地址，未占用其他任务的机器。因此本片所有 Windows 运行时行为都 **未验证**。

### 复跑

```sh
# macOS（quiet，WebView 面板；注入按键，不产生键盘事件）
apps/gui-go/build.sh e2e
python3 apps/gui-go/e2e/webview_panel_shortcut_run.py --out <dir>   # 控制器断言通过则退出 3（原生可见性未验证）；失败退出 1
# 任何有 Go 的主机：Windows 编译检查
python3 apps/gui-go/e2e/build_windows.py --mode e2e --cross-check-only
# 专用 Windows 测试主机（会发送真实按键、改变前台窗口）：
python apps/gui-go/e2e/build_windows.py --mode e2e
set UC_GUI_GO_E2E_DEDICATED_HOST=1
python apps/gui-go/e2e/windows_quick_panel_run.py --out <dir> [--allow-clipboard]
```

`windows_quick_panel_run.py` 的失败方式与检查：目标窗口是脚本自己启动的 PowerShell WinForms 窗体（文本写回文件，只停止该 PID）；真实 `SendInput` 按键切换面板并验证前台窗口归属；另一进程持有同一组合键时更换快捷键必须得到 `Conflict` 且旧绑定保留，释放后同一更换成功；路径输入（含 `\n` 与非 BMP 字符）；`--allow-clipboard` 才覆盖并恢复剪贴板以验证 Ctrl+V 粘贴；无记录窗口时报错并重新显示面板；退出后 GUI 退出码 0、daemon 被 GUI 停止、快捷键可再次被脚本注册。该脚本只做过语法检查，**从未在 Windows 上执行**。

## 验收边界

- Wails 与 runtime 同时固定为 `3.0.0-beta.28`；这是 beta 原型，不是生产迁移完成。
- daemon 的 CORS 只新增精确的 `wails://localhost` 来源，未扩大为任意来源。
- 首次启动可创建独立 daemon；已有兼容持久 daemon 会被复用；不兼容或 oneshot daemon
  会明确拒绝，不执行替换或强制结束。
- macOS SDK 的链接版本警告仍存在；本轮验证当前系统实际运行，不证明最低系统版本兼容。
- Windows 17b：同上，另可为 arm64 编译、安装器脚本可编译；daemon 以 `TerminateProcess` 强制终止（非优雅关闭）；Windows 生产入口、安装器、原位更新、自启迁移、双击修饰键的真实读取/焦点/可见性均未验证；真实 Rust daemon + NSIS/便携包的原生安装与更新仍 OPEN；官方发布签名验证仍 OPEN。
- Windows：17a 代码可为 windows/amd64 编译（普通与 e2e 标签、`go vet` 通过），没有任何 Windows 运行证据（真实可见、焦点、按键、冲突、粘贴、托盘、通知、daemon 停止、单实例均未验证，runner 离线）；Linux、安装签名、Windows 更新与 GPUI 在 Windows 的 N/A 说明见上。
- 单实例：投递尽力而为；`application.New`→`Run` 的毫秒窗口内的激活不可接收；Windows/Linux 与 macOS Dock 再点击未验证；窗口重放曾触发原生 `SIGSEGV`（根因未定位，见“Wails 能力审计”）。
- 17a macOS：`webview_panel_shortcut_run.py` 控制器断言 17 项通过、原生可见性 3 项未验证（显示器仍休眠，`CGDisplayIsAsleep=1`，仅相关性，不断定根因）；`single_instance_run.py` 因 `--quick-panel` 行为变化重跑一次通过；未重跑整套 quiet 回归。
- 显示器休眠时（本片回归时 `CGDisplayIsAsleep=1`）`startup_run.py`（轻量重开 `mainVisible=false`）、`native_panel_run.py`（页面驱动在 `native-ready` 后不再推进，GUI 因此不会自行退出）、`run.py`（`driver-complete` 超时）失败；基线 `ae8738f2a` 在同一状态下同样失败，故不归因本片。`tray_devices_run.py` 与 `single_instance_run.py` 在该状态下通过。需在显示器唤醒时重跑这三项并与基线对照。
- App Nap：已验证“真实 native background scheduler 在长空闲 Accessory 进程中由系统回调并进入同一唤醒路径”，**未证明** 进程实际进入 App Nap 或定时器被暂停（1Hz 探针 `maxLate` 仅 2~5ms；探针与系统其他因素都可能影响）；`passed=true` 与“idle”不等于已进入 App Nap。下次可执行验收：数十分钟以上空闲（显示器休眠或其他应用在前台，最好电池供电），探针出现明显延迟或活动监视器“App Nap”列对该 PID 为“是”，同时回调仍到达；再用无 E2E 接缝的 6h 生产间隔验证。autoCheck 关闭阶段的原始证据：`library/e2e-app-nap/phase-b-autocheck-off-evidence.json`（`decision=checking` 只是守卫结果，开关在检查内判定，窗口内 feed 与分析请求均为 0）。
- 更新唤醒：只验证了经 Wails 观察者分发链注入的 `NSWorkspaceDidWakeNotification`，未验证真实 macOS 睡眠恢复，Windows/Linux 原生事件无主机未验证；macOS App Nap 补检查已实现并验证了真实系统回调（见“Wails 能力审计”），但未证明进程确处于 App Nap、未测 6h 生产间隔、未测睡眠中的系统行为；Windows/Linux 无对应机制。
- analytics：只验证到 daemon 的 debug 日志汇，未向生产分析服务发送任何测试事件；release 汇（PostHog）端点硬编码，未在隔离环境运行。
- `scheduler_run.py` 的“更新窗口可见”断言依赖显示器处于唤醒状态：显示器休眠时窗口 `IsVisible` 为假，同一状态下未触碰的 `quick_panel_settings_run.py` 也同样失败，属环境因素，需在显示器唤醒时重跑。
- quiet 模式不证明真实窗口聚焦、视觉位置与真实全局快捷键；这些仍需可见模式的人工或原生验收。
