# Go GUI 的 Linux AppImage 真实宿主应用链路（切片 17c11）

本文是 17c11 的契约，**先于** 运行器和镜像之外的任何实现写成（提交顺序见 git 历史）。17c10（[gui-go-linux-appimage-host-helpers.md](gui-go-linux-appimage-host-helpers.md)）用 `sh` 记录脚本证明了 `xdg-open`/`gio` 的环境；本片把处理程序换成发行版真实维护的浏览器、文件管理器和图片查看器，并在 **非便携模式**（真实 `HOME`，用户自己的默认应用）下验证。

## 范围与前提变化

- 为了让 `xdg-open` 能启动真实应用，运行镜像必须带宿主 GTK（浏览器/文件管理器的依赖）。17c4/17c7/17c10 的「宿主无 GTK」前提在 17c11 的镜像里 **不再成立**；TLS 与内容回归仍在无 GTK 的 17c7 镜像上跑。后果：GUI 自己必须仍然从挂载内映射 GTK/WebKitGTK/GLib（`/proc/<pid>/maps`），否则宿主 GTK 会让包装偶然通过。该项是断言，不是说明。
- 修复路径不允许是「删光包内库」「让宿主 GTK 兜底」或放宽断言。
- 本片的基线就是当前 HEAD（含 17c10 修复）。绿 = 验收；红 = 新缺陷，需先留可信基线，再窄修同一链路并做正负对照。

## 夹具选择（以包管理器事实为准）

| 发行版 | 浏览器 | 文件管理器 | 图片查看器 | 说明 |
| --- | --- | --- | --- | --- |
| Ubuntu 24.04 | Epiphany 46.5（GNOME Web，deb，WebKitGTK） | Nautilus 46.4 | Loupe 46.2 | Ubuntu 的 `firefox`/`chromium-browser` 是 snap 过渡包（`snapd` Pre-Depends），不可用 |
| Fedora 44 | Firefox 157（rpm） | Nautilus 50.3.1 | Loupe 50.0 | 第二轮，首条跑通后扩展 |

默认应用 **不用夹具注册**：软件包自己安装的 `.desktop`、`mimeinfo.cache` 决定默认（`xdg-mime query default` 与 `gio mime` 的结果记录在证据里）。「用户默认应用」场景由用户 `uc` 通过宿主自己的 `xdg-mime default` 写入真实 `~/.config/mimeapps.list`（非便携模式下 GUI 看到同一个 `HOME`），**禁止** 写 `/etc/xdg/mimeapps.list` 或 `/usr/share/applications` 来掩盖用户默认应用问题。

## 失败方式（每项对应一条断言）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| G1 | 真实应用在清理后的环境下仍启动失败或崩溃（17c10 修复遗漏的变量，例如 `GTK_THEME`、`GDK_BACKEND`、`APPIMAGE`/`ARGV0`/`OWD`） | 目标必须真的出现：浏览器向受控 HTTP 服务器发出请求，文件管理器/图片查看器出现窗口 |
| G2 | 应用启动但加载了挂载内的库（`/proc/<pid>/maps`）或环境变量指向挂载（`/proc/<pid>/environ`） | 对存活的浏览器、其子进程（WebKit 辅助进程）、文件管理器、图片查看器逐一读取，root + `SYS_PTRACE` |
| G3 | 浏览器去了错误或额外目标 | 受控服务器只监听 `127.0.0.1`，容器无外网；断言恰好一次 `GET /uc11-<nonce>` 且 `User-Agent` 属于该浏览器；页面 `<title>` 带 nonce，X 窗口标题含 nonce（`xwininfo -root -tree`） |
| G4 | 文件管理器/图片查看器打开了错误对象 | 窗口标题等于目标目录的 basename（`reveal_path` 为其父目录 = 产品既有语义）/ 图片文件名；进程 `cmdline` 记录；每个目录名含 nonce |
| G5 | 非便携模式下用户默认应用对 helper 不可见 | 用户级 `xdg-mime default` 改默认为另一个真实应用，GUI 链路必须打开改后的应用（正对照）；去掉用户注册则回到包默认（负对照） |
| G6 | 夹具冒充宿主行为 | 无 GUI 的宿主对照（宿主环境下 `xdg-open`）必须到达同一真实应用；没有处理程序的类型不得到达；不使用 `sh` 记录脚本、`--version` 或桩 |
| G7 | 宿主 GTK 使 GUI 自己偶然通过 | 断言 GUI 与 WebKit 辅助进程仍从挂载映射 `libgtk-3`、`libwebkit2gtk-4.1`、`libglib-2.0`、`libgio-2.0` |
| G8 | 浏览器首次运行行为（欢迎页、遥测）造成额外请求 | 容器用 `--network none`（受控服务器在回环），额外请求没有出口；Epiphany 不需要额外策略则不加配置 |
| F7 | 便携模式把 `HOME` 换为 `<AppImage>.home`，用户默认应用不可见 | 用真实应用重现并 **记录**（不判定通过/失败），保持 OPEN，不改产品 |

## 明确不证明

- 真实桌面会话、portal（`xdg-desktop-portal`）、真实 Wayland（`GDK_BACKEND=x11` 在 Xvfb 上无法对照）、原生 GPU、原生 amd64（容器是 arm64）。
- `GDK_BACKEND`/`GTK_THEME` 未清理：除非真实应用出现新失败，否则不改。
- Wails `Env.OpenFileManager`（FileManager1 `ShowItems`）：Nautilus 上线后可测，本片不改 `reveal_path` 语义，列为后续候选。
- 其他浏览器/文件管理器（Dolphin、Thunar、Chromium）、其他发行版、F7 的产品决策。

## F7：产品语义（待协调者决策，不在本片改）

便携 AppImage 运行时把 `HOME` 改为 `<AppImage>.home` 并作用于整个进程树，原始 `HOME` 不导出（仅可经 `getpwuid` 恢复）。行为后果：宿主辅助程序看不到用户 `~/.config/mimeapps.list`、主题、浏览器配置。成熟产品的便携模式通常只重定向自己的数据目录，不改子进程可见的用户环境。本片在真实应用上重现并记录该后果；不静默更改产品。
