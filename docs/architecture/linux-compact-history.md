# Linux 窄窗口历史记录导航

## 窗口与布局

Linux 主窗口最小宽度为 640 个逻辑像素，最小高度仍为 600；macOS 与 Windows 的最小尺寸保持原值。不要按窗口宽度缩放字体或整页内容。

历史记录使用以下布局：

| 内容视口宽度 | 侧栏 | 列表与详情 |
| --- | --- | --- |
| ≥ 1100 | 默认并排，可手动隐藏 | 并排显示 |
| 761–1099 | 默认关闭，按钮打开覆盖抽屉 | 并排显示 |
| ≤ 760 | 默认关闭，按钮打开覆盖抽屉 | 单栏切换 |

侧栏的抽屉状态由 `LibraryMainLayout` 统一决定，通过 `SplitView.sidebarOverlay` 传入皮肤；皮肤不另设侧栏宽度断点。单栏详情由 Linux 皮肤的 `adaptive-layout.ts` 判断，样式读取状态属性，不重复判断该断点。

窄窗口工具栏分成两行，搜索框独占第二行，避免压缩搜索输入和快捷键提示。

## 交互

- 窄窗口初始显示列表。普通点击条目或在列表按 Enter 打开详情；方向键只移动选中项，Ctrl/Shift 多选保留在列表。
- 详情替换列表，提供“返回列表”按钮；未被菜单或弹窗处理的 Escape 返回列表。打开时焦点进入返回按钮，返回时回到浏览列表。
- 列表保持挂载、保留布局尺寸，同时设置不可见、`inert` 和 `aria-hidden`，保留虚拟列表滚动位置，避免隐藏内容进入键盘导航和辅助技术。
- 打开侧栏不改变内容列宽。抽屉打开时内容区域不可交互，Tab 留在抽屉内；点击遮罩或按 Escape 关闭，焦点回到侧栏按钮。
- 选择侧栏分类、标签或设备后关闭抽屉，并回到对应列表。
- 网格延续已有激活入口打开详情；单栏时使用相同返回入口。宽窗口保持列表与详情并排。
- 调整窗口宽度不会清空选中项；已显式打开的详情在回到窄窗口时继续显示。
- 从详情进入顶部搜索框会回到结果列表，输入框保留焦点。

业务页面持有详情打开状态，通过 `SplitView.detailNavigation` 提供给皮肤。`GroupedList.onInspect` 表达查看详情，原有 `onActivate` 的宽窗口复制语义保留。其他皮肤保持现有显示行为。

## 验证

浏览器检查使用真实组件与合成内容，模拟资源接口，不读取真实用户资料：

```bash
cd apps/gui-go/frontend
UC_UI_PLATFORM=linux \
  UI_FIXTURE_ENTRY=e2e/fixtures/history-compact.tsx \
  UI_FIXTURE_PORT=1474 \
  node e2e/visual-effects-server.mjs
```

另开终端执行：

```bash
node apps/gui-go/frontend/e2e/history-compact-browser.mjs target/e2e/history-compact
```

脚本支持 `PLAYWRIGHT_MODULE`、`PLAYWRIGHT_EXECUTABLE_PATH` 和 `HISTORY_COMPACT_URL`；覆盖浅深色、640–1280 像素宽度、普通选择与多选、键盘导航、返回焦点、虚拟列表滚动保持、抽屉关闭以及窗口跨断点变化。截图和结果写入指定目录。这不替代原生宿主连接真实隔离后台的完整验收。
