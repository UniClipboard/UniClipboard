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
| GDK 后端 | 插件脚本导出 `GDK_BACKEND=x11`（与 Tauri 一致） | 采用，并作为 **明确兼容选择** 记录：AppImage 内的窗口始终经 X server（Wayland 下是 Xwayland），GDK 原生 Wayland 后端与 Layer Shell（`libgtk-layer-shell` 需要 Wayland 后端的 GTK 窗口）在 AppImage 内 **不工作**，不得称 AppImage 覆盖 Wayland 面板。deb/rpm 没有该限制 |
| 自启动 | `app.Autostart`（XDG） | 已采用；AppImage 内 `os.Executable()` 指向临时挂载，已有最小适配（`autostart_linux.go`，`Exec=$APPIMAGE`）；本片只做真实 AppImage 验证，不新增机制 |
| 更新安装 | 无 | 业务语义（`internal/update/appimage.go`），17c3 已有 |

不能用 `wails3 generate appimage` 的原因因此是证据确凿的三项缺口，而非偏好。打包脚本只补这三项，其余（GTK 插件、AppDir 约定）沿用。

## 打包设计

1. 工具链：`linuxdeploy-07333c6`（SHA-256 常量与 `scripts/linux-appimage-tools.mjs` 同源，脚本从该文件解析，不另存一份）；GTK 插件为固定版 Wails 内嵌脚本；成品封装继续使用已固定标签的 `appimagetool 1.9.0`，并校验其 SHA-256。
2. `linuxdeploy --appdir … -e usr/bin/uniclipboard -e usr/bin/uniclipd --plugin gtk`，不使用 `--output appimage`。
3. WebKit：复制三件辅助文件（`WebKitWebProcess`、`WebKitNetworkProcess`、`WebKitGPUProcess`，若存在）与 injected bundle 目录；对 AppDir 内的 `libwebkit2gtk-4.1.so.*` 做 `/usr` → `././` 的等长替换。`AppRun` 在导出环境后 `cd "$APPDIR"`，使相对路径可解析。这是 Tauri bundler 的既有做法，不是新发明；副作用见失败方式 F6。
4. `AppRun`：自有的极短脚本，依次 source `apprun-hooks/*.sh`、设置 `GIO_MODULE_DIR`（取代 Tauri 路径里 `process_environment.rs` 做的事；插件只设置追加语义的 `GIO_EXTRA_MODULES`，见策略文档「GIO 模块 ABI 混用」）、导出 `UC_APPIMAGE_ORIGINAL_CWD`、`exec usr/bin/uniclipboard "$@"`。
5. daemon 来自 `cargo build --locked --release -p uc-daemon --bin uniclipd`（与 `scripts/prepare-sidecars.mjs` 相同的命令与 `[profile.release]`），Engine 来自 `Cargo.lock` 记录的不可变 git 修订。构建证据写入 `build-evidence.txt`：仓库 HEAD、daemon 相关源码是否有未提交改动、Engine 修订与 `Cargo.lock` 条目、`rustc -Vv`、产物 SHA-256。

## 失败方式（先于实现列出）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| F1 | AppImage 实际依赖宿主的 GTK/WebKitGTK/GLib（不自包含） | E2E 在 **没有** 安装 `libwebkit2gtk-4.1-0`、`libgtk-3-0` 的容器里启动；先对照跑「仅 Wails 式打包、不做重定位」的包，必须失败，证明测试能区分 |
| F2 | WebKit 辅助进程从宿主 `/usr/lib` 启动，或找不到而 WebView 空白 | 启动后遍历 `/proc/*/exe`，要求 `WebKitWebProcess`、`WebKitNetworkProcess` 的可执行文件位于 AppImage 挂载内；前端就绪由 evidence 的 `panelReady` 证明，不看截图 |
| F3 | 打包进了 `libwayland-client`、`libEGL`、`libGL*`、`libdrm`、Mesa 等宿主驱动栈库 | 解包后的文件清单检查（排除列表即策略）；`check-linux-bundles.py` 的同一断言在本包上执行 |
| F4 | `GIO_MODULE_DIR` 缺失，自带 GLib 加载宿主 GIO 模块（gvfs/dconf）崩溃 | 干净容器没有宿主 GIO 模块；另在带 gvfs 的 17c2 容器里再跑一遍启动，日志不得含 GIO/EGL/loader 错误 |
| F5 | daemon 不是真实 release 产物或与 GUI 版本不匹配（GUI 对 daemon 版本做握手） | 记录来源证据；启动后 `daemon.conn` 的 pid 的 `/proc/<pid>/exe` 位于挂载内，其 SHA-256 等于 `build-evidence.txt`；GUI 到 daemon 的 HTTP/WS 握手成功（`panelReady`） |
| F6 | `cd "$APPDIR"` 破坏相对路径参数 | GUI 不消费相对路径的命令行参数（`%U` 来自桌面入口）；`UC_APPIMAGE_ORIGINAL_CWD` 保留原目录。本片只确认现有参数处理没有相对路径依赖，deep link/文件关联不在范围 |
| F7 | 自启动条目指向临时挂载路径，或旧 Tauri 条目与新条目并存 | 真实 AppImage 内 `update_autostart(true)`：`Exec=` 必须等于 `$APPIMAGE` 且不在 `/tmp/.mount_*`；预置 Tauri 风格旧条目被清除；禁用后条目消失 |
| F8 | 更新验证链：未受信任签名被安装 | 复用 17c3 的 fixture 密钥：不受信任签名的下载必须被拒绝，AppImage 文件字节不变 |
| F9 | 更新后文件被替换但重启仍运行旧映像，或旧 daemon 残留 | 更新后新进程的 `/proc/<pid>/exe` 在新映像挂载内、`$APPIMAGE` 文件的 SHA-256 等于 v2、旧 daemon pid 已退出 |
| F10 | 重建 deb/rpm 丢失 17c2 加入的 `libgtk-layer-shell` 依赖，或 rpm 触发 build-id 冲突 | `dpkg-deb -I` / `rpm -qpR` 读取依赖；`rpmbuild` 无 build-id 报错并检查 `rpm -qp --list` |
| F11 | 把 e2e 标签二进制的 E2E 结果说成 release 构建的证明 | 打包 E2E 用 `gtk3,e2e` 二进制装入同一套 AppImage 管线；release 标签包另做 **无控制面** 的启动冒烟（窗口出现、daemon 存活、进程持续存活、日志无致命错误），两者在文档中分开陈述 |
| F12 | 用合成文本或 stub 当 AppImage/daemon | 所有 E2E 输入是 `package_linux.py` 的真实输出；daemon 的 ELF 检查与 SHA 对照 build 证据；脚本拒绝 `--packaging-check-fixture` 产物 |

## E2E 设计

镜像：在 `ubuntu:24.04` 上 **只** 安装 Xvfb、xauth、D-Bus、xdotool、`libgl1`、`libegl1`、`libgl1-mesa-dri`、`libx11-6`、`libwayland-client0`、字体与 `libfuse2`，不装 GTK/WebKit。该镜像按 AppImage 规范属于「宿主驱动栈」，与策略文档一致。构建镜像沿用 `uc-gui-go-linux-build:17c2`。

脚本 `apps/gui-go/e2e/linux_appimage_run.py`（运行在干净镜像中，复用 `linux_xvfb_run.py` 的 `Gui` 控制文件通道）：

1. `clean-host`：`ldconfig -p` 中没有 `libwebkit2gtk-4.1`、`libgtk-3`。
2. `launch`：用隔离临时 HOME、`UC_PORTABLE=1`、file keystore、`UC_DISABLE_SYSTEM_CLIPBOARD=1` 启动 AppImage；断言 `panelReady`、辅助进程与 daemon 的映像归属、daemon SHA。
3. `autostart`：F7 的三项断言。
4. `update-bad` / `update-good`：F8、F9；v2 AppImage 与 v1 由同一管线构建，v2 多一个 e2e 专用标记文件，签名用 `e2e/updatetool` 的隔离 fixture 私钥，feed 是本地 HTTP 服务。
5. `negative-control`：对照包（无重定位）在同一环境必须启动失败。

产物目录：`/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c4/`，仓库只索引。

## 明确不证明的边界

- **官方签名发布验证**：没有发布私钥；更新机制用隔离 fixture 私钥验证，不等于官方签名发布验证。
- **真实发布服务端与 FlareRelease**：feed 是本地 HTTP 服务。
- **原生桌面**：容器内只有 Xvfb，无 GNOME/KDE/Hyprland、无 portal、无托盘宿主、无通知守护进程；GPU 加速、真实 Wayland 会话不在范围。
- **amd64**：见「验证结果」；没有原生 x86_64 Linux 主机时，只有能真实构建并运行的部分才记为已证明。
- **release 标签 AppImage 的 UI 行为**：release 构建没有测试控制面，只有冒烟级证据。
- **AppImage 内的 Wayland / Layer Shell**：由 `GDK_BACKEND=x11` 排除，是兼容选择而非已验证能力。
