# Linux GTK4 打包迁移

2026-10-23：所有当前 Linux GUI 开发、编译与打包入口统一使用固定 Wails beta.28 的 GTK4 后端。历史 GTK3 的测量和切片验收保留为历史证据，不能证明新产物可安装或运行。迁移本身不代表触控板滚动拖影已解决。

## 依赖与兼容范围

Wails 使用 GTK 4.14 起提供的 `GdkMonitor.get_scale`，因此要求 GTK ≥ 4.14（`GtkFileDialog` 自 GTK 4.10 起提供）；WebKit 使用 `webkitgtk-6.0` ABI。公共 GUI 构建镜像改为 Ubuntu 24.04；打包产物中 ELF 的 GLIBC 符号版本需求上限为 2.39，而非宿主 glibc 的版本上限；这是明确提高的构建基线，不再承诺 Debian 12 或 Ubuntu 22.04 的原生 deb/rpm/AppImage 兼容性。最终产物中每个 ELF 的实际需求仍由 `elf_floor.py` 检查。

| 渠道 | GTK4 契约 |
| --- | --- |
| deb | `libc6 (>= 2.39)`、`libgtk-4-1 (>= 4.14)`、`libwebkitgtk-6.0-4`、`libx11-6` |
| rpm / COPR | GTK4、WebKitGTK 6.0、X11 的 soname；glibc ≥ 2.39；GTK 包版本 ≥ 4.14。COPR 与上游 RPM 使用相同能力声明，回归检查防止漂移 |
| AppImage | 使用 Wails 自带 GTK 插件的 `DEPLOY_GTK_VERSION=4`；携带 GTK4、WebKitGTK 6.0、JavaScriptCore 6.0 及 WebKit 进程助手；将 `webkitgtk-6.0` 的编译期助手路径等长重定位到包内 |
| AUR | `gtk4>=4.14`、`webkitgtk-6.0`、`libx11`、`libsoup3`；源码使用 `gtk4,production,release` 标签 |
| Snap | 使用 `core24`、strict、devel；GTK ≥ 4.14 由 GNOME 46 SDK/内容平台提供，应用打包 WebKitGTK 6.0；细节见 [分发渠道](gui-go-distribution-channels.md) |
| Nix | GTK4 发布使用 `wrapGAppsHook4`、`gtk4`、`webkitgtk_6_0`；更新源 pin 前校验两个 deb 的真实 SHA-256 和 GTK4 ELF 依赖 |
| Flatpak / Flathub | GNOME 50 运行时；构建时拒绝 GTK3 输入 deb。发布 URL、哈希与 AppStream 占位项仍须在提交前替换，不能视为已可发布 |

安装脚本对 Ubuntu 20.04 / 22.04、对应派生发行版和 Debian 11 / 12 选择 Snap，避免自动安装不满足原生 GTK4 依赖的 deb。其他发行版仍须满足包声明的 GTK 与 glibc 条件；Snap 在旧宿主上的实际兼容性需要验收。

GTK4 快捷面板使用普通窗口。本项目尚未实现 GTK4 Layer Shell 适配，因此新的包不声明或捆绑 GTK3 的 `libgtk-layer-shell`，Wayland 的覆盖层、独占键盘和定位不能沿用旧 GTK3 验收结论。仅用于在旧提交中重现 GTK3 Layer Shell 实验的 `run_17c*` 脚本保留其原始标签；它们不属于当前包的构建入口。

实际 CI 发现 GNOME 42 SDK 的 GTK 4.12.3 无法编译 Wails 的 `gdk_monitor_get_scale` 调用，因此 Snap 基线改为 core24/GNOME 46。core24 是包内运行时，不代表宿主必须是 Ubuntu 24.04；Ubuntu 20.04 的 snapd、内核与桌面兼容性仍须验收。AppImage 在部署结束后按宿主库策略移除 Vulkan 和 D-Bus 入口库，干净验收宿主须提供 Vulkan 加载器。

Snap 的 GNOME 平台统一提供共享库，应用仅携带平台未提供的 WebKitGTK 6.0 等依赖。最后执行的 `platform-cleanup` 部件根据实际内容平台的文件路径及 ELF SONAME 清理重复库，避免应用内较旧的 HarfBuzz、GLib 等遮蔽平台版本；不同版本的实际文件名也按 SONAME 归属清理。此阶段随后检查 WebKitGTK 6.0 的库和进程助手完整性。

AUR 安装验收在可丢弃容器中由 root 安装软件包，再由独立普通用户启动密钥环、Xvfb 和应用；不向应用授予宿主 `SYS_ADMIN`。启动前先检查普通用户的 bubblewrap 用户及网络命名空间，随后按可执行文件路径识别 WebKit 子进程，并确认真实窗口映射，避免将沙箱包装进程或仍存活的托盘宿主误当作成功启动的界面。

## 已发布产物的迁移顺序

Nix 当前仍锁定已发布的 GTK3 `1.2.0-alpha.7`，不能把这份产物改称 GTK4，也不能为尚未发布的 deb 编造校验和。配方仅为这一个准确的版本及 `gtkMajor: 3` 保留原有依赖；其他源必须声明 `gtkMajor: 4`。首个 GTK4 deb 发布后，使用 `packaging/nix/update.py <version> <SHA256SUMS.txt> <deb-directory>` 验证并更新 pin，同时删除 `legacyGtk3` 分支及对应验收兼容逻辑。CI 同时评估 GTK4 分支，默认构建继续核验真实旧 pin。

Flatpak manifest 原本使用占位 URL/哈希，目前仍不可提交发布。GTK4 构建检查会拒绝误填入的旧 GTK3 deb；必须填入真实 GTK4 发布物之后再运行 `flatpak-builder`。

## 验证与边界

- `scripts/tests/test_linux_gtk4_packaging.py` 使用可丢弃 ELF 和真实 deb/rpm 工具检查旧 ABI/混合 ABI 拒绝、助手路径重定位、包依赖及 Nix pin 校验。
- `scripts/ci/check-gui-go.sh` 编译与静态检查 GTK4 发布/E2E 标签；PR GUI 作业在 Ubuntu 24.04 安装 GTK4 构建依赖，Rust/CLI 继续使用独立 bookworm 镜像，不抬高其基线。
- 当前包验收矩阵改为 Ubuntu 24.04、Debian 13 和 Fedora；AppImage 内容及进程映射检查改为 GTK4/WebKitGTK 6.0，并拒绝旧 GTK3 库。
- 本轮环境无 Docker socket 权限、Snapcraft 和 Flatpak Builder，完整各架构容器构建、真实包安装及桌面功能需要 CI/专用主机验收。静态检查、合成包检查与本机 GUI 编译不能替代这些结果。

## 上游依据

- [GdkMonitor.get_scale：GTK 4.14 起提供](https://docs.gtk.org/gdk4/method.Monitor.get_scale.html)
- [GtkFileDialog：GTK 4.10 起提供](https://docs.gtk.org/gtk4/class.FileDialog.html)
- [GNOME 50 平台构成：包含 WebKitGTK 6.0](https://raw.githubusercontent.com/GNOME/gnome-build-meta/gnome-50/elements/sdk-platform.bst)
- [Arch Linux WebKitGTK 6.0 包](https://archlinux.org/packages/extra/x86_64/webkitgtk-6.0/)
- Wails 固定版本及内嵌 GTK 插件以 `apps/gui-go/go.mod` 解析的模块为准；插件已支持 GTK4，无须复制或修改上游脚本。
