# Linux GTK4 打包迁移

2026-10-23：所有当前 Linux GUI 开发、编译与打包入口统一使用固定 Wails beta.28 的 GTK4 后端。历史 GTK3 的测量和切片验收保留为历史证据，不能证明新产物可安装或运行。迁移本身不代表触控板滚动拖影已解决。

## 依赖与兼容范围

Wails 使用 GTK 4.14 起提供的 `GdkMonitor.get_scale`，因此要求 GTK ≥ 4.14（`GtkFileDialog` 自 GTK 4.10 起提供）；WebKit 使用 `webkitgtk-6.0` ABI。公共 GUI 构建镜像改为 Ubuntu 24.04；打包产物中 ELF 的 GLIBC 符号版本需求上限为 2.39，而非宿主 glibc 的版本上限；这是明确提高的构建基线，不再承诺 Debian 12 或 Ubuntu 22.04 的原生 deb/rpm/AppImage 兼容性。最终产物中每个 ELF 的实际需求仍由 `elf_floor.py` 检查。

| 渠道 | GTK4 契约 |
| --- | --- |
| deb | `libc6 (>= 2.39)`、`libgtk-4-1 (>= 4.14)`、`libwebkitgtk-6.0-4`、`libx11-6` |
| rpm / COPR | GTK4、WebKitGTK 6.0、X11 的 soname；glibc ≥ 2.39；GTK 包版本 ≥ 4.14。COPR 与上游 RPM 使用相同能力声明，回归检查防止漂移 |
| AppImage | 使用 Wails 自带 GTK 插件的 `DEPLOY_GTK_VERSION=4`；携带 GTK4、WebKitGTK 6.0、JavaScriptCore 6.0 及 WebKit 进程助手；携带匹配构建时 GLib 的 `xdg-dbus-proxy`，宿主提供 bubblewrap；使用界面子进程的私有挂载命名空间提供编译期绝对路径 |
| AUR | `gtk4>=4.14`、`webkitgtk-6.0`、`libx11`、`libsoup3`；源码使用 `gtk4,production,release` 标签 |
| Snap | 使用 `core24`、strict、devel；GTK ≥ 4.14 由 GNOME 46 SDK/内容平台提供，应用打包 WebKitGTK 6.0；细节见 [分发渠道](gui-go-distribution-channels.md) |
| Nix | GTK4 发布使用 `wrapGAppsHook4`、`gtk4`、`webkitgtk_6_0`；更新源 pin 前校验两个 deb 的真实 SHA-256 和 GTK4 ELF 依赖 |
| Flatpak / Flathub | GNOME 50 运行时；构建时拒绝 GTK3 输入 deb。发布 URL、哈希与 AppStream 占位项仍须在提交前替换，不能视为已可发布 |

安装脚本对 Ubuntu 20.04 / 22.04、对应派生发行版和 Debian 11 / 12 选择 Snap，避免自动安装不满足原生 GTK4 依赖的 deb。其他发行版仍须满足包声明的 GTK 与 glibc 条件；Snap 在旧宿主上的实际兼容性需要验收。

GTK4 快捷面板使用普通窗口。本项目尚未实现 GTK4 Layer Shell 适配，因此新的包不声明或捆绑 GTK3 的 `libgtk-layer-shell`，Wayland 的覆盖层、独占键盘和定位不能沿用旧 GTK3 验收结论。仅用于在旧提交中重现 GTK3 Layer Shell 实验的 `run_17c*` 脚本保留其原始标签；它们不属于当前包的构建入口。

实际 CI 发现 GNOME 42 SDK 的 GTK 4.12.3 无法编译 Wails 的 `gdk_monitor_get_scale` 调用，因此 Snap 基线改为 core24/GNOME 46。core24 是包内运行时，不代表宿主必须是 Ubuntu 24.04；Ubuntu 20.04 的 snapd、内核与桌面兼容性仍须验收。AppImage 在部署结束后按宿主库策略移除 Vulkan 和 D-Bus 入口库，干净验收宿主须提供 Vulkan 加载器。

Snap 的 GNOME 平台统一提供共享库，应用仅携带平台未提供的 WebKitGTK 6.0 等依赖。最后执行的 `platform-cleanup` 部件根据实际内容平台的文件路径及 ELF SONAME 清理重复库，避免应用内较旧的 HarfBuzz、GLib 等遮蔽平台版本；不同版本的实际文件名也按 SONAME 归属清理。此阶段随后检查 WebKitGTK 6.0 的库和进程助手完整性。

AUR 安装验收在可丢弃容器中由 root 安装软件包，再由独立普通用户启动密钥环、Xvfb 和应用；不向应用授予宿主 `SYS_ADMIN`。启动前先检查普通用户的 bubblewrap 用户及网络命名空间，随后按可执行文件路径识别 WebKit 子进程，并确认真实窗口映射，避免将沙箱包装进程或仍存活的托盘宿主误当作成功启动的界面。

AppImage 的 GTK4 WebKit 沙箱会把编译期路径用作 bubblewrap 的绝对挂载目标，旧 GTK3 的相对路径替换不适用。固定 Wails 版本的 AppImage 命令没有重定位这些路径的接口，因此打包器将助手目录与 D-Bus 代理路径等长替换为 `/run/uc-webkit` 下的路径，并在包内记录唯一的运行时布局。只有界面子进程通过宿主 `/usr/bin/bwrap` 建立私有用户与挂载命名空间，将只读 AppDir 映射到该路径；原有 `/run` 子项、设备、网络、控制管道和用户文件仍可访问。驻留宿主、daemon 与更新流程不进入该命名空间，WebKit 继续在其内建立自己的进程沙箱。该机制替代旧的相对路径方案，不修改系统挂载，也不在启动时复制或重写库。界面状态中的 `uiPid` 由界面进程自身报告，不使用 bubblewrap 监控进程的 PID。

Ubuntu 托管 runner 的 AppArmor 会让未命名的 `unconfined` 容器进入限制命名空间能力的配置，普通用户的 bubblewrap 网络初始化仍会失败。AUR 与原生包 CI 在宿主加载一个仅供显式标记的测试容器使用的 `userns` 配置；保留宿主全局限制，并继续启用 WebKit 沙箱。此操作仅允许一次性的 GitHub 托管 runner 执行，Snap 严格隔离验收不使用这项测试配置。

deb/rpm 与 AppImage 验收同样使用普通用户启动实际界面。AppImage 的独立密钥环容器使用相同测试 UID，避免 D-Bus 身份不一致；宿主没有 GTK/WebKit 的检查保持有效。真实网页就绪要求前端通过宿主服务写入 `ui-page-ready`，驻留宿主的 `bootstrapped` 事件不能作为网页已加载的证明。

## 已发布产物的迁移顺序

Nix 当前仍锁定已发布的 GTK3 `1.2.0-alpha.7`，不能把这份产物改称 GTK4，也不能为尚未发布的 deb 编造校验和。配方仅为这一个准确的版本及 `gtkMajor: 3` 保留原有依赖；其他源必须声明 `gtkMajor: 4`。首个 GTK4 deb 发布后，使用 `packaging/nix/update.py <version> <SHA256SUMS.txt> <deb-directory>` 验证并更新 pin，同时删除 `legacyGtk3` 分支及对应验收兼容逻辑。CI 同时评估 GTK4 分支，默认构建继续核验真实旧 pin。

Flatpak manifest 原本使用占位 URL/哈希，目前仍不可提交发布。GTK4 构建检查会拒绝误填入的旧 GTK3 deb；必须填入真实 GTK4 发布物之后再运行 `flatpak-builder`。

## 验证与边界

运行时验收尚未完成：修复前的 CI 已构建两种架构的 Snap、deb/rpm/AppImage，但 strict Snap 在 GUI 初始化前因 HarfBuzz 的 `hb_free` 符号缺失失败，AUR 因 root 下 bubblewrap 的 `Failed RTM_NEWADDR` 失败，deb/rpm 因 root 下沙箱命名空间创建被拒绝失败，干净宿主 AppImage 因缺少 bubblewrap 失败。上面的共享库清理、普通用户验收和 AppImage 命名空间设计是这些问题的修复方案；必须以修复后 CI 的实际结果确认，不能把流程描述当成已经通过的证据。


- `scripts/tests/test_linux_gtk4_packaging.py` 使用可丢弃 ELF 和真实 deb/rpm 工具检查旧 ABI/混合 ABI 拒绝、助手路径重定位、包依赖及 Nix pin 校验。
- `scripts/ci/check-gui-go.sh` 编译与静态检查 GTK4 发布/E2E 标签；PR GUI 作业在 Ubuntu 24.04 安装 GTK4 构建依赖，Rust/CLI 继续使用独立 bookworm 镜像，不抬高其基线。
- 本机用私有 Xvfb、D-Bus、密钥环和独立 profile 运行了新挂载命名空间下的真实 WebView：20 项历史列表、主题与快捷面板检查通过；这只是路径与功能验证，不能替代真实 AppImage 的挂载、更新或触控板体感验收。
- 当前包验收矩阵改为 Ubuntu 24.04、Debian 13 和 Fedora；AppImage 内容及进程映射检查改为 GTK4/WebKitGTK 6.0，并拒绝旧 GTK3 库。
- 本轮环境无 Docker socket 权限、Snapcraft 和 Flatpak Builder，完整各架构容器构建、真实包安装及桌面功能需要 CI/专用主机验收。静态检查、合成包检查与本机 GUI 编译不能替代这些结果。

后续 CI 已确认两种架构的原生包与 AppImage 构建通过，Snap 的重复 HarfBuzz 文件也已实际移除；Snap 启动仍未达到 WebKit 检查，正在补齐发布宿主的文件日志。AUR 的普通用户命名空间探针仍被 Ubuntu 宿主策略拒绝，因此增加了上述仅供测试容器使用的 AppArmor 配置；尚待新一轮运行确认。

## 上游依据

- [GdkMonitor.get_scale：GTK 4.14 起提供](https://docs.gtk.org/gdk4/method.Monitor.get_scale.html)
- [GtkFileDialog：GTK 4.10 起提供](https://docs.gtk.org/gtk4/class.FileDialog.html)
- [GNOME 50 平台构成：包含 WebKitGTK 6.0](https://raw.githubusercontent.com/GNOME/gnome-build-meta/gnome-50/elements/sdk-platform.bst)
- [Ubuntu 24.04 的 AppArmor 用户命名空间限制与配置规则](https://documentation.ubuntu.com/release-notes/24.04/)
- [Arch Linux WebKitGTK 6.0 包](https://archlinux.org/packages/extra/x86_64/webkitgtk-6.0/)
- Wails 固定版本及内嵌 GTK 插件以 `apps/gui-go/go.mod` 解析的模块为准；插件已支持 GTK4，无须复制或修改上游脚本。
