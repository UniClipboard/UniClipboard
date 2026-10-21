# Go GUI 流畅模式

本文描述当前 Go/Wails 宿主，替代旧 Tauri 实施记录中的模块与验收入口。产品语义沿用 [原规格](2026-09-08-adaptive-smooth-mode-spec.md)，并按用户确认限定范围：只控制 GUI 主窗口、更新窗口和 WebView 快捷面板，不控制 macOS/Windows 独立原生快捷面板。

## 模式行为

| 选择 | 系统允许动效 | 系统要求减少动效或读取结果未知 |
| --- | --- | --- |
| 自动 | Linux 固定流畅；macOS/Windows 使用本次 GUI 启动的自动判断 | 减少动画与视觉效果 |
| 效果优先 | 完整效果 | 减少动画，保留静态视觉效果 |
| 流畅优先 | 减少动画、模糊与重阴影，使用不透明表面 | 同左 |

手动切换立即生效；页面不重新挂载，输入、选择、焦点和滚动保持。用户选择独立于当前有效结果，因此 Linux 自动仍显示选中「自动」。系统偏好变化即时覆盖，关闭系统偏好后恢复手动选择或本次自动结果。无法读取系统偏好时保守减少动效。

## 所有权与保存

本次属于结构性缺陷修复：宿主迁移后只保留了会话占位，完整策略与保存链路缺失。现在由唯一状态服务替换占位逻辑，前端只投影有效状态；没有保留并行的新旧决策路径。

- `apps/gui-go/visual_effects.go` 是进程内唯一状态来源，负责模式、窗口系统偏好汇总、采样许可和快照版本。宿主启动后初始化，避免在 Wails 事件循环就绪前调用原生调度。
- `visual_effects_storage.go` 沿用 profile/便携数据根中的 `visual-effects.json`。配置不进入 daemon、导入导出或设备同步。原子替换失败时本次仍生效，界面显示未保存。
- 各 WebView 通过 Wails 提供的调用窗口身份上报；保守汇总存活窗口的偏好，销毁后移除。快照通过 sessionId/revision 丢弃过期响应，显示或重新获得焦点时重读。
- React store 只消费宿主快照。`VisualEffectsProvider` 将有效布尔值传入纯展示上下文，皮肤不导入宿主绑定，构建期首屏可以独立渲染。
- 三套皮肤的模糊、阴影与动画共用有效状态；Windows Fluent 使用其 `MotionBehaviourProvider`。保留焦点轮廓、布局和反馈元素，不全局清除定位属性。
- macOS GUI 的原生 `NSVisualEffectView` 通过 Wails 提供的 `NativeWindow()` 句柄在 UI 线程隐藏并停用；流畅模式同时绘制不透明表面。固定 Wails beta.28 没有运行时 backdrop 设置 API，适配不涉及独立原生快捷面板。

保存格式继续为 schemaVersion=1、policyVersion=2，只有以下字段：

| 字段 | 含义与隐私范围 |
| --- | --- |
| schemaVersion、policyVersion | 配置格式与决策版本整数，不含用户内容 |
| mode | 三档模式枚举 |
| nextAuto | 下次启动自动结果枚举或 null |
| evidenceClass | capable/constrained/unknown 粗分类，不保存设备型号或原始硬件数据 |

这是原规格提出的有限明文配置范围，没有扩展持久化字段；不得据此保存其他用户内容。按 VISION.md，合入时仍需对该非敏感配置的明文例外完成逐字段审查。实现、测试和本地提交不代表合入审批。

读取上限 4 KiB；损坏、超大和未来格式只读保留，不覆盖原文件。旧策略或粗能力分类变化只清除失效自动证据，保留用户选择。写入使用同目录临时文件、刷新与替换；Windows 使用 `MoveFileEx` 的替换与写穿标志。

## 自动模式

Linux 不进行硬件探测和运行采样，自动固定流畅。

macOS/Windows 在后台执行一次能力初判，最长等待 2 秒；超时按 unknown 冻结，迟到结果不改变当前会话。物理核心数至少 4、物理内存至少 8 GiB 且满足现代硬件图形能力时判为 capable；任一已知不达标为 constrained，信息不足为 unknown。只有 capable 且没有有效历史降级时自动采用完整效果。

macOS 枚举 Metal 设备，要求所有可用设备支持 Mac2 family；Windows 使用默认 HARDWARE 适配器查询 Direct3D feature level，要求至少 12_0，旧运行时拒绝新级别时只重试旧硬件级别，不尝试软件渲染。能力支持不是 WebView 帧率保证，不使用型号白名单。[Microsoft 的 D3D11CreateDevice 说明](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-d3d11createdevice)允许不创建返回设备对象而只查询支持级别；Metal 枚举依据 [MTLCopyAllDevices](https://developer.apple.com/documentation/metal/mtlcopyalldevices())。

运行采样只在自动且实际完整效果时执行：

- 启动至少 10 秒，业务就绪、原生窗口可见且未最小化，可信交互后启动。
- 全进程一次一个许可，单次不超过 2 秒，间隔至少 30 秒，每会话最多 10 次。前端留出 IPC 时间余量；许可归属于调用窗口与快照版本。
- 隐藏、最小化、休眠、模式变化或就绪状态丢失时取消；恢复后等待至少 2 秒。没有后台轮询或逐帧 IPC。
- 至少 30 个有效帧间隔；超过 50 ms 算长帧，至少 6 个且占比达到 20% 算差样本；超过 500 ms 的停顿丢弃整段。
- 拒绝过期会话、错误窗口、重复许可、旧版本、超时与不一致数值。只保存汇总后的下一启动结果，不保存帧数据或计数。
- 同会话 3 次独立差样本后保存 `nextAuto=smooth`，本次保持完整效果；下次 GUI 启动持续采用流畅。保存失败可在后续差样本重试。
- 流畅期间不采样推断自动恢复；只有策略或粗分类变化才重新初判，用户始终可手动选择效果优先。

## 验证与边界

原生入口是 `apps/gui-go/e2e/visual_effects_run.py`，驱动真实 Wails WebView、宿主、磁盘与隔离 Rust daemon。脚本把二进制复制到唯一便携沙箱，设置唯一 `gui-go-*` profile 并禁用系统剪贴板，不修改用户 profile。

Linux 依赖 GTK3/WebKitGTK、Xvfb 与 D-Bus，先构建带测试入口的前端和宿主：

```bash
VITE_GUI_GO_E2E=1 UC_UI_PLATFORM=linux bun run --cwd apps/gui-go/frontend build
mkdir -p target/gui-go/effects-bin
(cd apps/gui-go && go build -tags gtk3,e2e -o ../../target/gui-go/effects-bin/gui-go .)
xvfb-run -a dbus-run-session -- python3 apps/gui-go/e2e/visual_effects_run.py \
  --gui target/gui-go/effects-bin/gui-go --daemon target/debug/uniclipd \
  --cli target/gui-go/uniclip --out target/gui-go/effects-evidence
```

前置 daemon 和 CLI 必须为当前版本；不要用 daemon 的 `--version` 探测版本，该程序会启动服务。宿主测试控制面只存在于 `e2e` 标签构建，生产代码无探测结果或时钟环境覆盖。

浏览器入口 `frontend/e2e/visual-effects-server.mjs` 与 `visual-effects-browser.mjs` 支持 `UC_UI_PLATFORM`、`PLAYWRIGHT_MODULE`、`PLAYWRIGHT_EXECUTABLE_PATH`、`SMOOTH_TEST_PLATFORM` 和输出目录，使用真实皮肤组件与模拟连接。模拟连接只证明界面交互，不替代原生存储或硬件证据。

本次验证记录：

| 检查 | 证据范围 |
| --- | --- |
| Linux 原生 WebKitGTK | 模式/系统偏好组合、双窗口 DOM、失效会话、全局许可、重复样本、下次启动降级、保存失败仍生效、重启恢复、坏配置保留、旧策略迁移、profile 隔离 |
| 三套皮肤的 Chromium 浏览器 | 进行中动画终态、输入保留、弹窗关闭与焦点返回、系统偏好、重载、浅深色与窄窗口、阴影 token；截图位于 `target/gui-go/effects-browser/` |
| 能力分类与降级 | 原生测试服务注入分类与采样间隔，执行真实宿主状态和保存路径；不是 macOS/Windows 硬件测量 |
| Linux 构建与 vet、Windows 交叉编译与 vet | 检查 Go 代码与相应平台分支；交叉编译不等于 Windows 真机运行 |
| 前端类型、构建、首屏依赖检查 | 三目标构建；macOS/Windows 首屏无宿主 IPC 引入 |

macOS 的 Metal/AppKit 适配、Windows 的实际 GPU 查询及原子文件替换仍需对应系统的真机运行证据；当前 Linux 环境不能将这些项目记为通过。旧 Tauri 测试和本次分类注入均不能替代这项验证。
