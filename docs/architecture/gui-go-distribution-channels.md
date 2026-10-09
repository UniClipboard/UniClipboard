# Go 宿主的分发渠道与 PR 构建门禁（issue #1899）

本文记录 `apps/gui-go` 成为唯一桌面宿主之后，各分发渠道的去留决定、PR 构建门禁的设计、失败模型、实测证据与未证明的边界。它是 [gui-go-tauri-retirement.md](gui-go-tauri-retirement.md) 里「暂缓移植的打包渠道」一节的后续；打包本身见 [gui-go-linux-ci-packaging.md](gui-go-linux-ci-packaging.md)、[gui-go-windows-packaging.md](gui-go-windows-packaging.md) 与 `apps/gui-go/README.md`「macOS 发布构建」。

## 范围与不做的事

- 做：PR 阶段对 `apps/gui-go` 的编译门禁；snap、AUR、COPR、Flathub、nix、Homebrew cask、Chocolatey、winget、scoop 逐个给出决定，把仍然从源码构建旧宿主的渠道（snap、AUR）移植到 Go 宿主，把重打包已发布产物的渠道（COPR、Flathub、nix、cask、Chocolatey、winget、scoop）核对到当前产物的名字与布局。
- 不做：任何向外部渠道的写入（AUR、COPR、Snap Store、nixpkgs、Flathub、homebrew-cask、Chocolatey、winget、Scoop）；`release.yml` 首个步骤的失败关闭不动；生产更新通道与 FlareRelease 的登记；分支保护。
- 不写单元测试：验证用真实容器里的真实构建与安装，工件带 SHA-256。

## issue 描述与主线的出入（2026-10-09 核对 `origin/main` `19fc8bc0e`）

| issue 的说法 | 主线事实 |
| --- | --- |
| Linux 包「还没有」CI | 已有：`package-linux-gui.yml` 在原生 amd64/arm64 上构建 deb、rpm、AppImage 与更新归档（#1898，#1922 之后包名统一为 `uniclipboard`）。 |
| Windows 安装包没有 CI | 已有：`build.yml` 的 `package-windows-gui`（#1897，PR #1920）；未做 Authenticode。 |
| 产物名「靠约定」 | `scripts/collect-release-assets.py` 是唯一的命名来源：`UniClipboard_<v>_<amd64\|arm64>.deb`、`UniClipboard-<v>.<x86_64\|aarch64>.rpm`（实际文件名带 `-1`）、`UniClipboard_<v>_<amd64\|aarch64>.AppImage[.tar.gz]`、`_<x64\|arm64>-setup.exe`、`_<x64\|arm64>-portable.zip`、`_<aarch64\|x64>.dmg`。 |
| AUR `aur.yml` 在每次 push 到 main 时发布 | 属实，且 PKGBUILD 仍构建旧宿主：合入后 `-git` 包无法构建。 |
| PR 门禁没有 Go 编译 | 属实：`pr-check.yml` 只有前端类型检查与构建。`packages/**`（`desktop-host-go`）也不在 `pr-check.yml` 的 `paths` 里。 |
| `release.yml` 失败关闭 | 属实，本任务不动。 |

## 失败模型（先于实现）

| # | 失败方式 | 如何被发现或防住 |
| --- | --- | --- |
| G1 | Linux 专有（cgo、GTK）或 Windows 专有文件里的编译错误，在 macOS 开发机和前端作业里都看不到 | PR 作业在 Linux 镜像里 `go vet` 并链接 `gtk3,production,release`，并对 windows/amd64、windows/arm64 做 `vet` 与链接；故意注入错误必须使作业失败（见「验证结果」） |
| G2 | 门禁绿了，但只检查了子集（缺 `packages/desktop-host-go`、缺 `e2e` 标签、`./...` 被缩成 `.`） | 脚本显式覆盖这三处；`desktop-host-go` 的 `buildinfo` 与 Rust 工作区的版本漂移也被拦截 |
| G3 | 路径过滤让必需检查永远 pending：workflow 级 `paths` 不命中的 PR 不会出现该检查 | 作业放进已有的 `pr-check.yml`，复用它的 `changes` 过滤器；`packages/**` 补进 `paths`。把该检查设为必需（分支保护）属于需要授权的动作，见报告 |
| G4 | 自动推送把坏的 PKGBUILD 发布到 AUR | 推送改为只能由 `main` 上显式的 `workflow_dispatch(publish=true)` 触发，且排在同一提交的 `makepkg` 构建与干净安装之后；PR 与 push 只构建 |
| G5 | PKGBUILD 在用户机器上才发现缺依赖或需要联网下载工具链 | 干净 `archlinux:base-devel` 里 `makepkg --syncdeps` 真实构建；`prepare()` 预取全部 Cargo/Go/bun 依赖；`go` 版本满足 `go.mod` 的 `toolchain` 行，不下载工具链 |
| G6 | 把重打包渠道的包头信息丢掉：COPR 只带走文件，`Requires`、`Obsoletes`、`Provides` 要重写 | spec 重新声明并与 `package_linux.py` 的 `build_rpm` 对齐；E2E 比较上游 rpm 与重打包 rpm 的 `rpm -q --requires/--provides/--obsoletes/--conflicts` |
| G7 | 重打包后的 daemon 不再是 Releases 页上的那个文件（`rpmbuild` 安装后 strip、重写 ELF） | spec 关闭 `__os_install_post`；E2E 比较两个 rpm 里 `/usr/bin/uniclipd` 的 SHA-256 |
| G8 | 包管理器安装的程序自己替换自己的文件 | 现状已防住：只有 `$APPIMAGE` 非空才自更新（`apps/gui-go/host_install_linux.go`）；AUR、snap、Flatpak、nix 的可执行文件路径都不是 `$APPIMAGE`，返回 `unknown` 并拒绝原地安装 |
| G9 | snap 在没有验证 strict 限制的情况下被推到商店 | `grade: devel`（商店拒绝 candidate/stable）；`snap.yml` 的 `release` 输入默认 false，`release.published` 与 `pull_request` 事件只构建；推送必须由人显式 `workflow_dispatch` |
| G10 | 把容器、交叉编译、模拟器上的结果说成真实渠道安装或真实桌面 | 每个结论标明环境；渠道安装类项目没有渠道账号或真机就记 not-run，并写原因 |
| G11 | 证据的源码 SHA 与代码 HEAD 混淆 | 每个工件都记录自己的源码提交；使用他人 CI 工件时标明 run 与源码提交，不与本分支 HEAD 混为一谈 |
| G12 | `release.yml` 被过早解除 | 不动；解除条件见文末 |

（渠道专属的详细失败模型与结果在对应章节。）
