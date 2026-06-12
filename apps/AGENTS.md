# apps 本地规则

`apps/` 存放可直接运行的二进制 crate；库 crate 一律放 `crates/`。Rust workspace 的导航与知识库见 `crates/AGENTS.md`。

| 目录 | 包名 | 产物 | 本地规则 |
| --- | --- | --- | --- |
| `cli/` | `uc-cli` | `uniclip` | `apps/cli/AGENTS.md` |
| `daemon/` | `uc-daemon` | `uniclipd` | （暂无；遵循 workspace 规则） |
| `../src-tauri/`（物理位置见说明） | `uniclipboard` | 桌面 GUI（Tauri） | `src-tauri/AGENTS.md` |

桌面 GUI 在逻辑上也是一个 app，但物理目录必须叫 `src-tauri/` 且位于仓库根——这是 tauri-cli 的项目发现约定（`src-tauri/` + `tauri.conf.json`），官方不支持重命名，所以它不放在本目录下。

未来 iOS / Android 的 app core crate 也放在这里。新增 app 时：路径依赖指向 `../../crates/uc-*`，在根 `Cargo.toml` 的 members 中注册，并补一行本表。
