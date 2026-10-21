# 调查记录

## 已核实

- apps/gui-go/host_commands_native.go 的 visualEffects 是进程内状态，启动固定 auto；不读写已有 visual-effects.json。
- snapshot 固定 autoForSession=effects；smooth 仅开启 lowEffects，reduceMotion 只跟随系统 reduce。
- BeginVisualEffectsSample 恒返回 nil，ReportVisualEffectsSample 丢弃采样。
- 前端仍有统一 store、Provider、效果 CSS 和有界采样器；不能据此认为 Go 宿主已经实现完整功能。
- profile=a 的旧配置 mode=auto，policyVersion=2，nextAuto=null，evidenceClass=unknown。当前读取到的 GUI 进程未设置 UC_PROFILE=a，不能据磁盘配置声称已验证 a 的运行态。
- VISION.md 明确 Linux 自动流畅，macOS/Windows 按能力与运行表现自动选择。
- 旧规格 docs/specs/2026-09-08-adaptive-smooth-mode-spec.md 包含完整产品语义，但模块、原生探测与测试路径有 Tauri 历史，不能直接照搬。
- 旧规格明确持久化明文例外须在 PR 逐字段论证并获批；实现与测试不以此为前置许可。
- 新发现：macOS/Windows 默认是独立 GPUI 快捷面板，已有 helper 监督器没有视觉状态下发能力；用户已明确将其排除在本开关范围之外，不需要扩展该通信能力。
- 新发现：macOS 主窗口原生材质在创建时固定，单独去掉 CSS 模糊不能证明已关闭原生效果。
- 固定 Wails beta.28 的本地源码确认 application.WindowKey 随调用 context 注入窗口身份；可用于系统偏好报告与采样许可归属。
- 已完成 implementation-brief.md，记录行为表、缺口、所有权、配置迁移、自动评估、效果覆盖及验收标准。

## 实施与验证

- 已替换进程内占位策略，实现配置迁移、原子保存、窗口身份汇总、采样许可与下次启动降级。
- 原生快捷面板代码与监督器未修改。
- macOS GUI 材质通过 Wails NativeWindow 适配；固定版本无运行时 backdrop setter。宿主 package 全局使用 ARC，新 Metal 代码须使用 ARC 所有权与 bridge 转换。
- Wails InvokeAsync 在 UI 线程可以同步执行，回调不能重入状态锁；原生材质采用 UI 线程独占 revision 校验。
- UiRoot 也参与构建期首屏，不能直接导入实时 store；使用纯展示 Context 消费 Provider 的两个有效布尔值，保持首屏依赖隔离。
- Linux 真实 Wails/WebKitGTK 与隔离 daemon 的七阶段验收已通过一轮，新增实际写入失败和第二 profile 隔离。后续采样边界修改正在复核。
- 三套皮肤的浏览器检查、截图与 macOS/Windows 首屏依赖检查通过；Windows 交叉编译通过。
- 全量前端 232 个文件、1503 项通过一轮；新增生命周期取消路径后重新执行最终检查。
- React Doctor 全文件差异扫描仍报告 e2e-driver 中既有的四处 any；正在对新增/实际功能改动文件单独扫描。
