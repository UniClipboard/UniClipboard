# apps 本地规则

`apps/` 存放可直接运行的生产程序；开发专用的 crate（如 Rust 开发 CLI `uc-dev-cli`）在 `tools/`。Rust 库 crate 一律放 `crates/`。`cli-go/` 是独立的 Go 模块，不属于 cargo workspace。Rust workspace 的导航与知识库见 `crates/AGENTS.md`。

| 目录 | 包名 | 产物 | 本地规则 |
| --- | --- | --- | --- |
| `cli-go/` | Go 模块 `github.com/UniClipboard/UniClipboard/apps/cli-go` | 用户端终端客户端 `uniclip`（Go 实现，发布产物由它构建） | `apps/cli-go/AGENTS.md` |
| `daemon/` | `uc-daemon` | `uniclipd` | （暂无；遵循 workspace 规则） |
| `quick-panel/` | `quick-panel` | GPUI 快捷面板（macOS 随安装包发布，可执行文件 `uniclip-quick-panel`） | `README.md` |
| `android-probe/`、`ios-probe/`、`ohos-probe/` | - | 移动端验收宿主应用（非 Rust） | 不发布；**当前不可构建**：它们依赖已移出本仓的 `uc-mobile-probe-core` 与 `uc-ohos-napi`（`scripts/architecture/check-engine-repository.mjs` 禁止其回到本仓），处置待定 |
| `gui-go/` | Go 模块 `github.com/UniClipboard/UniClipboard/apps/gui-go` | 桌面 GUI 宿主（Go/Wails），目前唯一的桌面宿主；应用标识、版本与更新公钥的唯一来源是 `apps/gui-go/app.json` | `apps/gui-go/AGENTS.md` |
| `gui-go/frontend/` | `@uniclipboard/gui-go-frontend`（前端，JS） | 桌面 GUI 的 React 前端源码（`src/`）、测试、浏览器夹具与设计规范；随包产物由 `apps/gui-go/frontend/vite.config.ts` 构建，Go 只嵌入 `frontend/dist` | `apps/gui-go/frontend/src/AGENTS.md` |

桌面宿主（窗口、托盘、更新、打包）在 `apps/gui-go/`；React 前端在 `apps/gui-go/frontend`（源码 `src`），测试用 `bun run test`，类型检查用 `bun run typecheck`。开发运行用仓库根的 `bun wails:dev`；本地 macOS 构建与打包用 `apps/gui-go/build.sh`。仓库根的 `package.json` 只是 bun workspace 根和命令转发入口，GUI 依赖与脚本以 `apps/gui-go/frontend/package.json` 为准。旧 Tauri 宿主已退役，记录见 `docs/architecture/gui-go-tauri-retirement.md`。

新增 app 时：路径依赖指向 `../../crates/uc-*`，在根 `Cargo.toml` 的 members 中注册，并补一行本表。
