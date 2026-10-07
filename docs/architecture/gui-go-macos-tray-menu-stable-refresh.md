# Go GUI 托盘菜单：内容未变化时的定时刷新不得收起子菜单（17c16）

状态：失败模型与验收契约（先于任何运行器改动、生产改动与 E2E 落盘）。结果与边界在文末「结果」节随证据补写。

## 写入顺序（如实记录）

1. 核对 live git/gh：PR #1892 HEAD `42e95652f`（base `fix/gui-go-linux-tray-menu-refresh`，#1891），均为 OPEN，不触碰任何父 PR。
2. 从该 HEAD 建本地分支 `fix/gui-go-macos-tray-skip-unchanged-refresh`（有业务含义的名称，stack base 为 `test/gui-go-macos-tray-menu-tracking-lock`）。`pr-context-audit` 与 `branch-name-guard` 的执行顺序（先 audit 后 guard）在建 PR 时记录于本文「PR 前检查」。
3. 阅读固定 Wails `v3.0.0-beta.28` 的 `menu.go`、`menu_darwin.go`、`menuitem.go`、`menuitem_darwin.go`、`menuitem_windows.go`，以及本仓 `tray.go`、`tray_devices.go`、`tray_publish_*.go`。**此时没有任何探针或运行，也没有生产改动。**

## 17c15 留下的事实

17c15 的真实证据审查发现：12/12 次发布都使 `Menu.Update` 之后展开的设备子菜单收起，其中包括内容完全相同的 10 秒定时刷新。用户若把鼠标停在子菜单上，每 10 秒会丢失一次子菜单。

## 源码事实（固定 Wails，已读）

- `Menu.Update` 在 macOS 上是整体重建：`clearMenu`（`removeAllItems`）后重新创建每个 `NSMenuItem` 与子菜单（`macosMenu.update` / `processMenu`）。
- `MenuItem.SetLabel` / `SetEnabled` / `SetChecked` 在条目已有平台实现（`impl`）后立即作用到现有原生条目（macOS `macosMenuItem.setLabel/setDisabled/setChecked`，Windows `windowsMenuItem.set*` 经 `SetMenuItemInfo`）。因此 **只有菜单结构变化（增删条目）才需要重建**；仅标签、勾选、可用状态变化在 macOS 与 Windows 上已经由 setter 直接生效。
- Linux 的 StatusNotifierItem 通过 `SystemTray.SetMenu` 重建 dbusmenu 布局，**任何可见变化**（含标签、勾选、可用状态、根菜单同步开关标签）都只有经发布才可见。
- `deviceMenu.render` 已经在设备集合未变时原地更新条目，但 **无条件** 调用 `publishMenu`；`deviceMenu.setLanguage` 同样无条件发布。
- 根菜单项（同步开关标签、`reserveSync` 的可用状态、语言标签）由 `trayMenu.mu` 保护，其变化在 Linux 上只靠下一次发布才可见。

## 失败模型

- **U1 无变化的定时刷新收起展开的子菜单**：`render` 在内容与上次相同时仍调用 `Menu.Update`。17c15 观测到 12/12 次发布收起子菜单。预期：无变化时不发布，子菜单保持展开；与父版本（`ce8649967` 构建）对照区分。
- **U2 过度抑制（修复本身的风险）**：若以“`render` 参数相同”判定无变化，会漏掉不经 `render` 的变化。具体：(a) Linux 上 `setSyncEnabled`、`reserveSync` 只修改内存条目，靠下一个定时发布可见，抑制后永不可见；(b) `click` 把条目置为禁用/恢复勾选后，保存完成的 `render` 把条目改回启用，若以上次 `render` 的快照比较，会认为“无变化”而不发布，Linux 上条目保持禁用；(c) 语言变化。**判定依据必须是“当前条目状态与上一次实际发布时的条目状态”的比较，而不是上一次 `render` 的输入。**
- **U3 发布锁与并发快照**：快照必须在发布路径内读取，持有与现有相同的锁序（`languageMu` → `deviceMenu.mu` → `trayMenu.mu`），使“比较、发布、记录已发布状态”对任何其他 goroutine 是一个原子步骤；快照不能在锁外读取后再发布。
- **U4 真实变化后的收起**：成员增删、设备偏好变化、语言变化、全局同步变化会改变快照并发布；macOS 上 `Menu.Update` 对展开的子菜单收起是 AppKit 的实际行为，**作为边界记录，不作为需要隐藏的缺陷，也不降低“无变化稳定”的标准**。
- **U5 其余发布路径**：M1–M5（锁、设备动作、同步开关、退出）不得回退。

## 修复设计（最小，先审核已有逻辑）

不新增哈希或抽象层。已有的“设备集合相同则原地更新”逻辑保留。唯一新增是在发布闭包（持 `trayMenu.mu`）内读取根与子菜单条目的当前状态（标签、勾选、可用），与上一次实际发布时的同一份状态比较：相同则跳过，不同则发布并记录。比较类型为可比较的结构体切片（`slices.Equal`），不是哈希。该闭包是 `deviceMenu.render`、`deviceMenu.setLanguage` 与定时刷新的唯一发布出口。

不使用 Wails 更高层能力的原因：固定 Wails 没有“仅在内容变化时更新”的 API，`Menu.Update` 与 `SystemTray.SetMenu` 都是无条件重建；条目 setter 已是 Wails 内建的原地更新，保持使用。

## 验收契约

运行器新增场景 `stable`（macOS，真实生产状态项）：

1. 状态项经精确 GUI PID 与 role 复核后真实右键打开；不用 `SystemTray.OpenMenu`。
2. 设备子菜单通过 AX 按压实际展开（与 17c15 同一方式），并以“第二个子菜单大小的弹出窗口”为准。运行器 **只展开一次**，此后只被动读取；任何收起都记为失败，**不重新展开**。
3. 无变化期间跨至少三个自然定时发布时刻（以宿主 `tray-refresh` 记录，来源标注为定时器），子菜单每次读取都保持展开。
4. 对照：同一场景在父版本二进制（`ce8649967`，`final6` 清单哈希）上必须失败（保留为失败证据），在修复构建上通过。
5. 变化路径各自单独记录并核对 daemon 权威状态：成员添加/删除、语言、设备偏好、全局同步；每条记录触发路径（定时器或事件 `devices://sync-changed`），**不把定时器路径说成事件路径**。
6. M1–M5 沿用 17c15 的完整场景与轻量场景（`full`、`lightweight`），在修复构建上各复跑。
7. 真实指针悬停：运行器的 `hover` 是真实 `CGEvent` 指针移动（诊断用，调用者先验证目标）。如可用，作为补充记录；不可用或不能展开则写明证据边界，不以过期 AX 树当作“已打开”。
8. 轻量场景补充：守护进程存活检查前等待 3–5 秒并记录 CLI `stop` 返回码与停止结果。

## 证据分级

源码推断、编译、模拟、真实证据分别标注。Linux/Windows 共享逻辑（`tray.go`、`tray_devices.go`）改变，按平台可用门禁（本机 `go vet`、macOS 构建、Linux/Windows 交叉 vet 的真实退出码）与原生回归覆盖记录；macOS 上 Linux 交叉 vet 因 Wails cgo 失败不得说成通过。

## 结果

### 实现

`apps/gui-go/tray.go` 的发布闭包在持有 `trayMenu.mu` 与 `deviceMenu.mu` 时读取根菜单与设备子菜单所有条目的当前标签、勾选、可用状态，与上一次实际发布时的同一份状态比较（`slices.Equal`，无哈希），相同则跳过。`deviceMenu.render` 对空设备列表改为原地更新占位条目（此前每个 tick 都 `Clear` 后重建）。独立只读评审指出：Linux 上 `click` 修改条目但不发布，保存后恢复原状会被跳过逻辑当作无变化，因此 `click` 现在在置为禁用后发布一次。E2E 构建新增两条证据：`tray-refresh`（原因 `timer`、`event`、`save`、`initial`）与 `tray-publish-skipped`。

### 证据分级

- **源码推断**：Wails beta.28 的条目 setter 在 macOS 与 Windows 立即作用于原生条目，只有结构变化需要重建；Linux 任何可见变化都要经 `SetMenu`。上文「源码事实」。
- **编译**：darwin、darwin+e2e、windows、windows+e2e `go vet` 与 `gofmt` 均 rc=0（`final2/gates/`）。Linux 交叉 `go vet` 在 macOS 上因 Wails cgo 失败（rc=1），**不是** 通过，也不是原生 Linux 门禁。Linux/Windows 共享逻辑没有在 Linux 与 Windows 真实托盘上运行。
- **真实证据（macOS，真实生产状态项，精确 GUI PID 与 role 复核的右键，`stateMutated=false`）**：见下。

### 父版本对照（失败证据，保留）

`parent-control/stable1`：`final6` 二进制（源码 `ce8649967`，清单哈希匹配）。第一个定时刷新（第 8.24 秒）后子菜单收起，144 个被动样本中 113 个未展开，3 次定时发布；S3 两项失败。

### 修复构建 `final2`（源码 `4fd8d19a5`，清单 `final2/build-manifest.json`）

| 场景 | 第 1 轮 | 第 2 轮 |
| --- | --- | --- |
| stable（17 项） | 通过 | 通过 |
| lightweight（11 项） | 通过 | 通过 |
| full（34 项） | **失败（3b）** | 通过 |

- stable：子菜单只展开一次，之后只被动读取，跨 4 个自然定时刷新（`tray-refresh` 原因 `timer`）全程保持展开；窗口内 0 次发布，4 次 `tray-publish-skipped`。没有任何自动重开。
- 变化路径（stable 内，触发均为定时器路径，**不是事件路径**）：语言变化（手动调用）、通过 CLI 改设备偏好（daemon 权威读取）、成员添加、成员移除，菜单内容均与 daemon 一致。**真实变化后系统收起子菜单**：语言变化与成员添加后子菜单不再展开，这是 `Menu.Update` 的实际边界，没有隐藏。
- full：M1–M5（锁、设备动作、同步开关、退出）沿用 17c15 的完整检查，第 2 轮 34/34；daemon 权威状态直接核实。
- lightweight：同一 daemon pid 在 GUI 退出后存活（等待 4 秒后检查），`stop` 返回码 0、状态 `stopped` 且停止的是同一 pid。
- **full 第 1 轮失败**：3b 中菜单在窗口第 20.33 秒消失，该窗口内 0 次发布，所以不是本修复的重建所致。菜单为何消失 **没有证据**，没有断言为干扰。按用户决定不再复跑，失败结果原样保留。
- `final1`（源码 `f9579d18a`，被评审指出的缺口之前）：stable1、lightweight1、full1 通过，stable2 与 full2 失败，菜单在运行中消失；用户当时动了鼠标，但无法据此证明因果。`final1` 不作为验收，全部保留。

### 未覆盖与边界

- 真实指针悬停：`tray_ax.swift hover` 是真实 `CGEvent` 指针移动，但属诊断用途且会移动用户鼠标，本片未作为验收路径运行；子菜单展开用 AX 按压，这是 17c15 的同一方式。
- 事件路径（`devices://sync-changed`）：宿主已能记录 `event` 与 `save` 原因，但本片的变化步骤都由定时器触发，事件路径未单独验收。
- Windows 与 Linux 真实托盘：未运行；跳过逻辑对它们的影响只有源码推断与编译证据。
- 轻量模式的 daemon 存活只在一次 4 秒等待后检查，不是持续观察。
- 28 条 `ld` 警告保留（older macOS 兼容性未证明）。

### PR 前检查

按任务顺序：先 `pr-context-audit`，后 `branch-name-guard`，结果在 PR 描述中记录。
