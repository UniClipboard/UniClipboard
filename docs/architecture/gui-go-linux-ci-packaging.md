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

以下均为容器或 GitHub 托管 runner 证据，不是真实桌面证据。工件位于 `~/.herdr-projects/uni/t-0203-artifacts/`（不入库）。

### 原生 CI（run 37755610047，源 `ab8e5b5ae`）

- amd64（`ubuntu-24.04`）与 arm64（`ubuntu-24.04-arm`）的 `Package` 均成功：构建证据 `head` 等于固定 SHA、`daemon_source_dirty=false`、`build_mode=release`；验证器无问题；glibc 下限 AppImage 2.36、deb 2.34；只上传四个命名文件。
- AppImage 验收在 Debian 12（下限）与 Ubuntu 24.04（较新）上：full、negative、smoke 全部通过（启动、daemon、WebView、HTTPS、受信任 fixture 密钥的原位更新、不受信任签名被拒、自启动、数据目录）。
- 六个安装 job（deb：Debian 12、Ubuntu 24.04；rpm：Fedora latest；各 amd64、arm64）：安装、对已发布 Tauri 包的升级、启动、移除均通过。
- `telemetry-injection-contract` 通过；验收在上报关闭下运行，没有生产 Sentry 写入与 source map 上传。
- 当时软件包名为 `uni-clipboard`，与已发布 Tauri 包一致；后续改名关系见下文。

### 验收脚本修正（C 类：验收脚本，非产品缺陷）

- `2 the mount is gone after exit`：FUSE 卸载在进程退出后异步完成，原先只采样一次；改为最多轮询 20 秒，断言不变。
- `7 persisted user data files survived the update`：缺失文件只有 `*.sqlite-wal`、`*.sqlite-shm`；SQLite 干净关闭并 checkpoint 后会删除这些副本，不属于持久数据；仅按后缀排除，数据库文件本身仍参与比较。
- 证据导出：容器以 root 写入 0600 文件；导出改为 sudo 读取加白名单（日志、jsonl、断言 JSON、清单、哈希），不含 profile、身份、密钥、home 副本与原始二进制。harness 只跳过消失的文件，其他复制错误使验收失败。

### 代理与 PAC 回归（Debian 12 构建，Ubuntu 24.04 主机，arm64 容器，完整 27 场景）

| 产物 | 结果 |
| --- | --- |
| 无 PAC 插件的旧产物 | 14/27 失败（libproxy 0.4 无 PAC 运行时，PAC 走直连），作为阴性对照 |
| 带 `libproxy1-plugin-webkit` | 24/27；3 个 portable 场景失败：自动拉起的会话总线继承了 `LD_LIBRARY_PATH`，宿主 `glib-pacrunner` 加载捆绑的旧 GLib，`undefined symbol: g_once_init_enter_pointer`（状态 127） |
| 插件加 AppRun 提前启动自动拉起的总线（`52d3cb08e`） | 27/27 通过 |

- 体积：插件使压缩后增加约 9.0 MB（7.6%），解包后增加约 31.6 MB。
- 版本：`libproxy1-plugin-webkit` 与 `libproxy1v5` 0.4.18-1.2，`libjavascriptcoregtk-4.0-18` 2.50.6-1~deb12u2。
- 许可证：随 Debian 版权文件分发（libproxy：LGPL-2.1+，含 Netscape PAC 工具文件；JavaScriptCore：LGPL 与 MPL、Expat 混合）；未做法务复核。
- 失败语义：构建镜像缺少插件时打包直接失败并给出安装指令；模块路径错误时 libproxy 静默直连（即阴性对照现象）。
- 日志隐私：产品 GUI 与 daemon 日志不含 PAC URL 与凭据；仅测试 harness 的断言 JSON 含回环夹具的 PAC URL 与一次性代理凭据；`_PX_DEBUG` 从未设置；这些文件不上传。

### 未运行（not-run）

- Fedora 代理矩阵；Arch、openSUSE、sid、Alpine。
- 真实主机（`ssh fedora`、`ssh omarchy` 当时不可达）；真实 GNOME、KDE、Wayland 桌面；真实登录自启动；`--appimage-extract-and-run`；真实只读挂载。

## 已安装包的更新提示补充验收

`apps/gui-go/e2e/linux/package_update_check.sh` 在一次性 Ubuntu / Fedora 容器安装
带 `gtk3,production,release,e2e` 控制面的真实 deb / rpm，通过原生 WebView 完成初始化、
关闭遥测提示、进入设置并点击检查更新。它要求宿主查询返回 `deb` / `rpm`，前端提示的
apt / dnf 命令使用真实包名 `uniclipboard`，并记录截图、包数据库归属、工件及已安装
可执行文件的哈希、GUI 和 daemon 日志。通知通过 GUI 发起并由私有 D-Bus 接收器记录；
这不证明真实桌面通知的展示。运行依赖现有真实应用镜像与 Secret Service 镜像。

更新 feed 只监听容器 loopback，公钥使用 E2E 更新工具产生的公开夹具；不下载或安装系统包更新。
普通发布包没有测试控制面，不能用于这项自动化。可先用现有 `package_linux.py --gui-binary`
将 E2E 宿主和来源可验证的 release daemon 打成测试包。

```bash
apps/gui-go/e2e/linux/package_update_check.sh \
  deb /absolute/path/E2E-UniClipboard_version_arm64.deb \
  /absolute/path/fixture-pubkey.b64 /absolute/path/new-evidence
```

当前补充脚本固定使用原生 arm64 Docker 平台，Ubuntu / Fedora 容器不等于物理 rpm 宿主。
AppImage 完整验收则在原位更新前保持自启动启用，替换后核对条目和执行 `Exec`；
手动执行该命令不等于真实登录管理器读取自启动条目。

## 手动 deb/rpm 包身份迁移

包名统一为 `uniclipboard`。已发布的 Tauri 与此前 Go 包名 `uni-clipboard` 是兼容来源，
不是第二套安装。deb 使用版本化 `Conflicts` + `Replaces` + `Provides`；rpm 使用
`Obsoletes: uni-clipboard <= %{version}-%{release}` 与版本化普通及 `%{?_isa}` 架构能力 `Provides`，并用
`Conflicts: uni-clipboard > %{version}-%{release}` 拒绝较高版本旧包共存。
RPM 会允许完全相同的文件被两个包共同拥有，不能只依靠文件冲突兜底；CI 用相同 payload 的较高版本旧包验证拒绝事务且包数据库、文件均不变。
等号覆盖同版本 Go 包改名，旧名保留为依赖能力；包数据库唯一安装身份和卸载命令使用新名。

采用 [Debian Policy 7.6.2](https://www.debian.org/doc/debian-policy/ch-relationships.html#replacing-whole-packages-forcing-their-removal)
和 [RPM Obsoletes](https://rpm.org/docs/latest/manual/dependencies.html#obsoletes) 的整包替换机制。
不添加手动删除文件的维护脚本、第二个过渡包或资料搬迁。现有 `dpkg-deb` / `rpmbuild` 足够。
更高版本旧名与异架构包不属于这个候选的升级范围，不能用忽略依赖或强制覆盖绕过。
已发布的最高版本是 Tauri v1.1.2（2026-10-09，从未合回主线的 `release/v1.1.2` 分支发布），而主线 `apps/gui-go/app.json` 仍是 1.1.1。替换关系只覆盖不高于新包版本的旧名包，因此 Go 的第一个发布版本必须高于 1.1.2，否则包管理器会拒绝升级（实测记录见 [gui-go-tauri-upgrade-continuity.md](gui-go-tauri-upgrade-continuity.md)）。

手动安装使用 `apt install ./新包.deb` 或 `dnf install ./新包.rpm` 执行一次改名事务。
`apt/dnf upgrade` 的包名提示不能代替发行仓库：没有新包仓库时它不会自动发现下载文件。
COPR 的独立维护和发布不属于此改名。应用 ID、可执行路径、数据/日志路径、Secret Service
属性和 XDG 自启动文件不改名。卸载保留用户资料及自启动偏好，清空资料必须另行明确操作。

`package_install_check.sh` 使用真实包管理器覆盖新装和旧包迁移，再安装和明确 reinstall，
确认旧身份消失、文件唯一归属、退役文件移除、GUI/WebKit/daemon 启动和卸载。
事务前初始化真实加密 profile，保存 enabled entry；停止进程后比较资料及 entry 摘要，
升级后执行 `/usr/bin/uniclipboard --autostart`，通过真实 daemon API 核验设备身份、初始化状态、
加密会话与设置，并读取隔离 Secret Service 中的夹具。输出命令日志、输入 SHA、摘要和断言。
Tauri 场景使用新 daemon 生成有效资料夹具，证明包事务保留资料，不证明全部历史 schema 转换。
这属于 Xvfb 容器验收，不是桌面登录管理器或真实登录验收。

CI 在原生 amd64/arm64 上增加 `legacy-go-fixture`：以候选的实际 Go/daemon payload 及旧包
元数据生成事务回归夹具，其清单明确标识为生成夹具，不能冒充已发布历史包。旧 Tauri 使用
已发布 v1.0.1 与仓库固定 SHA。历史 Go 的真实工件另行验收，来源与 CI 生成夹具分开记录。

## rpm 的依赖声明（issue #1903）

rpm 关闭了自动依赖生成（`AutoReqProv: no`），`Requires` 是依赖的全部声明。此前写的是 Fedora 的包名
`gtk3, webkit2gtk4.1, gtk-layer-shell`。在 openSUSE Tumbleweed（固定摘要 `cb0b66ea…`，arm64）上用真实
`zypper install` 安装 CI 产出的 rpm 失败：`nothing provides 'gtk-layer-shell'`；`webkit2gtk4.1` 在该发行版
也没有同名提供者（openSUSE 的包名是 `libwebkit2gtk-4_1-0`、`libgtk-3-0`、`libgtk-layer-shell0`）。
包名只在一个发行版族内成立，soname 在两个族里都成立，所以改为 soname 能力
（`libgtk-3.so.0()(64bit)`、`libwebkit2gtk-4.1.so.0()(64bit)`、`libgtk-layer-shell.so.0()(64bit)`，常量 `RPM_REQUIRES`）。

用同一份 CI 载荷经 `package_linux.build_rpm` 重建后：openSUSE `zypper install` 解析并安装 175 个包，
`ldd` 无缺失库，`rpm -V` 干净；Fedora 44 `dnf install` 解析到 `gtk3`、`webkit2gtk4.1`、`gtk-layer-shell`，同样干净。
这一步只是依赖解析与文件级安装的验证；已安装 rpm 的 GUI 启动、升级与卸载在随后的「openSUSE 上已安装 rpm 的生命周期」一节另行运行过（其中 `WebKitWebProcess` 的失败见该节）。同一 AppImage 在该发行版的启动与库存结果见
[gui-go-linux-appimage-runtime-deps.md](gui-go-linux-appimage-runtime-deps.md) 的「运行时库存结果」）。
`packaging/uniclipboard.spec`（COPR，Fedora 系）是另一份独立的 `Requires` 声明，仍用包名 `webkit2gtk4.1`、`gtk-layer-shell`，因 COPR 只面向 Fedora 系而保留；两处并存，不是同一事实来源。

### openSUSE 上已安装 rpm 的生命周期（issue #1903）

`package_install_check.sh` 增加了 zypper 分支（工具包名按 openSUSE 取）。在 openSUSE Tumbleweed（固定摘要，arm64 容器、Xvfb、一次性 gnome-keyring，不是桌面会话）上，
用与 CI 相同的载荷重建的 rpm 依次执行：旧身份 `uni-clipboard` 安装并播种加密 profile 与 enabled 自启条目 → `zypper install` 新身份（`Obsoletes` 迁移）→ 重复安装 → `--force` 重装 → 重启后校验 → 移除。
33 项断言中 32 项通过：文件唯一归属、`rpm -V`、无缺失库、profile 与自启条目字节不变、daemon 与 GUI 从安装路径运行、移除后文件消失而用户数据保留。
**唯一失败**：`WebView process runs from the installed WebKitGTK`——GUI 与 daemon 都在运行，窗口存在，`WebKitNetworkProcess` 在运行，但 `WebKitWebProcess` 在启动约 0.1 秒后被 `SIGILL` 杀死。
**根因已定位，不在 UniClipboard**：崩溃点在发行版的 `libjavascriptcoregtk-4.1.so.0`（Tumbleweed 的 WebKitGTK 2.52.6）内，该库带 BTI / PAC / GCS 标记，页面脚本进入 `JSC::evaluate` 后经间接调用落到一个函数入口
（`stp x29, x30, [sp, #-16]!`，不是 `bti` 着陆点），所在映射的 `VmFlags` 含 `bt`，内核按 BTI 规则发出 `SIGILL`（`si_code=ILL_ILLOPC`）。
不含任何 UniClipboard 代码的最小 WebKit2 客户端（只加载一段循环加正则的脚本）在同一容器里同样得到 `web-process-terminated`（崩溃），只含一行赋值的脚本则正常；
同一个脚本在 Fedora 44（WebKitGTK 2.54.1）上、同一台 Docker Desktop 虚拟机和内核上正常。因此这是 Tumbleweed 该版 JavaScriptCore 在带 BTI 的 aarch64 内核上的发行版缺陷，只在实际执行较热的脚本时出现，
不是包依赖问题，也不是 AppImage 的问题（AppImage 自带 WebKit，在同一宿主上通过）。
`JSC_useJIT=false` 等 JIT 开关、关沙箱、放宽 seccomp、`GDK_BACKEND=x11` 均不能规避；`GLIBC_TUNABLES=glibc.cpu.aarch64_bti=0` 也无效（它的默认值已是 0，BTI 由 ELF 标记和内核决定）。
没有在不带 BTI 的 aarch64 或 x86_64 的 Tumbleweed 上对照，所以不断言它影响所有 Tumbleweed 用户，只断言带 BTI 的 aarch64。
这一缺陷让 openSUSE Tumbleweed 的 aarch64 rpm 在这类硬件上无法显示界面，除非发行版修复 JavaScriptCore；产品是否在支持声明里排除该组合由产品决定。
校验路径的白名单已加入 openSUSE 的 `/usr/libexec/libwebkit2gtk-4_1-0/`。

## 发布与运行验收的边界

`release.yml` 调用 `build.yml` 时固定 `run_acceptance: false`。Linux 的 `package`
作业只构建与打包，保留四种命名产物、manifest、源码与 daemon 哈希、架构和 glibc
下限的必要校验；不构建 E2E GUI、升级夹具或遥测测试 daemon。

`linux-gui-acceptance.yml` 承担 AppImage 的 full / negative / smoke、运行时库库存、
deb/rpm 安装升级卸载与遥测注入合同。`package-linux-gui.yml` 在相关 PR 与独立手动
运行时默认调用它，使用同一次运行中的包和固定源码 SHA。验收从已验证 deb 提取真实
release daemon，核对 manifest 哈希，再构建测试控制面的 GUI；它不重复编译生产 daemon。
因此 Linux 构建作业的成功只表示打包和必要结构校验成功，完整运行验收有独立状态。

macOS/Windows 的运行 smoke 作业也遵循同一个边界。Windows 签名故障注入属于独立
验收；最终返回文件集合、签名前后内容一致性、证书与最终载荷验证始终保留。
非 SignPath 发布仅构建 shipped 包。既有 SignPath 外部配置要求 newer 目录与第二个
setup，所以该后端仍构建并签名这个夹具以满足精确签名输入合同，但发布不运行它的
安装或更新验收。移除该签名输入需要先单独更新并验证外部 artifact configuration；
本改动不修改配置、证书、secret 或 stable/beta/rc 的签名要求。

标准发布顺序仍是手动 prepare-release、release PR、合并后创建 tag、tag 触发发布。
旧 tag 的重跑使用旧工作流，不会获得 main 上的修复；恢复发布必须另行选择可审查的
新发布提交与新 tag，或明确授权对旧发布流程进行恢复。不得用夹具或旧 SHA 的包冒充
修复提交的发布证明。
