# Go GUI Linux 托盘菜单刷新（17c14）

状态：已实现并验收（范围与未验见文末）。

## 写入顺序（如实记录）

1. 先读 `handoff.md`/日志，阅读 Wails 与 `tray_devices.go` 源码，形成候选根因（仅在对话中，未落盘）。
2. 运行最小探针 `probe1`（`G_DEBUG=fatal-criticals`，`update` 模式）取得真实栈。**此时本契约尚未落盘。**
3. 协调者指出契约缺失后，才写下本文件。因此本契约晚于 `probe1`；`probe2` 与此后的所有生产代码改动都在契约之后；`probe1` 的原始崩溃证据保留，不覆盖。

## 现象

17c13 的每次 Linux 运行（原生主机与容器）都在 `gui.log` 中记录 GTK3 断言：
`gtk_container_foreach` / `gtk_menu_shell_insert` / `gtk_menu_item_set_submenu`，约每 10 秒一组，与 `deviceMenu` 的 10 秒刷新周期一致。

## 失败模型（候选，待探针证实）

- Linux 托盘由 Wails 的 StatusNotifierItem + dbusmenu 渲染；菜单结构只在 `SystemTray.SetMenu` 时被快照进 dbusmenu 布局。
- `tray_devices.go` 用 `Menu.Update()` 刷新。在 Linux 上它与托盘无关：它为根菜单新建一份 GTK3 `GtkMenuBar/GtkMenu`（`menu_linux_gtk3.go`），并在调用方 goroutine（非 GTK 主线程）里清空、重建。
- 第二次调用时，`menuClear(root)` 销毁根下的 `GtkMenuItem`，连带释放子菜单的 `GtkMenu`；随后 `processMenu(submenu)` 对已释放的 `native` 调用 `menuClear`，触发断言（释放后使用）。
- 推论 1：CRITICAL 是真实缺陷，不是无害噪声。
- 推论 2：设备子菜单在 Linux 上的更新从未到达 dbusmenu 宿主（新增/清空后的条目没有 `impl`），即功能缺失。

## 验收契约

必须保留：设备子菜单随 10 秒刷新与 `devices://sync-changed` 更新；条目动作（点击触发回调）；连接状态/同步开关标签刷新；托盘生命周期（注册、退出）。不得：隐藏日志、删除刷新、删除托盘功能、`G_DEBUG` 之类屏蔽。

1. 探针（最小）：`update` 模式在 `G_DEBUG=fatal-criticals` 下复现崩溃（已得 `probe1`，Go 栈见 `menuClear`←`processMenu`←`Menu.Update`）。
2. 探针：`settray` 模式在同样严格条件下 16 秒无 CRITICAL，宿主观察到子菜单每次重建后的新条目，`Probe action` 点击触发回调，`Probe quit` 点击后进程以 0 退出。
3. 真实产品链（gui-go E2E 二进制 + 真实 Rust daemon，Xvfb/X11 容器）：宿主读取 dbusmenu，设备子菜单反映 daemon 配对设备及其同步开关；点击设备项后 daemon 状态翻转；点击退出项后进程退出；日志 0 条与托盘相关的 CRITICAL。
4. native Wayland 与 X11 各一条：真实机 omarchy（Hyprland）、VM fedora（niri），用同一观察器在任务目录/私有总线内运行；无托盘宿主的主机先记录缺口，由观察器提供标准 StatusNotifierWatcher（来源标注为观察器，而非桌面自带托盘）。
5. 回归：非 Linux 构建的刷新路径不变（macOS/Windows 仍用 `Menu.Update`）；`go vet`、`go build -tags gtk3,production,release`。
6. 无关 CRITICAL（如 `gdk_seat_get_keyboard`、`gdk_window_get_state`）单独归类，不与托盘混淆，也不声称已修。

## 证据分类

源代码 / 容器构建 / Docker(Xvfb、Weston 容器) / VM(fedora) / 真机 (omarchy) 各自标注，不互相替代。

## 实现

- `apps/gui-go/tray_publish_linux.go`：Linux 上结构变化通过 `SystemTray.SetMenu`（Wails 固定版本 `v3.0.0-beta.28`）发布，由其在主线程重建 dbusmenu 布局并发出 `LayoutUpdated`。
- `apps/gui-go/tray_publish_other.go`：macOS/Windows 仍调用 `Menu.Update`，但发布回调现在在 `deviceMenu.mu` 与 `trayMenu.mu` 持有期间执行，且 `trayMenu.setLanguage` 的加锁方式有改动；这是对这两个平台托盘路径的真实改动，**本片只做了 `vet`/编译，没有重跑 macOS/Windows 托盘 E2E**。
- `apps/gui-go/tray_devices.go`、`tray.go`：发布回调在持有 `deviceMenu.mu` 与 `trayMenu.mu` 时调用，保证本项目内没有 goroutine 在平台读取菜单期间修改它；锁序固定为 `trayMenu.languageMu` → `deviceMenu.mu` → `trayMenu.mu`；`trayMenu.setLanguage` 整体由 `languageMu` 串行化，并在进入设备菜单前释放 `trayMenu.mu`，因此并发调用不会让根菜单与设备子菜单停在不同语言（独立审查发现的缺陷，已修复）。
- 并发契约（固定源码已核对）：Wails 的菜单点击经 `menuItemClicked` 通道到独立 goroutine，再 `go m.callback`，主线程不会取这两把锁，因此持锁等待 `InvokeSync` 不会与主线程互等。`publish` 在 `initTray` 中先于刷新 goroutine 与事件处理注册赋值，首次渲染必然看到它。
- 残余（Wails 内部，非本项目可控）：`MenuItem.handleClick` 对复选项的自动翻转与 `SetMenu` 的读取之间没有同步；点击后 `deviceMenu.click` 本就把状态恢复为已存状态。
- E2E 构建专用钩子 `e2eTrayLanguage` 记录每次 `set_tray_language` 调用，生产构建中为空函数。

## 证据（来源分开列出）

观察器（`apps/gui-go/e2e/linux/tray_probe/sni_host.py`）是标准 StatusNotifierWatcher 加 dbusmenu 读取方，**不是桌面外壳**；两台主机上真实的 quickshell 托盘在用户的会话总线上，本片未使用，**桌面原生可见托盘未验证**。

| 层 | 证据 |
| --- | --- |
| 源码 | 失败机制由固定 Wails 源码与探针栈共同确定：`Menu.Update` → GTK3 `processMenu` → `menuClear` 对已释放的子菜单调用 |
| 容器（Docker，Ubuntu 24.04 aarch64，Xvfb + 私有总线，真实 gui-go E2E 二进制 + 真实 Rust daemon，生产 rendezvous 配对） | 探针 `probe1` 栈（`G_DEBUG=fatal-criticals`）、`probe2`；产品对照 `e2e4`：修复前 `before` 24 项中 11 项失败（对端条目始终不出现、点击无效、菜单 CRITICAL 114 条），修复后 `after` 24 项全过 |
| VM（Fedora 44，niri 25.11，aarch64） | 最终包 `ed3f389d…`：原生 Wayland 3 次、X11（XWayland）1 次，各 17 项全过 |
| 真机（Omarchy，Hyprland 0.56.1，aarch64） | 同一最终包：原生 Wayland 3 次、X11 1 次，各 17 项全过 |

- 修复后容器 `after`：对端经 10 秒刷新出现在宿主读取的设备子菜单；点击后 **daemon 自己的** `member/<id>/sync-preferences` 变为 `send=false/receive=false`，再点恢复；全局 `syncEnabled` 同样由 daemon 读取核对；`member list --json` 证明 A、B 互为已配对（设备 id 交叉一致）；中文切换整菜单更名；退出项使 GUI 以 0 退出、daemon 停止、托盘项从宿主消失。
- 主机运行不配对对端，设备子菜单为占位项；占位项每个周期也会被重建，因此同样走被修复的路径；**主机上的真实设备行未验证**，仅容器有。
- `before` 对照构建不含后来加入的语言调用钩子，其 `7b` 前置条件失败是观察能力缺口，不是旧产品新增故障。
- 无关的 `Gdk-CRITICAL gdk_window_get_state`（窗口显示路径，非托盘菜单）在容器 `after` 与 X11 运行中仍出现 1 条，**未修复、未归因**；原生 Wayland 运行 0 条 CRITICAL。`gdk_seat_get_keyboard` 属 17c13 另一类，本片未覆盖。

## 语言切换的一次失败（nat5-native，已保留）

最终包之前，用早期包（`8bfafc99…`，无语言调用钩子）在 Fedora 原生 Wayland 上跑过若干次，全部保留：`nat1` 因部署缺依赖模块未启动；`nat2` 的 daemon 停止检查因 `daemon.conn` 在 daemon 停止时被删除而取不到 PID（探针缺陷）；`nat3`、`nat4` 的标准输出里各项检查（含中文切换）都显示通过，但观察器自身在总线关闭时被 GDBus 的 `exit-on-close` 发出 SIGTERM（rc=143），结果文件为空，因此它们不是完整的通过记录；`nat5` 是观察器修复后的第一次完整运行：`set_tray_language(zh-CN)` 返回成功，但宿主 30 秒内菜单始终是英文，该项失败，其余通过（X11 的 `nat5-x11env` 通过）。能证实的：前端 `SettingContext` 在语言设置加载时也会调用 `set_tray_language`（最终包的钩子记录到启动期有两次 `en-US` 调用）；当时没有记录钩子，无法证明那次失败正是被前端晚到的调用覆盖。已采取的措施不是重跑取绿：加入 E2E 钩子，让运行器先等前端的初始调用出现再发 `zh-CN`，并断言最后一次调用就是测试的 `zh-CN`。**`nat5` 的确切原因仍是推断，未证明。**

## 未验

桌面外壳（quickshell 等）里托盘的真实绘制与点击；GNOME/KDE 托盘宿主；真实设备行在主机上的呈现；多输出/其他缩放；非 aarch64；Windows、macOS 的托盘行为与上述加锁改动的回归（仅 `vet`/编译通过；macOS 既有 `tray_devices_run.py` 未重跑，持锁等待主线程的互等分析只对 Linux 做了源码核对）。

## 包与来源标识

- 最终包源提交 `aaa1417bcce1097d1eae366e0b1b95e9da5ede4d`（manifest `dirty=false`、`immutable=true`），AppImage SHA-256 `ed3f389dd86b2a47d8fc57fb2c48d6fff5641a0a5b97b58a60068a61cf04c734`，E2E 前缀包（`productionUsable=false`），内置 pinned release daemon `ea0f0bcb…`（与 manifest 一致）。此后的提交只含文档，不改变该包。
- 容器 `e2e4` 使用的 daemon 与 CLI 是 `/cache/out` 中较早构建的 debug 版（来源提交记录在工件 `e2e4/daemon-cli-built-from.txt`），不是 pinned release daemon；主机运行使用包内 release daemon。
- PR 上下文审计由本会话的 `general-purpose` 只读子代理按审计提示词逐字执行（本会话没有 `context-update-reviewer` 类型），结论 `NO_UPDATE`；分支守卫 `OK_MEANINGFUL`。

## 独立只读源码审查（范围 `de2257e15..HEAD`）

CodeRabbit 在本 PR 上是 `Review skipped`（基线不是默认分支），**没有审查**；PR 上没有任何评审。以下是本会话派出的独立只读子代理的审查，不是 CodeRabbit，也不是人工评审，且它没有运行 Go、Docker 或竞争检测器，只读固定 Wails 源码与工件。

- 锁序、`InvokeSync` 与主线程互等：未发现缺陷。Linux 托盘菜单点击由 `gtkDispatch(item.handleClick)` 分发，回调再在 `go` 中执行，主线程不会取 `deviceMenu.mu`/`trayMenu.mu`；`setMenu` 只取 Wails 的 `itemMapLock`。（此前文档说点击经 `menuItemClicked` 通道，对 SNI 路径不准确，已改正；结论不变。）退出时若有发布正卡在 `InvokeSync`，它持有两把锁，但 `shutdown` 不取这两把锁，退出不会因此挂起。
- 发现并修复：`setLanguage` 并发调用时根菜单与设备子菜单可能停在不同语言（新的 `languageMu`）；`publish` 字段赋值未在 `deviceMenu.mu` 下进行（已加锁）；运行器“≥3 个刷新周期”的睡眠公式与断言不相称（容器运行改为以托盘注册时刻起算；原生运行改为统计宿主实际收到的结构性重发布次数 ≥3）；本文档对 `nat2`–`nat4` 的描述不准确（已改正）。
- 未验证或仍存在：macOS 上 `NSMenu` 正在跟踪时 `InvokeSync` 是否被及时服务（若阻塞，`deviceMenu.click` 会在 `deviceMenu.mu` 上等到菜单关闭）；Windows 的 Win32 线程亲和；E2E 控制 `e2e_controls.go` 读取菜单不加锁（仅 E2E 构建）；Wails 内部 `handleClick`/`item.impl`/`checked` 的无同步读写依旧存在，但本项目内的写入现在都与 `SetMenu` 串行，且 `deviceMenu.click` 会恢复已存状态、下一次渲染 10 秒内纠正。

## 契约补充：独立审查后的新增失败模型与验收（落盘先后如实记录）

**先后**：独立审查（对 reviewed HEAD `7d996b169`，工件 `review-7d996b1/`，只读子代理，非 CodeRabbit、非人工）完成后，提交 `3fbe2c2bf` **先** 修了 `setLanguage` 串行化、`publish` 赋值加锁、运行器的周期观察，**之后** 才写下本节。因此本节晚于这些修复；其中真实产品 E2E 的并发语言验证与对照构建在本节之后才加入。

新增失败模型：
- F-A `setLanguage` 并发交错：A、B 同时调用，根标签取后者、设备子菜单标题取前者，最终两处语言不一致。预期：并发调用结束后，宿主读到的根菜单与设备子菜单标题语言一致，且等于某一次调用的语言，`trayMenu.language` 与之相同。
- F-B `publish` 赋值与首次渲染的先后：预期首次渲染的发布必然可见，即第一次结构性重发布在宿主观察到的布局里有设备子菜单的更新；回调字段的读写都在 `deviceMenu.mu` 下。
- F-C 周期证据：不得仅凭睡眠时长推断“跑过 ≥3 个周期”。预期：原生运行统计宿主实际收到的结构性重发布次数；容器运行在对端行不变时无法按布局计数，改为要求设备行真实出现（刷新路径被走通）与睡眠自托盘注册起算，并把这一限制写明。

验收（真实产品 E2E，不写单元测试）：E2E 构建新增控制动作并发触发多次 `set_tray_language`，再由宿主读取 dbusmenu；并以去掉 `languageMu` 的对照构建检验该检查能否发现 F-A（若对照构建在 N 次迭代内未复现，如实记为“检查未能区分”，不据此宣称检查有效）。
