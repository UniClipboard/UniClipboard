# 托盘猫头图像来源

`tray-cat@2x.png` 是 `general.trayIcon` 为 `cat` 时使用的来源图像。

- 来源：已退役宿主在 `v1.1.2` 发布的 macOS 托盘图像（git blob `b745edc61efe18b8895d427830919280d62693b9`），逐字节复制。退役记录见 `docs/architecture/gui-go-tauri-retirement.md`。
- sha256：`21e459b6f6fd8fa4de3335370f8198a400ae7d555550388cf48d239d4e34a93b`。
- 格式：44 × 44 RGBA PNG。所有可见像素均为黑色，形状在 alpha 通道中（macOS 模板图）；原始 alpha 包围盒为 40 × 39 px。
- `v1.1.2` 只在 macOS 使用此图（由已退役宿主的托盘模块嵌入）；Windows 和 Linux 使用应用图标。#1914 之前的 Go 宿主嵌入同哈希的 `assets/tray-icon@2x.png`。
- 此图不是 `c7209b188..a16a466e7` 中的实心猫矢量设计；该设计从未进入发布标签。
- 平台显示：
  - macOS：资源字节不变；运行时用已有的 Catmull–Rom alpha 缩放路径，把整张来源图缩到 34 × 34 px，居中放入 44 × 44 px 透明画布，填充黑色后交给 `SetTemplateIcon`，菜单栏负责着色。2x 下可见 alpha 包围盒为 32 × 31 px（16 × 15.5 pt），相较原图的 40 × 39 px（20 × 19.5 pt）更接近原生菜单栏尺寸。完整来源图居中，因原图 alpha 略不对称，可见区域上下留白相差 1 px。尺寸事实来源是 `tray_icon_macos.go`；不另存派生资源。
  - Windows 和 Linux：继续提取原图 alpha，缩到平台托盘尺寸，再填充平台前景色（`trayIconSpec()`），输出不变。它们是从 macOS 资源派生的新渲染；`v1.1.x` 从未在这两个平台发布猫头托盘图标。
- 校验：在仓库内运行 `apps/gui-go/icons/check-tray-cat.sh`，三个哈希相同时退出码为 0。校验的是嵌入资源的来源字节，不是 macOS 运行时缩放后的 PNG。
