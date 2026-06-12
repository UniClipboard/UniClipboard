# UniClip

多端剪贴板同步移动客户端，目前仅支持 Android。

## 功能特性

### 剪贴板同步和历史记录

- 文本、图片、单文件类型的剪贴板同步
  - 通过通知栏快捷方式、桌面快捷方式、分享菜单手动触发同步
  - 后台自动同步剪贴板
- 历史记录同步
- 自动上传短信验证码

### 服务器支持

- **SyncClipboard 协议服务器**
- **WebDAV 服务器**
- **S3 对象存储**

## 截图

<p align="center">
  <img src="docs/screenshorts/Screenshot01.jpg" width="250" alt="首页" />
  <img src="docs/screenshorts/Screenshot02.jpg" width="250" alt="历史记录" />
  <img src="docs/screenshorts/Screenshot03.jpg" width="250" alt="设置" />
</p>

## 开发

### 安装依赖

```bash
npm install
```

### 生成原生项目

```bash
npm run prebuild
```

### 调试运行

```bash
# Android
npm run android

# iOS
npm run ios
```

### 构建 APK

```bash
npm run build:apk
```

### 其他命令

```bash
# 类型检查
npm run type-check

# 代码检查
npm run lint

# 自动修复代码问题
npm run lint:fix

# 格式化文档（JSON/Markdown）
npm run format-docs

# 构建 Expo 原生插件
npm run plugin:build
```

## 发布与版本号

发版流程、版本号策略、上游同步工作流见 [docs/RELEASE.md](./docs/RELEASE.md)。

## 致谢

UniClip 基于以下开源项目改进，特此致谢：

- [Jeric-X/SyncClipboard](https://github.com/Jeric-X/SyncClipboard) — 原始 SyncClipboard 协议与桌面端实现（MIT）
- [Jeric-X/syncclipboard-mobile](https://github.com/Jeric-X/syncclipboard-mobile) — 移动端原始实现（MIT，作者 JericX）

UniClip 兼容 SyncClipboard 协议，可与 SyncClipboard 服务端配合使用。

## 开源依赖

### JavaScript / TypeScript 依赖

| 仓库                                                                                                              | 说明                          |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| [facebook/react-native](https://github.com/facebook/react-native)                                                 | 跨平台移动框架                |
| [expo/expo](https://github.com/expo/expo)                                                                         | React Native 工具链与原生模块 |
| [react-navigation/react-navigation](https://github.com/react-navigation/react-navigation)                         | 导航库                        |
| [pmndrs/zustand](https://github.com/pmndrs/zustand)                                                               | 轻量状态管理                  |
| [Shopify/flash-list](https://github.com/Shopify/flash-list)                                                       | 高性能列表渲染                |
| [software-mansion/react-native-reanimated](https://github.com/software-mansion/react-native-reanimated)           | 动画库                        |
| [software-mansion/react-native-gesture-handler](https://github.com/software-mansion/react-native-gesture-handler) | 手势处理                      |
| [software-mansion/react-native-screens](https://github.com/software-mansion/react-native-screens)                 | 原生导航屏幕容器              |
| [th3rdwave/react-native-safe-area-context](https://github.com/th3rdwave/react-native-safe-area-context)           | 安全区域适配                  |
| [callstack/react-native-pager-view](https://github.com/callstack/react-native-pager-view)                         | 原生分页视图                  |
| [satya164/react-native-tab-view](https://github.com/satya164/react-native-tab-view)                               | Tab 切换视图                  |
| [react-native-async-storage/async-storage](https://github.com/react-native-async-storage/async-storage)           | 本地键值存储                  |
| [react-native-netinfo/react-native-netinfo](https://github.com/react-native-netinfo/react-native-netinfo)         | 网络状态监听                  |
| [axios/axios](https://github.com/axios/axios)                                                                     | HTTP 客户端                   |
| [dotnet/aspnetcore (SignalR)](https://github.com/dotnet/aspnetcore)                                               | 实时推送客户端                |
| [expo/vector-icons](https://github.com/expo/vector-icons)                                                         | 矢量图标库                    |
| [jiang0508/react-native-feather](https://github.com/jiang0508/react-native-feather)                               | Feather 图标组件              |
| [onubo/react-native-logs](https://github.com/onubo/react-native-logs)                                             | 日志工具                      |
| [margelo/react-native-worklets](https://github.com/margelo/react-native-worklets)                                 | JS Worklets 运行时            |
| [emn178/js-sha256](https://github.com/emn178/js-sha256)                                                           | SHA-256 哈希计算              |
| [linonetwo/segmentit](https://github.com/linonetwo/segmentit)                                                     | 中文分词（词语选取功能）      |

### Android 依赖

| 仓库                                                                                                                  | 说明                                    |
| --------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| [facebook/react-native](https://github.com/facebook/react-native)                                                     | React Native Android 运行时             |
| [facebook/hermes](https://github.com/facebook/hermes)                                                                 | Hermes JavaScript 引擎                  |
| [react-native-community/jsc-android-buildscripts](https://github.com/react-native-community/jsc-android-buildscripts) | JavaScriptCore Android 引擎（备选）     |
| [RikkaApps/Shizuku](https://github.com/RikkaApps/Shizuku)                                                             | Shizuku API：无需 Root 的系统 API 访问  |
| [dotnet/aspnetcore (SignalR Java 客户端)](https://github.com/dotnet/aspnetcore)                                       | SignalR 实时推送（Java/Android 客户端） |
| [google/gson](https://github.com/google/gson)                                                                         | JSON 序列化（SignalR 协议层）           |

## 许可协议

本项目包含以下版权声明：

- Copyright (c) 2026 JericX（上游 SyncClipboard 原作者）
- Copyright (c) 2026 mkdir700（UniClip）

详见 [LICENSE](./LICENSE)。
