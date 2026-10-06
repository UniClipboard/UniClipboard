# Go GUI 的 Linux AppImage 运行时动态依赖（切片 17c7）

本文是 17c7 的契约：先于实现列出失败方式与验收，结果在文末「验证结果」补录。库的宿主/捆绑总策略沿用 [linux-appimage-library-policy.md](linux-appimage-library-policy.md)，AppImage 打包设计见 [gui-go-linux-appimage.md](gui-go-linux-appimage.md)。

## 问题

17c4 的文档已记录：`libGLESv2.so.2` 被 `dlopen`，不在 linuxdeploy 排除列表，打包检查不覆盖它；`dlopen` 依赖从未被系统枚举过。17c7 还要回答一个更严重的问题：AppRun 把 `GIO_MODULE_DIR` 指向包内 **刻意留空** 的 `usr/lib/gio/modules`（避免捆绑 GLib 加载宿主 gvfs/dconf 模块，见库策略「GIO 模块 ABI 混用」）。libsoup 3 的 TLS 由 GIO 的 TLS 后端模块（glib-networking 的 `libgiognutls.so`）提供，空目录意味着 WebView 的 HTTPS 没有后端。这是包自身缺陷，不是「宿主缺 glib-networking」：宿主模块恰好被 `GIO_MODULE_DIR` 屏蔽。

## 功能 → 固定版 API / 现成库 → 集成 → 缺口 → 最小适配 → E2E

| 功能 | 固定版 / 已有库 | 集成状态 | 实际缺口 | 最小适配 | E2E |
| --- | --- | --- | --- | --- | --- |
| GTK/GDK-pixbuf/GSettings 路径 | Wails beta.28 内嵌 `linuxdeploy-plugin-gtk.sh` | 已采用（17c4） | 无 | 无 | 既有 |
| GIO 模块（TLS） | 该插件只复制 `libgio` 库，不复制 `giomoduledir` 下任何模块（脚本里只有 `gio_libdir`） | 部分：库在，模块不在 | `usr/lib/gio/modules` 为空，HTTPS 无后端 | 把构建镜像里与捆绑 GLib 同源的 `glib-networking` 模块 `libgiognutls.so` 复制进已存在的模块目录，并生成 `giomodule.cache` | 17c7 主 E2E |
| 同类先例 | 仓内 Tauri 路径 `crates/uc-tauri/src/process_environment.rs` 校验的就是挂载内的 `gio/modules` 目录，该目录在 Tauri 包里有内容 | 参照 | 无 | 无 | 无 |
| GL/EGL 驱动栈 | linuxdeploy 排除列表（libEGL、libGL、libdrm、libgbm、libwayland-client 等） | 宿主提供 | `libGLESv2`、`libGLESv1_CM`、`libGLX`、`libOpenGL` 不在排除列表，但同属 libglvnd，包内不得带；此前只靠一次失败发现 | 打包检查的禁止列表显式加入这四个，运行时映射断言要求它们来自宿主 | 映射断言 |
| 信任链（CA） | GnuTLS 的编译期默认信任文件 | 取决于宿主 | 见 F3 | 不打包 CA，不改验证；只记录事实 | 第二发行版 |
| 自启动、对话框、更新、runtime 固定 | `app.Autostart`、`app.Dialog`、17c6 | 不变 | 无 | 无 | 回归 |

## 边界表（谁提供什么）

| 类别 | 例子 | 来源 | 理由 |
| --- | --- | --- | --- |
| 应用库 | GTK3、WebKitGTK、JavaScriptCore、libsoup 3、GLib/GIO/GObject、GnuTLS、GDK-pixbuf 及其加载器 | AppImage | 宿主不保证存在或版本一致 |
| GIO TLS 模块 | `libgiognutls.so` | AppImage（本片新增） | 必须与捆绑的 GLib/GnuTLS 同源同 ABI；宿主模块被 `GIO_MODULE_DIR` 屏蔽，且是为宿主 GLib 构建的 |
| 其他 GIO 模块 | gvfs、dconf、libproxy、gnome-proxy | 不使用 | 17c4 已证实宿主副本会崩溃；本片不带 libproxy / gnome-proxy（系统代理发现不在范围，见「明确不证明」） |
| GPU 驱动栈 | libEGL、libGL、libGLX、libOpenGL、libGLESv1_CM、libGLESv2、Mesa 驱动、libdrm、libgbm | 宿主 | 自带会遮蔽宿主驱动（库策略第 1 条根因） |
| 其他 `dlopen` 可选项 | `libcap.so`、`libcryptsetup`、`libdebuginfod`、`libsepol`、`libnss_mdns*` | 不加载即可 | 静态审计 `audit_dlopen.py` 只显示它们在字符串里出现，所属库（libmount、libdw、libselinux、libavahi-client）对缺失已有降级路径；是否有行为后果以运行结果为准，不据此加包 |
| `libgtk-layer-shell.so.0` | 由 GUI 自己 `dlopen` | 宿主（deb/rpm 声明依赖；AppImage 内无 Wayland 后端） | 17c2 已有缺失时的降级 |
| CA 信任存储 | 系统 CA 文件 | 宿主 | 用户必须能用自己的信任设置；不得打包 CA，不得关闭校验 |

## 失败方式（先于实现）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| F1 | 捆绑 GLib 但 GIO 模块目录为空：WebView 的 HTTPS 请求在 TLS 握手前就失败 | 对 17c6 的现成 AppImage 跑 HTTPS 场景，必须失败（红灯）；服务端没有收到任何请求行；GUI 日志给出 TLS 后端缺失类文字 |
| F2 | 为了修 F1 把 libglvnd 的库打进包，或带上宿主 GIO 模块 | 打包检查的禁止列表与模块白名单（只允许 `libgiognutls.so` 及缓存文件）；运行时映射断言 |
| F3 | 捆绑的 GnuTLS 的编译期默认信任文件路径在另一发行版不存在，合法证书也被拒 | 先从 `libgnutls.so.30` 的字符串取出该路径，再在第二发行版里 `ls` 它；在第二发行版里把测试 CA 装进 **该发行版自己的** 信任机制，请求必须成功，否则如实记录并给出可行的既有机制，不自写探测层 |
| F4 | 测试靠关闭验证、信任非隔离 CA 或打包 CA 才通过 | 无任何 TLS 开关；CA 只装进容器的宿主信任链；不可信服务端（另一个隔离 CA）必须被拒，服务端日志有握手失败（客户端未知 CA 告警），且没有请求行 |
| F5 | 进程映射的库来自宿主 `/usr` 而不是挂载 | 读 GUI、`WebKitWebProcess`、`WebKitNetworkProcess`、`WebKitGPUProcess` 的 `/proc/<pid>/maps`：每个 `.so` 映射要么在挂载内，要么在显式的宿主 allowlist；`WebKitNetworkProcess` 必须映射挂载内的 `libgiognutls.so` 与 `libgnutls.so.30` |
| F6 | 把静态字符串审计当作运行证明 | `audit_dlopen.py` 只是清单辅助（看不到按目录扫描的 GIO 模块、pixbuf 加载器、GStreamer 插件）；验收只看真实 WebView 的行为与映射 |
| F7 | 用 stub daemon、合成页面或纯 ctypes 代替真实 GUI | 同 17c4：真实 AppImage、真实 release daemon（SHA 对照 17c5 构建证据）、真实 e2e 标签 GUI 的 WebView 内 `fetch` |
| F8 | 把 arm64 容器结果说成 amd64 或真桌面 | 文档明确：本机无原生 amd64（Docker 的 amd64 是 QEMU 模拟）；容器不等于真桌面 |
| F10 | 包内捆绑 `libdbus-1`，经 AppRun 的 `LD_LIBRARY_PATH` 遮蔽宿主的 libdbus，宿主辅助进程（`dbus-launch`）加载到包内副本而失败，GUI 因无会话总线静默 `exit 1`（第二发行版 Fedora 暴露；Ubuntu 上宿主与包内版本相同，碰巧可用） | 第二发行版真实启动；`strace` 的 `execve(dbus-launch)` 与 `version LIBDBUS_PRIVATE_x not found`；包内容检查要求 AppDir 无 `libdbus-1*`，运行时映射要求 libdbus 来自宿主 |
| F11 | 把宿主环境缺陷（空 `/etc/machine-id`、无会话总线）说成包缺陷或反过来 | 因果对照：每个启动失败都用 `strace` 取到终止调用链再下结论；镜像缺陷在镜像里修（`dbus-x11`、`dbus-uuidgen`），不加宿主 GTK/WebKit |
| F9 | 修复破坏既有行为：GIO 模块 ABI 混用复发、MIME 缓存回退、portable/更新/自启动回退 | 回归：portable、non-portable full、negative control、release smoke、runtime 身份 |

## E2E 设计

1. 测试服务（runner 内，Python `ssl`）：隔离 CA 签发 `127.0.0.1` 叶证书；两个端口，一个由受信 CA 签发，一个由第二个隔离 CA 签发（宿主不信任）；响应带 `Access-Control-Allow-Origin: *`（页面与服务端跨源，`fetch` 需要它），响应正文是随机一次性令牌；服务端记录请求行与握手失败。
2. 驱动：GUI 的既有 e2e 控制文件新增一个 e2e 标签专用动词 `webview-fetch <label> <url> <report url>`，在 **主窗口真实 WebView** 内执行 `fetch`，把成功读到的正文或失败原因回报给 runner 的回环监听。release 标签没有该动词，release 冒烟不变。
3. 受信 CA 只装进容器的宿主信任链（Ubuntu：`update-ca-certificates`；Fedora：`update-ca-trust`），不进 AppImage。
4. 通过条件：受信端 WebView 读到令牌且服务端有对应请求行；不可信端 WebView 拒绝、服务端无请求行且有握手失败；四个进程的映射断言。
5. 红灯：17c6 的 v1 包跑同一场景，受信端必须失败。
6. 第二发行版：Fedora aarch64 运行镜像，宿主无 GTK/WebKitGTK，GLib 是宿主自己的版本；运行同一产物与同一场景。

## 明确不证明的边界

- 原生 amd64（本机 Docker 的 amd64 仅 QEMU，不等于原生）：OPEN，需要原生 x86_64 主机或 CI。
- 真实桌面、GPU 驱动、Wayland、portal、托盘、通知：容器内只有 Xvfb 与软件渲染。
- 系统代理发现（libproxy / gnome-proxy）：不带，未验证。
- 官方签名发布：测试服务与更新 feed 仍是隔离 fixture。

## 验证结果

（实现与运行后补录。）
