# VISION.md

**UniClipboard 是一款端到端加密的跨平台剪贴板同步工具，通过 P2P 网络让个人拥有的多台设备共享同一份剪贴板。**

---

## 项目定义

- 产品定位：「多台设备服务一个人」——不是协作工具，不是消息队列
- 支持 macOS、Windows、Linux 桌面全平台，iOS/Android 作为 LAN 伴侣设备
- 同步内容涵盖文本、图片、文件、链接、富文本、代码片段
- 交付形态：GUI 桌面应用（Tauri + React）、CLI 工具（uniclip）、后台 daemon（uniclipd）
- 核心交互入口：Quick Panel（Spotlight 式全局快捷面板，搜索历史、即时粘贴）

## 核心目标

- **数据主权**：剪贴板内容永远只存在于用户自己的设备上，不经过任何中心服务器存储
- **端到端加密**：所有数据在静态存储和传输过程中均使用 XChaCha20-Poly1305 AEAD 加密，passphrase 在 Space 创建时强制设定
- **零配置可用**：设备配对后即自动同步，Quick Panel 默认启用，开箱即用
- **轻量常驻**：GUI 是 daemon 的纯客户端，不加载 SQLite/iroh，可随时进入 Lightweight Mode（GUI 退出、daemon 继续）
- **跨平台一致性**：同一份 uc-core 领域模型驱动所有平台，差异仅在适配器层

## 架构原则

- **六边形架构（Ports & Adapters）**：uc-core 定义纯领域模型和 Port trait，零基础设施依赖；uc-infra/uc-platform 提供具体实现
- **uc-core 禁区**：不得出现数据库（SQLite/Diesel）、网络框架（iroh/HTTP）、OS API、加密算法实现（Argon2/ChaCha20）——只允许领域概念
- **GUI 与 daemon 分离**：GUI 进程通过 HTTP/WS 连接外部 daemon，绝不内嵌 AppFacade 或打开数据库；daemon 绝不依赖任何 GUI 框架
- **uc-desktop GUI 框架无关**：该 crate 禁止依赖 Tauri/AppKit/egui 等，由 uc-tauri 负责 GUI 壳适配
- **薄中间层隔离重依赖**：uc-daemon-contract/uc-daemon-client/uc-daemon-process 作为叶子 crate，不携带 iroh/diesel/sqlite，使 CLI 和 GUI release 二进制免于链接重型依赖
- **可测试性**：76+ async trait Port 均为 Send + Sync，通过 Arc&lt;dyn Port&gt; 注入，应用层测试使用 mockall/手写 fake，永不触碰真实基础设施

## 安全与隐私底线

- **加密不可绕过**：Space 初始化强制设定 passphrase，MasterKey 直接加密所有本地历史；无 passphrase 则无法解密、无法存储
- **双层传输加密**：应用层 AEAD（MasterKey per-chunk 加密）+ 传输层 QUIC 通道加密（iroh Ed25519 身份认证）
- **AAD 绑定防重放**：每条密文通过 Additional Authenticated Data 绑定到具体实体（event_id、blob_id、transfer_id+chunk_index），密文不可跨实体迁移
- **密钥材料 zeroize-on-drop**：MasterKey、Kek、Passphrase、Plaintext、ProofDerivedKey 均实现内存归零
- **配对安全**：passphrase 验证使用 HMAC 挑战 - 应答协议；mDNS 广播配对码的 blake3 哈希前缀，被动观察者无法获取明文码
- **日志脱敏**：所有敏感类型 Debug impl 输出 [REDACTED]，tracing 禁止记录明文剪贴板内容、密码、密钥、完整 token
- **遥测隔离**：analytics ID 与业务 DeviceId 完全独立，永不关联；遥测永不上传剪贴板内容、文件名、文件路径、用户名、原始 IP
- **LAN-only 模式**：allow_relay_fallback=false 时完全禁用 relay，仅 mDNS 本地发现，无任何流量离开局域网
- **PID 身份验证**：向 daemon 发送信号前必须验证 PID 存活性 + 可执行文件路径匹配，防止信号误发给复用 PID 的无关进程

## 用户体验哲学

- **背景优先**：窗口关闭仅隐藏到托盘，应用始终作为系统服务常驻
- **Quick Panel 零延迟感知**：启动时预创建隐藏 WebView 并转换为 NSPanel（macOS），首次唤起无窗口创建开销
- **粘贴不夺焦**：NSPanel NonactivatingPanel 样式保持前一应用焦点，粘贴时先确认焦点恢复再发送按键
- **模糊事件防抖**：300ms show-debounce + 100ms verify-delay 消除 IME/系统通知导致的误关闭
- **跨平台视觉适配**：Linux 自动禁用 backdrop-blur/动画/阴影等重效果，macOS/Windows 保留完整视觉
- **自愈 autostart**：每次启动 reconcile OS 登录项到当前可执行文件路径，修复因更新/移动导致的静默失效

## 锁定决策

| 决策 | 理由 |
|------|------|
| iroh 作为唯一 P2P 传输 | QUIC NAT 穿越 + Ed25519 身份 + relay fallback，一栈解决连接问题 |
| 单 MasterKey 扁平加密 | v1 简化实现，代价是无法可靠撤销已泄露设备（需 DEK 信封分层，留待 v2） |
| 剪贴板语义为瞬时性 | 离线不重发、不排队、不最终一致——失败即报告，用户手动重发 |
| daemon per-profile 单例 | fs2 文件锁保证一个 profile 只有一个 daemon 实例 |
| AGPL-3.0-only 许可 | 任何修改后通过网络提供服务的实体必须开源对应源码 |
| 遥测事件名一旦上线永不重命名 | 防止历史数据聚合断裂，演进通过创建 *_v2 + 废弃旧事件 |
| Mobile 走独立 LAN HTTP 协议 | 移动端无法运行 iroh full node，SyncClipboard v3 协议足够轻量 |

## 绝对禁区

- **禁止中心化存储剪贴板内容**：不建 relay blob store、不建 store-and-forward 邮箱、不建云同步
- **禁止自动重发/最终一致性**：离线 = 预期状态，自动恢复是协作工具语义，与本项目定位冲突
- **禁止用户账号体系（v1）**：Space 是本地密钥信任组，不是云账户；无登录、无注册、无中心认证
- **禁止 daemon 依赖 GUI 框架**：daemon 是纯后台服务，编译结果不可包含 Tauri/GTK/AppKit UI 组件
- **禁止 GUI 内嵌业务栈**：GUI 进程不打开 SQLite、不实例化 AppFacade、不运行 blob worker
- **禁止 release 二进制暴露 dev 路由**：daemon dev-token 端点 #[cfg(debug_assertions)] 硬门控，CLI dev 命令 feature-gate 隔离
- **禁止遥测关联真实身份**：analytics_device_id 不得从 DeviceId 派生，不得存入业务持久层，$geoip_disable 强制为 true
- **禁止无身份验证的 PID 信号**：任何 kill/signal 操作前必须 verify_pid_identity，防止信号误投
