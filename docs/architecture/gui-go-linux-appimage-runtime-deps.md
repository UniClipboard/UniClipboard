# Go GUI 的 Linux AppImage 运行时动态依赖（切片 17c7）

本文是 17c7 的契约：先于实现列出失败方式与验收，结果在文末「验证结果」补录。库的宿主/捆绑总策略沿用 [linux-appimage-library-policy.md](linux-appimage-library-policy.md)，AppImage 打包设计见 [gui-go-linux-appimage.md](gui-go-linux-appimage.md)。

## 问题

17c4 的文档已记录：`libGLESv2.so.2` 被 `dlopen`，不在 linuxdeploy 排除列表，打包检查不覆盖它；`dlopen` 依赖从未被系统枚举过。17c7 还要回答一个更严重的问题：AppRun 把 `GIO_MODULE_DIR` 指向包内 **刻意留空** 的 `usr/lib/gio/modules`（避免捆绑 GLib 加载宿主 gvfs/dconf 模块，见库策略「GIO 模块 ABI 混用」）。libsoup 3 的 TLS 由 GIO 的 TLS 后端模块（glib-networking 的 `libgiognutls.so`）提供，空目录意味着 WebView 的 HTTPS 没有后端。这是包自身缺陷，不是「宿主缺 glib-networking」：宿主模块恰好被 `GIO_MODULE_DIR` 屏蔽。

## 功能 → 固定版 API / 现成库 → 集成 → 缺口 → 最小适配 → E2E

| 功能 | 固定版 / 已有库 | 集成状态 | 实际缺口 | 最小适配 | E2E |
| --- | --- | --- | --- | --- | --- |
| GTK/GDK-pixbuf/GSettings 路径 | Wails beta.28 内嵌 `linuxdeploy-plugin-gtk.sh` | 已采用（17c4） | 无 | 无 | 既有 |
| GIO 模块（TLS） | 该插件只复制 `libgio` 库，不复制 `giomoduledir` 下任何模块（脚本里只有 `gio_libdir`） | 部分：库在，模块不在 | `usr/lib/gio/modules` 为空，HTTPS 无后端 | 把构建镜像里与捆绑 GLib 同源的 `glib-networking` 模块 `libgiognutls.so` 复制进已存在的模块目录，并生成 `giomodule.cache` | 17c7 主 E2E |
| 同类先例 | 旧 Tauri 外壳（已退役）的进程环境初始化校验的就是挂载内的 `gio/modules` 目录，该目录在 Tauri 包里有内容 | 参照 | 无 | 无 | 无 |
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
| `libgtk-layer-shell.so.0` | 由 GUI 自己 `dlopen` | 17c13 起 AppImage 自带（来源与闭包见 `gui-go-linux-appimage-native-wayland.md`）；deb/rpm 声明依赖 | 17c2 已有缺失时的降级，17c13 的容器缺库对照再次验证 |
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

证据目录 `/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c7/`（仓库只索引）。全部是容器内 arm64：Docker、Xvfb、非 root 用户、portable 模式（无 Secret Service）、真实 AppImage、真实 release daemon（SHA-256 `ea0f0bcb…f6c6`，输入与 17c5 构建证据一致，未重建）、e2e 标签 GUI 的真实 WebView。

### 红灯（修复前）

对 17c6 的现成包（`E2E-UniClipboard_1.1.1_arm64.AppImage`，`usr/lib/gio/modules` 为空）跑 HTTPS 场景（`red1-17c6-ubuntu`）：受信 CA 的服务端也被 WebView 拒绝（`TypeError: Load failed`），服务端没有收到请求行，只看到握手中断（`UNEXPECTED_EOF`）。`libGLESv2.so.2` 同时被确认为由宿主映射（GUI 与 `WebKitWebProcess` 的 `/proc/<pid>/maps`）。静态审计 `audit_dlopen.py` 对该包得到 14 个「字符串里出现、但既非 NEEDED 也未随包」的 soname（libglvnd 全家、`libgtk-layer-shell`、`libnss_mdns*` 和几个可选 dlopen），只是线索：`libGLESv2` 由 libepoxy 与 WebKit 加载，属于 libglvnd，包内不得带。

### 修复

1. `package_linux.py` 的 `deploy_gio_modules`：把构建镜像里 `glib-networking`（`dpkg -S` 校验归属）的 `libgiognutls.so` 复制进 `usr/lib/gio/modules`，与捆绑的 GLib 2.80 同源同 ABI；校验其 NEEDED 全在 AppDir 或为 libc 家族；manifest 记录来源、包版本与 SHA-256。不带 gvfs、dconf、libproxy、gnome-proxy。
2. `libdbus-1` 从 AppDir 移除（见下文 F10）；`HOST_ONLY_LIBS` 加入 `libGLESv1_CM`、`libGLESv2`、`libOpenGL`、`libdbus-1`，打包检查在产物里发现它们就失败。linuxdeploy 的 `--exclude-library` 对主流程生效，但 GTK 插件自己的部署轮次不读它（日志里先出现 `Skipping … blacklisted`，随后插件又部署），所以在 linuxdeploy 之后显式移除，并由检查兜底。
3. AppRun 注释更新（`GIO_MODULE_DIR` 现在指向有内容的目录）。

### 最终运行（干净提交 `53aa76756`，目录 `final-53aa76756`，20:34–20:45 UTC 2026-10-06（约 11 分钟））与补验

流水线整体 `pipeline-exit=1`，原因只有一个：`content-negtls=1`，那是 **测试入口缺陷**（`run.sh appimage-content-check` 没把 `UC_CONTENT_CHECK_ARGS` 传进容器，对照包被按「应有 TLS 模块」检查），不是产品缺陷；原始失败保留。同一运行还暴露：`run.sh appimage-tls-e2e` 的 `main()` 里有早退 `return`，跳过了末尾 `sys.exit`，导致 `control-17c6-fedora` 虽有失败断言却退出码 0。两个缺陷都在 harness 修复后提交 `6bbc91dfe`，并以 `run_17c7_supplement.sh` 在 **同一批保留的产品包与镜像** 上补验（`supplement-6bbc91dfe`，先 `shasum -c` 确认原包字节未变，没有重新构建任何产品）。

| 项 | 结果（读自各 JSON） |
| --- | --- |
| 内容检查 `content-v1` | 8/8：模块目录恰为 `libgiognutls.so`，字节等于 manifest（`glib-networking 2.80.0-1build1`，SHA-256 `d5a50688…b2a7`），NEEDED 全在包内，无 GL/EGL/GLES/libdrm/libgbm/libwayland-client/libdbus，MIME 缓存仍在，manifest 记录 libdbus 移除 |
| 内容检查 `content-negtls`（对照包） | 5/5：模块目录存在且为空 |
| `tls-ubuntu`（Ubuntu 24.04 宿主，无 GTK/WebKit） | 19/19 |
| `tls-fedora`（Fedora 44 宿主，无 GTK/WebKit；宿主 GLib 2.88、自带 glib-networking 模块） | 19/19 |
| 无 TLS 模块对照包，双发行版 | 各 13/13：受信 HTTPS 也被拒，服务端无请求、无证书告警 |
| 17c6 旧包在 Ubuntu | 13/13（旧包 HTTPS 被拒；与 `red1-17c6-ubuntu` 一致） |
| 17c6 旧包在 Fedora | 按预期失败：5 项检查，仅 `T1 daemon 启动` 失败；**这不是 TLS 证据**，是 F10（libdbus）的证据。补验里 wrapper 退出码为 1，并有脚本断言「失败阶段只能是 T1」 |
| 回归（最终运行，同一 v1 包） | portable 64/64，非 portable 完整 34/34（含真实更新与重启），negative 2/2，release 冒烟 5/5，runtime 身份（runtime 字节、manifest）通过 |

`tls-*` 的 19 项覆盖：干净宿主；测试 CA 只装进容器自己的信任机制（Ubuntu `update-ca-certificates`，Fedora `update-ca-trust`）；**宿主信任控制**（用宿主信任库的客户端接受受信服务端、拒绝不可信服务端，才让 fixture 可用）；真实 WebView 经 `panel-js` 对受信服务端 `fetch`，读到随机令牌，服务端收到的请求路径带本次随机 nonce、UA 是 Wails 页面的 `…AppleWebKit/605.1.15 … wails.io/605.1.15`、来源 `wails://localhost`；不可信服务端被拒，服务端无请求行且握手失败；**T5 因果对照**：把不可信 CA 装进宿主信任库并重启 GUI（GLib 的默认 TLS 数据库每进程读一次），同一服务端、同一页面的请求这次成功，请求行带新 nonce，因此之前的拒绝可归因于证书信任，而不是服务端、CORS 或可达性；映射断言。

映射断言（GUI、`WebKitWebProcess`、`WebKitNetworkProcess`、`WebKitGPUProcess` 的 `/proc/<pid>/maps`）：每个 `.so` 要么在挂载内，要么是被分类的宿主库；包内带的 soname 不得从宿主加载；`WebKitNetworkProcess` 必须映射挂载内的 `libgiognutls.so` 与 `libgnutls.so.30`（两发行版都通过，`GIO_MODULE_DIR` 指向挂载内）。Fedora 宿主自己的 `libgiognutls.so`（为宿主 GLib 2.88 构建）在场、没有被使用。`libGLESv2.so.2` 在两个发行版上都由宿主的 libglvnd 提供（映射路径 `/usr/lib*/…/libGLESv2.so.2*`），没有进包。

### 实际遇到的失败与根因（原始日志都保留）

- **F10 libdbus（包缺陷，Ubuntu 上被掩盖）**：Fedora 上 GUI 静默 `exit 1`，没有任何输出。`strace -f`（`diag-fedora-start8/10`）显示：GUI 在没有会话总线时找 `dbus-launch`；Fedora 的 `dbus-launch`（libdbus 1.16.2）经 AppRun 的 `LD_LIBRARY_PATH` 加载了包内的旧 `libdbus-1.so.3`，报 `version 'LIBDBUS_PRIVATE_1.16.2' not found`（rc 127）。Ubuntu 宿主与包内是同一版 libdbus，所以一直碰巧可用。修复是不再随包带 `libdbus-1`（宿主的才与宿主的 dbus 助手匹配）。
- **镜像缺陷，不是产品缺陷**：Fedora 容器镜像先后缺 `dbus-x11`（无会话总线自动拉起）与有效的 `/etc/machine-id`（libdbus 报 `D-Bus library appears to be incorrectly set up`）。这些在镜像里修复；每轮镜像构建日志都独立保留（`fedora-image-build.log`…`build6`，其中 build5 因 `dbus-uuidgen` 对空文件拒绝而失败，并且之后误在旧镜像上跑了 `dev5`，被保留为失败 attempt）。最终运行在构建失败时立即中止，并记录镜像 ID、os-release 与全部包版本。Fedora 基础镜像固定为 `fedora:44@sha256:43b29f65…`。
- **`LD_DEBUG` 里的 `soup_uri_new` symbol lookup error**：Ubuntu 与 Fedora 上都出现（`diag-ubuntu-ld1`），而 Ubuntu 上 GUI 正常运行；它是 WebKit 的可恢复探测，不是原因，没有为它添加 libsoup2。
- **`gui2` 崩溃**：第二次启动后 e2e 控制面的 `shortcut-state` 在宿主 bootstrap 完成前解引用了尚未绑定的 daemon client（仅 e2e 标签代码）；runner 在第二次启动时可能读到旧 daemon 的残留 `daemon.conn` 并提前轮询。runner 改为等待新 daemon pid 与 `bootstrapped` 证据步骤。
- **T3 的告警文本不稳定**：不可信服务端有时只看到 `Connection reset by peer`，客户端的 `unknown ca` 告警被 TCP 复位抢先（`dev5-green-ubuntu`，保留）。告警文本因此只作证据记录（`untrustedAlertSeen`），断言的是「握手失败、没有请求行」，并由 T5 因果对照提供归因。
- **GnuTLS 的编译期信任文件**（`/etc/ssl/certs/ca-certificates.crt`）在 Fedora 容器里存在，所以 F3 没有在 Fedora 44 上发生；这只是观察，不是对其他发行版的保证。
- 我手写的 ELF SONAME 解析被换成 `readelf -d`（读取失败会拒绝分类，不放过）；Fedora 的 `libbz2.so.1`（宿主）与包内 `libbz2.so.1.0` 是两个不同的 soname，两者同时加载不构成遮蔽，该判断来自 `readelf` 读取的 SONAME，而不是放宽文件名。

### 复跑

```bash
# 干净提交；先 run.sh daemon-release；需要 Docker、bun、网络（镜像构建）
UC_LINUX_IMAGE=uc-gui-go-linux-build:17c2 apps/gui-go/e2e/linux/run_17c7.sh <新目录>
# 只在保留的包与镜像上复验 harness 修复：
apps/gui-go/e2e/linux/run_17c7_supplement.sh <最终运行目录> <新目录>
```

### 仍 OPEN（没有被缩小）

- 原生 amd64：本机 Docker 的 amd64 是 QEMU 模拟（`uname -m` 为 `x86_64` 并不等于原生），没有原生 x86_64 主机；amd64 的构建、嵌入与运行只能在原生主机或 CI 上证明。
- 其他发行版与 Mesa/glibc 版本：只测了 Ubuntu 24.04 与 Fedora 44；Arch、openSUSE、Debian sid、Alpine/musl 未测。
- 系统代理（libproxy、gnome-proxy）：没有带，未验证。
- 真实桌面、GPU 驱动、Wayland、portal、托盘、通知、休眠、焦点；deb/rpm 原生安装。
- `AppRun` 的 `LD_LIBRARY_PATH` 仍会进入 GUI 启动的所有宿主助手进程（`xdg-open`、`notify-send` 等）；libdbus 是这一类问题里已证实的一例，其他助手没有逐个审计。
- 真实登录会话的自启动、注销后登录与更新后条目；官方签名发布；Windows/macOS 全部事项。

## 动态观测库存的内容断言

`appimage_content_check.py` 的 `--runtime-inventory <json>` 接收真实运行时收集的库存。
检查器首先要求库存的 `imageSha256` 与当前打包清单中的 AppImage 哈希一致，再逐项验证：

- `bundled`：`bundleRelativePath` 指向 AppDir 内的文件，字节等于观测记录的 `sha256`。
- `host-owned` / `host-staged`：观测文件名和 ELF `soname` 都没有重复捆绑。
- 未知分类、空库存或缺少覆盖说明均失败。

输入的顶层字段为 `imageSha256`、`scope`、`libraries`；每项至少包含 `name`、
`classification`、`soname`，捆绑项另含 `bundleRelativePath` 与 `sha256`。
`scope` 必须说明实际执行的场景及未覆盖的加载路径。
库存来自 GUI、WebKit 与 daemon 的 `/proc/<pid>/maps` 或加载器跟踪；静态扫描不能替代它。
有限运行只证明已观测的加载，不证明全部潜在 `dlopen`、媒体插件或 GPU 组合均已覆盖。

```bash
python3 apps/gui-go/e2e/linux/appimage_content_check.py \
  squashfs-root package-manifest.json content-check.json \
  --runtime-inventory runtime-inventory.json
```
