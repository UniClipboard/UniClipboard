# GPUI 快捷面板

这是独立运行、不参与发行的原生快捷面板验证程序，复用现有 UniClipboard 后台。

## 当前窗口设计

按用户的最新要求，历史与预览使用两个真正独立的 GPUI 原生窗口，各有自己的绘制根和窗口边界。历史窗默认 360×420。预览内容宽 360，另有 8 宽的箭头区域；高度由实际内容排版决定，限制在 96–480（随界面缩放）。预览以当前记录的可见区域为锚点，优先右侧、空间不足时左侧；上下位置避让屏幕边缘，箭头继续指向记录。历史窗口不变宽、不移动。

两窗共享会话生命周期：在两窗之间切换焦点保持显示；离开两窗、按 Esc 或完成复制粘贴时共同隐藏。隐藏后复用原窗口。预览首次延迟 500ms，切换记录延迟 120ms。预览使用只读显示快照，所有后台操作仍由历史面板统一持有。

窗口尺寸通过 GPUI 的 `Window::resize` 更新，禁止直接修改原生窗口尺寸。后者会在当前窗口仍被更新时同步触发回调，导致系统窗口与绘制区域不同步。原生接口负责定位、显示、隐藏和无边框外壳。GPUI 的 `titlebar: None` 在 macOS 仍会创建带系统窗框的窗口，预览需要明确使用无边框 NSPanel，避免箭头后方露出矩形底板。

## 运行

从仓库根目录执行，先启动并解锁现有 UniClipboard：

```bash
cargo run -p uc-gpui-quick-panel
```

开发配置需两端使用相同的 `UC_PROFILE`。默认快捷键为 macOS 的 Command + Control + V，连接后读取已保存的快捷键配置。原型与正式应用同时运行时，可显式指定测试快捷键：

```bash
UC_PROFILE=dev UC_GPUI_SHORTCUT=ctrl+alt+space cargo run -p uc-gpui-quick-panel
```

`UC_GPUI_SCALE` 可设为 0.8 至 1.5。当前原型不读取 Tauri 私有的 WebView 本地存储。

macOS 构建需要包含 Metal 编译工具的 Xcode。如默认版本缺少组件，可仅为本次命令设置 `DEVELOPER_DIR`，不改系统默认设置。

## 已接入的操作

- 后台搜索，最多 50 条；输入防抖，清空立即查询，过期请求取消。
- 类型栏与 Tab／Shift+Tab 循环；标签、来源、时间、扩展名条件及输入提示。
- 上下键、Control+N/P、前十项数字快捷键；中文输入法组合状态不被列表按键接管。
- 回车／点击粘贴，Option 纯文本；搜索框无选中文字时 Command+C 复制并收起；空查询 Command+V 粘贴。
- 右键复制、收藏、删除、发送设备、文件路径粘贴与定位文件。
- 图片缩略图与三列图片墙、文字／图片／文件预览、后台变更订阅。
- 现有主题预设和自定义颜色、Inter 与 JetBrains Mono 字体。

自动粘贴需要 macOS 辅助功能权限；原应用已退出或焦点变化时保留错误提示，不盲发按键。原型只连接已有后台，不负责启动、初始化后台，不创建用户内容数据库或磁盘缓存。

## 验证

```bash
cargo test -p uc-gpui-quick-panel
cargo clippy -p uc-gpui-quick-panel --all-targets --no-deps -- -D warnings
bun apps/gpui-quick-panel/export-theme.ts --check
```

真实后台只读检查：

```bash
UC_PROFILE=dev cargo test -p uc-gpui-quick-panel live_daemon_search -- --ignored --nocapture
```

合成后台与原型分别在两个终端启动：

```bash
node apps/gpui-quick-panel/tests/fixture.mjs
```

```bash
UNICLIPBOARD_DAEMON_BASE_URL=http://127.0.0.1:48173 \
UNICLIPBOARD_DAEMON_TOKEN_PATH=apps/gpui-quick-panel/tests/fixture-token.txt \
UC_GPUI_SHORTCUT=ctrl+alt+space cargo run -p uc-gpui-quick-panel
```

测试复制会改写系统剪贴板，运行中的其他剪贴板后台可能正常采集这条合成文本。`tests/reference.html` 直接使用现有 React 组件，仅用于同数据视觉对照，不参与原型运行。

实际窗口验证：

```bash
swift apps/gpui-quick-panel/tests/window_geometry.swift <原型进程号>
uv run --with pillow python apps/gpui-quick-panel/tests/check_surface.py <历史窗截图> <预览窗截图>
```

截图使用系统 `screencapture -x -o -l <窗口号>`，排除窗口阴影。检查历史窗四边，以及预览主体和箭头透明区域，能发现透明正文或多余外边距。系统窗框和阴影需要另外检查包含阴影的实机截图。

## 已取得的证据与剩余范围

2026-09-10：macOS 编译、14 项自动测试及原型自身严格检查通过。系统确认历史与预览是不同窗口；短文本预览高度 145，多行文本 233，长文本上限 480。左侧展开和连续箭头轮廓已截图确认。实测两窗间切换、共同隐藏、复用同一窗口再次唤起；合成记录成功恢复并粘贴到文本编辑。此前真实后台只读搜索返回 45 条历史。

完整功能对齐仍以 [PARITY.md](../../.planning/research/gpui-quick-panel/PARITY.md) 为准。传输进度与取消、完整来源元数据、双击修饰键、所有语言与实时配置更新等尚未完成完整验收，不将其称为正式客户端替代品。Windows/Linux 的原生窗口组、文件定位和自动粘贴尚未实现。

全依赖严格检查仍会遇到既有 `uc-app-paths` 文档缩进警告，因此上面的严格检查使用 `--no-deps`。验证从隔离源码目录运行，避开本机祖先目录的 Engine 路径覆盖；锁文件保留仓库固定的 Engine 提交。

主题和字体由已有前端来源导出，禁止独立维护另一套颜色值：

```bash
bun apps/gpui-quick-panel/export-theme.ts
uv run --with fonttools --with brotli python apps/gpui-quick-panel/export-fonts.py
```
