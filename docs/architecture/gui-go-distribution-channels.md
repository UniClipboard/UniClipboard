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

## PR 门禁

`pr-check.yml` 的 `gui-go` 作业（名称 `Go Host Compile Check`）在 `ghcr.io/uniclipboard/build-bookworm` 容器里运行 `scripts/ci/check-gui-go.sh all`：

- `packages/desktop-host-go`：重新生成 `buildinfo` 并要求与已提交内容一致，再 `go vet ./...`。
- Linux：`go vet -tags gtk3,production,release ./...`、`go vet -tags gtk3,e2e .`，并链接 `gtk3,production,release`。
- Windows：amd64 与 arm64 各做 `go vet -tags production,release ./...` 与链接（`CGO_ENABLED=0`，`-H windowsgui`）。`build_windows.py --cross-check-only` 只覆盖 amd64 与非 release 标签，是它的子集，因此没有再调用它。
- 作业不缓存 Go 模块（PR 只读默认分支缓存），上传链接产物与 `SHA256SUMS` 作为证据，不发布任何东西。
- macOS 的 `go vet`/链接已经在 macOS 打包作业里；Linux、Windows 的真实运行不在 PR 门禁内。

PR 门禁没有在 GitHub 上运行过（未 push）。设为必需检查、在草稿 PR 里演示失败，需要另行授权。

## 渠道决定

| 渠道 | 决定 | 改动 | 实测 | 未验证 |
| --- | --- | --- | --- | --- |
| snap | 移植：源码构建 Go 宿主 + Rust daemon；`grade: devel`；默认不推送 | `snapcraft.yaml`、`snap.yml`（PR 触发，仅构建；`release` 输入默认 false） | Ubuntu 22.04 arm64 容器里按配方的 Go/前端步骤编译成功（WebKitGTK 2.50.4、GTK 3.24.33） | 没有运行 `snapcraft`（容器里没有 snapd/LXD）；strict 限制下的启动、托盘、通知、自启动、全局快捷键完全未验证，不声称支持；Rust daemon 步骤未在 core22 里执行 |
| AUR | 移植：源码构建；发布只能由 main 上的 `workflow_dispatch(publish=true)` | `PKGBUILD`、`aur.yml`、`build-aur-package.sh`、`check-arch-package.sh` | 干净 `archlinux:base-devel`（amd64，模拟）里 `makepkg` 成功并在全新容器里安装、启动通过，见下 | 原生 amd64/aarch64 构建；真实桌面；合入后需要有权限的人 dispatch 一次才会更新 AUR；`namcap` 警告（无 PIE/RELRO、`uniclipd` 含 `$srcdir` 引用）未处理 |
| COPR | 保持重打包 Go rpm | spec、`copr.yml`（`run_ids` 取 `linux-gui-*`，`dry_run`）、`build-copr-srpm.sh` | Fedora 44 arm64 容器里 SRPM → 重建 → 与上游 rpm 对比 → 安装启动，见下 | 真实 COPR 提交；amd64；EPEL；Fedora 之外的发行版 |
| Flathub | 保持重打包 deb | 注释与 README | 仅静态核对：deb 布局与 manifest 的 `install` 命令一致 | `flatpak-builder` 未运行；GNOME 46 运行时已 EOL，新运行时是否带 webkit2gtk-4.1 未查实；沙箱内托盘/剪贴板/快捷键；`libgtk-layer-shell` 不在运行时里，快捷面板回退为普通窗口 |
| nix | 保持包装 AppImage | 注释与 README | 在 1.1.1 的 Go AppImage 里核对了根目录 `uniclipboard.desktop`（`Exec=uniclipboard %U`）与 `usr/share/icons`，与 `package.nix` 一致 | `nix-build` 未运行；FHS 环境里能否启动未知 |
| Homebrew cask | 保持 | 无 | 静态：dmg 名、`UniClipboard.app`、zap 路径（`Application Support`/`Caches`/`Logs` 下 `app.uniclipboard.desktop`）与 Go 宿主一致；`:monterey` 无法表达最低 12.5 | 没有 `brew install`（不在个人 macOS 上运行发布形态） |
| Chocolatey | 保持包装 NSIS 安装器 | 一行注释 | 静态：`/S` 静默、用户级（`RequestExecutionLevel user`，HKCU 卸载项）、卸载项名 `UniClipboard` 与 `softwareName` 匹配 | 没有 `choco install`（Windows 只在一次性托管 runner 上运行发布形态）；Authenticode 未做 |
| winget | 保持 | README | 静态：`nullsoft`、`Scope: user` 与 NSIS 一致，x64/arm64 安装器名匹配收集器 | 没有 `winget install`；#1140 的首次提交不变 |
| scoop | 保持 portable zip | 无 | 静态：zip 含 `UniClipboard.exe`、`uniclipd.exe`、`portable.dat`，数据目录 `data` 与 `persist` 一致 | 没有 `scoop install` |

## 验证结果

环境说明：除 Fedora/Ubuntu 22.04/CI 同款 Debian 12 的 arm64 原生容器外，Arch 为 amd64 模拟（`ghcr.io/archlinux/archlinux:base-devel`，digest `sha256:827747df…`，pacman 沙箱因模拟被关闭）。均为容器证据，不是真实桌面。

### PR 门禁（arm64 原生容器，`build-bookworm`，Go 1.27.1）

- 源码 `81c06b06f`，干净树，绿：Linux `uniclipboard` `2fbf0030…cb9`（三次运行相同）、`windows-amd64/gui-go.exe` `b9035ad4…`、`windows-arm64/gui-go.exe` `b7fc035f…`。
- 红：在 `autostart_linux.go`、`host_install_windows.go`、`e2e_enabled.go` 各追加一个类型错误，门禁分别在 Linux vet、Windows vet、e2e 标签 vet 失败。macOS 宿主对 Linux 错误的 `go vet` 没有任何输出。

### AUR

- 源码提交 `c1aca2f70652a006c18dae6143dba80f8b22e8eb`（本分支，`makepkg` 针对该提交的本地克隆），包 `uniclipboard-git-1.1.1.r89.gc1aca2f-1-x86_64.pkg.tar.zst`，sha256 `2029a526cc1f7d7b6676bf9668f9014e20e609cf9893de8b10eb7a23a75fef5f`。工具链：go 1.27.2、rust 1.99.0、bun 1.4.2、webkit2gtk-4.1 2.54.1、gtk3 3.24.52。
- 全新容器安装（只装包声明的依赖）：包文件校验、GUI 与 daemon 无缺失库、desktop 文件合法、GUI 与 daemon 从 `/usr/bin` 运行、WebKitWebProcess 来自已装的 WebKitGTK、15 秒后仍存活、daemon 发布了 `daemon.conn`，全部 PASS。amd64 模拟下 `/proc/PID/exe` 是模拟器，检查改用其命令行的第 2 个参数。

### COPR

- 上游 rpm 取自 run 37894007183（源码 `4658ffeec`，#1924 head，非本分支 HEAD）的 `linux-gui-arm64`；`UniClipboard-1.1.1-1.aarch64.rpm` sha256 `6203239c…`，其中 `uniclipd` sha256 `638bfe07…`，`uniclipboard` `c2cce146…`。
- 重打包 rpm（`uniclipboard-1.1.1-1.fc44.aarch64.rpm` sha256 `6d9a5409…`）：`Requires` 相同；`Provides`/`Obsoletes`/`Conflicts` 只多了 `.fc44` 发行版后缀；两个可执行文件与上游逐字节相同；文件列表相同；License 为 `AGPL-3.0-only`（上游为 `Proprietary`）。
- 该 rpm 通过既有的 `package_install_check.sh rpm fedora:latest fresh`：安装、启动、重装、卸载共 30 项全部 PASS。

### snap

- Ubuntu 22.04 arm64 容器按配方编译 Go 宿主成功，二进制 sha256 `4ed42194…`。第一次尝试暴露配方缺 C 编译器，已加入 `build-essential`。

## 未证明的边界

- 所有渠道的真实发布或安装；AUR、COPR、Snap Store、nixpkgs、Flathub、cask、Chocolatey、winget、scoop 的账号与权限。
- snap strict 限制下的任何行为；Flathub 沙箱；nix FHS 环境。
- 原生 amd64 的 Arch/Fedora/Ubuntu 22.04 构建；真实 GNOME/KDE/Wayland 桌面。
- PR 门禁在 GitHub 上的运行，以及它被设为必需检查之后对不触及这些路径的 PR 的影响（workflow 级 `paths` 过滤会让必需检查一直 pending；本作业放在已有的 `changes` 过滤器之后，但整个 `pr-check.yml` 仍有 `paths`）。
- `aur.yml`、`snap.yml`、`copr.yml` 的新工作流只做了 YAML 解析，没有在 Actions 上运行。

## 发现但未处理

- `package_linux.py` 的 rpm：`Version` 含 `-`（预发版本）时 rpmbuild 报 `Illegal char '-'`，alpha 版本打不出 rpm；`License: Proprietary` 与 AGPL-3.0-only 不符。
- `release.yml` 的 alpha 路径会以 `release=true` 触发 `snap.yml`；解除失败关闭时要重新决定。

## 解除 `release.yml` 失败关闭的条件（不在本次做）

1. 四个平台包（macOS、Windows、Linux、更新签名）都有对应已合并 PR，且 `collect-release-assets.py` 的收集集合与上面各渠道使用的名字一致。
2. 签名与生产更新通道（#1896、Windows Authenticode 决定）就绪。
3. 一次非生产渠道的 `workflow_dispatch` 运行，以及对结果的评审。
4. 需要明确授权；本任务不解除。
