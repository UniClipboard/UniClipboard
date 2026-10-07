# Go GUI macOS 托盘菜单跟踪与刷新锁（17c15）

状态：失败模型与验收契约（先于任何探针、E2E 脚本与生产改动落盘）。结果与边界在文末「结果」节随证据补写。

## 写入顺序（如实记录）

1. 核对 git/gh：PR #1891 HEAD `d62bd579f`，CI 全绿，CodeRabbit "Review skipped"（未审查）。从该 HEAD 建本地分支 `test/gui-go-macos-tray-menu-tracking-lock`，父 PR #1891/#1890/#1889/#1885/#1884 全部保留、均未合并。
2. 阅读固定 Wails `v3.0.0-beta.28` 的 macOS 源码（`menu.go`、`menu_darwin.go`、`systemtray_darwin.go/.m`、`mainthread_darwin.go`、`mainthread.go`、`menuitem.go`），得出下面的候选失败模型。**此时没有任何探针或测试运行，也没有生产改动。**
3. 查到宿主能力：本机进程具备 Accessibility 与 Screen Recording（`AXIsProcessTrusted` 为真），可用 AX 读取并按压真实 `NSMenu`，不需要移动真实指针。
4. 本文件落盘，然后才写探针。

## 17c14 之后 macOS 上改变了什么

17c14 在 `tray.go`/`tray_devices.go` 加了锁，并让发布回调 `republishTrayMenu` 在持有 `deviceMenu.mu` 与 `trayMenu.mu` 期间调用。Linux 走 `SystemTray.SetMenu`；macOS 仍走 `Menu.Update()`，其实现是：

```text
Menu.Update -> macosMenu.update -> InvokeSync(main thread):
    clearMenu(nsMenu)            // [menu removeAllItems]
    processMenu                  // 销毁旧 NSMenuItem，重建每个 item 与子菜单
```

即 **同一个 `NSMenu` 实例**（状态栏项使用的那个）被原地清空并重建；且发布方在主线程完成前一直持有两把锁。这条路径在 17c14 只做过 `vet`/编译。

## 源码事实（固定 Wails，已读）

- `InvokeSync` 在 macOS 经 `CFRunLoopPerformBlock(main, kCFRunLoopCommonModes)` 投递，源码注释明确说明这样是为了在菜单/模态对话期间仍被送达。**这是源码意图，不是实测。**
- 托盘左键在本项目里有 `OnClick` 处理器（`h.showMainWindow`），所以 `systrayPreClickCallback` 对左键返回 0（不弹菜单）；右键无自定义处理器，走原生菜单跟踪（`showMenu`，阻塞直到关闭）。
- 菜单项点击：主线程 `handleClick` → `processMenuItemClick` → 通道 `menuItemClicked` → 独立 goroutine 执行 `handleMenuItemClicked`；因此回调不在主线程，不取 `deviceMenu.mu`/`trayMenu.mu` 也不会与持锁的发布互等。
- `MenuItem.handleClick` 对复选项先翻转 `checked`；`deviceMenu.click` 随后把它恢复为已存状态。

## 失败模型（候选，待真实 NSMenu 跟踪证实或证伪）

- **M1 打开菜单时的定时刷新**：用户保持托盘菜单打开，10 秒定时刷新触发 `Menu.Update`，在跟踪中的 `NSMenu` 上执行 `removeAllItems` 并重建。可能结果：(a) 菜单原地正确更新；(b) 跟踪中的菜单显示陈旧/空白/丢失高亮；(c) AppKit 抛出异常或进程崩溃；(d) 跟踪中点击落到已销毁的旧 `NSMenuItem` 而无效。需用真实 `NSMenu` 跟踪区分，**不得** 用脚本直接调用回调代替。
- **M2 主线程等待与锁序**：发布方持 `d.mu`+`t.mu` 调 `InvokeSync`；若跟踪期间主线程投递没有被送达，发布与所有等这两把锁的 goroutine（刷新循环、`setLanguage`、`setSyncEnabled`、`reserveSync`、`click`）会一直阻塞到菜单关闭。预期：投递在跟踪期间被送达，发布耗时为毫秒级。需测量发布起止时间，并记录是否阻塞到菜单关闭。
- **M3 设备子菜单与语言变更**：菜单打开时（含子菜单展开时）语言变更与设备结构变化触发重建；预期整菜单（根与子菜单）在同一语言，且子菜单条目与 daemon 的已配对设备一致。人工调度的并发语言变更须标注为人工。
- **M4 动作与 daemon 权威状态**：通过真实 AX 按压菜单里的设备复选项后，**daemon 自己的** `member/<id>/sync-preferences` 翻转；菜单勾选状态在刷新后与 daemon 一致；再按压恢复。全局同步开关同样以 daemon 的 `syncEnabled` 核对。
- **M5 退出**：真实按压 `退出` 项（以及轻量模式项）后，GUI 进程按预期退出；完整退出时 daemon 也停止，轻量退出时 daemon 保留（由编排器随后停止）。

## 验收契约

必须保留 17c14 的行为：设备子菜单随 10 秒刷新与 `devices://sync-changed` 更新、条目动作、同步开关标签、托盘生命周期。不得：隐藏日志、删除刷新、删除托盘功能、降低锁保证、用 compile/vet 或脚本直接调用代替原生证据。

1. **最小真实链**：复用 `apps/gui-go/e2e/tray_devices_run.py` 的 GUI + 真实 daemon + 生产 rendezvous 配对（独立 profile/HOME/端口，不碰真实 profile/keyring/历史/`generalPasteboard`）。再在其上用 AX 取得 `NSStatusItem` 的 `NSMenu`：先观察（只读），再按压。
2. **打开期间刷新（M1/M2）**：AX 打开菜单（`AXShowMenu`/等价原生入口），保持打开跨过至少一次自然定时刷新，读取每次 AX 菜单结构与项目状态；E2E 构建的钩子记录每次发布的起止时间戳（钩子必须被验证确实触发：计数随定时刷新增长）。观察三类：菜单内容是否保持完整、发布耗时、进程是否仍存活。
3. **子菜单与语言（M3）**：展开设备子菜单后保持打开，经 E2E 控制触发语言变更（标注为“人工调度”并验证钩子触发），AX 读回整菜单语言；自然对照没有区分度时如实保留。
4. **动作（M4）**：AX 按压设备项和同步开关，读 daemon 状态核对；菜单布局仅作菜单证据。
5. **退出（M5）**：AX 按压退出，核对 GUI 与 daemon 终态，用精确 PID/handle，不用模式匹配。
6. 若发现缺陷：窄修复，并在修复前后各留一条同样的真实 E2E 对照；若未发现缺陷：仍交付可复跑入口、文档和 PR，如实写明“未发现”及其边界。
7. 不执行 `uniclipd --version`；不停止用户自己的 app/helper；不改全局代理/默认程序/快捷键/自启/系统配置；截图与日志去除秘密与原始内容。

## 证据分类（预先声明）

源码推断 / 真实 AX + NSMenu 跟踪实测 / daemon 权威状态 / 人工调度 / 自然对照 各自标注，不互相替代。Windows 真实托盘仍是 OPEN。

## 结果

（待补）
