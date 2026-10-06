# Go GUI 的 Linux AppImage 宿主辅助程序（切片 17c10）

本文是 17c10 的契约：宿主辅助程序（host helper）在 AppRun 环境下的行为。库的宿主/捆绑总策略见 [linux-appimage-library-policy.md](linux-appimage-library-policy.md)，AppImage 设计见 [gui-go-linux-appimage.md](gui-go-linux-appimage.md)，运行时依赖见 [gui-go-linux-appimage-runtime-deps.md](gui-go-linux-appimage-runtime-deps.md)。

## 写作顺序（如实记录）

本文 **不是** 先于实现写成的。实际顺序：先清点产品里的辅助程序调用并读 Wails 源码；先写 E2E 与镜像、跑出基线（红）并保留；然后写了修复代码（工作区未提交）；被协调者指出缺少文档后才补写本文。失败方式 F1–F6 是在基线运行之前就已写进 E2E 的断言（`apps/gui-go/e2e/linux_appimage_helpers_run.py`），本文把它们补成书面契约。后续提交顺序以 git 历史为准：测试与契约（基线红）先于修复提交。

## 问题

AppRun 把 `$APPDIR/usr/lib` 放进 `LD_LIBRARY_PATH`，linuxdeploy 的 GTK 钩子（Wails 固定版内嵌的 `linuxdeploy-plugin-gtk.sh`）还导出 `XDG_DATA_DIRS`、`GIO_MODULE_DIR`、`GTK_*`、`GSETTINGS_SCHEMA_DIR`、`GI_TYPELIB_PATH`、`GDK_PIXBUF_MODULE_FILE`、`GDK_BACKEND=x11`、`GTK_THEME`，AppRun 还把工作目录切到挂载内的 `$APPDIR/usr`。GUI 以 `exec.Command` 或 Wails 的 `Browser` 启动宿主程序时，这些全部被继承。17c7 在 Fedora 上已发现同一机制对 `dbus-launch` 的破坏（当时只修了 libdbus 一个库）；其余辅助程序没有验证过。

## 产品实际启动的宿主辅助程序（清点，以代码为准）

| 调用点 | 程序 | 触发 | 平台 |
| --- | --- | --- | --- |
| `host_commands_files.go` `openWithSystem` | `xdg-open <路径>`（打开数据目录、日志目录、「在文件管理器中显示」= 其所在目录、「用外部程序打开图片」） | host 命令 `open_data_directory` / `open_logs_directory` / `reveal_path` / `open_image_externally` | Linux（macOS `open`、Windows `explorer`/`start` 不在本片） |
| 共享前端 `openUrl`（`@tauri-apps/plugin-opener`，适配器 `frontend/src/host/opener.ts`） | Wails `Browser.OpenURL` → `xdg-open <url>`（`internal/browser/browser_other.go`） | 发布说明链接、链接预览、更新窗口的发布页等 | Linux |
| `install_kind_linux.go` | `dpkg-query -S`、`rpm -qf` | 仅当可执行文件在 `/usr`、`/opt` 等且 `APPIMAGE` 未设置 | deb/rpm 安装，**AppImage 不触发**，不在本片 |
| `lifecycle.go`、`host_install_linux.go` | 本 AppImage / 本可执行文件自己（重启、更新后拉起） | 重启 | 必须保持完整环境，不是宿主辅助程序 |
| `packages/desktop-host-go/daemonproc` | 随包 `uniclipd` | 启动 | 同上，依赖包内库 |
| Wails `Dialog`、`Notifications`、托盘、`Autostart` | 进程内（GTK、D-Bus、写 `.desktop` 文件） | - | 不启动宿主进程 |
| Rust 侧（`crates/uc-daemon-process` 等） | `netstat`/`lsof` 等进程元数据 | 守护进程 | 未在 Linux AppImage 上验证，见「明确不证明」 |

Wails 固定版（beta.28）能力核查：`app.Browser.OpenURL/OpenFile` 与 `app.Env.OpenFileManager`（`fileexplorer_linux.go`：D-Bus FileManager1 `ShowItems`，退回 `xdg-open`）都在 Wails 进程内用继承的环境启动子进程，`internal/browser` 的 `openCmd` 是包内变量，**没有注入环境的公开 API**；所以环境问题无法在 Wails API 内部解决。`OpenFileManager` 不能解决问题（同一继承环境），本片也不改变产品的「显示」语义。

## 失败方式（E2E 断言对应项）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| F1 | 宿主 `xdg-open`/`gio` 在 `LD_LIBRARY_PATH` 下加载包内的 `libgio`/`libglib`/`libpcre2`…，与宿主版本不匹配而失败（Fedora 的 GLib 2.88 对包内 2.80） | 真实 Ubuntu 24.04 与 Fedora 44 容器，真实 GUI 经 host 命令触发；strace 记录 xdg-open 子进程树的退出码与 `openat` 的库路径；受控目标（`sh` 记录脚本，系统级默认应用）必须收到产品给出的确切目标 |
| F2 | 不失败但加载了包内库（Ubuntu：同版本，碰巧可用） | 同一 strace：宿主进程的 `.so` 不得来自挂载；记录脚本自己的 `/proc/self/maps` 也不得含挂载内库 |
| F3 | 其他 AppRun 变量泄漏给辅助程序启动的程序（`GIO_MODULE_DIR`、`XDG_DATA_DIRS`、`GTK_*`…） | xdg-open 被启动时的 envp（strace）与记录脚本的 environ：不得有任何值指向挂载 |
| F4 | 修复靠删光包内依赖或让宿主偶然启动而通过 | GUI、TLS、portable 回归必须保持（17c7 的 TLS E2E、17c5 的 portable E2E 在同一包上重跑，见「验证结果」）；修复不改 AppRun、不改包内容，只改辅助程序的启动环境 |
| F5 | 产品把辅助程序失败当成功：`cmd.Start()` 不等待、不收集退出码，用户看不到失败 | 负对照：没有默认应用的类型，产品调用返回 ok，strace 显示 xdg-open 非零退出，记录脚本未收到；用来证明「到达」断言能区分到达与未到达 |
| F6 | 夹具冒充宿主行为 | 夹具与宿主行为分开：记录脚本和系统级 mimeapps 是任务夹具；`xdg-mime query default` 由宿主工具确认；宿主对照（无 GUI、宿主环境）必须到达，缺少默认应用的类型必须不到达 |
| F7 | 便携模式（portable）下 `HOME` 被 AppImage 运行时改为 `<AppImage>.home`，用户自己的默认应用（`~/.config/mimeapps.list`）对辅助程序不可见 | 第一轮红灯（`attempt2`）：记录脚本注册在用户 HOME，GUI 环境下 `gio open` 报 `Failed to find default application`。这是便携模式的既有语义，**不是** AppRun 泄漏；本片的夹具改为系统级默认应用以把两者分开，F7 作为边界记录，不修 |

## 夹具与观察器

- 镜像：`Dockerfile.17c10-ubuntu`（Ubuntu 24.04：`xdg-utils`、`libglib2.0-bin`（`gio`）、`shared-mime-info`、`strace`，基于 17c7 运行时镜像，仍无 GTK/WebKitGTK）与 `Dockerfile.17c10-fedora`（Fedora 44：`xdg-utils`、`shared-mime-info`、`strace`，`gio` 来自 `glib2`），`verify_image_17c10.sh` 失败即中止构建。
- 运行器：`linux_appimage_helpers_run.py`（`run.sh appimage-helpers-e2e`），`--desktop generic|gnome` 两种真实 xdg-open 分发（`XDG_CURRENT_DESKTOP` 未设：按 `xdg-mime` 与 desktop 文件执行；`GNOME`：`gio open`）。不设 `UC_GUI_GO_E2E_OPEN_LOG`（否则 `openerOverride` 会短路真实 xdg-open）。GUI 在 `strace -f -u uc`（root 追踪，非 root 追踪会使内核忽略 `fusermount` 的 setuid 位，第一轮 `attempt1`）下运行，只观察 `execve`/`clone`/`openat`。
- URL 链路：E2E 构建的 quick-panel 页面暴露 `window.__ucE2eOpenUrl`，调用共享前端同一个 `openUrl`（`frontend/src/e2e-secondary.ts`，仅 `VITE_GUI_GO_E2E=1`）。
- 直接重放：对运行中 GUI 的真实 `/proc/<pid>/environ` 重放 `xdg-open`，并逐项还原变量（`restore-LD_LIBRARY_PATH` 等）做归因，`LD_DEBUG=libs` 取加载器自己的说法。重放只证明机制，不替代 GUI 链路的 strace 证据。

## 修复（只在基线红之后）

见「验证结果」。形状：辅助程序启动时的环境去掉 AppImage 条目（`LD_LIBRARY_PATH` 里挂载内的项、钩子在 `XDG_DATA_DIRS` 前缀、所有指向挂载的单值变量），工作目录改为用户主目录；GUI 自身、守护进程、重启保持完整环境。Linux 的链接打开因 Wails `Browser.OpenURL` 无环境注入点而改走 host 命令 `open_url`（其他平台仍调用 Wails 的 `Browser.OpenURL`）。

## 明确不证明

- 沙箱里的处理程序是 `sh` 记录脚本，**不是** 真实浏览器或文件管理器；真实 Firefox/Chromium/Nautilus/Dolphin 在被打开后如何受 `GDK_BACKEND=x11`、`GTK_THEME` 的影响未验证（修复也不去掉这两个变量，因为无法区分它们是用户设置还是钩子设置，没有钩子前的备份）。
- 真实桌面会话、portal（`xdg-desktop-portal`）、Wayland、GPU、原生 amd64（容器是 arm64）、更多发行版。
- `APPIMAGE`/`ARGV0`/`OWD` 不指向挂载，修复不去掉它们；宿主默认处理程序本身若是另一个 AppImage 的交互未验证。
- macOS/Windows 的辅助程序路径、deb/rpm 的 `dpkg-query`/`rpm`。
- Tauri 包的 libdbus/helper 对照：见「验证结果」。
