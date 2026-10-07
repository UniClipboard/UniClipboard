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

- **min10-control 结果（保留）：H1 被否定，`OpenMenu` 控制路径无效**：打开前后（只读反射）`impl`、`nsStatusItem`、`nsMenu` 均 **非空**（H1 不成立）；但 Wails 的 `SystemTray.menu`（Go 层字段）为 **空**，`clickHandler` 非空，`rightClickHandler` 为空。固定 Wails 源码：`SystemTray.OpenMenu()` 开头 `if s.menu == nil { return }`，所以 **`OpenMenu()` 在这里是空操作**，此前用它的 min1–min4、min7、min9、min10 的“打开”从未真正请求过菜单，**这条打开路径不能用于验收**。原因（源码）：`SystemTray.SetMenu` 在托盘已运行（`impl != nil`）时只调用 `impl.setMenu`，不设置 `SystemTray.menu`；因此 `applySmartDefaults` 的 `hasMenu` 为假，右键处理器保持为空，**生产右键因此走 pre-click 监视器返回 1、用缓存的 `nsMenu` 进入原生跟踪**（`systrayPreClickCallback`：右键且 `rightClickHandler == nil`）。即上文“真实右键调用 `ShowMenu`”的推断被源码与运行态读数 **更正**：生产右键不经 `ShowMenu`/`OpenMenu`。**唯一有效的打开方式是真实右键事件**，且此前对旧 `OpenMenu` 路径的“两条路径覆盖差别”表述作废。min5/min6/min8 的真实右键仍未读到菜单，尚无结论（见下一轮）。
- **注**：`OpenMenu` 的空操作是 Wails 在“托盘运行后再 `SetMenu`”场景下的行为；不影响用户右键，也不是本项目的产品缺陷；本项目没有调用 `OpenMenu`。

- **目标验证缺口（min5、min6、min8、min11、min13，保留）**：这几轮的真实右键/Peekaboo 点击都是对 AX 报告的状态项中心坐标的 **盲点击**——点击前没有验证该点的最上层窗口属于本进程。因此它们“没读到菜单”**不能** 归因于产品或菜单锁，只是“目标未验证”的无效对照；同时这些点击可能落在了别的应用的控件上（min13 的 Peekaboo 输出为 `App: Google Chrome`、`Mode: foreground`、`Coordinate space: global`，工具的 App 字段表示前台应用而非命中，不能据此推断应用收到了事件；它的 `peekabooItem: []` 是我自己的调用写错——`list menubar` 不支持 `--include-raw-debug`，列表请求失败——不是“我们的项不存在”的证据）。`Click successful` 只表示工具发出了事件，不表示应用回调收到。
- **一个需要核实的现象**：min7 的有效截图（已删除）在 AX 报告的状态项位置附近 **没有看到本应用的托盘图标**（图标为黑色猫剪影，`assets/tray-icon@2x.png`），这台机器是刘海屏且菜单栏很满，macOS 会隐藏放不下的状态项；这是候选解释，**未证实**。新增原生核对：窗口服务器对本 pid 的窗口列表（`windows`：层级、边界、是否在屏幕上、alpha）与点击点的命中测试（`hittest`：最上层窗口是否属于本 pid）。**新规则：命中测试不是本 pid 时不点击，立即失败并只读诊断**（`TargetNotVerified`）；Peekaboo 点击路径已从运行器中移除。

- **min14-rightclick（保留；目标未验证，未点击）与命中测试方法的更正**：运行器在点击前用窗口服务器做命中测试，结果该点最上层窗口属于 pid 626（层级 24，菜单栏本身），本 pid 的窗口列表只有 3 个且 **没有状态栏层级窗口**（一个层级 0 不在屏幕上、一个在 (-20000,-20000) 的快捷面板、一个层级 0 在屏幕上的 1099×719 窗口），所以 **没有发出任何点击**（`TargetNotVerified`）。结论：在这版 macOS 上状态项不是应用自己的窗口，窗口服务器的“最上层窗口”命中测试 **不适用**（那是我选错的门槛）；改用辅助功能的系统级“该坐标处的元素”（`AXUIElementCopyElementAtPosition`）及其所属 pid 作为点击前的目标验证；元素不属于本 pid 则同样不点击。

- **min15-rightclick（保留；目标未验证，未点击）与对旧右键的更正**：点击前用辅助功能的系统级“该坐标处元素”核对，(714,15) 处的元素属于 **pid 1107（`/System/Library/` 下的系统进程，已运行 46 天），角色 `AXButton`**，不是本 GUI。因此 **min5/min6/min8/min11 的四次真实右键都落在这个系统控件上，而不是本应用的状态项**（它们的“没读到菜单”完全是目标错误，与产品、菜单锁无关）。这些点击对用户的影响：对一个系统菜单栏控件做了四次短暂的右键（指针事后恢复），未更改任何设置；已如实记录。本应用 AX 报告的状态项边框（x≈697–731）与系统实际在该位置的元素不一致：**我们的状态项在系统布局里是否被隐藏/重排，是下一步要查明的事实，尚无结论**。

- **min16 / probe-bar1 / probe-bar2（保留）：状态项被系统收进菜单栏溢出区（主机布局，非产品缺陷）**：`AXUIElementCopyElementAtPosition` 在本应用 AX 报告的状态项中心 (714,15) 返回 **`MenuBarAgent`（pid 1107）的 `AXButton`，`AXDescription = "显示隐藏菜单栏项目"`**（系统菜单栏溢出按钮；菜单栏代理树中该按钮的边框为 x=713.5, w=17.5，覆盖了我们 AX 边框 697–731 的中心）；本应用 AX 边框存在但系统在该处的元素是溢出按钮，所以 **本应用的状态项在这台主机的当前菜单栏布局里被隐藏（溢出）了**，用真实输入点不到它。“代理 owner”（`MenuBarAgent`，pid 1107）与“应用 owner”（本 GUI）不同：这版系统的菜单栏项由代理承载，所以按 pid 归属的命中测试对应用项不成立，必须以元素说明（描述/标识）判定。该溢出按钮 **没有任何 AX 动作**（`AXUIElementCopyActionNames` 为空，`AXPress` 返回 `-25206`），因此没有“成熟的 AX 展开动作”可用；展开前后的状态记录证实无变化（我们的 AX 边框与“点位元素”前后一致）；因没有支持的动作，**没有对溢出按钮做坐标点击**。不移动/隐藏其他应用，不改菜单栏排列。
- **主机能力缺口（精确）**：本主机当前菜单栏布局下，**真实右键输入无法到达本应用状态项**（被系统溢出）。因此“真实右键路由（pre-click 监视器）”这一项在本机 **未验证**，保持 OPEN；要验证需要一个状态项不会溢出的菜单栏（例如更空的菜单栏或另一台主机）。
- **替代的原生跟踪路径（范围明确缩小）**：仍可用 Wails 自己的 `SystemTray.OpenMenu()` 驱动真实 AppKit 菜单跟踪，但它在这里是空操作（`SystemTray.menu` 为空，Wails 在托盘已运行后 `SetMenu` 的行为）。E2E 构建里 **只读判定后** 用反射把该私有字段补成与 `impl` 里同一个 `Menu`，再调用 Wails 的 `OpenMenu()`；产品代码不变，仅 E2E 构建。**它覆盖**：NSMenu 跟踪循环、重建（`Menu.Update`）、主线程等待与锁序、AX 读取与按压、动作到 daemon；**不覆盖**：真实右键的事件路由、状态项的实际可见性。若菜单因按钮位置在屏外而无法被 AX 读取，则如实记录为缺口。

## 生产路径的真实 NSMenu 跟踪：结果记录（按时间）

- **min17-control（保留；`stateMutated=true`，仅为诊断实验）**：反射补齐 `SystemTray.menu` 后 `OpenMenu()` 才生效；`sample` 出现 `NSMenuTrackingSession`、`popUpMenu:atLocation:…`、`showMenu`，AX 在约 2 秒内 47 次读到打开的菜单。**它改变了产品状态，不作为生产菜单通过的证据**，只作为“探针能读到真实跟踪中的 NSMenu”的正对照。后续任何验收运行的 provenance 记录 `stateMutated`；真实右键路径为 `false`。
- **min18–min21（保留）：生产路径成功打开**：对已验证目标（点击前用 AX 系统级“点位元素”核对）先做一次普通合成左键展开系统溢出区（`显示隐藏菜单栏项目`，描述、坐标、所属代理 pid 1107 先记录），展开后本应用状态项出现在 (359,15)，系统报告该点元素 `ownerPid=本 GUI`、`AXMenuBarItem`（`mine=true`），再做真实右键，AppKit 进入菜单跟踪，AX 读到打开的菜单。**没有调用任何反射补字段**（`stateMutated=false`）。收起：GUI 退出后该位置重新是系统溢出按钮（`overflowAtEnd`）。
- **关闭与“打开”判定的更正（min17–min20）**：`AXCancel` 返回成功但对跟踪中的状态栏菜单 **不关闭**；关闭菜单后 AX 子树仍可读（min19 的 6 秒时间线都为 `true`，而 `sample` 显示主线程已不在 `NSMenuTrackingSession`）。**所以“AX 可读”不能表示“菜单仍打开”**。新的观察量是窗口服务器里本 pid 的屏幕上、层级 ≥ 101 的窗口（弹出菜单层）。**区分力已实证（min21）**：打开前 0 个；跟踪中 1 个（291×224，层级 101，位于状态项下方）；关闭后 0 个；打开时采样窗口内有 5 个 `NSMenuTrackingSession` 帧。其局限：它只数“本 pid 的 ≥101 层屏幕窗口”，若本应用另有同层级屏幕窗口会被误计，因此每次都把本 pid 的完整窗口列表保存在工件里（`windowsBefore/During`）。
- **full1（保留，`passed=false`）的三个失败及根因（均为测试侧，不是产品缺陷）**：(1) 初始根菜单是中文：前端第一次 `zh-CN` 在 +0、静默控制在 +5.1 秒返回、测试固定的 `en` 在 +5.2 秒、**前端的第二次启动期 `zh-CN` 在 +5.6 秒**，即在测试固定值之后 0.4 秒覆盖了它；菜单显示中文是产品“最后一次调用生效”的正确行为；这是 17c14 起 OPEN 的“前端启动期重复调用 `set_tray_language`”，且其间隔可超过我原来的 5 秒静默窗口。(2) 保持期间每次读取完整但都是中文根菜单：与 (1) 同因，菜单内容完整、不是空/半构建；断言把英文写死是测试的固定文案假设。修正：固定后再等 8 秒静默并在被覆盖时重新固定（保留所有调用作证据）；期望值取“最后一次语言调用”的语言，断言保持中/英完整根菜单，不放宽内容。(3) `exact daemon pids gone` 的 `daemons=[]`：用进程环境找 daemon pid 在 macOS 上 **什么都没找到**（`daemonPidsAtStart=[]`），所以这条检查是空检查，既不证明 daemon 没退出也不证明已退出；修正：身份链改为 profile 数据目录 → `.uniclipd.lock` 的持有者（`lsof`）→ 可执行文件路径，启动后即断言非空，Quit 之前取精确 pid，Quit 之后逐个核对已消失。

## 补充失败模型：子菜单真实展开与轻量模式生命周期（落盘先于对应脚本改动）

覆盖审计发现 full2/final 只证明了“根菜单跟踪、根树里能读到子项、完整 Quit”，**没有** 覆盖 M3 要求的“设备子菜单实际展开时”的语言与设备结构变化，也没有 M5 的轻量模式。根树里递归读到子项 **不等于** 子菜单已展开。

- **M3b 子菜单真实展开**：展开方式是对根菜单里 `Device Sync` 这个子菜单项做 AX 按压（`AXPress` 在 AppKit 菜单里对应打开子菜单）。“已展开”的判定观察量：窗口服务器里本 pid 的屏幕上、层级 ≥ 101 的窗口由 1 个变为 2 个，第二个窗口是子菜单大小且紧邻根菜单窗口，同时子菜单条目可读；展开前后各保存本 pid 的完整窗口列表。若按压后窗口数没有变化，如实记为“未展开”，不用递归 AX 子树代替。
- **M3c 跟踪期间、子菜单展开时的变化**（三种，均在跟踪中发生并记录窗口数时间线，**不预设结果**）：(1) 自然 10 秒刷新发布（`Menu.Update` 清空并重建同一个 NSMenu）：记录子菜单窗口是否保持、条目是否完整；(2) 人工调度：语言变更到 zh-CN 再回 en：根与子菜单标题/条目是否同语言；(3) 设备结构变化：在菜单打开且子菜单展开时，用生产 rendezvous 把第二个对端配对进同一空间，子菜单在下一次刷新或设备变化事件后应出现第二行（`tray-peer-b`、`tray-peer-c`）；由 `member list` 与 daemon 权威状态核对对端存在，菜单布局只作菜单证据。重建可能使子菜单收起（可接受的 AppKit 行为，需如实记录），但不能出现空菜单、崩溃、主线程停滞或与 daemon 不一致。
- **M5b 轻量模式生命周期（独立 profile）**：真实右键打开（同样先验证目标）→ AX 按压 `Lightweight Mode (Background Sync)` → 精确 GUI pid 以 0 退出；**锁文件持有的 daemon 精确 pid 仍活着**（可执行文件路径核对）、daemon HTTP 健康（`daemonget /settings` 成功）；随后由编排器经 `uniclip stop` 停止，同一精确 pid 消失、锁文件无持有者。完整 Quit（full2）与轻量退出是两条不同生命周期，分别记录。

## 独立评审后的失败模型（落盘先于对应改动）

独立只读评审（原文：`t-0188-artifacts/macos-17c15/review-792dc721a/review-raw.md`）对 `792dc721a` 提出的问题，按“本轮发现的失败方式”记录，随后才改运行器和探针；原四轮（`final3`）保留为 **partial evidence**，不覆盖、不删除。

- **F1（3c 与 M3 范围不符）**：`final3` 的 3c 只要求“配对开始时子菜单已展开”。工件显示配对期间的 publish 在 0.2/0.6 秒收起了子菜单，新行在约 10 秒的下一次 publish 时出现，此时子菜单是收起的。所以“设备结构变化发生在子菜单展开期间”**没有** 被观察到，原 3c 的措辞和覆盖范围说明过强。处理方式不是把文档弱化到“收起”就结束：运行器要在整个配对与等待期间沿用 3b 的做法（采样并在发现收起时按真实状态重新展开），并把“加入新行的那次 publish”与它开始前的展开采样配对；该 publish 开始前没有展开采样，则 3c 失败；重新展开后要复验两行。
- **F2（默认开启方式无效）**：`--open-with` 默认是 `control`（反射填充私有字段，`stateMutated=true`），裸跑会得到无效运行。默认改为 `rightclick`，`control` 运行无论检查如何都不得 `passed=true`。
- **F3（重开菜单没有断言）**：第 3、4、5 步的 `open_menu` 返回值被丢弃，已关闭菜单的 AX 子树仍可读，所以“在真实菜单里按压”没有被强制。`open_menu` 改为等到出现弹出窗口，调用方对每次重开都断言有弹出窗口，之后才按压。
- **F4（点击目标身份）**：目标校验原先接受“标识符包含 bundle id”，另一个 E2E 实例也会通过。现在应用状态项的点击只接受元素属主 pid **等于本次运行的 GUI pid**（且角色为菜单栏项目）；bundle 名、标识符、“没有其他同名进程”都不作为身份依据，只作诊断。系统“显示隐藏菜单栏项目”按钮是单独、明确校验的一步（属主不是本 pid，描述匹配）。历史工件里状态项处的 `ownerPid` 始终等于 GUI pid（`mine=true`），所以这个更严的条件在本机可满足；若某台主机取不到，应记录验证缺口再研究，而不是用进程排除代替目标属主。
- **F5（collapse_overflow 名不副实）**：该函数不点击，只记录；`overflowAtEnd` 是写死的本机坐标。改名为只记录状态，文档不再声称“收起”。
- **F6（安全声明与代码不符）**：头部写“不移动指针、不发按键”，但 `clickat`、`rightclick`、`hover`、`escape` 会。头部更正；`escape` 之前立即重新确认本 pid 的弹出窗口仍在。
- **F7**：`clickat` 在 Swift 内部用同一次调用重新做 `AXUIElementCopyElementAtPosition` 的属主与身份校验，不匹配就拒绝点击。
- **F14**：清理的每一步各自防护（`ax()` 超时返回错误行而非抛异常），一步卡住不能遗留 GUI、daemon 或 caffeinate。
- **F16**：`tray_ax.swift` 与 `e2e/linux/daemonget` 的源码在仓库内；运行器从源码构建（命令与源码/二进制哈希写入 provenance），二进制缺失或不是该源码构建的则失败，不再依赖 `/private/tmp` 下的预置文件。
- **F17**：`stateMutated` 由实际日志（`tray-open-menu-*` 记录里的 `menuFieldFilledByE2E`）推导，不再取自命令行参数。
- **F18**：运行中 daemon 进程的可执行文件哈希与本次构建的 `uniclipd` 哈希核对，不一致则失败。
- **其他较小问题**（2 的 `popupWindows`、publish 窗口按首次/末次出现弹出窗口界定、3b MANUAL 前的展开采样、按标题选行、死代码、watcher 收尾、`_alive` 的 `PermissionError`）按条修复；决定保留的条目在评审映射表里逐条写明原因。

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

以下先列探索性中间记录，随后是冻结二进制的最终运行。

### 子菜单展开期间的刷新（中间工件 `final2/full`、`final2/full2`、`final2/full3`）

- `final2/full`：3a 失败（按压后窗口数未变化的判定过窄），原工件保留，不改写。
- `final2/full2`：3a 通过，窗口服务器里出现根菜单窗口（291×224）和子菜单窗口（117×34）。3b 时间线显示两个窗口只存在于 0.06–7.34 秒，7.66 秒起只剩根菜单窗口。自然 publish 发生在 7.46 秒与 17.46 秒：**只有第一次是在子菜单展开时发生的，第二次发生在已经收起之后**，因此 full2 只证明“一次展开期间的自然 publish”，不能算两次。collapse（7.66 秒）与 publish（7.46 秒）相差约 0.2 秒，采样间隔 0.25 秒，这只是时序邻近证据，**不是因果证明**；没有独立实验区分“重建收起了子菜单”与其他原因。3c 同样：publish 在 9.1 秒，下一次采样 9.6 秒已收起。
- 因此 full2 中 3b 的断言写成“每次 collapse 都在某次 publish 附近”是不够的。runner 已改为逐次 publish 判定：每次 publish 之前按真实状态重新展开，记录该 publish 开始前最后一次采样是否展开、其后第一次收起的时刻、重新展开后子菜单的行与根菜单语言；“至少两次 publish 开始时处于展开状态”不满足则该检查失败，不保持虚假的持续展开。

- `final2/full3`（逐次 publish 判定，探索性中间运行，非冻结最终）：36 秒内 3 次自然 publish（每次 6–8 ms 返回）。其中 **2 次开始前最后一次采样为展开状态**（采样年龄 0.12 秒、0.04 秒），之后第一次收起采样分别在 publish 开始后 0.03 秒、0.12 秒，重新展开分别在 0.08 秒、0.17 秒，重新展开后行为 `tray-peer-b`、根菜单为英文。第 3 次开始时已处于收起状态，不计入展开期间的 publish。该时序与“重建后子菜单收起”一致，但仍只是时序证据；采样与 AX 按压都是外部观测，不独立证明机制。

### 冻结二进制的运行（`final3`，**partial evidence，不是最终验收**）

独立评审（`792dc721a`）指出这四轮运行所用的运行器有缺陷：3c 实际没有观察到“展开期间的结构变化”（新行出现时子菜单已被前一次 publish 收起），第 3、4、5 步重开菜单后没有断言弹出窗口存在，目标身份校验接受了 bundle 名，等等（见上一节的失败模型和 `review-792dc721a/fix-map.md`）。因此下表只证明：在当时的运行器口径下通过。其中 3a、3b 的逐次 publish、语言变更、设备按压、同步开关、Quit 和轻量模式的结果不依赖有缺陷的那几处，但 3c 的结论 **不成立**，也不能据此声称“展开期间的结构变化已验证”。修正后的运行器已提交（`307ac2b49`），**真实菜单验收待宿主会话解锁后重跑（PENDING）**；该 partial 工件保留，不覆盖。


来源 `71c455d2f`（干净），Engine `d4dd324a` 与 Cargo.lock 一致且无路径覆盖，manifest 见 `final3/build-manifest.json`，构建日志保留 28 条 ld 警告（对象按 macOS 27.0 编译、按 11.0 链接；更老 macOS 的兼容性 **未证明**）。顺序运行四轮，provenance 的 `stateMutated` 均为 false（菜单由真实右键经 AppKit 跟踪打开，未使用反射填充的开启方式）：

| 运行 | 场景 | 结果 |
| --- | --- | --- |
| `final3/full1` | full | 28 项通过，rc=0 |
| `final3/light1` | lightweight | 9 项通过，rc=0 |
| `final3/full2` | full | 28 项通过，rc=0 |
| `final3/light2` | lightweight | 9 项通过，rc=0 |

逐次 publish（3b）：`full1` 4 次 publish 全部开始时子菜单展开，其后第一次收起采样在 0.04–0.12 秒内，重新展开在 0.09–0.17 秒；`full2` 4 次中 3 次开始时展开（收起采样 0.03–0.05 秒、重新展开 0.08–0.11 秒），第 4 次开始时已收起、不计入。每次重新展开后行均为 `tray-peer-b`、根菜单为英文。publish 返回均远小于 2 秒，主线程等待在跟踪期间得到服务。这仍是时序证据；子菜单收起的机制未独立证明，也没有声明“持续展开”。

这些运行 **没有** 证明：Windows 真实托盘；更老的 macOS；其他主机的菜单栏溢出布局；右键路由之外的左键行为（左键执行应用的 `OnClick`）。

### 评审后修正的运行记录（`final4-explore`，探索性，非冻结验收）

- `final4-explore/full`：**失败，保留**。运行器的 `daemonget` 源码路径写错（`e2e/daemonget` 不存在，实际在 `e2e/linux/daemonget`），工具构建在启动 GUI 之前失败，目录里只有 `run.log`（当时启动失败路径还不写 assertions）。已改为从仓库源码构建并修正路径；没有退回 `/private/tmp` 下的预置二进制。
- `final4-explore/startup-failure`：对启动失败路径的真实注入（去掉 `go`、让 `daemonget` 需要重建）：写出 `assertions.json`（`passed=false`、`phase=tools-build`），没有启动任何 GUI、daemon 或 caffeinate，没有遗留进程。
- `final4-explore/full2`：**目标验证拒绝点击（安全拒绝，保留）**。状态项的 AX 位置 (391, 15) 上的元素是系统「登录」窗口（pid 630），`CGSessionCopyCurrentDictionary` 的 `CGSSessionScreenIsLocked` 为 1：宿主会话已锁屏。运行器没有发出任何点击；清理干净（GUI 与三个 daemon 均无遗留，无 `cleanupErrors`）。这是宿主条件，不是产品结论；按约束不解锁、不输入凭据，等会话解锁后再验收。

### 覆盖边界

- 3c（结构变化）在 `final3` 中 **没有** 覆盖展开期间的变化（见上）；修正后的运行器要求加入新行的那次 publish 开始时子菜单处于展开状态，该验收 PENDING。3b MANUAL（语言）以变更前一次采样确认展开，同样 PENDING。
- Windows 真实托盘仍是 OPEN；整个迁移 OPEN 清单（见全迁移计划与交接文件）不因本切片减少。
