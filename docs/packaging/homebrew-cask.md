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

Cask 一旦进入官方仓库并带有 `livecheck` stanza，Homebrew 的 **BrewTestBot** 会自动检测新 stable release 并自动提交纯版本 bump（只改 `version` + `sha256`）的 PR。因此 **版本升级不再需要我们主动提 PR**，`.github/workflows/homebrew-cask.yml` 已移除 `release.published` 自动触发，只保留 `workflow_dispatch`。

手动运行该 workflow 的场景仅限 BrewTestBot 不会代劳的情况：

- **首次新增 cask**（BrewTestBot 不会从零创建）。
- **元数据变更**（`desc` / `zap` / `depends_on` / `livecheck` 等）。这类改动应与版本 bump 分开提，单独一个 PR。

> **注意：模板必须与上游保持一致。** workflow 采用整文件替换（`cp ... > Casks/u/uniclipboard.rb`），所以模板里任何与上游 cask 不一致的字段都会搭着这次提交一起进 PR。历史上模板 `desc` 漂移成 `"Privacy-first cross-device clipboard sync"`，把元数据修改夹进版本 bump PR，导致 [homebrew-cask#272803](https://github.com/Homebrew/homebrew-cask/pull/272803) 被维护者以"改动范围超出 bump"为由关闭。修改元数据前先确认上游当前值，避免夹带。

## 本地校验

在 macOS 上可先渲染模板，再运行：

```bash
brew audit --cask --new Casks/u/uniclipboard.rb
brew install --cask ./Casks/u/uniclipboard.rb
brew uninstall --cask uniclipboard
```
