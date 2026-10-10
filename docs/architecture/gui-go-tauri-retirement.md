# Tauri 桌面宿主退役记录

Go/Wails 宿主（`apps/gui-go`）现在是唯一的桌面宿主。本文记录退役的失败模型、删除/迁移/保留清单、验证结果、对 CI 与发布的影响，以及余项 issue 与旧 OPEN 清单的逐条映射。

本文是历史记录，不是活文档：守卫 `scripts/architecture/check-tauri-retired.mjs` 允许它提到已退役的名称。

## 失败模型

退役要避免的失败，按先后顺序：

1. **断引用**：删除 `apps/gui/src-tauri` 会同时带走版本号、应用标识、更新公钥、图标与 NSIS 钩子。这些必须先迁到唯一维护位置，所有消费者随之更新。
2. **前端双份**：共享 React 前端源码留在 `apps/gui/src`，只由 `apps/gui-go/vite.config.ts` 构建；不复制第二份。
3. **假通过**：删除后仍能编译，但 Go 宿主 + 真实 daemon 的启动、身份、前端、daemon 链路已断。因此在删除前后各跑一次真实 E2E 并比对。
4. **回流**：已退役的路径、crate、脚本名被重新引入。由守卫 `bun run check:tauri-retired` 在 PR 门禁中拦截。
5. **发布静默坏掉**：Go 宿主还没有发布流水线。`release.yml` 的 `validate` 首步改为显式失败（fail-closed），而不是继续用已删除的 Tauri 构建产物。

## 变更清单

### 迁移（唯一来源）

| 内容 | 新位置 |
| --- | --- |
| 应用名、标识、版本、最低 macOS、更新公钥 | `apps/gui-go/app.json` |
| 图标 | `apps/gui-go/icons/` |
| NSIS 安装钩子 | `apps/gui-go/windows/installer-hooks.nsh` |
| 测试夹具 `google.png` | `tests/e2e/fixtures/` |
| 前端入口文档 | `apps/gui-go/frontend/index.html` |
| daemon 暂存脚本 | `scripts/stage-daemon.mjs`（原 `prepare-sidecars.mjs`） |
| 版本号升级 | `scripts/bump-version.js`：`package.json`、`apps/gui-go/app.json`、`Cargo.toml` |

### 删除

- `apps/gui/src-tauri`、`crates/uc-tauri`、`third_party/tao`、`patches/tao-0.35.3-pr1207.diff`。
- 根 `Cargo.toml` 的 `tauri`、`tauri-plugin-autostart` 依赖、`[patch.crates-io] tao`；`Cargo.lock` 只有移除，没有存活依赖的版本变化。
- wdio E2E 套件、辅助脚本与 `wdio-test-bridge`；`@tauri-apps/cli`、`@wdio/*`、`mocha`、`concurrently` 等开发依赖。
- 多个仅服务 Tauri 的脚本（`prepare-linux-bundle.mjs`、`check-linux-bundles.py`、`linux-appimage-tools.mjs`、`dev-update-loop.mjs`、`sweep-dev-port.mjs` 等）。
- `build.yml` 的 `build-gui` 作业、`alpha-build.yml`；`TAURI_CONFIG` 环境变量。

### 保留及原因

| 保留 | 原因 |
| --- | --- |
| 宿主命令契约 | 已由 Go `HostService` 方法签名与 Wails 生成的绑定取代，冻结的 `ipc-bindings.generated.ts`、`error-severity.generated.ts` 已删除，见 `docs/architecture/gui-go-host-commands.md` |
| React 前端（退役时在 `apps/gui/src`，后已并入 `apps/gui-go/frontend/src`） | 唯一前端 |
| Rust Engine、Iroh、加密存储、`uniclipd` | 与宿主无关 |
| `apps/quick-panel`、`crates/quick-panel-core`（GPUI） | macOS 原生快捷面板；已确认 GPUI 不依赖 tao |
| 开发 profile | 已移除无激活者的 Cargo 特性；Wails 启动器通过 `UC_PROFILE` 选择 profile，默认生产路径不变 |

### tao 的处置

`tao` 只经 `tauri-runtime-wry → tauri` 可达，GPUI 不使用。`tauri` 删除后 `cargo metadata --locked` 中不再出现 `tao`、`wry`，故一并删除补丁与源码。

### 暂缓移植的打包渠道

`snap/snapcraft.yaml`、`packaging/aur/uniclipboard-git/PKGBUILD`、`docs/packaging/AUR.md` 仍描述从源码构建 Tauri。`aur.yml` 会在推送到 main 时发布 `packaging/aur/**`，因此不在无法验证的情况下盲改。守卫把它们列为“待移植”，由 #1899 跟踪。**合入后 AUR `-git` 包将无法构建**，这是已知影响。

## 行为变化

- Go 宿主主文档由原 `apps/gui/index.html` 提升而来，因此获得启动画面与主题闪烁修复。
- macOS 的 `Info.plist` 现在写入 `LSMinimumSystemVersion`（来自 `app.json`）。

## 验证

- 退役前基线与退役后（冻结提交 `96c2d7816`）各跑一次真实 Go GUI + 真实 daemon 的 E2E，均 rc=0，布尔判定项完全一致。
- `bun run typecheck`、根与 `apps/gui` 的 vitest、`go build -tags production ./...`、`go vet -tags e2e .`、`cargo check --workspace --locked`、`cargo check -p quick-panel --locked`、`check:engine-repository`、`check:tauri-retired`。
- 隔离的 `scripts/wails-dev.mjs` 冒烟：Vite、Go 开发二进制、真实 `uniclipd` 均启动；应用对未按规则隔离的 HOME/profile 会拒绝启动（符合预期）；结束时仅停止本任务启动的进程。
- 只在 macOS（arm64）实测。Windows、Linux 原生验证未做，见下方 issue。

## 对 CI 与发布的影响

- PR 门禁：前端作业跑 `typecheck` 与 Wails 前端构建；Rust 作业跑 `cargo check --workspace`、守卫与 OpenAPI 漂移检查。
- **发布被阻塞**：`release.yml` 在 Go 发布流水线落地前会显式失败。解除条件是 #1895 至 #1899。macOS 应用包的构建、签名、公证、DMG 与更新归档（#1895）已由 `build.yml` 的 `package-macos-gui` 作业实现，其验收状态与缺口见 `apps/gui-go/README.md`「macOS 发布构建」；更新签名（#1896）及 Windows、Linux 安装包（#1897、#1898）仍待完成，因此守卫保持不变。
- 现有已发布版本的更新公钥未变（与旧 `tauri.conf.json` 逐字节一致）。

## Issue 索引

| Issue | 主题 |
| --- | --- |
| #1895 | macOS 发布包构建、签名、公证 |
| #1896 | 更新签名与 feed，并用真实签名发布验证 |
| #1897 | Windows 安装器、便携包与更新包 |
| #1898 | Linux deb、rpm、AppImage 与更新归档（含原生 amd64） |
| #1899 | 分发渠道移植与 `apps/gui-go` 的 PR 构建门禁 |
| #1900 | 从 Tauri 应用升级与身份连续性 |
| #1901 | Windows 真机原生验收 |
| #1902 | Linux 真实桌面环境验收 |
| #1903 | Linux 运行时依赖与打包矩阵 |
| #1904 | Linux 代理与环境解析（R1 至 R5、PAC、F7） |
| #1905 | macOS 长时间运行与真实焦点/粘贴 |
| #1906 | macOS 托盘菜单验收缺口（含 #1893 `coverage` 失败） |
| #1907 | macOS 持有窗口重放 SIGSEGV 根因与窗口生命周期 |
| #1908 | 共享前端宿主契约审计（62 个命令、事件） |
| #1909 | UI 验收：多图片、修饰键回滚、实时历史 |
| #1910 | 将 wdio 多端 E2E 移植到 Go 宿主 |
| #1911 | 退役遗留与未验证项 |

这些 issue 都没有被修复；其中记录的失败与未测边界保持原样。#1893 的结果仍是 PARTIAL，不得视为全部通过。

## 旧 OPEN 清单映射

| 旧 OPEN 项 | Issue |
| --- | --- |
| 正式签名发布 / 生产 feed | #1896 |
| macOS release-no-profile | #1900（入口形态）、#1895（构建） |
| macOS immutable Engine / identity / legacy 自启 / multi-profile / login restore | #1900 |
| macOS main（原文含义不明，暂按“主应用旧项迁移”处理） | #1900（不确定，待澄清） |
| 真实登录/注销自启、更新后自启 | Linux：#1902、#1903；macOS、Windows：#1900、#1897 |
| Windows 真实 Rust daemon 的 NSIS/便携包安装、卸载、更新、自启、签名 | #1897（覆盖 Tauri 副本的更新亦见 #1900） |
| Windows 原生键、双击、焦点、粘贴、冲突、托盘、通知、单实例、强制结束 | #1901 |
| Linux 原生 amd64、deb/rpm 安装与升级、其他发行版、dlopen/Mesa/glibc | #1898、#1903 |
| AppImage extract-and-run / 只读 / legacy 边界 | #1903 |
| Tauri 包 libdbus/helper 行为、Tauri AppImage 强制 X11 | #1903（升级上下文亦见 #1900） |
| Engine 无默认路由启动 | #1903 |
| GNOME / KDE / portal / FileManager1 / 真实 GPU / HiDPI / Hyprland / 通知 / suspend | #1902 |
| Linux 真实快捷键 / 焦点 / 粘贴 | #1902 |
| Linux 代理 PAC 决策、Go 更新器读 GNOME 设置、F7、`GDK_BACKEND=""`、R1 至 R5 | #1904 |
| macOS 焦点 / 粘贴、睡眠 / App Nap 6 小时、电池 `shouldDefer`、清醒启动对比 `ae8738f2a`、托盘重复通知 | #1905 |
| 托盘 3b 消失、`devices://sync-changed`、真实指针 hover、两轮完整通过与独立审查、Linux/Windows 真实托盘（Windows 部分见 #1901，Linux 部分见 #1902）、双语言、checkbox、`wails:dev` 通知、#1893 `coverage` 失败 | #1906 |
| 窗口重放 SIGSEGV | #1907 |
| 62 个命令 / 前端 / 产品 / 宿主审计 | #1908 |
| 修饰键保存失败回滚、多图片真实 UI | #1909 |
| wdio 多端套件移植 | #1910 |
| 退役遗留项（含 `dev-profile`、Sentry source map 等） | #1911 |

无法放置的项：旧文本中含义不明的“macOS main”（见上），以及“Swift 同次调用 owner/role 保存”（在 #1906 中标注为含义不清）。

## 后续更新：前端并入 Go 宿主目录

React 前端的源码、资源、测试、浏览器夹具、`package.json`、Vite/Vitest 与 TypeScript 配置已从 `apps/gui` 迁入 `apps/gui-go/frontend`，`apps/gui` 目录不再存在（守卫把 `apps/gui` 列为退役路径）。同时移除了 `@tauri-apps/*` 的包名别名与三个 npm 依赖：页面直接导入 `apps/gui-go/frontend/src/host` 下的宿主模块（`@/host/event`、`@/host/window` 等），`check:tauri-retired` 拒绝任何 `@tauri-apps/` 引用或依赖。上文正文保留退役当时的路径与做法，仅作历史记录。


## 后续清理核对（#1911）

核对基线是 `add157ed31250525a07cf0199fa27fa370c1cf41`（PR #1940）。PR #1934 的发布链、
#1936 的 Wails 生成绑定、#1939 的文档已经合入。这里记录源码结论与验收边界，不能据此宣布生产发布完成。

- **前端边界**：使用 `@/host/*` 和 `__UC_DESKTOP_HOST__`。剩余平台检测和 Sentry 初始标签
  已改为复用平台检测；退役门禁同时拒绝旧全局标记。CI 调用 `check:tauri-retired`。
- **profile 兼容**：删除无激活者的 Cargo 特性和默认 profile 函数。`uc-app-paths` 仍拥有
  路径策略，运行时 `UC_PROFILE` 保留；空值/未设置仍使用 `app.uniclipboard.desktop`，
  非空仍追加 `-<profile>`。缓存、钥匙串 service、升级备份目录同样使用运行时 profile；
  日志仍走 `app_log_dir()`，portable 路径策略未改。曾手工激活旧特性的开发者须显式设置
  `UC_PROFILE=dev`，不自动迁移任何用户数据。
- **编译器**：保留 Rust 1.95.0 构建基线；锁定依赖声明最高 MSRV 为 1.91（iroh/openmls）。
  这是声明值的审计，不是所有目标能降级的证明；未进行编译器降级。
- **Linux Rust 作业**：coverage/cache-warmup 仅构建 Rust 工作区，移除 GTK/WebKit、
  AppIndicator、rsvg、patchelf。`uc-platform → keyring → libdbus-sys` 使用 libdbus；
  GPUI 的测试目标实际链接 FreeType、xcb、xkbcommon 与 xkbcommon-x11；
  `libxkbcommon-x11-dev` 传递安装 xcb 开发包，明确安装 `libfreetype6-dev`。
  Wayland 和 Fontconfig 的动态加载不要求这两个库的开发包；
  coverage 使用 Xvfb/xauth。当前 Linux clipboard 是原生 X11/Wayland，macOS/Windows 才使用
  clipboard-rs；锁文件没有 arboard。Wails 的 GTK/WebKit 和 Linux 打包工具仍由 Go GUI 作业安装，
  不可从产品构建统一删除。隔离 Debian arm64 容器验证与 Ubuntu 远端 CI、真实桌面是不同验收。
- **Sentry source map**：发布工作流已对 macOS/Windows 注入 token/project，插件只在两者都有时启用，
  并删除上传后的 map。macOS 打包入口修正到 frontend；所有本地/Windows 构建传入应用版本，
  上传时缺失版本明确失败。macOS 测试构建与 Windows 一样不注入生产 WebView DSN。
  `app.json.version → VITE_APP_VERSION → 插件 release.name/WebView release` 是版本链。
  仍未完成真实 token 上传与线上 `.tsx` 行号解析；token/project 只影响构建，用户同意开关
  控制运行时发送，两者不可混为一谈。授权验收应使用隔离 Sentry 项目：记录源码 SHA、版本、
  环境、架构、bundle/map SHA，确认打包无 map，开启同意触发可定位错误，保存 event id 与
  原始 `.tsx` 文件/行号；随后关闭同意验证无 envelope 外发。不能用本地 map 存在宣称线上解析成功。
- **macOS 最低版本**：继续使用 `app.json` 的 12.5，打包器传递给 cgo deployment target 和 plist。
  固定 Wails beta.28 的通知实现有 11/12 的 availability 检查，SMAppService 有 13 的检查和
  LaunchAgent 回退；这只能解释已知 API 路径，不能证明完整链接图、GPUI/Metal、AX 或运行时兼容。
  缺少 12.5 隔离宿主上的启动、托盘、面板、通知证据；新 macOS 构建通过不算 12.5 支持。
- **构建所有权**：`stage-daemon.mjs` 是 daemon/macOS helper 的构建和 staging 入口；
  `build.sh` 以 `--debug` 调用并复制这次 staging 的两个文件到 app 同级。
  生产 packager 只消费指定架构的 release sidecar，并保留现有 provenance 校验。
  本地 debug bundle 不作为 release sidecar 验收；不从其他线程复制旧产物。
- **宿主观测决策**：Go 宿主不引入独立 Sentry/OTLP SDK，也不恢复旧 Desktop OTLP 管线（`uc-bootstrap` 当前使用 Sentry Logs；Engine 依赖图仍
  包含 OTLP，不能由 Go SDK 不存在推断 daemon 的所有导出行为）。Go 更新分析事件继续经 daemon `/analytics/capture` 发送，由 daemon
  使用情况统计同意开关控制；WebView Sentry 有 transport 级遥测同意门禁，启动默认关闭。
  Go `log.Printf` 和 panic 当前仍是 stderr，端到端脚本把它保存为 GUI 工件；这不是应用内的
  每日 `uniclipboard-gui.json.<date>` 持久日志，也不保证 Go 崩溃可在产品诊断 ZIP 取到。
  `packages/desktop-host-go/diaglogs` 只收集已有日志，不能据此宣称 Go 已写日志。
  当前保留此诊断缺口，不通过复用 daemon/WebView 日志冒充 Go 宿主日志，也不未经授权开启远端上传。

### 删除测试的逐文件映射

`python3 .planning/retirement/retired-host-test-inventory.py` 可从 `de8381d54` 重新提取完整测试名。
36 个文件共 **207** 个函数：204 个无参数属性加 3 个带参数 Tokio 属性；原 issue 的 204 少计后者。
下表路径相对当时的 `crates/uc-tauri/src`；脚本名相对当前 `apps/gui-go/e2e`。
“替代”表示检查了脚本的真实断言，**不表示本轮全部运行通过**。“缺口”不能标为功能 dropped；
“dropped”仅表示已删除宿主专属实现或 wire 编码的测试不再适用。未逐项证明的边界显式保留。

| 历史文件 | 函数数 | 映射状态 | 真实断言及未覆盖边界 |
| --- | ---: | --- | --- |
| `activity_hud/actions.rs` | 1 | 缺口 | 没有 Go 原生活动浮窗；取消结果显示/恢复缺少等价端到端覆盖，不以下载取消脚本代替。 |
| `activity_hud/clock.rs` | 3 | dropped | 旧浮窗专用时钟与 Send/Sync 实现测试随实现退役；不代表活动浮窗功能已迁移。 |
| `activity_hud/emitter.rs` | 12 | 缺口 | 没有旧浮窗事件监听器的 Go 等价行为；入站/出站区分、迟到取消与重试归属未覆盖。 |
| `activity_hud/state.rs` | 16 | 缺口 | 旧浮窗聚合状态机无 Go 等价物；速度窗口、终态、过期事件与失败保留未覆盖。 |
| `activity_hud/ui/macos.rs` | 15 | 缺口 | 四窗口布局、展开、悬浮关闭、辅助功能与玻璃回退无 Go 原生活动浮窗覆盖。 |
| `commands/config.rs` | 3 | 部分替代 | `config_package_run.py` 的 STEPS 验证错误密码/预览/取消/重启导入；旧 IPC 枚举编码 dropped（已换 Wails），非 HTTP 错误分级仍无等价断言。 |
| `commands/mod.rs` | 1 | 部分替代 | `host_contract_run.py` 通过真实绑定读取设备元数据；旧 Rust DTO 编码 dropped，生成绑定由 `check:host-contract` 门禁负责。 |
| `commands/quick_panel.rs` | 2 | 部分替代 | `linux_xvfb_run.py` 验证模拟 Wayland 时拒绝双击修饰键，X11 接受；真实 Wayland 由 `linux_wayland_run.py` 验证，绝不能把模拟环境当真实桌面。 |
| `commands/settings.rs` | 3 | 部分替代 | `quick_panel_settings_run.py` 验证禁用/启用与尺寸设置；null 删除快捷键覆盖及并发启动预留的等价端到端断言未确认。 |
| `commands/severity.rs` | 3 | dropped/缺口 | Rust command severity 表随旧 IPC 退役；Wails 错误映射需由绑定与端到端错误路径覆盖，未证明所有旧错误码分类完整。 |
| `commands/startup.rs` | 4 | 部分替代 | `startup_run.py` 验证静默/轻量/正常启动；`single_instance_run.py` 验证启动中挂起显示；新版 daemon 拒绝分类与引导失败快照需额外故障注入。 |
| `commands/storage.rs` | 1 | 部分替代 | `file_ops_run.py` 验证保存字节与安全文件名；恶意 basename 全矩阵未确认，不视作全覆盖。 |
| `commands/updater.rs` | 11 | 部分替代 | `updater_signatures_run.py` 和 `update_manifest_run.py` 验证真实下载/签名与清单；`update_wake_run.py` 断言失败分类/事件顺序。旧 Rust DTO 编码 dropped，未知渠道/全部分类仍有缺口。 |
| `desktop_theme/omarchy/tests.rs` | 6 | 缺口 | `host_contract_run.py` 只断言非 Omarchy 环境返回不可用；脚本化 Hyprland 不能证明 Omarchy 调色板。调色板语义、损坏内容脱敏和目录替换监听没有等价验收。 |
| `desktop_theme/preferences.rs` | 2 | 缺口 | 没有证实持久化主题损坏与写入失败的端到端路径；不能以主题 API 存在替代。 |
| `lightweight.rs` | 9 | 部分替代 | `startup_run.py` 断言轻量冷启 GUI 退出/daemon 保留与再开可见；`run.py` 两轮验证 keep/full 退出与 daemon 生命周期；全部旧退出真值表未覆盖。 |
| `main_window.rs` | 16 | dropped/部分替代 | 旧 Tauri 的两阶段 frame/page readiness 与窗口代际测试随实现退役；Go 由宿主直接显示，`startup_run.py`/`single_instance_run.py` 验证可见性和早到请求。超时/销毁竞争无等价故障注入。 |
| `modifier_double_tap_platform.rs` | 2 | 部分替代 | `linux_xvfb_run.py` 的真实 XTEST 双击与模拟 Wayland 拒绝；真实 Wayland 环境另见 `linux_wayland_run.py`。 |
| `process_environment.rs` | 8 | 部分替代 | `linux_appimage_guard_downstream_run.py` 断言内嵌/系统 GIO 模块选择；`linux_appimage_proxy_run.py`/`linux_appimage_tls_run.py` 验证请求与 TLS；全部变量归一化组合未证明。 |
| `quick_panel/linux.rs` | 3 | 部分替代 | `linux_wayland_run.py` 验证 layer shell 实际布局，`linux_x11_wm_run.py` 使用窗口管理器；完整 offset/小屏矩阵未证明。 |
| `quick_panel/mod.rs` | 19 | 部分替代 | `quick_panel_settings_run.py` 的 panel-center/follow-near/flipped/disabled/reenabled 真实调用；双 toggle 抵消、每个轴 offset/夹紧及缩放矩阵未全覆盖。 |
| `quick_panel/native.rs` | 1 | 部分替代 | `quick_panel_settings_run.py` 设置 WebView 路径；GPUI/helper 专属迁移由其他线程处理，本任务不重复改动，默认/覆盖环境变量矩阵未运行。 |
| `quick_panel/paste_sequence.rs` | 4 | 缺口 | `windows_quick_panel_run.py` 的真实目标粘贴只是部分替代；UTF-16 逐单元与 Alt 按下期间 Ctrl-V 顺序需要专属测试主机，不在个人主机执行。 |
| `run.rs` | 4 | 替代 | `single_instance_run.py` 验证普通/quick-panel 第二次启动、首实例 quick-panel 拒绝与早到请求；本次未跑该完整脚本。 |
| `runtime_environment.rs` | 2 | dropped/替代 | 生产取消旧禁用单实例环境开关（显式宿主决策）；单实例隔离与第二实例退出由 `single_instance_run.py` 行为断言替代。 |
| `tray.rs` | 1 | 部分替代 | `tray_devices_run.py` 的 tray-menu-zh/language-set 断言语言切换；每个支持语言的完整标签矩阵未确认。 |
| `tray/device_sync.rs` | 4 | 部分替代 | `tray_devices_run.py` 列表/单设备开关与语言切换；离线列表、拒绝保存以及保留所有其他设备选项仍需故障/多设备扩展。 |
| `update_scheduler/last_check_at.rs` | 5 | 部分替代 | `update_wake_run.py` 断言最近检查跳过/过期唤醒/手动与托盘更新 last check；系统时钟倒退边界未覆盖。 |
| `update_scheduler/last_notified.rs` | 9 | 部分替代 | `scheduler_run.py` 断言版本落盘且至少三次后续 feed 检查不重复弹窗；缺失/损坏/多渠道/重启去重未完整覆盖。 |
| `update_scheduler/prompt_throttle.rs` | 6 | 部分替代 | `scheduler_run.py` 读取持久时间戳与同版本不重复提示；未来时间戳、完整 cooldown 时间和损坏文件未覆盖。 |
| `update_scheduler/scheduler.rs` | 13 | 部分替代 | `update_wake_run.py` 的 E_cadenceGapSeconds 真实间隔、setup 后检查、下载去重/新版本下载；生产 6 小时 jitter、失败 30 分钟、全部安装类型矩阵未运行。 |
| `update_scheduler/skipped_version.rs` | 3 | 缺口 | Go 有 skip 状态，但没有确认跨重启/多渠道忽略版本的真实行为断言；不能把文件或函数存在算覆盖。 |
| `visual_effects.rs` | 8 | 缺口 | Go 有 effects 实现，主启动脚本不证明能力策略/销毁窗口聚合/过期报告；旧策略行为的等价 E2E 未确认。 |
| `visual_effects_probe.rs` | 3 | 缺口 | 冷发现、超时保守回退、宿主启动预算未在 Go 上做等价性能验收；构建成功不代表已验收。 |
| `visual_effects_storage.rs` | 2 | 缺口 | 落盘/重启恢复与保留损坏文件未发现等价端到端断言。 |
| `window_frame_environment.rs` | 2 | 部分替代 | `linux_window_frame_run.py` 断言真实窗口 decoration 默认为 false/切 true/切 false；所有 desktop/session 名优先级未覆盖。 |

| 另删文件 | 替代或 dropped 理由 |
| --- | --- |
| `tests/specta_export.rs` | dropped：旧 IPC/schema 生成器已删除。当前 `gen-host-bindings.mjs` 和 `check:host-contract` 对 Go 方法/生成绑定做漂移门禁；运行时绑定由 `host_contract_run.py` 验证。 |
| `examples/layer_shell_smoke.rs` | dropped：旧 GTK layer-shell 手工示例不再可运行。`linux_wayland_run.py` 在隔离 compositor 验证 Wails layer shell 的实际布局/关闭等行为，Xvfb 不能替代此验收。 |


### profile/sidecar 集成验收的失败模式

编写隔离验收脚本前列出：Go CLI 与 Rust daemon 解析到不同数据目录；未设置或空 profile
意外追加 dev 后缀；显式 profile 丢失；portable 忽略 executable 相邻 data 目录；日志写回数据根
而非平台日志目录；打包复制了不同源码/架构/构建模式的 sidecar；daemon 生命周期失败或退出后
遗留本轮进程；使用真实系统剪贴板、钥匙串或真实 HOME；CLI 误连其他 profile 的 daemon。
验收从本轮 debug staging 复制 sidecar、使用新建沙箱和 development 文件密钥库、禁用系统
剪贴板，保留每次命令结果及产物哈希；不启动 GUI 或登录项，不以单次健康响应代替重启后可查询行为。

审查补充的夹具失败模式：最终 stop 命令报错但进程恰好已退出，仍误报通过；macOS 命令名
被误当作可执行文件路径，导致来源误判或漏检残留进程；Git diff 子进程阻塞或超时后未留下
失败工件。分别要求命令退出码与进程退出都通过、使用系统可执行路径接口、限定 diff 时间并
将超时纳入 assertions.json 的失败记录。
