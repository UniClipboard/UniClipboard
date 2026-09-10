# GPUI 快捷面板验证

日期：2026-09-10
状态：实施中

进展：代码与自动检查已实现；实机处于锁屏，交互验收待解锁。

## 范围与完成标准

在 `apps/gpui-quick-panel` 实现独立、非发布的 macOS 快捷面板。验证全局快捷键唤起、中文搜索输入、真实后台搜索、上下键选择、恢复剪贴板并粘贴到原应用、Esc 关闭、失败可重试。不是完整桌面客户端，不替换正式快捷面板。

## 方案

- 固定 GPUI 0.2.2 与兼容的 gpui-component 0.5.1；使用其 Input、Lucide 图标与主题，避免自行实现输入法。
- GPUI 的 WindowKind::PopUp 在 macOS 已使用 NSPanel 和 NonactivatingPanel，直接使用原生能力，不修改 Objective-C 窗口类。
- 复用 uc-daemon-client 的发现、认证、搜索和恢复接口。后台请求在 Tokio 运行，通过异步结果回到界面；查询取消和版本检查避免旧结果覆盖新输入。
- 自动粘贴只在辅助功能权限可用、原应用仍存在且焦点确认后执行。失败保留可见说明，不向其他应用盲发按键。
- 独立快捷键避免占用正式面板的快捷键；本原型关闭窗口后仍等待下一次唤起。
- 不加载数据库、不直接链接 Engine、不持久化用户内容。日志不包含搜索词、历史内容或认证信息。

## 备选与限制

完整客户端超出本次范围。Tauri 的快捷面板包含框架相关代码，不能直接依赖；GPUI 原生弹出窗口替代该窗口适配部分。Windows/Linux、托盘、自动启动、多屏定位和完整历史操作不属于本次验收。

## 来源

- https://docs.rs/gpui/0.2.2/gpui/ ：框架 API。
- https://github.com/zed-industries/zed/tree/main/crates/gpui/examples：官方窗口、列表示例。
- https://docs.rs/gpui-component/0.5.1/gpui_component/ ：兼容版本输入组件。
- GPUI 0.2.2 源码 `src/platform/mac/window.rs`：PopUp 的 NSPanel 实现。
- `src-tauri/crates/uc-tauri/src/quick_panel/macos.rs`：现有焦点与粘贴流程。
- `crates/uc-daemon-client/src/http/search.rs`：后台搜索契约。

## 验收

- [x] 编译与针对性测试（三项通过）
- [ ] 实际窗口、搜索、键盘操作
- [x] 后台连接与失败响应测试；真实后台只读查询返回 45 条
- [ ] 原应用粘贴与重复唤起
- [x] 记录结果与未验证边界（见 apps/gpui-quick-panel/README.md）
