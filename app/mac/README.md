# Unic for macOS · SwiftUI UI preview

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

Mock 状态保存在内存中，重启重置；设置选项通过 UserDefaults 保存，仅演示 UI。复制会将样例文本写入系统剪贴板。PDF 当前展示文本占位，图片为原生渐变示例；发送、同步、登录项及保留期限不执行后台操作。尚未接入 unicd，也未实现全局快捷键或自动粘贴。

```sh
swift build
```

工程遵循 0006 的客户端边界，后续由独立客户端接入 daemon 的 HTTP / SSE 接口。用户指定目录为 `app/mac`，因此本原型采用该路径（0006 草案中的 `app/macos` 尚未迁移）。

## 新版 macOS 适配与验证

macOS 26+ 使用 ToolbarSpacer 划分工具栏操作组，详情面板的圆形按钮与反馈浮层使用 Liquid Glass，旧系统回退为标准按钮与材质。导航层玻璃由系统管理，内容区保持不透明、清晰。

已使用 Xcode 26.3（17C529）与 macOS 26.2 SDK 构建。当前宿主系统为 macOS 15.6.1，因此构建成功只验证 SDK 兼容性；macOS 26/27 的 Liquid Glass 实际外观、辅助功能效果及窗口适配仍需在对应系统验收。安装新版 Xcode 不会改变旧系统的原生渲染。

## 开发时截图

Debug 构建支持 `UNIC_SNAPSHOT=/tmp/out.png .build/debug/Unic` 把主窗口渲染为 PNG 后退出（加 `UNIC_DARK=1` 为深色）。用 `cacheDisplay`，不需要屏幕录制权限；窗口未激活，所以侧栏和按钮会偏灰，且不含 Liquid Glass 合成效果。
