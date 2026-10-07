# Go GUI Linux 托盘菜单刷新（17c14）

状态：契约已写；实现与验收见文末「结果」（随切片更新）。

## 写入顺序（如实记录）

1. 先读 `handoff.md`/日志，阅读 Wails 与 `tray_devices.go` 源码，形成候选根因（仅在对话中，未落盘）。
2. 运行最小探针 `probe1`（`G_DEBUG=fatal-criticals`，`update` 模式）取得真实栈。**此时本契约尚未落盘。**
3. 用户指出契约缺失后，才写下本文件。因此本契约晚于 `probe1`，但早于 `probe2` 之后的所有生产代码改动与 `probe2` 结论的使用；`probe1` 的原始崩溃证据保留，不覆盖。

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
