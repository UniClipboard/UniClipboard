# 桌面应用

先读 [README](README.md)。本目录是 `engine` 的桌面宿主，Go 代码里的约束：

- `internal/` 下的包只供桌面端使用；核心（根上的 `engine`、`internal/`）不能 import 本目录，`go test ./engine` 会检查。
- 命令：`go build -o unic ./app/service/cmd/unic && go build -o unicd ./app/service/cmd/unicd`；第二个本地节点 `UNIC_HOME=/tmp/unicA ./unicd`。
- HTTP `/v1` 的响应形状是对外约定（CLI、GUI、`curl --unix-socket`），改动须同步客户端与契约；按根目录约定，发布前不保留兼容分支。普通状态不含票据，审批强制携带指纹与修订，见[身份与信任](../../docs/architecture/identity-and-trust.md)。
- 事件和日志不得携带剪贴板内容；`Info` 只写 `text` / `image` / `N files` 这类种类。
- 剪贴板适配（`../../internal/clipboardintegration/clipboard.go`）遵循契约第 8 节的观察/应用协议，改动前先读它。

- Build the graphical program with `go build -o dist/unic-desktop ./app/desktop/cmd/unic-desktop`. The service and CLI belong to `app/service/`.

- Quick-panel system interaction follows the [Desktop contract](../../docs/contracts/desktop-quick-panel-contract.md). Keep OS target identity and synthetic input in the Desktop host; service and Engine must not own them.

- Use `make desktop-e2e` as the canonical Desktop acceptance entry. Add declarative cases and regenerate the [catalogue](e2e/CASES.md); follow the [evidence contract](../../docs/contracts/desktop-e2e-contract.md). Native-window recordings do not establish OS focus or cross-application paste.

- Follow the [Desktop E2E runtime rules](e2e/README.md#runtime-budget) when adding or changing cases or CI shard assignments. Measure affected cases in hosted PR CI and review any case that reaches the review threshold.

## UI 稳定性约定（适用于全部桌面页面：历史、设备管理、设置、插件及弹窗）

- **局部变更不得引起整页刷新或无关组件闪灰**：用户触发单次局部写操作（如切换设备同步策略/路径开关、切换设置项、收藏/打标签/复制等）时，不得让同页无关按钮、开关或工具栏出现视觉重绘/闪灰。
- **分离防重入与视觉忙碌态**：写操作并发互斥必须在动作入口立即检查 `l.busy`（`mutate` 内部已拦截），但**严禁**将控件的视觉 `Disabled`、`Opacity` 或「正在处理…」瞬态提示直接绑定到瞬时 `l.busy`；视觉禁用与忙碌提示必须经过延迟阈值（`l.showBusy()`、`showForegroundLoading()`，`120ms`）再生效，避免本地毫秒级 RPC 引起 1 帧整页闪灰或状态胶囊抖宽。
- **写操作结果就地回写，禁止丢弃返回值等待整页重拉**：本地写接口（如 `SetBindingSync`、`UpdateChannel`、`RenamePeer`、`SaveConfig` 等）返回最新状态时，必须在 `mutate` 清除 `l.busy` 前就地回写到本地视图状态（开关与策略类切换应支持即时更新与失败回滚），再由后台刷新对账；严禁丢弃写接口返回值而依赖全量 `loadHistory`/`Status` 刷新，避免出现「忙碌态已结束但界面仍显示旧值」的空窗帧或 Switch 滑块来回弹跳。
- **禁止在 `busy` / `loading` 期间卸载控件**：工具栏按钮、标签移除按钮、卡片快捷操作等在忙碌期间必须保持占位，仅通过 `Disabled` 或回调拦截禁交互，严禁用 `if !busy` 跳过渲染，避免即时模式 Flex/居中布局横向跳动。
- **内容缓存按内容变化失效，不按元数据 `Revision` 失效**：仅当条目的 `Kind`、`TotalBytes`、`Parts` 变化或旧缓存处于错误态时才调用 `dropDetail` 清理 `l.details`；收藏、标签、同步状态等元数据变更不得清空已加载预览或重复拉取 `/content`。
- **回归验证**：在任何桌面页面新增或修改交互写操作、开关、策略切换或工具栏控件时，需在单元测试中用 `ui.Tester` 逐帧校验：快路径（`< 120ms`）无关控件像素与 `Disabled` 零闪烁、控件 `Bounds` 零位移、写完成后无旧值空窗帧、并发点击仍被 `l.busy` 拦截，以及元数据变更不触发多余内容请求（参考 `internal/desktopui/copy_stability_test.go` 与 `internal/desktopui/devices_stability_test.go`）。
