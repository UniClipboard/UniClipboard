# 项目概览

UniClipboard 为个人保存加密的剪贴板历史，并通过 P2P 连接在自己的设备之间同步内容。产品范围和安全要求见 [VISION.md](../VISION.md)。

## 运行时结构

```text
Go/Wails + React GUI ──┐
原生 Quick Panel ──────┼── HTTP / WebSocket ── uniclipd ── uc-engine ── P2P 对等设备
Go CLI（uniclip）──────┘
```

GUI、Quick Panel 和 CLI 是 daemon 的客户端。它们不打开核心数据库，也不运行 P2P 节点。daemon 提供本机 HTTP 与 WebSocket 服务，并把业务操作交给 Engine。

Engine 拥有设备身份、配对、加密存储、搜索、连接、传输和后台任务。桌面宿主提供目录、系统安全存储、系统剪贴板和文件访问能力。消费边界见 [Engine 仓库消费者检查](architecture/engine-repository-checks.md)。

## 源码位置

| 位置 | 职责 |
| --- | --- |
| `apps/gui-go/` | Go/Wails 桌面宿主、托盘、窗口、更新和打包 |
| `apps/gui-go/frontend/` | React 19、TypeScript、Vite 界面 |
| `apps/quick-panel/`、`crates/quick-panel-core/` | 原生快捷面板及共享模型 |
| `apps/cli-go/` | 用户 CLI `uniclip` |
| `apps/daemon/` | `uniclipd` 入口与 Engine 生命周期 |
| `crates/uc-bootstrap/` | 桌面路径、系统适配器和 Engine 宿主能力准备 |
| `crates/uc-platform/` | 桌面系统能力适配 |
| `crates/uc-webserver/` | daemon HTTP 与 WebSocket 服务 |
| `crates/uc-daemon-{contract,client,local,process}/` | daemon 合同、客户端、连接发现和进程操作 |
| `crates/uc-app-paths/`、`crates/uc-desktop-settings/`、`crates/uc-observability/` | 桌面路径、设置和观测接入 |
| `tools/uc-dev-cli/` | 开发诊断工具，不属于正式 CLI |
| `tests/e2e/`、`scripts/e2e/` | 端到端验证与工件生成 |

完整 Rust 成员列表以根 `Cargo.toml` 为准。`uc-engine` 来自 `UniClipboard/Engine` 的固定提交；`uc-core`、`uc-application`、`uc-infra` 不在本仓维护。

主窗口使用 `@/ui-kit`。构建通过 `UC_UI_PLATFORM=windows|macos|linux` 选择皮肤。共享状态与 API 缓存使用 Redux Toolkit 和 RTK Query。界面规范见 [DESIGN.md](../apps/gui-go/frontend/DESIGN.md)。

## 开发命令

先安装 Bun、Node.js、Rust 和 Go。Rust 版本以 `rust-toolchain.toml` 为准。Go 版本以相应模块的 `go.mod` 为准。

从仓库根目录执行：

```bash
bun install --frozen-lockfile

# 启动独立开发资料的完整桌面应用。
bun run wails:dev

# 仅启动指定平台的前端。
UC_UI_PLATFORM=macos bun run --cwd apps/gui-go/frontend dev

# 检查前端类型。
bun run typecheck

# 检查三端构建隔离，并生成模块图证据。
bun run check:ui-kit-bundles target/ui-kit-bundles target/ui-kit-bundles.txt
```

桌面构建与打包见 [宿主说明](../apps/gui-go/README.md)。更多验证和发布入口见 [文档首页](README.md)。
