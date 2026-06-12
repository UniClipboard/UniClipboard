# apps 本地规则

`apps/` 存放可直接运行的二进制 crate；库 crate 一律放 `crates/`。Rust workspace 的导航与知识库见 `crates/AGENTS.md`。

| 目录 | 包名 | 产物 | 本地规则 |
| --- | --- | --- | --- |
| `cli/` | `uc-cli` | `uniclip` | `apps/cli/AGENTS.md` |
| `daemon/` | `uc-daemon` | `uniclipd` | （暂无；遵循 workspace 规则） |
| `../src-tauri/`（物理位置见说明） | `uniclipboard` | 桌面 GUI（Tauri） | `src-tauri/AGENTS.md` |
| `ios/` | —（非 Rust crate） | iOS App（Swift/Xcode） | 自带工具链与规则，根工具链不介入 |
| `android/` | —（非 Rust crate） | Android App（React Native） | 自带工具链与规则，根工具链不介入 |

桌面 GUI 在逻辑上也是一个 app，但物理目录必须叫 `src-tauri/` 且位于仓库根——这是 tauri-cli 的项目发现约定（`src-tauri/` + `tauri.conf.json`），官方不支持重命名，所以它不放在本目录下。

iOS / Android 客户端经 git subtree 从原独立仓库（uc-ios / uc-android）导入，完整历史已保留在本仓库。它们不是 cargo workspace 成员，cargo 不会构建它们；各自的格式化/lint 工具链独立，根 `eslint.config.js` 与 `.prettierignore` 已显式排除这两个目录。未来承载共享业务逻辑的 mobile core crate 放 `crates/`（库），FFI 绑定层与各端工程如何对接届时再补充本表。

新增 Rust app 时：路径依赖指向 `../../crates/uc-*`，在根 `Cargo.toml` 的 members 中注册，并补一行本表。
