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
| W3 | 包内没有 `libgtk-layer-shell`：面板静默退化为普通窗口 | 合成器侧 layer 列表中必须出现命名空间 `uniclipboard-quick-panel`；缺库的负对照（临时目录里的包副本去掉该库）必须显示回退为普通窗口而不是崩溃 |
| W4 | `libgtk-layer-shell` 与 `libwayland-client` 的加载顺序（库文档要求它先于后者）导致初始化失败 | `gtk_layer_is_layer_window` 在真实窗口上为真，合成器侧确认 layer 类型；失败则记录库自己的错误并选择成熟的补救（链接/预加载），不自写协议 |
| W5 | layer surface 的类型、位置、键盘焦点与隐藏、退出不符合 Tauri 原合同 | 合成器侧：layer 为 `overlay`、锚点与边距落在所选输出的可用区域内、键盘交互在显示时为独占、隐藏后 layer 消失、退出后无残留；输入通过合成器自己的 IPC 或 `wtype` 在任务自有窗口上做，不用全局键位绑定 |
| W6 | X11 / XWayland 既有路径退化：X11 会话、`GDK_BACKEND=x11` 由用户设置、无 `WAYLAND_DISPLAY` | 在同一不可变包上，Xvfb 与 XWayland 强制场景仍通过 17c8/17c9 的既有 X11 面板检查；用户显式设置的 `GDK_BACKEND` 必须被尊重（含 `x11`） |
| W7 | 不支持 `wlr-layer-shell` 的合成器（GNOME）上崩溃 | 本片没有 GNOME 主机：只能用缺协议的合成器或缺库的负对照间接覆盖，并明确标为间接，不声称 GNOME 通过 |
| W8 | 去掉强制后，宿主辅助程序继承到的环境变化（`GDK_BACKEND` 不再被设为 `x11`） | 17c10/17c11 的辅助程序回归必须在新包上重跑 |
| W9 | 打包的 `libgtk-layer-shell` 来源不明或与包内 GTK 的 ABI 不匹配 | 清单记录 dpkg 所属包、版本、SHA-256、NEEDED 闭包；包内内容检查把它纳入固定集合 |

## 范围与边界

- 主机：`ssh fedora`（niri，虚拟机，aarch64）与 `ssh omarchy`（Hyprland，真机，aarch64）。只做任务自有目录、独立 profile（便携 HOME）、独立端口和私有总线；不改会话全局代理、默认应用、按键绑定、自启动、钥匙串，不碰真实 profile、space、密钥、历史与通用剪贴板，不停止用户的真实 GUI 与辅助进程。
- 测试在 **最终不可变包** 上做；记录源码 HEAD 与是否干净、daemon SHA-256、包 SHA-256、依赖来源、主机与进程。旧包（17c12 的 `3669e047…`）的绿色结果不迁移到新包。
- 没有 amd64、GNOME、KDE 的主机：保持 OPEN，不写成通过。Omarchy 是已授权真机：GPU、输出（显示器）数量与现有缩放比例的只读识别在授权范围内，是否具备多屏 / HiDPI 的可验证条件要先查（只读，不改分辨率、缩放或桌面配置）再判定，结果写入「契约修订」；登录注销与 suspend 另有授权边界，本片不做，仍 OPEN。

## 契约修订

1. **更正（只读复查）：两台原生主机 *都有* `libgtk-layer-shell`**（Omarchy `gtk-layer-shell 0.10.1`，Fedora `gtk-layer-shell-0.10.0-1.fc44`）。上文「源码调查」表里「两台原生主机没有安装」是我第一次只读识别时 `ls` 的 zsh 通配符报错造成的误读，不是事实。所以 W3 在这两台主机上不会因缺库而退化；包内自带该库的理由改为：GNOME/Ubuntu 一类桌面默认不装它，AppImage 不应依赖宿主；并且与 Tauri 的 AppImage 一致。自带副本优先于宿主副本（`LD_LIBRARY_PATH` 以 `$APPDIR/usr/lib` 开头），因此最终验收要用进程映射证明加载的是包内副本；宿主副本只用于对照。包内是 Ubuntu 24.04 的 0.8.2，宿主是 0.10.x，也是一次跨版本的真实检验。
2. **预实验 exp1（探索性，不是产品证据）**：17c12 不可变包解压后，仅在副本里去掉钩子那一行，在两台主机上各跑 25 秒：原生 Wayland 下 GUI 与 daemon 启动、不崩溃、没有 EGL/协议错误，合成器侧 `xwayland=0`（Hyprland）/Wayland app id（niri）；保留钩子则是 X11 套接字与 `xwayland=1`。前端就绪、HTTP/WebSocket、layer surface 与面板行为在 exp1 中 **未探测**，由最终包的 E2E 回答。摘要 `exp1-summary.json` 与完整日志在工件目录 `linux-17c13/exp1/`。
3. **多屏 / HiDPI 条件（只读识别）**：Omarchy 只有 1 个输出（`eDP-1`，3024x1964@120，scale 2），Fedora 虚拟机只有 1 个输出（`Virtual-1`，3360x1890@75，scale 2）。两台都有 HiDPI scale 2，可以验证缩放；**都没有第二个输出**，多屏验收需要改变会话输出配置，不在授权范围内，保持 OPEN。
