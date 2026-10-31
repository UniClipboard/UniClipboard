# TesterArmy GUI 测试迁移

本仓使用 `e2e@0.22.0` 和 `@e2e-dev/web@0.15.4` 运行已验证的浏览器测试。原生测试和没有等价通过结果的测试继续保留旧路径。

## 来源与依赖

- 官方源码快照：[`caf51eb9f8de765b1ecac5ce3cd511a925b2fc2f`](https://github.com/tester-army/e2e/tree/caf51eb9f8de765b1ecac5ce3cd511a925b2fc2f)。
- 上游采用 Apache-2.0，包含 `LICENSE` 和 `NOTICE`。本仓没有复制上游实现。npm 产物由 `bun.lock` 的版本和完整性值标识，不假定它与源码快照字节相同。
- Node 要求为 `^22.22.3 || >=24.8.0`。CI 使用 Node 24。
- Web 引擎使用 Playwright 1.64.1。使用它自己的安装入口，避免浏览器版本不匹配。
- 全部测试使用确定性操作。不配置模型、订阅或云浏览器。命令通过 `E2E_TELEMETRY_DISABLED=1` 关闭遥测。

## 可交付范围

`apps/gui-go/frontend/e2e/tester-army/suites.mjs` 是运行矩阵的唯一来源。`migration.json` 记录每个原入口的迁移状态。不要把候选测试接入完成等同于运行通过。

已接入 21 组原有浏览器断言。15 组至少有一个通过的皮肤，共 29 个测试组与皮肤组合。其中 12 组按原默认参数完整通过，3 组只完成部分皮肤。

| 测试组 | 已验证皮肤 |
| --- | --- |
| 设置侧边栏、文件预览、设备任务、主题调色板 | macOS、Windows、Linux |
| 设置侧边栏动效 | macOS、Linux |
| 初始化与恢复页面、组件库 | Windows、Linux；macOS 保留旧路径 |
| 跨窗口效果设置 | Windows；macOS/Linux 保留旧路径 |
| 启动首帧 | macOS、Windows、Linux、无平台构建 |
| 标签菜单、侧边栏动画 | macOS |
| 外观、旧初始化页面、移动连接、设置滚动记忆 | 无平台构建；不代表当前原生皮肤验收 |

此外，两个中继测试使用通用 SDK 定位器运行。真实 daemon 用例验证历史显示、页面置顶、API 回读、重载持久化和进程清理。它使用真实 HTTP/WS 客户端与既有模拟原生宿主，不验证原生窗口或剪贴板。

## 运行

从仓库根目录执行：

```bash
bun install --frozen-lockfile
node node_modules/@e2e-dev/web/dist/bin.js install chromium
bun run e2e:browser
bun run e2e:browser:layouts
```

默认布局命令只运行矩阵中已验证的组合，并打印保留原因。指定测试组时可以复跑全部候选组合，包括已知失败。失败不会改成跳过或成功。

```bash
bun run e2e:browser:layouts device-tasks --platform macos
bun run e2e:browser:layouts ui-kit --platform macos
node apps/gui-go/frontend/e2e/tester-army/assertion-inventory.mjs
```

多数已完整迁移的旧可执行文件成为薄入口，断言只保存在新测试中。`startup-first-frame-browser.mjs` 仍支持 `--platform` 和 `--out` 导出。`theme-palette-run.mjs` 直接委托新运行器，不再启动第二套服务。部分皮肤失败的原脚本，以及侧边栏动画的自定义宽度入口仍保留。旧 `ui-kit-run.mjs --serve` 继续用于人工预览。

仅在 macOS 上运行当前真实 daemon 用例：

```bash
bash scripts/build-cache/mbx.sh --exec cargo build --locked -p uc-daemon --bin uniclipd
mkdir -p target/gui-go
(cd apps/cli-go && go build -o ../../target/gui-go/uniclip ./cmd/uniclip)
bun run e2e:browser:daemon
python3 apps/gui-go/frontend/e2e/tester-army/receipt.py
```

## 断言与工件

- 中继测试使用框架的通用定位器。其余旧测试通过官方 `surfaceOf()` 接口取得框架管理的 Playwright 页面。它保留节点求值、元素句柄、鼠标、截图和原有断言，避免重写这些断言。
- 初始化页面的键盘流程显式选择系统窗口框架，使标题栏控件保持在内容区之外。移动连接测试等待退出中的标签页完成转换，再执行原有单面板与页脚断言。
- `surfaceOf()` 的底层操作不产生通用 SDK 的逐步操作事件。报告只证明框架实际观察到的内容。原断言结果、场景 JSON 和截图单独保存。多页面测试的额外上下文由测试夹具管理并关闭。
- `assertion-inventory.mjs` 从原基线 `7ae0a059c9e2c3cf2dd753240da9ef65f4343d40` 读取 21 个原脚本，记录断言源码、位置数量和 SHA-256。数量检查不能替代语义评审或运行结果。运行它需要本地保留该 Git 对象。
- 服务只监听 `127.0.0.1`，由框架分配端口。每个服务使用独立临时构建目录，退出时删除。测试先打开空白页，再安装路由并导航到夹具，避免夹具在拦截器安装前发出请求。
- 布局报告位于 `target/tester-army-layout-report/<皮肤>/<测试组>/`。原断言截图与场景 JSON 位于 `target/tester-army-layout/<皮肤>/<测试组>/`。
- 中继报告和真实 daemon 报告分别位于 `target/tester-army/`、`target/tester-army-daemon/`。其中包含 JSON、JUnit、Markdown 和截图。
- `receipt.py` 记录源码 HEAD、脏状态、源码和二进制 SHA-256、运行编号及报告摘要。`target/tester-army-SHA256SUMS` 校验公开工件。不要把旧轮次工件当作新 HEAD 的通过证据。

## 已确认的原路径失败

这些断言在旧运行器下也失败。保留旧路径和原断言，不通过放宽断言来完成迁移。

| 测试组 | 原路径失败 |
| --- | --- |
| macOS 初始化页面、macOS 组件库 | 模拟原生材质标记断言失败 |
| macOS/Linux 效果设置 | 流畅模式的阴影 token 断言失败 |
| 历史行悬停 | 旧 `history-favorite` DOM 查询返回空值 |
| 排版 | 合成文件传输的 `50%` 状态断言超时 |
| 历史详情回归 | `warm-image-stable` 断言发现没有图片的帧 |
| 紧凑历史布局 | 原点击被仍打开的弹层拦截 |
| 旧设置设计 | 旧“启动方式”组合框断言不匹配当前控件 |
| macOS 设计原型 | 原型背景色断言失败 |

`legacy-baseline.mjs <测试组> <皮肤>` 可复跑这些保留的合成浏览器脚本。它使用同一固定 Playwright 版本，保存旧路径退出码和日志。它不运行用户资料或真实设备场景。

修正了两个测试夹具缺陷：中继探测信封按当前 API 使用 `tag`；设置夹具安装当前页面要求的 Redux 和设备信任 Provider。设备信任 Provider 在合成夹具中禁用后台刷新。这些修正没有修改产品逻辑或放宽断言。

## 未迁移的真实后端套件

`history-kit-browser.mjs`、`devices-kit-browser.mjs`、`history-search-browser.mjs`、`tag-manager-layout-browser.mjs`、`quick-panel-unlock-browser.mjs` 和 `sidebar-toggle-app-browser.mjs` 保留原路径。它们依赖既有多条目、LAN 手机、配对、重新建组或 Rust 测试夹具。新的单条目 daemon 用例不是等价替代。

这些套件尚未完成迁移，不应标记为框架固有限制。后续须先建立等价的隔离数据与凭据生命周期，再逐条迁移断言。非 GUI 的 API、模块图、构建和发布检查保留原入口。

## 原生可行性

官方 Web 引擎只启动 Playwright 浏览器或连接 Chromium CDP。官方移动引擎只接受 iOS 和 Android。通用 Engine SPI 允许开发适配器，但当前包没有现成 WKWebView 或 WebKitGTK 驱动。

- WKWebView：使用官方非持久网站数据存储和 `isInspectable` 接口运行隔离 JavaScript 回读。不创建窗口，不激活 App。`isInspectable` 对接 Safari Web Inspector，不是 Chromium CDP。Web 引擎明确拒绝 `browser: 'webkit'` 与 CDP 连接同时使用。
- WebKitGTK：在一次性 CI 的专属 Xvfb 中使用官方临时 WebContext 回读 JavaScript。它只证明原生运行时可用。现有 Web 引擎不接收其原生 Web Inspector/WebDriver 协议。
- WebView2：使用当前固定 Wails 版本在一次性 Windows runner 上启动独立可见原生控件，设置专属临时用户数据目录和随机 loopback CDP 端口。先读取真实原生页面的 target ID，再用 TesterArmy 的持久连接契约尝试驱动同一 target。若框架创建其他页面，不计为原生通过。上下文协议不兼容与其他执行失败分别报告。正常点击需要可见控件；隐藏控件不作为可点击验收。先关闭控件并等待宿主退出，再删除临时数据目录，避免宿主的 COM 引用仍持有文件。进程退出和临时数据删除均须通过。

原生探针工作流为 `.github/workflows/tester-army-native.yml`。探针结果不等于 UniClipboard 产品的原生 GUI 验收。所有原生窗口、托盘、全局快捷键、系统剪贴板、安装包及发布验收继续使用旧路径。

参考：[Playwright WebView2 官方说明](https://playwright.dev/docs/webview2)、[Apple 的应用内容检查接口](https://developer.apple.com/documentation/safari-developer-tools/enabling-inspecting-content-in-your-apps)、[TesterArmy Web 引擎契约](https://github.com/tester-army/e2e/blob/caf51eb9f8de765b1ecac5ce3cd511a925b2fc2f/docs/reference/web.mdx)。

## 隔离边界

- macOS daemon 复用 `apps/cli-go/e2e/isolated.py`，每次创建临时 HOME 和唯一 `gui-go-*` profile。设置开发环境、`UC_GUI_GO_ISOLATED=1`、`UC_DISABLE_SYSTEM_CLIPBOARD=1`。其他平台被该用例显式拒绝，避免把 HOME 隔离误用于 Windows Credential Manager。
- CLI 停止 daemon 前验证进程身份。辅助进程再确认 PID 已退出，并记录 HOME 已删除。清理失败会使测试失败，并保留私有沙箱。
- 随机 passphrase 和会话凭据只经管道和内存传递，不进入 URL 或公开工件。开发资料中的密钥存储与 `daemon.conn` 继续遵守项目既有例外。
- 合成浏览器夹具不连接用户 daemon，不使用系统剪贴板，不发送桌面键盘事件，不激活用户 App，不停止其他线程的进程。
- 原生控件探针使用一次性 runner 或没有可见窗口的本任务进程。不得把调试端口指向用户正在使用的 WebView。

浏览器 CI 位于 `.github/workflows/tester-army-e2e.yml`，覆盖三种主机系统。工作流配置、一次本机通过、远端 CI 通过和真实设备验收是不同证据。
