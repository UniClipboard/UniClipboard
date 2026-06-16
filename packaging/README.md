# 打包与分发渠道总览

本目录汇集 UniClipboard 各分发渠道的打包定义源。真正生效的副本在各渠道自己的仓库里（nixpkgs、Scoop Extras、homebrew-cask 等），这里保存源 + 提交指引，与 `packaging/aur/` 的惯例一致。

## 渠道全景

「Repology」列指该渠道是否被 [repology.org](https://repology.org/project/uniclipboard/versions) 抓取——即是否能让 packaging-status 徽章多一行。**Snap / COPR / 自建 Homebrew tap / npm / winget / Flathub 都不被 Repology 抓取**（已用 7zip 的完整源列表交叉验证）。

| 渠道 | 目录 | Repology | 状态 |
| --- | --- | --- | --- |
| AUR (`uniclipboard-git`) | `packaging/aur/` | ✅ 抓 | CI 自动同步（`aur.yml`） |
| COPR (Fedora/RHEL) | `packaging/uniclipboard.spec` | ❌ 不抓 | CI 自动发布（`copr.yml`） |
| Snap Store | `snap/snapcraft.yaml` | ❌ 不抓 | CI 自动发布（`snap.yml`） |
| Homebrew tap（自建） | 外部 `UniClipboard/homebrew-tap` | ❌ 不抓 | CI 自动更新（`homebrew-tap.yml`） |
| npm CLI | `npm/` | ❌ 不抓 | CI 自动发布（`npm-publish.yml`） |
| **nixpkgs** | `packaging/nix/` | ✅ 抓（+7 行） | **本次新增，待提交** |
| **Scoop Extras** | `packaging/scoop/` | ✅ 抓 | **本次新增，待提交** |
| Homebrew Cask（官方） | `packaging/homebrew/casks/` | ✅ 抓 | **origin/main 官方已实现（#1086）**：`homebrew-cask.yml` + 占位模板 |
| **Chocolatey** | `packaging/chocolatey/` | ✅ 抓 | **本次新增**，首版手动 → 后续 CI 自动（`choco-publish.yml`） |
| **winget** | `packaging/winget/` | ❌ 不抓 | **本次新增**，首版手动 → 后续 CI 自动（`winget-publish.yml`） |
| **Flathub** | `packaging/flathub/` | ❌ 不抓 | **本次新增，待提交** |

## 铺开优先级

按性价比（对 Repology 徽章的贡献 ÷ 落地难度）：

1. **nixpkgs** — 一次 PR ≈ 徽章 +7 行，可自荐，性价比最高。
2. **Scoop Extras** — Windows 侧最省力，autoupdate 自动维护。
3. ~~Homebrew Cask（官方）~~ — **已由 #1086 实现**（`homebrew-cask.yml` 在 release 时自动提 PR 到官方 homebrew-cask），无需再做。
4. **Chocolatey** — 有人工 moderation，较慢。
5. **winget** / **Flathub** — 不进 Repology，但分别补 Windows 官方包管理器 / Linux 桌面的真实分发缺口；Flathub 现场调试量最大。

## 通用约定：hash 一律不手填

所有 manifest 里的 sha256 都是占位（`0000…` 或 `REPLACE_WITH_SHA256` / `lib.fakeHash`）。**不要用任何不可信环境产出的哈希**——错误 hash 是供应链风险。每个渠道的 `README.md` 都给了用该渠道官方工具自动生成真值的命令（`nix-build` / `checkver.ps1 -Update` / `brew fetch` / `Get-RemoteChecksum` / `wingetcreate` / `sha256sum`）。所有 release 还自带 minisign 签名的 `SHA256SUMS.txt` 可交叉核对。

## 后续版本的自动化

首版上架后，各渠道的「后续版本升级」分三类：

- **上游 bot 自动**：nixpkgs（`r-ryantm`）、Scoop Extras（`excavator`）合并后由对方机器人按 release / manifest `autoupdate` 自动开升级 PR，本仓库无需 workflow。
- **本仓库 CI 自动**：Chocolatey 与 winget 无官方 bot，由 `.github/workflows/choco-publish.yml`、`winget-publish.yml` 在 stable release 发布后自动 push / 提 PR（监听 `release: published`、跳过 prerelease，与 `copr.yml`/`homebrew-tap.yml` 一致）。需配置 secret：
  - `CHOCOLATEY_API_KEY` — community.chocolatey.org 的 API key
  - `WINGET_TOKEN` — GitHub PAT（classic，`public_repo`），用于 fork `microsoft/winget-pkgs` 提 PR
- **官方 CI 自动**：Homebrew Cask 由 #1086 的 `homebrew-cask.yml` 在 release 时自动提 PR（见 `docs/packaging/homebrew-cask.md`）。

> 两个 CI workflow 首版都不接管——首版需人工提交 / 过审（见各子目录 README），workflow 从第二版起自动跟随。

## 各渠道提交指引

本次新增渠道见对应子目录的 `README.md`：`packaging/nix/`、`packaging/scoop/`、`packaging/chocolatey/`、`packaging/winget/`、`packaging/flathub/`。Homebrew Cask（官方）见 `docs/packaging/homebrew-cask.md`。
