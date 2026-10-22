# 包管理器版本状态

`README.md` 和 `README_ZH.md` 的安装章节采用“平台／包管理器／当前版本／安装命令”四列表格，主要渠道直接引用 Shields.io 的原生版本徽章。完整渠道状态继续保存在 `package-status` 分支，两版 README 都链接到该快照。版本来自对应渠道已合并的打包定义或公开仓库元数据，不以本仓库的 `package.json`、打包模板或发布任务是否成功推断安装版本。

## 数据来源

采集入口是 `scripts/package-status.py`，只依赖 Python 标准库与 Git：

| 渠道 | 查询来源与范围 |
| --- | --- |
| AUR | 克隆 AUR Git 远端后读取 `.SRCINFO` 的 `pkgver` 和 `pkgrel`；不执行远端 `PKGBUILD`，避免网页验证与 RPC 缓存 |
| Homebrew tap GUI / CLI | `UniClipboard/homebrew-tap` 默认分支的 Cask / Formula |
| Homebrew 官方 GUI | `Homebrew/homebrew-cask` 默认分支中的 Cask |
| Scoop | `ScoopInstaller/Extras` 默认分支的 manifest |
| winget | `microsoft/winget-pkgs` 中已合并的版本目录，按数字版本比较 |
| Chocolatey | 官方 OData 源中已批准、未明确撤下的版本 |
| npm | `@uniclipboard/cli` 的 `latest` 与 `alpha` dist-tag |
| Snap | 官方商店 `latest/stable` 与 `latest/edge` 的 amd64 / arm64；两个架构不同步时分别列出 |
| COPR | `mkdir700/uniclipboard` 与 `uniclipboard-alpha` 的 Fedora 44 x86_64 RPM 仓库元数据；不以成功构建记录代替可安装状态 |
| Nix 项目 flake | 默认分支 `packaging/nix/sources.json` 中固定的发行版版本；该入口仍是实验性打包定义，不代表已进入 nixpkgs |
| nixpkgs | `nixos-unstable` 分支 `pkgs/by-name/un/uniclipboard/package.nix` |
| Flathub | `app.uniclipboard.desktop` 的公开 AppStream 元数据 |

“未发布”只用于渠道明确返回不存在或没有已发布版本的情况。网络错误、限流与格式解析错误显示“查询失败”，不会沿用上次成功版本。“—”只表示表中不展示该渠道。AUR 的徽章是发布的元数据快照；用户安装开发版时，`makepkg` 仍从源码重新计算版本。

## 自动刷新

`.github/workflows/package-status.yml` 每 6 小时执行一次，并在本仓库相关渠道发布工作流成功结束、相关文件合入主分支及手动触发时执行。外部仓库更新和 Homebrew tap 的异步更新最迟由下一次定时查询补齐。

工作流只检出可信 `main` 代码，PR 只运行离线测试。发布快照的作业拥有 `contents: write`，仅更新独立的 `package-status` 分支，不向 `main` 提交定时生成文件，不使用额外凭据。首次执行会创建该分支。私有分支或 fork 上的手动执行不能写入正式状态。

快照包含：

- `status.json`：UTC 查询时间、每个渠道的版本或明确失败状态。
- `badges/*.svg`：完整状态快照配套的徽章，文字经过 XML 转义；README 的主要渠道表格使用 Shields.io 原生徽章。
- `badges/checked-at.svg`：可见的查询时间，防止长期未刷新时仍被误认为实时数据。

Shields.io 原生徽章由该服务独立查询与缓存，不受本仓库每 6 小时的快照刷新时间控制。GitHub 图片缓存也可能延迟更新；README 的“完整渠道状态”链接可查看本仓库采集的原始快照。查询异常详情保留在 `status.json`，采集结果也上传为工作流工件。

## 本地验证

```bash
python3 -m unittest discover -s scripts/tests -p 'test_package_status.py' -v
python3 -m unittest discover -s scripts/tests -p 'test_aur_package.py' -v
python3 scripts/package-status.py --output /tmp/uniclipboard-package-status
```

若渠道调整包名、路径、发布轨道或 COPR 发行版范围，须同步修改采集器、响应契约测试及 README 展示范围。
