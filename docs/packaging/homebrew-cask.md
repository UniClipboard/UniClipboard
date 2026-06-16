# Homebrew 官方 Cask 接入

本文记录 UniClipboard GUI 进入 `Homebrew/homebrew-cask` 的维护流程。CLI 仍由 `UniClipboard/homebrew-tap` 维护；GUI 的目标是让用户直接运行：

```bash
brew install --cask uniclipboard
```

## 范围

- 官方 Cask 只发布 macOS GUI：`UniClipboard.app`。
- Cask 下载 GitHub Release 中的 DMG：
  - Apple Silicon：`UniClipboard_<version>_aarch64.dmg`
  - Intel：`UniClipboard_<version>_x64.dmg`
- 版本只跟随 stable release；alpha、beta、rc 不提交到官方 Cask。
- 私有 tap 可继续保留 CLI formula，避免 GUI 和 CLI 的所有权混在一起。

## 文件

- `packaging/homebrew/casks/uniclipboard.rb` 是提交到 `Homebrew/homebrew-cask` 的 Cask 模板。
- `.github/workflows/homebrew-cask.yml` 会把模板中的版本和 SHA256 占位符替换成 release 资产的真实值，然后向官方仓库打开 PR。

## 首次接入步骤

1. 在 GitHub 创建或准备一个可推送分支的 fork，例如 `UniClipboard/homebrew-cask`。
2. 配置仓库 secret：`HOMEBREW_CASK_TOKEN`。该 token 需要能向 fork push，并能向 `Homebrew/homebrew-cask` 打开 PR。
3. 手动运行 `Homebrew Official Cask` workflow，填写 stable 版本号和 fork 仓库。
4. 等待 workflow 生成 PR 后，按 Homebrew 维护者反馈调整 `packaging/homebrew/casks/uniclipboard.rb`，保持模板为单一事实来源。

## 发布后维护

`release.published` 事件只会对非 prerelease 触发官方 Cask 提交流程。若 Homebrew 侧已有自动 bump 或维护者要求手动更新，可临时禁用 `.github/workflows/homebrew-cask.yml` 的 release 触发，只保留 `workflow_dispatch`。

## 本地校验

在 macOS 上可先渲染模板，再运行：

```bash
brew audit --cask --new Casks/u/uniclipboard.rb
brew install --cask ./Casks/u/uniclipboard.rb
brew uninstall --cask uniclipboard
```
