# Go GUI 托盘图标：一个字形（one glyph）与历史猫头

状态：默认图标是一个固定字形（内容不变、没有状态和动画，由代码在运行时渲染）。用户可以在设置中改用历史猫头（`general.trayIcon`，见「图标样式偏好」节）。失败模型与验收契约先于实现落盘，证据与边界见文末「结果」节。

## 范围

默认样式 `glyph` 是设计里“一个字形”方案的“同步完成”静止形态，两张错位的卡片（一份剪贴板在多台设备上），描边 2、圆角 3.5、圆头。它不依赖 daemon 的任何状态，也没有动画。

设计稿的其余内容没有实现：另外 8 个状态（传输中、收到新内容、已暂停、仅局域网、离线、暂不记录、已锁定、需要处理）、右下角标和动效。需要时再做，并且要先有产品决定：这些状态同时成立时显示哪一个（例如“已锁定”与“已暂停”同时成立），以及“暂不记录”“内容被拦截”目前没有 daemon 事实来源。

此前评估过另一个方向（“实心猫”，B solid cat）并实现过它的 9 状态、耳朵动效和 daemon 状态订阅，最终选定字形方案，实心猫的实现已删除，也从未进入任何发布版本，不得用作 `cat` 样式。它们在提交历史里：猫的实现（含 9 状态、耳朵动效和 daemon 状态订阅）在 `c7209b188..a16a466e7`（`git log c7209b188..a16a466e7 -- apps/gui-go`），猫与字形并存的一版是 `90b57af6d`。

## 设计来源（可验证快照）

- 设计稿：Claude Design 画布 `https://claude.ai/artifact/5GGBoRRmhDyHDwJa8fc31y`，读取时的版本 `1791410974-e2c2`；画布注记 `Tray icon — one glyph, nine states`，画板 `TrayIconBuild`（构造）与 `TrayIconStates`（9 状态 × 5 环境）。
- 画板源码的哈希和验收用的参照（`glyph.svg`、独立栅格化的 `glyph.png`、`SHA256SUMS`、`SOURCE.md`）在 `apps/gui-go/e2e/tray-icon-design/`。
- 几何以设计画板 SVG 的路径数据为准，写入 `apps/gui-go/tray_icon_design.go`：前面的卡片是圆角矩形 `x7 y3 w13 h15 rx3.5`，后面的卡片是路径 `M4 7.5v10A3.5 3.5 0 0 0 7.5 21H15`，都是描边、不填充。

## 实现

- 一份渲染器：`tray_icon_raster.go` 解析设计的 SVG 路径并栅格化描边（圆头圆角；描边由许多互相重叠的小块组成，所以按 4 倍分辨率绘制再平均，单次光栅化会在重叠处把部分覆盖相加，使边缘偏重），`tray_icon_design.go` 画字形，各平台文件只决定尺寸与颜色。
- 平台适配：
  - macOS：模板图（只用 alpha，菜单栏深浅色自动着色），保留 44 × 44 px（2x、22 pt）图像，按设计的 36 px（18 pt）网格居中绘制，两侧网格留白各 4 px。字形几何约 13.5 × 15 pt；含抗锯齿的 alpha 包围盒为 28 × 30 px（14 × 15 pt）。此前把网格放大到 44 px 的实现导致图标偏大（#2135），现恢复设计尺寸。尺寸常量与生成函数位于不受平台构建标签限制的 `tray_icon_macos.go`，Linux CI 也能验证 macOS 几何。
  - Windows：按当前任务栏主题画一份（浅色 `#1B1B1B`，深色 `#FFFFFF`），尺寸取系统小图标尺寸（`SM_CXSMICON`，系统 DPI）；任务栏主题变化时（`SystemThemeChanged`）在独立 goroutine 里重画。不用 Wails 的亮/暗两份图标：读源码可知，运行期调用 `SetIcon` 或 `SetDarkModeIcon` 时，只要两个模式共用一个句柄，后设置的就会同时替换两个模式，两份图标无法保持分开。
  - Linux：22 px，StatusNotifierItem 忽略模板标志，必须给出显式颜色。`XDG_CURRENT_DESKTOP` 按 `:` 分段、不区分大小写匹配 `KDE` 时，按 XDG 配置目录优先级读取 `plasmarc` 的 `[Theme] name`；缺失则取 `kdeglobals` 的 `[KDE] LookAndFeelPackage` 对应默认值中的 `[plasmarc][Theme] name`，仍缺失则用 `default`。按 XDG 数据目录优先级读取 `plasma/desktoptheme/<name>/colors` 的 `[Colors:Window] BackgroundNormal`；无有效颜色则取 `kdeglobals` 同名颜色，再取 `[General] ColorScheme`（缺失时取外观包默认值）对应的 `color-schemes/<Scheme>.colors`。以 KDE 的 `qGray=(r*11+g*16+b*5)/32 < 192` 判定深色面板，用浅色图标 `#CDD6F4`，否则用深色 `#1B1B1B`。未能解析 KDE 颜色及非 KDE 桌面保持原有 `gsettings` 规则：仅 `prefer-light` 用深色图标，其余用浅色。`SystemThemeChanged` 触发重画；KDE 另每秒按路径检查各配置目录的 `plasmarc`、`kdeglobals` 的修改时间、大小和存在状态，变化时重画当前样式。
- Entry points: `initTray` in `tray.go` calls `newTrayIcon(...).show()` to show the default glyph; `applySettings` in `settings_watch.go` calls `setStyle` to apply `general.trayIcon`. `show` and `setStyle` in `tray_icon.go` render and apply under one lock, and a theme repaint draws the style currently shown. The cat is in `tray_icon_cat.go` and in `catTrayImage` of each platform file.

## Tray icon style preference (`general.trayIcon`)

- Values: `glyph` (the default, the glyph above) and `cat` (the cat shipped in v1.1.2). The daemon's product settings store the preference (ADR-013). The host only reads it and never writes it back.
- Cat image: `apps/gui-go/icons/tray-cat@2x.png`. It is byte-identical to the macOS tray image of the retired host at tag `v1.1.2` (git blob `b745edc61efe18b8895d427830919280d62693b9`): 44 x 44, black pixels only, shape in the alpha channel (a template image). The source note and the hash check script are in `apps/gui-go/icons/TRAY-CAT-SOURCE.md`. v1.1.2 used it on macOS only (`include_bytes!` references `@2x` only); Windows and Linux showed the application icon.
- Per platform:
  - macOS：运行时复用猫头的 alpha 缩放路径，以 Catmull–Rom 把整张原图缩到 34 × 34 px，居中放入 44 × 44 px 透明画布（四周各 5 px），填充黑色后交给 `SetTemplateIcon`。alpha 包围盒从原图的 40 × 39 px（20 × 19.5 pt）缩到 32 × 31 px（16 × 15.5 pt）；原图略不对称，最终可见区域的上下留白相差 1 px。嵌入资源的字节与来源校验保持不变。
  - Windows and Linux: the host takes the image's alpha, scales it to the size from `trayIconSpec()` and fills it with that spec's foreground colour. This is a new rendering derived from the macOS image; v1.1.x never shipped a cat on these platforms.
- Apply: the host calls `applySettings` at startup and after every read of the authoritative settings. The tray icon executor compares with the style it shows and makes no native call when nothing changed. A lightweight cold start has no GUI process and therefore no tray owner; that is not an apply failure.

## 失败模型

- **F1 模板图 alpha**：macOS 模板图只读 alpha。预期：交给托盘的图像里，卡片内部（只有描边的地方）alpha 为 0，描边中心线 alpha 为 255（采样点选在 16 px 以上的尺寸都落在整像素上的位置）。
- **F2 菜单被图标刷新牵连**：换图标若走 `SetMenu` / `Menu.Update`，会在菜单展开时关闭子菜单（t-0188 已修复的缺陷）。预期：只调用图标 API，不碰菜单发布路径；启动、样式偏好变化、Windows 任务栏主题变化、Linux 系统主题事件或 KDE 配置变化时设置图标。
- **F3 主线程自锁**：Wails 的图标调用在主线程执行并等待，应用事件回调可能就在主线程上。预期：Windows、Linux 的主题变化回调通过独立 goroutine 调用 `show`，回调本身不等待；KDE 轮询复用同一回调。
- **F4 平台差异**：macOS 模板图随菜单栏自动着色；Windows 按任务栏主题着色；Linux 忽略模板标志，必须显式着色。
- **F5 重复来源**：字形的几何只有一份，各平台只在尺寸与颜色上不同。
- **F6 持久化**：图标不写任何东西到磁盘或数据库，没有持久化明文的问题。
- **F7 尺寸**：图像被边缘裁切，或在 macOS 上大于原生菜单栏图标。预期：两种样式的生产帧均有透明留白，macOS 上可见部分的长边为 15 到 16 pt。

- **F8 Field path mismatch**: if the JSON path of `View.General.TrayIcon` differs from the daemon's, it decodes to an empty string without an error. Expected: the real-daemon Go e2e asserts the decoded value with the non-default value `cat`.
- **F9 Unknown value**: a value other than `glyph` or `cat` (or an empty string) is read. Expected: the glyph is shown; the log says only "unknown style" and never prints the received string.
- **F10 Duplicate calls**: repeated reads of the same value, or several reads after one change (the host's own write and the daemon event). Expected: zero native icon calls while the style is unchanged; the comparison is against the style the tray shows, not the last requested one.
- **F11 Apply failure**: decoding, scaling or PNG encoding of the image fails. Expected: an error without the setting value is logged, the shown style is not updated and the next read tries again; the saved preference stays as it is and the host does not write to the daemon. Wails' `SetIcon` / `SetTemplateIcon` return no error, so a failure inside the native layer cannot be detected.
- **F12 Theme repaint with the wrong style**: the Windows taskbar theme changes. Expected: the style currently shown is repainted, not the glyph.
- **F13 Concurrency**: a theme repaint and a style change happen together. Expected: both render and apply under the same lock, the latest style wins, and an event handler does not wait for the icon call on the main thread (as F3).
- **F14 Startup order**: `initTray` shows the glyph first and switches to the cat after the first settings read. Expected: a user who chose the cat may see the glyph briefly at startup; this is a known boundary, and the tray still has an icon when the daemon is unavailable.
- **F15 Image source**: the unshipped solid-cat vector is used by mistake. Expected: the sha256 of the embedded file equals that of `tray-icon@2x.png` at `v1.1.2` (`21e459b6f6fd8fa4de3335370f8198a400ae7d555550388cf48d239d4e34a93b`).

## 验收契约

1. 渲染对照（e2e 构建的 `tray-icon-compare`）：形状对照，用设计的 44 px / 36 px 几何和 macOS 浅色配色渲染，与独立栅格化的设计画板 `glyph.png` 逐像素比对，30% 容差下差异像素不超过 30；以及生产帧检查，按托盘实际收到的图像（`trayIconSpec`）检查模板图 alpha 采样点（透明点在前面卡片的内部，实心点在它左边描边的中心线上）、不被图像边缘裁切，并且在 macOS 上长边为 15 到 16 pt。形状对照只验证轮廓，不代表菜单栏里的实际尺寸。
2. 导出（`tray-icon-export`）：平台实际交给托盘的图像与设计几何下的参照图都能导出为 PNG。
3. 原生状态项（真实 Go GUI、独立 profile、同一个真实 daemon）：截取这个 pid 自己状态项周围的一小块菜单栏，与设计对照。
4. 菜单不回退：右键打开菜单（十个条目，八项加两条分隔线；标签随会话语言，所以只检查结构）；菜单里的同步开关使 daemon 的 `syncEnabled` 翻转并恢复；Quit 使这个 GUI 进程以 0 退出，对应的 daemon 进程消失。
5. 边界：只在 macOS 本机实测；Windows 做 `go vet`，Linux 在测试主机上做 `go vet` 与 `go build`，运行结果写在文末。

6. Tray icon style (`general.trayIcon`):
   - Persistence and events: in the real-daemon Go watcher e2e, a write of `cat` decodes to `View.General.TrayIcon == "cat"` (F8).
   - Frontend save path: `apps/gui-go/frontend/e2e/tray-icon-settings-api.mjs` runs the real frontend settings modules against an isolated real daemon and records the request body, the HTTP result, the value read back and the hash of `desktop-settings.json` for `cat` and `glyph`.
   - Executor: an isolated test replaces the native call with a recorder and covers F9-F12. It proves the number of calls and the images passed, not what the tray displays.
   - 图像：来源哈希脚本（F15）；`TestTrayMacImageGeometry` 在各平台验证两种 macOS 样式的 44 px 画布、alpha 包围盒、纯黑像素与抗锯齿边缘。包围盒同时检查 alpha > 0 与原生 e2e 使用的 alpha > 20：字形为 `(8,7)-(36,37)`，猫头为 `(6,6)-(38,37)`，右下界不包含在内。
   - The native display is accepted by the user by hand: switching between glyph and cat, keeping the style after a restart, silent start, and light/dark theme changes.

## 结果

以下 `run15` 至 `run17` 是 #2135 修复前、macOS 网格仍为 44 px 时的历史证据，不代表当前尺寸的原生验收。

证据在任务库 `library/run15/`（源码 `c54361f68`）与 `library/run16/`（源码 `348415f90`，只比前者多删除未用的路径命令与参数、改了一个采样点），每次运行的 `provenance.json` 记录源码提交、产物哈希与工具。

- 渲染对照：两次运行都通过。形状对照差异像素为 0（容差 30%）；生产帧（macOS 44 px 图像、44 px 网格）模板图 alpha 采样点为 0 与 255，不被裁切，长边 38 px（19 pt），短边 34 px。
- 原生（`run15`，屏幕未锁、显示器唤醒）：这个 pid 自己的状态项被截取，右键打开真实菜单且条目结构正确，菜单里的同步开关使 daemon 的 `syncEnabled` 翻转并恢复，Quit 使 GUI 以 0 退出且对应 daemon 进程消失。截图里是字形在真实菜单栏上的样子。
- `run16` 时屏幕又被锁住，原生检查报告为受阻，没有绕过，所以最终源码上没有原生运行；`run15` 之后只改了未使用的路径命令与参数、采样点和文档，渲染对照在 `run16` 里重新通过。
- `go vet`（默认与 `-tags=e2e`）在 macOS 通过。`GOOS=windows go vet` 在 main 上已有的 `validateIsolation` 重复定义处中止，所以 Windows 的图标文件没有被类型检查到；这不是通过。

未验证：macOS 浅色菜单栏的外观（`run15` 的菜单栏是深色外观）；Windows 运行时（任务栏主题切换、小图标尺寸，16 px 时字形线条是否清晰，以及不同显示器 DPI 不重画这个已知边界）；真实 KDE/GNOME 桌面的 Linux 运行时（包括主题切换时延与菜单保持展开），以及精简后的源码在 Linux 上的构建（测试主机当时不可达）。Linux 新增单元测试仅覆盖配置检测、颜色选择与文件指纹变化，不代表原生运行时验收；非 KDE 栏独立配色（如 XFCE/waybar）、忽略 `colors` 文件的第三方 Plasma SVG 样式、壁纸上的半透明面板仍是边界。

合并 `origin/main` 之后（普通 merge，源码 `7f71d7bbc`）：

- 原生（`run17`，屏幕未锁）：同样 8/8 通过，最终源码上有了原生运行。
- macOS 打包：`package.py` 原本还复制已删除的托盘 PNG，已删掉那两行；用现有隔离入口（`stage-daemon.mjs`、`package.py bundle --variant acceptance`、ad-hoc `sign`、`macos_bundle_run.py`）验证，37/37 通过，公证检查跳过，没有遥测密钥输入。
