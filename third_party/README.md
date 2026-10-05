# third_party

本目录存放需要本地修补的第三方 crate 源码，通过根 `Cargo.toml` 的 `[patch.crates-io]` 引用。
每个条目只能是临时措施：上游发布修复后必须删除目录和对应的 `[patch]`。

## tao

- 基线：crates.io 发布的 `tao 0.35.3`（Apache-2.0，见 `tao/LICENSE`）。
- 修改：应用上游 PR [tauri-apps/tao#1207](https://github.com/tauri-apps/tao/pull/1207)（`fix(macos): keep webview synced during window zoom`），原始补丁见 `patches/tao-0.35.3-pr1207.diff`。
- 目的：macOS 上通过缩放按钮或标题栏双击缩放窗口时，`WKWebView` 滞后于 `NSWindow`，窗口出现空白边。
- 本地精简：删除了 `examples/` 及对应的 `[[example]]` 条目，其余与上游发布包一致。
- 删除条件：该 PR 合并并随 tao 发布后，升级 `tao` 版本，删除 `third_party/tao`、`patches/tao-0.35.3-pr1207.diff` 和根 `Cargo.toml` 中的 `[patch.crates-io]`。
