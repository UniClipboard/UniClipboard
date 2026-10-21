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
  - macOS：模板图（只用 alpha，菜单栏深浅色自动着色），44 px 图像，网格按 44 px（22 pt）绘制，字形可见部分约 16.5 × 18.3 pt。设计写的是 18 pt 画稿，那样只有约 13.5 × 15 pt，在 Mac 上会比旁边的状态栏图标小（此前的猫方案在真机上就是这个反馈），所以放大了网格；这是对设计尺寸的有意偏离。
  - Windows：按当前任务栏主题画一份（浅色 `#1B1B1B`，深色 `#FFFFFF`），尺寸取系统小图标尺寸（`SM_CXSMICON`，系统 DPI）；任务栏主题变化时（`SystemThemeChanged`）在独立 goroutine 里重画。不用 Wails 的亮/暗两份图标：读源码可知，运行期调用 `SetIcon` 或 `SetDarkModeIcon` 时，只要两个模式共用一个句柄，后设置的就会同时替换两个模式，两份图标无法保持分开。
  - Linux：22 px，StatusNotifierItem 忽略模板标志，必须给出显式颜色：默认浅色图标 `#CDD6F4`（设计的深色栏环境），桌面配色方案为 `prefer-light`（`gsettings` 读取）时用深色 `#1B1B1B`；配色在显示图标时读一次。
- 接入点：`tray.go` 的 `initTray` 调用 `newTrayIcon(...).show()` 显示默认字形；`settings_watch.go` 的 `applySettings` 调用 `setStyle` 应用 `general.trayIcon`。`tray_icon.go` 的 `show` 与 `setStyle` 持同一把锁渲染并应用，主题变化重画的是当前已生效的样式。猫头见 `tray_icon_cat.go` 与各平台文件的 `catTrayImage`。

## 图标样式偏好（`general.trayIcon`）

- 取值：`glyph`（默认，即上文的字形）与 `cat`（v1.1.2 发布过的猫头）。偏好保存在 daemon 的产品配置中（ADR-013），宿主只读取，不写回。
- 猫头素材：`apps/gui-go/icons/tray-cat@2x.png`，与 tag `v1.1.2` 的 `apps/gui/src-tauri/icons/tray-icon@2x.png` 逐字节相同，44 × 44，只有黑色像素和 alpha（模板图）。来源说明与哈希核对脚本见 `apps/gui-go/icons/TRAY-CAT-SOURCE.md`。v1.1.2 只在 macOS 上用它（`include_bytes!` 只引用 `@2x`），Windows 与 Linux 当时显示应用图标。
- 平台处理：
  - macOS：把嵌入的字节原样交给 `SetTemplateIcon`，与 v1.1.2 收到的字节相同。
  - Windows、Linux：取素材的 alpha，按 `trayIconSpec()` 的尺寸和前景色着色缩放后交给托盘。这是由 macOS 素材派生的新呈现，v1.1.x 没有在这两个平台发布过猫头。
- 生效：宿主在启动时和每次读取权威设置后调用 `applySettings`，托盘图标执行层与已生效的样式比较，没有变化就不调用原生接口。轻量模式冷启动时 GUI 进程已经退出，没有托盘所有者，这不算应用失败。

## 失败模型

- **F1 模板图 alpha**：macOS 模板图只读 alpha。预期：交给托盘的图像里，卡片内部（只有描边的地方）alpha 为 0，描边中心线 alpha 为 255（采样点选在 16 px 以上的尺寸都落在整像素上的位置）。
- **F2 菜单被图标刷新牵连**：换图标若走 `SetMenu` / `Menu.Update`，会在菜单展开时关闭子菜单（t-0188 已修复的缺陷）。预期：只调用图标 API，不碰菜单发布路径；图标只在启动时和（Windows）任务栏主题变化时设置。
- **F3 主线程自锁**：Wails 的图标调用在主线程执行并等待，应用事件回调可能就在主线程上。预期：主题变化回调通过独立 goroutine 调用 `show`，回调本身不等待。
- **F4 平台差异**：macOS 模板图随菜单栏自动着色；Windows 按任务栏主题着色；Linux 忽略模板标志，必须显式着色。
- **F5 重复来源**：字形的几何只有一份，各平台只在尺寸与颜色上不同。
- **F6 持久化**：图标不写任何东西到磁盘或数据库，没有持久化明文的问题。
- **F7 尺寸**：图像被边缘裁切，或在 macOS 上比旁边的状态栏图标明显小。预期：生产帧不被裁切，macOS 上可见部分的长边为 17 到 19 pt。

- **F8 字段路径不符**：`View.General.TrayIcon` 的 JSON 路径与 daemon 不一致时会静默解码为空串。预期：真实 daemon 的 Go e2e 用非默认值 `cat` 断言解码结果。
- **F9 未知值**：读到 `glyph`、`cat` 以外的值（或空串）。预期：按 `glyph` 显示；日志只记录"未知样式"，不原样输出收到的字符串。
- **F10 重复调用**：同一值的重复读取、watcher 与页面 `settings://changed` 两个触发同时到达。预期：样式未变化时原生图标调用为 0 次；比较对象是已生效的样式，不是上次请求的样式。
- **F11 应用失败**：素材解码、缩放或 PNG 编码失败。预期：记录不含设置值的错误，不更新已生效样式，下一次读取重试；已保存的偏好保持不变，宿主不写回 daemon。Wails 的 `SetIcon` / `SetTemplateIcon` 不返回错误，所以原生层自身的失败不可检测。
- **F12 主题重画用错样式**：Windows 任务栏主题变化时重画。预期：重画当前已生效的样式，不回到字形。
- **F13 并发**：主题重画与样式切换同时发生。预期：两者在同一把锁下渲染并应用，最后生效的是最新样式；事件回调不在主线程上同步等待图标调用（同 F3）。
- **F14 启动顺序**：`initTray` 先显示字形，首次读取设置后才切换为猫头。预期：选择猫头的用户在启动时可能短暂看到字形；这是已知边界，daemon 不可用时托盘仍有图标。
- **F15 素材来源**：误用未发布的实心猫矢量。预期：嵌入文件的 sha256 等于 `v1.1.2` 的 `tray-icon@2x.png`（`21e459b6f6fd8fa4de3335370f8198a400ae7d555550388cf48d239d4e34a93b`）。

## 验收契约

1. 渲染对照（e2e 构建的 `tray-icon-compare`）：形状对照，用设计的 44 px / 36 px 几何和 macOS 浅色配色渲染，与独立栅格化的设计画板 `glyph.png` 逐像素比对，30% 容差下差异像素不超过 30；以及生产帧检查，按托盘实际收到的图像（`trayIconSpec`，不是设计几何）检查模板图 alpha 采样点（透明点在前面卡片的内部，实心点在它左边描边的中心线上）、不被图像边缘裁切，并且在 macOS 上长边为 17 到 19 pt。形状对照只验证轮廓，不代表菜单栏里的实际尺寸。
2. 导出（`tray-icon-export`）：平台实际交给托盘的图像与设计几何下的参照图都能导出为 PNG。
3. 原生状态项（真实 Go GUI、独立 profile、同一个真实 daemon）：截取这个 pid 自己状态项周围的一小块菜单栏，与设计对照。
4. 菜单不回退：右键打开菜单（十个条目，八项加两条分隔线；标签随会话语言，所以只检查结构）；菜单里的同步开关使 daemon 的 `syncEnabled` 翻转并恢复；Quit 使这个 GUI 进程以 0 退出，对应的 daemon 进程消失。
5. 边界：只在 macOS 本机实测；Windows 做 `go vet`，Linux 在测试主机上做 `go vet` 与 `go build`，运行结果写在文末。

6. 图标样式（`general.trayIcon`）：
   - 持久化与事件：真实 daemon 的 Go watcher e2e 写入 `cat` 后，`View.General.TrayIcon` 解码为 `cat`（F8）。
   - 执行层：隔离测试以记录调用的原生函数代替 Wails，覆盖 F9–F12。它证明调用次数与传入的样式，不证明托盘实际显示的图像。
   - 素材：哈希核对脚本的输出（F15）。
   - 原生显示由用户手动验收：glyph 与 cat 切换、重启后保持、静默启动、深浅色主题变化。

## 结果

证据在任务库 `library/run15/`（源码 `c54361f68`）与 `library/run16/`（源码 `348415f90`，只比前者多删除未用的路径命令与参数、改了一个采样点），每次运行的 `provenance.json` 记录源码提交、产物哈希与工具。

- 渲染对照：两次运行都通过。形状对照差异像素为 0（容差 30%）；生产帧（macOS 44 px 图像、44 px 网格）模板图 alpha 采样点为 0 与 255，不被裁切，长边 38 px（19 pt），短边 34 px。
- 原生（`run15`，屏幕未锁、显示器唤醒）：这个 pid 自己的状态项被截取，右键打开真实菜单且条目结构正确，菜单里的同步开关使 daemon 的 `syncEnabled` 翻转并恢复，Quit 使 GUI 以 0 退出且对应 daemon 进程消失。截图里是字形在真实菜单栏上的样子。
- `run16` 时屏幕又被锁住，原生检查报告为受阻，没有绕过，所以最终源码上没有原生运行；`run15` 之后只改了未使用的路径命令与参数、采样点和文档，渲染对照在 `run16` 里重新通过。
- `go vet`（默认与 `-tags=e2e`）在 macOS 通过。`GOOS=windows go vet` 在 main 上已有的 `validateIsolation` 重复定义处中止，所以 Windows 的图标文件没有被类型检查到；这不是通过。

未验证：macOS 浅色菜单栏的外观（`run15` 的菜单栏是深色外观）；Windows 运行时（任务栏主题切换、小图标尺寸，16 px 时字形线条是否清晰，以及不同显示器 DPI 不重画这个已知边界）；Linux 运行时，以及精简后的源码在 Linux 上的构建（测试主机当时不可达）。

合并 `origin/main` 之后（普通 merge，源码 `7f71d7bbc`）：

- 原生（`run17`，屏幕未锁）：同样 8/8 通过，最终源码上有了原生运行。
- macOS 打包：`package.py` 原本还复制已删除的托盘 PNG，已删掉那两行；用现有隔离入口（`stage-daemon.mjs`、`package.py bundle --variant acceptance`、ad-hoc `sign`、`macos_bundle_run.py`）验证，37/37 通过，公证检查跳过，没有遥测密钥输入。
