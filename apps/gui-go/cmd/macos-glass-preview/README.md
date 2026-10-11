# macOS 窗口材质预览

`macos-glass-preview` 是开发用预览程序，用于在真实的 Wails 窗口里核对 macOS 窗口材质。它不属于任何产品构建，不读取 profile，也不连接 daemon。

## 使用的现成能力

- 窗口背景：`MacBackdropLiquidGlass`，来自固定版本的 Wails v3.0.0-beta.28。
  - macOS 26 及以上：NSGlassEffectView。
  - 更低版本：回退到 NSVisualEffectView。
- 标题栏：`MacTitleBarHiddenInset`，红绿灯是系统原生绘制的。
- 页面：通过反向代理从 Vite dev server 加载，因此 Wails 运行时可用。

## 运行

```bash
# 1. 启动前端 dev server（原型入口不在正式构建的 rollup inputs 里）
cd apps/gui-go/frontend && UC_DEV_SERVER_PORT=1531 npx vite

# 2. 原生材质：网页透明，侧栏显示窗口材质
cd apps/gui-go && go run -tags private_mac_apis ./cmd/macos-glass-preview -theme light -generation 27

# 3. 公开 API 对照：网页保持不透明，页面使用 CSS fallback
cd apps/gui-go && go run ./cmd/macos-glass-preview -theme light
```

可用参数：

| 参数 | 作用 | 默认值 |
| --- | --- | --- |
| `-theme light\|dark\|system` | 同时决定窗口的 NSAppearance 和页面主题 | `system` |
| `-generation 26\|27` | 设计稿中窗口布局的版本 | `27` |
| `-width`、`-height` | 初始窗口尺寸 | — |
| `-dev-server` | 页面来源 | — |
| `-path` | 打开的页面 | `/macos-design-preview.html` |

## 宿主参数

预览程序在窗口 URL 上带 `window-material` 参数，取值为 `liquid-glass`、`visual-effect` 或 `none`。前端通过 `src/host/window-material.ts` 读取，mac 皮肤的 `Surface` 据此在节点上标记 `data-window-material`。

这个值由宿主根据两点推导，前端不做猜测：

- 是否带 `private_mac_apis` 构建：不带时为 `none`。
- macOS 主版本：26 及以上为 `liquid-glass`，更低为 `visual-effect`。

## 风险与限制

- **私有 API 依赖**：WebView 透明只能通过私有的 `drawsBackground` 实现，Wails 只在 `-tags private_mac_apis` 时启用。正式构建尚未采用这个 tag。是否采用，需要单独评估 Mac App Store 和公证风险。
- **窗口外观不能动态切换**：Wails beta.28 只在创建窗口时设置 `Appearance`。应用内切换主题需要原生桥接或重建窗口；跟随系统时由 AppKit 自动处理。
- **只有整窗玻璃**：设计稿中 macOS 26“内缩悬浮侧栏”这类局部玻璃，需要自写原生视图并与网页布局同步。
- **网页浮层透不出材质**：WKWebView 的 `backdrop-filter` 只采样网页内容。因此在原生材质下，mac 皮肤的菜单、popover 等浮层使用不透明底色。

## 截图核对

`capture/capture-over-backdrop.sh` 的流程：

1. 构建预览程序。
2. 在预览窗口正下方放一个合成的条纹背景窗口。
3. 截取该区域。截取前会确认这块区域的前两个窗口正是预览窗口和背景窗口，否则拒绝截图，避免拍到其他应用的内容。

```bash
apps/gui-go/cmd/macos-glass-preview/capture/capture-over-backdrop.sh /tmp/glass-check light-27 -theme light
PUBLIC=1 apps/gui-go/cmd/macos-glass-preview/capture/capture-over-backdrop.sh /tmp/glass-check public-27 -theme light
```

使用前提：终端需要“屏幕录制”权限。脚本不修改任何系统设置。
