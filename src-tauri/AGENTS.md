# src-tauri 本地规则（Tauri 打包壳）

Rust workspace 的导航与知识库在 `crates/AGENTS.md`；本文件只覆盖 Tauri 打包相关内容。

## 定位

自根 workspace 重构后，本目录只保留 Tauri 打包所需的内容：

| 内容 | 说明 |
| --- | --- |
| `Cargo.toml` | `uniclipboard` bin 包（纯 package manifest，workspace 在仓库根） |
| `src/main.rs` | 12 行薄入口，移交 `uc_tauri::run(generate_context!())` |
| `crates/uc-tauri/` | Tauri 适配 crate：commands（tauri-specta）、tray、quick panel、run loop |
| `tauri.conf.json` / `tauri.dev.conf.json` | Tauri 配置；相对路径以本目录为基准（`frontendDist: "../dist"` 等） |
| `icons/`、`capabilities/`、`gen/` | 打包资源与能力声明 |
| `binaries/` | daemon sidecar 暂存（gitignored；由 `scripts/prepare-daemon-sidecar.mjs` 从根 `target/` staging 为 `uniclipd-<triple>`） |

## 必守事项

- 所有 cargo 命令从仓库根执行（workspace 根）；构建产物在根 `target/`。
- `tauri.conf.json` 的 `externalBin: binaries/uniclipd` 要求构建/检查 `uniclipboard` 包前先运行 `node scripts/prepare-daemon-sidecar.mjs --debug`，否则 tauri-build 校验 sidecar 资源失败。
- 改动 Tauri command 后跑 `cargo test -p uc-tauri --test specta_export` 重新生成 `src/lib/ipc-bindings.generated.ts` 并一并提交（规则详见 `docs/agent/rust-tauri-rules.md`）。
- `uc-tauri` 依赖根 `crates/` 下的库 crate（路径 `../../../crates/uc-*`）；不要在本目录新增业务逻辑 crate。
