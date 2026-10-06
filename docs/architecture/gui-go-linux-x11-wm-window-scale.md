# Go GUI Linux：X11 窗口管理器下的快捷面板（窗口缩放、工作区、首次映射、焦点）（第 17c9 片）

范围：Linux 上 Go/Wails GUI 的快捷面板（WebView 面板，X11 会话，非 Layer Shell）。本文先于任何产品代码修改落盘：记录契约来源、预计失败方式和修改前（baseline）的实际结果；修复与修复后结果在同一文件后续小节补充。不改 Rust daemon / Engine，不改共享前端，不升级 Wails，不自写窗口管理器或协议。

证据边界：容器内私有 Xvfb + 私有 D-Bus + 成熟的窗口管理器 Openbox（未改配置，系统默认 `/etc/xdg/openbox/rc.xml`）。这是“真实 X11 窗口管理器管理真实 GUI 窗口”，不是原生实体桌面，不是 Wayland，没有 GPU，没有 GNOME/KDE 的窗口管理器。

## 契约（来源）

| 编号 | 契约 | 来源 |
| --- | --- | --- |
| W1 | `windowScale` 是快捷面板自己的前端状态（`localStorage` 键 `uniclipboard.quickPanel.windowScale`），不是 daemon 设置；步长 0.1，范围 0.8–1.5，前端与 Rust 两侧都钳制；只在 Linux Tauri 面板生效。它与显示器 DPI/GTK scale factor 无关（逻辑像素，由 toolkit 换算）。 | `apps/gui/src/quick-panel/window-layout.ts`（`normalizeScale`、`adjustQuickPanelScale`、`isLinuxPanel`）、`crates/uc-tauri/src/quick_panel/mod.rs`（`MIN/MAX_WINDOW_SCALE`、`resized_panel_dimensions`） |
| W2 | 用户通过面板里的真实快捷键调整：`ctrl+=` 放大、`ctrl+-` 缩小（`quickPanel.windowIncrease/Decrease`）。越界按键不再改变（钳制）。 | `apps/gui/src/shortcuts/definitions.ts`（`QUICK_PANEL_SCALE_SHORTCUTS`）、`useQuickPanelScaleShortcuts.ts` |
| W3 | 尺寸 = 800x560 逻辑像素 × 钳制后的缩放，四舍五入；0.8 → 640x448，1.1 → 880x616，1.5 → 1200x840。 | `quick_panel/mod.rs`：`LINUX_PANEL_WIDTH/HEIGHT`、`resized_panel_dimensions` |
| W4 | 每次显示：宿主先按基础尺寸准备，前端在 `prepare-show` 之后、`finalize_quick_panel_show` 之前用已保存的缩放调用 `set_quick_panel_layout`，所以窗口映射时已是缩放后的尺寸。面板已显示时用户改缩放，窗口立即改变尺寸（Tauri 的 `set_size` 对已映射窗口生效）。 | `quick_panel/mod.rs`：`show`、`set_layout`；`apps/gui/src/quick-panel/QuickPanelApp.tsx`：`finalizeShow` |
| W5 | 缩放值保存在面板的 `localStorage`，重启后第一次显示就使用已保存的缩放。 | `window-layout.ts`（`readWindowScale`） |
| W6 | 工作区：Tauri 面板窗口设置了 `always_on_top`、`skip_taskbar`、无边框、不可缩放，**没有** 设置“所有工作区可见”（sticky）。所以契约只是“窗口管理器对一个普通 X11 窗口的默认行为”：用户在当前工作区按快捷键，面板应该出现在当前工作区并获得焦点。本片不添加 sticky，也不添加 Tauri 里没有的工作区行为。 | `quick_panel/mod.rs`：`pre_create` 的构建参数 |
| W7 | 焦点：显示后调用 `set_focus`；失去焦点（非 Layer Shell 路径）在防抖后隐藏；Escape 关闭面板。 | `quick_panel/mod.rs`：`finalize_show`；`windows.go`：`WindowLostFocus` 钩子 |
| W8 | 在 X11 上缩放对 GTK 窗口只是请求；Tauri 用 `window.set_size`（tao 0.35 `WindowRequest::Size` 调用 `gtk::Window::resize`，即 `gtk_window_resize`），Wails beta.28 在本仓使用的 `gtk3` 构建里 `SetSize` 同样是 `gtk_window_resize`（`pkg/application/linux_cgo_gtk3.go`）；非 `gtk3` 的 `linux_cgo.go` 才是 `gtk_window_set_default_size`（17c8 文档里的那句说的是后者，对本仓产物不适用）。 | tao 0.35.3 `src/platform_impl/linux/event_loop.rs`；`wails/v3@v3.0.0-beta.28/pkg/application/linux_cgo_gtk3.go`（`setSize`、`setResizable`、`windowSetGeometryHints`） |

## 预计失败方式（修改前，E2E 先行）

1. W4/W8：面板已映射时按 `ctrl+=`，Go 侧 `set_quick_panel_layout` 只调用 `w.SetSize`（默认尺寸），预计已映射窗口不改变尺寸（静默无效）。
2. W4：隐藏后再次显示时，默认尺寸是否在 GTK 重新映射时被重新采用，未知；预计可能仍是旧尺寸。
3. W5：重启后第一次映射是否直接是已保存缩放的尺寸，未知。
4. W6：切换工作区后快捷键是否让面板出现在当前工作区，未知。
5. W7：Openbox 的焦点窃取保护是否拒绝 `gtk_window_present` 的激活请求，未知。

这些是需要事实回答的问题；不预设哪一个一定失败，也不为了制造修复而改业务代码。

## 预计失败方式的结论

1. W4/W8（缩小）：**确证的产品缺陷**。窗口缩放小于 1.0 时面板仍是 800x560：实时按键、再次显示、重启后第一次映射都一样。放大（1.1–1.5）正常。
2. W5：缩放值保存与重启生效正常（前端 `localStorage`），重启后第一次映射已是已保存缩放的尺寸；缩小缺陷导致 0.8 的重启也停在 800x560，修复后正常。
3. W6：无缺陷。在当前工作区按快捷键，面板出现在当前工作区（桌面 0/1/2 各一次），并获得焦点。
4. W7：无缺陷。面板显示后是窗口管理器的活动窗口（上一个目标应用失去焦点）；真实 Escape 到达页面并隐藏面板，之后焦点回到上一个目标应用；另一个窗口取得焦点时面板隐藏（失焦隐藏）。离开面板所在工作区时面板同样被失焦路径隐藏（观测事实，不是新增契约）。

## 基线与失败实验（全部保留）

构建来源与证据目录：`/Users/mark/.herdr-projects/uni/t-0188-artifacts/linux-17c9/`（约 1.5 GB，仓库只索引）。镜像 `uc-gui-go-linux-build:17c9-wm`，镜像 ID `sha256:d797f33c3cdcfd81e1daea096d6de93e823675a44731def11c355ec7586b5a5a`，Openbox 3.6.1（`openbox --version`），wmctrl 1.07，`wmctrl -m` 报告 `Name: Openbox`，4 个桌面；Xvfb 1920x1200。窗口管理器确实管理真实 GUI 窗口：主窗口和面板都在 `_NET_CLIENT_LIST`，带 `_NET_WM_DESKTOP`、`_NET_FRAME_EXTENTS`，面板有 `_NET_WM_STATE_ABOVE`。daemon 沿用 17c5 的 release 构建（SHA-256 `ea0f0bcb…29f6c6`，未重建、未执行 `--version`）。

| 工件 | 来源 | 结论 |
| --- | --- | --- |
| `red1` | base 构建（`439bc0ce2`，干净树） | 44 项里 9 项失败。其中 Escape 未隐藏、焦点未归还、再次显示失败是 **测试前提缺陷（fixture）**：新启动没有内容授权，面板显示的是“解锁”视图（没有 Escape 处理器）。不是产品缺陷。缩小失败是真实产品缺陷。 |
| `red2` | 同一 base 二进制，加入页面自身的按键/焦点/缩放观测（仅改外部 Python，`red2/PROVENANCE.txt`） | 页面收到了 `keydown Escape`，`localStorage` 缩放值正确（0.9/0.8 已保存），页面 `innerWidth` 仍是 800：请求到达，窗口没变小。 |
| `red3` | 同一 base 二进制，Escape 诊断 | 页面文本是“Unlock to access your clipboard history”，直接调用宿主 `dismiss_quick_panel` 可隐藏：Escape 失败 = 解锁视图。 |
| `red4` | 解锁作为前提（与 `linux_wayland_run.py` 同一做法，真实宿主命令 `unlock_content`） | Escape/焦点/失焦隐藏/工作区全绿；再次显示偶发“空”来自测试里 hide 完成前就按下一次快捷键，不是产品缺陷。 |
| `red5` | 每次 hide 都等宿主状态与 X 端都确认隐藏后再继续，并断言 | 再次显示的偶发失败消失，只剩缩小失败（6 项）。 |
| `red6-final-runner-base` | 最终场景 + base 二进制 | 7 项失败，全部是缩小（<1.0）相关（含 0.8 的固定尺寸对照，因为窗口停在 800x560）。 |
| `green1`（`6ca0ffae3`） | 在 `SetSize` 前后临时把窗口设为可缩放再立即锁回 | 无效，仍 6 项失败。 |
| `dbg1`（临时 DEBUG） | 同上加日志 | 请求 720x504 / 640x448 到达，`WindowDidResize` 报告目标尺寸，**随后立刻** 又报告 800x560。结论：窗口缩小成功，重新锁定后回到 GTK 的创建尺寸。 |
| `green2`（`85a7efc82`） | 以 resize 事件重新锁定 | 无效，同上原因。 |
| `green3`（`76475f279`） | 面板永久 toolkit 可缩放 | 因果实验：57 项全过，证明“非可缩放标志”是阻碍。**不是最终方案**：`green3-negctl`（加入窗口管理器与客户端的 resize 请求负对照）显示三处“固定尺寸”检查失败，窗口管理器能把面板改大，违反固定窗口契约。 |
| `green4`（`a42d5315f`） | 几何提示 min = max = 目标（Wails `SetMinSize`/`SetMaxSize`），创建时就设到所有 Linux 窗口 | X11 61/61 通过；但 Wayland 回归套件 `wayland-fix4` 中 F7 `place-out2-small-output-cap-720x400` 失败：Layer Shell surface 被限制到 720x400，低于 800x560 的最小提示。**回归是本片引入的**。 |
| `green6`（`fix5`，`2c6eeb558`，干净树） | 最终：只对普通 X11/XWayland 面板窗口设几何提示，Layer Shell 路径保持原样 | X11 61/61；`wayland-fix5` 31/31（与 17c8 的 31/31 同一套场景）。 |

## 根因（GTK + Wails 源码与最小实验）

- 面板创建时 `DisableResize: true`，Wails 把它变成 `gtk_window_set_resizable(FALSE)`。GTK 对不可缩放窗口把 `WM_NORMAL_HINTS` 的最小/最大都钉成当前尺寸（X 端实测：`minimum size: 800 by 560`、`maximum size: 800 by 560`）。
- gtk3 构建的 Wails `SetSize` 是 `gtk_window_resize`。对不可缩放窗口只能放大（放大看起来正常），不能缩小。最小 GTK 实验（纯 GTK3，无 Wails，Openbox 下）：不可缩放窗口 `set_default_size` 或 `resize` 缩小无效；临时 `set_resizable(True)` 同一步里再锁回无效（相邻主循环轮次内锁回早于 resize 完成）；等 resize 完成再锁回会回到默认尺寸（默认尺寸没有被 `gtk_window_resize` 改写）。
- Tauri（tao）面板同样是不可缩放窗口，但 tao 的约束机制不同；这里不假定 Tauri 在 GTK 层如何实现，只对齐它的 **行为**：固定尺寸、缩小有效。

## 修复

`apps/gui-go/windows.go`：Linux 普通窗口面板（`!layerPanelActive()`）不再依赖 GTK 的不可缩放标志，改用 Wails 内建的 `SetMinSize`/`SetMaxSize` 几何提示固定尺寸：每次设置尺寸时先放开最小值、设最大值为目标、`SetSize`，再把最小值设为目标（`setPanelSize`，在 `showQuickPanel`、`set_quick_panel_layout` 与创建后各调用一次）。窗口管理器因此仍不能改变面板尺寸，窗口缩放变小也能生效。Layer Shell 路径、macOS、Windows 未改。没有新增单元测试（行为只能在真实 X11 窗口管理器下观察）。

## 最终结果（`green6` 与重复运行 `green5`，同一 `fix4`/`fix5` 构建族）

- X11 + Openbox，61 项全过（`green6`；`green4`、`green5` 在 `fix4` 上同样 61/61，该构建的 Wayland 回归如上）。
- 首次映射：默认 800x560；重启后分别为 880x616（1.1）、640x448（0.8）、1200x840（1.5）。每次隐藏再显示尺寸不变。
- 实时缩放：1.0→1.1→…→1.5 与 1.1→1.0→0.9→0.8 逐步按键，已映射的面板每一步都改变到 `round(800×s)×round(560×s)`；越界按键（0.8 再减、1.5 再加）保持不变（钳制）。
- 固定尺寸：在 1.0、0.8、1.5 三处，用 `wmctrl -e`（EWMH `_NET_MOVERESIZE_WINDOW`）和 `xdotool windowsize`（客户端 `XResizeWindow`）请求改变面板尺寸，尺寸都不变；同样请求对普通可缩放窗口（GTK 目标窗口，400x200→520x290 / 300x120）有效，所以负对照有判别力。
- 工作区：桌面 0、1、2 各按一次快捷键，面板出现在当前桌面（`_NET_WM_DESKTOP` 等于当前桌面）、800x560、是活动窗口。
- 焦点/显示隐藏：见上，`Escape` 经真实 XTEST 到达页面；隐藏后窗口管理器把焦点交还上一个目标窗口；另一个窗口取得焦点时面板隐藏。
- 面板首次映射在窗口管理器下的结构事件（客户端窗口被窗口管理器重新父化到框架窗口，根窗口只看见框架窗口）保存在 `xev-root-substructure.log` 与 `linux-assertions.json` 的 `panel_x11_timeline`；尺寸判定以 `wmctrl -lG`（EWMH 管理的客户端窗口）采样为准。

## 边界（OPEN，没有删减目标）

- Openbox 是一个 X11 窗口管理器：GNOME/KDE/Xfwm、Mutter 的 XWayland、原生实体桌面、Wayland 原生窗口管理（Layer Shell 之外的路径）、多显示器、HiDPI 显示器缩放（`GDK_SCALE`）都没有验证；本片的“窗口缩放”是前端状态，与显示器 DPI 无关（契约 W1）。
- 工作区只验证了“当前工作区出现并获得焦点”；Tauri 没有 sticky，本片也没有添加。离开面板所在桌面时面板被失焦路径隐藏，是观测事实。
- 跨应用粘贴（真实“上一个应用”）需要真实合成器/窗口管理器的额外边界，本片没有用脚本化 IPC 当证据，也没有验证。
- 真实 GPU、AppImage 内重跑、原生 amd64。
- 窗口缩放与文本缩放是两件事；文本缩放没有在本片验证。
- 几何提示在 Wayland 的 XWayland 回退窗口上的行为没有单独验证。

## 复跑

```bash
apps/gui-go/e2e/linux/run_17c9.sh image                         # Dockerfile.17c9-wm：17c2 镜像 + openbox + wmctrl，打印镜像 ID
apps/gui-go/e2e/linux/run_17c9.sh build <新标签> <新目录>        # 真实前端（VITE_GUI_GO_E2E=1）+ 容器内构建 + 来源记录；标签不可复用
apps/gui-go/e2e/linux/run_17c9.sh run <标签> <新目录>            # Xvfb + Openbox 内的场景（退出码即断言结果）
apps/gui-go/e2e/linux/run_17c9.sh wayland <标签> <新目录>        # 既有 sway 套件，影响面核对
```
