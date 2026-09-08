# 流畅模式效果覆盖检查

日期：2026-09-08。依据：[实施规格](2026-09-08-adaptive-smooth-mode-spec.md)。

## 控制方式

当前锁定的 Motion 提供 `MotionGlobalConfig.skipAnimations` 与导出的 `visualElementStore`。统一入口在偏好变化时更新已挂载元素的减少动效标记，结束其值动画及布局动画；新挂载元素由各窗口 `VisualEffectsProvider` 控制。原有页面不重挂载，因此输入、选择和焦点不随模式切换丢失。此逻辑集中在 `src/lib/visual-effects-motion.ts`，升级 Motion 后必须重跑浏览器检查。

## 来源清单

| 来源 | 接入方式 | 当前验证 |
| --- | --- | --- |
| 主窗口、快捷面板、更新窗口 | 统一 provider；快捷面板通知也在范围内 | 主入口构建通过；双页模拟通知通过；原生窗口结果见验收记录 |
| 开关、选择指示、弹出面板 | 从统一状态读取减少动效，已有布局节点同步更新 | 开关真实浏览器切换通过；相关组件测试通过 |
| 输入框、操作替换、展开操作栏、通知、菜单、Select | 替换原 Motion 系统偏好 hook；局部 transition 仍受统一跳过控制 | 相关组件测试通过；尚未逐个场景真机交互 |
| 页面导航 | 共享布局读取统一减少动效值 | 类型检查及构建通过 |
| 正在执行的 opacity/x 动画 | Motion 值动画 complete 与 projection.finishAnimation | 浏览器中 2 秒动画进行中切换，立即到终态，输入内容保留 |
| 输入错误抖动 | 减少动效时不启动；清理时 complete 回到最终 x=0 | 代码检查；需补输入错误场景真机验证 |
| 主题圆形过渡 | 订阅模式，结束 Web Animations 并 skipTransition；处理取消拒绝 | 代码检查；完整主题切换交互尚未纳入本轮浏览器脚本 |
| 弹窗 CSS 动画 | 独立 reduce-motion 标记；保持定位 transform | 浏览器打开、Escape 关闭、焦点返回通过 |
| CSS 过渡、动画、平滑滚动 | 根部 reduce-motion 样式 | 浏览器切换通过 |
| 玻璃背景、快捷面板半透明、菜单投影 | low-effects 标记和公共语义样式 | 样式检查；原生快捷面板视觉待验收 |
| 加载、进度、错误 | 保留现有文字、数值及静态图标，不移除状态元素 | 代码检查；完整传输与加载矩阵待实测 |
| 非动画 rAF：搜索聚焦、虚拟文本滚动、菜单布局准备 | 保留，不当作装饰动画删除 | 代码检查 |

## 重复执行

```bash
node e2e/visual-effects-server.mjs
PLAYWRIGHT_CHANNEL=chrome node e2e/visual-effects-browser.mjs
```

浏览器脚本需要可解析的 Playwright 安装；可用 `PLAYWRIGHT_MODULE` 指向独立安装的模块入口，避免修改生产依赖。测试服务构建实际公共组件，用浏览器专用模拟连接隔离后台业务，不能替代 Tauri 事件、存储或强弱设备性能证明。

脚本覆盖双页状态传播、进行中动画结束、输入保留、弹窗焦点、系统减少动效、重新加载与 360 宽深色布局；图片输出由 `SMOOTH_TEST_OUTPUT` 指定。
