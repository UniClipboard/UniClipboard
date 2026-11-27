# 0031 · 移动端集成 Engine：进程内绑定、Expo 原生宿主与迁移接入

- Status: Engine bindings, native hosts, the TypeScript adapter and Setup are implemented. See the [integration receipt](../../exec/0031-mobile-engine-integration.md), [iPhone physical-device receipt](../../exec/0031-mobile-m2-receipt.md#ios-physical-device-acceptance) and [release preparation receipt](../../exec/0031-mobile-m5-receipt.md). Two-device physical synchronization and Store rollout remain open. Note: the iOS Keyboard Extension no-Engine restriction in §3 is revised by [0035](0035-ios-keyboard-live-sync.md) (shared dynamic framework + time-sliced App Group `DataDir` ownership).
- 日期：2026-11-23
- 关联：[0003 独立 Engine](0003-engine-sdk.md)、[0011 v1 历史迁移](0011-v1-history-migration.md)、[0018 独立构建与发布](0018-independent-product-build-and-release.md)、[0026 桌面首次安装引导与迁移](0026-first-install-setup.md)
- 落地目标：[仓库布局与依赖边界](../../architecture/overview.md)、[Engine 公共契约](../../contracts/engine-contract.md)、[本机历史契约](../../contracts/history-contract.md)
- 交互原型：[iOS 首次安装与迁移原型](../../../prototype/setup-ios.html)、[Android 首次安装与迁移原型](../../../prototype/setup-android.html)

## 背景与目标

桌面端已完成 [0003](0003-engine-sdk.md) 落地与无头服务/图形客户端拆分（`unic` / `unicd` / `unic-desktop`），业务能力全部收敛在根模块公开包 `github.com/uniclipboard/unic/engine` 中。移动端已完成统一的 Expo 57 展示层迁移（[app/expo](../../../app/expo/README.md)，包含 iOS Liquid Glass 与 Android Material 3 Expressive 原生组件），但当前完全使用内存 demo 状态，未接入任何 Engine、数据库、网络同步或旧版迁移能力。

移动端无法照搬桌面的 `unicd` 独立后台进程 + 本地 socket/HTTP 架构：iOS 禁止应用启动独立后台守护进程，Android 后台服务也受系统电源与生命周期策略严格限制；同时，移动端必须完整复用 Go 核心的身份、配对审批、会话传输、本机历史、流水线、容量清理与 v1 只读迁移语义，避免维护第二套业务实现。

本决策确定移动端（iOS 与 Android）集成 Go Engine 的分层架构、跨语言绑定契约、Expo 原生宿主职责、v1 迁移接入方案、前置技术探针与分阶段验收门控。

## 决策

### 1. 进程内嵌入与四层单向架构

移动端采用**进程内（in-process）嵌入**方式运行 Go Engine，不拉起本地 HTTP 服务。模块与依赖保持严格单向：

```text
app/expo/src/ (TypeScript UI / Zustand Store / Engine TS 适配层)
        │  Expo Local Module (AsyncFunction + EventEmitter)
        ▼
app/expo/modules/unic-engine/ (Swift / Kotlin 原生宿主集成层)
        │  gomobile bind 产物 (UnicCore.xcframework / unic-engine.aar)
        ▼
bindings/mobile/ (嵌套 Go module，薄绑定层：句柄 / 取消 / 事件 / 流)
        │
        ▼
根 Go 模块：engine/ + internal/plugins/tailcat + internal/plugins/builtintext + internal/v1migration
```

- **仓库与模块约束**：延续 [仓库布局与依赖边界](../../architecture/overview.md) 的既有约定，`bindings/` 作为独立嵌套 Go module（`bindings/go.mod`，本地 `replace github.com/uniclipboard/unic => ..`）存在，仓库根目录不保留常驻 `go.work`；使用独立版本标签 `bindings/vX.Y.Z`。根模块的 `go test -race ./...` 与 `engine/boundary_test.go` 不受移动端工具链影响。
- **单向依赖边界**：根模块 `engine/`、`internal/`、`plugin/` 不得依赖 `bindings/` 或 `app/expo/`。`app/ios` 与 `app/android` 继续仅保留平台说明文档，不建独立工程。

### 2. `bindings/mobile` 薄绑定层设计

[0003](0003-engine-sdk.md) 已明确 Go 的 `context.Context`、channel、泛型、`uint64` 与任意 `io.Reader` 不直接跨语言导出。`bindings/mobile` 仅做薄适配，不承载业务规则或第二套状态机：

1. **显式装配内置能力**：在 `bindings/mobile` 创建 Engine 时显式装配 `builtin.tailcat` 同步后端与 `builtin.text` 内置处理器（按需装配受控 Lua 沙箱）。移动端**不支持外部独立进程 Go 插件（`pluginproc`）**，对外能力视图如实反映该限制。
2. **对象句柄化**：将 `*engine.Engine`、`LeaseID`、`*engine.ContentReader`、`*engine.Subscription` 封装为不透明句柄，统一提供幂等 `Close` / `Release`；不暴露内部实现类型。
3. **显式取消令牌**：将阻塞调用的 `context.Context` 映射为 `CancelToken`（支持原生层随时取消），沿用 [Engine 公共契约](../../contracts/engine-contract.md) 语义：取消只终止等待或直接 I/O，不回滚已提交的信任决定、导入资源或已落盘记录。
4. **类型化 DTO 与事件泵**：跨语言结构化参数与返回值使用固定版本的 JSON 编码（`uint64` 序号一律编码为十进制字符串防 JS 精度丢失）；Go 侧后台 goroutine 消费 `Subscription.Next(ctx)` 并通过回调接口 `EventSink` 推向原生层。队列溢出时发出 `resync_required`，由宿主调用 `GetState` 与历史分页重新对齐快照。事件、普通状态与错误严格脱敏，不含设备地址、配对票据、剪贴板正文或绝对路径。
5. **大文件流式与引用桥接**：禁止把文件内容整体编码进 JSON 或一次性跨边界拷贝为大 `[]byte`。
   - **读取内容**：`ReadContent` 暴露分块读取句柄（`ReadChunk(maxBytes)`）或受控导出到宿主提供的安全临时文件句柄，避免移动端 OOM。
   - **导入引用文件**：由原生宿主实现跨边界 `FileSource` 回调接口（打开流、返回声明大小与变更版本），不把系统路径或 `content://` URI 写入公共业务 DTO；传输前若源文件变动或消失，按契约返回 `source_changed`。
6. **统一错误模型**：所有失败统一映射为 `{code, message, retryable}`，严格沿用 [Engine 公共契约](../../contracts/engine-contract.md) 定义的稳定错误码。

### 3. Expo 原生宿主模块（`app/expo/modules/unic-engine`）

`app/expo` 使用 Expo CNG（`expo prebuild` 生成并忽略 `ios/`、`android/`），原生集成必须以 **Expo Local Module**（`expo-modules-core`）及配套 Config Plugin 形式交付：iOS 通过 Podspec `vendored_frameworks` 链接 `UnicCore.xcframework`（与 `UnicEngine` Pod 同名冲突隔离），Android 通过 Gradle 引入 `unic-engine.aar`（解包 `classes.jar` 与 `jni/`，`minSdk 24`）。

平台宿主系统集成全部归属原生层，绝不下沉进 Go core，也不依赖 JS 轮询兜底：

| 宿主职责 | iOS 集成边界 | Android 集成边界 |
|---|---|---|
| **数据目录与独占锁** | 主 App 私有 `Application Support`（若未来与扩展交换文件，仅将待投递暂存目录放 App Group；**严禁在 App Extension 内启动第二个 Engine 实例**，避免突破 15–50 MB 扩展内存上限及破坏单进程独占锁） | 主进程私有 `context.filesDir`；`DataDir` 持有进程级独占锁 |
| **前后台运行态** | 监听 `UIApplication` 前后台通知，后台挂起前调用 `SetRuntimeState(suspended)`，回前台调用 `SetRuntimeState(active)` | 监听 `ProcessLifecycleOwner`（及显式启用前台服务时的服务生命周期）切换 `active / suspended` |
| **网络变化通知** | 使用 `NWPathMonitor` 监听网络路径切换，触发 `NotifyNetworkChanged()` | 使用 `ConnectivityManager.NetworkCallback` 监听网络变更，触发 `NotifyNetworkChanged()` |
| **系统剪贴板与防回环** | 封装 `UIPasteboard`，严格走 `ObserveClipboard` / `BeginApply` / `ReportApplyResult`（apply token 防回环）；通过 `UpdateHostCapabilities` 如实上报前台访问状态，不静默触发系统粘贴授权弹窗 | 封装 `ClipboardManager`，走同一防回环协议；Android 10+ 后台无焦点不可读剪贴板时如实上报 `unavailable` / `permission_required`，不伪造成功 |
| **文件与媒体 URI** | 将 `UIDocumentPicker` / 系统分享拿到的 security-scoped URL 在 Swift 侧包装为 `FileSource`，管理访问作用域生命周期 | 将 SAF `content://` URI 在 Kotlin 侧通过 `ContentResolver` 包装为 `FileSource` |
| **线程与并发** | 所有 Go 阻塞调用在 Swift 后台并发队列执行；事件回调切回安全线程并通过 Expo `sendEvent` 派发，绝不阻塞主线程 | 所有 Go 阻塞调用在 Kotlin `Dispatchers.IO` 执行；事件通过 Expo `sendEvent` 派发，绝不阻塞主线程 |

### 4. TypeScript 业务适配层（替换 `src/shell` Demo 状态）

- 在 `app/expo/src/` 建立独立的 Engine 客户端模块，提供与 [Engine 公共契约](../../contracts/engine-contract.md)、[本机历史契约](../../contracts/history-contract.md) 与 [历史搜索契约](../../contracts/history-search-contract.md) 一致的类型化接口，逐步替换当前 `app/expo/src/shell/store.ts` 的内存演示数据与不可用提示。
- **状态驱动与恢复**：订阅 Expo Module 事件驱动界面刷新；应用回前台或收到 `resync_required` 时重新拉取状态快照与当前视口历史分页。
- **租约生命周期**：列表项仅持元数据；打开详情、大图预览或导出/分享时显式调用 `RetainHistoryContent`，离开页面或回收视口时必须调用 `ReleaseContent`，禁止泄漏内容租约。
- **生产数据边界**：接入真实 Engine 后，生产路径不再回落到演示数据或模拟成功提示。

### 5. 移动端首次安装 Setup 与 v1 迁移接入

移动端首次安装与旧版历史迁移沿用 [0026](0026-first-install-setup.md) 与 [0011](0011-v1-history-migration.md) 的全部业务语义（三段逻辑页：欢迎 → 迁移 → 结果；后台只读检测不触达剪贴板与凭据；显式确认后才解锁；导入与校验分阶段展示、不虚构剩余时间；支持安全停止、中断保留已提交记录、幂等续跑与五种终态区分），双端界面与交互分别遵循 [iOS 原型](../../../prototype/setup-ios.html) 与 [Android 原型](../../../prototype/setup-android.html)：

- **复用统一迁移实现**：`bindings/mobile` 暴露独立的 `MigrationSession` 封装根仓库 `internal/v1migration`（桌面 v1 加密备份）与 `internal/mobilemigration`（旧版移动端 Expo SQLite `uniclipboard.db` 历史与附件），提供检测、预检、分阶段进度回调与安全停止（`Stop`），按需在首次覆盖升级检测到旧版移动端数据库时自动离线迁移并校验。
- **启动门控与独占所有权**：
  - 首次安装或存在未完成 `setup` 标记时，在调用 `Engine.Start` 之前完成 Setup 门控；迁移期间不启动网络同步，不注册或观察系统剪贴板。
  - 已有非空 `unic.db` 且无未完成标记的存量 v2 安装直接进入历史，不重复弹 Setup。
  - 从「设置 › 存储 › 从 v1 迁移」发起后续迁移时，宿主须先 `Stop` 并 `Close` 当前 Engine 释放 `DataDir` 独占锁，持久化待迁移状态后再运行迁移，完成后重建并启动新 Engine。
- **平台数据源与凭据差异**：
  - **iOS**：后台默认检测本机覆盖升级沙盒 / App Group 中的 v1 数据，并允许通过原生 `UIDocumentPickerViewController`（文件 App）选择导出的 v1 备份目录/包；点击「开始迁移」后由 Swift 宿主读取 iOS Keychain（触发 Face ID / Touch ID 或设备密码）并将原始密钥字节传给迁移器，不可用或被拒绝时在当前流程内回落至旧口令输入；导入与校验期间临时启用屏幕常亮（`isIdleTimerDisabled`）防止锁屏挂起。
  - **Android**：后台默认检测应用私有目录中的 v1 数据，并允许通过 SAF（`ACTION_OPEN_DOCUMENT_TREE`）选择自定义或外部存储目录；点击「开始迁移」后由 Kotlin 宿主经 Android Keystore + `BiometricPrompt` 读取旧密钥，失败或被拒绝时在当前流程内回落至旧口令输入；导入与校验期间保持前台执行与屏幕常亮，系统返回手势触发 M3 `AlertDialog` 并走同一安全停止路径。
- **口令安全**：旧口令仅通过内存方法参数传递，不写入参数日志、持久化存储、崩溃报告或事件流。

## 前置技术风险探针（Phase 0 门控）

在冻结跨语言接口并大规模替换 `app/expo` 界面数据之前，必须先完成最小可运行探针并在 iOS 与 Android 真机（及模拟器）上验证以下三项高风险依赖（基于当前 [go.mod](../../../go.mod)）：

1. **SQLite 移动端兼容性**：验证当前纯 Go 驱动 `modernc.org/sqlite` 在 `ios/arm64`、`iossimulator`、`android/arm64` 下的交叉编译、WAL 读写、独占锁与崩溃恢复。若 `modernc.org/libc` 在任一平台存在不可接受的缺陷或构建失败，需在 Phase 0 决策是否按 build tag 在移动端切换为 CGO SQLite（如链接系统 `libsqlite3`）。
2. **`tailcat` / `tailscale.com` 移动端网络栈**：验证 Android 11+ 无 netlink 枚举权限以及 iOS 沙盒下 `builtin.tailcat` 的启动、直连与 DERP 中继连通性、Wi-Fi/蜂窝切换重连与前后台 `suspended ↔ active` 恢复；明确是否需由原生宿主注入网络接口回调。
3. **二进制体量与内存预算**：实测 `UnicCore.xcframework` 与 `unic-engine.aar` 对安装包体积、冷启动耗时与空载/大文件传输期 RSS 的影响，形成基线数据后再推进后续阶段（构建与包体记录见 [0031 执行回执](../../exec/0031-mobile-engine-integration.md)）。

## 备选方案与取舍

| 备选方案 | 评估与结论 |
|---|---|
| **A. 移动端跑本地 HTTP 守护进程（仿桌面 `unicd`）** | 否决。iOS 不允许随意拉起独立后台守护进程；即使在同进程起 loopback HTTP，也会引入额外端口绑定、本地鉴权、JSON 编解码与大文件二次拷贝开销，并破坏移动端挂起模型。 |
| **B. C ABI（`c-archive`/`c-shared`）+ React Native C++ TurboModule/JSI** | 暂不作为首版默认方案。`gomobile bind` 直接生成 Swift/ObjC 与 Kotlin/Java 习惯类型，与现有 Expo 57 Local Module 工具链契合度更高、维护成本更低。若 Phase 0 探针证实 `gomobile` 存在不可绕过的 Go 版本或构建链阻塞，再启动本备选方案评估。 |
| **C. 用 TypeScript / Swift / Kotlin 重写核心同步与历史** | 否决。违背 [0003](0003-engine-sdk.md) 统一协议、信任审批、防回环与历史模型的核心目标，长期维护成本与一致性风险过高。 |

## 分阶段实施与验收标准

进度与真机实测证据记录到 `docs/exec/`（不写进本决策）：

1. **Phase 0（编译与真机探针）**：建立最小 `bindings/mobile` 嵌套模块，产出 `.xcframework` 与 `.aar`；在 iOS 与 Android 真机验证 `New → Start → SQLite 写入 → Tailcat 与桌面连通 → SetRuntimeState(suspended/active) → Stop/Close`，并记录包体与内存数据。
2. **Phase 1（核心闭环）**：落地 `app/expo/modules/unic-engine` Expo Local Module 与 TS 适配层；双端打通生命周期管理、配对码与指纹审批、设备列表/撤销、前台文本显式发送、接收列表与手动写入系统剪贴板（含防回环）。
3. **Phase 2（历史、搜索与媒体文件）**：接入本机历史分页/筛选/搜索、标签展示与管理、图片预览与文件流式 `FileSource`（iOS security-scoped URL / Android SAF URI）导入导出及租约释放。
4. **Phase 3（v1 迁移 Setup 与系统入口）**：接入 `MigrationSession`、iOS Keychain/Face ID 与 Android Keystore/`BiometricPrompt`、首次安装门控与「设置 › 存储 › 从 v1 迁移」，对照 [iOS 原型](../../../prototype/setup-ios.html) 与 [Android 原型](../../../prototype/setup-android.html) 完成全状态验收；按需接入系统分享入口（主 App 独占 Engine，不于扩展中启动 Engine）。

验收规则：TypeScript 类型检查、`expo prebuild` 与 Metro/Hermes bundle 校验仅验证前端构建边界，不构成原生编译、Engine 集成或交互验收；每阶段验收须包含真实原生构建与模拟器/真机执行回执。

## 范围边界（本阶段不做）

- 鸿蒙（HarmonyOS NEXT）Native 桥接、工具链适配与发布流程。
- 移动端外部独立进程 Go 插件（`pluginproc`）与桌面端在线插件市场。
- 为绕过 iOS/Android 系统剪贴板或后台策略引入非常规权限依赖（如无障碍、ADB/Shizuku、悬浮窗保活不作为 Engine 契约前提）。
- 修改设备间 wire 协议，或新增远端应用 ACK、断点续传与离线可靠投递。
- 移动端商店签名、正式发布流水线与旧版包名替换策略（由 [0018](0018-independent-product-build-and-release.md) 另行承接）。
