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

## 运行器失败记录与新增模型（落盘先于诊断改动）

- **base1**（保留，`passed=false`）：`tray_devices_run.py` 的英文断言失败，根菜单为中文。证据：驱动调用序列 `en`（驱动）→ `zh-CN`（前端），本机语言为 `zh-Hans-CN`。契约是：前端在 `SettingContext` 的语言副作用里于启动与设置加载时调用 `set_tray_language`，驱动的英文固定值只有在前端静默之后才不会被覆盖；旧运行器没有这个前置条件。这是 **运行器前置条件缺陷（语言/时序），不是产品缺陷**；来源核实仍为：Rust daemon 与 `target/debug/uniclipd` 为共享缓存路径产物，base1 的来源标为“探索，未核实”。
- 修复（测试侧）：驱动在固定英文前等待前端静默（`tray-language-quiet`，E2E 控制），运行器按证据行顺序断言“英文固定值是读菜单前最后一次语言调用”。
- **base2**（保留，`passed=false`）：静默步骤已记录、`tray-publish` 钩子每 10 秒触发一次（钩子确实触发，`durMs`≈0），但驱动此后没有产生 `tray-language-call en`，也没有 `driver-error`；运行器在截止时间后失败于 `GUI did not exit cleanly`。`set_tray_language` 的第一行即 e2e 钩子，所以调用没有到达宿主。
- **R1（待证实）**：静默控制返回后，驱动 JS 或其对宿主的调用被卡住。候选：(a) WebView 脚本在窗口不可见时被节流；(b) `Control` 调用的响应没有回到 JS；(c) 调用已发出但在宿主入口前阻塞。区分办法：驱动在每个 await 前后写进度记录（E2E 驱动代码），不改产品。

- **base3**（进行中时的观察，运行器终态另记）：驱动进度记录 `before-quiet`、`after-quiet` 均写出，静默控制 `ok=true`，此后仍没有 `tray-language-call en`，定时发布继续（约 10 秒）。一次只读 `sample`（采样时 3 秒）看到主线程停在 RunLoop 的 `mach_msg`，**只能说明采样时未观察到主线程被锁住；不能排除间歇阻塞或其他 goroutine 在等待，也不能证明这是卡点的唯一原因**。
- **R2（待证实，R1 的细化）**：`after-quiet` 之后驱动对 `main.HostService.Invoke("set_tray_language")` 的调用在到达 `set_tray_language` 处理器之前停住（处理器首行即 e2e 钩子，未触发）。候选：(a) JS 调用没有发出；(b) 调用发出但 Wails 绑定调用分发/传输未到宿主；(c) 宿主 `Invoke` 入口前被阻塞。区分办法（E2E 专用，不改产品行为）：驱动在调用前后写进度记录并在 3 秒后记录“仍挂起”；宿主 `Invoke` 入口的 E2E 钩子记录 `set_tray_language` 的进入与返回。三者组合即可区分 (a)(b)(c)。未证实之前不下结论，也不据此改产品代码。

- **base4（保留）修正 R2**：加了宿主入口钩子后，`set_tray_language("en")` 这次 **到达了宿主并返回**（`invoke-enter` → `tray-language-call en` → 发布 4 → `invoke-return`），但驱动既没有 `call-returned`，独立的 3 秒定时器记录 `call-pending-after-3s` 也没有出现。所以 base2/base3 的“调用没有进入宿主”只适用于那两次运行，**不能外推到 base4**；三次运行共同的现象是：驱动 JS 在大约 10–15 秒后不再产生任何记录。已证实的只有这一现象；原因（WebView 脚本被节流或挂起、WebView 到宿主的调用传输停滞、或长时间阻塞的 `Control` 调用之后的传输状态）**未确立**，也没有证据表明与托盘菜单刷新有关（`tray-publish` 一直按 10 秒周期、`durMs`≈0）。
- **R3（待证实）**：驱动因“前端静默”这个长阻塞控制调用（至少 5 秒）而在其后停滞。区分实验（只改驱动）：用驱动自己的 `sleep` 等待，不发起长阻塞的宿主调用；若不再停滞则指向长阻塞调用；若仍停滞则指向 WebView 脚本在无焦点/不可见时的行为。无论结果，本片的真实 NSMenu 跟踪验收 **不依赖** WebView 驱动：新运行器通过控制文件（宿主侧文件监视，不经 WebView 传输）与 AX 驱动，配对和读取 daemon 状态通过 CLI。

- **R4（候选，未证实）：窗口可见性/App Nap/页面节流**。这只是候选解释：宿主已返回且 3 秒定时器记录缺失，不能单独证明 JS 定时器全部停转，也不能排除其他响应问题；`e2e_timer_probe.go` 与 `app_nap_run.py` 的既有结论是“没有无特权 API 能报告进程是否处于 App Nap”，本片同样不声称能。对照实验（同一调用链：配对、静默控制、`set_tray_language("en")`）：条件 A 为默认（E2E 默认的安静模式），条件 B 为 `UC_GUI_GO_E2E_VISIBLE=1`（复用既有开关，其含义是不扭曲真实指针，不等于窗口前置）。两种条件都记录：宿主每 2 秒的主窗口原生状态（存在/可见/最小化/聚焦，来自 Wails 窗口 API）、驱动每秒一次的 JS 心跳（序号、`performance.now()`、`document.visibilityState`、`hasFocus`）、宿主入口与返回钩子、调用返回记录。判读规则预先写明：心跳持续而调用不返回 → 传输/响应问题；心跳中断且宿主返回正常 → JS 被节流或挂起（仍只是与 App Nap/可见性相符，不是证明）；两条件结果相同 → 该因素不是区分变量。不改全局 App Nap 设置，不用永久禁用产品省电换取通过。
- **两个结论分开**：(1) 隐藏 WebView 驱动的基线条件（本节：测试驱动是否在无焦点/隐藏时停滞）；(2) 托盘菜单锁与主线程等待在真实 NSMenu 跟踪下的行为（M1–M5）。(1) 不改变 (2) 的验收要求，(2) 不因 (1) 的任何结果而降低。

- **ctlA-hidden（保留，终态 `passed=false`）**：原生状态全程 `visible=false/focused=false/minimised=false`；驱动心跳 `n=1..4`（JS 时间 3.1–6.7 秒，间隔约 1.17–1.2 秒，`visibilityState=hidden`），这 4 次心跳都发生在 `tray-language-quiet` 控制调用挂起期间——即长阻塞的宿主调用没有挡住 JS 计时器和 `record` 传输（R3 的“长阻塞调用”解释因此被削弱，但不排除其他原因）；`n=5` 及之后没有，`call-start` 之后的 `set_tray_language("en")` 没有进入宿主。观察窗口：驱动启动后约 7 秒内。
- **ctlB-visible（保留，条件无效）**：`UC_GUI_GO_E2E_VISIBLE=1` 已传入 GUI 进程环境（以进程环境核实），但该开关在代码中只控制 `quiet()`（不扭曲真实指针），**不控制窗口显示**；26 条原生状态没有一次 `visible=true`，心跳 `visibilityState` 全为 `hidden`。因此 B **不是** 可见条件，不能据此归因或排除 App Nap/页面节流；它只证明“该开关不是可见性控制”。
- **修正（失败模型 R4 的测试控制）**：可见条件改为运行器经 E2E 控制文件触发窗口显示（只 `Show`，不 `Focus`），并要求原生状态实际出现 `visible=true` 才算条件成立；同时记录触发前后系统最前台应用，核对是否抢占了前台。条件未成立（无 `visible=true`）则对照作废，原样保留日志。

- **ctlC-shown（保留）与“可见”的实际含义**：`show-main m1` 被控制文件消费、进入宿主并返回（`show-main-m1`，`ok=false`，`visible=false`，`focused=false`），调用前后系统最前台应用相同（`lsappinfo front`，`ASN:0x0-0x1001`）。源码核对（固定 Wails）：`Window.Show()` = `InvokeSync(makeKeyAndOrderFront)`；`IsVisible()` 读的是 `NSWindow.occlusionState & Visible`（被遮挡状态），不是“已 orderFront”。E2E 默认的安静模式（`quiet()`）把 app 设为 Accessory 策略且窗口建在 (-20000, -20000) 屏外，所以 C 里 `Show()` 是在屏外执行的，`visible=false` 意味着 **“可见条件未成立”**（窗口被遮挡/在屏外）。它 **不能** 说明窗口可见性与调用链无因果；三次都隐藏的条件（A、B、C）也排除不了可见性的影响。B（`VISIBLE=1`）关闭了安静模式但从未 `Show`；C 执行了 `Show` 但仍在安静模式。**没有任何一次对照同时满足“窗口在屏幕上且被 orderFront”**，需要 `--visible --show-main` 两者同时（该条件会在测试者桌面上显示窗口，且需验证原生 `visible=true`）。
- **心跳模式（仅为观察）**：A（n=4）、B、C（n=5）的驱动 JS 心跳都在 JS 时间约 6.5 秒、`tray-language-quiet` 返回前后停止；C 中心跳停止后，由宿主调用驱动的步骤（`call-returned`、`tray-menu-initial`、`tray-device-listed`）仍继续出现。这只是模式，**不是根因**；三次运行不足以推断间歇性的根因。
- **下一步最小可检验模型（R5）**：窗口同时在屏幕上且 orderFront（原生 `visible=true`，且条件在长跑前即时核实）后，若心跳仍在约 6.5 秒停止，只证明“使窗口可见不足以消除本次停顿”，**不能** 断言与窗口可见性无关（可能有其他共同原因、多因素、或显示后历史状态）；若心跳持续，只支持“与可见性相关”，仍未排除其他变量。无论结果，托盘锁与 NSMenu 跟踪的验收不依赖 WebView 驱动。

## 菜单打开方式与新增失败模型（落盘先于最小 open/read/cancel 运行）

- **AX 能力实测（ax2，保留）**：本应用状态栏元素为 `AXMenuBarItem`（`AXMenuExtra`），菜单关闭时没有子元素，**唯一动作是 `AXPress`**；`AXShowMenu` 返回 `-25206`（动作不受支持，`ax1`/`ax2` 的失败原样保留）。`AXPress` 对应左键，而本应用给托盘设置了左键处理器（`OnClick` → 显示主窗口），Wails 的 pre-click 监视器因此对左键返回 0、不进入原生菜单跟踪；只有右键（无自定义右键处理器）进入原生菜单跟踪。**不为测试改变生产的 `OnClick` 语义。**
- **两条打开路径及其覆盖差别（均为真实 AppKit 菜单跟踪，均不经菜单回调或脚本快照）**：
  1. `control`：E2E 控制文件触发 `SystemTray.OpenMenu()`，即 Wails 固定版本的原生入口（`showMenu`：在主线程合成按钮鼠标按下，进入跟踪循环直到菜单关闭）。**由 E2E 控制触发，不是真实右键输入**；不动指针；覆盖：NSMenu 跟踪循环、重建、主线程等待、AX 读取与按压；不覆盖：pre-click 监视器的右键路由、真实鼠标事件的投递。
  2. `rightclick`：真实右键事件投递到 **本轮自己 GUI 进程** 状态栏元素的 AX 位置（取自 AX 的 `AXPosition/AXSize`，事件后恢复指针）；覆盖路由与真实输入；会短暂移动真实指针，因此单独成一条路径并单独记录。
- **模型 O1（OpenMenu 的返回与控制写入不是成功证据）**：`OpenMenu()` 内的 `InvokeSync` 只投递 `dispatch_async` 后即返回，真正的跟踪在随后的主线程块里开始并阻塞到菜单关闭，所以控制步骤的写入时刻 **早于** 跟踪真正生效，记录顺序可能误导。成功标准只有一个：**AX 在限时内真的读到了打开的 `AXMenu` 子树**；没有读到则该次打开失败，不论控制是否写入、`OpenMenu` 是否返回。
- **模型 O2（跟踪期间控制链与主线程）**：主线程在跟踪循环内；控制文件监视在 goroutine 中、`invoke` 走宿主侧 `Invoke`，不依赖主线程，故在跟踪中仍可触发语言变更与读取；但发布（`InvokeSync`）要等主线程服务投递——这正是 M2 要测的。控制链不负责关闭菜单：关闭由 AX `AXCancel` 完成，运行器在任务内设超时，并在清理中先取消菜单再终止本轮自己的 GUI 进程。
- **最小模式先行**：先做“打开→读→取消→确认菜单已消失”，确认真实跟踪发生，再扩展到刷新/语言/动作/daemon/退出矩阵；最小模式未通过则后续矩阵不得称完成。

- **min3-control 观察（保留）与模型 S0（状态栏项缺失）**：本轮目标 GUI（pid 87071）的 `AXExtrasMenuBar` 读到 0 个状态栏项（之前的 ax1/ax2/min1/min2 为 1 个，同一 `AXMenuBarItem`/`AXMenuExtra`），运行 2 分钟后仍为 0，所以不是启动时序的瞬时竞争。候选：(a) 状态栏项没有被创建；(b) 菜单栏空间不足或用户菜单栏设置使该项未显示；(c) AX 列举失败。区分办法：步骤 0 对 `items` 做 30 秒内重试，失败时立即采集诊断（`describe`、`scan`、菜单栏局部截图、进程 `sample`）并 **快速失败**，不再带着缺失的目标继续长等。原运行器的缺陷：`check()` 只记录不抛出，步骤 0 失败后仍执行配对后的全部步骤，浪费数分钟并产生不可解释的后续噪声。

- **min3-control 终态与模型 S1（保留）**：运行器终态 `timeout waiting for invoke-en0`；精确 pid 核对时运行器 86933、GUI 87071 均已不在进程表（无存活、无 defunct），本任务无残留，daemon 清理列表为空。证据：启动后 `native-window-state` 只写出 2 条（正常约每 2 秒一条）、`tray-publish` 只有第 1 次（+3.4 秒，`durMs=7`，此后定时刷新的第 2 次从未出现）、`invoke-enter` 之后没有 `invoke-return`、AX 读到 0 个状态栏项、前端从未调用 `set_tray_language`（静默控制因此等满 60 秒）。这与“主线程在 bootstrap 后约 3 秒起不再服务派发”相符，但 **现场没有被抓取**（没有 `sample`），起因（宿主状态、模态阻塞、产品缺陷、测试环境）**未知**，不据此断言产品缺陷，也不断言与前面 WebView 驱动停顿相关。修正：运行器在任何控制步骤超时时先对本轮自己的 GUI pid 做 `sample` 与 AX 扫描再退出；记录 GUI 的退出码；在 provenance 记录采集时的宿主空闲时间与睡眠设置。

## 宿主条件：显示器休眠（混杂因素，已确认）

- **证据（只读）**：`CGDisplayIsAsleep(main)=true`、`CGDisplayIsActive(main)=false`、`CGDisplayIsOnline=true`；`pmset -g log`：`2026-10-06 22:48:55 -0700 Display is turned off`；`pmset -g`：`displaysleep 20`，`PreventUserIdleDisplaySleep 0`（系统睡眠被 `UURemote`、`caffeinate` 阻止，显示器睡眠没有被阻止）；`HIDIdleTime` 约 5800–6100 秒（min4/min6 的 provenance）。`screencapture -R` 失败（`could not create image from rect`），整屏截图可生成但裁剪区域为纯黑；`peekaboo list screens` 仍报告 3360×1890 Retina。
- **结论边界**：**纯黑截图无效**，不能据此得出“菜单不存在”或产品缺陷；`min4-control`/`min5-rightclick`/`min6-rightclick` 的 `ax-open-*-screen.png`、`diag-menubar.png` 作废（保留原件，不作证据）。本片自 base1 起的所有运行都发生在显示器休眠期间（`ctlA/B/C` 的窗口遮挡状态、驱动心跳在约 6.5 秒停止、`min3` 的主线程停滞、`min4–min6` 的 AX 读不到菜单），**显示器休眠是它们共同的混杂因素；是否是原因未证实**——WebKit/AppKit 在显示器休眠时的行为是候选解释，不是结论。
- **边界**：不更改用户的显示、锁屏、权限设置或无关应用。要在显示器唤醒的会话中验收原生 NSMenu 跟踪，需要使显示器处于唤醒状态（例如用户在场、或授权运行器在每轮运行期间声明一次瞬时用户活动断言）。**未获授权前，原生 NSMenu 跟踪验收标记为“宿主能力缺口：显示器休眠”，不冒称已验证。**

- **min7-control（保留；显示器已唤醒）**：授权的瞬时唤醒生效（`displayBefore.asleep=true` → `displayAfterWake.asleep=false`，会话未锁定：无 `CGSSessionScreenIsLocked`，在控制台，登录完成；唤醒进程在清理时被终止，返回码 -15）。控制路径 `OpenMenu` 之后，40 ms 间隔 AX 观察 323 次、0 次读到菜单；运行器当场裁剪的截图（图像有效、非黑）显示状态栏项正下方 **没有画出菜单**（仅作观察记录，见下）。**该截图同时拍到了无关应用的内容（浏览器页面等），已立即删除，不作存档证据**；此后截图默认不留存（只记录“已捕获”），除非显式设置 `TRAY_KEEP_SHOTS=1`。对旧失败的边界：min4–min6 发生在显示器休眠期间，其结论仍受该因素限制；min7 是第一次在唤醒显示器上的对照，**只对 `OpenMenu` 控制路径成立**，对真实右键路径还没有结果。

- **min8-rightclick / min9-control（保留；显示器已唤醒）与最窄假设 H1**：真实右键（本轮自己 GUI 的 AX 位置，`target` 在状态栏项中心）与 `OpenMenu` 控制两条路径都没有被 AX 读到菜单（322/0、min9 同）。min9 的 `sample` 窗口（3 秒，5 ms，**先于** 打开请求开始并覆盖它）显示主线程 434 次采样中 428 次空闲在 RunLoop、5 次在服务我们自己的 AX 查询，**没有** `showMenu`/`mouseDown`/菜单跟踪帧。这只说明窗口内主线程没有进入跟踪，不说明入口没被调用。**H1（待证实）**：Wails 的 `macosSystemTray.nsMenu`（或 `nsStatusItem`）在运行时为空，于是 `openMenu`（及 pre-click 回调中的 `systemTray.nsMenu == nil` 判断）直接返回，菜单无论由控制、右键都不会打开。若 H1 成立，这是产品缺陷（托盘菜单在 Go GUI 的 macOS 上打不开），需窄修；若不成立，改查 H2（`showMenu` 执行但跟踪未开始，如合成事件被忽略）。检验：E2E 构建在打开前后用只读反射记录该托盘的 `impl/menu/clickHandler/rightClickHandler/nsStatusItem/nsMenu` 是否为空，不改产品行为。

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
