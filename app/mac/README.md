# Unic for macOS · SwiftUI 实验原型（停止推进）

2026-11-09：SwiftUI 客户端路线已取消，桌面界面（含 macOS）采用 mygo + 自定义 UI Kit，界面原型待定。此目录保留已有实验代码与插件设置 Schema 样例，不再计划接入 daemon；以下构建和交互说明仅描述实验原型。当前方向见[仓库总览](../../docs/architecture/overview.md)。

运行最低 macOS 14；构建需要 Xcode 26+（macOS 26 SDK）。无第三方依赖，不需要启动 Go daemon。

```sh
cd app/mac
swift run Unic
```

打包为可双击启动的本地 `.app`（ad-hoc 签名）：

```sh
./scripts/build-app.sh
open .build/Unic.app
```

也可以用 Xcode 打开 `Package.swift`，选择 Unic executable 运行。

## 界面与交互

沉浸式图库：内容铺满整张缩略图，没有侧栏。设计语言参考近期的 Mac 剪贴板/素材类应用，同时遵循 Adopting Liquid Glass 的分层原则（内容保持不透明，玻璃只用于工具栏与圆形按钮）。

- 左上是搜索（⌘F 聚焦），其下是胶囊分类：History / Text / Links / Code / Images / Files（带计数）与“设备”下拉；右上是同步状态和固定、最近删除两个圆形开关。
- 缩略图：文字与代码铺满整块，链接与图片用渐变压暗；底部是来源应用的真实图标、相对时间（“1 hr ago”）和大小。选中显示高对比描边。
- 点击一张缩略图，右侧弹出悬浮详情面板（覆盖在图库上，不参与布局，不会让网格重排）：顶部圆形按钮（固定、删除、关闭），大预览，小号大写分节（Created / Source / Link / Delivery / Tags），底部整宽“Copy to Clipboard”与发送到。Esc 或 × 关闭。
- 键盘：方向键移动、↩ 复制、空格 Quick Look、⌫ 删除（最近删除中为恢复）。可恢复的删除不弹确认。
- 菜单栏弹窗为最近 5 条 + 打开、暂停同步、设置；设置为标准 TabView；⌥⌘V 浮动面板保留。

Mock 状态保存在内存中，重启重置；设置选项通过 UserDefaults 保存，仅演示 UI。复制会将样例文本写入系统剪贴板。PDF 当前展示文本占位，图片为原生渐变示例；发送、同步、登录项及保留期限不执行后台操作。尚未接入 unicd；代码已注册 ⌥⌘V 全局快捷键，未实现自动粘贴。

Plugins 设置页的 “Preview plugin settings…” 从 Go 配置定义生成的静态 schema 创建原生表单，覆盖文本、数字、选项、开关、多行、密码、分组和基础列表。支持即时校验、保存预览、取消和恢复默认值；预览配置仅在内存中，不写 UserDefaults、daemon 或 TOML。Foundation 模型在 `Sources/SettingsSchema`，通用控件在 `SchemaSettingsForm.swift`；`swift test` 与 Go 使用同一组校验向量。更新 fixture 使用仓库根的 `go generate ./plugins/examples/settingsdemo`。

```sh
swift build
swift test
```

本实验原型使用 `app/mac` 路径，不再按已取消的 SwiftUI 客户端路线接入 daemon。

## 新版 macOS 适配与验证

macOS 26+ 使用 ToolbarSpacer 划分工具栏操作组，详情面板的圆形按钮与反馈浮层使用 Liquid Glass，旧系统回退为标准按钮与材质。导航层玻璃由系统管理，内容区保持不透明、清晰。

已使用 Xcode 26.3（17C529）与 macOS 26.2 SDK 构建。当前宿主系统为 macOS 15.6.1，因此构建成功只验证 SDK 兼容性；macOS 26/27 的 Liquid Glass 实际外观、辅助功能效果及窗口适配仍需在对应系统验收。安装新版 Xcode 不会改变旧系统的原生渲染。

## 开发时截图

Debug 构建支持 `UNIC_SNAPSHOT=/tmp/out.png .build/debug/Unic` 把主窗口渲染为 PNG 后退出（加 `UNIC_DARK=1` 为深色）。用 `cacheDisplay`，不需要屏幕录制权限；窗口未激活，所以侧栏和按钮会偏灰，且不含 Liquid Glass 合成效果。

设置表单截图可加 `UNIC_TARGET=plugin-settings`。截图使用没有密码默认值的固定样例。
