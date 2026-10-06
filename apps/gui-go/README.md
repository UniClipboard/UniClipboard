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
主窗口关闭隐藏与重开。第二窗口：真实 updater（dev 预览）与 quick panel 页面经多页构建加载，Go 宿主负责窗口创建、两阶段显示、失焦隐藏与尺寸；E2E 只用原生控制触发显示（全局快捷键尚未实现）。托盘与退出语义：托盘菜单（同步开关、打开、设置、检查更新、重启、轻量模式、退出，六种语言标签）；普通退出（托盘退出、Cmd-Q）停止 daemon，轻量模式与重启保留 daemon。更新服务（`internal/update`）：同一份 Tauri 更新清单格式、minisign 签名校验（含 trusted comment）、下载进度与取消、macOS 原位安装并重启；公钥构建时从 Tauri 更新配置注入，E2E 构建才允许用本地清单与临时密钥覆盖。后台更新调度已实现（含系统唤醒补检查与 macOS App Nap 补检查，见“Wails 能力审计”）。尚未实现：全局快捷键、Windows/Linux 原位安装、设备同步子菜单、轻量模式通知、更新、通知、文件预览协议、
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
| 全局快捷键 | `app.GlobalShortcut`（`global_shortcut_darwin.go`、`global_shortcut_linux_x11.go`、`global_shortcut_linux_portal.go`；Windows 另有实现） | macOS：GPUI 辅助进程自己持有快捷键与双击修饰键（既定决策，Wails 不提供双击修饰键触发）；其他平台尚未实现 | Windows / Linux WebView 面板需用 `app.GlobalShortcut` 注册快捷键并在设置变更时重注册 | 第 17 片（Windows、Linux），采用 Wails API，不手写 |
| 原生对话框 | `app.Dialog`（OpenFile / SaveFile / Info / Error） | 已集成 | 无 | 已覆盖 |
| 托盘与菜单 | `app.SystemTray`、`app.NewMenu` | 已集成（`tray.go`、`tray_devices.go`） | 仅 Uni 业务菜单内容 | 已覆盖 |
| 单实例 | `application.Options.SingleInstance`（`single_instance_*.go`；darwin 为 `flock` 锁文件 + `NSDistributedNotificationCenter`，Linux 为 D-Bus，Windows 为命名互斥体 + 窗口消息） | **已启用（单实例切片）**：`single_instance.go` 只保留 Uni 语义适配，锁与激活消息全部由 Wails 完成。生命周期：`application.New`（取锁；第二个同 scope 进程在此交出参数并以 0 退出）→ `app.Run` 启动事件循环（Wails 此时才注册激活观察者）→ `ApplicationStarted` 后才做 daemon 仲裁/启动、窗口、托盘、GPUI helper、登录项对账、冷启动序列、更新调度。所以第二个进程不接触 daemon/数据/登录项，且数秒的冷启动 daemon 拉起位于观察者之后，期间的激活不会丢失。`UniqueID` = bundle id + `UNICLIPBOARD_ENV` + profile + 数据根哈希（darwin 锁文件在 `NSTemporaryDirectory()` 而不在 HOME，临时 HOME 不隔离锁，隔离完全靠 ID）；E2E 构建的 bundle id 带 `.e2e`，与正式应用互不串扰。回调语义：普通第二次启动 → 首实例 `showMainWindow`（Tauri 契约；启动尚未完成时先挂起，冷启动序列结束后执行一次，轻量模式冷启动遇到挂起的请求则改为显示窗口而不退到后台）；`--autostart` → 仅记录、不弹窗（登录自启不强制弹主窗口，Tauri 未区分，属显式决策）；`--quick-panel` → 忽略（Go 宿主尚无面板切换入口，随全局快捷键切片接入）；首实例自身带 `--quick-panel` 启动 → 退出码 1（对应 `validate_primary_launch`）。silent/lightweight/autostart 的窗口行为不变。重启：新进程带 `UC_GUI_RESTART_PARENT_PID`，在 `application.New` 之前等旧进程退出再取锁（旧进程退出前仍持锁）。 | 适配层保留：① 回调只做 `go handle…` 转交——**Wails 缺陷（beta.28）**：回调与 darwin 主线程通知处理共用容量为 1 的 `secondInstanceBuffer`，回调内同步做 UI 工作会令突发启动把主线程阻塞在满通道上、同时回调 goroutine 等主线程而死锁（实测：未转交时 8 次突发只送达 2 条，之后所有启动都丢失；转交后 8/8）；② 重启等待旧进程；③ 范围 ID 与参数白名单；④ 启动期挂起并重放窗口请求。**仍存在的 Wails 限制（不手写补丁）**：观察者在 `app.Run` 才注册，`application.New` 到 `Run` 之间（只剩窗口之外的构造，毫秒级，无 daemon）仍无法接收；投递尽力而为（无队列，背靠背的相同消息可能合并）；消息不加密（`EncryptionKey` 为零，同用户任何进程可向该名字发消息，故参数只按白名单解释）。**已观察的原生崩溃**：把窗口重放放在 daemon 引导后立即执行（与托盘、通知、冷启动并发创建窗口）时 `SIGSEGV`（`diagnostic-sigsegv-replay/`），根因未定位；现在重放排在冷启动序列之后，两次完整运行无崩溃，但并发创建窗口的根因仍属未证实。不提供 Tauri 的 `UC_DISABLE_SINGLE_INSTANCE`（生产二进制不带测试开关）。 | 证据 `library/e2e-single-instance/`（`single_instance_run.py`，两次完整通过）：真实第二次启动，首实例 PID / daemon PID / GPUI helper PID / 进程表不变，第二进程约 0.4s 退出码 0 且不写 `launch` 步骤；`--autostart`/`--quick-panel`/普通/8 次突发（8/8）/无首实例的 `--quick-panel`（退出码 1，无 daemon）；冷启动中的早到启动（`--autostart` 静默，普通启动由首实例在启动完成后执行：`bootstrapped.replayedHeldShow=true`、`mainExists=true`，仅 1 个 daemon）；不同 profile、同 profile 名不同 HOME、另一 bundle id 的实例各自独立（4 个不同 `UniqueID`，4 个锁文件，激活只到自己的 scope）；重启后新进程仍是首实例；首实例退出后可接管；清理后无遗留进程，生存时间在 assertions JSON。**注入与未验证**：`--autostart` 由手工传入代替 `launchctl bootstrap`（真实 bootstrap 会在真实 HOME 下起 job，未执行）；“另一应用”是同一源码换 bundle id，不是正式生产二进制（生产入口仍未实现）；Windows/Linux 单实例未验证（slice 17）；未经 sandbox 容器、真实 Dock/Finder 再次点击（`Reopen`）验证；`application.New`→`Run` 毫秒窗口未覆盖 |
| 窗口事件 | `events.Common.WindowClosing`、`WindowLostFocus` 等窗口事件与 hook | 已集成 | 无 | 既有主流程 / 面板 E2E |
| 系统睡眠/恢复（sleep/resume） | `events.Common.SystemDidWake` / `SystemWillSleep`。固定源码的派发路径：macOS `application_darwin.go` 在 `NSWorkspace` 通知中心注册 `NSWorkspaceDidWakeNotification` → `workspaceDidWake:` → `Mac.ApplicationDidWake` → `events_common_darwin.go` 映射为 `Common.SystemDidWake`；Windows `application_windows.go` 的 `WM_POWERBROADCAST` → `Windows.APMResumeAutomatic`（每次恢复都发）→ `events_common_windows.go` 映射为 `Common.SystemDidWake`，`APMResumeSuspend`（仅用户输入触发的恢复后补发）**不** 映射到 Common；Linux `application_linux_dbus.go` 订阅 logind `PrepareForSleep` → `Linux.SystemDidWake` → `Common.SystemDidWake`，无 logind/elogind 时只记 warning 不触发 | **已集成（slice 15）**：`main.go` 只订阅 `Common.SystemDidWake` 一个（平台事件已被重发为 Common 事件，再订阅会重复），回调只向容量为 1 的通道做非阻塞发送；`update_scheduler.go` 的循环用 `lastCheckAt`（墙钟，初值为启动时刻）判断，距上次任意来源的检查不足 1 小时则跳过，且不改动周期计时器；退出时 `shutdown` 先取消订阅再停调度器 | Tauri 契约（`crates/uc-tauri/src/update_scheduler/scheduler.rs` 的 `WAKE_MIN_RECHECK_SECS`）：Windows 监听 `PBT_APMRESUMEAUTOMATIC` 与 `PBT_APMRESUMESUSPEND`，Wails 的 Common 事件只映射前者，而前者每次恢复都会发送，覆盖范围等价。**Linux 在 Tauri 中没有唤醒源**，Go 侧新增，行为未在本机验证。Go 的单调时钟在 macOS 睡眠期间不前进，故必须用墙钟守卫 | 证据 `e2e/update_wake_run.py`：经 Wails 自己的观察者分发链注入唤醒（e2e 构建向 `NSWorkspace` 通知中心发布 `NSWorkspaceDidWakeNotification`，**机器并未睡眠**），listener 被调用的次数记在 GUI 日志中。**未验证**：真实 macOS 睡眠恢复；Windows/Linux 的原生事件（无主机，保持未验证） |
| macOS App Nap | Tauri 契约：`background_activity_macos.rs` 用 `NSBackgroundActivityScheduler`（标识 `app.uniclipboard.update-check`，间隔 6h，容差 10%），在 App Nap 挂起定时器时仍会触发并经同一 Wake 守卫补一次检查。固定源码核查（slice 15c 重跑）：`grep -rIl "NSBackgroundActivity\|beginActivity\|NSActivity\|AppNap\|App Nap" $(go list -m -f '{{.Dir}}' github.com/wailsapp/wails/v3)` 与对整个 `GOMODCACHE` 的同样检索均为 0 个文件，**Wails 不覆盖**；`SystemDidWake` 对应的是系统睡眠恢复，不是 App Nap 退出；仓内也无现成 Go 依赖，沿用既有的 cgo 桥接模式（`cursor_darwin.go`、`accessibility_darwin.go`），`Info.plist` 无 `NSAppSleepDisabled` | **已实现（slice 15c）**：`app_nap_darwin.go` 是最小适配，cgo 调 `NSBackgroundActivityScheduler`（标识、重复、间隔取 `defaultSchedulerTiming.activityInterval`=6h、容差 10%，与 Tauri 一致）；回调在系统的 XPC 队列上（非主线程）立即完成，只向与 `SystemDidWake` 相同的 `h.wake` 通道投递来源 `background-activity`，不做任何检查；系统要求延后（`shouldDefer`）时按 Apple 约定回 `Deferred` 且不唤醒调度器。调度循环、`lastCheckAt` 墙钟守卫、`autoCheckUpdate` 开关、setup 门禁、去重与忙碌行为全部复用，没有第二条检查路径。启动在 `bootstrap` 中与调度器一起；`shutdown` 在停止调度循环前 `invalidate`。非 macOS 为空实现（`app_nap_other.go`）：Windows/Linux 没有 App Nap，其恢复事件已由 `SystemDidWake` 覆盖 | 观察：系统在 `scheduleWithBlock` 之后会立即回调一次（与 Tauri 相同），此时守卫正确跳过；Wails 之外没有可替代的现成依赖。无法证明进程“确实处于 App Nap”（无非特权接口），E2E 的定时器偏差探针只证明定时器延迟这一效应，不证明状态 | `app_nap_run.py`（见 `library/e2e-app-nap/`）：Accessory 离屏进程，空闲下真实系统回调，不注入任何唤醒；生产 6h 间隔不实测，E2E 构建用 `UC_UPDATE_BACKGROUND_ACTIVITY_INTERVAL` 缩短 |
| 应用更新 | beta.28 的 `pkg/services` 仅有 `dock`、`fileserver`、`kvstore`、`log`、`notifications`、`sqlite`，**没有更新服务** | 自研 `internal/update`（Tauri 清单格式、minisign 签名含 trusted comment、macOS 原位替换） | Wails 不覆盖；发布格式与签名契约由现有 Tauri 发布流程决定，故保留自研 | 无替换；第 14 片验证生产公钥注入路径 |

审计规则：以后每个新宿主能力在实现前，先在本表补一行并写出源码证据；已完成的切片按此表回头审计，发现重复实现就列为替换切片，不因“已经写过”而保留。

## 验收边界

- Wails 与 runtime 同时固定为 `3.0.0-beta.28`；这是 beta 原型，不是生产迁移完成。
- daemon 的 CORS 只新增精确的 `wails://localhost` 来源，未扩大为任意来源。
- 首次启动可创建独立 daemon；已有兼容持久 daemon 会被复用；不兼容或 oneshot daemon
  会明确拒绝，不执行替换或强制结束。
- macOS SDK 的链接版本警告仍存在；本轮验证当前系统实际运行，不证明最低系统版本兼容。
- Windows/Linux、安装签名、更新、托盘、全局快捷键、原生粘贴与 GPUI 尚未验收。
- 单实例：投递尽力而为；`application.New`→`Run` 的毫秒窗口内的激活不可接收；Windows/Linux 与 macOS Dock 再点击未验证；窗口重放曾触发原生 `SIGSEGV`（根因未定位，见“Wails 能力审计”）。
- 显示器休眠时（本片回归时 `CGDisplayIsAsleep=1`）`startup_run.py`（轻量重开 `mainVisible=false`）、`native_panel_run.py`（页面驱动在 `native-ready` 后不再推进，GUI 因此不会自行退出）、`run.py`（`driver-complete` 超时）失败；基线 `ae8738f2a` 在同一状态下同样失败，故不归因本片。`tray_devices_run.py` 与 `single_instance_run.py` 在该状态下通过。需在显示器唤醒时重跑这三项并与基线对照。
- App Nap：已验证“真实 native background scheduler 在长空闲 Accessory 进程中由系统回调并进入同一唤醒路径”，**未证明** 进程实际进入 App Nap 或定时器被暂停（1Hz 探针 `maxLate` 仅 2~5ms；探针与系统其他因素都可能影响）；`passed=true` 与“idle”不等于已进入 App Nap。下次可执行验收：数十分钟以上空闲（显示器休眠或其他应用在前台，最好电池供电），探针出现明显延迟或活动监视器“App Nap”列对该 PID 为“是”，同时回调仍到达；再用无 E2E 接缝的 6h 生产间隔验证。autoCheck 关闭阶段的原始证据：`library/e2e-app-nap/phase-b-autocheck-off-evidence.json`（`decision=checking` 只是守卫结果，开关在检查内判定，窗口内 feed 与分析请求均为 0）。
- 更新唤醒：只验证了经 Wails 观察者分发链注入的 `NSWorkspaceDidWakeNotification`，未验证真实 macOS 睡眠恢复，Windows/Linux 原生事件无主机未验证；macOS App Nap 补检查已实现并验证了真实系统回调（见“Wails 能力审计”），但未证明进程确处于 App Nap、未测 6h 生产间隔、未测睡眠中的系统行为；Windows/Linux 无对应机制。
- analytics：只验证到 daemon 的 debug 日志汇，未向生产分析服务发送任何测试事件；release 汇（PostHog）端点硬编码，未在隔离环境运行。
- `scheduler_run.py` 的“更新窗口可见”断言依赖显示器处于唤醒状态：显示器休眠时窗口 `IsVisible` 为假，同一状态下未触碰的 `quick_panel_settings_run.py` 也同样失败，属环境因素，需在显示器唤醒时重跑。
- quiet 模式不证明真实窗口聚焦、视觉位置与真实全局快捷键；这些仍需可见模式的人工或原生验收。
