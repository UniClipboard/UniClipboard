# 设备传输状态验收

基线为 PR #2172 合并后的 `main`，提交 `575553e2c3e5004a7ebe91821744920689afdf65`。

## 状态规则

- 每个条目和目标设备使用自己的发送快照。设备列表不再在下方重复显示发送详情。
- 有发送快照时，以快照的批次状态决定完成与失败。已接收回执、字节达到总量和重试请求成功均不能代替文件完成。
- 未确认快照显示中性状态。悬浮详情也显示未确认状态，不继续宣称正在传输。
- 总量未知或正在提交重试请求时，圆形进度不显示确定百分比。零字节文件仍由完成状态决定结果。
- 多文件详情按 `kind` 与原始 `index` 关联。文件名来自当前条目的文件列表；未提供文件名时显示项目编号。目录和内容表示不使用文件索引。
- 部分失败仍显示已完成文件。重试沿用 `useResendAction`，先核对原任务，再按既有确认流程决定是否创建新传输。
- 本机来源才提供重发操作。请求期间阻止重复操作。其他来源仍可查看详情。
- 文件预览中的逐文件进度保留。全局发送活动和无平台旧页面仍使用各自的既有组件。

## 已覆盖的失败场景

`delivery-status` 使用真实设备行、状态组件、悬浮详情和 HTTP 客户端。测试使用合成快照和脚本化 HTTP 响应，不把它视为真实跨设备传输证明。

1. 设备行重复渲染发送详情。
2. 字节达到 100% 后提前显示完成。
3. 未知总量变成零或确定百分比。
4. 未确认快照仍显示正在传输或完成。
5. 多文件详情缺项、文件名关联错误或无法滚动。
6. 键盘聚焦不能打开详情，或 Escape 不能关闭详情。
7. 部分失败隐藏已完成文件。
8. 点击失败图标重试错误设备，或请求期间重复派发。
9. 未确认就创建新传输，或取消确认后仍创建新传输。
10. 远端来源提供不可执行的重试操作。

## 复跑与工件

使用项目要求的 Node 版本。当前验证环境为 macOS、Node 24.22.2、Chromium。先安装锁定依赖，再执行统一入口：

```bash
bun install --frozen-lockfile
bun run e2e:gui layouts delivery-status file-preview
python3 apps/gui-go/frontend/e2e/tester-army/receipt.py
```

`delivery-status` 覆盖三种皮肤和明暗主题，共 12 个测试。`file-preview` 保留 PR #2172 的发送与接收回归场景，共 6 个测试。

截图与场景 JSON 位于 `target/tester-army-layout/<platform>/<suite>/`。框架报告、运行编号、断言结果与清理回执位于 `target/tester-army-layout-report/`。`target/tester-army-receipt.json` 记录源码身份与报告摘要。`target/tester-army-SHA256SUMS` 记录工件校验和。

原生产品窗口、真实双端失败重试、Windows/Linux 原生传输和移动真机不由本套件证明。它们需要对应原生或双端验收。

## 本轮验证边界

类型检查、改动范围静态检查、现有文字规格检查与 React Doctor 已通过。React Doctor 扫描包含新增文件，未报告问题。

曾启动完整默认布局矩阵。该轮在 Windows 皮肤暴露了程序化聚焦测试的时序问题；测试现已改用真实 Tab 操作，并通过三种皮肤复跑。该轮还在 `startup-first-frame/none` 构建失败，错误为 `@/ui-kit needs UC_UI_PLATFORM=windows|macos|linux (got nothing)`。随后主动中断默认矩阵并完成测试进程清理。因此，本轮不能声明完整默认布局矩阵通过。后续针对 PR #2179 的 CI 故障修复了共享设置组件直接引用平台 kit 的问题；无平台构建现已通过。

失败工件分别保留于 `target/delivery-status-failure-evidence/` 与 `target/delivery-status-focus-failure-evidence/`。最终范围以统一入口的选择回执为准。

## 平台 kit 与信息层级

业务层只把 Engine 状态转换为展示数据。`TransferStatusProps` 与 `TransferStatusItem` 是统一的展示契约。macOS、Windows、Linux 分别实现 `TransferStatus`；业务组件不直接引用通用 `HoverCard`、Fluent 或平台皮肤。

- macOS 由 Base UI 管理悬浮和键盘交互，使用 macOS kit 的材质、图标、进度条与主题作用域。
- Windows 使用 Fluent 的 Popover、Button、Spinner、系统图标与进度条。Fluent 管理浮层，键盘聚焦也能打开详情。
- Linux 由 Base UI 管理交互，使用 Adwaita kit 的面板、图标和进度条。
- 自定义状态已经承担设备行的状态展示，因此三种皮肤都不再显示左侧重复状态点。
- 浮层标题和汇总固定，文件列表独立滚动。只有活动文件显示进度条与字节数。已完成文件使用紧凑行与完成标记。失败行单独显示原因。
- GUI E2E 额外检查所选 kit、完成和失败行没有进度条、360 宽度下无横向溢出、长文件名、列表滚动，以及浮层底部未被裁切。夹具显式设置视口高度，因为 Tailwind 的源码扫描不包含 E2E 夹具。

三平台正式构建和 gallery 构建均成功，模块图未发现其他皮肤或旧页面泄漏。修复共享设置组件的构建边界后，`check:ui-kit-bundles` 整体通过，包括无平台正式构建和非法平台拒绝检查。最新结果位于 `target/pr2179-fixed-bundles.txt`。

## PR #2179 CI 构建修复

CI 作业 `124865750645` 在未设置 `UC_UI_PLATFORM` 的正式构建中失败。共享 `QuickPanelAccessibilityRow` 直接引用平台 kit，导致旧页面构建进入平台解析守卫。修复沿用现有 `@ui-setting-parts` 构建别名，为按钮提供平台与旧页面实现，不修改 CI 环境或平台守卫。

本地使用 CI 的原命令 `bun --bun run --cwd apps/gui-go/frontend build` 复跑成功。类型检查、静态检查与格式检查通过。以下相关 GUI E2E 共 10 个测试通过，覆盖三平台辅助功能设置的明暗主题，以及三平台和无平台启动首帧：

```bash
bun run e2e:gui layouts quick-panel-accessibility startup-first-frame
```

全量前端覆盖率检查未通过：241 个测试文件中 238 个通过；1561 个测试中 1557 个通过。剩余失败为语言键缺失、接收状态测试刷新，以及两个快捷面板设置测试。修复前提交 `f75c4fc73427cb576c40678cff066a80bf766892` 上已独立复现这些失败；设置测试使用明确平台运行，以绕过本次修复的构建故障。设置测试的模拟模块缺少 `getQuickPanelAccessibilityPermission`。本次未改动这些测试或接收状态逻辑。

构建与覆盖率日志分别位于 `target/pr2179-build-after.log`、`target/pr2179-coverage-after.log`。修复前对照日志位于 `target/pr2179-baseline-tests.log`、`target/pr2179-baseline-setting-tests.log`。这些结果证明构建故障已修复，不证明整个 CI 已通过。
