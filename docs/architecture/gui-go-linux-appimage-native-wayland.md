# Go GUI 的 Linux AppImage 原生 Wayland 与 Layer Shell（切片 17c13）

本文是 17c13 的契约，**先于** 实现与真机运行写成。运行中发现的偏差记录在「契约修订」一节，不改写原条款。没有源码或运行证据的内容标为 OPEN，不当作事实。

## 问题

17c2 在容器里用真实 sway 验证了 `internal/layershell`（动态加载成熟的 `libgtk-layer-shell`，不自写 Wayland 协议），但 AppImage 里没有激活路径：linuxdeploy 的 GTK 插件钩子无条件 `export GDK_BACKEND=x11`，包内也没有 `libgtk-layer-shell`。17c12 的两台原生主机因此只证明了「Wayland 会话里经 XWayland 运行」。本片要让 AppImage 在真实 Wayland 会话里使用 GDK 原生 Wayland 后端，并在支持 `wlr-layer-shell` 的合成器上让快捷面板成为真正的 layer surface；X11 与 XWayland 的既有路径保持可用。

## 源码调查（针对固定版本 Wails `v3.0.0-beta.28`，写运行器之前所做）

| 事实 | 来源 |
| --- | --- |
| Wails 自身只在 `GDK_BACKEND` 未设置 **且** `XDG_SESSION_TYPE` 为空、`unspecified` 或 `x11` 时才把 `GDK_BACKEND` 设为 `x11`；`XDG_SESSION_TYPE=wayland` 的会话不被强制 | 模块 `pkg/application/application_linux_gtk3.go` 的 `init`（第 31-35 行） |
| 强制 X11 的是 GTK 插件钩子：`export GDK_BACKEND=x11 # Crash with Wayland backend on Wayland`，不管会话类型 | 模块 `internal/commands/linuxdeploy-plugin-gtk.sh` 第 246 行（包内已用这份脚本，SHA-256 写入包清单） |
| SSH 登录的 shell 没有 `XDG_SESSION_TYPE`（`tty` 或空）：从 SSH 启动 GUI 时，Wails 的 `init` 会因此自己设 `x11`。这是 **测试方法的陷阱**：运行器必须使用会话的真实环境，并以「GUI 进程实际连接了哪个显示服务器」判定，不以环境变量判定 | 本片只读识别（两台主机的 `loginctl` 与 `XDG_SESSION_TYPE`） |
| 包内 `libwayland-client` 被刻意排除（宿主驱动栈，17c6 策略）；GDK Wayland 后端因此使用宿主的 libwayland | `e2e/package_linux.py` 的 `HOST_ONLY_LIBS`、`docs/architecture/linux-appimage-library-policy.md` |
| 两台原生主机没有安装 `libgtk-layer-shell`；因此 AppImage 必须自带它，否则面板静默退化为普通窗口 | 本片只读识别 |
| `internal/layershell` 已有：`dlopen("libgtk-layer-shell.so.0")`、缺库或缺协议时保持普通窗口、需要未实现（unrealized）窗口 | `apps/gui-go/internal/layershell/layershell_linux.go` |

结论：不新增第二套窗口或 Wayland 模块。要做的是（1）去掉钩子对后端的无条件强制并改成可解释的策略，（2）带上并核对 `libgtk-layer-shell` 的来源，（3）用真实合成器证明。具体做法在实验之后写入「实现」一节。

## 上游钩子为何强制 X11（调查，2026-10-06）

- 钩子行的注释是 `Crash with Wayland backend on Wayland`，没有指向具体崩溃。
- 上游 `linuxdeploy/linuxdeploy-plugin-gtk` 的开放议题 #64（「Reconsider unconditional GDK_BACKEND=x11」）指出：被引用的依据 `tauri-apps/tauri#8541` 其实是「在 `ubuntu:20.04` 容器里构建的 AppImage 在较新宿主上因 GSettings 模式（schema）不匹配崩溃（`org.gnome.settings-daemon.plugins.xsettings` 没有键 `antialiasing`）」，`GDK_BACKEND=x11` 只是该错误的绕过办法；同一议题报告强制 X11 使 NVIDIA + Wayland 上的 WebKitGTK 退回软件渲染。议题作者没有 Wayland 崩溃的复现。
- `tauri-apps/linuxdeploy-plugin-gtk` 曾提交「fix: Disable GDK_BACKEND overwrite」（`4f78576b`，2023-12-06），随后被还原（`b5eb8d05`，2024-01-04）；两个提交消息都没有给出理由。
- 这些是 **第二手线索**，不是本产品的证据：本包在 Ubuntu 24.04 构建镜像里构建，GSettings 模式来自包内（钩子导出 `GSETTINGS_SCHEMA_DIR`），不是 20.04。该崩溃在当前 GTK 3 / WebKitGTK 2.5x / Mesa 与两台主机上是否仍出现，只能由本片的差分实验回答：同一包、同一主机，仅变化后端选择，记录 GUI/WebProcess 是否崩溃、stderr 的 GLib/GTK/WebKit 错误、渲染与前端就绪。**只删掉 `GDK_BACKEND` 一行不等于支持原生 Wayland。**

## 对旧记录的更正

17c12 的 handoff 与 `gui-go-linux-appimage-system-proxy.md` 把强制 X11 归因于「Wails GTK 插件」。源码核对：Wails `init` 只在会话类型为空、`unspecified` 或 `x11` 时设置；强制来自 AppRun 钩子（见上表）。17c12 原生主机结果「经 XWayland 运行」的结论不变：GUI 环境确实是 `GDK_BACKEND=x11`，并非原生 Wayland，不是 amd64。

## 失败方式与验收

| # | 失败方式 | 如何被发现（只认实际观察，环境变量与构建成功都不算证据） |
| --- | --- | --- |
| W1 | 钩子仍强制 `x11`：GUI 经 XWayland 运行，Layer Shell 不会激活 | 真实主机上 GUI 进程自身报告的 GDK 显示类型（`GdkWaylandDisplay`），GUI 与 WebKitWebProcess 持有到合成器 Wayland socket 的连接，且 GUI 进程没有到 `.X11-unix` 的连接 |
| W2 | 去掉强制后，包内的 GTK3/WebKitGTK 在原生 Wayland 下崩溃、空白或无法渲染（钩子注释所称的 crash） | 窗口真实出现（合成器侧列出窗口且 `xwayland` 为假），前端经真实 daemon 的 HTTP 与 WebSocket 就绪，持续运行，退出码与 stderr 记录 |
| W3 | 包内没有或没有加载 `libgtk-layer-shell`：面板退化为普通窗口，或「成功」其实用的是宿主副本（不是自带包的证明） | 真实进程映射（`/proc/<pid>/maps`）显示被加载的 `libgtk-layer-shell.so.0` 来自 AppImage 挂载内，且 GTK 闭包（libgtk-3、libgdk-3、libglib/libgio）也来自挂载；合成器 layer 列表中出现命名空间 `uniclipboard-quick-panel`。**缺库负对照不在这两台主机上做**：它们都装有宿主副本，删除或改名用户的库不在授权内，装软件也不允许；缺库回退由任务自有的受控环境（17c2 的容器 sway，`run.sh wayland-nolib` 在抛弃式容器里真的移除库）覆盖，并明确标为容器证据，不作原生主机结果 |
| W4 | `libgtk-layer-shell` 与 `libwayland-client` 的加载顺序（库文档要求它先于后者）导致初始化失败 | `gtk_layer_is_layer_window` 在真实窗口上为真，合成器侧确认 layer 类型；失败则记录库自己的错误并选择成熟的补救（链接/预加载），不自写协议 |
| W5 | layer surface 的类型、位置、键盘焦点与隐藏、退出不符合 Tauri 原合同 | 合成器侧：layer 为 `overlay`、锚点与边距落在所选输出的可用区域内、键盘交互在显示时为独占、隐藏后 layer 消失、退出后无残留；输入通过合成器自己的 IPC 或 `wtype` 在任务自有窗口上做，不用全局键位绑定 |
| W6 | X11 / XWayland 既有路径退化：X11 会话、`GDK_BACKEND=x11` 由用户设置、无 `WAYLAND_DISPLAY` | 在同一不可变包上，Xvfb 与 XWayland 强制场景仍通过 17c8/17c9 的既有 X11 面板检查；用户显式设置的 `GDK_BACKEND` 必须被尊重（含 `x11`） |
| W7 | 不支持 `wlr-layer-shell` 的合成器（GNOME）上崩溃 | 本片没有 GNOME 主机：容器里的缺库对照与已有的「无协议」分支只能间接覆盖，并明确标为间接，不声称 GNOME 通过 |
| W8 | 去掉强制后，宿主辅助程序继承到的环境变化（`GDK_BACKEND` 不再被设为 `x11`） | 17c10/17c11 的辅助程序回归必须在新包上重跑 |
| W9 | 打包的 `libgtk-layer-shell` 来源不明或与包内 GTK 的 ABI 不匹配 | 清单记录 dpkg 所属包、版本、SHA-256、NEEDED 闭包；包内内容检查把它纳入固定集合 |
| W10 | 会话类型未知（`XDG_SESSION_TYPE` 为空或 `unspecified`）但存在可用的 Wayland socket，且用户没有设置 `GDK_BACKEND`：Wails 的 `init` 把 `GDK_BACKEND` 设为 `x11`，GUI 仍经 XWayland，Layer Shell 不会激活（final-1 的 `--no-session-type` 两台主机各一次实际复现，原始失败保留） | 同一包、同一主机：不导出 `XDG_SESSION_TYPE` 启动，GUI 必须连接 Wayland socket、无 X11 连接、面板为 layer surface；显式 `GDK_BACKEND=x11`（含空会话类型）、`XDG_SESSION_TYPE=x11`、没有 Wayland socket 的情形必须保持 X11 |

## 范围与边界

- 主机：`ssh fedora`（niri，虚拟机，aarch64）与 `ssh omarchy`（Hyprland，真机，aarch64）。只做任务自有目录、独立 profile（便携 HOME）、独立端口和私有总线；不改会话全局代理、默认应用、按键绑定、自启动、钥匙串，不碰真实 profile、space、密钥、历史与通用剪贴板，不停止用户的真实 GUI 与辅助进程。
- 测试在 **最终不可变包** 上做；记录源码 HEAD 与是否干净、daemon SHA-256、包 SHA-256、依赖来源、主机与进程。旧包（17c12 的 `3669e047…`）的绿色结果不迁移到新包。
- 没有 amd64、GNOME、KDE 的主机：保持 OPEN，不写成通过。Omarchy 是已授权真机：GPU、输出（显示器）数量与现有缩放比例的只读识别在授权范围内，是否具备多屏 / HiDPI 的可验证条件要先查（只读，不改分辨率、缩放或桌面配置）再判定，结果写入「契约修订」；登录注销与 suspend 另有授权边界，本片不做，仍 OPEN。

## 契约修订

1. **更正（只读复查）：两台原生主机 *都有* `libgtk-layer-shell`**（Omarchy `gtk-layer-shell 0.10.1`，Fedora `gtk-layer-shell-0.10.0-1.fc44`）。上文「源码调查」表里「两台原生主机没有安装」是我第一次只读识别时 `ls` 的 zsh 通配符报错造成的误读，不是事实。所以 W3 在这两台主机上不会因缺库而退化；包内自带该库的理由改为：GNOME/Ubuntu 一类桌面默认不装它，AppImage 不应依赖宿主；并且与 Tauri 的 AppImage 一致。自带副本优先于宿主副本（`LD_LIBRARY_PATH` 以 `$APPDIR/usr/lib` 开头），因此最终验收要用进程映射证明加载的是包内副本；宿主副本只用于对照。包内是 Ubuntu 24.04 的 0.8.2，宿主是 0.10.x，也是一次跨版本的真实检验。
2. **预实验 exp1（探索性，不是产品证据）**：17c12 不可变包解压后，仅在副本里去掉钩子那一行，在两台主机上各跑 25 秒：原生 Wayland 下 GUI 与 daemon 启动、不崩溃、没有 EGL/协议错误，合成器侧 `xwayland=0`（Hyprland）/Wayland app id（niri）；保留钩子则是 X11 套接字与 `xwayland=1`。前端就绪、HTTP/WebSocket、layer surface 与面板行为在 exp1 中 **未探测**，由最终包的 E2E 回答。摘要 `exp1-summary.json` 与完整日志在工件目录 `linux-17c13/exp1/`。
3. **多屏 / HiDPI 条件（只读识别）**：Omarchy 只有 1 个输出（`eDP-1`，3024x1964@120，scale 2），Fedora 虚拟机只有 1 个输出（`Virtual-1`，3360x1890@75，scale 2）。两台都有 HiDPI scale 2，可以验证缩放；**都没有第二个输出**，多屏验收需要改变会话输出配置，不在授权范围内，保持 OPEN。
4. **W10 的发现（final-1，原始失败保留）**：`--no-session-type` 在两台主机上各跑一次：真实 Wayland socket 存在、没有显式 `GDK_BACKEND`，GUI 却连接 X11（31 项中 Fedora 11 项、Omarchy 10 项失败）。原因是 Wails 的 `init`（见源码调查）。真实会话本身会导出 `XDG_SESSION_TYPE=wayland`（读取了 niri 与 Hyprland 进程的环境），所以这是启动器没有导出该变量时的边界，不是常规路径；但它使「原生 Wayland」依赖一个环境变量的有无，按本片目标修复。见「实现」。
5. **观测模型更正（final-2 `x11session`，原始失败保留）**：`x11-session` 场景第一版断言「GUI 进程环境里 `GDK_BACKEND` 为 x11」，在两台主机上失败，而 socket 是 X11、退出正常。原因是观测模型错误：`/proc/<pid>/environ` 是进程启动时的初始环境块，Wails 在进程内 `os.Setenv` 的值不在其中（生产修复本身也依赖这一区别）。因此该场景的证据改为 socket 与合成器列表，并断言初始环境 **没有** 该变量；同一包（`2ec65ac8…`）上用唯一标签 `final2-x11session-b-*` 重跑，两台主机通过。第一版的两个失败运行目录保留。
6. 其余运行器说明：Omarchy 会话自己为所有应用导出 `GDK_BACKEND=wayland,x11,*`（读取用户环境得到）。旧钩子会无视它，新包遵守它；已作为 `--gdk-backend` 场景实测。

## 实现（最窄、复用成熟机制）

| 变化 | 位置 | 说明 |
| --- | --- | --- |
| AppRun 钩子不再无条件导出 `GDK_BACKEND=x11` | `apps/gui-go/e2e/package_linux.py` `release_gdk_backend` | 从生成的 `apprun-hooks/linuxdeploy-plugin-gtk.sh` 删除这一行，必须恰好一行；清单记录被删行和钩子前后 SHA-256。GTK 自己选后端（有 Wayland 则 Wayland），用户显式的 `GDK_BACKEND` 被遵守 |
| 包内自带 `libgtk-layer-shell.so.0` | `package_linux.py` `deploy_layer_shell` | 来自构建镜像的 `libgtk-layer-shell0`（Ubuntu 24.04，0.8.2-1build2），记录所属包、版本、SHA-256、NEEDED 闭包；唯一允许的宿主依赖是 `libwayland-client.so.0`（宿主驱动栈，17c6 策略）。没有自写协议、没有改 `internal/layershell`（它本来就 `dlopen`） |
| 会话类型未知时撤销 Wails 的 X11 默认 | `apps/gui-go/gdk_backend_linux.go` | 仅当：初始环境（`/proc/self/environ`）没有 `GDK_BACKEND`、当前值是 `x11`、`XDG_SESSION_TYPE` 为空或 `unspecified`、`WAYLAND_DISPLAY` 指向存在的 Unix socket。显式 `GDK_BACKEND`（含 `x11`）、`XDG_SESSION_TYPE=x11`、无 Wayland socket 的主机保持 X11 |
| 差分与负对照包 | `package_linux.py` | `--negative-control-keep-x11-hook`（`X11HOOK-`，保留旧钩子行为）、`--negative-control-no-layer-shell`（`NOLAYER-`，不带库） |
| 内容检查 | `e2e/linux/appimage_content_check.py` | C10（库字节等于清单、soname 解析）、C10b（闭包）、C11（任何钩子都不导出 `GDK_BACKEND`），两个对照用 `--expect-x11-hook`、`--expect-no-layer-shell` |

Wails 优先复查（固定版本 `v3.0.0-beta.28`）：Wails 没有 Layer Shell、没有 AppImage 后端选择 API；它的 `init` 只给出上述 X11 默认。其余（窗口、自启动、托盘、单实例）原样复用。

## 验证结果

来源：最终包由干净提交 `dbd9ea238` 构建（`final-2`，清单 `dirty=false`、`immutable=true`；之后只有测试运行器与文档提交）。`final-1`（提交 `0aee9c966`，不含 W10 修复）保留，其结果不转记到 final-2。daemon 仍是官方固定 SHA-256 `ea0f0bcb…`（构建前核对）。

| 包 | SHA-256 | 来源 |
| --- | --- | --- |
| final-2 产品包（`E2E-UniClipboard_1.1.1_arm64.AppImage`） | `2ec65ac854c6c40f91837aa0b5a36bc6b14bb28eef2cdbc5131a5c0bed0970c8` | `dbd9ea238`，E2E 控制面构建（`gtk3,production,release,e2e`） |
| `X11HOOK-` 差分对照 | `364c3393844f1000db42…` | 同提交，保留旧钩子 |
| `NOLAYER-` 缺库对照 | `ec905dce95375d54f712…` | 同提交，不带库 |
| final-1 产品包 | `4863f38bc90733f6da75ffbe2c00caf368bbfd21d1b2c4061db1234926a7ff8a` | `0aee9c966`（无 W10 修复） |

**原生主机（aarch64；每次运行是任务自有目录、便携 HOME、私有总线；结构化结果 `native-host-summary.json` 与各运行目录的 `native-wayland-result.json`）**

| 场景（final-2 包） | Omarchy Hyprland 0.56.1（真机） | Fedora 44 niri 25.11（虚拟机） |
| --- | --- | --- |
| `native`（真实会话环境，`XDG_SESSION_TYPE=wayland`） | 33/33，rc 0：Wayland socket、无 X11 连接、合成器 `xwayland=0`；包内 `libgtk-layer-shell` 与 GTK 闭包映射自挂载；合成器列出 `uniclipboard-quick-panel`（overlay）与 `…-dismiss` 层；矩形在输出内且居中；独占键盘；**`wtype` 注入的按键到达面板页面**；热键切换隐藏后 layer 消失、第二次显示再现；`control exit` 状态 0；无残留 | 31/31，rc 0，3 项 UNKNOWN（见下） |
| `native` + 未导出 `XDG_SESSION_TYPE`（W10） | 33/33，rc 0 | 31/31，rc 0 |
| `native` + 会话的 `GDK_BACKEND=wayland,x11,*` | 33/33，rc 0 | 未运行（Fedora 会话没有该变量） |
| `x11-env`（用户设 `GDK_BACKEND=x11`） | 18/18，rc 0（`xwayland=1`） | 17/17，rc 0 |
| `x11-env` + 未导出 `XDG_SESSION_TYPE` | 18/18 | 17/17 |
| `x11-session`（`XDG_SESSION_TYPE=x11`，有 Wayland socket） | 第二版 18/18；第一版 17/18（观测模型错误，见修订 5） | 第二版 17/17；第一版失败 1 项 |
| `x11-hook`（`X11HOOK-` 包） | 18/18：旧行为复现 | 17/17 |

**final-1 的原始失败保留**：`final1-nosessiontype-*`（W10 实际复现，Omarchy 10 项失败、Fedora 11 项失败，rc 1）；final-1 其余原生与 X11 场景全部通过（含 `X11HOOK-`）。

**容器（Docker，arm64，抛弃式，明确是容器证据，不是 GNOME，不是原生主机）**

| 对照 | 结果 |
| --- | --- |
| Weston 13 无头合成器，真实 `wayland-info` 注册表中 `zwlr_layer_shell_v1` **未通告**（计数 0） | 19/19：GUI 用 Wayland 后端，Layer Shell 报告不支持，面板保持普通窗口，前端就绪，退出码 0；包内库已加载但协议不存在 |
| sway（注册表通告 `zwlr_layer_shell_v1`，计数 1），宿主已移除 `libgtk-layer-shell`（镜像检查断言），`NOLAYER-` 包 | 19/19：GUI 报告 `GTK3 Layer Shell runtime is not installed`，`maps` 中没有任何 `libgtk-layer-shell`，面板为普通窗口，前端就绪，退出码 0 |
| 内容检查（产品包、两个对照包） | 全部通过（含 C10/C10b/C11） |
| 17c10 宿主辅助程序（Ubuntu/Fedora × generic/gnome，4 个） | 全部通过 |
| 17c7 WebView TLS（Ubuntu、Fedora） | 通过 |
| 17c5 便携 + 更新门（`e2e-portable`，带标记 v2） | 通过 |
| 17c4 完整 AppImage 运行（Xvfb，`e2e-full`） | 通过 |

## 本片没有验证（OPEN，不当作通过）

- **Fedora/niri**：面板矩形与居中无法由 `niri msg layers` 读取（UNKNOWN）；该主机没有按键注入工具，layer 的键盘焦点只由客户端状态与合成器的键盘交互报告证明，没有实际按键到达页面的证据。Hyprland 的光标跟随（`follow_cursor`）需要移动真实指针，未做。
- **多屏**：两台主机各只有一个输出，多输出的 per-output 背板与面板输出选择未在原生主机验证（容器 sway 17c2 有 2 输出结果）。HiDPI 只覆盖 scale 2 的单输出。
- **GNOME、KDE、amd64、其他发行版与 Mesa/glibc、真实 NVIDIA 等 GPU**：没有主机。Weston 对照只证明「协议缺失」分支，不是 GNOME。
- 登录注销自启、suspend、portal、托盘通知、deb/rpm 安装、AppImage 的 `extract-and-run`/只读/legacy 边界：本片未做。
- 渲染正确性（没有截图比对）、长时间运行（每次 8–10 秒再加面板周期）。
- 17c12 的代理矩阵没有在新包上重跑（改动不触及代理路径：GIO 模块、PAC 助手与 `NO_PROXY` 代码未变）；17c12 的 R1-R5 仍 OPEN。
- Tauri 的 AppImage 本身仍强制 X11；Tauri 参照包未运行。

## 复现

```bash
# 容器与回归（干净提交，输出目录必须为空）
UC_FEED_INPUTS=<目录：pubkey.b64 与 good.sig.b64> apps/gui-go/e2e/linux/run_17c13.sh <outdir>
# 原生主机（在目标主机上，任务自有目录；最终包作为 --appimage）
python3 apps/gui-go/e2e/native_wayland_probe.py --appimage <AppImage> --out <新目录> --mode native|x11-env|x11-session|x11-hook [--no-session-type] [--gdk-backend=<值>]
```

