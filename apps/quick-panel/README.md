# GPUI 快捷面板

这是原生快捷面板，复用现有 UniClipboard 后台。macOS 与 Windows 安装包随附并默认使用它（可执行文件名为 `uniclip-quick-panel`，Windows 上为 `uniclip-quick-panel.exe`，crate 名仍为 `quick-panel`）；Linux 继续使用 WebView 面板。

## 代码结构

业务规则与界面分离，依赖只向内。详细设计与迁移记录见 `.planning/research/gpui-quick-panel/ARCHITECTURE.md`。

```text
crates/quick-panel-core/      # 无 GPUI、无 AppKit、无异步运行时，可在任意平台编译与测试
  src/query/                     # 搜索条件、chip、建议、时间范围
  src/state/                     # PanelState：方法同步修改状态，返回要执行的 Effect
  src/ports/                     # HistoryService、PasteTarget 等接口与类型化错误
  src/geometry/  actions.rs  content.rs  grid.rs  text.rs ...

apps/quick-panel/src/
  main.rs                        # 只调用 app::run
  app/                           # 装配根：运行时、触发器（快捷键与双击修饰键）、开窗
  adapters/daemon/               # HistoryService 的守护进程实现
  adapters/host.rs               # 向监督它的 GUI 发请求
  platform/                      # 唯一按目标系统选择实现的地方（macos/、win32/、fallback.rs）
  ui/                            # GPUI 视图；Panel 是薄壳：事件 → PanelState → Effect 执行器
```

新增内容时的落点：

- 新快捷键：`PanelState::on_key`（`state/keys.rs`）加分支并补测试，界面不用动。
- 新筛选维度：`query/filters.rs`，界面只渲染。
- 新平台：新建 `platform/<os>/`，实现 `platform/mod.rs` 列出的同名函数与 `CAPABILITIES`，在 `mod.rs` 加一行选择。不支持的能力在 `CAPABILITIES` 里声明为 `false`，界面据此降级（例如没有自动粘贴时回车只复制）。
- 新的后台调用：先在 `ports/history.rs` 加方法，再在 `adapters/daemon/` 实现；状态机用假实现测试。

## 当前窗口设计

按用户的最新要求，历史与预览使用两个真正独立的 GPUI 原生窗口，各有自己的绘制根和窗口边界。历史窗默认 360×420。预览内容宽 360，另有 8 宽的箭头区域；高度由实际内容排版决定，限制在 96–480（随界面缩放）。预览以当前记录的可见区域为锚点，优先右侧、空间不足时左侧；上下位置避让屏幕边缘，箭头继续指向记录。历史窗口不变宽、不移动。

两窗共享会话生命周期：在两窗之间切换焦点保持显示；离开两窗、按 Esc 或完成复制粘贴时共同隐藏。隐藏后复用原窗口。预览首次延迟 500ms，切换记录延迟 120ms。预览使用只读显示快照，所有后台操作仍由历史面板统一持有。

窗口尺寸通过 GPUI 的 `Window::resize` 更新，禁止直接修改原生窗口尺寸。后者会在当前窗口仍被更新时同步触发回调，导致系统窗口与绘制区域不同步。Windows 是例外：GPUI 的 Windows `resize` 调用 `SetWindowPos` 时不带 `SWP_NOACTIVATE`，已显示的预览窗被缩放会因此成为活动窗口并抢走键盘，所以 `platform/win32` 的 `set_frame` 自己在当前更新结束后用 `SWP_NOACTIVATE` 同时设置位置与尺寸。原生接口负责定位、显示、隐藏和无边框外壳。GPUI 的 `titlebar: None` 在 macOS 仍会创建带系统窗框的窗口，预览需要明确使用无边框 NSPanel，避免箭头后方露出矩形底板。

## 运行

从仓库根目录执行，先启动并解锁现有 UniClipboard：

```bash
cargo run -p quick-panel
```

开发配置需两端使用相同的 `UC_PROFILE`。默认快捷键为 macOS 的 Command + Control + V，连接后读取已保存的快捷键配置。原型与正式应用同时运行时，可显式指定测试快捷键：

```bash
UC_PROFILE=dev UC_GPUI_SHORTCUT=ctrl+alt+space cargo run -p quick-panel
```

`UC_GPUI_SCALE` 可设为 0.8 至 1.5。当前原型不读取宿主 WebView 的本地存储。

macOS 构建需要包含 Metal 编译工具的 Xcode。如默认版本缺少组件，可仅为本次命令设置 `DEVELOPER_DIR`，不改系统默认设置。

## 发行与默认行为

- **默认启用（macOS 与 Windows）**：GUI 启动时选择原生面板，环境变量 `UC_GPUI_QUICK_PANEL=0` 可关回 WebView 面板，`=1` 在 Linux 上显式启用。找不到助手可执行文件时自动退回 WebView 面板。
- **打包**：`node scripts/stage-daemon.mjs` 在 macOS 与 Windows 上除 `uniclipd` 外还构建并暂存快捷面板辅助程序。macOS 由 `apps/gui-go/build.sh` 放入应用包，安装后位于 `Contents/MacOS/`；Windows 由 `apps/gui-go/e2e/package_windows.py`（`--helper`）放入 NSIS 安装包和便携 zip，与 `UniClipboard.exe` 同目录，和 `uniclipd.exe` 一样先核对 `sidecar-provenance.json`、再经 Authenticode 签名（安装器的 `installer-hooks.nsh` 在覆盖和卸载前一并结束它）。
- **内容锁（daemon 侧）**：授权由 daemon 持有（`crates/uc-webserver/src/api/content_lock.rs`），只存在内存里。`可读 = 授权 && 已初始化 && 会话就绪 && 后台就绪`，每次请求现算。声明类型为 `gui`（主窗口）和 `helper`（本面板）的会话，在未授权时读历史相关路由一律得到 423 `content_locked`，读写都一样；CLI 与其他客户端语义不变。路由分类集中在一张表里，并由测试遍历 OpenAPI 全部路径，新增路由不分类就测试失败，未分类的路由按内容处理。WS 上 `clipboard` 与 `file-transfer` 主题的事件在锁定期间不发给这类连接；`content-lock` 主题的 `content_lock.changed` 通知变化。授权在 daemon 重启、授权者进程退出（GUI 崩溃）、会话丢失或恢复流程时失效；首次读取时按 `security.auto_unlock_enabled` 播种。撤销授权不会锁定加密会话，后台同步与捕获照常。助手一直运行：锁定时显示锁定页，收到锁定通知或 423 时立即丢弃内存中的条目、缩略图、预览、操作列表以及标签与设备名，正在进行的搜索结果作废；解锁通知到达后自行刷新。**边界**：daemon 信任持有其令牌的同一操作系统用户，客户端类型由调用方在 `/auth/connect` 时声明，所以这是对 GUI 界面的锁，不是对声称自己是 CLI 的本地进程的防线。
- **主窗口关闭**：WebView 随窗口销毁，应用留在托盘，助手继续运行，占用很低。
- **交付检查**：`python3 apps/quick-panel/tests/delivery_check.py <UniClipboard.app> [输出目录]` 在打包产物上用全新 profile、不设 `UC_GPUI_QUICK_PANEL` 复现首次安装，检查包内含助手、默认启动、未初始化与锁定时 daemon 拒绝 GUI 类客户端、锁定与解锁后内容是否提供、GUI 崩溃后不遗留助手、重启后未授权且 GUI 解锁按钮可授权、关闭主窗口后托盘与助手保留、`=0` 关闭。内容锁本身的验证见 `tests/content_lock_check.py`：真实 GUI、daemon 与助手，覆盖全部内容路由的读与写、WS、错误与正确口令、多客户端通知、撤销、在途请求、面板丢弃与刷新、GUI 崩溃与重启、关闭主窗口，`--baseline` 对旧包复现“GUI 已锁定仍可读历史”。需要唤醒的显示器且期间不动键鼠，结果写入 `results.txt`，不删除任何 profile 或日志。

## Windows

平台层在 `apps/quick-panel/src/platform/win32/`，只用 `windows` crate（与 GPUI 同一个）。能力声明：自动粘贴、跟随光标定位、在资源管理器中显示均可用；没有异形预览窗，预览是普通矩形窗口。

- **窗口**：GPUI `PopUp` 窗口；显示用 `SetWindowPos(HWND_TOPMOST)`，随后用 `AttachThreadInput` + `SetForegroundWindow` 取得前台（热键和修饰键双击不带输入事件，直接调用会被系统拒绝，做法与 `apps/gui-go/previous_app_windows.go` 相同）。预览窗带 `WS_EX_NOACTIVATE`，显示与缩放都不激活，所以面板保持活动窗口，焦点离开两窗才收起。
- **原生调用必须延后**：`ShowWindow`、`SetWindowPos`、`SetForegroundWindow` 会同步投递窗口消息，GPUI 在处理它们时再借用正在更新的窗口状态，表现为 `RefCell already borrowed`、丢失激活通知、搜索框拿不到键盘。`platform/win32` 一律在当前更新结束后经前台执行器执行。
- **定位与 DPI**：锚点用光标所在显示器的工作区（`rcWork`）和该显示器的缩放（`GetDpiForMonitor`），逻辑像素 = 物理像素 / 缩放，历史窗与预览窗共用这一个缩放。
- **粘贴**：用 `SendInput` 发 Ctrl+V，纯文本模式逐字符发 Unicode 事件。粘贴前先让目标窗口回到前台并等待；仍按住的 Alt、Shift、Win 会先抬起、粘贴后再按下（默认快捷键里的 Alt 就是这种情况）。目标进程以管理员身份运行而本进程不是时，系统（UIPI）会丢弃注入的输入，面板直接报“没有权限”而不是盲发。
- **快捷键**：默认 Ctrl+Alt+V，用 `RegisterHotKey`（`global-hotkey`）。被别的程序占用时助手不退出，而是记录警告并继续运行；GUI 在保存新快捷键前先用宿主的同一个注册器试注册，被占用时返回 `Conflict` 且不保存。
- **修饰键双击**：沿用 macOS 的契约与产品设置（`quick_panel.double_tap_modifier`），读取方式与主窗口的监视器相同：`GetAsyncKeyState` 轮询 `0x07..0xFE`，只有选中修饰键按下、且没有其他键按下才算一次点按。
- **助手进程**：由 Go 宿主以 `CREATE_NO_WINDOW` 启动并监督（意外退出 1 秒后重启，宿主退出时助手随之退出，不遗留进程）。`show_main_window` 与 `open_settings` 请求经标准输出 JSON 行回到宿主，与 macOS 相同。
- **输入法**：字符经 GPUI 的 `WM_CHAR`/输入法路径进入搜索框，中文输入法的组合状态按 GPUI 的规则处理。
- **防火墙**：Windows 防火墙会为每个新路径的 `uniclipd.exe` 弹出“允许访问网络”，弹窗未回答前 daemon 被挡在后面。这是 daemon 的行为，不是面板的；自动化测试因此固定使用同一路径（见下）。

### 真实桌面验收

`apps/gui-go/e2e/windows_native_panel_run.py` 在已登录的 Windows 桌面会话里跑完整链路：Go 宿主 → GPUI 助手 → 隔离的 daemon，真实键盘事件（`SendInput`）开关面板、搜索选择、粘贴到隔离的编辑器、冲突与改键、修饰键双击、助手被杀后重启、退出无残留，并保存截图与 `native-assertions.json`。一条命令的远程驱动是 `apps/gui-go/e2e/windows_native_panel_remote.py <ssh-host>`（目标机只需要 Rust 和 tar）。运行期间会占用该桌面的键盘与前台，运行时不要在同一会话里启动控制台进程（它们会抢走前台，面板按设计会因此收起）。跨编译、WSL 和假助手不能代替这个验收。

## 已接入的操作

- 后台搜索，最多 50 条；输入防抖，清空立即查询，过期请求取消。
- 单一搜索框：普通文字始终检索历史，命中的类型和标签显示为建议；确认后仅将命中的文字转换为条件，保留其余关键词。支持 `#标签`、`@设备`、`/类型` 前缀；输入日期或范围（如 `9.1-9.15`、`上周`、`3d`）时建议时间条件，按 Tab 后变成时间标签；中文界面下还识别拼音首字母（`tp` 图片、`zt` 昨天）。建议不自动应用。已移除 `type:`、`from:`、`time:`、`ext:` 前缀和扩展名筛选。
- 筛选行优先展示已选条件，按实际文字宽度尽量铺满；仅溢出时显示 `+N`。同一维度的多个值取“或”，不同维度之间取“且”；时间只有一个值，新的会替换旧的。点击选中条件取消。标签取“且”需要后台支持，目前不可用。
- Tab／Shift+Tab 进入或切换建议，上下键移动、Enter 确认、Esc 收起。未进入建议时，Enter 仍操作历史结果。Command+K 或 `+N` 展开全部类型与标签，复用同一输入框，支持连续选择。
- 上下键、Control+N/P、前十项数字快捷键；中文输入法组合状态不被列表按键接管。
- 回车／点击粘贴，Shift+ 回车（或 Shift+ 点击）纯文本，Command+ 回车粘贴并保持面板；Command+O 打开链接或文件（图片不支持，避免把解密内容写入临时文件）；Command+Backspace 清空搜索与筛选（Windows 的 Ctrl+Backspace 待阶段 3 处理，目前仍是输入框的删词）；搜索框无选中文字时 Command+C 复制并收起；空查询 Command+V 粘贴。
- Command+K（或右键）在预览卫星窗中列出操作：粘贴、纯文本、保持面板、只复制、粘贴文件路径、打开、在文件夹中显示、发送到设备、收藏、删除；上下键选择，回车执行，Esc 返回。右键菜单已移除。尚未提供：在主窗口中显示、设置（需要助手进程通知 GUI 的通道）、钉住面板。
- 列表区的状态页：首次使用（显示唤起键）、已锁定（回车解锁）、后台未连接（每 3 秒自动重连并显示次数，回车立即重连，Command+L 打开日志目录）、无结果（列出放宽条件及各自条数，上下键选择，回车应用）。已锁定时回车请求 GUI 打开主窗口，在那里输入口令解锁，面板每 3 秒静默重试。Command+Shift+O 打开主窗口，Command+, 打开设置（也在 Command+K 列表里）。这些请求由助手在标准输出写一行 JSON（`{"request":"show_main_window"}`），GUI 的监督端读取并执行；未由 GUI 启动时拒绝并提示。「在主窗口中显示」目前只是打开主窗口，不定位到该条记录。
- 图片缩略图与 3×3 图片九宫格（方向键二维移动、⌘1–9 直选、按行滚动）、文字／图片／文件预览、后台变更订阅。
- 界面语言与主窗口一致：已设置 `general.language` 时用它，未设置时跟随系统语言，均经 `crates/quick-panel-core/src/language.rs` 的 `Language::for_locale` 归一到主窗口支持的六种语言（简体中文、繁体中文、英文、日文、俄文、巴西葡萄牙文），无对应语言包时为英文。文案位于 `crates/quick-panel-core/src/text/`，每种语言一个文件，缺少字段即编译失败。语言在每次唤起读取设置后生效；唤起时重置的条件、菜单与提示随之更新，但设置返回前的首帧仍为上一语言。设置与标签、设备分开读取：内容锁定时后台拒绝标签与设备，但设置照常读取，锁定页同样使用所配置的语言。拼音首字母建议只在中文界面启用。
- 现有主题预设和自定义颜色；字体使用系统字体（正文为系统界面字体，等宽在 macOS 为 Menlo、其他平台为 Consolas），不打包字体文件。

### Windows 已知限制

- 命令键在状态机里是“Win 键或 Ctrl”（`state/keys.rs`），页脚与操作列表在 Windows 上显示 Ctrl 字样；Win 键组合同样会被面板处理，但系统自己占用的 Win 组合到不了面板；
- 多显示器与混合 DPI 的锚点计算已按显示器缩放实现，但没有在多显示器的真机上验证；
- 没有异形（带箭头）预览窗；
- Windows 防火墙对新路径 `uniclipd.exe` 的首次弹窗需要人工回答一次。

自动粘贴在 macOS 需要辅助功能权限，在 Windows 不需要权限但受 UIPI 限制；原应用已退出或焦点变化时保留错误提示，不盲发按键。原型只连接已有后台，不负责启动、初始化后台，不创建用户内容数据库或磁盘缓存。

## 图片专属预览

图片使用独立的显示组件，不再套用文字预览的顶部信息栏和底部删除提示。默认完整显示原图，窗口按图片比例与屏幕空间调整；小图不放大。透明区域显示低对比度棋盘格。鼠标悬停时淡入尺寸、大小和缩放按钮，移开后淡出。

双击在完整显示与原始像素尺寸之间切换，放大时可拖动查看，再次双击恢复。原始尺寸按一个图片像素对应一个屏幕物理像素处理（包括 Retina）。图像移动受边界约束；统一的窗口轮廓遮罩保证放大后也不会越过圆角或箭头。

图片布局与交互由 `crates/quick-panel-core/src/geometry/image_geometry.rs` 和 `src/ui/image_preview.rs` 持有；获取与缓存仍由现有面板后台逻辑持有，其他内容类型可以继续使用各自的预览样式。

生成并启动图片测试场景：

```bash
uv run --with pillow python apps/quick-panel/tests/make_images.py
UC_GPUI_IMAGE_FIXTURES=1 UC_GPUI_FIXTURE_THEME=light node apps/quick-panel/tests/fixture.mjs
```

`UC_GPUI_FIXTURE_LANGUAGE` 设置合成后台返回的界面语言（默认 `zh-CN`，空值表示未设置），`/__test/language?value=<语言>` 可在运行中切换。将 `UC_GPUI_FIXTURE_THEME` 改为 `dark` 可验证深色界面，仅影响合成后台返回的设置，不修改系统主题。再按下文的环境变量连接此测试后台。

## 验证

```bash
cargo test -p quick-panel -p quick-panel-core
cargo clippy -p quick-panel -p quick-panel-core --all-targets --no-deps -- -D warnings
cargo check -p quick-panel-core --target x86_64-unknown-linux-gnu   # 核心不含平台依赖
bun apps/quick-panel/export-theme.ts --check
```

真实后台只读检查：

```bash
UC_PROFILE=dev cargo test -p quick-panel live_daemon_search -- --ignored --nocapture
```

合成后台与原型分别在两个终端启动：

```bash
node apps/quick-panel/tests/fixture.mjs
```

```bash
UNICLIPBOARD_DAEMON_BASE_URL=http://127.0.0.1:48173 \
UNICLIPBOARD_DAEMON_TOKEN_PATH=apps/quick-panel/tests/fixture-token.txt \
UC_GPUI_SHORTCUT=ctrl+alt+space cargo run -p quick-panel
```

测试复制会改写系统剪贴板，运行中的其他剪贴板后台可能正常采集这条合成文本。`tests/reference.html` 直接使用现有 React 组件，仅用于同数据视觉对照，不参与原型运行。

实际窗口验证：

```bash
swift apps/quick-panel/tests/window_geometry.swift <原型进程号>
uv run --with pillow python apps/quick-panel/tests/check_surface.py <历史窗截图> <预览窗截图>
```

截图使用系统 `screencapture -x -o -l <窗口号>`，排除窗口阴影。此脚本适用于文字预览：检查历史窗四边，以及预览主体和箭头透明区域，能发现透明正文或多余外边距。系统窗框和阴影需要另外检查包含阴影的实机截图。

## 已取得的证据与剩余范围

2026-09-10：macOS 编译、21 项自动测试及原型自身严格检查通过。系统确认历史与预览是不同窗口；短文本预览高度 145，多行文本 233，长文本上限 480。左侧展开和连续箭头轮廓已截图确认。实测两窗间切换、共同隐藏、复用同一窗口再次唤起；合成记录成功恢复并粘贴到文本编辑。此前真实后台只读搜索返回 45 条历史。图片阶段另用合成横图、竖图、透明图和小图验证了完整显示、原始尺寸、拖动、悬停淡入淡出、明暗主题及图片／文字切换。

完整功能对齐仍以 [PARITY.md](../../.planning/research/gpui-quick-panel/PARITY.md) 为准。传输进度与取消、完整来源元数据、双击修饰键、所有语言与实时配置更新等尚未完成完整验收，不将其称为正式客户端替代品。Windows 的原生平台层见上文“Windows”一节；Linux 的原生窗口组、文件定位和自动粘贴尚未实现。

全依赖严格检查仍会遇到既有 `uc-app-paths` 文档缩进警告，因此上面的严格检查使用 `--no-deps`。验证从隔离源码目录运行，避开本机祖先目录的 Engine 路径覆盖；锁文件保留仓库固定的 Engine 提交。

主题颜色由已有前端来源导出，禁止独立维护另一套颜色值：

```bash
bun apps/quick-panel/export-theme.ts
```

## 高级搜索验证（2026-09-10）

单一输入框的文字同时用于历史检索和筛选建议；建议不会隐式更改条件。合成场景实测“工作 设计”确认 `#工作` 后保留“设计”，再叠加图片类型与收藏标签；搜索请求和显示结果一致。验证了 Tab 进入建议、回车确认、上下键滚动、连续选择和 Esc 返回，修正了组件默认 Tab 焦点切换与建议选择的冲突。

（已被取代）2026-09-10 的原型曾在客户端遍历分页结果计算标签交集。现已移除：多个标签一次请求发给后台，按并集返回，客户端不再翻页收窄。标签取“且”等待后台的 `tagMode=all`（桌面后台任务 C），在此之前不提供，也不回退到客户端扫描。

## 界面语言端到端测试

```bash
cargo build -p quick-panel
UC_GPUI_L10N_E2E_CONFIRM=1 UC_GPUI_E2E_BINARY=target/debug/uniclip-quick-panel \
  node --test apps/quick-panel/tests/localization_e2e.mjs
```

用真实 GPUI 程序和合成后台，逐一核对六种语言、无效语言标签、未设置时跟随系统、运行中切换、锁定页、日期条件和首次使用页。文字由 `tests/read_text.swift` 对面板自身窗口截图做系统 OCR 读取；非中文语言另做一遍中文识别，确认旧中文文案不再出现。

隔离方式：面板以 `UC_GPUI_TEST_CONTROL=stdin` 启动，不注册全局快捷键和修饰键双击，只由标准输入的 `toggle` 行开合，标准输入结束即退出；按键只投递给面板进程。合成后台设 `UC_GPUI_FIXTURE_NO_CLIPBOARD=1`，拒绝一切恢复请求而不写系统剪贴板，测试结束时若有任何恢复请求即判失败。`/__test/locked?on=1` 模拟内容锁定：历史、标签、设备返回 423，设置照常返回。面板仍会在屏幕上出现并获得焦点，所以未设 `UC_GPUI_L10N_E2E_CONFIRM=1` 时测试在启动任何进程前就失败。截图、OCR 文字、提交号和可执行文件哈希写入 `UC_GPUI_E2E_ARTIFACTS`（默认临时目录）下的 `manifest.json`。切换语言后首次打开的首帧仍可能是旧语言（设置返回前），测试只记录这一帧，不作断言。

## 原生客户端端到端测试

```bash
cargo build -p quick-panel
node --test apps/quick-panel/tests/e2e.mjs
```

要求 macOS 桌面已解锁、Xcode 命令行工具可用，以及 Peekaboo 已有辅助功能和屏幕录制权限。测试不静默跳过环境故障。自定义编译输出时，通过 `UC_GPUI_E2E_BINARY` 指定刚构建的可执行文件；测试报告会打印文件路径和构建时间，避免误把旧程序作为当前结果。`PEEKABOO_BIN` 可覆盖 Peekaboo 路径。

测试会自动启动真实 GPUI 程序、随机端口的合成后台及独立粘贴接收窗口。每条用例重新启动客户端，通过系统按键和实际窗口点击驱动，不直接调用界面处理函数。后台请求用于核对关键词和筛选条件，实际恢复的记录用于验证多标签并集，接收窗口中的文本摘要用于验证完整粘贴。剪贴板原内容只保存在接收窗口进程的内存中，退出时恢复；不读取真实历史或同步数据。

覆盖 13 条流程：空输入 Tab 首项选择、文字命中不自动筛选、复制搜索框选中文字、标签和类型转换后保留关键词、Esc 返回、连续选择／取消、点击取消后继续输入、等待搜索时禁止粘贴旧结果、多标签并集的单次请求与实际记录选择、`@` 设备与 `/` 类型前缀、日期识别为时间范围并被下一个替换、拼音首字母建议与替换、建议可见时普通回车粘贴历史、点击溢出后选择隐藏标签、键盘访问滚动区域，以及展开后原位置的历史条目仍可点击。

只想跑某几个用例时，设 `UC_GPUI_E2E_ONLY`：用例名包含该文字即运行（不区分大小写，多个词用 `|` 分隔），其余显示为跳过，环境准备仍只做一次，例如 `UC_GPUI_E2E_ONLY='Up and Down' node --test apps/quick-panel/tests/e2e.mjs`。

失败时自动保留请求记录和测试窗口截图；成功时清理临时文件及自建进程。`UC_GPUI_E2E_ARTIFACTS` 指定证据目录，`UC_GPUI_E2E_THEME=dark` 可在深色设置下运行同一套用例。这是客户端对合成后台的端到端验证，不代表真实后台加密存储、跨设备同步或图片字节粘贴已经验证。

2026-09-10 实测：上述 10 条流程在浅色、深色设置下各完整通过一轮（约 27 秒／25 秒）。旧可执行文件能复现“空输入 Tab 跳过首项”和“历史复制被输入框接管”两个失败，修复后的可执行文件全部通过。断言检查复制搜索文字时不恢复历史、筛选过程中不粘贴，以及最终真实粘贴到独立接收窗口；截图只作为失败诊断材料。

### 筛选行浮动展开

筛选与智能建议共用固定高度的一行。点击 `+N` 后，首行保留原位，下方以相同胶囊样式展开；最多显示三行，后两行所在区域内部竖向滚动。展开／收起使用连续高度动画，不另开卡片或增加列表排布高度。点击外部、Esc 或收起箭头可关闭。浮层拦截自身区域内的鼠标事件，避免误操作下方历史。

本轮补充三项原生端到端回归，并截图确认首行固定、三行高度上限及滚动后可见末尾标签。普通文字输入只更新同一行的建议，不再挤压历史列表。
