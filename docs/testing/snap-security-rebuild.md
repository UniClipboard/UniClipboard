# Snap libxml2 修复与运行验收

## 库所有权

应用显式拥有 Ubuntu Noble 的 `libxml2=2.9.14+dfsg-1.3ubuntu3.10`。配方复用已有 stage 依赖，不引入新的库。

Snapcraft 在 `override-build` 前将 stage 包解包到 `CRAFT_PART_INSTALL`。安装阶段使用 `cp -a` 将目标架构的 `libxml2.so.2*` 保存到 `usr/lib/uniclipboard-security/`。这个目录只包含应用拥有的 libxml2。符号链接和实际库文件一起复制。缺少可读取的 SONAME 时构建失败。

应用命令为 `usr/bin/uniclipboard-launch`。GNOME/Mesa 命令链先执行，启动器随后只将私有目录前置到 `LD_LIBRARY_PATH`，再通过 `exec` 启动原 GUI。启动器保留全部参数。GUI、Wails 子进程和 daemon 继承该环境。私有 SONAME 缺失时启动器直接失败。

其他平台 ABI 库继续由平台提供。修复不使用 `LD_PRELOAD`，不修改沙箱，不写用户配置。固定版本不可获取时构建必须失败；更新修订号时必须重新验证来源和实际字节。

## 顺序验收

1. 保留旧候选及其运行证据。不要发布旧候选。
2. 先在隔离 VM 中使用旧候选 payload、私有 `.10` 库和新启动器制作受控重打包。不要覆盖可运行的原候选。
3. 对比旧候选和受控重打包的实际进程库映射。验证 GUI、Wails UI 和 WebKit 进程均加载私有目录中的修复字节。验证 AppArmor 保持 enforce，WebKit 沙箱保持启用，窗口出现，daemon 健康。
4. 受控验证通过后，再从修改后的源代码构建 amd64 和 arm64 Snap。
5. 分别核对两个真实 Snap 中的私有 SONAME、链接目标和实际库文件。将实际文件与对应架构的官方 `.10` deb 逐字节比较。manifest 声明不能代替此验证。
6. 对两个源码构建包重做 strict 运行验收。记录进程身份、Snap/base/content revision、实际映射路径和映射文件 SHA256。未使用 libxml2 的 daemon 不要求出现该映射，但必须保持健康。
7. 验证 GTK、GLib 等平台库仍来自原平台。保留脱敏命令、JSON 和运行结果。
8. 只在上述验收通过后选择新候选，另行执行已授权的 edge 发布。不要从旧候选摘要直接发布。

现有 recipe 检查只证明兼容性。它不能证明真实动态加载结果。受控重打包、源码构建、strict 运行和 Store 发布必须分别记录状态。
