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
| `dev-profile` 特性（`uc-platform`、`uc-bootstrap`） | 当前无人启用，已记录为清理项（见 #1911），不在本 PR 内处理 |

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
