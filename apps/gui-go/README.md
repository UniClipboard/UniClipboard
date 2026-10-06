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
主窗口关闭隐藏与重开。第二窗口：真实 updater（dev 预览）与 quick panel 页面经多页构建加载，Go 宿主负责窗口创建、两阶段显示、失焦隐藏与尺寸；E2E 只用原生控制触发显示（全局快捷键尚未实现）。托盘与退出语义：托盘菜单（同步开关、打开、设置、检查更新、重启、轻量模式、退出，六种语言标签）；普通退出（托盘退出、Cmd-Q）停止 daemon，轻量模式与重启保留 daemon。更新服务（`internal/update`）：同一份 Tauri 更新清单格式、minisign 签名校验（含 trusted comment）、下载进度与取消、macOS 原位安装并重启；公钥构建时从 Tauri 更新配置注入，E2E 构建才允许用本地清单与临时密钥覆盖。尚未实现：全局快捷键、后台更新调度、Windows/Linux 原位安装、设备同步子菜单、轻量模式通知、更新、通知、文件预览协议、
autostart、原生粘贴与 GPUI 宿主、Windows/Linux 原生验收。

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
```

宿主的 daemon 客户端（`packages/desktop-host-go/daemonclient`）现在缓存会话令牌至刷新时间，401 时重新交换一次。
此前每个请求都交换令牌，会触发 daemon 对 `/auth/connect` 的每 IP 每分钟 100 次限流（429），表现为主流程间歇失败。

## Wails 能力审计

设计原则：**Wails 已有的能力就集成，不重复造轮子**。每个宿主能力先查固定版本（`github.com/wailsapp/wails/v3 v3.0.0-beta.28`）的源码，确认存在再用；官方文档描述的是当前主线，不代表固定版本已有该 API，必须以模块源码为准，需要更高版本时先评估受控升级，不直接跳到 main。

下表证据均来自 beta.28 模块源码 `pkg/application`、`pkg/events`、`pkg/services`。

| 能力 | Wails beta.28 API（源码证据） | 当前实现 | 适配缺口 | 替换切片 / 验收 |
| --- | --- | --- | --- | --- |
| 开机自启 | `app.Autostart`：`Enable` / `EnableWithOptions` / `Disable` / `IsEnabled` / `Status`；选项 `Identifier`、`Arguments`；macOS 打包且 ≥13 用 `SMAppService`，否则（含裸二进制）写 LaunchAgent，Windows 写 `HKCU\...\Run`，Linux 写 XDG `.desktop`（`autostart_darwin.go`、`autostart_windows.go`、`autostart_linux.go`） | **重复实现**：`packages/desktop-host-go/autostart`（手写 LaunchAgent plist，仅 macOS）+ `apps/gui-go/autostart.go`（PR #1862） | 需保留：按 profile 区分登录项名（`Identifier`，开发实例不得改动已安装应用）、启动参数、与 daemon 设置的一致性、OS 失败回滚、启动对账。需验证：`SMAppService` 分支不接收 `Arguments`（`smAppServiceRegister()` 无参数）；Tauri 旧版在 `~/Library/LaunchAgents` 注册的项如何迁移或并存；注册仅在下次登录生效 | **单独替换切片（下一片）**：改用 `app.Autostart`，删除手写 OS 层；验收重复启停、真实平台机制（SMAppService / LaunchAgent）与 profile 隔离 |
| 通知 | `services/notifications`（`NotificationService`） | 已集成（`host_notifications.go`） | 仅 Tauri 通知插件语义适配（权限、点击回调事件） | 已覆盖，无替换 |
| 全局快捷键 | `app.GlobalShortcut`（`global_shortcut_darwin.go`、`global_shortcut_linux_x11.go`、`global_shortcut_linux_portal.go`；Windows 另有实现） | macOS：GPUI 辅助进程自己持有快捷键与双击修饰键（既定决策，Wails 不提供双击修饰键触发）；其他平台尚未实现 | Windows / Linux WebView 面板需用 `app.GlobalShortcut` 注册快捷键并在设置变更时重注册 | 第 17 片（Windows、Linux），采用 Wails API，不手写 |
| 原生对话框 | `app.Dialog`（OpenFile / SaveFile / Info / Error） | 已集成 | 无 | 已覆盖 |
| 托盘与菜单 | `app.SystemTray`、`app.NewMenu` | 已集成（`tray.go`、`tray_devices.go`） | 仅 Uni 业务菜单内容 | 已覆盖 |
| 单实例 | `application.Options.SingleInstance`（`single_instance_*.go`） | **未使用**；单实例由 daemon 锁与 PoC 的持久 daemon 检查间接保证 | 需审计：第二次启动应唤起已有窗口而不是起第二个 GUI | 审计切片：核查行为后决定直接启用 `SingleInstance` |
| 窗口事件、唤醒 | `events.Common.SystemDidWake` / `SystemWillSleep`、`events.Linux.SystemDidWake`、Windows `APMResumeSuspend`；窗口事件与 hook（`events.Common.WindowClosing`、`WindowLostFocus`） | 窗口事件已集成；**唤醒未实现** | 第 15 片的“原生唤醒源”应订阅这些事件，不自写 NSWorkspace / 电源通知代码；`LastCheckAt` 防抖仍是 Uni 业务 | 第 15 片：用 Wails 事件 + e2e 控制模拟唤醒 |
| 应用更新 | beta.28 的 `pkg/services` 仅有 `dock`、`fileserver`、`kvstore`、`log`、`notifications`、`sqlite`，**没有更新服务** | 自研 `internal/update`（Tauri 清单格式、minisign 签名含 trusted comment、macOS 原位替换） | Wails 不覆盖；发布格式与签名契约由现有 Tauri 发布流程决定，故保留自研 | 无替换；第 14 片验证生产公钥注入路径 |

审计规则：以后每个新宿主能力在实现前，先在本表补一行并写出源码证据；已完成的切片按此表回头审计，发现重复实现就列为替换切片，不因“已经写过”而保留。

## 验收边界

- Wails 与 runtime 同时固定为 `3.0.0-beta.28`；这是 beta 原型，不是生产迁移完成。
- daemon 的 CORS 只新增精确的 `wails://localhost` 来源，未扩大为任意来源。
- 首次启动可创建独立 daemon；已有兼容持久 daemon 会被复用；不兼容或 oneshot daemon
  会明确拒绝，不执行替换或强制结束。
- macOS SDK 的链接版本警告仍存在；本轮验证当前系统实际运行，不证明最低系统版本兼容。
- Windows/Linux、安装签名、更新、托盘、全局快捷键、原生粘贴与 GPUI 尚未验收。
- quiet 模式不证明真实窗口聚焦、视觉位置与真实全局快捷键；这些仍需可见模式的人工或原生验收。
