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
| W8 | 在 X11 上缩放对 GTK 窗口只是请求；Tauri 用 `window.set_size`（tao 0.35 `WindowRequest::Size` 调用 `gtk::Window::resize`，即 `gtk_window_resize`），Wails beta.28 的 `SetSize` 是 `gtk_window_set_default_size`。 | tao 0.35.3 `src/platform_impl/linux/event_loop.rs`；`wails/v3@v3.0.0-beta.28/pkg/application/linux_cgo.go`（`setSize`） |

## 预计失败方式（修改前，E2E 先行）

1. W4/W8：面板已映射时按 `ctrl+=`，Go 侧 `set_quick_panel_layout` 只调用 `w.SetSize`（默认尺寸），预计已映射窗口不改变尺寸（静默无效）。
2. W4：隐藏后再次显示时，默认尺寸是否在 GTK 重新映射时被重新采用，未知；预计可能仍是旧尺寸。
3. W5：重启后第一次映射是否直接是已保存缩放的尺寸，未知。
4. W6：切换工作区后快捷键是否让面板出现在当前工作区，未知。
5. W7：Openbox 的焦点窃取保护是否拒绝 `gtk_window_present` 的激活请求，未知。

这些是需要事实回答的问题；不预设哪一个一定失败，也不为了制造修复而改业务代码。

## 复跑

```bash
apps/gui-go/e2e/linux/run_17c9.sh image                       # Dockerfile.17c9-wm：17c2 镜像 + openbox + wmctrl
apps/gui-go/e2e/linux/run_17c9.sh build <tag> <artifact-dir>  # 真实前端 + 容器内构建 + 来源记录
apps/gui-go/e2e/linux/run_17c9.sh run <tag> <artifact-dir>    # Xvfb + Openbox 内的场景
```
