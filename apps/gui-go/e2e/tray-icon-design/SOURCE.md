# 托盘图标设计参照（一个字形）

本目录是验收用的设计参照快照，不参与构建。

- 设计稿：Claude Design 画布 `5GGBoRRmhDyHDwJa8fc31y`，读取时的版本 `1791410974-e2c2`；画布注记 `Tray icon — one glyph, nine states`，画板 `TrayIconBuild`（构造）与 `TrayIconStates`（9 状态 × 5 环境）。
- 画板源文件哈希（sha256）：
  - `TrayIconBuild.dc.html` `658cbca8eeee9e8659b03222614f9b4ecf9b7aad4a42026d017f7c0dafb864da`
  - `TrayIconStates.dc.html` `ac6c259548cbea5846903a8c2ae46485b25a4aa763423b0b434a831b63138491`
- `glyph.svg`：两张错位的卡片（描边 2、圆角 3.5、圆头），取自上面画板里“同步完成”的 macOS 浅色图标；画布 44 px，画稿 36 px 居中，浅色菜单栏底色 `#E9E9EC`，前景 `#1D1D1F`。
- `glyph.png`：用 `@resvg/resvg-js` 2.6.2 对 `glyph.svg` 独立栅格化（`rasterize.mjs`），与 Go 渲染器无共享代码。
- 对比由 e2e 构建的 `tray-icon-compare` 控制命令完成（`e2e/tray_icon_run.py`）。它比较的是设计自己的 44 px / 36 px 几何下的形状；托盘实际收到的图像（macOS 上网格按 44 px 绘制）另有检查，不由这个参照代替。
- 其他 8 个状态和动效没有实现，也没有保留它们的参照文件；需要时从设计画板重新取。
