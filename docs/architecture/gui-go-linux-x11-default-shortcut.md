# Go GUI Linux：产品默认快捷键与 X11 快捷面板尺寸（第 17c8 片）

范围：Linux 上 Go/Wails GUI 的快捷面板（WebView 面板，X11 会话）。本文先于产品代码修改落盘：记录要验证的契约、预计失败方式和修改前（baseline）的实际结果；修复与修复后结果在同一文件的“修复后”一节补充。不改 Rust daemon / Engine，不改共享前端。

证据边界：私有 Xvfb + 私有 D-Bus，没有窗口管理器、合成器、Wayland、portal。这不是原生桌面验收。

## 契约（来源）

| 编号 | 契约 | 来源 |
| --- | --- | --- |
| C1 | 新资料首次启动、设置里没有保存过快捷键时，快捷面板全局快捷键为 `ctrl+alt+v`（Linux / Windows），设置页显示同一值 | `crates/uc-desktop/src/shortcuts.rs`（`DEFAULT_QUICK_PANEL_SHORTCUT`）、共享前端 `apps/gui/src/shortcuts/definitions.ts`（`global.toggleQuickPanel`）、Go `defaultQuickPanelShortcut()` |
| C2 | 默认是否启用由 daemon 设置 `quickPanel.enabled` 的默认值决定，宿主不得替用户改动；启用时注册默认值，禁用时不注册任何全局快捷键 | `apps/gui-go/host_commands_quick_panel.go` 的 `panelShortcutTarget` |
| C3 | 通过真实设置页（Settings > Quick panel 的快捷键录入弹层）改绑：先注销旧键、注册新键，失败回滚；新值落 daemon；重启后注册的是已保存值，不是默认值 | `updateKeyboardShortcuts`（对应 Tauri `update_shortcuts`） |
| C4 | X11 快捷面板窗口尺寸固定 800x560 逻辑像素，不随内容缩放（`windowScale` 只在 0.8–1.5 内等比放缩），不沿用 macOS 的 360x420 加 16 像素边距 | `crates/uc-tauri/src/quick_panel/mod.rs`：`LINUX_PANEL_WIDTH/HEIGHT`、`panel_dimensions`、`resized_panel_dimensions` |
| C5 | 首次映射（MapNotify）时窗口已是目标尺寸，其后没有改变尺寸的 ConfigureNotify | 本片要求；以 X 服务器事件序列为证，最终截图不算证据 |

## Wails beta.28 源码事实（固定版本）

- 全局快捷键：`global_shortcut_linux_x11.go` 用 `XGrabKey`，同时抓取 CapsLock/NumLock 四种锁定修饰组合（`gsLockMasks`）；已被其他客户端抓取时返回 `the shortcut is already registered (possibly by another application)`，宿主据此返回 `Conflict`。这些语义直接复用，宿主没有自写协议。
- 窗口尺寸：`linux_cgo.go` 的 `setSize` 调用 `gtk_window_set_default_size`。它对尚未映射的窗口生效；对已映射窗口不会立即缩放。所以首次显示之前设定的尺寸才是首帧尺寸。

## 预计失败方式（修改前）

1. C1/C2/C3：预计已满足（前几片只是用 `UC_GUI_GO_E2E_DEFAULT_SHORTCUT` 覆盖默认值，从未走过真实默认值和真实设置页），所以这些检查预计为绿，作为新增的回归防线和证据，不是已发现的缺陷。
2. C4：`windows.go` 的 `panelSize` 对所有平台使用 macOS/Windows 常量（360x420 加两侧 16 像素），预计 X11 面板首次映射为 392x452，而不是 800x560。
3. C5：预计在默认 `windowScale` 下没有后续尺寸变化；非默认 `windowScale` 时前端随后调用 `set_quick_panel_window_size` 的结果未量化（见“未验证”）。

## Baseline 实际结果（产品代码未改）

- 构建来源：HEAD `8151f2ccf`（本片分支父提交），工作树只有 E2E 驱动与脚本改动（`inputs/status.txt`、`inputs/dirty.diff`）；前端用 `VITE_GUI_GO_E2E=1` 实际编译，dist 内容含本片驱动场景（`inputs/dist-contains-driver.txt`，dist 文件清单 SHA-256 在 `inputs/dist.sha256`）。GUI 与 CLI 在 Linux arm64 容器内构建；daemon 是 17c5 的 release 构建（SHA-256 `ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6`），未重建、未执行 `--version`。
- 产物：`t-0188-artifacts/linux-17c8/`：`red1-baseline`（保留：观察器缺陷，见下）、`red2-baseline`（保留：窗口名解析在 GUI 退出后才执行，仍无事件）、`red3-baseline`（保留：面板窗口按名字筛选无匹配）、`red4-baseline`（有效的修改前基线，使用 `red2-baseline/inputs` 对应的同一组二进制）。
- **产品默认值（实证）**：新资料首次启动，daemon `quickPanel.enabled` 默认为 **true**，没有保存过快捷键（`stored: null`）；设置页显示 `Ctrl+Alt+V`；宿主登记 `ctrl+alt+v`，Wails 报告 `Alt+Ctrl+V`。宿主没有、也不应该启用任何默认关闭的功能。
- C1–C3 全绿：真实 XTEST `ctrl+alt+v` 显示再隐藏面板；真实设置页录入器接收真实按键并保存 `Ctrl+Alt+Shift+F9`；旧键在 X 服务器上被释放，新键被抓取，旧键不再显示面板，新键显示面板；同一资料重启后登记的是已保存值，旧默认键空闲。
- C4 失败（产品缺陷）：X11 面板首次映射尺寸为 **392x452**，不是 800x560（`red4-baseline/linux-assertions.json` 的 `facts.first_map`）。
- C5：默认 `windowScale` 下，首次 MapNotify 之后没有改变尺寸的 ConfigureNotify（`size_changes_after_first_map` 为空），第二次显示只有 MapNotify。
- 观察器缺陷（不是产品失败）：red1–red3 中尺寸两项失败来自测试工具：xev 行格式解析错误（red1），窗口标题在 GUI 退出后才查询（red2），面板窗口在 X11 上的 `WM_NAME` 是 `gui-go` 而不是配置的 `Quick Panel`（red3）。它们各自保留，没有被当作产品结论。
- 窗口创建时序：面板窗口在首次显示时才创建（按键后约 37 ms 内出现 CreateNotify，随后 ConfigureNotify 定位，再 MapNotify），不是启动时预创建。

## 未验证（保持 OPEN）

- 非默认 `windowScale` 下首帧之后的尺寸行为（`gtk_window_set_default_size` 对已映射窗口的语义，需要单独的 X11 事件序列）。
- Wayland / GNOME / KDE / Hyprland 实机、真实 GPU、窗口管理器下的焦点与放置、粘贴到前一个应用。
- XGrabKey 的 CapsLock/NumLock 变体：已读 Wails 源码，未在 Xvfb 里用真实锁定修饰键验证。
- 原生 amd64、AppImage 内的同一场景（本片不改变打包输入，没有重跑 AppImage 回归；原因见 PR 说明）。
