# 跨平台应用图标圆角问题修复

## 问题描述

打包后的应用图标在不同平台上的圆角显示问题:
- **macOS**: 图标没有显示系统圆角效果
- **Linux**: 部分桌面环境(GNOME/KDE)图标缺少圆角
- **Windows**: 无需圆角(Windows 系统不添加圆角效果)

## 根本原因

当前使用的源图标 `assets/logo.png` 包含**渐变背景**,而:
- macOS 系统的圆角蒙版只能正确应用于**透明背景**的图标
- Linux 桌面环境通常不会自动添加圆角
- 如果要保留背景同时显示圆角,需要在图标本身添加圆角和透明边缘

## 解决方案

### ✅ 推荐方案: 保留背景 + 添加圆角(跨平台)

**一键生成所有平台图标:**

```bash
./scripts/generate-all-icons.sh assets/logo.png --keep-background
```

这个脚本会:
- ✅ **macOS**: 生成带圆角的 `.icns` 文件
- ✅ **Linux**: 生成带圆角的 PNG 图标
- ✅ **Windows**: 生成标准 `.ico` 文件(无圆角,符合 Windows 设计规范)

**生成的文件:**
- `src-tauri/icons/icon.icns` (macOS)
- `src-tauri/icons/icon.ico` (Windows)
- `src-tauri/icons/32x32.png` (Linux)
- `src-tauri/icons/128x128.png` (Linux)
- `src-tauri/icons/128x128@2x.png` (Linux)

### 方案 2: 仅生成 macOS 图标

如果只需要更新 macOS 图标:

```bash
./scripts/generate-macos-icon.sh assets/logo.png --keep-background
```

### 方案 3: 使用透明背景图标

1. **准备透明背景的源图标**
   - 使用图像编辑工具(Photoshop / GIMP / Figma / Sketch)
   - 删除背景,保留 alpha 通道
   - 导出为 PNG 格式,尺寸建议 1024x1024 或更大
   - 保存到 `assets/logo-transparent.png`

2. **运行图标生成脚本**
   ```bash
   ./scripts/generate-all-icons.sh assets/logo-transparent.png
   ```

3. **重新打包应用**
   ```bash
   bun run tauri build
   ```

## 前置要求

需要安装 ImageMagick:

```bash
# macOS
brew install imagemagick

# Linux (Debian/Ubuntu)
sudo apt install imagemagick

# Linux (Fedora/RHEL)
sudo dnf install imagemagick
```

## 验证

生成新的 `.icns` 文件后,检查是否包含 alpha 通道:

```bash
sips -g hasAlpha src-tauri/icons/icon.icns
```

应该输出: `hasAlpha: yes`

## 技术细节

### 各平台图标系统差异

| 平台 | 格式 | 圆角处理 | 解决方案 |
|------|------|----------|----------|
| **macOS** | `.icns` | 系统自动添加,但需要透明背景 | 主动添加圆角 + 透明边缘 |
| **Linux** | `.png` | 桌面环境通常不添加 | 主动添加圆角 + 透明边缘 |
| **Windows** | `.ico` | 不添加圆角 | 保持原样,无需圆角 |

### macOS 图标工作原理

- macOS 使用 `.icns` 格式存储多分辨率图标
- 系统会自动为应用图标添加圆角蒙版
- **圆角蒙版只对透明背景有效**
- 如果图标有不透明背景,圆角区域会显示背景色,看起来像"没有圆角"
- **解决方案**: 在图标本身添加圆角(半径约为图标尺寸的 22.37%),并将四角设为透明

### Linux 图标工作原理

- 使用标准 PNG 格式
- 不同桌面环境(GNOME, KDE, XFCE)行为不同
- 大多数桌面环境不会自动添加圆角
- **解决方案**: 与 macOS 相同,主动添加圆角

### Windows 图标工作原理

- 使用 `.ico` 格式,包含多个尺寸的位图
- Windows 系统**不添加圆角**,保持图标原样
- **无需处理**: 直接使用原始图标即可

### 所需图标尺寸

**macOS (.icns)**:
- 16x16, 32x32 (16@2x)
- 32x32, 64x64 (32@2x)
- 128x128, 256x256 (128@2x)
- 256x256, 512x512 (256@2x)
- 512x512, 1024x1024 (512@2x)

**Linux (.png)**:
- 32x32, 128x128, 256x256

**Windows (.ico)**:
- 16x16, 32x32, 48x48, 64x64, 128x128, 256x256

## 相关文件

- 源图标: `assets/logo.png`
- 跨平台生成脚本: `scripts/generate-all-icons.sh` ⭐️ 推荐
- macOS 专用脚本: `scripts/generate-macos-icon.sh`
- 输出目录: `src-tauri/icons/`
  - `icon.icns` (macOS)
  - `icon.ico` (Windows)
  - `32x32.png`, `128x128.png`, `128x128@2x.png` (Linux)
- Tauri 配置: `src-tauri/tauri.conf.json`

## 参考资料

- [Apple Human Interface Guidelines - App Icons](https://developer.apple.com/design/human-interface-guidelines/app-icons)
- [iconutil man page](https://ss64.com/osx/iconutil.html)
