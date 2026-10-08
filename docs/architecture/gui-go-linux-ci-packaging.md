# Go GUI 的 Linux 包在 CI 中构建与验收（issue #1898）

本文是 Linux deb、rpm、AppImage 与 `.AppImage.tar.gz` 在 CI 中构建、验收的契约：失败模型、状态不变量、验收合同、构建镜像与 glibc 下限的决定依据、明确未证明的边界。实现之前先写；结果在文末「验证结果」补录。打包本身的设计见 [gui-go-linux-appimage.md](gui-go-linux-appimage.md) 与 [linux-appimage-library-policy.md](linux-appimage-library-policy.md)，本文不重复。

## 范围与不做的事

- 做：原生 amd64 与 arm64 runner 上构建真实 release `uniclipd` 与 GUI，运行 `apps/gui-go/e2e/package_linux.py`，只上传四种命名工件，保留构建证据，在真实 Debian/Ubuntu 与 Fedora 环境验证 deb/rpm，在下限发行版与较新发行版上运行 AppImage。
- 不做：更新签名（`.sig`）与更新通道（归 [gui-go-updater-signatures.md](gui-go-updater-signatures.md) 与渠道 issue）；tag、release、feed、Pages、FlareRelease 的任何写入；生产 Sentry 上传；`release.yml` 的 Linux 接入（它对未验收平台继续失败关闭）。

## issue 描述与主线的出入（2026-10-08 核对 `origin/main` `101ffb38e`）

| issue 的说法 | 主线事实 |
| --- | --- |
| `package_linux.py … [--frontend-dist <dir>]` | 没有该参数；脚本只检查 `apps/gui-go/frontend/dist` 是否存在。证据参数是 `--daemon-evidence`。不新增 `--frontend-dist`：它没有消费者。 |
| AppImage 命名 `UniClipboard_<v>_<amd64\|aarch64>.AppImage` | 代码对 AppImage 与 deb 共用 `deb_name`，arm64 产出 `_arm64.AppImage`。发布收集器 `scripts/collect-release-assets.py` 只接受 `_amd64\|aarch64.AppImage`，已发布的 Tauri v1.0.1/v1.1.1 资产同为 `_aarch64.AppImage`。因此 arm64 AppImage 会被收集器 **静默丢弃**。这是缺陷，不是约定。 |
| 没有 workflow 构建 Linux Go 包 | 属实，但 `build.yml` 的 `build-sidecar` 仍在 bookworm 容器里为两个 Linux 目标构建并上传 `uniclipd` sidecar，且没有任何作业消费 `sidecar-*-linux-gnu`（macOS 与 Windows 的 sidecar 有消费者）。它是没有消费者的并行旧逻辑，且在 `build_mode != test` 时会向生产 Sentry 上传调试符号。收敛计划见「build.yml 集成」。 |
| Engine 使用已合并的不可变修复 | 主线 pin 是 `e86f94ce`（`1.1.0-rc.22`）。`0e25f418`（Engine #163）是它之后的一个提交，只补 Windows 的 `windows-sys` 声明与 Windows 构建门禁，不改 Linux 目标的依赖解析。PR #1920 把 pin 升到 `0e25f418`；本任务不抢先修改 `Cargo.toml`/`Cargo.lock`，Linux 构建沿用主线 pin，并在证据里记录实际解析到的 Engine 修订。 |
| 构建镜像是 `ubuntu:24.04` | `apps/gui-go/e2e/linux/Dockerfile` 是 Ubuntu 24.04；已发布 Tauri 包由 `ghcr.io/uniclipboard/build-bookworm`（Debian 12，glibc 2.36）构建。 |

## glibc 下限的实测

方法：`apps/gui-go/e2e/linux/elf_floor.py` 对解开的包（不执行）逐个 ELF 读取 `readelf -V` 的 Version needs，取 `GLIBC_*`、`GLIBCXX_*`、`CXXABI_*` 的最大值。原始输出在证据根的 `floor/`。

| 包 | 构建环境 | GLIBC | GLIBCXX | 决定下限的文件 |
| --- | --- | --- | --- | --- |
| Tauri v1.0.1 AppImage，arm64 与 amd64 | Debian 12 | 2.36 | 3.4.30 | `usr/lib/libcups.so.2` |
| Tauri v1.0.1 deb，两个架构 | Debian 12 | 2.34 | - | `usr/bin/uniclipboard`、`usr/bin/uniclipd` |
| Go AppImage，arm64（`E2E-` 前缀，来自既有 17c14 产物） | Ubuntu 24.04 | **2.38** | 3.4.30 | 175 个 ELF 中多个自带库（`libXcursor`、`libblkid`、`libatk-bridge` 等） |
| Go AppImage，arm64（占位 daemon） | Debian 12 | 2.36 | 3.4.30 | `usr/lib/libcups.so.2`，与 Tauri 完全相同 |

结论：Ubuntu 24.04 构建把 AppImage 的 glibc 下限从 2.36 抬到 2.38，没有记录过决定；Debian 12 构建精确还原旧下限。

## 决定：构建镜像与下限

默认构建镜像为 `debian:bookworm`（glibc 2.36），保持既有兼容范围。依据是上表的实测，而不是镜像名推断。在 Debian 12 上直接运行既有打包器，第一处失败是 `libpxbackend-1.0.so is missing in the build image: install libproxy1v5`：打包器把 libproxy 0.5 的依赖闭包（`libduktape`、`libcurl-gnutls` 等）和 `t64` 包名冻结成了常量。Debian 12 的 libproxy 0.4 是单个自包含库。打包器因此按构建镜像实际提供的 libproxy 选择支持库集合（由库文件是否存在决定，不是开关）。

产品层面的差异（由此带来的权衡，等待产品确认）：

| | Debian 12 构建（默认） | Ubuntu 24.04 构建 |
| --- | --- | --- |
| AppImage glibc 下限 | 2.36（与 Tauri 相同） | 2.38 |
| 仍支持的最老发行版 | Debian 12、Fedora 37+、Ubuntu 23.04+（AppImage）；Ubuntu 22.04 不在内（旧包同样不在内） | Debian 13、Ubuntu 24.04、Fedora 39+ |
| 随包 WebKitGTK / GLib / libproxy | 2.50.x / 2.74 / 0.4 | 2.52.x / 2.80 / 0.5 |
| libproxy 路径下的 PAC | 无（0.4 的 PAC 是未安装的可选插件）；GNOME 解析器路径的 PAC 仍由随包的 `glib-pacrunner` 提供 | 有（duktape） |
| 既有 17c7–17c14 证据 | 是在 Ubuntu 24.04 构建的包上取得，需要在 Debian 12 构建上重做关键项 | 直接适用 |

两个镜像都由同一个参数化的 Dockerfile 构建，CI 以参数选择；本文的验收在默认镜像上做，Ubuntu 24.04 作为「更新的发行版」宿主运行同一个 Debian 12 构建的 AppImage。

## 失败模型（先于实现）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| C1 | arm64 AppImage 命名为 `_arm64.AppImage`，发布收集器丢弃它，更新清单生成器拿不到它 | `verify_package_set.py` 用 `collect-release-assets.py` 的同一个收集函数处理输出目录，要求每个架构正好得到四个文件，名字逐字等于期望集合；对未修复的打包器必须先红 |
| C2 | 上传集合混入 raw `uniclipboard`、`package-manifest.json`、`E2E-`/`FIXTURE-`/`NEGCONTROL-` 前缀包或证据 | 上传目录只由收集函数产生（单一事实来源）；验证器断言恰好四个文件，且清单 `sha256` 中的这四项与文件一致 |
| C3 | daemon 与证据不对应：SHA 不同、证据的源码头与检出不同、树不干净、证据的目标架构与 `--arch` 不同、`Cargo.lock` 哈希与检出不同、Engine 修订与 `Cargo.toml` 的 pin 不同 | `read_daemon_evidence` 增加架构、`Cargo.lock` 哈希、Engine 修订三项交叉检查；验证器重做并断言清单 `source.dirty == false` |
| C4 | 工件之间的源码漂移：daemon 作业与打包作业检出的提交不同（`inputs.branch` 在中途前进） | 第一个作业解析 SHA 作为输出，后续作业一律检出该 SHA；清单的 `source.head` 必须等于它 |
| C5 | glibc 下限回归：包内任一 ELF 需要比声明下限更新的 `GLIBC_*` | 验证器对解开的 deb 与 AppImage 运行 `elf_floor.py --max-glibc 2.36`，失败即作业失败 |
| C6 | 构建镜像悄悄漂移（基础镜像、包版本） | 证据记录镜像引用、`/etc/os-release`、ldd 版本、关键 `-dev` 包版本；清单记录随包库的 `dpkg` 来源（已有） |
| C7 | 把模拟或交叉编译当作原生运行 | 作业开头断言 `uname -m` 与 `--arch` 对应并写入证据；E2E 报告记录宿主架构；QEMU 路径不被任何作业使用 |
| C8 | 生产副作用：Sentry 调试符号与 source map 上传、遥测上报 | 新作业没有任何 Sentry 上传步骤，前端构建环境不设置 `SENTRY_AUTH_TOKEN`/`VITE_SENTRY_PROJECT`；daemon 的编译期注入沿用 `SENTRY_DSN`/`POSTHOG_PROJECT_KEY`/`APP_ENV` 合同，证据只记录是否注入；验收环境不设置任何上报目标 |
| C9 | release 与 test 构建证据混淆：`build_daemon_release.sh` 写死 `build_mode=release` | 脚本读取 `CARGO_PROFILE_RELEASE_*` 覆盖并如实写入；`read_daemon_evidence` 只接受没有覆盖的 release |
| C10 | CI 检出被构建步骤弄脏（`bun install`、`go generate ./buildinfo`、前端构建） | 打包后断言 `git status --porcelain` 为空；清单 `source.dirty` 为 false |
| C11 | deb/rpm 在下限发行版上依赖无法满足、升级遗留旧文件、移除遗留文件 | `package_install_check.sh` 在 Debian 12、Ubuntu 24.04、Fedora 容器里真实 `apt`/`dnf` 安装、从已发布的 Tauri v1.0.1 包升级、启动、移除，并逐步断言文件系统状态 |
| C12 | AppImage 只在构建发行版上能跑 | 同一个 AppImage 在下限发行版（Debian 12）与较新发行版（Ubuntu 24.04）的干净宿主（没有 GTK/WebKitGTK）里跑完整 E2E（既有 `linux_appimage_run.py`） |
| C13 | 误把容器证据说成真实桌面证据 | 本文「验证结果」把容器、托管 runner 主机、真实桌面分列，真实桌面项没有主机就记 not-run |

## 验收合同

1. 每个架构：原生 runner 上由一个干净检出（固定 SHA）构建 release `uniclipd`，保留 `build-evidence.txt`，用它运行 `package_linux.py`，得到 deb、rpm、AppImage、`.AppImage.tar.gz` 与 `package-manifest.json`。
2. 上传的发布候选工件正好四个命名文件；清单、证据、日志、E2E 包进入另一个 artifact。
3. 验证器（`verify_package_set.py`）在每个架构的作业中通过：命名、清单不变量、证据交叉检查、ELF 架构、glibc 下限、deb/rpm 元数据。
4. AppImage 在下限发行版与较新发行版上通过 E2E：启动、daemon、WebView、HTTPS、受信任 fixture 密钥的原位更新、未受信任签名被拒绝、自启动注册。
5. deb/rpm 的安装、启动、升级、移除在 Debian/Ubuntu 与 Fedora 上通过（容器与托管 runner 主机分别记录）。
6. 余下的限制（真实 GNOME/KDE/Wayland 桌面、登录自启、`--appimage-extract-and-run`、真实只读挂载）逐项列出 pass 或 not-run。

## build.yml 集成与 PR #1920 的协作边界

PR #1920（Windows 打包）修改 `build.yml` 的：`workflow_dispatch`/`workflow_call` 输入（新增 `signing_self_test`、`upload_symbols`）、`setup-matrix` 的 `outputs` 与新增的 `Select Windows GUI targets` 步骤、`build-sidecar` 中 `stage-daemon` 之后的 `record sidecar provenance` 步骤与 Sentry 上传条件、`package-macos-gui` 的两个 Sentry 环境变量，并新增 `scripts/ci/write-sidecar-provenance.mjs`。该文件未合并，本任务不复制它，也不把它当依赖。

本任务的 Linux 作业不经过 `build-sidecar`，因此不依赖 `sidecar-provenance.json`；Linux 的构建证据由 `build_daemon_release.sh` 写出的 `build-evidence.txt`（字段是 `write-sidecar-provenance.mjs` 的超集，对应关系：`sourceHead`↔`head`、`sourceDirty`↔`daemon_source_dirty`、`cargoLockSha256`↔`cargo_lock_sha256`、`rustc`↔`rustc -Vv`、`files.*.sha256`↔`sha256`）。两边合并后可以收敛为同一个证据文件，由后合并的一方负责；本任务不预先改动共享脚本。

新逻辑放在新的 reusable workflow `.github/workflows/package-linux-gui.yml`，`build.yml` 只加一个调用它的作业和一个 `linux-gui` 平台选项。冲突面：`on.workflow_dispatch.inputs.platform.options`（第 11-19 行附近，#1920 不改）、`setup-matrix` 的平台分支（#1920 不改该分支，只在其后新增步骤）、`build-sidecar` 的 `if`（#1920 不改头部）。需要手工合并的位置只有 `jobs:` 末尾新增作业的相邻行，以及两边各自新增的 `outputs` 键。

收敛计划：Linux 的 `build-sidecar` 条目（`ubuntu-22.04`、`ubuntu-22.04-arm`，bookworm 容器）没有消费者，且会触发生产 Sentry 符号上传。Linux 包经 release 流水线发布之前，把这两个条目从 `all` 矩阵与平台选项中移除；本任务只让 `linux-gui` 平台不填充 `build-sidecar` 矩阵，不改变现有平台的行为。

## 明确不证明的边界

- 真实 GNOME、KDE、Wayland 桌面、真实登录自启动、`--appimage-extract-and-run`、真实只读挂载：除非某个主机真正运行过，否则记 not-run。
- 官方签名发布验证、真实更新服务：fixture 密钥只证明机制。
- Arch、openSUSE、Debian sid、Alpine/musl：未验证。

## 验证结果

（实现与运行后补录。）
