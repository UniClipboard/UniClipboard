# Nix 打包

本目录让 Nix 用户安装已发布的 UniClipboard 桌面应用（GUI 宿主 `uniclipboard` 加它自带的守护进程 `uniclipd`）。仓库根目录的 `flake.nix` 是入口，`package.nix` 是可被 `callPackage` 的 derivation，`sources.json` 保存版本与按架构的哈希。

## 状态

- **未在 Nix 里构建过。** 编写环境没有 Nix，Docker 守护进程未启动，两台 Linux 测试机不可达。`package.nix` 是经过静态核对的实现，不是已验证的实现。验证由 `.github/workflows/nix.yml` 在原生 x86_64 与 aarch64 runner 上完成，见「验证」。
- 打包版本：`1.2.0-alpha.7`（预发布）。这是目前唯一由 Go 宿主构建的发布；最新正式版 `v1.1.2` 仍是已退役的 Tauri 宿主，布局和依赖不同（`libayatana-appindicator`），本包不支持。出正式的 Go 宿主版本后用 `update.py` 切换。
- 支持架构：`x86_64-linux`、`aarch64-linux`，与发布的 `.deb` 一致。macOS 与其他架构不支持。
- 需要 nixpkgs 26.05 或更新版本（`package.nix` 使用顶层属性 `libx11`）。flake 固定在 `nixos-26.05`。

## 安装与运行

这些命令用到 `nix-command` 与 `flakes` 两个实验特性（Nix 2.35 手册里 `nix run`、`nix profile` 仍标注为实验性）。在 `~/.config/nix/nix.conf` 写入 `experimental-features = nix-command flakes`，或在每条命令后加 `--extra-experimental-features 'nix-command flakes'`。

`github:UniClipboard/UniClipboard` 指向默认分支，合入之前不存在这个 flake；在本仓库的工作树里先对新文件执行 `git add -N .`（flake 只看得到已进入 git 索引的文件），再用 `nix run .` 与 `nix build .#uniclipboard`。

```bash
# 试运行，不安装
nix run github:UniClipboard/UniClipboard

# 安装到用户 profile（Nix 2.35 手册的命令名；旧版本叫 `nix profile install`）
nix profile add github:UniClipboard/UniClipboard
```

NixOS 上，把下面几行加进现有配置。`inputs` 与 `outputs` 的写法取决于你的 flake，这里只给出与 UniClipboard 有关的部分：

```nix
{
  # flake inputs
  inputs.uniclipboard.url = "github:UniClipboard/UniClipboard";

  # in a NixOS module (it needs `pkgs` and the flake's `uniclipboard` input in scope)
  environment.systemPackages = [ uniclipboard.packages.${pkgs.stdenv.hostPlatform.system}.default ];
  # The daemon keeps its keys in the Secret Service. Use any provider; GNOME Keyring is one.
  services.gnome.gnome-keyring.enable = true;
  # Only if you use the commands above on this machine.
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
}
```

不要让 `uniclipboard` 的 `nixpkgs` 输入 `follows` 低于 26.05 的 nixpkgs。

升级：`nix profile upgrade` 或 `nix flake update uniclipboard`。仓库 `main` 上的 `sources.json` 只在发布之后由维护者更新，所以 `main` 指向最近一次已发布且已更新的版本。

## 为什么重打包 `.deb`

| 方案 | 结论 |
| --- | --- |
| 包装 AppImage（`appimageTools.wrapType2`，旧 `package.nix` 的做法） | 放弃。宿主在 `APPIMAGE`/`APPDIR` 上分支（安装类型、原地自更新、重启、自启动、便携数据根），包装后 Nix 安装会表现成会自更新的 AppImage；还要 FHS 沙箱和用户命名空间，并带一份与 nixpkgs 不同的 GTK/WebKitGTK。 |
| 从源码构建 | 暂不做。需要固定输出的 bun 依赖树、Go vendor 哈希、带 git 依赖的 `cargoLock` 和 Engine 构建工具；落地与维护成本高。需要时再迁移，`meta` 与桌面文件处理可复用。 |
| **重打包 `.deb`（采用）** | `.deb` 只含 `uniclipboard`、`uniclipd`、桌面文件和图标，动态链接系统 GTK 3、WebKitGTK 4.1、libsoup 3。这些库由 nixpkgs 提供，用 `autoPatchelfHook` 修正解释器与 RUNPATH，不需要 FHS 环境，也没有 AppImage 分支。发布物按 SHA256 固定，glibc 下限 2.36，低于 nixpkgs。 |

`.deb` 来自 `https://github.com/UniClipboard/UniClipboard/releases/download/v<version>/`，`fetchurl` 用 `sources.json` 里的 SRI 哈希固定，没有可变 URL。哈希等于发布的 `SHA256SUMS.txt`（已用 `SECURITY.md` 的发布公钥验证过签名），`update.py` 负责转换。

## 运行时设计

- **守护进程不包装。** 宿主只在 `/proc/<pid>/exe` 的文件名是 `uniclipd` 时接受正在运行的守护进程（`packages/desktop-host-go/daemonproc/pid.go`）。被 `wrapProgram` 改名为 `.uniclipd-wrapped` 会破坏复用与接管判断，所以 `uniclipd` 只做 `autoPatchelf`。`wrapGAppsHook3` 用 `dontWrapGApps` 关掉，只在 `postFixup` 包装 GUI。
- **GUI 与守护进程同在 `bin/`。** 宿主按“自身可执行文件旁边，再 PATH”查找 `uniclipd`（`ResolveDaemonExe`）。GUI 被包装后真实文件是 `bin/.uniclipboard-wrapped`，其旁边仍是 `bin/uniclipd`。
- **`libgtk-layer-shell.so.0` 是 `dlopen`，不在 `DT_NEEDED`。** Wayland 快捷面板靠它；缺库时面板静默退回普通窗口。`runtimeDependencies` 把它写进 RUNPATH，CI 检查该库可从 RUNPATH 找到。
- **GTK 与 WebKitGTK 数据**（gsettings schema、GIO 模块、pixbuf 加载器、图标主题）由 `wrapGAppsHook3` 放进 GUI 包装器。`xdg-open`（打开链接与文件）和 `gsettings`（托盘主题）以 `--suffix PATH` 追加，宿主自己的工具优先。
- **剪贴板**：Wayland 与 X11 后端是纯 Rust（`wayland-backend`、`x11rb`），不依赖外部命令或 `dlopen`，不需要额外包。
- **托盘**走 StatusNotifierItem（D-Bus），不链接 `libappindicator`；GNOME 需要自带的 AppIndicator 扩展。
- **用户数据不在 store 里。** 数据根仍是各平台的用户目录（`$XDG_DATA_HOME/app.uniclipboard.desktop`），profile 语义不变。

## 已知缺口

1. **应用内“开机自启”在 Nix 安装下不可靠。** Wails 把 `os.Executable()` 解析后的真实路径写进 XDG 自启动条目，这里是 `/nix/store/…/bin/.uniclipboard-wrapped`：它绕过包装器（缺 GTK 数据路径），升级并 GC 后路径失效，启动时对已有条目不再改写（`apps/gui-go/autostart.go` `apply` 的 `reconcile`）。在 Nix 里请不要使用该开关，改用声明式自启动，例如 Home Manager 的 `xdg.autostart.entries = [ "${pkg}/share/applications/uniclipboard.desktop" ];`。根治需要改产品代码（见 Next）。本包不创建任何 systemd 用户服务，也不修改主机配置。
2. **应用内更新对 Nix 安装不适用。** `/nix/store` 下的安装类型是 `unknown`，前端不会走“请用包管理器更新”对话框，而宿主会拒绝原地安装（`ErrInstallUnsupported`）。需要新增 `nix` 安装类型与前端文案，属于产品改动，未做。
3. **没有 `uniclip` 命令行包。** 发布的 CLI 是静态 musl 构建，自带一份与 GUI 守护进程链接方式不同（无 `libdbus`）的 `uniclipd`，且文件名与本包冲突；在确认两者对同一 profile 的密钥存储一致之前不提供。
4. **`flake.lock` 缺失。** 编写环境没有 Nix，无法生成锁文件，手写 `narHash` 出错会让 flake 无法求值。首次在有 Nix 的机器上运行 `nix flake lock` 并提交，之后 nixpkgs 才是可复现固定的。
5. **非 NixOS 主机的 GPU。** Nix 的 libglvnd 在没有 `/run/opengl-driver` 的发行版上找不到驱动，WebKitGTK 可能没有 EGL。这是所有 Nix GUI 程序的通病（常用 nixGL 解决），不是本包独有；NixOS 不受影响。
6. **外部提交。** 没有提交 nixpkgs，也没有二进制缓存。nixpkgs 此前的 PR（NixOS/nixpkgs#536692，包装 AppImage）已关闭，未合入。

## 验证

动手前列出的失败模型，以及各自由谁发现：

| 失败模型 | 发现方式 |
| --- | --- |
| `DT_NEEDED` 库名与 nixpkgs 属性对不上（如 `libx11`） | `autoPatchelfHook` 构建失败 |
| `patchelf` 破坏非 PIE 的 Go 可执行文件 | 启动检查：GUI 与守护进程进程存活 |
| `dlopen` 的 layer-shell 库不在 RUNPATH，快捷面板静默退化 | `check-nix-package.sh` 静态检查 RUNPATH 中的库文件 |
| `uniclipd` 被包装，守护进程身份判断失败 | 检查 `bin/uniclipd` 是 ELF，且运行中的 `/proc/<pid>/exe` 就是它 |
| GUI 找不到旁边的守护进程 | 启动检查：守护进程由 GUI 拉起并写出 `daemon.conn` |
| GTK/GIO/gsettings 数据缺失导致崩溃或空白 WebView | 启动检查：GUI 与 WebKit 进程在 WebView 出现 15 秒后仍存活；日志含 `Main window revealed`；日志没有加载器、GTK、GSettings 错误（与 `scripts/linux-appimage-smoke.sh` 同一组信号）；EGL 错误行只作信息记录 |
| 数据写到 store 或主机真实目录 | 启动检查用一次性 HOME 与 XDG 目录，`daemon.conn` 必须出现在其中 |
| 哈希与发布物不一致 / 可变 URL | `fetchurl` 固定哈希；`update.py` 对同一个 `SHA256SUMS.txt` 复算得到相同的 `sources.json` |

`.github/workflows/nix.yml` 在 `ubuntu-24.04` 与 `ubuntu-24.04-arm` 上执行 `nix flake check --no-build --all-systems`、`nix build`，再运行 `scripts/ci/check-nix-package.sh`：静态检查（布局、`ldd`、RUNPATH、桌面文件与图标），然后在 Xvfb、私有 D-Bus 与一次性 GNOME Keyring 中启动 GUI，核对进程路径、存活、`daemon.conn`。证据（`results.tsv`、`commands.log`、`gui.log`、`launch-ps.txt`）作为 artifact 上传，可重复运行。Wayland、托盘、真实 GPU 与桌面环境的目视验收需要在 NixOS 上人工进行，CI 不覆盖。

本次在编写环境里实际做过的：对 `v1.2.0-alpha.7` 的两个 `.deb` 与 `SHA256SUMS.txt` 逐项核对（SHA256 完整比对，`rsign` 验证 `SHA256SUMS.txt.minisig` 通过）；用 `readelf` 读取两个架构上两个二进制的 `DT_NEEDED` 与 `dlopen` 字符串；`update.py` 复算 `sources.json` 与手写结果一致。

## 维护：更新到新发布

```bash
gh release download v<version> -R UniClipboard/UniClipboard -p 'SHA256SUMS.txt*'
# Verify the signature first; SECURITY.md has the public key and the command.
packaging/nix/update.py v<version> SHA256SUMS.txt
nix build .#uniclipboard   # and review the diff of packaging/nix/sources.json
```

如果新版 `.deb` 的二进制、依赖或安装路径变了，先核对 `package.nix`（`DT_NEEDED` 列表、`dlopen` 的库、`usr/share` 内容），再提交。
