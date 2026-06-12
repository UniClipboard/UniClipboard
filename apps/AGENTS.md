# apps 本地规则

`apps/` 存放可直接运行的二进制 crate；库 crate 一律放 `crates/`。Rust workspace 的导航与知识库见 `crates/AGENTS.md`。

| 目录 | 包名 | 产物 | 本地规则 |
| --- | --- | --- | --- |
| `cli/` | `uc-cli` | `uniclip` | `apps/cli/AGENTS.md` |
| `daemon/` | `uc-daemon` | `uniclipd` | （暂无；遵循 workspace 规则） |
| `../src-tauri/`（物理位置见说明） | `uniclipboard` | 桌面 GUI（Tauri） | `src-tauri/AGENTS.md` |
| `ios/` | —（git submodule） | iOS App | 独立仓库 [uc-ios](https://github.com/UniClipboard/uc-ios)，遵循其仓库内规则 |
| `android/` | —（git submodule） | Android App | 独立仓库 [uc-android](https://github.com/UniClipboard/uc-android)，遵循其仓库内规则 |

桌面 GUI 在逻辑上也是一个 app，但物理目录必须叫 `src-tauri/` 且位于仓库根——这是 tauri-cli 的项目发现约定（`src-tauri/` + `tauri.conf.json`），官方不支持重命名，所以它不放在本目录下。

iOS / Android 客户端以 git submodule 形式挂载（不是 workspace 成员，cargo 不会构建它们）；未来若有共享的 app core crate，仍放在本目录下。新增 Rust app 时：路径依赖指向 `../../crates/uc-*`，在根 `Cargo.toml` 的 members 中注册，并补一行本表。
