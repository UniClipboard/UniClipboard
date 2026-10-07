# Go GUI 托盘图标：实心猫（B solid cat）状态与耳朵动效

状态：失败模型与验收契约（先于实现落盘）。实现、状态映射与证据边界在文末「结果」节随证据补写。

## 设计来源（可验证快照）

- 设计稿：Claude Design 画布 `https://claude.ai/artifact/5GGBoRRmhDyHDwJa8fc31y`，版本 `1791372485-ac1a`；锚点标题 `Tray icon — B solid cat · states & ear motion`（画布注记 `rowTrayB`），所属画板为 `TrayBBuild`（构造）、`TrayBStates`（9 状态 × 5 环境）、`TrayBMotion`（耳朵动效）。`TrayCatV3` 只是 A/B/C 方向选择，不取形。
- 画板源码快照与哈希：由任务库保存（`TrayB*.dc.html`、`canvas.json`、`SHA256SUMS`）；本仓库只引用上面的版本号。
- 几何以设计画板 SVG 的路径数据为准，原样写入 `apps/gui-go/tray_icon_design.go`，不重画、不调整轮廓。

## 要实现的设计

- 24 × 24 网格；整只猫是一块实心：圆脸 + 左右耳（耳朵压在脸下，可独立转动）。
- 耳朵转轴：左 (7.4, 8.6)，右 (16.6, 8.6)；只旋转，不位移、不缩放，最大 18°。
- 眼睛是挖空（透明）：椭圆 rx 1.3 ry 1.6；“需要处理”为 rx 1.6 ry 1.95。构造板写 1.3 × 1.6，状态矩阵的“需要处理”图标用 1.6 × 1.95，以状态矩阵为准。
- 右下角标：圆心 (19, 19)，半径 4.6，外圈 2.4 宽挖空；新内容圆点：圆心 (21.2, 9)，半径 2.3，同样外圈挖空，可与右下角标同时出现。
- 输出尺寸：macOS 18 pt、Windows 16 px（100%）、Linux 22 px。
- 9 个状态（视觉定义见设计画板）：同步完成、传输中、收到新内容、已暂停、仅局域网、离线、暂不记录、已锁定、需要处理。
- 动效（产品时序以画板文字与关键帧条为准；画板里的 CSS 演示循环百分比与之不完全一致，处处以前者为准）：

| 触发 | 时长 | 动作 | 次数 |
| --- | --- | --- | --- |
| 收到新内容 | 520 ms | 右耳 0 → 18° → 0 → 12° → 0，关键帧 0 / 110 / 250 / 370 / 520 ms | 一次 |
| 发送 / 同步完成 | 360 ms | 两耳同时向外 14° 再回，关键帧 0 / 190 / 360 ms | 一次 |
| 传输中 | 1.6 s 一轮 | 左右耳轮流 9° | 最多 3 s，之后只保留角标 |
| 需要处理 | `GET /member/device-group-choices` 的 `deviceTrust.currentChange` 非空（有信任变更等用户决定），或 `inboundPairings` 里有 `awaiting_confirmation` / `needs_attention`（有设备等着被接纳）；`device-trust.changed` 事件触发立即读取。另：投递读到 `failed` 状态（到用户查看为止）。与设计“到用户查看为止”不同，待决定项由 daemon 持有，决定之前一直显示，打开窗口不清除 | `iconFeed.refresh`、`deliveryChanged` |
| 暂不记录 | **没有 daemon 来源**：daemon 没有“前台应用被排除”这个事实。渲染器能画出它（设计对齐检查包含它），但生产代码不会进入该状态，只有 e2e 控制能强制 | 缺口 |

已知缺口与取舍：

- “内容被拦截”（设计里“需要处理”的第三种触发）没有 daemon 事实，未实现。
- 发送失败只认投递视图的 `failed`；接收端文件传输失败的事件不带方向，不算“发送失败”。
- 宿主新增了一条只读的 daemon WebSocket 连接，订阅 `file-transfer`、`clipboard`、`peers`、`device-trust`、`content-lock`、`paired-devices`，断线后每 3 秒重连。每条已认证的控制 WebSocket 在 daemon 里持有一个控制租约（`crates/uc-webserver/src/api/control_lease.rs`）。主窗口关闭只是隐藏、WebView 的连接随窗口一直存在，轻量模式则退出整个 GUI 进程（托盘一起消失），所以托盘连接新增租约的情形只有：GUI 在运行但还没有创建过窗口（静默启动）。目前没有生产路径消费租约数量：`apps/daemon/src/daemon/oneshot.rs` 的自终止与受控重启排空只对 `Oneshot` 常驻模式生效，而代码注释写明在 L8d 之前生产环境没有 `Oneshot` daemon。若以后 GUI 拉起的 daemon 变成 `Oneshot`，这条常驻连接要随受控重启的排空一起让出租约；这是留给 L8d 的约束，不是现在的行为变化。
- 刚启动、连接尚未建立的几秒里，已配对设备都未连接，图标会短暂显示“离线”；这是 daemon 当时报告的事实，没有额外抑制。
- Linux 的图标颜色按桌面配色方案选择（`org.gnome.desktop.interface color-scheme`，进程内只读一次），不能知道状态栏自己的底色，配色方案之后变化需要重启才生效。
- Windows 不使用 Wails 的“亮/暗两份图标”（`SetIcon` + `SetDarkModeIcon`）：读源码可知，运行期调用时只要两个模式共用一个句柄，后设置的图标就会同时替换两个模式，两份图标无法保持分开。这里按当前任务栏主题（`SystemUsesLightTheme`）画一份，并在 Wails 的 `SystemThemeChanged` 应用事件上重画。设计写的是“4 帧 ICO 序列”，这里用的是同一份时间线的插值帧（约每 40 ms 一帧），不使用 ICO。以上只做了源码核对与交叉编译，没有 Windows 运行证据。

## 失败模型

- **F1 模板图标 alpha**：设计里的眼睛、角标外圈、角标内符号用“背景色填充”表达。macOS 模板图只读 alpha，若按原样导出会变成实心猫。预期：导出的 alpha 中，眼睛区域与角标外圈为 0，符号为不透明。
- **F2 菜单被图标刷新牵连**：换图标若走 `SetMenu`/`Menu.Update`，会在菜单展开时关闭子菜单（t-0188 已修复的缺陷）。预期：动画与状态切换只调用图标 API，期间菜单发布次数不变。
- **F3 假状态**：任何状态必须来自 daemon 权威事实；未知时回到“同步完成”而不是猜测。daemon 不提供的状态不实现，并在文末登记缺口。
- **F4 持续高频动画**：idle 不得有持续 timer 或重绘；动画只在触发时存在，结束或退出时 timer 被清理。
- **F5 平台差异**：macOS 模板图随菜单栏深浅自动着色；Windows 按系统深浅切换两份图标并使用系统强调色与警示红；Linux 的 StatusNotifierItem 忽略模板标志，必须给出显式颜色。
- **F6 重复来源**：猫的几何与状态逻辑只有一份，各平台只在“把同一份像素交给 Wails”的最后一步不同。

## 验收契约

1. 渲染验收（`go run -tags e2e` 导出）：9 个状态 × {macOS 模板、Windows 浅/深、Linux} 全部可导出 PNG；与设计画板 SVG 的同尺寸栅格化逐像素比对，差异在抗锯齿容差内；模板图眼睛区域 alpha = 0。
2. 状态链验收（真实 Go GUI 原生状态项 + 同一真实 daemon + 独立 profile）：初始为“同步完成”；经托盘菜单关闭同步后，图标变为“已暂停”，再恢复；状态来自 daemon 的 settings，而不是 GUI 本地假设。
3. 动效验收：e2e 控制面（人工控制，明确标注）触发每种动画；记录每帧的角度与时间点，与上表关键帧一致；动画期间菜单发布次数为 0；动画结束后无 timer。
4. 菜单不回退：右键打开菜单、设备动作、退出仍按 17c15 的真实 NSMenu 跟踪路径通过。
5. 边界：只在 macOS 本机实测；Windows/Linux 做源码与编译核对，Linux 另在测试主机上验证，未验收部分写在文末。

## 结果

（实现后补写。）
