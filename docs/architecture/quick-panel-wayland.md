# Wayland 快捷面板

快捷面板继续使用 React 与 Tauri WebView。在支持 Layer Shell 的 Wayland 合成器中，GTK 窗口使用原生 Layer Shell surface；X11 和不支持该协议的桌面使用普通窗口。

## 窗口生命周期

`src-tauri/crates/uc-tauri/src/quick_panel/linux.rs` 在快捷面板同步创建期间注册 GTK `Application::window-added` 信号，先初始化 Layer Shell，再由 Tauri 完成窗口和 WebView 创建。钩子只处理第一个窗口，创建结束后立即断开，并校验它与 Tauri 返回的窗口相同。不得在已经实现底层资源的 GTK 窗口上初始化 Layer Shell，也不得把 WebView 移入不受 Tauri 管理的窗口。

初始化完成后允许 GTK 内部调整大小：否则不可调整大小的 GTK 窗口会保留 WebKit 的自然尺寸，使小屏高度上限失效。Layer Shell surface 不具备普通桌面窗口的交互缩放边框，实际大小仍由本模块的尺寸请求控制。

面板使用 overlay 层，不预留桌面工作区；显示期间保持独占键盘交互。每个输出上的透明 GTK 背景窗口位于面板下方，只接收外部点击，不承载 WebView，也不获取键盘焦点。点击背景会关闭面板及全部背景窗口；首次点击仅用于关闭，不透传到其他应用。背景窗口由面板拥有，隐藏、销毁和创建失败时均释放。

不能在获得焦点后立即切换为按需交互：Hyprland 的鼠标重新定位可能把键盘焦点交回其他应用。只保持独占交互也不够，因为外部点击不会自动关闭面板。透明背景窗口使两种交互各有明确的处理入口。

Linux 面板打开即同时显示左侧历史和右侧预览，默认尺寸为 800 × 560 逻辑像素，并跟随应用界面缩放。Layer Shell 后端在每次打开时读取鼠标所在输出的 GTK 逻辑工作区，缩放后的尺寸分别限制为可用宽度的 90% 和高度的 80%；大屏保持默认尺寸，不随分辨率膨胀，也不重复乘系统缩放。GTK 若不能提供排除桌面栏的工作区，则使用其报告的输出区域。历史与预览按 45%／55% 分配宽度，各自在内部滚动。两栏共用连续背景与外边框，内部仅保留分隔线；没有条目时右侧显示空状态。选择条目直接更新预览，不等待浮动面板的展开延时，也不改变窗口尺寸或位置。macOS 和 Windows 保留原有按需展开的浮动面板设计。

位置与尺寸使用输出内的逻辑坐标、Layer Shell 边距和 GTK 尺寸请求；界面缩放时按输出边界约束位置。普通窗口的 `set_position()` 不参与这一后端。

GTK3 运行库 `libgtk-layer-shell.so.0` 按需加载。库不可用时会记录能力降级事件；GTK 已安装的回调要求库在进程生命周期内保持加载。GTK4 的同名用途库不能替代 GTK3 版本。

Deb、RPM、AUR 和 Nix 包装声明了此运行时依赖。直接运行开发二进制或 AppImage 时，需要确认宿主或包内可以找到该库；动态加载的库不会自动成为 ELF 链接依赖。

## Omarchy 配置

以下针对使用 Lua 配置的 Omarchy / Hyprland 0.56。先安装 GTK3 Layer Shell：

```bash
omarchy pkg add gtk-layer-shell
```

安装后重新启动 GUI，使预创建的面板采用新的窗口后端。

在个人 `~/.config/hypr/bindings.lua` 中为 GUI 的现有入口绑定快捷键，例如：

```lua
o.bind("SUPER + SHIFT + V", "UniClipboard", "uniclipboard --quick-panel")
```

使用前先检查该组合是否已有绑定；如有冲突，应选择空闲组合，或者明确取消旧绑定后替换。GUI 必须启用快捷面板；重复执行该命令切换面板显示状态，未运行时启动 GUI。开发模式若禁用了单实例机制，应通过生产构建或显式启用单实例验证此入口。

不需要针对快捷面板添加浮动、居中窗口规则。Layer Shell namespace 为 `uniclipboard-quick-panel`，可通过 `hyprctl layers -j` 验证。Wayland 下应用内的 X11 全局快捷键注册被禁用，设置页显示桌面配置入口；单独修饰键双击仍受 Wayland 输入隔离限制。

## Omarchy 主题跟随

在 Omarchy 会话中，外观设置选择“跟随系统”后，主窗口、快捷面板和更新窗口同时跟随桌面深浅模式与配色。选择固定浅色或深色后，使用原有应用预设和自定义颜色；回到“跟随系统”时恢复当前桌面配色。主题切换无需重启，隐藏的快捷面板也保持订阅。快捷面板的主题生命周期由窗口根组件拥有，首次绘制前先应用系统深浅色，连接中和连接失败页面也接收桌面配色；daemon 就绪后再读取用户主题设置，进入历史页面时不重建主题订阅。

该适配仅由 GUI 的 `src-tauri/crates/uc-tauri/src/desktop_theme/` 管理：检查会话的 `OMARCHY_PATH` 与当前主题目录，从用户主目录下的 `.local/state/omarchy/current/theme/colors.toml` 读取调色板。只安装 Omarchy 包、未进入 Omarchy 会话时不开启；其他系统不提供配色覆盖。此入口针对使用上述状态目录的 Omarchy 版本。

Omarchy 会整体替换主题目录，因此监听其稳定父目录并合并文件事件。读取失败或主题内容无效时保留最近一次有效配色；后续文件变化会重新读取。调色板仅驻留内存，不写入业务设置，不修改系统 GTK 配置，不安装主题钩子。

前端通过 `src/lib/window-theme.ts` 统一选择最终主题，窗口只消费深浅模式与语义颜色变量。初始查询和实时事件带版本号，避免旧查询覆盖新主题；GUI 退出时取消文件监听。daemon、Engine 和其他平台不承担 Omarchy 适配逻辑。

## 粘贴与能力边界

Hyprland 后端在显示前记录原窗口身份，选择条目后先恢复系统剪贴板，再隐藏面板、释放键盘交互，并在工作线程中确认原窗口仍存在、恢复焦点、验证身份，最后向该窗口发送粘贴快捷键。整个流程不通过 shell 执行命令，不把正文或窗口标题放入命令及日志。常见终端使用 `Ctrl+Shift+V`，普通应用使用 `Ctrl+V`；用户自定义的粘贴按键可能不同。

合成器 IPC 位于 `crates/uc-desktop/src/hyprland.rs`，不依赖 GUI 框架。当前输入适配使用 Hyprland Lua dispatcher（0.55 及以上）；其他支持 Layer Shell 的桌面可以显示面板，但自动粘贴尚不支持，可使用复制操作。文件路径直接键入功能未在 Linux 实现。

多屏选择在 Hyprland 下根据鼠标位置进行；其他合成器缺少全局鼠标查询时使用默认输出。GTK 提供输出的逻辑尺寸，避免重复应用缩放系数。实际多屏、分数缩放、不同输入法及 XWayland 目标应用仍需分别验证。

## 验证

```bash
cargo test -p uc-desktop hyprland
cargo test -p uc-tauri quick_panel
cargo test -p uc-tauri --test specta_export
cargo run -p uc-tauri --example layer_shell_smoke
```

冒烟程序使用合成测试页面与独立按键接收窗口，不启动 daemon、不读取用户历史、不写入系统剪贴板；依次检查显示、键盘焦点、布局更新、自动粘贴按键送达、再次显示，以及通过 GTK 信号触发的关闭回调和背景窗口清理。应在支持 Layer Shell 的 Hyprland 会话内运行，且测试期间不要主动切换焦点。

上述流程已在 Hyprland 0.56.1、Tauri 2.11.5 / Tao 0.35.3 下通过。GTK 信号验证不等同于合成器的实际鼠标事件验证：当前虚拟鼠标测试工具在独立普通 GTK 窗口中也未产生点击回调，因此真实点击关闭、输入法及多屏组合仍需交互验收。
