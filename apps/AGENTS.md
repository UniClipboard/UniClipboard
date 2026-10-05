# apps 本地规则

`apps/` 存放可直接运行的生产程序；开发专用的 crate（如 Rust 开发 CLI `uc-dev-cli`）在 `tools/`。Rust 库 crate 一律放 `crates/`。`cli-go/` 是独立的 Go 模块，不属于 cargo workspace。Rust workspace 的导航与知识库见 `crates/AGENTS.md`。

| 目录 | 包名 | 产物 | 本地规则 |
| --- | --- | --- | --- |
| `cli-go/` | Go 模块 `github.com/UniClipboard/UniClipboard/apps/cli-go` | 用户端终端客户端 `uniclip`（Go 实现，发布产物由它构建） | `apps/cli-go/AGENTS.md` |
| `daemon/` | `uc-daemon` | `uniclipd` | （暂无；遵循 workspace 规则） |
| `quick-panel/` | `quick-panel` | GPUI 快捷面板（macOS 随安装包发布，可执行文件 `uniclip-quick-panel`） | `README.md` |
| `android-probe/`、`ios-probe/`、`ohos-probe/` | - | 移动端验收宿主应用（非 Rust） | 不发布；**当前不可构建**：它们依赖已移出本仓的 `uc-mobile-probe-core` 与 `uc-ohos-napi`（`scripts/architecture/check-engine-repository.mjs` 禁止其回到本仓），处置待定 |
| `gui/` | `uniclipboard`（`gui/src-tauri`）、`uniclipboard-gui`（前端，JS） | 桌面 GUI：Tauri + React。`gui/` 是标准 Tauri 项目根（`package.json`、`src/`、`src-tauri/`）；Tauri 适配 crate 为 `crates/uc-tauri` | `apps/gui/src/AGENTS.md`（前端）、`apps/gui/src-tauri/AGENTS.md`（打包壳） |

桌面 GUI 的全部文件（React 前端、Vite/TypeScript/Tailwind 配置、GUI 端到端测试、设计规范、Tauri 打包壳）都在 `apps/gui/`。`src-tauri/` 这个目录名沿用 Tauri 默认，位置不受限：tauri-cli 2.11.1 从仓库根或 `apps/gui/` 运行都能发现 `apps/gui/src-tauri/tauri.conf.json`（2026-10-02 用 `tauri info` 与带 `cwd` 的构建钩子实测）。仓库根的 `package.json` 只是 bun workspace 根和命令转发入口（`bun tauri:dev` 等照旧可用），GUI 依赖与脚本以 `apps/gui/package.json` 为准。

新增 app 时：路径依赖指向 `../../crates/uc-*`，在根 `Cargo.toml` 的 members 中注册，并补一行本表。
