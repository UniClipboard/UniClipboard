# Go GUI 托盘图标：实心猫（B solid cat）

状态：已实现为一个静态图标。失败模型与验收契约先于实现落盘，证据与边界见文末「结果」节。

## 范围

托盘只显示一个图标，是设计里的“同步完成”静止形态。同一份设计稿里有两个方向，都实现了，用来在真机菜单栏上对比，选定后删掉另一个：

- 实心猫（`B solid cat`，默认）：圆脸、两只耳朵向下、两只挖空的眼睛。
- 一个字形（`one glyph`）：两张错位的卡片，描边 2、圆角 3.5、圆头。

用环境变量 `UC_TRAY_ICON=glyph` 启动时显示字形，不设或其他值显示猫（`designFromEnv`，只在进程启动时读一次）。图标不依赖 daemon 的任何状态，也没有动画。

设计稿的其余内容没有实现：另外 8 个状态（传输中、收到新内容、已暂停、仅局域网、离线、暂不记录、已锁定、需要处理）、新内容圆点和耳朵动效。需要时再做，并且要先有产品决定：这些状态同时成立时显示哪一个（例如“已锁定”与“已暂停”同时成立），以及“暂不记录”“内容被拦截”目前没有 daemon 事实来源。

做过但已撤销的完整实现（9 状态、耳朵动效、daemon 事实订阅、WebSocket）在提交历史里，范围是 `53cfd9794` 到 `a16a466e7`，含当时的状态映射文档与多轮独立评审结论；撤销它同时去掉了托盘新增的控制 WebSocket 连接，因此不再有控制租约的影响。

## 设计来源（可验证快照）

- 设计稿：Claude Design 画布 `https://claude.ai/artifact/5GGBoRRmhDyHDwJa8fc31y`，版本 `1791372485-ac1a`；锚点标题 `Tray icon — B solid cat · states & ear motion`（画布注记 `rowTrayB`），画板 `TrayBBuild`（构造）、`TrayBStates`、`TrayBMotion`。`TrayCatV3` 只是 A/B/C 方向选择，不取形。
- 画板源码快照与哈希在任务库；本仓库的 `apps/gui-go/e2e/tray-icon-design/` 保存验收用的参照（SVG、独立栅格化的 PNG、`SHA256SUMS`、`SOURCE.md`）。
- 几何以设计画板 SVG 的路径数据为准，原样写入 `apps/gui-go/tray_icon_design.go`，不重画、不调整轮廓。

## 实现

- 24 × 24 网格。实心猫：圆脸、左右耳（压在脸下），眼睛是椭圆挖空（rx 1.3、ry 1.6，圆心 (9, 13.8) 与 (15, 13.8)）。字形：圆角矩形 `x7 y3 w13 h15 rx3.5` 与路径 `M4 7.5v10A3.5 3.5 0 0 0 7.5 21H15`，描边宽 2、圆头圆角、不填充。
- 一份渲染器：`tray_icon_raster.go` 解析设计的 SVG 路径并栅格化，`tray_icon_design.go` 画猫，各平台文件只决定尺寸与颜色。
- 平台适配：
  - macOS：模板图（只用 alpha，菜单栏深浅色自动着色），44 px 图像。网格按设计分别放大并居中：猫 48 px（24 pt），可见部分约 18 × 17 pt；字形 44 px（22 pt），可见部分约 16.5 × 18.3 pt。设计写的是 18 pt 画稿，但猫只占网格的约 74%，实际只有 14 × 13 pt，在 Mac 上明显比旁边的状态栏图标小（用户在真机上看到的反馈），所以放大了网格；这是对设计尺寸的有意偏离。
  - Windows：按当前任务栏主题画一份（浅色 `#1B1B1B`，深色 `#FFFFFF`），尺寸取系统小图标尺寸（`SM_CXSMICON`，系统 DPI）；任务栏主题变化时（`SystemThemeChanged`）在独立 goroutine 里重画。不用 Wails 的亮/暗两份图标：读源码可知，运行期调用 `SetIcon` 或 `SetDarkModeIcon` 时，只要两个模式共用一个句柄，后设置的就会同时替换两个模式，两份图标无法保持分开。
  - Linux：22 px，StatusNotifierItem 忽略模板标志，必须给出显式颜色：默认浅色图标 `#CDD6F4`（设计的深色栏环境），桌面配色方案为 `prefer-light`（`gsettings` 读取）时用深色 `#1B1B1B`；配色在显示图标时读一次。
- 接入点：`tray.go` 的 `initTray` 用 `newTrayIcon(...).show()` 代替原来内嵌的 PNG；`tray_icon.go` 的 `show` 持锁渲染并应用，保证两次主题变化不会让较旧的图像留在托盘上。

## 失败模型

- **F1 模板图 alpha**：设计里的眼睛用背景色填充表达。macOS 模板图只读 alpha，若按原样导出会变成实心猫。预期：导出的 alpha 中眼睛区域为 0，脸为 255。
- **F2 菜单被图标刷新牵连**：换图标若走 `SetMenu` / `Menu.Update`，会在菜单展开时关闭子菜单（t-0188 已修复的缺陷）。预期：只调用图标 API，不碰菜单发布路径；图标只在启动时和（Windows）任务栏主题变化时设置。
- **F3 主线程自锁**：Wails 的图标调用在主线程执行并等待，应用事件回调可能就在主线程上。预期：主题变化回调通过独立 goroutine 调用 `show`，回调本身不等待。
- **F4 平台差异**：macOS 模板图随菜单栏自动着色；Windows 按任务栏主题着色；Linux 忽略模板标志，必须显式着色。
- **F5 重复来源**：猫的几何只有一份，各平台只在尺寸与颜色上不同。
- **F6 持久化**：图标不写任何东西到磁盘或数据库，没有持久化明文的问题。

## 验收契约

1. 渲染对照（e2e 构建的 `tray-icon-compare`），两个设计各做两件事：形状对照，用设计的 44 px / 36 px 几何和 macOS 浅色配色渲染，与独立栅格化的设计画板（`synced.png`、`glyph.png`）逐像素比对，30% 容差下差异像素不超过 30；以及生产帧检查，按托盘实际收到的图像（`trayIconSpecFor`，不是设计几何）检查模板图 alpha（透明点为 0、实心点为 255）、不被图像边缘裁切，并且在 macOS 上可见部分的长边为 17 到 19 pt。形状对照只验证轮廓，不代表菜单栏里的实际尺寸。
2. 导出（`tray-icon-export`）：平台实际交给托盘的图像与参照图都能导出为 PNG。
3. 原生状态项（真实 Go GUI、独立 profile、同一个真实 daemon）：截取这个 pid 自己状态项周围的一小块菜单栏，与设计对照。
4. 菜单不回退：右键打开菜单且标签不变；菜单里的同步开关使 daemon 的 `syncEnabled` 翻转并恢复；Quit 使这个 GUI 进程以 0 退出，对应的 daemon 进程消失。
5. 边界：只在 macOS 本机实测；Windows 做 `go vet`，Linux 在测试主机上做 `go vet` 与 `go build`，运行结果写在文末。

## 结果

对应源码 `e1cddb957` 加上随后删除 `e2e/linux/build_in_container.sh` 里一行旧 PNG 拷贝的提交；证据在任务库 `library/run9/`（`provenance.json` 记录源码提交、产物哈希与工具）。

已通过：

- 渲染对照：静止猫与独立栅格化的设计画板 `synced.png` 一致，模板图眼睛 alpha 为 0、脸 alpha 为 255（`tray-icon-compare`，运行 `run9`）。
- 平台图像与参照图都能导出（`tray-icon-export`）。
- `go vet`（默认与 `-tags=e2e`）在 macOS 通过；`GOOS=windows go vet` 只剩 main 上已有的 `validateIsolation` 重复定义。
- 独立评审（只读）覆盖了这次精简，发现的一处 Linux e2e 构建脚本遗留已修。

未通过 / 受阻：测试 Mac 一直处于锁屏、显示器休眠，状态项不可达，所以原生状态项截图和真实右键菜单（同步开关、Quit）没有验收，运行器的 `preflight` 把它们报告为受阻，没有绕过。

未验证：

- macOS 浅色 / 深色菜单栏的原生外观。
- Windows 运行时（任务栏主题切换、小图标尺寸，以及不同显示器 DPI 不重画这个已知边界）。
- Linux 运行时（StatusNotifierItem 像素图）。精简前的较大版本在 Linux arm64 上做过 `go vet` 与 `go build`（`a16a466e7`）；精简后的源码没有重新在 Linux 上构建，因为测试主机当时不可达。
