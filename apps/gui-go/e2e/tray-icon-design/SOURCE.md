# 托盘图标设计参照（实心猫）

本目录是验收用的设计参照快照，不参与构建。

- 设计稿：Claude Design 画布 `5GGBoRRmhDyHDwJa8fc31y`，版本 `1791372485-ac1a`；锚点标题 `Tray icon — B solid cat · states & ear motion`，画板 `TrayBBuild`、`TrayBStates`、`TrayBMotion`。
- 画板源文件哈希（sha256）：
  - `TrayBBuild.dc.html` `9efba8585e6825c971d53a008a8a73dd1455dbacfbacd7908224783fbb4d91e6`
  - `TrayBStates.dc.html` `86d22b9cebb4d7e10189abb183523075fe6b2fab15d8df852f1245a5eeaae5d5`
  - `TrayBMotion.dc.html` `0596ea1900fab75c8aeae9c3376d576f35ef109ac046e693c9ea898d651b0538`
- `*.svg`：由 `TrayBStates` 的“macOS 浅色”列图标原样截取（每状态一个），外加 44 px 画布（18 pt 画稿居中于 22 pt）与浅色菜单栏底色 `#E9E9EC`；`new` 是“同步完成”加新内容圆点。
- `*.png`：用 `@resvg/resvg-js` 2.6.2 对上面的 SVG 独立栅格化（`rasterize.mjs`，输入为画板源文件路径和输出目录），与 Go 渲染器无共享代码。
- 对比由 e2e 构建的 `tray-icon-compare` 控制命令完成（`e2e/tray_icon_run.py`）。当前托盘只显示一个静态图标，验收只用 `synced.svg` / `synced.png`；其余状态的文件保留作设计参照，供以后恢复状态时使用。

## 一个字形（glyph）设计

- 画布同一份设计稿的另一组画板：`TrayIconBuild`（构造）、`TrayIconStates`（9 状态 × 5 环境），画布注记 `Tray icon — one glyph, nine states`。读取时的版本 `1791410974-e2c2`；`TrayIconBuild.dc.html` sha256 `658cbca8eeee9e8659b03222614f9b4ecf9b7aad4a42026d017f7c0dafb864da`，`TrayIconStates.dc.html` sha256 `ac6c259548cbea5846903a8c2ae46485b25a4aa763423b0b434a831b63138491`。
- `glyph.svg` / `glyph.png`：两张错位的卡片（描边 2、圆角 3.5、圆头），取自上面两块画板里“同步完成”的 macOS 浅色图标，布局和栅格化方式与 `synced.*` 相同（`@resvg/resvg-js` 2.6.2，44 px 画布、36 px 画稿）。
