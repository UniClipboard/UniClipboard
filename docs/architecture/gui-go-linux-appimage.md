# Go GUI 的自包含 Linux AppImage（切片 17c4）

本文是 Go/Wails GUI 的 AppImage 打包契约：失败方式、E2E 范围、明确未证明的边界。实现之前先写，结果在文末「验证结果」章节补录。库策略（哪些库必须来自宿主）沿用 [linux-appimage-library-policy.md](linux-appimage-library-policy.md)，本文不重复其根因分析。

## 背景与现状

17c 的 `apps/gui-go/e2e/package_linux.py` 用 `appimagetool` 把 `usr/bin/{uniclipboard,uniclipd}` 直接打包，**不带任何 GTK、WebKitGTK 与 GLib**，依赖宿主已装 `libwebkit2gtk-4.1`。它不是 Tauri AppImage 的替代：Tauri 的 AppImage 自带这些库，用户机器上没有 WebKitGTK 也能启动。17c4 的目标是让 Go GUI 的 AppImage 达到同样的自包含程度，并把 17c3 证明过的更新机制接到真实 AppImage 上。

## Wails 优先审计（固定版 `v3.0.0-beta.28`）

核对对象是模块缓存中的源码（`internal/commands/appimage.go`、`linuxdeploy-plugin-gtk.sh`、`build_assets/linux/appimage/build.sh`），不是官方文档。

| 需求 | Wails 提供 | 结论 |
| --- | --- | --- |
| AppImage 生成命令 | `wails3 generate appimage`：linuxdeploy + 内嵌 `linuxdeploy-plugin-gtk.sh` + 复制 `WebKitWebProcess`、`WebKitNetworkProcess`、injected bundle 到 AppDir 的同一绝对路径 | 部分采用：**直接复用其内嵌的 GTK 插件脚本**（从固定版模块读取并记录 SHA-256，不复制进仓库） |
| 固定工具版本 | 否：下载 `linuxdeploy/continuous` 与 `AppImageKit/continuous` 的 AppRun，均不固定 | 缺口：库策略要求固定 `linuxdeploy-07333c6`（排除 `libwayland-client`）；`generateAppImage` 会先 `RMDIR` AppDir，无法预置固定的 AppRun |
| 两个可执行文件 | 否：只处理一个 `Binary` | 缺口：daemon `uniclipd` 必须与 GUI 同在 `usr/bin`（`spawn.rs` 先找兄弟路径） |
| WebKit 辅助进程重定位 | 否：只复制文件，不改 `libwebkit2gtk` 里硬编码的 `/usr/lib/<triple>/webkit2gtk-4.1`（已用 `strings` 在 Ubuntu 24.04 的 2.52.6 上确认存在该字符串） | 缺口：宿主没有 WebKitGTK 时辅助进程找不到；Tauri 的 bundler 用同长度的字符串替换解决，见下 |
| GDK 后端 | 插件脚本导出 `GDK_BACKEND=x11`（与 Tauri 一致） | **17c13 起不再采用**：17c4–17c12 把它作为兼容选择记录（AppImage 经 X server），17c13 删除该行并自带 `libgtk-layer-shell`，AppImage 在真实 Wayland 会话里使用原生 GDK 后端与 Layer Shell（`gui-go-linux-appimage-native-wayland.md`）；用户显式的 `GDK_BACKEND` 仍被遵守 |
| 自启动 | `app.Autostart`（XDG） | 已采用；AppImage 内 `os.Executable()` 指向临时挂载，已有最小适配（`autostart_linux.go`，`Exec=$APPIMAGE`）；本片只做真实 AppImage 验证，不新增机制 |
| 更新安装 | 无 | 业务语义（`internal/update/appimage.go`），17c3 已有 |

不能用 `wails3 generate appimage` 的原因因此是证据确凿的三项缺口，而非偏好。打包脚本只补这三项，其余（GTK 插件、AppDir 约定）沿用。

## 打包设计

1. 工具链：`linuxdeploy-07333c6`（固定值现在写在 `e2e/package_linux.py` 的 `LINUXDEPLOY_RELEASE` / `LINUXDEPLOY_SHA256`；原先共用的 `scripts/linux-appimage-tools.mjs` 已随 Tauri 打包器移除）；GTK 插件为固定版 Wails 内嵌脚本；成品封装继续使用已固定标签的 `appimagetool 1.9.0`，并校验其 SHA-256。
2. `linuxdeploy --appdir … -e usr/bin/uniclipboard -e usr/bin/uniclipd --plugin gtk`，不使用 `--output appimage`。
3. WebKit：复制三件辅助文件（`WebKitWebProcess`、`WebKitNetworkProcess`、`WebKitGPUProcess`，若存在）与 injected bundle 目录；对 AppDir 内的 `libwebkit2gtk-4.1.so.*` 做 `/usr` → `././` 的等长替换。`AppRun` 在导出环境后 `cd "$APPDIR"`，使相对路径可解析。这是 Tauri bundler 的既有做法，不是新发明；副作用见失败方式 F6。
4. `AppRun`：自有的极短脚本，依次 source `apprun-hooks/*.sh`、设置 `GIO_MODULE_DIR`（取代 Tauri 路径里 `process_environment.rs` 做的事；插件只设置追加语义的 `GIO_EXTRA_MODULES`，见策略文档「GIO 模块 ABI 混用」）、导出 `UC_APPIMAGE_ORIGINAL_CWD`、`exec usr/bin/uniclipboard "$@"`。
5. daemon 来自 `cargo build --locked --release -p uc-daemon --bin uniclipd`（与旧 Tauri 外壳的 sidecar 准备脚本、现在的 `scripts/stage-daemon.mjs` 相同的命令与 `[profile.release]`），Engine 来自 `Cargo.lock` 记录的不可变 git 修订。构建证据写入 `build-evidence.txt`：仓库 HEAD、daemon 相关源码是否有未提交改动、Engine 修订与 `Cargo.lock` 条目、`rustc -Vv`、产物 SHA-256。

## 失败方式（先于实现列出）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| F1 | AppImage 实际依赖宿主的 GTK/WebKitGTK/GLib（不自包含） | E2E 在 **没有** 安装 `libwebkit2gtk-4.1-0`、`libgtk-3-0` 的容器里启动；先对照跑「仅 Wails 式打包、不做重定位」的包，必须失败，证明测试能区分 |
| F2 | WebKit 辅助进程从宿主 `/usr/lib` 启动，或找不到而 WebView 空白 | 启动后遍历 `/proc/*/exe`，要求 `WebKitWebProcess`、`WebKitNetworkProcess` 的可执行文件位于 AppImage 挂载内；前端就绪由 evidence 的 `panelReady` 证明，不看截图 |
| F3 | 打包进了 `libwayland-client`、`libEGL`、`libGL*`、`libdrm`、Mesa 等宿主驱动栈库 | 解包后的文件清单检查（排除列表即策略）；`check-linux-bundles.py` 的同一断言在本包上执行 |
| F4 | `GIO_MODULE_DIR` 缺失，自带 GLib 加载宿主 GIO 模块（gvfs/dconf）崩溃 | 干净容器没有宿主 GIO 模块；另在带 gvfs 的 17c2 容器里再跑一遍启动，日志不得含 GIO/EGL/loader 错误 |
| F5 | daemon 不是真实 release 产物或与 GUI 版本不匹配（GUI 对 daemon 版本做握手） | 记录来源证据；启动后 `daemon.conn` 的 pid 的 `/proc/<pid>/exe` 位于挂载内，其 SHA-256 等于 `build-evidence.txt`；GUI 到 daemon 的 HTTP/WS 握手成功（`panelReady`） |
| F6 | `cd "$APPDIR/usr"` 破坏相对路径语义 | 已核对事实：GUI 唯一消费的参数是 quick-panel 启动标志（`main.go` 的 `hasArg`）与自启动的 `--autostart`，没有文件参数、没有 deep link/URL 处理，桌面入口里的 `%U` 无消费者。因此没有需要保留的相对路径语义，AppRun 不导出原工作目录（没有消费者的变量不加）；将来若加文件关联，必须在那时设计原目录传递 |
| F7 | 自启动条目指向临时挂载路径，或旧 Tauri 条目与新条目并存 | 真实 AppImage 内 `update_autostart(true)`：`Exec=` 必须等于 `$APPIMAGE` 且不在 `/tmp/.mount_*`；预置 Tauri 风格旧条目被清除；禁用后条目消失 |
| F8 | 更新验证链：未受信任签名被安装 | 复用 17c3 的 fixture 密钥：不受信任签名的下载必须被拒绝，AppImage 文件字节不变 |
| F9 | 更新后文件被替换但重启仍运行旧映像，或旧 daemon 残留 | 更新后新进程的 `/proc/<pid>/exe` 在新映像挂载内、`$APPIMAGE` 文件的 SHA-256 等于 v2、旧 daemon pid 已退出 |
| F10 | 重建 deb/rpm 丢失 17c2 加入的 `libgtk-layer-shell` 依赖，或 rpm 触发 build-id 冲突 | `dpkg-deb -I` / `rpm -qpR` 读取依赖；`rpmbuild` 无 build-id 报错并检查 `rpm -qp --list` |
| F11 | 把 e2e 标签二进制的 E2E 结果说成 release 构建的证明 | 打包 E2E 用 `gtk3,e2e` 二进制装入同一套 AppImage 管线；release 标签包另做 **无控制面** 的启动冒烟（窗口出现、daemon 存活、进程持续存活、日志无致命错误），两者在文档中分开陈述 |
| F12 | 用合成文本或 stub 当 AppImage/daemon | 所有 E2E 输入是 `package_linux.py` 的真实输出；daemon 的 ELF 检查与 SHA 对照 build 证据；脚本拒绝 `--packaging-check-fixture` 产物 |

## E2E 设计

镜像 `uc-gui-go-linux-runtime:17c4`（`apps/gui-go/e2e/linux/Dockerfile.17c4-runtime`）：`ubuntu:24.04` 上 **没有** GTK3/GTK4、WebKitGTK、JavaScriptCore、libsoup、cairo、pango、gdk-pixbuf。它只提供 AppImage 约定留给宿主的库。第一次启动失败（`probe1`：`libharfbuzz.so.0: cannot open shared object file`）后，没有凭「AppDir 里缺了」就往宿主里装库，而是先把包内二进制的 `NEEDED` 减去 AppDir 内已有的库，得到 23 个宿主提供的 soname（`probe2/host-provided-sonames.txt`），再逐个核对它们是否是 **固定版 linuxdeploy 排除列表** 中的一行（`probe3/exclude-check-exact.txt`：从 `linuxdeploy-07333c6` 二进制中提取的排序列表，23/23 为精确行）。因此 `libharfbuzz`、`libfreetype`、`libfontconfig`、`libfribidi`、`libexpat`、`libstdc++`、`libgcc_s`、`libgmp`、`libgpg-error`、`libcom_err`、`libz`、`libxcb`、`libX11`、`libX11-xcb`、`libdrm`、`libgbm`、`libEGL`、`libGL`、`libwayland-client` 与 libc 家族都是 **标准的宿主前置条件**，不是打包缺陷。

**宿主 GLib 的存在**：`libharfbuzz0b` 在 Debian/Ubuntu 上依赖 `libglib2.0-0t64`，所以这个宿主有 GLib/GIO，每个桌面都一样。因此验收口径是「宿主没有 GTK/WebKitGTK」，**不是**「宿主没有 GLib」。GLib 不在排除列表中，AppImage 自带一份；E2E 用 `/proc/<pid>/maps` 证明 GUI 与 WebKit 进程实际映射的是包内的 `libglib-2.0`、`libgio-2.0`（挂载路径下），而不是宿主的副本。

脚本 `apps/gui-go/e2e/linux_appimage_run.py`（运行在干净镜像中，复用 `linux_xvfb_run.py` 的 `Gui` 控制文件通道）：

1. `clean-host`：`ldconfig -p` 中没有 `libwebkit2gtk-4.1`、`libgtk-3`。
2. `launch`：用隔离临时 HOME、`UC_PORTABLE=1`、file keystore、`UC_DISABLE_SYSTEM_CLIPBOARD=1` 启动 AppImage；断言 `panelReady`、辅助进程与 daemon 的映像归属、daemon SHA。
3. `autostart`：F7 的三项断言；分别用 `release+e2e` 构建（release-no-profile：没有 `UC_PROFILE`，条目名是产品名 `UniClipboard.desktop`）与带 `UC_PORTABLE=1` 的运行；**非 portable 数据根** 需要 Secret Service，而 gnome-keyring 会把 GTK 拖进宿主，故先在干净宿主里直接观察 daemon 在无 Secret Service 时的表现，结果写入「验证结果」；若 daemon 不能启动，非 portable 数据根保持 OPEN 并给出该原因
4. `update-bad` / `update-good`：F8、F9；v2 AppImage 与 v1 由同一管线构建，v2 多一个 e2e 专用标记文件，签名用 `e2e/updatetool` 的隔离 fixture 私钥，feed 是本地 HTTP 服务。
5. `negative-control`：对照包（无重定位）在同一环境必须启动失败。

产物目录：`/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c4/`，仓库只索引。

## 明确不证明的边界

- **官方签名发布验证**：没有发布私钥；更新机制用隔离 fixture 私钥验证，不等于官方签名发布验证。
- **真实发布服务端与 FlareRelease**：feed 是本地 HTTP 服务。
- **原生桌面**：容器内只有 Xvfb，无 GNOME/KDE/Hyprland、无 portal、无托盘宿主、无通知守护进程；GPU 加速、真实 Wayland 会话不在范围。
- **amd64**：见「验证结果」；没有原生 x86_64 Linux 主机时，只有能真实构建并运行的部分才记为已证明。
- **release 标签 AppImage 的 UI 行为**：release 构建没有测试控制面，只有冒烟级证据。
- **AppImage 内的 Wayland / Layer Shell**（17c13 更新）：钩子行已删除，两台原生主机验证，范围与未验证项见 `gui-go-linux-appimage-native-wayland.md`；17c4–17c12 的「由 `GDK_BACKEND=x11` 排除」不再成立。

## 验证结果

完整运行（干净提交 `b57fd28e1`，`apps/gui-go/e2e/linux/run_17c4.sh`，细节、保留的失败运行与复跑命令见 `apps/gui-go/README.md`「自包含 Linux AppImage（第 17c4 片）」）：

| 运行 | 结果 |
| --- | --- |
| 干净宿主 + 真实 AppImage，`gtk3,production,release,e2e`（F1–F5、F7–F9、数据根、持久数据） | 34/34 |
| 无重定位对照包（F1/F2 的红灯） | 2/2，日志精确给出 `WebKitNetworkProcess (No such file or directory)` |
| `gtk3,production,release` 真实 release 包的冒烟（F11，无控制面，不是前端握手） | 5/5 |
| deb/rpm 重建（F10） | deb 依赖含 `libgtk-layer-shell0`；rpm `Requires` 含 `libgtk-layer-shell.so.0()(64bit)`（soname 能力，见 gui-go-linux-ci-packaging.md）；文件表无 `.build-id` |

设计与实现中被事实修正的几处：

- 宿主前置条件不是「没有任何库」：`libharfbuzz` 等 23 个 soname 都是固定版 linuxdeploy 排除列表的精确行（宿主提供是约定，不是打包缺陷）；它们带来宿主 GLib，所以验收口径是「宿主没有 GTK/WebKitGTK」，包内 GLib/GIO 由 `/proc/<pid>/maps` 证明被实际映射。
- `libGLESv2.so.2` 被 WebKit 与 libepoxy `dlopen`，不在排除列表中，linuxdeploy 也看不到它。17c4 当时把它作为同属 libglvnd 的宿主库处理（运行镜像装 `libgles2`），但没有系统地枚举过 `dlopen` 依赖，因此那时的绿灯只代表一个 Ubuntu 24.04 干净容器。17c7 已做审计与验证：见 [gui-go-linux-appimage-runtime-deps.md](gui-go-linux-appimage-runtime-deps.md)（`libGLESv2` 等 libglvnd 入口点被打包检查禁止，并由运行时 `/proc/<pid>/maps` 证明来自宿主；WebView HTTPS 的 GIO TLS 模块补上；在 Ubuntu 24.04 与 Fedora 44 两个宿主上验证）。仍然 **不能推广为任意 Linux 发行版可运行**：其他发行版、Mesa/glibc 版本与原生 amd64 仍未验证。
- linuxdeploy 会给传入的可执行文件加 `RUNPATH`，改变 daemon 字节；清单记录哈希链，包内的 daemon 放回原文件，身份可与构建证据逐字节对照。
- AppImage 的 runtime（嵌在 SquashFS 之前的 ELF）自 17c6 起由 `package_linux.py` 用 `--runtime-file` 固定，见 [gui-go-linux-appimage-runtime-pin.md](gui-go-linux-appimage-runtime-pin.md)。
- AppImage 内的 portable 模式在 17c4 **不可用**（数据根落在只读挂载，daemon 起不来，`probe5`）。**17c5 已解决并验收**：portable 的可写根改用 AppImage runtime 自己的 `<AppImage>.home` 约定，见 [gui-go-linux-appimage-portable.md](gui-go-linux-appimage-portable.md)。
  17c4 的验证仍是非 portable 的 XDG 数据根（因此需要 Secret Service，且必须是常驻、已解锁的单个实例：先前的 `--unlock` 交棒方式曾被 D-Bus 重新激活成锁定实例，`/encryption/state` 因等待无法显示的提示而挂起，保留的 `final-70dd6dcbe`）。
- 17c7 之后的状态：GIO 模块目录不再为空（含 `libgiognutls.so`），`libdbus-1` 不再随包。本文「打包设计」第 4 点说 `GIO_MODULE_DIR` 取代宿主目录仍然成立，但 17c4–17c6 的该目录是空的（插件不部署模块），F4 只验证了「不加载宿主模块」，没有覆盖 TLS 后端，这正是 17c7 补上的缺口。
- 自启动只验证注册（`Exec=` 是 AppImage 文件而不是临时挂载、旧条目替换、禁用）；真实注销/登录启动与更新之后的条目有效性、amd64、原生桌面、deb/rpm 实装、官方签名发布验证均未验收。
