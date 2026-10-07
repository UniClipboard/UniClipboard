# Go GUI 托盘图标：实心猫（B solid cat）状态与耳朵动效

状态：已实现；失败模型与验收契约先于实现落盘，证据与边界见文末「结果」节。

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
- 宿主新增了一条只读的 daemon WebSocket 连接（`iconFeed.follow`），订阅 `file-transfer`、`clipboard`、`peers`、`device-trust`、`content-lock`、`paired-devices`，断线后每 3 秒重连。没有可复用的现有事件连接：Go 宿主进程里除它之外没有任何 WebSocket 消费者（`DialWS` 只有这一处调用），WebView 的事件连接在前端 JS 里，静默启动时甚至不存在，也无法跨进程共享。daemon 没有不持租约的事件通道，每条已认证的控制 WebSocket 在 `crates/uc-webserver/src/api/ws.rs` 里获得一个控制租约；加一个不持租约的通道需要改 daemon 协议，不在本任务范围。
  对现有契约的实际影响（逐条读源码核对，不是“未见消费者”）：租约的消费者只有 `apps/daemon/src/daemon/oneshot.rs` 的 Oneshot 自终止监督器（`host.rs` 只在 `DaemonResidency::Oneshot` 时启动它）、受控重启的排空，以及 WebSocket 接入门禁 `ensure_not_quiescing`（只在受控重启排空期间拒绝新连接）。受控重启 `POST /lifecycle/restart` 在非 Oneshot daemon 上一律返回 `NotPromotable`，所以 `quiescing` 永远不会被置位。GUI 拉起或复用的 daemon 都是常驻的：`apps/gui-go/main.go` 的 `bootstrap` 冷启动时用 `SpawnDetachedDaemon("gui")` 起常驻 daemon，遇到已存在的 Oneshot daemon 则直接 `log.Fatal` 拒绝复用。因此 GUI 运行期间这条连接不会让任何 daemon 失去自终止或排空的机会；Oneshot 只由 `apps/cli-go` 的 `uniclip` 命令拉起，并通过 `/lifecycle/restart` 升级为常驻，那条路径上没有 GUI。轻量模式与退出：GUI 进程退出时连接随进程关闭，常驻 daemon 不受影响（轻量模式保留 daemon，完整退出则停止它）。这个结论依赖“GUI 不附着 Oneshot daemon”这条现有启动约束；若以后放开它，托盘连接必须在 `/health` 报告 `oneshot` 时不建立（只靠 10 秒的 HTTP 快照），或在排空开始时让出租约，那属于 L8d 的工作。
- 刚启动、连接尚未建立的几秒里，已配对设备都未连接，图标会短暂显示“离线”；这是 daemon 当时报告的事实，没有额外抑制。
- Linux 的图标颜色按桌面配色方案选择（`org.gnome.desktop.interface color-scheme`，进程内只读一次），不能知道状态栏自己的底色，配色方案之后变化需要重启才生效。
- Windows 不使用 Wails 的“亮/暗两份图标”（`SetIcon` + `SetDarkModeIcon`）：读源码可知，运行期调用时只要两个模式共用一个句柄，后设置的图标就会同时替换两个模式，两份图标无法保持分开。这里按当前任务栏主题（`SystemUsesLightTheme`）画一份，并在 Wails 的 `SystemThemeChanged` 应用事件上重画。设计写的是“4 帧 ICO 序列”，这里用的是同一份时间线的插值帧（约每 40 ms 一帧），不使用 ICO。以上只做了源码核对与交叉编译，没有 Windows 运行证据。图标尺寸取系统小图标尺寸（`SM_CXSMICON`，系统 DPI），不随任务栏所在显示器的 DPI 变化重画，这是已知边界。

## 状态与事实来源

图标显示的基础状态按下表优先级取第一个成立的（这个顺序是我自定的规则，设计没有给出，未经产品批准）；“新内容”圆点叠加在任意状态上。

| 状态 | 事实 | 来源（daemon） | 清除条件 |
| --- | --- | --- | --- |
| 需要处理 | 有待用户决定的设备信任变更或待接纳的设备；或投递读到 `failed` | `GET /member/device-group-choices`（`deviceTrust.currentChange`、`inboundPairings` 状态 `awaiting_confirmation` / `needs_attention`），事件 `device-trust.changed`；`clipboard.delivery_status_changed` 后读 `GET /clipboard/entries/{id}/delivery` | 待决定项消失；发送失败在用户打开或聚焦窗口后 |
| 已锁定 | 内容锁已开且空间已建立 | `GET /content-lock`（`unlocked`）与 `GET /v2/setup/state`（`hasCompleted`），事件 `content_lock.changed` | 解锁 |
| 已暂停 | 同步开关关闭 | `GET /settings`（`sync.syncEnabled`） | 重新开启 |
| 离线 | 已配对设备都不可达 | `GET /paired-devices`（`connected`），事件 `paired-devices.*`、`peers.*` | 任一设备可达或没有配对设备 |
| 暂不记录 | 无来源（只有 e2e 能强制） | — | — |
| 传输中 | 有传输持续超过 1 秒 | 事件 `file-transfer.progress` / `.status_changed`；15 秒无进展视为结束；已结束的传输 id 记 30 秒，其后到达的进度不算 | 全部传输结束 |
| 仅局域网 | 关闭了中继回退 | `GET /settings`（`network.allowRelayFallback` 为 false） | 重新开启 |
| 同步完成 | 以上都不成立 | — | — |
| 新内容圆点 | 收到来源为 remote 的新内容，且主窗口当时没有聚焦 | 事件 `clipboard.new_content` | 用户打开主窗口或快捷面板 |

快照每 10 秒读一次，相关事件到达时立即读；读失败时保留原值，不把“读不到”当成证据。窗口已聚焦时不点亮圆点、不提升发送失败，因为用户已经在看。

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

证据目录在任务库（`library/run4/`，含 `provenance.json`：Engine 固定版本 `e86f94ce…`、`cargo build --locked -p uc-daemon` 产出的 debug `uniclipd`、Go 工具链版本与产物哈希）。运行器为 `apps/gui-go/e2e/tray_icon_run.py`。

已通过（宿主层，同一真实 daemon + 独立 profile）：

- 渲染与设计对齐：9 个状态与独立栅格化的设计画板逐像素比对，30% 容差下差异不超过 21 / 1936 像素；模板图眼睛与角标外圈 alpha 为 0。
- 初始状态为“同步完成”；人工强制的 9 个状态（标注为 MANUAL）逐一保持；这些是 e2e 人工夹具，不代表对应状态在产品里会被 daemon 事实触发（只有“已暂停”“仅局域网”经 daemon 驱动链验证）。
- 4 种动画的角度时间线与关键帧一致（角度误差不超过 5°，帧间隔不超过 100 ms，结束回到静止）；动画期间菜单发布次数为 0；结束后无 timer；idle CPU 安静。
- 由 daemon 驱动的状态链（不点击）：从外部改 `sync.syncEnabled` 后图标变为“已暂停”、改 `allowRelayFallback` 后变为“仅局域网”，恢复后回到“同步完成”。

未通过 / 受阻（原生层）：测试 Mac 在每次运行时都处于锁屏、显示器休眠状态，状态项截图是模糊的壁纸，右键菜单无法点击。运行器的 `preflight` 将这些检查标为 blocked，没有绕过。受阻项：原生状态项截图（全部状态与动画）、真实右键菜单中的同步开关与退出、macOS 浅色 / 深色菜单栏的原生截图。需要一个未锁屏、显示器常亮、且不与其他任务的 GUI 验收并行的时段重跑。

未验证的平台：Windows 只做了交叉编译与源码核对；Linux 没有运行证据（StatusNotifierItem 像素图路径未实测）。设计画板的浏览器原貌截图也没有取到（无登录会话），对照对象是用 resvg 从画板 SVG 独立栅格化的图。

与设计的差异：

- 状态之间的优先级（需要处理 > 已锁定 > 已暂停 > 离线 > 暂不记录 > 传输中 > 仅局域网 > 同步完成）是我自定的规则，设计没有给出，也没有经过产品批准，待产品确认；改动它只需改 `iconFacts.base`。
- “需要处理”直到 daemon 的待决定项消失才清除，而不是“到用户查看为止”；发送失败则在用户打开窗口后清除。
- 动画帧是同一时间线的插值帧（约每 40 ms），不是 Windows 的 4 帧 ICO。
- “需要处理”动画：关键帧条只画出两个峰值，CSS 演示为 280 ms 周期；这里沿用 280 ms 周期并在 700 ms 收尾。
- macOS 图标为 44 px 图像、36 px 艺术框（18 pt 居中于 22 pt 方块）。
- “暂不记录”与“内容被拦截”没有 daemon 来源（见上文已知缺口）。
