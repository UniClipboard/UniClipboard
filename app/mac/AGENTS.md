# macOS app

- 原生 SwiftUI UI 原型，最低运行 macOS 14，构建需要 Xcode 26+ / macOS 26 SDK；用 `swift build --package-path app/mac` 构建。
- 参考 UI 位于相邻 uniclipboard 仓库的 macos-design prototype 与 macos skin。
- 使用系统控件、工具栏、材质和语义颜色，支持深色模式和键盘操作。
- mock 数据仅在内存中；接入真实服务前先读 docs/design/decisions/0006-macos-swiftui-client.md 和 API 契约。
- 不在 Swift 重写同步或读取 SQLite；设备地址、配对票据与正文不得写入日志。

## Apple 官方设计依据

修改 UI / UX 前，先阅读与任务相关的官方资料。参考项目用于复原产品布局与交互，系统外观及行为以 Apple 官方指南为准。

| 资料 | 用途 |
|---|---|
| [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/) | 布局、导航、材质、控件和交互的总体规范 |
| [Designing for macOS](https://developer.apple.com/design/human-interface-guidelines/designing-for-macos) | Mac 窗口、信息密度、菜单、键盘及窗口适配 |
| [Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/liquid-glass) | 新设计体系与官方示例入口 |
| [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass) | 新 SDK 迁移、标准组件及自定义界面的适配 |
| [Get to know the new design system · WWDC25](https://developer.apple.com/videos/play/wwdc2025/356/) | 层级、形状、内容与导航层的关系 |
| [Build a SwiftUI app with the new design · WWDC25](https://developer.apple.com/videos/play/wwdc2025/323/) | SwiftUI 分栏、工具栏分组、搜索、控件与玻璃效果 |
| [Build an AppKit app with the new design · WWDC25](https://developer.apple.com/videos/play/wwdc2025/310/) | macOS 26 侧栏、inspector、工具栏、滚动边缘及窗口圆角 |
| [Modernize your AppKit app · WWDC26](https://developer.apple.com/videos/play/wwdc2026/289/) | macOS 27 的设计调整，重点参见 14:09 起的 Design updates |

### 实现与验收约定

- 使用原生控件只是基础；仍需按官方设计处理导航与内容层级、工具栏操作分组、搜索位置、inspector 及滚动边缘。
- Liquid Glass 优先用于导航和操作层，正文保证清晰可读；不要给所有内容卡片添加玻璃，也不要用普通透明度或模糊冒充系统 Liquid Glass。
- 区分 macOS 26 与 27 的系统行为，让原生组件随系统适配，避免硬编码窗口圆角和侧栏外形来模拟某个版本。
- 新 API 使用 availability 检查，保留最低支持系统的回退实现。最低部署版本与构建 SDK 版本是不同概念。
- 验证新版外观时记录 macOS、Xcode 和 SDK 版本；在对应系统与新版 SDK 下检查浅色/深色、窗口缩放、键盘操作、降低透明度及增强对比度。旧 SDK 编译通过不能作为 macOS 26/27 外观验收依据。
