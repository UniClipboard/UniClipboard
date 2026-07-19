# Plan 004：交付三种移动绑定并完成四平台真机互通

> **执行要求**：绑定层必须薄，只做类型转换和系统调用，不得复制配对、连接恢复、入站解码、文件组装或同步决策。每个平台正式包只允许使用完整 P2P 核心。
>
> **漂移检查**：`git diff --stat 1c229e9e1..HEAD -- crates/uc-mobile .github/workflows/build-mobile-core.yml docs/packaging/mobile-core-build-release.md crates/uc-engine`

## 状态

- **优先级**：P0
- **工作量**：L
- **风险**：HIGH
- **依赖**：`plans/003-introduce-unified-core-interface.md`
- **类别**：migration
- **计划基线**：`1c229e9e1`，2026-07-19

## 为什么必须做

现有 `uc-mobile` 是 LAN HTTP 客户端，不能扩名冒充完整节点。iOS/Android 已有 UniFFI 与 XCFramework/AAR 接入经验，HarmonyOS 社区版已有 N-API 经验；应复用交付方式，但三种绑定都只依赖计划 003 的统一入口。

## 当前事实

- `crates/uc-mobile/Cargo.toml` 和 `src/` 面向旧移动同步协议。
- `crates/uc-mobile/scripts/build-ios-xcframework.sh` 已实现 Swift 绑定与二进制同次生成。
- `crates/uc-mobile/scripts/build-android-aar.sh` 仍是拒绝执行的占位脚本。
- HarmonyOS 桥接目前约 1900 行并直接依赖四个内部 crate；新绑定不得重复该形态。
- Android 当前前台服务使用 `dataSync` 类型；后台在线能力必须按当前 Android 政策单独验收，不能写入核心假设。

## 范围

**允许修改**：

- 新建 iOS/Android 共用 UniFFI 绑定 crate
- 新建 HarmonyOS N-API 绑定 crate
- iOS Keychain、Android Keystore、HarmonyOS HUKS/Asset Store 接入
- 四平台产物构建和发布脚本
- 四平台一致性测试与真机测试应用
- 正式移动客户端的核心接入

**禁止修改**：

- 在绑定层实现业务状态机
- 用普通文件替代移动系统安全密钥库
- 让 iOS 永久后台在线成为验收条件
- 把 Android 后台剪贴板读取等同于节点在线
- 在正式构建中保留运行时 LAN/P2P 切换开关

## 步骤

### 1. 建立 iOS/Android 共用绑定

将 `uc-engine` 的稳定操作、事件和错误映射为 Swift/Kotlin 类型。绑定负责调用宿主适配器，不得暴露内部 Rust 类型。二进制和生成代码必须由同一次构建产生。

**验证**：

```bash
cargo build -p uc-engine-uniffi --release --target aarch64-apple-ios
cargo ndk -t arm64-v8a -t x86_64 build -p uc-engine-uniffi --release
```

预期均退出 0，且两种产物报告同一个 `core-vX.Y.Z`。

### 2. 建立 HarmonyOS 薄绑定

用 N-API 暴露同一操作和事件。把社区版桥接中的保活循环、入站解析、文件组装和运行任务收回核心。HarmonyOS 只保留系统剪贴板、密钥库、文件句柄、生命周期通知和 ArkTS 类型转换。

**验证**：HarmonyOS 构建退出 0；桥接 crate 只依赖公开核心入口，不依赖 `uc-core`、`uc-application`、`uc-infra`、`uc-bootstrap`。

### 3. 接入系统安全存储和文件出口

正式移动构建缺少系统安全存储时必须启动失败，不能静默改用明文文件。收到的文件保持核心密文，只有用户明确保存/分享时才向宿主文件句柄流式解密。

**验证**：密钥库不可用测试返回稳定错误；明文探针扫描通过；HarmonyOS 不再把接收文件明文长期写入 cache。

### 4. 定义真实移动生命周期

- iOS：前台完整节点；进入后台时按系统 deadline 收尾并暂停；恢复前台后重新启动 endpoint，身份不变。
- Android：前台完整节点；用户明确启用且系统政策允许时使用合规前台服务；被系统停止后正常离线并可恢复。
- HarmonyOS：按系统提供的后台能力运行；无授权时与 iOS 一样暂停和恢复。

节点在线时长可以不同，但协议、身份、内容能力和存储规则必须相同。

“对等”不表示系统剪贴板自动化程度完全相同。移动系统可能限制后台读取或监听剪贴板；最低标准是应用获得运行机会时能以完整节点身份创建/加入空间，并能由用户主动发送和接收文本、图片、文件。平台限制只能改变触发方式和在线时长，不能把节点降级成只接收、只支持文本或只能连接桌面的客户端。

### 5. 四平台一致性矩阵

至少覆盖以下设备对：desktop↔iOS、desktop↔Android、desktop↔HarmonyOS、iOS↔Android、iOS↔HarmonyOS、Android↔HarmonyOS。每一对测试创建/加入、文本、图片、文件、relay、换网、暂停恢复和版本混跑。

协议测试必须使用同一组 golden vectors；真实测试报告记录核心版本、设备系统版本和网络路径，不记录用户内容。

### 6. 统一发布产物

在当前桌面仓对同一候选版本运行完整发布干跑，但不创建公开 `core-v*` 标签或 Release。干跑产生：

- Rust 源码与锁文件
- iOS XCFramework、Swift 绑定、SwiftPM 校验值
- Android 多架构 AAR 与 Kotlin 绑定
- HarmonyOS HAR、动态库与 ArkTS 声明
- 校验值、签名、依赖与许可证清单、调试符号

首个公开 `core-v*` 标签只允许在计划 005 建立的唯一核心仓库中创建，避免两个仓库出现同名但来源不同的核心版本。

## 完成标准

- [ ] 三种移动绑定只依赖统一核心入口。
- [ ] 六种设备对的真机 P2P 互通矩阵全部通过。
- [ ] 四平台干跑产物来自同一核心候选版本和提交。
- [ ] 正式移动构建强制使用系统安全存储。
- [ ] iOS/Android/HarmonyOS 暂停和恢复后身份不变。
- [ ] 绑定和本地库无法被错误版本混用。

## 停止条件

- 任一绑定必须直接调用内部 facade 或协议实现。
- 任一平台只能靠旧 LAN HTTP 完成互通。
- 商店政策不允许所选后台服务方式，且产品仍要求永久后台在线。
- 任一移动平台需要不同协议或不同加密规则。
- 发布产物无法从同一提交可重复生成。
