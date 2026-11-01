# TesterArmy GUI 测试迁移

新增或修改测试先阅读 [GUI E2E 编写规范](gui-e2e-authoring.md)。本文记录迁移状态、运行细节和已证实限制，不作为新增测试步骤的第二份来源。

本仓使用 `e2e@0.22.0` 和 `@e2e-dev/web@0.15.4` 运行已验证的浏览器测试。未覆盖的原生测试和没有等价验证结果的测试继续保留原路径。支持的运行组统一由 `bun run e2e:gui` 选择。

## 来源与依赖

- 官方源码快照：[`caf51eb9f8de765b1ecac5ce3cd511a925b2fc2f`](https://github.com/tester-army/e2e/tree/caf51eb9f8de765b1ecac5ce3cd511a925b2fc2f)。
- 上游采用 Apache-2.0，包含 `LICENSE` 和 `NOTICE`。本仓没有复制上游实现。npm 产物由 `bun.lock` 的版本和完整性值标识，不假定它与源码快照字节相同。
- Node 要求为 `^22.22.3 || >=24.8.0`。CI 固定使用 Node 26.13.0。该版本包含修复 macOS `setTypeOfService EINVAL` 的 Undici 8.x。
- Web 引擎使用 Playwright 1.64.1。使用它自己的安装入口，避免浏览器版本不匹配。
- 全部测试使用确定性操作。不配置模型、订阅或云浏览器。命令通过 `E2E_TELEMETRY_DISABLED=1` 关闭遥测。

## 可交付范围

`apps/gui-go/frontend/e2e/tester-army/catalog.mjs` 是布局与后端运行矩阵的唯一来源。`suites.mjs` 与 `backend-suites.mjs` 只读取本次选择。`migration.json` 记录每个原入口的迁移状态。不要把候选测试接入完成等同于运行通过。

已接入 21 组原有浏览器断言。17 组按原有目标范围通过，共 35 个测试组与皮肤组合。其余四组保留原断言和旧入口。

| 测试组 | 已验证皮肤 |
| --- | --- |
| 设置侧边栏、文件预览、设备任务、主题调色板 | macOS、Windows、Linux |
| 设置侧边栏动效 | macOS、Linux |
| 初始化与恢复页面、组件库、跨窗口效果设置 | macOS、Windows、Linux |
| 启动首帧 | macOS、Windows、Linux、无平台构建 |
| 标签菜单、侧边栏动画、macOS 设计原型 | macOS |
| 紧凑历史布局 | Linux |
| 外观、旧初始化页面、移动连接、设置滚动记忆 | 无平台构建；不代表当前原生皮肤验收 |

此外，两个中继测试使用通用 SDK 定位器运行。真实 daemon 用例验证历史显示、页面置顶、API 回读、重载持久化和进程清理。它使用真实 HTTP/WS 客户端与既有模拟原生宿主，不验证原生窗口或剪贴板。

## 运行

从仓库根目录执行：

```bash
bun install --frozen-lockfile
node node_modules/@e2e-dev/web/dist/bin.js install chromium
bun run e2e:gui browser
bun run e2e:gui layouts
```

默认布局命令只运行矩阵中已验证的组合，并打印保留原因。指定测试组时可以复跑全部候选组合，包括已知失败。失败不会改成跳过或成功。

```bash
bun run e2e:gui layouts device-tasks --platform macos
bun run e2e:gui layouts ui-kit --platform macos
node apps/gui-go/frontend/e2e/tester-army/assertion-inventory.mjs
```

统一入口为 `bun run e2e:gui <组>`。`list` 只列出已验证矩阵、候选范围及原生宿主要求，不启动测试。`browser`、`daemon`、`layouts` 和 `backends` 使用已有框架配置；`native-macos-history` 与 `native-windows-product` 只在对应宿主运行，不回退到浏览器。Windows 产品组要求先构建隔离 daemon 与 E2E 产品，和 CI 使用同一条命令。

已删除 17 个完成迁移的薄入口、旧包命令、两套矩阵调度器及通用兼容启动器。布局与后端矩阵共用一个子进程所有者、信号转发和选择回执实现。`catalog.mjs` 只声明矩阵，不读取进程环境；配置专用模块仍负责本次选择。无效选择先失败，不启动子进程或删除工件。取消等待已拥有子进程的清理完成。每次运行只清理选中的标准工件目录，启动前重写选择回执，防止旧成功结果混入。套件内的明暗子场景归入同一个目录。回执只索引当前选择的矩阵目录；未执行的历史目录不作为本次结果。

`--out` 仅用于一个显式布局套件，目标必须是尚不存在且不与标准工件重叠的目录。多个皮肤分别导出到对应子目录，失败或取消时不导出。

```bash
bun run e2e:gui list
bun run e2e:gui layouts startup-first-frame --platform macos --out target/first-frame-export
bun run preview:ui-kit --platform windows --port 1460
```

四组已知失败、侧边栏动画自定义宽度、外部 daemon 和未验证原生宿主入口继续保留。人工组件预览与自动验收分开；`ui-kit-preview.mjs` 只启动预览服务。

仅在 macOS 上运行当前真实 daemon 用例：

```bash
bash scripts/build-cache/mbx.sh --exec cargo build --locked -p uc-daemon --bin uniclipd
mkdir -p target/gui-go
(cd apps/cli-go && go build -o ../../target/gui-go/uniclip ./cmd/uniclip)
bun run e2e:gui daemon
python3 apps/gui-go/frontend/e2e/tester-army/receipt.py
```

## 断言与工件

- 中继测试使用框架的通用定位器。其余旧测试通过官方 `surfaceOf()` 接口取得框架管理的 Playwright 页面。它保留节点求值、元素句柄、鼠标、截图和原有断言，避免重写这些断言。
- 初始化页面的键盘流程显式选择系统窗口框架，使标题栏控件保持在内容区之外。移动连接测试等待退出中的标签页完成转换，再执行原有单面板与页脚断言。
- `surfaceOf()` 的底层操作不产生通用 SDK 的逐步操作事件。报告只证明框架实际观察到的内容。原断言结果、场景 JSON 和截图单独保存。多页面测试的额外上下文由测试夹具管理并关闭。
- `assertion-inventory.mjs` 从原基线 `7ae0a059c9e2c3cf2dd753240da9ef65f4343d40` 读取 21 个原脚本，记录断言源码、位置数量和 SHA-256。数量检查不能替代语义评审或运行结果。运行它需要本地保留该 Git 对象。
- 服务只监听 `127.0.0.1`，由框架分配端口。每个服务使用独立临时构建目录，统一运行器为每次浏览器框架调用分配一次性构建目录。服务只写入该目录的 `site` 子目录并记录 PID。框架结束后，运行器确认服务 PID 已不存在，才回收自己创建的目录；状态不明时保留目录并使清理失败。每次回收写入 `fixture-cleanup.json`，不依赖 Windows 被终止进程的退出回调。人工预览也由父进程分配和回收构建目录。服务通过 Vite 公开的 `PreviewServer.close()` 关闭服务并解除其退出回调；即使 Windows 直接终止服务，父进程仍在确认服务 PID 退出后回收目录，并输出 `preview-cleanup` 回执。测试先打开空白页，再安装路由并导航到夹具，避免夹具在拦截器安装前发出请求。
- 布局报告位于 `target/tester-army-layout-report/<皮肤>/<测试组>/`。原断言截图与场景 JSON 位于 `target/tester-army-layout/<皮肤>/<测试组>/`。
- 中继报告和真实 daemon 报告分别位于 `target/tester-army/`、`target/tester-army-daemon/`。其中包含 JSON、JUnit、Markdown 和截图。
- `receipt.py` 记录源码 HEAD、脏状态、源码和二进制 SHA-256、运行编号及报告摘要。`target/tester-army-SHA256SUMS` 校验公开工件。不要把旧轮次工件当作新 HEAD 的通过证据。

## 已确认的原路径失败

这些断言在旧运行器下也失败。保留旧路径和原断言，不通过放宽断言来完成迁移。

| 测试组 | 原路径失败 |
| --- | --- |
| 历史行悬停 | 旧 `history-favorite` DOM 查询返回空值 |
| 排版 | 合成文件传输的 `50%` 状态断言超时 |
| 历史详情回归 | `warm-image-stable` 断言发现没有图片的帧 |
| 旧设置设计 | 旧“启动方式”组合框断言不匹配当前控件 |

`legacy-baseline.mjs <测试组> <皮肤>` 可复跑这些保留的合成浏览器脚本。它使用同一固定 Playwright 版本，保存旧路径退出码和日志。它不运行用户资料或真实设备场景。

修正了两个测试夹具缺陷：中继探测信封按当前 API 使用 `tag`；设置夹具安装当前页面要求的 Redux 和设备信任 Provider。设备信任 Provider 在合成夹具中禁用后台刷新。这些修正没有修改产品逻辑或放宽断言。

## 已修复的迁移阻塞

流畅模式的 CSS 策略原本不能覆盖皮肤内联主题变量。策略中的阴影变量现在显式覆盖内联调色板；焦点环和选择轮廓不变。三种皮肤执行原阴影、跨窗口、动效、表单和重载检查。

组件画廊显式提供正常效果的展示上下文，并使用现有 Motion 媒体钩子。入口夹具通过既有宿主传输提供效果快照并初始化偏好。模拟材质仅用于原浏览器分支，不代表原生窗口验收。设计原型直接复用统一主题函数，避免 token 已迁出 CSS 后变成透明色。原背景色和材质比较值保持。

紧凑历史在 macOS Chromium 上使用 Playwright 的 `ControlOrMeta` 选择手势，避免 Control 点击打开右键菜单。窗口缩放先等待原单栏布局生效，再执行原键盘与焦点检查。底层 Playwright 页面设置有限动作等待，失败仍返回非零。

历史交互独立聚焦列表后执行原方向键断言；标签表格使用语义数据行，明暗状态读取实际计算样式，菜单关闭后等待原目标恢复焦点。原比较值和真实 API 回读保持。网格分页使用 50 条驻留上限的有限窗口断言；测试数据需超过 100 条以覆盖多页，不扩大 `history-window-policy.ts` 的容量。完整候选仍运行并保留其他失败。

## 真实后端套件迁移

`backend-suites.mjs` 声明六组原套件、一个有限窗口套件与已验证运行矩阵。当前真实后端夹具仅在 macOS 宿主运行。Windows/Linux 皮肤在独立浏览器上下文中构建和运行，不代表对应原生宿主验收。其他宿主的旧外部 daemon 或便携目录入口继续保留。

| 套件 | 已验证范围 | 保留范围 |
| --- | --- | --- |
| 标签管理布局 | macOS/Linux 皮肤；原明暗、宽度、文字换行、筛选与行数断言 | 其他宿主的原启动入口 |
| 管理设备 | 三种皮肤；全部 19 个场景，包括关闭动画、两种凭据重开和四种密码变更流程 | 未验证宿主的原启动入口 |
| 快捷面板解锁 | 三种皮肤；全部 17 条检查，包括错误密码、客户端退出和显式撤销 | 原生面板及未验证宿主的原入口 |
| 历史页面 | 三种皮肤的空历史明暗场景；Windows 皮肤的全部 28 个有数据场景 | 其他皮肤的材质、选择与导航条件；原生完整候选需单独通过 |
| 历史搜索 | 真实五条目数据的组件阶段 | 完整页面的旧详情布局与平台图标栏断言 |
| 完整应用侧边栏 | 当前模态/内联导航、状态、标签、快捷键与焦点契约 | 旧 WebDriver 入口的焦点条件 |
| 有限窗口网格 | 三种皮肤；完整真实历史顺序、50 条上限、双向遍历 | 其他历史场景与原生宿主 |

默认命令只运行已通过的 17 个套件、皮肤、数据与阶段组合，并打印保留原因。显式指定套件时运行完整候选范围，原断言失败仍返回非零退出码。

```bash
bun run e2e:gui backends
bun run e2e:gui backends devices-kit --platform macos
bun run e2e:gui backends history-kit --platform windows --seed history
bun run e2e:gui backends history-search --platform none --phase components
node apps/gui-go/frontend/e2e/tester-army/backend-assertion-inventory.mjs
```

夹具使用真实 CLI 初始化、HTTP、文件发送及手机 LAN 入站接口。历史数据包含 130 条分页记录、两条文件、一条手机图片和五条命名文本。搜索数据保留原三条手机文本、`design-notes.md`、`todo.txt` 及设备名称。手机 LAN 监听器是实际后端服务，使用随机端口和随机凭据；它只在本次隔离 daemon 的生命周期内运行。其公布地址为 loopback；此接口本身不提供 loopback-only 监听保证，不能把它描述为已经验证的仅回环绑定。

`backend.py` 是新真实后端夹具及原最小 smoke 用例的生命周期所有者。会话、文件令牌与随机 passphrase 只通过管道和内存交接。浏览器导航 URL 不含凭据。项目现有 HTTP/WS 客户端仍可能在内部请求中使用查询参数认证，因此后端用例关闭原始协议 trace，只输出去除查询参数的状态记录，并脱敏诊断中的会话与配对码。手动截图遮盖配对码和凭据 QR；不将真实凭据写入公开结果。

历史和设备断言分别由 `e2e/scenarios/history-kit.mjs` 与 `devices-kit.mjs` 持有。它们不启动浏览器、原生宿主或 daemon。原 CLI 和原生 CDP 启动入口只保留未替代的启动职责，并导入同一个断言实现；TesterArmy 提供其管理的页面与上下文。无需维护两套业务断言。`backend-assertion-inventory.mjs` 保留六组原源码身份与当前断言所有者，静态数量不作为运行通过证据。

修正的夹具问题包括：API 回读不再对有效的全局内容授权重复解锁；故障注入按路径匹配，避免认证查询参数使拦截失效；宿主夹具提供当前效果、主题、更新与窗口代次快照；表单错误沿输入框的可访问说明定位；动效检查等待实际退出动画结束。连接手机流程增加当前产品要求的同步方式选择和完成步骤。原授权、密码、QR 生命周期与结果断言保持。

固定检查按皮肤定位同一个详情操作：Windows 使用详情区按钮，macOS/Linux 使用页面命令栏。固定成功后先等待真实 POST 响应，再执行原有授权、API 回读和持久化断言。

尚未通过的候选不是框架缺少浏览器驱动。未确认的焦点与旧 DOM 查询检查仍保留失败。已定位的条件包括：

- 固定 Engine `9c1d83d9a06c7d88d325a2d8aa4d6a214fa436ce` 的隔离图片入站验证中，原始资源接口返回有效 PNG，缩略图接口仍返回 503。没有用合成缩略图替换后端结果。
- 原 WebDriver 搜索脚本与新 Playwright 候选均在旧详情几何条件处失败。旧 Windows/Linux 图标栏条件继续保留，不能以组件阶段通过替代完整页面通过。
- macOS 侧边栏在窄窗口中使用模态抽屉。当前导航实现会把焦点移入导航区。旧入口的断言要求焦点留在切换按钮。框架断言已依据现有焦点管理契约更新，没有破坏产品抽屉行为。

后端工件分别位于 `target/tester-army-backend/<皮肤>/<套件>/<数据>/<阶段>/` 和 `target/tester-army-backend-report/`。每次矩阵运行清理自己选中的公开输出目录，避免旧失败或旧成功文件混入新结果。报告包含原场景结果、截图、脱敏诊断、执行阶段和清理回执。`receipt.py` 汇总每份报告自己的 HEAD 与脏状态；旧轮次不能作为新源码通过证据。

## 原生可行性

官方 Web 引擎只启动 Playwright 浏览器或连接 Chromium CDP。官方移动引擎只接受 iOS 和 Android。通用 Engine SPI 允许开发适配器，但当前包没有现成 WKWebView 或 WebKitGTK 驱动。

- WKWebView：使用官方非持久网站数据存储和 `isInspectable` 接口运行隔离 JavaScript 回读。不创建窗口，不激活 App。`isInspectable` 对接 Safari Web Inspector，不是 Chromium CDP。Web 引擎明确拒绝 `browser: 'webkit'` 与 CDP 连接同时使用。
- WebKitGTK：在一次性 CI 的专属 Xvfb 中使用官方临时 WebContext 回读 JavaScript。它只证明原生运行时可用。现有 Web 引擎不接收其原生 Web Inspector/WebDriver 协议。
- WebView2：使用当前固定 Wails 版本在一次性 Windows runner 上启动独立可见原生控件，设置专属临时用户数据目录和随机 loopback CDP 端口。先读取真实原生页面的 target ID，再用 TesterArmy 的持久连接契约尝试驱动同一 target。若框架创建其他页面，不计为原生通过。上下文协议不兼容与其他执行失败分别报告。正常点击需要可见控件；隐藏控件不作为可点击验收。先关闭控件并等待宿主退出，再删除临时数据目录，避免宿主的 COM 引用仍持有文件。若此前没有探针错误，则进程退出和临时数据删除均须通过；已有错误时保留原错误并记录清理结果。

原生探针工作流为 `.github/workflows/tester-army-native.yml`。探针结果不等于 UniClipboard 产品的原生 GUI 验收。产品原生覆盖按各自入口的实际结果单独记录。托盘、全局快捷键、系统剪贴板、安装包及发布验收继续使用旧路径。

参考：[Playwright WebView2 官方说明](https://playwright.dev/docs/webview2)、[Apple 的应用内容检查接口](https://developer.apple.com/documentation/safari-developer-tools/enabling-inspecting-content-in-your-apps)、[TesterArmy Web 引擎契约](https://github.com/tester-army/e2e/blob/caf51eb9f8de765b1ecac5ce3cd511a925b2fc2f/docs/reference/web.mdx)。

## 隔离边界

- macOS daemon 复用 `apps/cli-go/e2e/isolated.py`，每次创建临时 HOME 和唯一 `gui-go-*` profile。设置开发环境、`UC_GUI_GO_ISOLATED=1`、`UC_DISABLE_SYSTEM_CLIPBOARD=1`。其他平台被该用例显式拒绝，避免把 HOME 隔离误用于 Windows Credential Manager。
- CLI 停止 daemon 前验证进程身份。辅助进程再确认 PID 已退出，并记录 HOME 已删除。清理失败会使测试失败，并保留私有沙箱。
- 随机 passphrase 和会话凭据只经管道和内存传递，不进入 URL 或公开工件。开发资料中的密钥存储与 `daemon.conn` 继续遵守项目既有例外。
- 合成浏览器夹具不连接用户 daemon，不使用系统剪贴板，不发送桌面键盘事件，不激活用户 App，不停止其他线程的进程。
- 原生控件探针使用一次性 runner 或没有可见窗口的本任务进程。不得把调试端口指向用户正在使用的 WebView。

浏览器 CI 位于 `.github/workflows/tester-army-e2e.yml`，覆盖三种主机系统；真实后端测试单独在 macOS 运行。PR 由 `pr-check.yml` 统一调用两个 TesterArmy 可复用工作流，路径条件集中在 `.github/ci-paths.yml`，分别控制浏览器、真实后端、各平台能力检查和 Windows 产品测试。两个工作流保留手动全量入口。工作流配置、一次本机通过、远端 CI 通过和真实设备验收是不同证据。


## 后端启动诊断与清理回执

隔离辅助进程只输出有限诊断字段：阶段、操作类别、退出码、异常类型和系统错误码。它不保存命令参数、异常消息、HTTP 错误正文或原始标准错误。父夹具再次验证字段白名单，最多保留 200 条事件。`isolation.json` 保存独立的清理结果。清理失败时保留私有沙箱，并将 `daemonExited` 标为未知；不得推断清理成功。旧 CI 的就绪前退出仍没有已确认根因。

## 当前侧栏契约

`sidebar-toggle-app` 的框架断言依据 `use-split-navigation.ts` 验证当前模态导航。在 900 像素下，打开抽屉必须将焦点移入导航，并使背景内容不可交互；Escape 必须关闭抽屉并恢复按钮焦点。在 1200 像素下，侧栏保持内联行为。按钮数量、展开状态、标签和快捷键检查保留。旧 WebDriver 入口继续保留其原断言，不能将本次契约更新描述为旧断言全部通过。

## Windows 产品原生迁移入口

```bash
python apps/gui-go/e2e/build_windows.py --mode e2e --part go
cargo build --locked -p uc-daemon --bin uniclipd
node apps/gui-go/e2e/windows_startup_ready.mjs --tester-army
```

此入口运行真实产品宿主、Windows 皮肤和真实 daemon。它将原启动与初始化断言提取为共同入口，并运行原空历史明暗场景。TesterArmy 必须驱动产品原有 WebView2 target。独立浏览器页面不能计为通过。夹具使用便携沙箱、专属 profile、禁用系统剪贴板和原生面板。框架关闭协议跟踪，避免保存初始化密码。公开工件只包含源码与二进制身份、断言、截图和清理回执，不包含连接文件、私有存储或原始日志。

工作流 `tester-army-native.yml` 的 `windows-product` 作业执行这条链路。新增入口只有在实际 Windows 作业通过且清理回执成立时才计为已迁移。原入口不带该参数时继续运行原 CDP 路径。托盘、面板、系统快捷键、系统剪贴板、安装包及有数据历史仍使用各自原生入口。


排版候选已将过时的可见 `50%` 标签改为当前 `1.00 KB / 2.00 KB` 字节标签，并额外验证实际进度填充仍为 `50%`。文本规格、菜单和代码行对齐检查保留。原内容与进度区域不重叠断言在 360 像素下仍失败。此失败保留，未放入通过矩阵。未为了通过而改变列表行布局。


## 已复现的初始化参数失败

CLI 参数解析器明确拒绝以短横线开头的选项值，连写 `--passphrase=...` 也会被拒绝。旧夹具使用 `token_urlsafe(24)`，因此随机口令可能以短横线开头。真实 CLI 对受控口令稳定返回退出码 2。夹具改用 `token_hex(24)`，仍提供 192 位随机熵，并满足现有 CLI 契约。没有修改 CLI 解析器。此复现能证明一种启动失败原因，不能追溯证明旧 CI 的两个失败都属于此原因。

## 有限窗口网格验收

主分支 PR #2150 明确 50 条驻留上限及 PageUp/PageDown 导航。`history-bounded-grid.mjs` 是共享断言入口。它读取真实搜索 API 的完整顺序，在大视口中逐个验证重叠窗口、50 条上限、全部记录可达及反向返回。旧“无输入自动驻留超过 100 条”的条件由当前契约替换，并保留原执行证据。独立 `history-window-grid` 候选复用相同断言；它不能替代其他历史场景。

Windows 启动候选已根据实际原生失败工件，将旧设置链接定位更新为已翻译的当前导航按钮。退出使用现有控制文件通道，并要求 `control-exit` 确认。外层清理预算为 45 秒，高于宿主 UI 和 daemon 的三段 10 秒停止预算。强制终止仍不能计为清理成功。


## Windows 产品原生的有数据候选

已验证的最小原生链路先完成启动、真实 IPC、初始化与两项空历史检查。后续候选在同一原生 target 和便携 profile 内，使用实际 Windows CLI、HTTP 与手机 LAN 入站生成原 138 条数据，再执行完整 28 个历史场景。它复用原断言所有者和标准合成文本/PNG，不使用浏览器宿主夹具。空历史与有数据报告分别写入 `target/tester-army-windows-product/empty/` 和 `history/`。测试前后都校验原生 target 标识。CLI 输出与连接凭据只在内存或既有私有连接文件中使用，不复制到公开报告。

macOS 与 Linux 的列表组件明确以强调色填充表示多选，不显示复选框。当前断言保留三项范围选择、两项切换选择、全选与 Escape 检查，并验证两条选中行的填充不同于未选中行。Windows 的原复选标记检查保持。历史报告保存前统一脱敏已知凭据与传输令牌，断言输入不变。

两个生成标准数据的 Node 子进程分别设置 60 秒预算。超时继续进入辅助进程的原错误和清理路径。`.github/ci-paths.yml` 的 Windows 产品条件覆盖构建脚本、CLI、共享 Go 宿主、嵌套 Go 包、daemon 及其本地依赖、工具链与构建缓存输入，避免产品源码变更跳过原生检查。


夹具现在直接持有一个 `standalone` daemon 子进程，再执行真实 CLI 初始化与 `start`，并校验连接文件 PID 未变、健康回执仍为 `standalone`。并发回收线程负责回收已退出子进程，避免 CLI 等待僵尸 PID 消失。正常清理必须有退出码 0，且没有夹具强制终止。通用 CLI 的临时进程提升路径仍保留；此 fixture 变更不宣称修复该路径的竞态。


## macOS 最小原生历史适配器

`e2e.macos-history.config.ts` 使用本仓的 `uniclipboard-macos-accessibility` 引擎。它通过公共 `e2e/engine` 接口与 `agent-desktop` 的 macOS 可访问性驱动操作真实产品 WKWebView。它不启动浏览器，不使用 CDP，不运行旧的内部历史断言驱动。

运行前安装项目锁定依赖，并准备 Node 24 或更新版本、Bun、Go、Rust、Xcode 命令行工具和 `agent-desktop`。当前验证的原生驱动版本为 1.0.2。宿主必须具备可访问性与屏幕录制权限。

```bash
agent-desktop status
bun run e2e:gui native-macos-history
```

该命令先构建独立 E2E 应用，记录构建 HEAD、脏状态和宿主/UI/daemon/CLI 哈希，再执行原生 SDK 测试。夹具在私有 HOME 和唯一 profile 中启动真实 daemon，通过真实入站接口写入两条合成历史。产品以 `external` 模式启动，避免内部 E2E 驱动竞争操作。

适配器核对宿主、UI 子进程关系、进程启动身份和原生窗口 ID。每次读取、点击与截图都限定该窗口。可访问性缓存使用私有 `AGENT_DESKTOP_HOME`，不共享其他会话的节点引用。它只声明语义点击、观察、定位与截图能力。节点失效不会变成坐标点击；无法确认动作未投递的失败不会自动重放。

测试定位实际 List 控件，验证切换后的选中状态，再点击第一条合成历史。SDK 同时断言该行选中、相同文本出现在详情区，并保存真实窗口 PNG。截图遇到安全输入控件时拒绝执行。退出通过现有产品控制通道完成。产品确认退出、宿主与 UI 退出、daemon 退出及私有目录删除均须成立，强制清理不计为通过。

工件位于 `target/tester-army-macos-history/`。每次测试包含 SDK 报告与轨迹、`native-driver.json`、`native-launch.json`、`native-events.json`、窗口 PNG 和 `native-isolation.json`。原始 daemon 连接文件、会话令牌、口令和私有存储不进入工件。SDK 自带的脏状态字段不能单独证明源码身份；以独立 Git 回执、源码哈希和构建回执共同核对。

普通 macOS CI 只执行此入口的收集检查，不启动桌面，不计为原生 GUI 验收。真实执行需要已具备驱动和权限的隔离 macOS 宿主。该最小链路不等于完整原生历史迁移；原全量入口及其失败断言继续保留。
