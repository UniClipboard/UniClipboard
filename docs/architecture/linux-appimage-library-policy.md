# Linux AppImage 库打包策略

AppImage 自带 GTK、WebKitGTK 与 GLib，但与宿主 GPU 驱动栈（Mesa、libglvnd）运行在同一进程内。凡是宿主驱动会链接的库，只能来自宿主，不能由 AppImage 自带；否则自带副本会遮蔽宿主副本，驱动加载失败。

本文记录 [issue #1612](https://github.com/UniClipboard/UniClipboard/issues/1612)（PikaOS 4 上 AppImage 启动崩溃）的根因、修复与验证范围。

## 症状

| 现象 | 来源 |
| --- | --- |
| `libgvfscommon.so: undefined symbol: g_task_set_static_name`、`Failed to load module: .../libgvfsdbus.so`，随后段错误 | 自带 GLib 加载了宿主 GIO 模块 |
| 窗口空白；日志出现 `Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...`；用户设置 `LD_PRELOAD=<宿主 libwayland-client.so.0>` 后恢复 | 自带 `libwayland-client` 遮蔽宿主副本 |

两者互相独立，必须分别修复。

## 根因

### 1. 自带的 `libwayland-client` 遮蔽宿主副本

1. Tauri CLI 2.11.x 的 AppImage 打包器在构建时下载未固定版本的 `linuxdeploy`（`tauri-apps/binary-releases` 的 `linuxdeploy` 发布，提交 `4b24a49`，2024-07）。它的排除列表不含 `libwayland-client.so.0`，于是把构建镜像（Debian bookworm，libwayland 1.21）里的副本打进 `usr/lib/`。较新的 linuxdeploy（`07333c6`，Tauri 开发分支已采用）把该库列入排除列表。
2. `AppRun.wrapped` 把 `$APPDIR/usr/lib` 放在 `LD_LIBRARY_PATH` 最前，GTK 与 WebKit 的所有进程都绑定到自带副本。
3. 宿主 Mesa 26 的 `libEGL_mesa.so.0` 需要 `wl_fixes_interface`、`wl_display_create_queue_with_name`、`wl_display_dispatch_queue_timeout`，自带的 1.21 没有这些符号，libglvnd 加载该厂商库失败，WebKit 的 EGL 初始化失败。

在 Arch Linux ARM（Mesa 26.2.3，libwayland 1.26）上表现为 WebKit 进程中止与空白窗口；在 Debian sid x86_64 容器（Mesa 26.2.4，libwayland 1.26）上同样触发。PikaOS 4 报告的是段错误，推断为同一加载失败的另一种表现，**尚未在 PikaOS 实机上验证**。

### 2. GIO 模块 ABI 混用

linuxdeploy 的 GTK 插件只设置 `GIO_EXTRA_MODULES`，它是追加语义，自带 GLib 仍会扫描编译时写入的宿主目录（`/usr/lib/x86_64-linux-gnu/gio/modules`），加载与自带 GLib 2.74 不兼容的 gvfs、libproxy、dconf 模块。该问题已在 0.19.3 修复（`7748de5e`，报告者在 PikaOS 上确认 GVFS/GIO 错误消失），但 v1 系列分支（v1.0.0、v1.0.1 与 main）没有带上这个修复。

## 修复

| 改动 | 位置 |
| --- | --- |
| 打包前把 linuxdeploy 固定为 `linuxdeploy-07333c6`，校验 SHA-256 后放入 Tauri 工具缓存，使其排除 `libwayland-client` | `scripts/linux-appimage-tools.mjs`，由 `scripts/prepare-linux-bundle.mjs`（`beforeBundleCommand`）调用 |
| 发布产物检查：AppImage 内不得出现 `libwayland-client.so*` | `scripts/check-linux-bundles.py` |
| AppImage 启动时把 `GIO_MODULE_DIR` 设为包内已校验的模块目录，取代编译时的宿主目录 | `crates/uc-tauri/src/process_environment.rs` |

`libwayland-cursor`、`libwayland-egl`、`libwayland-server` 不在排除列表中，继续自带；它们只依赖 `libwayland-client` 的稳定接口，缺少宿主 `libwayland-client` 时 GTK 本身无法加载，这是所有 AppImage 沿用的排除列表假设。

不使用无条件的 `LD_PRELOAD`，也不写死任何用户路径。

取舍：`libwayland-client` 从此依赖宿主提供。宿主没有安装它的极简 X11 系统无法启动 AppImage；这是 linuxdeploy 排除列表对所有 AppImage 做的同一假设，GTK 桌面系统都满足。

### 移除计划

Tauri CLI 升级到默认使用不早于 `07333c6` 的 linuxdeploy 后，删除 `scripts/linux-appimage-tools.mjs` 及其调用与测试；`check-linux-bundles.py` 中的断言保留。

### Go GUI 的补充（17c7）

Go GUI 的 AppImage（`apps/gui-go`）不走上面的 Tauri 路径，其库边界、GIO TLS 模块与 `libdbus-1` 的决定记录在 [gui-go-linux-appimage-runtime-deps.md](gui-go-linux-appimage-runtime-deps.md)：宿主拥有 libglvnd 全家与 `libdbus-1`；包内只带与捆绑 GLib 同源的 `libgiognutls.so`。Tauri 包是否有同样的 `libdbus` 遮蔽问题没有检查，仍 OPEN。

## 验证

可复跑脚本：`scripts/linux-appimage-smoke.sh`。它在隔离的 HOME、D-Bus、Secret Service 和嵌套合成器（或 Xvfb）里启动 AppImage，要求窗口出现、前端上报就绪（日志 `Main window revealed`）、进程持续存活，且日志没有 EGL、GIO 或加载器错误；同时保存截图、环境信息与结果。通过条件不依赖截图，截图只作为人工核对材料。

```bash
# Linux 桌面会话中，嵌套 Hyprland（需要 Hyprland、grim）
WAYLAND_DISPLAY=/run/user/$UID/wayland-1 scripts/linux-appimage-smoke.sh --seconds 80 --artifacts runs/pinned UniClipboard_x.y.z_aarch64.AppImage

# 真实 X server：无需 Wayland 合成器
scripts/linux-appimage-smoke.sh --display xvfb --artifacts runs/xvfb UniClipboard_x.y.z_amd64.AppImage
```

**（Tauri 的 AppImage；Go 的 AppImage 自 17c13 起不再如此，见 `gui-go-linux-appimage-native-wayland.md`）** AppRun 钩子强制 `GDK_BACKEND=x11`，因此窗口始终经由 X server（嵌套 Hyprland 下是 Xwayland），GDK 原生 Wayland 后端不在覆盖范围内。`--session wayland|x11` 的区别只是应用能否看到 `WAYLAND_DISPLAY`，它影响守护进程的剪贴板协议选择和 Mesa 的平台探测。

### 对照材料的来源

红绿对照包由真实的 tauri-cli 2.11.1 打包器（`tauri bundle --bundles appimage`）在 `ghcr.io/uniclipboard/build-bookworm` 容器里生成，二进制取自官方 v1.0.1 发布包，只改变 linuxdeploy 这一项。这是 **混合包**：文件名带 1.1.0-alpha.2，内部是 v1.0.1 的 `uniclipboard` 与 `uniclipd`，仅用于验证打包机制，不是 main 的产品构建。aarch64 上，未固定时得到的 AppImage 与官方 v1.0.1 发布版大小一致（118667784 字节）；固定后与之相比只少了 `libwayland-client.so.0` 及其版权文件。

### 结果

| 环境 | 包 | 结果 |
| --- | --- | --- |
| Arch Linux ARM aarch64，Mesa 26.2.3，libwayland 1.26，嵌套 Hyprland（Xwayland），`WAYLAND_DISPLAY` 可见 / 不可见各一次 | 未固定 | 失败：`EGL_BAD_PARAMETER` 中止，前端就绪超时，窗口空白 |
| 同上 | 固定 | 通过：界面渲染（截图核对），前端就绪，守护进程 WebSocket 就绪，持续运行 90 秒（Wayland 会话另跑 300 秒） |
| Debian sid amd64（QEMU 用户态模拟），Mesa 26.2.4，GLib 2.90，gvfs 1.62，Xvfb | 官方 v1.0.1 | 失败：GIO 符号错误与 EGL 中止同时出现，前端就绪超时 |
| 同上 | 仅删除 `libwayland-client` | EGL 错误消失，前端就绪；GIO 错误仍在 |
| 同上 | 删除 `libwayland-client` 并设置 `GIO_MODULE_DIR` | 无致命日志，前端就绪。截图是白色窗口，模拟环境下不能据此确认界面已渲染 |
| 测试 | | `scripts/tests/test_check_linux_bundles.py`、`scripts/__tests__/linux-appimage-tools.test.ts`、`cargo test -p uc-tauri --lib process_environment`（aarch64 Linux） |

直接证据：在上述两个主机上，用包内 `libwayland-client` 加载宿主 `libEGL_mesa.so.0` 都得到 `undefined symbol: wl_fixes_interface`，用宿主副本则加载成功。

### 未验证

- PikaOS 4 实机（x86_64，glibc 2.44，GLib 2.89.3）：没有该环境，Debian sid 容器是最接近的替代。PikaOS 上的段错误是否就是同一加载失败，只是推断。
- x86_64 的固定版 linuxdeploy 实际打包：本地没有原生 x86_64 Linux 主机，QEMU 用户态无法运行 linuxdeploy AppImage。已核对固定校验和，并确认该二进制的排除列表含 `libwayland-client.so.0`（未固定的旧版不含）。实际打包由 CI 的 x86_64 构建完成，`check-linux-bundles.py` 会在发布流程里拒绝仍含该库的产物。
- 固定版 AppImage 在真实 X server（Xvfb）上运行：只测了等价内容的删库变体。
- 带 GIO 修复的 Rust 二进制在 AppImage 内的端到端运行：`GIO_MODULE_DIR` 的推导由单元测试覆盖，机制由手动设置同一变量验证，同一代码已在 0.19.3 的 PikaOS 实机上确认有效；完整构建未在本地执行。
- 模拟环境里的 amd64 守护进程 WebSocket 未就绪：QEMU 用户态下守护进程已监听，但 PID 存活检查失败，属于模拟器限制。
