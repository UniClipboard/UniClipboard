# Go GUI 的 Linux AppImage 真实宿主应用链路（切片 17c11）

本文是 17c11 的契约，**先于** 运行器与驱动写成（提交顺序见 git 历史：契约与 Ubuntu 镜像 → 运行器 → 驱动 → 契约修订）。契约写成之后在运行中发现的偏差记录在「契约修订」一节，没有改写原条款。17c10（[gui-go-linux-appimage-host-helpers.md](gui-go-linux-appimage-host-helpers.md)）用 `sh` 记录脚本证明了 `xdg-open`/`gio` 的环境；本片把处理程序换成发行版真实维护的浏览器、文件管理器和图片查看器，并在 **非便携模式**（真实 `HOME`，用户自己的默认应用）下验证。

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

## 契约修订（运行中发现，保留原失败轮；每条都有原始证据）

17c11 的首个最终运行 `final-b3aca3d11`（保留）中 generic 分发两组失败，最终运行之前的开发运行 dev1–dev14 也全部保留。修订如下，**不是** 放宽产品或断言：

| # | 发现 | 证据 | 修订 |
| --- | --- | --- | --- |
| R1 | 容器 `--network none` 下守护进程启动失败（`engine error 1101`，p2p 绑定）；bwrap（Epiphany 的 WebKit 沙箱）需要 `seccomp`/`systempaths` 不受限 | dev1、dev2、dev4；`engine-default-route-diag/`（bridge 与内部网络加默认路由通过，内部网络无默认路由与 `--network none` 失败）| 夹具：`--internal` 网络（无出口）+ 运行器加 `default dev eth0`（`NET_ADMIN`）。**这是夹具要求；「Engine 在没有默认路由的主机上启动失败」是未修复的 Engine 行为（OPEN，Engine 在本仓只读），不是本片修复的产品问题** |
| R2 | Epiphany 的 User-Agent 是 Safari 兼容串（不含 `Epiphany`）| dev4 | 浏览器由进程 exe 与窗口标题认定，User-Agent 只断言 `AppleWebKit`（Firefox 为 `Firefox/`）|
| R3 | generic 分发（`XDG_CURRENT_DESKTOP` 未设）下 `xdg-open <目录>` 直接以前台子进程运行 `nautilus --new-window`，窗口出现后 xdg-open 仍存活，直到 Nautilus 退出；GNOME 分发下 `gio open` 立即返回 0，Nautilus 是 gapplication-service | `xdg-open-generic-diag-ubuntu-v2/`、`-fedora/`（首个 `-ubuntu/` 因变量传递错误两组都是 generic，保留作失败轮）| 宿主对照与 GUI 链路都不把「未退出」当失败：观察效果（窗口/请求），记录 `alive/exit/childExes`；自然退出必须为 0，仍在前台等待时必须有存活的应用子进程；清理导致的退出码（例如被终止的 Nautilus 使 xdg-open 退出 4）与自然退出分开记录 |
| R4 | 「没有默认应用的类型就不会被打开」的假设被反证：generic 分发回退到浏览器（Ubuntu：`x-www-browser` → Epiphany 被执行，`dev12-ubuntu-generic`；Fedora：直接运行 Firefox，`final-a0ac923cb/real-fedora-generic`），GNOME 分发由 `gio` 原生拒绝（退出 4，`Failed to find default application for content type`）| `dev10-*`（文本文件也被打开）、`dev12-*` 的 strace | 该场景只作为观察记录（GNOME 下断言原生拒绝），**不再是 harness 的负面对照** |
| R5 | 独立的负面对照 | `dev13-*` | 不存在的路径：generic 下 xdg-open 退出 2（文档化的状态），GNOME 下 `gio open` 退出 4（xdg-open 把路径交给 gio，沿用 gio 的状态；首版两者都断言 2，在 GNOME 下失败，保留）；二者都带宿主原生「不存在」信息，不执行任何真实应用，无窗口与应用进程 |
| R6 | 观察器：未读到进程的 exe/environ/maps 不能作为「无挂载库」的证据 | dev8 | `inspect` 直接读取并记录状态与 errno：`exited` 排除并记录，存活但读不到则 G2 失败 |

## 明确不证明

- 真实桌面会话、portal（`xdg-desktop-portal`）、真实 Wayland（`GDK_BACKEND=x11` 在 Xvfb 上无法对照）、原生 GPU、原生 amd64（容器是 arm64）。
- `GDK_BACKEND`/`GTK_THEME` 未清理：除非真实应用出现新失败，否则不改。
- Wails `Env.OpenFileManager`（FileManager1 `ShowItems`）：Nautilus 上线后可测，本片不改 `reveal_path` 语义，列为后续候选。
- 其他浏览器/文件管理器（Dolphin、Thunar、Chromium）、其他发行版、F7 的产品决策。

## F7：产品语义（待协调者决策，不在本片改）

便携 AppImage 运行时把 `HOME` 改为 `<AppImage>.home` 并作用于整个进程树，原始 `HOME` 不导出（仅可经 `getpwuid` 恢复）。行为后果：宿主辅助程序看不到用户 `~/.config/mimeapps.list`、主题、浏览器配置。成熟产品的便携模式通常只重定向自己的数据目录，不改子进程可见的用户环境。本片在真实应用上重现并记录该后果；不静默更改产品。

## 验证结果

范围（原样）：容器内，arm64 Docker，Xvfb，非 root 用户 `uc`，**非便携模式**（真实 `HOME`，发布形态数据根），私有会话总线与解锁的 Secret Service 与应用在同一容器；发行版自己的浏览器、文件管理器、图片查看器；真实 17c5 发布守护进程（SHA-256 `ea0f0bcb…f6c6`，运行前核对，非桩）；`--internal` 网络（无出口）。不是真实桌面会话、不是 portal、不是 Wayland、没有 GPU、不是原生 amd64。

最终运行 `final-a0ac923cb`（干净检出 `a0ac923cb7c587163ccaaf24104937212411ba69`，`inputs/dirty.diff` 为空；本次构建的 AppImage SHA-256 `a2a001ac…acd3`，所有运行用同一文件；两个新镜像在构建前删除了旧标签，构建完整输出、退出码、`docker image inspect` 与 ID 在 `logs/`、`images/`）。证据目录 `t-0188-artifacts/linux-17c11/`（约 1.1 GB 最终运行加开发运行，库内只有索引）。

| 运行 | 结果 |
| --- | --- |
| 真实应用 E2E：Ubuntu 24.04（Epiphany 46.5 / Nautilus 46.4 / Loupe 46.2）× generic、GNOME | 30/30、31/31 |
| 真实应用 E2E：Fedora 44（Firefox 157 / Nautilus 50.3.1 / Loupe 50.0）× generic、GNOME | 30/30、31/31 |
| 控制组：17c10 修复 **之前** 的 AppImage（`baseline-03d304fb5`），同一 E2E，Fedora GNOME | **7 项失败（预期）**：`open_logs_directory`、`open_data_directory` 没有窗口，默认 Loupe 没有打开，「GUI 启动的真实应用都在运行」不成立（只有 Firefox 在运行），应用进程映射/环境检查失败，GUI 启动的 xdg-open 带挂载变量。`reveal_path` 与 URL 在该包上通过（不是所有行为都失败）|
| 17c10 回归（`sh` 记录脚本）Ubuntu/Fedora × generic/GNOME | 各 33/33 |
| 回归：WebView HTTPS（17c7，无 GTK 镜像）Ubuntu/Fedora | 各 19/19 |
| 回归：便携模式 E2E（17c5） | 64/64 |
| 静态内容检查 | 通过（`passed: true`）|

每个真实应用运行断言的内容：宿主对照（无 GUI，宿主环境）能打开真实文件管理器、浏览器（受控 HTTP 服务器收到恰好一次该路径的请求，窗口标题是页面标题；「没有其他请求」的判定排除 `/favicon.ico` 与宿主对照自己的 `/uc11-host-*` 路径，浏览器的 favicon 请求不判为额外目标）、图片查看器；GUI 链路：`reveal_path`（窗口标题 = 父目录）、`open_logs_directory`、`open_data_directory`、`open_image_externally`（包默认应用 Loupe）、**用户默认应用**（用户用宿主 `xdg-mime default` 在真实 `~/.config/mimeapps.list` 写入后产品打开的是浏览器；删除后回到 Loupe）、URL（共享前端 `openUrl` → host 命令 `open_url` → 真实浏览器 → 受控服务器：一次请求、浏览器 UA、页面标题、没有其他请求）；GUI 与 WebKit 辅助进程仍从挂载映射 GTK/WebKitGTK/JavaScriptCore/GLib/GIO（宿主有 GTK 时 G7）；所有存活的真实应用进程（Epiphany 及其 WebKit 进程、Firefox、Nautilus、Loupe）逐个读取 exe/environ/maps：没有挂载内的库、没有指向挂载的变量，读不到的存活进程判失败（本次 0 个）；GUI 启动的每个 `xdg-open` 的 strace envp 无挂载变量、无 `LD_LIBRARY_PATH`，退出 0 或是仍在前台等待存活应用的 generic 分发。

### 观察（记录，不是修复）

- **F7 用真实应用重现（四组一致，只记录）**：便携模式下（`HOME` = `<AppImage>.home`）用户写入真实 `HOME` 的默认应用（PNG → 浏览器）对 `xdg-open` 不可见，由包默认的 Loupe 打开。保持 OPEN，产品语义待决，未改。
- **`GDK_BACKEND=x11` 与 `GTK_THEME=Adwaita:light` 泄漏给真实浏览器**：Epiphany（及其 WebKit 进程）和 Firefox 的环境里有这两个变量（来源无法与用户设置区分；AppRun 的 GTK 钩子会导出它们）；GNOME 分发的运行里 Nautilus/Loupe 是 D-Bus 激活的服务，环境来自总线，没有这两个变量；generic 分发的运行里所有被启动的应用（含 Nautilus/Loupe）都带有。这两个变量是否由 AppRun 钩子设置无法与用户设置区分（原值没有备份）。应用均正常工作，没有新失败，所以按约定 **没有修**；用户可见后果（浏览器被强制浅色主题、在 Wayland 会话里被强制 X11）在真实桌面上未验证，保持 OPEN。
- **generic 分发的回退**：未注册类型在 generic 分发下回退到浏览器（Ubuntu：`x-www-browser` → Epiphany；Fedora：`x-www-browser: command not found` 之后由 xdg-open 自己的浏览器探测直接运行 Firefox），在 GNOME 分发下由 `gio` 原生拒绝（见 R4）。
- **Engine 在没有默认路由的主机上启动失败（`engine error 1101`，p2p 绑定）**：`engine-default-route/` 四种网络对照（bridge 通过、内部网络无默认路由失败、内部网络加默认路由通过、`--network none` 失败），原始守护进程日志在各子目录。**这是夹具要求（运行器加默认路由），不是本片修复的产品问题**；离线（无默认路由）启动失败是否可接受是 Engine/守护进程的 OPEN 问题。
- 真实 Nautilus 在 generic 分发下的 `GLib-GIO-CRITICAL`（`g_app_info_get_commandline`）出现在无 GUI 的宿主对照里，是宿主应用自己的输出。

### 保留的失败与过程

`dev1`（`--network none` 下守护进程启动失败；Epiphany 的 bwrap `pivot_root` 被拒）、`dev2`（bwrap 无法挂载 `/proc`）、`dev3`（`xdg-open` 经管道阻塞：真实应用继承描述符，`subprocess.run` 超时）、`dev4`（内部网络无默认路由，守护进程失败；Epiphany UA 断言错误）、`dev5`–`dev14`（通过的路径与观察器修订）、`dev7-fedora-baseline-appimage`（旧包控制组的第一次）、`final-b3aca3d11`（首个最终运行：generic 两组因宿主对照 `subprocess` 等待前台 `xdg-open` 而失败，其余通过）、`xdg-open-generic-diag-ubuntu`（变量传递错误，两组都是 generic）。诊断脚本 `diag_engine_default_route.sh` 的第一次在 Bash 3.2 下因空数组失败，其目录只有一个 `network-create.txt`，我随后删除了该目录并重跑（同名目录的失败原始输出只存在于终端，已无法恢复；遗留的 Docker 内部网络 `uc17c11-diag-12219` 已用精确名称移除）。

驱动 `run_17c11.sh` 在最终运行之后做过一次编辑（审计发现）：`UC_OLD_APPIMAGE` 不再有绝对路径默认值，驱动按各阶段结果设置退出状态。最终运行使用的是编辑之前的驱动；该编辑只改变默认路径与末尾的退出状态，只做了语法检查（`bash -n`），没有重跑。运行器没有在最终运行之后改动。
