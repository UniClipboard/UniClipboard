# 0013 桌面 UI Kit 迁移到 mygo —— 执行交接

状态：实现与自动化验收已完成；Windows/Linux 原生宿主验收待补充。分支 `v2`，**不要 commit**，除非用户要求。

2026-11-09 方向确认：桌面界面（含 macOS）统一使用 mygo + 自定义 UI Kit，SwiftUI 客户端路线已取消。界面原型待定；已完成的组件迁移与现有 GUI 验证不代表最终页面设计已确认。

## 目标

把 uniclipboard 仓库 `main` 分支的 React UI Kit(`/Users/LWP1/Projects/uniclipboard/apps/gui-go/frontend/src/ui-kit`,Linux Adwaita / macOS glass / Windows Fluent 三套皮肤；早期估算约 90 个，核对 contract.ts 实际为 59 个公共组件)移植为 mygo 原生 Go 组件,放在 `app/desktop/internal/uikit`;再把桌面 GUI(`app/desktop/cmd/unic/gui.go`、`history_gui.go`)从直接用 `ui.Button` 等改为走 `uikit.Kit`。用户已确认:控件全部移植、近似还原可接受、同意升级 mygo。

## 已完成

- mygo 升级到 `v0.8.4-0.20261108082913-9233663736ca`(go.mod/go.sum 已改,全仓能编译)。
- 骨架:`kit.go`(`Kit{Skin,Dark}`、`For(dark)`、`Raw/Color/Px/Sem/Theme/Apply`;`UNIC_UI_SKIN=linux|macos|windows` 可强制皮肤)、`parse.go`、`semantic.go`、`types.go`(共享枚举)、`doc.go`(**API 约定,先读**)、`tokens_gen.go`(由 `scripts/gen_tokens.py` 从 uniclipboard `themes/default.json` 生成,勿手改)。
- 分组实现(每个文件都配 `_test.go`,3 皮肤 × 亮暗,用 `ui.NewTester`):
  - A 基础:Icon(36 个 IconName 手绘矢量)、Text、Surface/UiRoot、Button、ToggleButton、ButtonRow、RemoveButton、Switch、Checkbox、Slider、ProgressBar、Badge、Tag、KindBadge、ColorDot、InlineNotice、Disclosure — 已汇报完成。
  - B 输入:TextField、SearchField、SearchToken、TokenSearchField、ComboField、CodeField、FormField、Select、SegmentedControl、ChoiceList — 已汇报完成。
  - C 浮层:Menu/MenuEntries/ContextMenu、AlertDialog、Sheet、QuickLook、WithTooltip、Toolbar*、ActionBar、TaskPanel — 已汇报完成。
  - D 列表导航(`list*.go grid*.go table*.go sidebar*.go source*.go step*.go inset*.go`):ItemList、GroupedList*、GroupedGrid、InsetList、SourceList、SidebarNav*、Table、StepList — **已完成**。基于 mygo 虚拟 `ui.List`/`ui.Table`;状态用 `ListState`/`NavState`/`SourceState` 读取(Selection/Activated/DeleteRequested/MenuChosen/Changed/Command)。近似:Footer 是列表下固定条、Table 各皮肤共用 mygo 表头(无 macOS 斑马纹/Windows 勾选列)、Grid 列数取上一帧宽度、SidebarNav 上下键即选中、无独立焦点环、macOS 折叠按钮常显。
  - E 布局复合(`split*.go masterdetail*.go pane*.go header*.go fact*.go unavailable*.go preview*.go settings*.go transfer*.go entry*.go`):SplitView、MasterDetail、Pane、DetailHeader、EntityHeader、FactList、ContentUnavailable、PreviewFrame、Settings*、Transfer*、EntryPage — 已汇报完成(80 个测试在临时副本通过)。近似:Windows 48px 侧栏轨未画、macOS 浮动工具栏无滚动渐隐、overlay 侧栏无滑入动画、TransferStatus 弹层未与触发器右对齐、Settings 行内控件未做强调色重映射、单栏模式下隐藏 Pane 不构建(状态丢弃)。

## 2026-11-09 续做记录

- 参考源：uniclipboard `main` 提交 `9e41d15f7ecf2be4e3bd03327625fd2b4adbbbd5`。逐一提取 `contract.ts` 的 `export declare function`，59 个公共组件在 `Kit` 中均有对应方法，无遗漏。辅助组件不计入这个数量。
- 已修正 golangci-lint v2.15.0 检出的 7 处组件包问题；网格 Reveal 同时补上条目序号到可视行的转换，并增加回归测试。CodeField 保留全角数字输入并在绘制前规范化。
- `gui.go`、`history_gui.go` 已迁移到 Kit 的 Text / Button / TextField / CodeField / SegmentedControl / Surface / SettingsGroup / SettingsRow / Tag / ProgressBar / Disclosure / ContentUnavailable / AlertDialog。保留历史/设备/动态/添加设备导航、配对码、指纹批准、地址配对、历史标签筛选与使用；设备移除改用初始聚焦取消的破坏性确认对话框。
- 用户提供的原生 macOS 截图暴露了此前交互测试未覆盖的视觉错误：分段底板仅包文字、轨道尾部空白、焦点环被裁剪、TextField 高度不符、空态没有获得剩余高度。均已修复。历史筛选栏在滚动列表外；其他滚动页面在视口内部保留边距；macOS 字段恢复 22pt/5pt/6pt 几何与单层 3.5pt 焦点环；空态在剩余内容区域居中。
- 新增实际 GUI 视图测试：三皮肤 × 亮暗主题下验证导航、移除确认的回车安全默认项与 Escape、审批指纹信息、配对码全角/不完整输入、地址字段、断线恢复，以及 2× 渲染、360pt 窄窗口、分段几何和焦点环四周像素对称。
- 已导出 60 张离屏渲染截图到 `.build/uikit-review/`（不入库）。已人工查看 macOS 亮色空历史聚焦态和窄窗口、Windows 历史和 Linux 配对输入等截图。macOS 原生进程已启动并与临时 profile 的 unicd 连通；用户截图证实真实窗口可运行。修正后的六套皮肤通过离屏截图和自动化检查，尚不等于全部平台原生视觉验收。
- 新增 [0013 决策](../decisions/proposed/0013-mygo-uikit.md)，已在根 AGENTS.md（CLAUDE.md 为符号链接）登记，并补充桌面 README 的目录和截图命令。

## 验证

- `go test -race ./...`：通过。
- `go build -o .build/unic ./app/desktop/cmd/unic` 与 `go build -o .build/unicd ./app/desktop/cmd/unicd`：通过。
- `golangci-lint run ./app/desktop/internal/uikit/... ./app/desktop/cmd/unic/...`：0 issues；同范围 `fmt --diff` 无差异。
- 全仓 `golangci-lint run ./...`：仍报告本次 UI 范围外的 136 项问题（包括 API/host/Engine/Lua VM/pluginproc 等包的 errcheck、staticcheck、unused 和格式问题）。未扩大本次改动去清理这些包，不能宣称全仓 lint 已通过。

复现截图（无网络或真实剪贴板副作用；只使用测试数据）：

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/uikit-review" go test ./app/desktop/cmd/unic -run TestGUI -count=1
```

## 已知近似与后续验收

- A–E 的结构近似仍沿用上文；动画、图标和毛玻璃不要求像素一致。弹层部分宽度仍固定，CodeField 光标不闪烁。
- 已补读 Linux `root.tsx` / `pane.tsx` / `preview.tsx`，ToolbarDivider 改为空间分隔。ActionBar 与 QuickLook 的精细尺寸仍为近似。
- Windows/Linux 原生宿主下的字体、输入法、窗口材质和辅助功能尚未实测；本机六皮肤离屏截图不能替代该验收。
- 后续新增组件或调整布局时，不能只运行点击测试：同时检查聚焦、空态、窄窗口和亮暗截图。

## 约定提醒

遵循根 `CLAUDE.md`:地址/配对票据是秘密,不写进日志;配对码进来的设备永远要批准;项目未发布,不写兼容/迁移分支。
