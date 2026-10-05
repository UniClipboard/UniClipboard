# tools 本地规则

`tools/` 存放 **开发专用** 的 Rust crate：开发、诊断与 E2E 辅助程序。它们是 cargo workspace 成员，
所以 `cargo ... --workspace` 与 `cargo ... -p <crate>` 仍会构建和测试它们，但 **不进入任何生产构建**。

| 目录 | 包名 | 产物 | 本地规则 |
| --- | --- | --- | --- |
| `uc-dev-cli/` | `uc-dev-cli` | `uc-dev-cli`（`dev-tools` 命令：`probe`、`blob`、`dev`、`mobile debug`；Go 实现的兼容性对照基线） | `tools/uc-dev-cli/AGENTS.md` |

## 必守边界

- 工具 crate 必须是叶子：任何生产包都不得依赖 `tools/` 下的 crate（`scripts/architecture/check-engine-repository.mjs` 检查）。
- 工具 crate 设 `publish = false`，并且不在根 `Cargo.toml` 的 `default-members` 中；`default-members` 必须等于 `members` 去掉工具 crate（`scripts/__tests__/cli-packaging.test.ts` 检查）。
- 发布、打包与镜像构建（`build.yml`、`build-cli.yml`、`release.yml`、`deploy/vps/Dockerfile`、`scripts/prepare-sidecars.mjs`）不得引用工具 crate。
- 用户端终端客户端 `uniclip` 是 Go 模块 `apps/cli-go`，不是这里的 crate。

新增工具 crate：路径依赖指向 `../../crates/uc-*`，在根 `Cargo.toml` 的 `members` 中注册，并 **不要** 加入 `default-members`，再补一行本表。
