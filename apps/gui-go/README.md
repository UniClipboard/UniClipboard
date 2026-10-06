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
主窗口关闭隐藏与重开。第二窗口：真实 updater（dev 预览）与 quick panel 页面经多页构建加载，Go 宿主负责窗口创建、两阶段显示、失焦隐藏与尺寸；WebView 面板由 `app.GlobalShortcut` 全局快捷键切换（Windows、无原生面板辅助进程时；见“Wails 能力审计”）。托盘与退出语义：托盘菜单（同步开关、打开、设置、检查更新、重启、轻量模式、退出，六种语言标签）；普通退出（托盘退出、Cmd-Q）停止 daemon，轻量模式与重启保留 daemon。更新服务（`internal/update`）：同一份 Tauri 更新清单格式、minisign 签名校验（含 trusted comment）、下载进度与取消、macOS 原位安装并重启；公钥构建时从 Tauri 更新配置注入，E2E 构建才允许用本地清单与临时密钥覆盖。后台更新调度已实现（含系统唤醒补检查与 macOS App Nap 补检查，见“Wails 能力审计”）。Windows 生产形态（第 17b 片，仅编译与离线核对，无 Windows 运行证据）：生产入口、`TerminateProcess` 停止 daemon、NSIS 原位更新调用、旧 Tauri `Run` 项清理、双击修饰键监视器、NSIS 安装包与便携包脚本，见“Windows 生产形态（17b）”。Linux 第 17c 片（容器内 Xvfb + 私有 D-Bus 的证据，不是原生桌面；见“Linux（第 17c 片）”）：X11 快捷键、X11 修饰键双击、Hyprland 粘贴链路、AppImage 原位更新代码、安装类型检测、XDG 自启适配、deb/rpm/AppImage 容器内构建。尚未实现 / 未验证：Linux 的 Wayland Layer Shell 与 Hyprland 光标定位（17c2）、Linux 更新清单架构键（17c3）、自包含 AppImage 与真实 daemon 核验（17c4）；Windows 与 Linux 的原生验收（无主机）；macOS 真实聚焦/位置/粘贴/睡眠/登录与 App Nap 实际进入；多图 `/host-file` 真实界面；官方发布签名验证；Tauri/tao 退役前的完整契约审计。（早先列在此处的设备同步子菜单、轻量模式通知、更新、通知、文件预览协议、原生粘贴与 GPUI 宿主均已在前面的切片实现。）

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
| 开机自启 | `app.Autostart`：`Enable` / `EnableWithOptions` / `Disable` / `IsEnabled` / `Status`；选项 `Identifier`、`Arguments`；macOS 打包且 ≥13 用 `SMAppService`，否则（含裸二进制）写 LaunchAgent，Windows 写 `HKCU\...\Run`，Linux 写 XDG `.desktop` | **已替换（slice 14b）**：`autostart.go` 只保留 Uni 适配层，直接调用 `app.Autostart`；已删除 `packages/desktop-host-go/autostart`（无其他消费者，CLI service 的 systemd/launchd 不受影响） | 适配层保留：偏好持久化优先与失败回滚、启动对账（已注册则不重复注册）、`Disable` 无条件调用、`--autostart` 标记（仅 LaunchAgent 路径生效）、旧 Tauri 登录项清理（只删本登录项同名且指向其他可执行文件的 plist，不 bootout）。**Wails 能力缺口（beta.28 源码）**：① `SMAppService` 路径 `Identifier` 只校验、`Arguments` 被丢弃，登录项归整个 bundle，`Identifier` 无法隔离 profile；② LaunchAgent 的 `Status`/`Disable` 按可执行路径而非 label 匹配；③ LaunchAgent 启用会 `launchctl bootstrap` 立刻拉起一个实例（不受 Wails 选项控制）。对策：具名 profile 在 bundle 内运行时拒绝改动系统登录项（仅隔离测试构建可放行）；同一二进制的多个 profile 共享 LaunchAgent 仅靠路径匹配，不声称相互隔离 | 证据 `library/e2e-autostart-wails/`：重复启停、`Status.Strategy`（`launchagent` / `smappservice`）、plist 记录、启动对账、旧项清理、回滚、profile 守卫、测试 bundle 的 `SMAppService` 注册与清理。**未验证**：真实注销/登录后自动启动；`SMAppService` 的独立（BTM）核对；Windows/Linux 机制（slice 17） **Linux（17c）**：Wails `app.Autostart` 写 XDG `.desktop`，Xvfb 隔离验证启停；缺口：AppImage 内 `os.Executable()` 是临时挂载路径（`resolvedExecutable` 无覆盖项），故 AppImage 下自写 `Exec=$APPIMAGE` 条目（代码已写，未运行）；旧 Tauri 同名条目清理（未运行）。 |
| 通知 | `services/notifications`（`NotificationService`） | 已集成（`host_notifications.go`） | 仅 Tauri 通知插件语义适配（权限、点击回调事件） | 已覆盖，无替换 **Linux（17c）**：Wails `notifications_linux.go`（D-Bus），无通知服务，未验证。 |
| 全局快捷键 | `app.GlobalShortcut`（`Register`/`Unregister`/`UnregisterAll`/`IsRegistered`/`GetAll`；Windows 为 `RegisterHotKey`，注册在应用的主线程窗口，被占用时 `Register` 返回错误；Wails 在 shutdown 钩子之后自动 `UnregisterAll`；其修饰键只认 `cmdorctrl/cmd/command/ctrl/optionoralt/alt/option/shift/super`，不含 `meta`/`control`；不提供两步 chord，也不提供双击修饰键） | **第 17a 片**：`global_shortcuts.go`。无原生面板辅助进程（Windows，及 `UC_GPUI_QUICK_PANEL=0`）时由宿主注册 `global.toggleQuickPanel`（默认 Windows/Linux `ctrl+alt+v`，macOS `super+ctrl+v`）；macOS 默认仍由 GPUI 辅助进程持有快捷键，路径不变 | 只保留 Uni 契约：设置值（字符串或列表）与默认值、物理键归一（`meta`→`super`、`mod`/`cmd`→平台主修饰键、`control`→`ctrl`）、两步 chord 与同键双击（1 s 窗口，对应 `shortcut_registry.rs`）、卸旧→防御性卸新→装新→失败回滚（对应 `update_shortcuts`）、OS 拒绝时返回 `Conflict`；`update_keyboard_shortcuts`/`set_quick_panel_enabled` 先改 OS 绑定、再保存设置、保存失败再回滚；`--quick-panel` 第二次启动与快捷键共用 `QuickPanelToggleController` 等价物（就绪前的请求按奇偶挂起）。Linux Wayland 的合成器托管归 17c；双击修饰键见下一行（17b，Wails 不提供，Windows 靠 `GetAsyncKeyState` 轮询而非键盘钩子） | 证据 `library/e2e-windows-17a/mac-shortcut/`（`webview_panel_shortcut_run.py`，macOS quiet、`UC_GPUI_QUICK_PANEL=0`）：**控制器断言 17 项通过**（真实 Wails/Carbon 注册、换绑、无法解析的键被拒绝并保持旧绑定与设置、chord 与双击时序、禁用/重新启用、真实第二进程 `--quick-panel`、退出）。**注入**：未产生任何键盘事件，`shortcut-press` 直接调用处理函数；macOS Carbon 不报告被其他进程占用的热键（实验：两个进程都得到 0），所以此处不证明冲突。**原生可见性断言 3 项未验证**（主显示器休眠，仅相关性；`lastShown` 只证明控制器发起了显示，不替代窗口显示契约）。第一次运行（未带 `lastShown` 时）4 项可见性相关检查失败，原始文件已被重跑覆盖，仅留文字记录。Windows 的真实按键、冲突、退出后释放：`e2e/windows_quick_panel_run.py` 已编写、**未执行**（无主机） **Linux（17c）**：X11 走 Wails 的 XGrabKey 后端（Xvfb 验证）；Wayland 走 portal 后端，但 `register` 恒返回 nil、失败不同步，无法确认已绑定，故界面仍按 Tauri 契约显示合成器绑定说明。 |
| 粘贴到前一个应用 | Wails beta.28 `pkg/w32` 有 `SetForegroundWindow`/`SetFocus`/`GetForegroundWindow`/`GetAsyncKeyState`；`x/sys/windows` 有 `GetGUIThreadInfo`/`GetWindowThreadProcessId`/`IsWindow`；**缺口**：`AttachThreadInput`，以及 `SendInput`（`pkg/w32/user32.go` 中仅为注释掉的 cgo 残留）。没有 Wails 的“向其他应用发送按键/记录前台窗口”API | **第 17a 片**：`previous_app_windows.go` 只桥接 `AttachThreadInput` 与 `SendInput`（`x/sys` 的 `NewLazySystemDLL`，无 cgo），其余用上述已有 API。语义逐项对应 `crates/uc-tauri/src/quick_panel/windows.rs` 与 `mod.rs`：显示面板前记录前台窗口及其内层焦点子窗口；恢复时（主线程）还原最小化、`AttachThreadInput`+`SetForegroundWindow`+`SetFocus`、再下沉到内层子窗口，记录用后即清；`paste_to_previous_app` 等 40 ms、最多 120 ms 等 Alt 释放（仍按下则前后中和）、发 Ctrl+V，不写剪贴板；`type_file_paths_to_previous_app` 校验非空，用 `\n` 连接，逐 UTF-16 单元发 Unicode 键事件，不经剪贴板；失败（含无记录窗口）重新显示面板并返回字符串错误。非 Windows 一律返回“not yet supported”，**绝不空成功**（macOS 由原生面板辅助进程粘贴，其行为不变） | 未调研 Go 生态中的其他按键模拟库（`robotgo` 需要 cgo 工具链，`keybd_event` 无 Unicode 输入）；仅按“最小桥接 + 已有依赖”选择。Windows 真实运行未验证；macOS 上仅验证了拒绝语义（`mac-shortcut` 工件） | 第 17a 片：编译为 Windows（见下）；`windows_quick_panel_run.py` 覆盖前台还原、路径输入、粘贴与失败重显示，**未执行** **Linux（17c）**：Wails 无对应能力；Hyprland IPC 自写（`internal/hyprland`，对脚本化 socket 验证），其他环境明确报不支持。 |
| 原生对话框 | `app.Dialog`（OpenFile / SaveFile / Info / Error） | 已集成 | 无 | 已覆盖 |
| 托盘与菜单 | `app.SystemTray`、`app.NewMenu` | 已集成（`tray.go`、`tray_devices.go`） | 仅 Uni 业务菜单内容 | 已覆盖 **Linux（17c）**：Wails StatusNotifierItem（无需 libappindicator），无托盘宿主，未验证。 |
| 单实例 | `application.Options.SingleInstance`（`single_instance_*.go`；darwin 为 `flock` 锁文件 + `NSDistributedNotificationCenter`，Linux 为 D-Bus，Windows 为命名互斥体 + 窗口消息） | **已启用（单实例切片）**：`single_instance.go` 只保留 Uni 语义适配，锁与激活消息全部由 Wails 完成。生命周期：`application.New`（取锁；第二个同 scope 进程在此交出参数并以 0 退出）→ `app.Run` 启动事件循环（Wails 此时才注册激活观察者）→ `ApplicationStarted` 后才做 daemon 仲裁/启动、窗口、托盘、GPUI helper、登录项对账、冷启动序列、更新调度。所以第二个进程不接触 daemon/数据/登录项，且数秒的冷启动 daemon 拉起位于观察者之后，期间的激活不会丢失。`UniqueID` = bundle id + `UNICLIPBOARD_ENV` + profile + 数据根哈希（darwin 锁文件在 `NSTemporaryDirectory()` 而不在 HOME，临时 HOME 不隔离锁，隔离完全靠 ID）；E2E 构建的 bundle id 带 `.e2e`，与正式应用互不串扰。回调语义：普通第二次启动 → 首实例 `showMainWindow`（Tauri 契约；启动尚未完成时先挂起，冷启动序列结束后执行一次，轻量模式冷启动遇到挂起的请求则改为显示窗口而不退到后台）；`--autostart` → 仅记录、不弹窗（登录自启不强制弹主窗口，Tauri 未区分，属显式决策）；`--quick-panel` → 切换 WebView 面板（第 17a 片；启动尚未就绪时按奇偶挂起；有原生面板辅助进程时无事可做）；首实例自身带 `--quick-panel` 启动 → 退出码 1（对应 `validate_primary_launch`）。silent/lightweight/autostart 的窗口行为不变。重启：新进程带 `UC_GUI_RESTART_PARENT_PID`，在 `application.New` 之前等旧进程退出再取锁（旧进程退出前仍持锁）。 | 适配层保留：① 回调只做 `go handle…` 转交——**Wails 缺陷（beta.28）**：回调与 darwin 主线程通知处理共用容量为 1 的 `secondInstanceBuffer`，回调内同步做 UI 工作会令突发启动把主线程阻塞在满通道上、同时回调 goroutine 等主线程而死锁（实测：未转交时 8 次突发只送达 2 条，之后所有启动都丢失；转交后 8/8）；② 重启等待旧进程；③ 范围 ID 与参数白名单；④ 启动期挂起并重放窗口请求。**仍存在的 Wails 限制（不手写补丁）**：观察者在 `app.Run` 才注册，`application.New` 到 `Run` 之间（只剩窗口之外的构造，毫秒级，无 daemon）仍无法接收；投递尽力而为（无队列，背靠背的相同消息可能合并）；消息不加密（`EncryptionKey` 为零，同用户任何进程可向该名字发消息，故参数只按白名单解释）。**已观察的原生崩溃**：把窗口重放放在 daemon 引导后立即执行（与托盘、通知、冷启动并发创建窗口）时 `SIGSEGV`（`diagnostic-sigsegv-replay/`），根因未定位；现在重放排在冷启动序列之后，两次完整运行无崩溃，但并发创建窗口的根因仍属未证实。不提供 Tauri 的 `UC_DISABLE_SINGLE_INSTANCE`（生产二进制不带测试开关）。 | 证据 `library/e2e-single-instance/`（`single_instance_run.py`，两次完整通过）：真实第二次启动，首实例 PID / daemon PID / GPUI helper PID / 进程表不变，第二进程约 0.4s 退出码 0 且不写 `launch` 步骤；`--autostart`/`--quick-panel`/普通/8 次突发（8/8）/无首实例的 `--quick-panel`（退出码 1，无 daemon）；冷启动中的早到启动（`--autostart` 静默，普通启动由首实例在启动完成后执行：`bootstrapped.replayedHeldShow=true`、`mainExists=true`，仅 1 个 daemon）；不同 profile、同 profile 名不同 HOME、另一 bundle id 的实例各自独立（4 个不同 `UniqueID`，4 个锁文件，激活只到自己的 scope）；重启后新进程仍是首实例；首实例退出后可接管；清理后无遗留进程，生存时间在 assertions JSON。**注入与未验证**：`--autostart` 由手工传入代替 `launchctl bootstrap`（真实 bootstrap 会在真实 HOME 下起 job，未执行）；“另一应用”是同一源码换 bundle id，不是正式生产二进制（生产入口仍未实现）；Windows/Linux 单实例未验证（slice 17）；未经 sandbox 容器、真实 Dock/Finder 再次点击（`Reopen`）验证；`application.New`→`Run` 毫秒窗口未覆盖 **Linux（17c）**：Wails D-Bus 单实例，Xvfb 私有总线验证 `--quick-panel` 转发。 |
| 窗口事件 | `events.Common.WindowClosing`、`WindowLostFocus` 等窗口事件与 hook | 已集成 | 无 | 既有主流程 / 面板 E2E |
| 系统睡眠/恢复（sleep/resume） | `events.Common.SystemDidWake` / `SystemWillSleep`。固定源码的派发路径：macOS `application_darwin.go` 在 `NSWorkspace` 通知中心注册 `NSWorkspaceDidWakeNotification` → `workspaceDidWake:` → `Mac.ApplicationDidWake` → `events_common_darwin.go` 映射为 `Common.SystemDidWake`；Windows `application_windows.go` 的 `WM_POWERBROADCAST` → `Windows.APMResumeAutomatic`（每次恢复都发）→ `events_common_windows.go` 映射为 `Common.SystemDidWake`，`APMResumeSuspend`（仅用户输入触发的恢复后补发）**不** 映射到 Common；Linux `application_linux_dbus.go` 订阅 logind `PrepareForSleep` → `Linux.SystemDidWake` → `Common.SystemDidWake`，无 logind/elogind 时只记 warning 不触发 | **已集成（slice 15）**：`main.go` 只订阅 `Common.SystemDidWake` 一个（平台事件已被重发为 Common 事件，再订阅会重复），回调只向容量为 1 的通道做非阻塞发送；`update_scheduler.go` 的循环用 `lastCheckAt`（墙钟，初值为启动时刻）判断，距上次任意来源的检查不足 1 小时则跳过，且不改动周期计时器；退出时 `shutdown` 先取消订阅再停调度器 | Tauri 契约（`crates/uc-tauri/src/update_scheduler/scheduler.rs` 的 `WAKE_MIN_RECHECK_SECS`）：Windows 监听 `PBT_APMRESUMEAUTOMATIC` 与 `PBT_APMRESUMESUSPEND`，Wails 的 Common 事件只映射前者，而前者每次恢复都会发送，覆盖范围等价。**Linux 在 Tauri 中没有唤醒源**，Go 侧新增，行为未在本机验证。Go 的单调时钟在 macOS 睡眠期间不前进，故必须用墙钟守卫 | 证据 `e2e/update_wake_run.py`：经 Wails 自己的观察者分发链注入唤醒（e2e 构建向 `NSWorkspace` 通知中心发布 `NSWorkspaceDidWakeNotification`，**机器并未睡眠**），listener 被调用的次数记在 GUI 日志中。**未验证**：真实 macOS 睡眠恢复；Windows/Linux 的原生事件（无主机，保持未验证） **Linux（17c）**：Wails `Linux.SystemDidWake`（logind，映射到 `Common.SystemDidWake`），现有调度器直接适用；未验证（需要真实挂起/恢复）。 |
| macOS App Nap | Tauri 契约：`background_activity_macos.rs` 用 `NSBackgroundActivityScheduler`（标识 `app.uniclipboard.update-check`，间隔 6h，容差 10%），在 App Nap 挂起定时器时仍会触发并经同一 Wake 守卫补一次检查。固定源码核查（slice 15c 重跑）：`grep -rIl "NSBackgroundActivity\|beginActivity\|NSActivity\|AppNap\|App Nap" $(go list -m -f '{{.Dir}}' github.com/wailsapp/wails/v3)` 均为 0 个文件，**Wails 不覆盖**；对现有项目与模块缓存的同样检索也没有找到可复用的桥接（这只说明本仓库与已下载模块里没有，不代表整个 Go 生态没有此类库，未做生态调研）；`SystemDidWake` 对应的是系统睡眠恢复，不是 App Nap 退出；本仓库无现成依赖，沿用既有的 cgo 桥接模式（`cursor_darwin.go`、`accessibility_darwin.go`），`Info.plist` 无 `NSAppSleepDisabled` | **已实现（slice 15c）**：`app_nap_darwin.go` 是最小适配，cgo 调 `NSBackgroundActivityScheduler`（标识、重复、间隔取 `defaultSchedulerTiming.activityInterval`=6h、容差 10%，与 Tauri 一致）；回调在系统的 XPC 队列上（非主线程）立即完成，只向与 `SystemDidWake` 相同的 `h.wake` 通道投递来源 `background-activity`，不做任何检查；系统要求延后（`shouldDefer`）时按 Apple 约定回 `Deferred` 且不唤醒调度器。调度循环、`lastCheckAt` 墙钟守卫、`autoCheckUpdate` 开关、setup 门禁、去重与忙碌行为全部复用，没有第二条检查路径。启动在 `bootstrap` 中与调度器一起；`shutdown` 在停止调度循环前 `invalidate`。非 macOS 为空实现（`app_nap_other.go`）：Windows/Linux 没有 App Nap，其恢复事件已由 `SystemDidWake` 覆盖 | 观察：系统在 `scheduleWithBlock` 之后会立即回调一次（与 Tauri 相同），此时守卫正确跳过；现有项目与模块缓存中没有找到可替代的现成依赖。无法证明进程“确实处于 App Nap”（无非特权接口），E2E 的定时器偏差探针只证明定时器延迟这一效应，不证明状态 | `app_nap_run.py`（见 `library/e2e-app-nap/`）：Accessory 离屏进程，空闲下真实系统回调，不注入任何唤醒；生产 6h 间隔不实测，E2E 构建用 `UC_UPDATE_BACKGROUND_ACTIVITY_INTERVAL` 缩短 |
| 应用更新 | beta.28 的 `pkg/services` 仅有 `dock`、`fileserver`、`kvstore`、`log`、`notifications`、`sqlite`，**没有更新服务** | 自研 `internal/update`（Tauri 清单格式、minisign 签名含 trusted comment、macOS 原位替换） | Wails 不覆盖；发布格式与签名契约由现有 Tauri 发布流程决定，故保留自研 | 无替换；第 14 片验证生产公钥注入路径 **Linux（17c）**：beta.28 无更新服务；AppImage 原位替换自研（`internal/update/appimage.go`，离线契约验证），deb/rpm 交给包管理器，清单 `linux-aarch64` 键缺口见 L19。 |

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
| 通知 | `notifications` 在 Windows 用 `go-toast`（`go.sum` 本片补上缺失条目，`go.mod` 把 `minisign` 与 `x/mod` 校正为直接依赖）；Windows 上的权限与点击回调行为未验证 **Linux（17c）**：Wails `notifications_linux.go`（D-Bus），无通知服务，未验证。 |
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
| 打包（R6、R7） | Wails 的 `project.nsi.tmpl` 需 wails3 CLI 生成 `wails_tools.nsh`、卸载键与安装目录不同于 Tauri、无 `/UPDATE`/`/ARGS`、不停进程、无版本比较，**不能原位升级 Tauri 安装**；`tauri bundle` 只认 Tauri 工程的 cargo 产物 | `windows/installer.nsi`（小型实现同一契约与注册表身份，复用 `installer-hooks.nsh`）；`e2e/package_windows.py`：固定版 `wails3 generate syso` 生成资源、`-H windowsgui` 的 release 构建、makensis、便携 zip（与 `build.yml` 内容一致）、manifest（HEAD、dirty、diff 哈希） | 脚本用标准库 `debug/pe`（`e2e/pecheck`）要求 daemon 文件是目标架构的结构有效 PE 可执行文件，损坏或架构不符一律拒绝；这 **不证明** 它是 Rust daemon（来源、身份、能否运行都未验证，manifest 记为 `daemon.kind=supplied-unverified-origin`、`identityVerified=false`）。`--packaging-check-fixture` 永远表示 fixture（即使文件是有效 PE）：输出带 `FIXTURE-` 前缀，manifest 标 `purpose=packaging-check`、`daemon.kind=fixture`；`productionUsable` 恒为 `false`。`library/e2e-windows-17b/package-amd64-fixture-v3/`（更早的 `package-amd64-fixture*` 已标注被取代）只证明 exe 可构建、安装器脚本可编译。**未签名；真实 Rust daemon 的包、安装、卸载、原位更新均未运行**。与 Tauri 模板逐项对照（见 `library/go-gui-migration-plan.md`“R6 安装器对照”）：版本比较/降级拒绝（用同一个 `nsis_tauri_utils` 插件，脚本下载并校验 Tauri 固定的 SHA-1）与卸载时“删除应用数据”**已实现、未在 Windows 运行**；WiX 迁移、语言选择、每机安装三项在本仓 Tauri 配置下无对应行为（`targets` 只有 `nsis`、未设 `displayLanguageSelector`/`installMode`），记为 N/A 而非缺失；安装方式采用标准的原位覆盖（同一注册表身份与目录直接覆盖文件，降级拒绝），**不移植** Tauri 模板的交互式“重装/先卸载”页面——这是 2026-10-06 的产品决定（评估过移植并撤销，见 PR #1874），不是待补缺口；该行为差异列入最终契约审计；发布者字符串取 Tauri 默认（标识符第二段 `uniclipboard`，`tauri-utils` `config.rs`） |
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

## Linux（第 17c 片）

分支 `hp/uni/t-0188-go-gui-linux-17c`，基于 17b2（#1874，Windows 安装器；重装页已按用户决定撤回，保留标准原位覆盖）。Linux 目标按 Tauri 当前契约（`crates/uc-tauri`、`crates/uc-desktop`）逐项核对，不缩减；固定 Wails 版本为 `v3.0.0-beta.28`，Linux 用 `gtk3` 构建标签（见下“GTK 版本”）。

**没有 Linux 原生桌面证据。** 项目记录里没有授权的 Linux 主机（只读核对：`gh api` 仅有离线的 Windows runner；本机只有 Docker 与 podman）。计划的运行证据来自本机 Docker（linux/arm64，Ubuntu 24.04）里的 **Xvfb + 私有 D-Bus 会话**：没有窗口管理器、合成器、Wayland、portal、托盘宿主、通知服务与 Secret Service。它最多能证明 X11 路径与 Linux 二进制本身，不能证明真实桌面、Wayland、Hyprland 或 GNOME/KDE 行为。

**证据状态规则：** 下表“状态”列只写有 `library/e2e-linux-17c/` 实际工件支撑的内容；写“未运行/未验证”的项没有证据，不得读作已证实。

### GTK 版本

beta.28 在 Linux 默认链接 GTK4 + `webkitgtk-6.0`；构建标签 `gtk3` 切换到 GTK3 + `webkit2gtk-4.1`（`pkg/application/linux_cgo_gtk3.go`，两套 cgo 文件均随模块提供）。选 `gtk3` 的依据：Tauri 发布的 deb/rpm/AUR/COPR 依赖的都是 `webkit2gtk-4.1` 与 GTK3，AppImage 也按它打包，沿用同一系统依赖才不引入新的运行时依赖。`gtk3` 标签随 Wails 提供，不是临时切换到无人维护的栈；GTK4 路径保留为将来整体升级时的选项。

### 需求表

| # | 需求（Tauri 契约） | 来源 | Wails beta.28 | 本片实现 | 状态 |
| --- | --- | --- | --- | --- | --- |
| L1 | 入口与环境：开发 profile、隔离模式、无 profile 生产入口 | `run.rs` P4-7 | 无 | `validateEnvironment` 放开 Linux；隔离沿用便携沙箱（`environment_portable.go`，与 Windows 共用）；`release` 标签允许 Linux | 代码；Xvfb 容器内启动、隔离沙箱、E2E 构建通过（`xvfb-run6/7/8`）；Linux 生产入口（`release` 标签）只编译与 vet，**未运行** |
| L2 | X11 全局快捷键，默认 `ctrl+alt+v`，冲突报错，两段和弦 | `shortcut_registry.rs` | **有**：`global_shortcut_linux_x11.go`（XGrabKey） | 直接用 `app.GlobalShortcut`，沿用既有 Uni 适配 | Xvfb（`xvfb-run6/7/8`）：E2E **测试绑定**（`ctrl+alt+shift+f11`，经 `UC_GUI_GO_E2E_DEFAULT_SHORTCUT` 覆盖，**不是产品默认 `ctrl+alt+v`**）经 Wails 注册；第二个 X 客户端无法抢占同一组合（证明 XGrabKey 真实存在）；真实 XTEST 按键显示/隐藏；另一客户端占用时更换返回 `Conflict`（“already registered”）且旧绑定保留，释放后同一更换成功。**产品默认快捷键与真实前端首次启动/配置同步路径未覆盖，仍需无测试接缝的后续 E2E** |
| L3 | Wayland：不注册，用户在合成器绑定 `uniclipboard --quick-panel`，界面给出说明 | `shortcut_registry.rs:26-31,90-94` | **有** portal 后端（`global_shortcut_linux_portal.go`），但 `register` 恒返回 nil，失败只走应用错误处理，**无法确认已绑定**，最终按键由合成器决定 | 仍交给 Wails portal 请求；`quick_panel_uses_compositor_shortcuts` 在 Wayland 下恒为 true（`compositor_linux.go`），界面显示绑定说明；`--quick-panel` 经单实例到达 | 模拟 Wayland 环境（`WAYLAND_DISPLAY`/`XDG_SESSION_TYPE=wayland`，无 portal）：compositor 标志为 true、GUI 不崩溃（`xvfb-run6/7/8` 检查 9）；`--quick-panel` 转发见 L12；**portal 真实绑定与 Wayland 会话未验证** |
| L4 | WebView 面板：显示/隐藏、失焦隐藏、聚焦、定位 | `quick_panel/mod.rs` | 窗口与屏幕 API | 复用既有 WebView 面板；Linux 的 `panelFocus` 用普通聚焦（无前台锁） | Xvfb 只证明显示/隐藏状态；**无窗口管理器，真实聚焦、失焦隐藏与位置未验证** |
| L5 | Wayland Layer Shell 面板（覆盖层、全屏点击关闭背板、独占键盘） | `layer_shell.rs`、`linux.rs` | **无**（beta.28 源码全文检索零命中；只提供 `NativeWindow()`） | `internal/layershell`（cgo，`dlopen("libgtk-layer-shell.so.0")`，与 Tauri 同一成熟库，不手写协议）+ `panel_layer_linux.go`；隐藏面板在 realize 之前 `Attach`；每个输出一张透明背板，背板输入区域挖掉面板矩形，点击以 **释放** 事件关闭；显示时键盘独占，任何隐藏路径都释放键盘并销毁背板 | 容器内真实无头 sway（wlroots）：协议角色、overlay 层、命名空间、独占键盘、Esc 经真实前端关闭、点击面板内/外、每输出背板；`wayland-run9/10/11/12` 31/31。**Hyprland、GNOME、KDE、真实 GPU/桌面未验证**；缺库回退只证明回退可用（不算 L5 完成） |
| L6 | Hyprland 光标定位与按显示器的可用区域/比例上限（90% 宽、80% 高） | `uc_desktop::hyprland`、`linux.rs:prepare_show` | 无 | `internal/hyprland` 的 `Cursor()`（`j/cursorpos`）+ `layerLayout`：光标所在输出（否则主显示器、再退到第 0 个）、该输出的工作区、90%/80% 上限、`axisAnchored` 的向前/翻转/夹紧、`windowScale` [0.8,1.5] | 两个不同尺寸/缩放/位置的 sway 输出，从合成器截图差分量出面板矩形并与独立重写的期望比较（含 720×400 小输出上限）；光标来自 **脚本化 Hyprland socket**，不是真实 Hyprland |
| L7 | 粘贴到前一个应用：仅 Hyprland（记录活动窗口、校验、聚焦、确认、`send_shortcut`，终端用 Ctrl+Shift+V）；其他环境明确报“不支持” | `hyprland.rs`、`linux.rs` | **无** | `internal/hyprland`（同协议、同校验、同期限）+ `previous_app_linux.go`；非 Hyprland 返回 Tauri 的原文错误并重新显示面板；`type_file_paths` 明确不支持 | 脚本化 socket 契约 29/29（`linux_contract`，`contract/`）；Xvfb 整链（显示时记录活动窗口 → 校验 → 聚焦 → 确认 → `CTRL SHIFT V` 发到该地址，对脚本化 socket）通过；非 Hyprland 环境的明确错误通过；**真实 Hyprland 未验证**（`hl.dsp.*` 语法随 Hyprland 版本） |
| L8 | 双击修饰键：仅原生 X11；有 `WAYLAND_DISPLAY`（含 XWayland）或无 `DISPLAY` 报 `unsupported_display_session` | `modifier_double_tap_platform.rs` | **无** 键盘状态接口 | `modifier_keys_linux.go`：cgo `XQueryKeymap`，选中键与其他键的快照语义与 Tauri 一致，复用既有检测器 | Xvfb 真实 XTEST：两次 Alt 轻击打开面板，Alt+ 其他键不算轻击；模拟 Wayland 环境返回 `unsupported_display_session` 并拒绝设置。采样为 20 ms 轮询，极短的按键会落在两次采样之间（见结果小节 run4） |
| L9 | 开机自启：XDG `.desktop`，参数 `--autostart`，单 profile | `adapters/autostart.rs` | **有**：`autostart_linux.go` | 直接用 `app.Autostart`；**缺口**：AppImage 内 `os.Executable()` 是镜像临时挂载路径（`resolvedExecutable`，无覆盖项），故 AppImage 下自写同格式条目，`Exec=$APPIMAGE`（`autostart_linux.go`）；旧 Tauri 同名条目（Exec 指向其他程序）在对账时清理 | Xvfb 隔离 `XDG_CONFIG_HOME`：启用写出 `UniClipboard-<profile>.desktop`（`Exec` 指向本可执行文件，带 `--autostart`），停用删除（走 Wails 路径）。**AppImage 自写条目与旧 Tauri 条目清理未运行**；Tauri `auto-launch` 的文件名为 **推断** 未核实 |
| L10 | 数据根、日志、便携模式 | `uc-app-paths` | 无 | 既有 `apppaths`（XDG） | 与 Tauri 同路径实现；Xvfb 便携沙箱运行通过，生产（非便携）数据根未运行 |
| L11 | daemon：兄弟路径 `uniclipd`，detached 启动，退出时 SIGTERM（轻量模式/重启保留） | `spawn.rs`、`daemon_probe.rs` | 无 | 既有 `daemonproc`/`daemonlife` | Xvfb：GUI 退出码 0、daemon 被 GUI 停止（SIGTERM 路径）；轻量模式/重启保留 daemon 未重跑 |
| L12 | 单实例与 `--quick-panel` 转发；无 GUI 时退出码 1 | `run.rs:391-455` | **有**：D-Bus（`single_instance_linux.go`） | 既有 `single_instance.go` | Xvfb 私有 D-Bus：`--quick-panel` 的第二个进程转发给运行中的 GUI 并显示面板（先确认面板原为隐藏）；普通第二次启动退出 0；无 GUI 时退出码 1 未测 |
| L13 | 托盘（appindicator） | `tray.rs` | **有**：StatusNotifierItem（`systemtray_linux.go`），不需要 libappindicator | 既有托盘逻辑。Wails 源码走 StatusNotifierItem，不链接 libappindicator；deb/rpm 现在与 Tauri 一致地声明 `libgtk-layer-shell0` / `gtk-layer-shell`（17c2 改了 `package_linux.py`，**包未重建、依赖声明未在包管理器里验证**） | **无托盘宿主，未验证** |
| L14 | 通知 | `lightweight.rs` | **有**：D-Bus（`notifications_linux.go`） | 既有通知逻辑 | **无通知服务，未验证** |
| L15 | 唤醒补检查 | Tauri 在 Linux 没有（`wake_source.rs` 空实现） | **有**：`Linux.SystemDidWake`（logind，映射到 `Common.SystemDidWake`） | 现有更新调度器的唤醒路径直接适用，比 Tauri 多一个能力 | **未验证**（需要真实挂起/恢复） |
| L16 | WebKitGTK DMABUF：Wayland 下默认关闭 | `run.rs:176-250` | 无 | `webkit_env_linux.go` | 代码；未在真实 Wayland 验证 |
| L17 | 安装类型：`appimage`/`deb`/`rpm`/`unknown` | `commands/updater.rs:1144-1280` | 无 | `install_kind_linux.go`（`$APPIMAGE`，`/usr` 等前缀 + `dpkg-query -S`/`rpm -qf`） | 代码；**包管理器分支未在真实安装的包里验证** |
| L18 | 更新：**只有 AppImage 原位更新**；deb/rpm 由包管理器负责（前端弹出命令提示） | `UpdateContext.tsx:45-50` | **无** 更新服务 | `internal/update/appimage.go` + `host_install_linux.go`：载荷可为 `.AppImage.tar.gz` 或裸 ELF，同目录暂存后原子替换 `$APPIMAGE`，从 `$APPIMAGE` 重启；其余安装类型明确拒绝 | 载荷提取/替换/拒绝路径的离线契约已运行（`linux_contract`，见结果小节）；**真实 AppImage 更新重启未验证** |
| L19 | 更新清单键：Tauri 只产出 `linux-x86_64` | `scripts/assemble-update-manifest.js:75-110` | — | **既有缺口**：脚本把任何 `.AppImage(.tar.gz).sig` 都归到 `linux-x86_64`，arm64 的 AppImage 既拿不到 `linux-aarch64` 键，还可能覆盖 x86_64 键；Windows 同类冲突已修，Linux 没有。Go 客户端在 arm64 上请求 `linux-aarch64`，会找不到条目 | 已记录，**本片未改生成器**；属于全迁移范围内的必做独立切片（17c3：修生成器，用隔离 fixture 清单验证，不触发正式发布、不改生产更新源）；在此之前不能称 arm64 Linux 更新已就绪 |
| L20 | 打包：AppImage、deb、rpm（Tauri 目标），AUR/COPR/Flatpak/Snap 为二次打包；**无 Linux 便携包** | `tauri.conf.json`、`build.yml` | 无 | `e2e/package_linux.py`：deb（dpkg-deb）、rpm（rpmbuild）、AppImage（appimagetool）、更新用 `.AppImage.tar.gz`；布局与 Tauri deb/AUR 相同（`/usr/bin/uniclipboard` + `/usr/bin/uniclipd` + 桌面项 + 图标） | 容器内构建与结构检查通过（deb 在容器内可安装，rpm 元数据与依赖已核对，AppImage 解包与更新归档内容已核对，见结果小节；**从未在真实桌面启动**）；**AppImage 不自包含，不是完成的产品打包**：Tauri 用 linuxdeploy 打包库并固定插件（`docs/architecture/linux-appimage-library-policy.md`），本片只是 appimagetool 的结构检查，依赖宿主 GTK3/WebKitGTK。**后续必做打包切片 17c4**：按该策略集成 linuxdeploy 与固定插件、核验真实 Rust daemon 来源（而非占位或自构建的未验证来源）、产出并运行实际 AppImage 启动与更新工件；这些是代码/集成工作，不是“只缺原生机器或签名”。未签名（签名在发布流程） |
| L21 | 严格 Secret Service 拒绝探针写入的回退 | daemon（#1819） | — | daemon 行为，GUI 不涉及 | 不在本片 |

### 失败方式与对应检查

| 失败方式 | 检查 | 工件 |
| --- | --- | --- |
| 错连真实 profile/剪贴板/密钥环 | 便携沙箱 + `HOME`/`XDG_*` 全在沙箱内 + `UC_DISABLE_SYSTEM_CLIPBOARD=1`，隔离校验拒绝其他布局 | `linux-assertions.json` |
| X11 组合键被他人占用时假成功 | 另一个 X 客户端 `XGrabKey` 同一组合，更换必须得到 `Conflict` 且旧绑定保留，释放后同一更换成功 | 检查 3 |
| 双击修饰键误触（Alt+ 其他键） | Alt 与其他键同按不算一次轻击 | 检查 4 |
| 非 Hyprland 环境假装粘贴成功 | 返回 Tauri 的错误原文并重新显示面板 | 检查 5 |
| Hyprland 窗口地址被复用/含恶意字符/合成器不回复/回复过大/聚焦未确认/拒绝 dispatch | 脚本化 socket 契约 | `linux-contract-assertions.json` |
| AppImage 载荷非法、目录只读、目标缺失导致半更新 | 同目录暂存后原子重命名；错误载荷与只读目录不改动已安装文件 | `linux-contract-assertions.json` |
| Wayland 下假装支持修饰键 | 模拟 `WAYLAND_DISPLAY` 时返回 `unsupported_display_session` 并拒绝设置 | 检查 9 |
| 第二次启动重复实例 | D-Bus 单实例：`--quick-panel` 转发，普通启动退出 0 | 检查 8 |
| 包缺 daemon 或架构错 | 缺文件、非 ELF、错架构拒绝；`--packaging-check-fixture` 恒标记 FIXTURE | `package-manifest.json` |

### 复跑

```sh
# 一次性：构建 Linux 镜像（Ubuntu 24.04，GTK3/WebKitGTK 4.1、Xvfb、dbus、xdotool、rpm、Go）
docker build --platform linux/arm64 -t uc-gui-go-linux-build:17c -f apps/gui-go/e2e/linux/Dockerfile apps/gui-go/e2e/linux

# 任意主机：Hyprland 协议与 AppImage 载荷的离线契约检查
go run ./apps/gui-go/e2e/linux_contract <dir>      # 在 apps/gui-go 目录下执行

# 宿主机：E2E 前端包（VITE_GUI_GO_E2E=1），然后在容器里构建 daemon/CLI/GUI 并跑 Xvfb 场景
VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go build
apps/gui-go/e2e/linux/run.sh build          # 容器内：cargo 构建 daemon、go 构建 CLI 与 GUI（gtk3,e2e）
apps/gui-go/e2e/linux/run.sh xvfb <dir>     # 容器内：Xvfb + dbus-run-session + linux_xvfb_run.py
apps/gui-go/e2e/linux/run.sh package <dir>  # 容器内：生产前端包 + package_linux.py
```

### Linux 17c 结果

工件在 `.herdr-project/uni-t-0188/library/e2e-linux-17c/`（仓库外的本地库，含原始日志）。`package-run1` 的包二进制（约 400 MB）曾被我误删（误以为复制上限要求删除），**已无法恢复**；只剩清单、SHA-256 与检查记录，且构建树是脏的，不能凭哈希按位复现，只算早期打包结构检查。提交后用干净源码重建的可审阅包保存在仓库与 worktree 之外（路径与哈希见 `report.md`）。构建时源码树 HEAD 为 `13fdd8548`，**有未提交改动**（清单里 `dirty=true`），因此这些二进制不能仅凭该 HEAD 复现。

**镜像构建**（`image-build/`，四次，原始日志全部保留）：第一次 `xdotool` 取包失败；第二次 `Unable to connect to ports.ubuntu.com:80 [IP: 198.18.6.45]`；第三次层显示 DONE 但 **包状态不健康**——六次 apt 尝试全部失败（`502 Bad Gateway`，`libwebkit2gtk-4.1-0` 等未配置），重试循环没有传播失败，`command -v Xvfb` 又恰好通过，缓存下了坏层；第四次保留该层的下载成果，新增严格修复层（`apt_install.sh`：失败必须返回非零、`dpkg --configure -a`、`dpkg --audit` 必须为空）并在最后 `verify_image.sh` 真实检查 `dpkg --audit` 与 `pkg-config --exists gtk+-3.0 webkit2gtk-4.1`（gtk3 3.24.41、webkit2gtk-4.1 2.52.6）。下载路径的隔离测量（`apt-diagnosis*.log`，各一次）：HTTP 默认 rc=100/406 s/4 错误；HTTPS 默认 rc=0/21 s/0 错误；HTTPS 且关闭 pipelining rc=0/98 s/0 错误。DNS 把 `ports.ubuntu.com` 解析到 `198.18.x.x`（本机侧的伪 IP 代理路径）。**这只是单次测量，不是根因证明，也不保证 HTTPS 长期稳定**；修复层因此改用官方 HTTPS 源并保留失败检查，没有改动宿主的代理、DNS 或 Docker 设置。

**编译与链接**（`gui-build-container.log`、`gui-elf-linkage.txt`）：`gtk3,e2e` GUI 在容器内（linux/arm64）编译通过，`gtk3,production,release` vet 通过；`gui-go` 为 aarch64 动态 ELF，依赖 `libgtk-3`、`libwebkit2gtk-4.1`、`libsoup-3.0`、`libX11` 等，无未解析库，没有 GTK4、`gtk-layer-shell`、appindicator。Rust daemon（debug）在同一容器内构建成功。**linux/amd64 未构建**。

**离线契约**（`contract/linux-contract-assertions.json`）：29/29。覆盖 Hyprland IPC 客户端对脚本化 socket 的行为（命令顺序、终端类的 `CTRL SHIFT`、窗口地址被复用、恶意地址、拒绝 dispatch、合成器不回复的期限、超大回复、聚焦未确认、无活动窗口）与 AppImage 载荷（裸 ELF、`.AppImage.tar.gz`、非法载荷、同目录原子替换、可执行位、目标缺失、只读目录）。脚本化 socket 只证明客户端发什么、怎么反应，**不证明真实 Hyprland 接受 `hl.dsp.*` 语法，也不证明按键到达应用**。

**Xvfb + 私有 D-Bus 场景**（`xvfb-run1`…`xvfb-run8`，全部保留）：

| 运行 | 结果 | 原因归属 |
| --- | --- | --- |
| run1 | 失败（22 项中 9 项） | **脚本/环境接缝，不是产品缺陷**。Xvfb 的默认键盘映射没有 F13/F14 的键码，Wails 如实报告 `key "f13" has no keycode on this keyboard`（`gui1.log`），所以绑定从未注册（`recorded/wails/stored` 为空）。随后：面板从未显示（`lastShown=0` 直到失败的粘贴之后），失败的粘贴按契约重新显示面板（`j/activewindow` 只出现一次，与“面板此前从未显示、粘贴时无记录窗口、失败后才显示并记录”的顺序一致）；`--quick-panel` 的第二个进程退出 0，但此时面板已因上一步而可见，转发的切换把它隐藏了。冲突检查在 F14 上只证明了“没有键码”，不能证明被其他客户端抢占。 |
| run2 | 通过（22/22） | 改用 F11/F12。**仍有空通过风险**：“同组合隐藏”在面板本来就隐藏时也会通过 |
| run3 | 失败（27 项中 1 项：Alt 与其他键不算轻击） | 脚本时序（推断，未单独证明）。`xdotool key a` 约 12 ms，而监视器每 20 ms 采样一次，按键落在两次采样之间，Alt+a 被看成干净轻击（`modifier_double_tap.go` 已写明的采样限制，与 Tauri 方案相同）；改为按住其他键 150 ms |
| run4 | 失败（3 项：Alt 轻击不开面板及其依赖项） | 脚本时序（推断；修复后连续三次通过，不是独立证明）：两次 `xdotool` 调用各增加进程启动延迟，第二次轻击未被识别；改为在 **同一个** xdotool 进程内完成两次轻击 |
| run5 | 通过（27/27） | 在 run4 修复前运行，偶然通过，**不作为证据** |
| run6、run7、run8 | 通过（27/27，连续三次） | 加入前置状态后的最终脚本：检查 1 同时证明第二个 X 客户端抢不到该组合（XGrabKey 真实存在）；“隐藏”要求此前可见；Hyprland 粘贴要求面板可见且 `j/activewindow` 先于任何粘贴命令；`--quick-panel` 要求原先隐藏；冲突要求消息含 `already registered` |

这个场景 **只证明**：X11 的 XGrabKey 绑定、冲突与释放、XTEST 按键显示/隐藏、X11 修饰键双击及其负例、Hyprland 链路（对脚本化 socket）、非 Hyprland 的明确错误、XDG 自启启停（Wails 路径）、D-Bus 单实例转发、模拟 Wayland 环境下的拒绝与不崩溃、退出时 daemon 被停止。**不证明**：产品默认 `ctrl+alt+v` 与真实前端首次启动/配置同步（本场景用 `UC_GUI_GO_E2E_DEFAULT_SHORTCUT` 接缝，只有 e2e 构建才有）、真实窗口聚焦与位置（无窗口管理器）、Wayland 会话与 portal、真实 Hyprland、托盘、通知、挂起恢复、真实桌面。

**包**（`package-run1/kept/`：`package-manifest.json`、`SHA256SUMS`、`package-inspection.txt`）：容器内产出 deb、rpm、AppImage 与 `.AppImage.tar.gz`（arm64）。deb 在容器内 `dpkg -i` 成功、`dpkg --audit` 干净、`ldd` 无缺库、桌面项通过 `desktop-file-validate`；rpm 元数据与 `Requires`（`gtk3`、`webkit2gtk4.1`）已核对，其文件表含 rpmbuild 自动加入的 `/usr/lib/.build-id/*`（待清理）；AppImage 解包后含 `AppRun`、桌面项、图标、`usr/bin/{uniclipboard,uniclipd}`。**包内 daemon 是本容器构建的 debug 版（358 MB），来源仍标 `supplied-unverified-origin`、`identityVerified=false`、`runsVerified=false`；清单 `productionUsable=false`；未签名；AppImage 不自包含；从未在真实桌面安装或启动。**

### Linux 未完成项（全迁移范围内，nothing dropped）

| 项 | 性质 | 去向 |
| --- | --- | --- |
| L5 Wayland Layer Shell 面板；L6 Hyprland 光标定位与可用区域上限 | **已实现，容器内真实 sway 验证**（17c2） | 真实 Hyprland/GNOME/KDE、真实 GPU 与桌面输入栈仍未验证；AppImage 的 `AppRun` 强制 `GDK_BACKEND=x11`，打包产物里 Layer Shell 不会激活，要在 17c4 的 AppImage 切片里处理（并随包带上 `libgtk-layer-shell.so.0`，Tauri 即如此） |
| L19 更新清单把所有 Linux AppImage 归到 `linux-x86_64` | 既有缺陷，生成器未改 | 必做独立切片 17c3：修 `scripts/assemble-update-manifest.js`，用隔离 fixture 清单验证，不触发正式发布、不改生产源 |
| L20 AppImage 自包含（linuxdeploy + 固定插件、`linux-appimage-library-policy.md`）、真实 Rust daemon 来源核验、真实 AppImage 启动与更新工件、rpm 的 `.build-id` 清理、amd64 构建 | **打包集成未完成** | 必做切片 17c4 |
| 产品默认 `ctrl+alt+v` 与真实前端首次启动/配置同步的 Linux E2E（不用测试接缝） | 脚本未写 | 后续必做 E2E |
| AppImage 自写自启条目、旧 Tauri 条目清理、`release` 标签生产入口与非便携数据根 | 代码已写，**脚本未运行** | 后续必做 |
| 真实 Hyprland（`hl.dsp.*` 语法与按键到达）、portal 快捷键、托盘、通知、`SystemDidWake`、窗口聚焦与位置、deb/rpm 的真实包管理器检测分支、AppImage 真实更新重启 | 原生/真实桌面未验证 | 需要授权的 Linux 主机，或为各项设计更真实的隔离环境 |
| 镜像下载失败的根因 | 未证明 | 不影响产物；若再出现，先复核代理路径 |

## Linux Layer Shell 面板（第 17c2 片，L5/L6）

分支 `hp/uni/t-0188-go-gui-linux-layer-shell-17c2`，叠加在 17c（#1875）之上。本节先写设计与失败方式，E2E 在实现之前定好范围。

### 选型依据（Wails 优先，已核对固定版本源码）

| 问题 | 核对 | 结论 |
| --- | --- | --- |
| Wails `v3.0.0-beta.28` 有无 Layer Shell | 在模块目录内按 `layer.shell`、`layershell`、`gtk_layer` 全文检索，**零命中**；`WebviewWindowOptions`/`LinuxWindow` 没有相关字段 | **缺口**，需要最小补充 |
| 能否拿到原生窗口 | `linuxWebviewWindow.nativeWindow()` 返回 `GtkWindow*`（`webview_window_linux.go`），经 `Window.NativeWindow()` 公开 | 可以，不需要 fork Wails |
| 窗口何时 realize | `run()` 里 `windowNew` 用 `gtk_application_window_new`，只有 `windowShow` 调用 `gtk_widget_realize`；`Hidden: true` 的窗口创建后保持 **未 realize** | 在 realize 之前对原生句柄初始化 Layer Shell 是可行的（须由真实运行验证，见失败方式 F1） |
| 协议实现 | 不手写 `zwlr_layer_shell_v1`；使用成熟库 `libgtk-layer-shell`（GTK3，Ubuntu 24.04 为 0.8.2）。Go 侧没有可复用的成熟绑定（`gotk3` 系绑定要求 gotk3 对象，不能接 Wails 的原始指针） | 与 Tauri 相同：运行时 `dlopen("libgtk-layer-shell.so.0")`，不链接、不增加硬依赖；缺库时回退到普通窗口 |
| 光标与活动窗口 | 沿用 `internal/hyprland`（与 `uc_desktop::hyprland` 同协议）并补 `Cursor()`（`j/cursorpos`） | 复用 |

接口保持最小：新包 `internal/layershell`（cgo，仅 Linux）只暴露 `Available`、`Attach`、`Place`、`Show`、`Hide` 之类对 `GtkWindow*` 的操作，所有调用在 GTK 主线程（`application.InvokeSync`）执行；定位、尺寸上限与光标选屏的数学在 Go 中，与 Tauri `layout()` 一致。

### Tauri 契约（逐项核对 `crates/uc-tauri/src/quick_panel/{layer_shell,linux,mod}.rs`）

- 激活条件：`GdkDisplay` 类型为 `GdkWaylandDisplay` 且 `gtk_layer_is_supported()`，不是环境变量；否则整条普通窗口路径保持原样。
- 面板：overlay 层、namespace `uniclipboard-quick-panel`、exclusive zone -1、锚定左 + 上，用左/上 margin 定位；显示时键盘模式 `exclusive`，隐藏时 `none`；非 resizable 的 GTK 窗口会保持 WebKit 的自然尺寸，所以初始化后允许 resizable，尺寸走 `set_size_request` + `resize(1,1)`。
- 背板：每个输出一张全锚定、namespace `uniclipboard-quick-panel-dismiss`、不可聚焦、透明的 layer surface，点击即隐藏面板；先于面板映射，使面板在同一层的上方；隐藏面板时销毁。
- 每显示 capture 一次：光标所在输出（Hyprland `j/cursorpos`，找不到则主显示器、再退到第 0 个）、该输出的 work area（逻辑坐标，不乘缩放）。尺寸上限：宽 ≤ work area 宽的 90%，高 ≤ 80%，下取整且至少 1；跟随光标时每个轴用与其他平台相同的 `axis_anchored_position`（间隙 6，向前、向后翻转、夹紧），否则在 work area 居中。
- Linux 面板基础尺寸固定 800×560，窗口缩放因子 `windowScale` 限制在 [0.8, 1.5]，`set_quick_panel_layout` 带 `windowScale`；预览展开侧在 Linux 恒为右侧。
- 粘贴：`PanelState.previous_window` 在 `prepare_show` 记录（Hyprland 活动窗口）；粘贴先隐藏面板（释放键盘独占），再聚焦并发送快捷键。

### 失败方式（先于实现）

| # | 失败方式 | 后果 | 检查（E2E，真实合成器） |
| --- | --- | --- | --- |
| F1 | 在窗口已 realize 之后才初始化 Layer Shell，或把已实现的普通 xdg-toplevel 当作 layer surface | 库只会报警告，窗口仍是普通 toplevel，看起来“能显示”却没有 overlay/独占键盘 | `Attach` 之后读 `gtk_layer_is_layer_window`，**且** 在合成器侧确认该表面的协议角色是 layer surface（`sway -d` 日志的 layer surface 创建记录，namespace 与 layer 值匹配）；负例：对已 realize 的窗口调用必须被拒绝并返回错误，不假装成功 |
| F2 | `libgtk-layer-shell.so.0` 缺失、符号缺失、`is_supported` 为假（GNOME、X11） | 崩溃或面板不可用 | 缺库：用 **真正没有安装该库** 的 `uc-gui-go-linux-build:17c` 镜像（保留的 17c 层不含 `libgtk-layer-shell0`）在 sway 下启动同一 GUI，记录 `dlopen` 的实际失败消息（环境变量如 `LD_LIBRARY_PATH` 不能排除 `ld.so.cache` 与默认目录，所以不用它冒充缺库）；X11：Xvfb 场景（既有）。两者都必须回退到普通窗口，并按原契约工作。**回退只证明不支持的环境下面板仍可用，不算 L5/L6 完成**；GNOME 在容器里不可证，只写出“协议不存在时走此回退” |
| F3 | libgtk-layer-shell 的链接顺序要求（在 libwayland-client 之后才加载） | 初始化失败或表面角色错误 | 在真实 sway 上以 `dlopen` 方式加载并读取协议角色；若失败，记录原始日志并改接入方式，不改需求 |
| F4 | 从非 GTK 线程调用 GTK/Layer Shell | 偶发崩溃 | 所有调用经 `application.InvokeSync`；E2E 多次显示/隐藏循环后进程仍存活 |
| F5 | 键盘独占未设置或隐藏后未释放 | 面板收不到按键，或隐藏后键盘卡死在不可见的表面 | 另开一个真实 xdg toplevel 并聚焦；显示面板后 `wtype` 的文本落到面板而非该窗口（合成器侧聚焦树）；隐藏后键盘回到该窗口 |
| F6 | 背板缺失、不覆盖全部输出、点击后面板不隐藏、背板泄漏 | 点外部无法关闭，或残留透明表面吞掉点击 | 两个输出各有一张背板（日志记录 namespace `...-dismiss` 次数）；合成器光标点击面板之外 → 面板隐藏、背板销毁；再次显示不累积；点击面板内部不关闭 |
| F7 | 面板在 margin 之外定位、超出 work area、比例上限错误 | 面板被裁切或跑到别的输出 | 两个不同分辨率/缩放/位置的输出；光标在第二个输出的各个角；`grim` 截图差分得到面板包围盒，与期望矩形比较；小输出下宽 ≤ 90%、高 ≤ 80% |
| F8 | 光标不可得（非 Hyprland 或 IPC 失败）或返回非有限值 | 面板落在错误输出 | 无 Hyprland 时居中到主显示器；脚本化 socket 返回 NaN/超大/不回复，回退并不崩溃 |
| F9 | `set_quick_panel_layout` 丢掉 `windowScale` | 缩放因子被忽略 | 对 1.5 与 0.8 调用后测量尺寸，并检查上限仍然生效 |
| F10 | 粘贴前未释放独占键盘 | 目标窗口聚焦失败，按键丢失 | 粘贴链路：隐藏先于聚焦，日志顺序可核对（脚本化 Hyprland socket 记录命令时序，合成器里键盘回到目标窗口） |
| F11 | Wails 的失焦隐藏与独占键盘互相干扰 | 面板立刻关闭或永不关闭 | 在 layer 模式下显示后保持可见、Esc 经前端关闭 |

### E2E 验收范围与边界

隔离容器（linux/arm64，Ubuntu 24.04，镜像 `uc-gui-go-linux-build:17c2` 在 `:17c` 之上加 `sway`、`libgtk-layer-shell0`、`grim`、`wtype`、`wayland-utils`）里的 **sway 1.9 无头后端（wlroots，pixman 渲染）**，真实 Wayland 协议、真实 layer-shell 合成器；不触碰宿主桌面。

- **能证明**：`zwlr_layer_shell_v1` 的协议角色、overlay 层、键盘模式、多输出背板、点击关闭、按输出定位与比例上限、显示/隐藏循环、回退路径（Xvfb/缺库）。
- **不能证明**：Hyprland 本身（Hyprland 不在 Ubuntu 仓库；光标与活动窗口继续用脚本化 socket，与真实 sway 并存）、GNOME（不实现 wlr-layer-shell，走回退）、KDE、真实 GPU 渲染、真实桌面的输入栈。
- AppImage：Tauri 的 `AppRun` 钩子强制 `GDK_BACKEND=x11`（`docs/architecture/linux-appimage-library-policy.md`），因此在打包产物里 Layer Shell 路径不会激活；本片不改变打包（17c4）。

### 17c2 结果

工件（542 MB，含截图与协议轨迹）在 `/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c2/e2e-runs/`，各文件哈希在同目录上级 `SHA256SUMS-e2e-runs.txt`；镜像构建日志在 `image-build/`。全部运行（含失败）保留，没有删除任何一轮。

**选型与镜像**：Wails 无 Layer Shell（源码检索零命中）；使用成熟的 `libgtk-layer-shell` 0.8.2，运行时 `dlopen`。`dlopen` 发生在 libwayland-client 已被 GTK 加载之后，真实 sway 上初始化成功，所以失败方式 F3（链接顺序）在此环境未出现；未对其他版本或合成器验证。测试用 `uc-gui-go-linux-build:17c2`（`:17c` 之上加 `sway 1.9`、`libgtk-layer-shell0`、`grim`、`wtype`、`wayland-utils`、`python3-gi`、Mesa；`Dockerfile.17c2` + `verify_image_17c2.sh` 严格逐项验收，不依赖工具 `-h` 退出码）。第一版 Dockerfile 把验收命令用 `;` 串在一起会掩盖前面的失败，已改正，`build1.log` 保留。

**运行记录**（场景脚本 `e2e/linux_wayland_run.py`，`run.sh wayland|wayland-nolib`）：

| 运行 | 结果 | 原因归属（有证据） |
| --- | --- | --- |
| run1 | 失败（脚本崩溃于焦点检查） | 脚本/环境：无头 sway 没有键盘设备，seat 不下发键盘能力，另一个应用从未获得 `focus-in`。修复：常驻虚拟键盘 |
| run2、run3、run4（run3 与 run2 同结果，run4 加入页面内监听器诊断） | 各有多项失败（Esc、面板内点击、数项位置测量） | **三个互相独立的原因**：(a) 脚本：面板内容处于 **锁定视图**（`QuickPanelApp.tsx` 只在内容已解锁时才挂载带 Esc 处理器的 `ClipboardHistoryPanel`），Esc 是该视图的无操作；用页面内注入的监听器（`wayland-run4` 的 `escDiagnostics`）证明按键 **到达页面**（`keydown/Escape`，`focus=true`），修复：先经真实主机命令 `unlock_content` 解锁；(b) 脚本：截图差分受其他窗口（平铺的探针窗口焦点边框、输入的字符）、鼠标指针图像污染（run2 的 720×400 面板量成 736×416 是右下角的指针），`hostPlaced` 的数值和合成器里面板矩形其实一致（run2 截图直接量得 1344,84,720×400）。修复：每次测量取同一指针位置的隐藏态基线，并在测量前结束探针窗口；(c) 级联：Esc 失败后面板状态和 toggle 奇偶错位 |
| run5 | 30/31 | 剩 1 项：**产品缺陷**。点击面板内部会关闭面板。`WAYLAND_DEBUG` 的客户端轨迹显示指针焦点仍在背板（`wl_surface@57`），随后三个 layer surface 全被销毁：**同一层内的堆叠顺序协议未规定**，sway 上背板压在面板之上，Tauri 的“先映射背板就在面板下方”只在 Hyprland 成立 |
| run6 | 未通过 | 修复为背板输入区域挖掉面板矩形后，第二个输出的点击关闭失败，其后的测量因 toggle 奇偶错位级联；运行约 5 分钟后才结束。现场诊断（同一容器，只读）：Python 在 `time.sleep` 的轮询里（`hrtimer_nanosleep`），GUI 仍在应答，不是死锁；是有界的 15 轮 × 10 s 超时级联（`run6-live-diagnosis.txt`）。同时 `Gui.step` 对重复标签返回旧行，`visible()` 复用标签得到 **过期答案**，是脚本缺陷，已改为唯一标签；无超时的虚拟指针 `readline()` 改为 `select` 带截止时间 |
| run7、run8 | 30/31 | 剩第二输出的点击。加入 GTK 侧点击计数与 5 个位置的探测：紧接在 **点击关闭** 之后的那次显示里，点击没有到达背板的 GTK 处理器（计数不变），紧接在 host 命令关闭之后的显示则正常。推断原因：在 **按下** 事件里同步销毁被按下的窗口，释放事件无人接收，GDK 残留隐式抓取（推断，未读 GDK 源码证明） |
| run9 | **31/31** | 修复：改为在 **释放** 事件上关闭；5 个探测点全部命中，无 resync、无 Esc 回退 |
| run10、run11 | 31/31（同一构建，连续两次） | |
| run12 | 31/31，**干净提交 `7d599d24c`**（`git status` 无改动）重建后 | 构建身份见 `wayland-run12/build-identity.txt` |
| wayland-nolib-run1 | 5/6，保留失败 | 回退面板上 Esc 没有关闭：与 run2–4 **同一原因**，我没有先解锁（锁定视图没有 Esc 处理器；页面确实收到 `keydown/Escape`）。脚本缺陷，非回退缺陷 |
| wayland-nolib-run2 | 7/7 | 先解锁；缺库回退下 Esc 关闭 |
| run13 | 31/31，**提交 `18b7344e0`**（加入 `gtk3` 构建约束后的最终代码，干净树） | 构建身份见 `wayland-run13/build-identity.txt` |
| xvfb-regression-run1、run2、run3 | 27/27 | 既有 X11 场景在新二进制上通过（run2 是 `7d599d24c`，run3 是 `18b7344e0`，都是干净提交构建） |

**离线契约**：`go run ./e2e/linux_contract` 现为 34/34（新增 `j/cursorpos` 的读取、非法回复、无应答期限共 5 项）。

**这些运行证明了什么**（容器内真实无头 sway，不是 Hyprland）：Layer Shell 协议全局存在；面板在 realize 之前被转成 layer surface，对已 realize 的主窗口调用被拒绝；sway 日志里面板是命名空间 `uniclipboard-quick-panel`、layer 3（overlay）、每个输出一张 `uniclipboard-quick-panel-dismiss`（锚定 15）；键盘独占：另一个真实 xdg 应用在面板显示时 `focus-out`、打字不到达它，隐藏后 `focus-in` 且打字到达；Esc 经合成器真实键盘到达 GTK、WebView、页面，真实前端关闭面板；面板内点击到达页面且不关闭，面板外点击（含第二输出）关闭并释放键盘、销毁背板；两个输出（1280×800@1、1600×1000@2 即逻辑 800×500）上按截图量得的面板矩形与独立期望相符（含 720×400 小输出上限与居中）；`windowScale` 1.5/0.8；15 轮显示/隐藏无泄漏；粘贴链路隐藏先于 Hyprland 聚焦（脚本化 socket）；缺库时真实 `dlopen` 失败（`OSError ... cannot open shared object file`，在容器里真正移除了库，不是环境变量）并回退到普通窗口。

**不证明**：真实 Hyprland（光标、活动窗口、`hl.dsp.*` 都是脚本化 socket；我让脚本把假光标和 sway 指针保持一致）；GNOME（不实现 wlr-layer-shell，仅有“协议不支持时走回退”的代码路径，未在 GNOME 或任何无该协议的合成器上运行——`supported()` 为假的分支在容器里只经缺库路径间接覆盖）；KDE；真实 GPU/渲染；真实桌面输入栈；portal；X11 下的回退只由既有 Xvfb 场景覆盖；带窗口缩放因子的真实前端交互（只验证了尺寸）。产品默认快捷键 `ctrl+alt+v` 与真实前端首次启动仍同 17c 未覆盖。

### 17c2 复跑

```sh
# 一次性：在已有的 :17c 镜像之上加 sway 等（依赖 :17c；Dockerfile.17c2 末尾由 verify_image_17c2.sh 逐项验收）
docker build --platform linux/arm64 -t uc-gui-go-linux-build:17c2 -f apps/gui-go/e2e/linux/Dockerfile.17c2 apps/gui-go/e2e/linux

# 宿主机：E2E 前端包，然后在 :17c2 容器里构建 GUI（run.sh 默认镜像仍是 :17c，要用环境变量指定）
VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go build
UC_LINUX_IMAGE=uc-gui-go-linux-build:17c2 SKIP_DAEMON=1 apps/gui-go/e2e/linux/run.sh build

# 无头 sway 场景（每次用新的输出目录；加 UC_WAYLAND_RUN_ARGS=--wayland-debug 会保存客户端协议轨迹，目录很大）
UC_LINUX_IMAGE=uc-gui-go-linux-build:17c2 apps/gui-go/e2e/linux/run.sh wayland <dir>
# 缺库回退：在一次性容器里真正移除 libgtk-layer-shell 后运行
UC_LINUX_IMAGE=uc-gui-go-linux-build:17c2 apps/gui-go/e2e/linux/run.sh wayland-nolib <dir>
# 既有 X11 回归与离线契约
apps/gui-go/e2e/linux/run.sh xvfb <dir>
(cd apps/gui-go && go run ./e2e/linux_contract <dir>)
```

`internal/layershell`、`panel_layer_linux.go` 与 e2e 探针只在 `linux && gtk3` 构建标签下编译（Wails beta.28 默认 GTK4，同一进程里同时链接 GTK3 与 GTK4 会在运行时失败）；其他构建走普通窗口路径。测试用的 `wlr-virtual-pointer` 协议 XML 的来源与哈希见 `apps/gui-go/e2e/linux/protocols/SOURCES.md`。

首帧尺寸与 Tauri 一致：Tauri 的 `show()` 也是先用未缩放的 `panel_dimensions(1.0, false)` 准备窗口，随后前端的 `set_quick_panel_layout` 才带入 `windowScale`，这里同样用 1。

**已知非本片引入的警告**：`gui1.log` 周期性出现 `gtk_container_foreach`、`gtk_menu_shell_insert`、`gtk_menu_item_set_submenu` 的 `Gtk-CRITICAL`（约每 10 秒一次，与托盘菜单刷新相关，没有托盘宿主）。17c 的 `xvfb-run8`（没有任何 Layer Shell 代码）里同样出现，故与本片无关；根因未单独追查，记入后续的托盘验证。

**未决项（本片范围内能做而没做的）**：Layer Shell 首次映射时 GTK 先以 WebKit 的 800×560 自然尺寸映射，随后才收缩到上限尺寸（轨迹里先 `set_size(800,560)` 再 `set_size(720,400)`，Tauri 同理），小输出上有一帧的尺寸跳变，未量化；X11 路径的面板尺寸仍是 macOS 的常量而非 Tauri 的 Linux 固定 800×560，只有 Layer 路径用 Linux 常量，两者统一留待后续。

## 更新清单生成器的 Linux 架构（第 17c3 片）

分支 `fix/gui-go-update-manifest-linux-arch`，叠加在 17c2（#1876）之上。本节先写问题、策略、失败方式与 E2E 范围，实现在后。不改生产 feed、不触发 release、不改渠道与真实资产。

### 问题（已核对源码与真实发布资产）

- 实际入口：`.github/workflows/release.yml` 与 `.github/workflows/mirror-desktop-gitcode.yml` 都调用同一个 `scripts/assemble-update-manifest.js`，其输出再交给 `scripts/build-flare-release-registration.js`；没有第二套生成器。
- 真实资产命名（只读取自 GitHub Release `v1.1.1` 的资产列表，`gh release view`）：`UniClipboard_1.1.1_amd64.AppImage(.sig)` 与 `UniClipboard_1.1.1_aarch64.AppImage(.sig)`；该版本没有 `.AppImage.tar.gz`。`release.yml` 把所有构建产物展平到 `release-assets/`，因此文件名（不是目录名）是唯一的架构来源。
- 消费者合同：Go `apps/gui-go/internal/update.DefaultTargets` 把 `GOARCH` `amd64/arm64` 映射为 `x86_64/aarch64`，查找 `linux-<arch>`（Linux 无 installer 后缀）；Tauri 更新插件 `tauri-plugin-updater` 2.10.1 的 `updater_arch()`（`src/updater.rs`）使用同一组词。两者都只在 `platforms` 里按键名查找，找不到就报“no artifact”。
- 现状：`detectPlatform` 把每个 `.AppImage.sig` / `.AppImage.tar.gz.sig` 都映射为 `linux-x86_64`，不存在 `linux-aarch64`。真实命名下 `sort()` 使 `_aarch64.AppImage.sig` 先于 `_amd64.AppImage.sig`，二者优先级相同，后者覆盖前者：现状输出的 `linux-x86_64` 碰巧正确，但 `linux-aarch64` 完全缺失，aarch64 用户永远收不到更新。若 aarch64 以 `.AppImage.tar.gz.sig`（优先级 30）发布而 amd64 是裸 `.AppImage.sig`（20），则 aarch64 载荷会占据 `linux-x86_64`：签名对载荷本身有效，minisign 校验拦不住，x86_64 用户会下载并安装 aarch64 二进制。真实 v1.1.1 没有这种组合，这是由代码推出的危险方向，E2E 里用 fixture 复现，不当作已发生的事实。

### 策略（先于实现决定）

1. 架构只取自文件 basename 中以非字母数字边界隔开的词：`aarch64`、`arm64` → `linux-aarch64`；`amd64`、`x86_64`、`x64` → `linux-x86_64`（与同文件里 darwin/windows 分支已有的词汇一致，不引入新词汇）。目录名不参与（真实流水线已展平，且目录名可能含 `ubuntu-22.04-arm` 之类的干扰）。
2. 两种词同时出现或都不出现（`armv7`、`i686`、无架构）：返回 `null`，沿用现有的“Skipping unrecognized .sig file”警告，**绝不** 回退到 `linux-x86_64`。后果是该文件不进清单（该架构暂无自更新），而不是被错误架构的用户下载。
3. 同一 `linux-*` 键出现两个 **相同优先级** 的候选：直接报错退出（`exit 1`，信息里列出两个文件名），不再由排序后的“最后一个”静默胜出。不同优先级沿用现有规则（`.AppImage.tar.gz.sig` 30 高于 `.AppImage.sig` 20）。该严格性只加在 Linux 键上；macOS/Windows 的选择规则、优先级、键序、格式、版本与签名语义保持不变（现有 release 没有任何相同优先级的 darwin/windows 配对）。
4. 不新增配置层、不改 `release.yml`/`mirror-desktop-gitcode.yml`/`workers/update-server`/FlareRelease，不改 `createMockArtifacts` 的数据；不改现有 vitest 文件（项目约束：不为本片补单元测试，用端到端验证）。

### 失败方式（先于实现）

| # | 失败方式 | 后果 | 检查（端到端） |
| --- | --- | --- | --- |
| F1 | aarch64 AppImage 被映射成 `linux-x86_64`，清单没有 `linux-aarch64` | aarch64 用户永远收不到更新（Go 消费者 `no artifact for linux-aarch64`） | 真实命名的 fixture 经 **真实生成器** 得到 `linux-aarch64` 与 `linux-x86_64` 各一，URL 的 basename 与签名内容属于各自架构的文件 |
| F2 | 相同优先级时由排序顺序决定谁胜出 | 结果取决于文件名/目录排序，不可复现 | 把两个架构的文件放进不同的目录嵌套、不同的创建顺序，输出的 Linux 两项必须完全相同 |
| F3 | `.AppImage.tar.gz.sig`（30）与裸 `.AppImage.sig`（20）跨架构混合时，高优先级的架构占据了另一架构的键 | 错误架构的二进制通过有效签名被安装 | 混合 fixture（aarch64 用 tar.gz，amd64 用裸文件）及其反向；两键各自指向各自架构的文件，签名经真实 minisign 校验该架构的载荷通过 |
| F4 | 未知或缺失架构（`armv7`、`i686`、无架构词）被默认成 x86_64 | 错误架构的二进制被分发 | fixture 含 `armv7`、无架构的 AppImage：不出现在任何键，stderr 有警告；已知架构不受影响；绝不默认 |
| F5 | 目录名里的 `arm`/`arm64` 污染判定，或词的子串误匹配（如 `x64` 出现在其他词里） | 误判 | 架构词在目录名而 basename 无词的 fixture 不得被分类；basename 里的边界判定用 `armv7`、`xx64` 之类反例验证 |
| F6 | 同一键出现两个相同优先级的候选（重复或重名资产） | 静默选择其一 | 两个 amd64 `.AppImage.sig` 放在不同目录：生成器必须非零退出并同时指出两个文件，且不写出 `--output` |
| F7 | 单平台发布（只构建 `ubuntu-22.04-arm`）输出 `linux-x86_64` | 对 x86_64 用户给出 aarch64 载荷 | 只含 aarch64 的 fixture：清单只有 `linux-aarch64` |
| F8 | macOS/Windows 的键、键序、URL、签名、版本被改动 | 现有平台自更新回归 | 同一份含 macOS+Windows+Linux 的输入，分别用基线版本（提交 `3a2cc01c5` 的脚本）与修复版本生成：除 `pub_date` 外，非 Linux 键的内容与键序逐字节一致；Go 消费者在 darwin/arm64 宿主上取到 `darwin-aarch64` 并通过签名 |
| F9 | 登记链路（`build-flare-release-registration.js`）只登记一个 Linux 制品，或 sha256/大小对不上 | 镜像/校验与 manifest 不一致 | 对生成的清单链式运行真实登记脚本，断言两个 Linux 制品各自的 `filename`、`size`、`sha256`（来自 fixture 载荷） |
| F10 | 签名与载荷错配（一个架构的签名挂在另一个架构的 URL 上） | 校验失败或（更糟）校验到错误载荷 | 真实 Go 消费者（`DefaultTargets` 在 linux/arm64 与 linux/amd64 容器里 **自然** 求出键）：检查、下载、minisign 校验全部通过；负例：用另一架构的签名校验必须失败 |
| F11 | 键名与消费者不一致（词汇漂移） | 找不到平台 | 容器里的真实 `DefaultTargets("")` 输出写入工件，并与清单键逐一比对 |

### E2E 验收范围与边界

- **生成器**：宿主的 Node（真实脚本、真实命令行）。红灯运行用 `git show 3a2cc01c5:scripts/assemble-update-manifest.js` 提取的未修复版本，绿灯运行用工作树版本，使用同一个 runner 与断言，原始输入、命令、输出与 SHA256 都保留。
- **输入（FIXTURE）**：文件名取自 `v1.1.1` 的真实发布命名，载荷是合成字节（每个架构不同），签名是用一次性密钥对载荷做的 **真实** minisign 签名（base64，与 Tauri `.sig` 同形）。fixture 不是真实发布资产，不含真实签名密钥。
- **消费者**：真实的 `internal/update`（`Check`/`Download`/`Verify`）。一个 e2e 驱动在容器内起本地 HTTP 服务提供 feed 与载荷，容器无网络。linux/arm64（宿主原生）与 linux/amd64（Docker 的 QEMU 仿真）各跑一次，`DefaultTargets` 取自真实 `runtime.GOARCH`。
- **不能证明**：真实的 Tauri 更新插件运行（只引用其源码行）；真实 AppImage 自更新与重启（17c4）；FlareRelease 服务端是否接受 `linux-aarch64` 这个平台字符串（`windows-aarch64` 已有先例，但服务端未核验）；Windows 键只做生成器输出与基线的逐字节对比，没有在 Windows 上运行消费者；本片不改 `.AppImage.tar.gz` 的现有优先级语义，v1.1.1 没有该资产，故其真实形态未观察。

### 17c3 实现与结果

- **实现（`scripts/assemble-update-manifest.js`，唯一生成器）**：`detectPlatform` 的 AppImage 分支按 basename 中的架构词返回 `linux-aarch64` 或 `linux-x86_64`，未知/无词/同时出现返回 `null`（沿用既有的“Skipping unrecognized”警告，不回退到 x86_64）；`scanArtifacts` 对同一 `linux-*` 键的相同优先级候选抛错，`main` 捕获后以 `Error: ...` 非零退出且不写 `--output`。macOS/Windows 的分支、优先级、键序与输出格式没有改动。没有新增配置层、没有改 workflow/worker。
- **E2E（`apps/gui-go/e2e/update_manifest_run.py` + `e2e/manifestprobe`）**：真实生成器子进程 → 真实消费者（`internal/update` 的 `DefaultTargets`/`Check`/`Download`/`Verify`，在 linux/arm64 与 linux/amd64 容器内自然取得键；amd64 为 Docker QEMU 仿真，容器 `--network none`）→ 真实 `build-flare-release-registration.js`。10 个生成场景（真实命名、目录嵌套与创建顺序、tar.gz/裸文件跨架构混合及其反向与两者并存、单平台、未知架构、误导性目录名、重复候选）+ 与基线的兼容比对 = 120 项断言。
- **红灯（未修复生成器 `3a2cc01c5`，`red-run2`）**：59/102 通过。最重要的复现是 S3a：aarch64 以 `.AppImage.tar.gz`、amd64 以裸 `.AppImage` 发布时，`linux-x86_64` 指向 aarch64 的 tar.gz，linux/amd64 的真实客户端 **下载并通过 minisign 校验** 拿到 aarch64 载荷（签名对载荷有效，校验拦不住）；S4（只构建 aarch64）同样把 aarch64 载荷放进 `linux-x86_64`；真实命名 S1 缺 `linux-aarch64`，arm64 客户端报 `no artifact for linux-aarch64`；S5 未知架构（`xx64`、`armv7`、无架构词）被映射为 x86_64；S6 重复候选静默通过。`red-run1` 的失败含我的断言过弱的脚本缺陷（S6 的 stderr 断言误通过、单平台时的交叉校验断言），保留原件（`red-run1` 只保存了断言 JSON 与逐场景原始文件，当时的控制台输出没有落盘），修正后重跑为 `red-run2`。
- **绿灯（修复后，从干净提交 `284655f8e`，`green-run2`）**：120/120。`green-run1`（未提交的同一份代码）也是 120/120。S1 在 arm64 与 amd64 容器里各自下载并校验自己架构的载荷，另一架构的签名对该载荷被拒绝；S2 的目录/创建顺序对 Linux 两项无影响；S3 三种混合都各指向各自架构的文件；S4 的 amd64 客户端得到 `no artifact`（不再拿到 aarch64 载荷）；S5/S5b 未知与目录名误导都按策略跳过并带警告；S6 非零退出、不写 manifest、报错列出两个路径；登记脚本为两个 Linux 制品给出各自的 filename/size/sha256；macOS/Windows 条目、键序、版本与 notes 与基线逐项一致，宿主 darwin 的真实消费者（installer `app`）通过校验。
- **其他验证**：现有 `scripts/__tests__/assemble-update-manifest.test.ts` 与 mirror 测试 12/12 通过（未修改）；`--test` 仍输出合法 JSON；`go vet`（宿主与 windows 交叉）通过。
- **工件**：`/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c3/`（`red-run1/2`、`green-run1/2`、`inputs/` 基线脚本、`SHA256SUMS.txt`；每次运行含原始输入、命令、stdout/stderr、返回码与断言 JSON，各约 31 MB，主要是交叉编译的探针）。库内只有 `e2e-linux-17c3-index/` 的小摘录。

### 17c3 复跑

```sh
# 基线脚本（未修复版本）用于红灯与兼容比对
git show 3a2cc01c5:scripts/assemble-update-manifest.js > <dir>/baseline.js
# 每次使用新的输出目录；需要 Docker（linux/arm64 与 linux/amd64 的 ubuntu:24.04 镜像）与 Go、Node
python3 -I apps/gui-go/e2e/update_manifest_run.py --generator scripts/assemble-update-manifest.js --baseline <dir>/baseline.js --out <dir>/green-runN
python3 -I apps/gui-go/e2e/update_manifest_run.py --generator <dir>/baseline.js --baseline <dir>/baseline.js --out <dir>/red-runN   # 预期失败
```

### 17c3 未证明

- 没有运行真实 Tauri 更新插件（只引用 `tauri-plugin-updater` 2.10.1 的 `updater_arch()` 源码）；没有真实 AppImage 的下载、替换与重启（17c4）。
- FlareRelease 服务端是否接受 `linux-aarch64` 这个平台字符串未核验（`windows-aarch64` 有先例）；`mirror-desktop-installers-to-gitcode` 按清单逐平台镜像，未在线上运行。
- Windows 键只与基线逐项比对，没有在 Windows 运行消费者；未运行 darwin/amd64（Rosetta）消费者。
- `.AppImage.tar.gz` 的真实形态（v1.1.1 没有）未观察，只用 fixture 覆盖了其优先级语义；`--test` 的模拟资产名（`amd64.AppImage.tar.gz.sig`）与真实发布命名不同，未改动。
- 对真实的 `release.yml` 没有运行；未改任何 workflow、渠道、feed 或真实资产。严格拒绝（相同优先级）会让同一键有两个同优先级资产的发布在生成清单一步失败，这是有意的取舍；该步骤位于 GitHub Release 创建之后，失败时会留下已创建的 release，需要人工处理。

## 验收边界

- Wails 与 runtime 同时固定为 `3.0.0-beta.28`；这是 beta 原型，不是生产迁移完成。
- daemon 的 CORS 只新增精确的 `wails://localhost` 来源，未扩大为任意来源。
- 首次启动可创建独立 daemon；已有兼容持久 daemon 会被复用；不兼容或 oneshot daemon
  会明确拒绝，不执行替换或强制结束。
- macOS SDK 的链接版本警告仍存在；本轮验证当前系统实际运行，不证明最低系统版本兼容。
- Linux 17c2：Layer Shell 面板与每输出定位/上限由容器内真实无头 sway 验证（见“17c2 结果”），Hyprland/GNOME/KDE 与真实桌面未验证。Linux 17c：证据来自容器内 Xvfb + 私有 D-Bus（无窗口管理器、Wayland、portal、托盘宿主、通知服务、Secret Service）与脚本化 Hyprland socket；默认快捷键用 e2e 测试接缝；AppImage 不自包含、daemon 来源未核验；没有任何真实 Linux 桌面运行证据。详见“Linux（第 17c 片）”。
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
