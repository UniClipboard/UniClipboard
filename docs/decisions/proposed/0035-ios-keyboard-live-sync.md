# 0035 · iOS 键盘扩展实时同步：自动捕获外发与激活期接收

- 状态：已采纳，待实现
- 日期：2026-11-27
- 关联：[0003 独立 Engine](0003-engine-sdk.md)、[0004 剪贴板流水线 + 插件](0004-clipboard-pipeline-lua.md)、[0018 独立构建与发布](0018-independent-product-build-and-release.md)、[0031 移动端集成 Engine](0031-mobile-engine-integration.md)
- 落地目标：[仓库布局与依赖边界](../../architecture/overview.md)、[Engine 公共契约](../../contracts/engine-contract.md)、[本机历史契约](../../contracts/history-contract.md)、[发布架构](../../architecture/releases.md)
- 前置探针回执：[0035 iOS 键盘实时同步探针回执](../../exec/0035-ios-keyboard-live-sync-probe.md)

## 背景与目标

现有 iOS 自定义键盘扩展（`app/expo/targets/keyboard`）为**离线只读快照**架构：它不链接 Go Engine，仅读取主 App 写入 App Group 的 `keyboard-bridge/snapshot.json` 与缩略图；主 App 退到后台后会挂起（Suspended），键盘无法在后台唤醒主 App 执行网络同步。

本次目标是让 iOS 键盘扩展在**激活期间**具备完整的实时双向剪贴板同步能力（聚焦内置同步后端 `builtin.tailcat`）：

1. **自动读取系统剪贴板并外发**：键盘在已获「允许完全访问」且自动同步开关开启（**默认开启**）时，唤起及停留期间自动检测 `UIPasteboard.general` 中的新增**文本与图片**（在密码/安全输入框中不暂停），落入共享本地历史并自动发送给已信任在线设备；若收起时对端离线或尚未发完，只需保证已写入本地历史、下次打开主 App 或键盘可见（不追求退键盘后后台保活重传）。
2. **激活期间实时接收并自动写剪贴板**：键盘打开期间保持与已配对设备的 `builtin.tailcat` 会话活跃；收到其他设备推送的文本或图片后，**实时刷新键盘卡片列表**，并**自动写入 iOS 系统剪贴板**（`UIPasteboard.general`），同时严格防回环。

### 对 0031 既有约束的修订

[0031](0031-mobile-engine-integration.md) 原先基于对扩展 15–50 MB 内存上限和单进程独占锁的担忧，规定“严禁在 iOS App Extension 内启动第二个 Engine 实例”。前置真机探针（[探针回执](../../exec/0035-ios-keyboard-live-sync-probe.md)）在 iPhone 16 Pro（iOS 27.3）实测证明：
- 通过共享动态框架封装与 `GOGC`/`GOMEMLIMIT` 内存预算，键盘扩展内运行完整 Go Engine + `builtin.tailcat` 收发文本及图片时，`phys_footprint` 峰值为 18–23.6 MB（实测 Jetsam 上限 177 MB，对 48–60 MB 老机型亦留有安全余量）；
- 主 App 退后台关闭 Engine 释放 `flock` 仅需 7–13 ms，键盘随后接管共享 `DataDir` 零 `0xdead10cc` 与锁冲突。

因此本决策修订 0031 的 iOS 扩展边界：**改为由主 App 与 `.Keyboard` 扩展分时互斥持有同一 App Group `DataDir` 上的单一 Engine 实例**；`.Share` 扩展保持不链接 Go Engine。

---

## 核心设计

### 1. 共享动态框架封装（`UnicCoreRuntime.framework`）

`gomobile bind` 当前产出的 `UnicCore.xcframework` 为静态归档（arm64 约 54 MB）。若宿主 App 与 `.Keyboard` 扩展分别静态链接，安装包体积将翻倍且无法共享代码页。

- **单一动态框架**：构建流程将 `UnicCore.xcframework` 按目标切片（`ios-arm64`、`ios-arm64_x86_64-simulator`）以 `-fapplication-extension -dynamiclib -Wl,-force_load` 封装为共享动态框架 `UnicCoreRuntime.framework`（约 45 MB，零 UIKit 依赖，通过 App Extension API 校验）。
- **嵌入与链接方式**：
  - 仅嵌入一份至 `App.app/Frameworks/UnicCoreRuntime.framework` 并随宿主签名；
  - 主 App（`UnicEngine` Pod）使用 `@executable_path/Frameworks` 动态链接；
  - `.Keyboard` 扩展通过 Expo Config Plugin / target 配置使用 `@executable_path/../../Frameworks` 动态链接，扩展自身可执行文件保持在 ~200–300 KB 级；
  - `.Share` 扩展不链接该框架。
- **发布与签名**：延续 [0018](0018-independent-product-build-and-release.md) 的开发/商店双套配置，CI 与 Store 打包流程确保 `UnicCoreRuntime.framework` 使用对应分发证书签名并仅保留目标架构。

### 2. 共享 `DataDir`、存量迁移与双进程分时互斥协议

#### 2.1 存储布局

将 iOS Engine 数据与缓存目录迁入已有的共享 App Group 容器（`group.<bundleID>.incoming-share`，位于系统允许访问的标准子目录下）：

```text
group.<bundleID>.incoming-share/
├── Library/
│   ├── Application Support/
│   │   └── unic-v2/                  # 共享 Engine DataDir（unic.db, unic.lock, 身份与 tailcat 凭据）
│   └── Caches/
│       └── unic-cache/               # 共享图片预览与临时导出缓存
├── keyboard-bridge/
│   ├── snapshot.json                 # 快速首屏 & 主 App 前台双活降级快照
│   ├── preferences.json              # 键盘偏好（按键声音、触感、自动同步开关）
│   ├── pasteboard-state.json         # 跨进程持久化防回环状态
│   ├── heartbeat.json                # 完全访问心跳
│   ├── actions/                      # 降级态待主 App 消费的轻量操作（如置顶）
│   ├── thumbnails/
│   └── images/
└── incoming-share/                   # .Share 扩展暂存目录（不变）
```

- 目录及关键文件统一设置 `FileProtectionType.completeUntilFirstUserAuthentication`，保证设备首次解锁后扩展可读写。
- 保留 `UNIC_MOBILE_DATA_DIR` / `UNIC_MOBILE_CACHE_DIR` 环境变量与启动参数覆写，确保自动化测试与隔离验证不受影响。

#### 2.2 存量用户一次性平滑迁移

为保护已安装 v2 主 App 的存量用户（数据此前位于主 App 私有 `Application Support/unic-v2/`）：

1. 主 App 在打开 Setup/Engine 之前，在独占串行队列中执行一次性目录迁移检测：
   - 若旧私有路径存在 `unic.db` 且新 App Group 路径不存在 `unic.db`：将旧目录原子移入（同卷 `moveItem`；跨卷时拷贝至临时目录、校验关键文件完整后再原子替换），并写入迁移完成标记；
   - 若新路径已存在 `unic.db`：直接使用新路径，绝不覆盖；
   - 若迁移失败或 App Group 不可用：记录诊断日志并回退保留旧路径，避免数据损坏。
2. 迁移必须在 `Engine.New` 与 `MobileOpenSetupFlow` 之前完成，且不触碰历史 v1 备份目录或 `group.<bundleID>` 只读旧版移动端容器。

#### 2.3 主 App 与键盘扩展的分时互斥生命周期（规避 `0xdead10cc`）

iOS watchdog 会强制终止在后台挂起（Suspended）状态下仍持有 App Group 共享文件锁或 SQLite 锁的进程（异常码 `0xdead10cc`）。因此必须严格执行以下交接协议：

| 事件 | 主 App 行为 | 键盘扩展行为 |
|---|---|---|
| **主 App 退到后台** (`OnAppEntersBackground`) | 申请 `UIApplication.beginBackgroundTask`；立即刷写一次 `snapshot.json`；调用 `eng.close(nil)` 释放 `unic.lock`、SQLite 与 Tailcat 连接（实测 7–13 ms）；结束 background task | 随后唤起时直接独占获取 `unic.lock`，无锁冲突 |
| **主 App 回到前台** (`OnAppBecomesActive`) | 若此前处于 live 状态，自动重开 Engine（遇 `CodeInUse` 时短退避重试，例如 `50ms × 20` 上限 ~1.5 s 等待键盘退出释放）；重开成功后发射 `onEngineResync("reopened")` | 键盘在切回主 App 时触发 `viewWillDisappear`，在 12–52 ms 内 `close(nil)` 释放锁 |
| **主 App 前台双活**（如设置页试打输入框） | 主 App 持续持有 Engine 与 `unic.lock`，正常负责剪贴板捕获与同步 | 键盘 `NewEngine` 短重试（`50ms × 4`）仍得 `CodeInUse` 后，**平滑降级为快照/桥接模式**（读取 `snapshot.json`，停止键盘侧剪贴板同步，置顶写 `actions/`，无报错 UI） |
| **键盘唤起** (`viewWillAppear` / `viewDidAppear`) | （挂起状态，未持锁） | 先加载 `snapshot.json` 秒开首屏（< 20 ms），再在后台队列打开 Engine、启动 `builtin.tailcat`、激活剪贴板观察与实时接收 |
| **键盘收起** (`viewWillDisappear`) | — | 停止剪贴板观察，刷写最新 `snapshot.json`，关闭 Engine 释放 `unic.lock` 与网络连接，清理内存缓存 |

- **JS 状态层同步修正**：`app/expo/src/engine/store.ts` 中事件过滤必须读取最新的 `get().snapshot?.instance_id`（而非初始化闭包捕获的旧 `instance_id`），否则主 App 后台重开新 Engine 实例后会静默丢弃后续事件；同时 `UnicEngineModule.swift` 的 `requireEngine()` 在前台恢复窗口内由串行 `engineQueue` 排在自动重开之后，避免竞态 `invalid_state`。

#### 2.4 键盘扩展内存预算与运行时调优

为兼容旧机型（48–60 MB Jetsam 上限）及连续多轮开关稳定性：

1. **`bindings/mobile` 新增内存调优入口**：暴露 `ConfigureRuntimeMemory(gcPercent int, memLimitMB int)` 与 `FreeOSMemory()`（封装 `debug.SetGCPercent` / `debug.SetMemoryLimit` / `debug.FreeOSMemory`）。键盘扩展启动前固定调用 `ConfigureRuntimeMemory(25, 18)`；主 App 保持默认 GC 策略。
2. **键盘精简插件装配模式**：`engineConfigDTO` 新增可选布尔字段 `extension_mode`（默认 `false`）。当 `extension_mode: true` 时，`bindings/mobile` 仍装配 `builtin.tailcat`、`builtin.classify`、`builtin.text`，但**跳过异步链接预览富化器 `builtin.preview`**，避免键盘激活期间因抓取网页元数据/OG 图增加额外内存与网络请求。
3. **内存告警响应**：键盘 `didReceiveMemoryWarning()` 立即清空图片/缩略图内存缓存并调用 `MobileFreeOSMemory()`。

---

### 3. 自动读取系统剪贴板、跨进程防回环与建连补发

#### 3.1 触发策略与门控

- **生效条件**：`hasFullAccess == true` ∧ `preferences.autoSyncEnabled == true`（**默认开启**）∧ 键盘当前持有 Engine（未处于主 App 前台降级态）。
- **密码/安全输入框**：按产品需求**不暂停**自动读取与同步。
- **检测时机**：
  - `viewDidAppear` 且 Engine 就绪后立即检测一次当前 `UIPasteboard.general`；
  - 键盘激活停留期间，同时监听 `UIPasteboard.changedNotification` 与轻量轮询 `UIPasteboard.general.changeCount`（间隔 600 ms，仅键盘可见期间活跃）；
  - 轮询仅读取 `changeCount`（零系统粘贴弹窗），仅在 `changeCount` 变化且未命中防回环记录时，才实际读取正文或图片。

#### 3.2 支持的内容类型与扩展内存护栏

1. **文本与链接**：
   - 当 `pb.hasStrings` 为真时读取 `pb.string`；
   - 忽略纯空白与 `unic-pair:` 配对票据；
   - 文本大小上限与 Engine 常规限制对齐（单条 ≤ 4 MiB），通过 `importText(text, html: "", obsKey: "ios-kb:\(changeCount)")` 导入。
2. **图片**：
   - 当无文本且 `pb.hasImages` 为真时，优先读取原始图片表示（避免无谓重编码）；若通过 `UIImage` 获取，则检查像素尺寸与体积：
     - 最大边超过 **2048 px** 时使用 `UIGraphicsImageRenderer`（`scale = 1`）等比缩放，防止在键盘扩展中解码超大照片（如 12MP/48MP 原图）触发 OOM；
     - 编码后单张上限 **4 MiB**（超限时自动降级缩放或跳过并提示，不撑爆内存）；
   - 调用 `importBytes("image/png", fileName: "", data: png)`——**`fileName` 必须为空字符串**，确保存为 `kind = "representation"` 剪贴板图片而非文件附件。

#### 3.3 跨进程持久化防回环（`pasteboard-state.json`）

探针证实，仅在控制器内存保存 `lastAppliedChangeCount` 会在键盘收起重建后失效，导致键盘把自己刚刚写入剪贴板的远端数据再次捕获回发。

- 在 `keyboard-bridge/pasteboard-state.json` 中持久化共享防回环状态（原子写入）：
  ```json
  {
    "change_count": 90,
    "content_sha256": "<hex>",
    "origin": "applied",
    "updated_at_ms": 1795732870524
  }
  ```
- **统一读写方**：主 App（`applyReceivedToClipboard`、`applyHistoryToClipboard`、`copyPlainText`、`observeCurrentPasteboardChange`）与键盘扩展（自动捕获、自动写远端剪贴板、用户点按图片卡片复制）共同维护同一份状态：
  - 任何一方写入 `UIPasteboard.general` 后，立即记录新的 `change_count`、`content_sha256`、`origin = "applied"`；
  - 任何一方准备自动捕获前，若 `currentChangeCount == state.change_count` 则直接跳过（零粘贴弹窗）；若 `changeCount` 变化（如系统重启计数归零或跨源复制）则读取后比对 `content_sha256`（短时间窗内与最近 `applied` 一致也跳过回发）。

#### 3.4 冷建连窗口期（2–5 s）待发补发队列

[Engine 发送契约](../../contracts/engine-contract.md) 与 `engine/send.go` 明确“不对离线设备排队补发”。而真机实测表明键盘在 `T ≈ 20 ms` 即可完成剪贴板捕获，但 `builtin.tailcat` 冷建连需要 **2.3–5.2 s**。

- 捕获到新内容后立即调用 `CommitContent`：
  - 立刻落盘共享 SQLite 历史，键盘列表在 < 30 ms 内显示该条目；
  - 解析返回的 `sync_disposition`：
    - 若为 `"queued"`（已有在线信任设备），正常发送完成；
    - 若为 `"no_targets"`（设备仍在建连中），将该 `history_id` 放入**本次键盘激活会话的内存待发队列**（有界容量如 10 条，记录已尝试补发的 `device_id` 集合）；
- 监听 `EventDevices`：每次检测到信任设备绑定变为 `connected` 时，对队列中尚未发送给该设备的 `history_id` 调用 `ResendHistoryItem(history_id, [newlyConnectedDeviceIDs])`；
- 键盘收起时丢弃内存待发队列，不持久化离线重试队列——内容已安全保存在共享历史中，满足需求预期。

---

### 4. 激活期间实时接收、列表刷新与自动写剪贴板

1. **实时列表更新**：
   - 键盘持有 Engine 期间，直接从本进程 Engine 查询最近 50 条 + 置顶历史并驱动卡片列表（复用现有 OTP / URL / 图片分类与卡片 UI）；
   - 收到 `EventHistory` / `EventReceived` / `EventDevices` 时做轻量防抖刷新（~100 ms），左侧栏同步指示灯准确反映 `connecting` / `connected` / `syncing` / `offline` 状态。
2. **自动写入系统剪贴板**：
   - 收到 `EventReceived` 后，对本次激活期新出现的 `receipt_id`（忽略启动前已存在的历史未读回执，防止每次打开键盘都把旧数据覆盖掉用户当前剪贴板！）执行标准 `BeginApply` → 写 `UIPasteboard.general` → 更新 `pasteboard-state.json` → `ReportApplyResult`：
     - **文本**：优先读取 `text/plain` representation（≤ 4 MiB）写入 `UIPasteboard.general.string`；
     - **图片**：优先读取 `kind == "representation"` 且 `mime` 以 `image/` 开头的部分（兼容单图文件回退场景），导出至临时文件并校验字节上限（≤ 8 MiB）后解码写入 `UIPasteboard.general.image`，写完立即删除临时文件；
   - 键盘内弹出轻量非阻塞提示（如「已同步来自 <设备名> 的文本/图片」），同时顶部出现该新卡片供直接点按插入或预览。
3. **点击卡片行为保持兼容**：
   - 点按文本/验证码/链接卡片：直接 `textDocumentProxy.insertText(...)`（不改剪贴板）；
   - 点按图片卡片：写入 `UIPasteboard.general.image` 前同步更新 `pasteboard-state.json`，确保不会被自动捕获逻辑误判为新复制内容。

---

### 5. 设置项、iOS 粘贴权限引导与隐私文案

1. **设置页开关**（`app/expo/src/screens/settings/ios/KeyboardPage.tsx`）：
   - `KeyboardPreferences` 新增 `auto_sync_enabled`（默认 `true`），持久化在 `keyboard-bridge/preferences.json`，通过 `getKeyboardExtensionStatus` / `setKeyboardPreferences` 读写。
   - 在「设置 › 键盘」新增「自动读取并同步剪贴板」开关（默认开启）及简短说明。
2. **iOS 16+「从其他 App 粘贴」权限引导**：
   - 真机探针证实，首次在其他 App 唤起键盘读取系统剪贴板时会弹出 iOS 系统粘贴确认框。
   - 在「设置 › 键盘」增加独立引导提示：告知用户可在「系统设置 › App › Unic › 从其他 App 粘贴」中选择「允许」，即可免去每次弹窗（附一键跳转系统设置按钮）。
3. **隐私说明与四语文案更新**：
   - 更新 `app/expo/targets/keyboard/KeyboardTheme.swift` 及 `app/expo/src/i18n/locales/{zh,en,ru,pt-BR}/settingsIos.json`：
     - 将此前「仅读取已同步内容」调整为如实说明：启用自动同步时，键盘仅在打开期间读取当前系统剪贴板并在你的设备间同步，**不记录任何外部按键输入**，并可随时在 App 设置中关闭。

---

## 备选方案与取舍

| 备选方案 | 评估与结论 |
|---|---|
| **A. 主 App 与键盘扩展双份静态链接 `UnicCore.xcframework`** | 否决。arm64 二进制多出 ~45 MB 重复体积，且无法共享只读代码页内存。 |
| **B. 使用 `NEPacketTunnelProvider` 网络扩展后台常驻同步** | 否决。需向用户申请系统 VPN 配置权限、审核心理门槛高、耗电大，且本需求明确限定为“键盘激活期间收发”，无需后台常驻。 |
| **C. 在键盘扩展用纯 Swift 重写精简 Tailcat/WireGuard/协议栈** | 否决。违背 [0003](0003-engine-sdk.md) 统一协议、设备信任审批与历史模型原则，长期维护成本与一致性风险极高。 |

---

## 分阶段实施与验收门控

具体执行进度与真机回执记录在 `docs/exec/`：

1. **Phase 1（Go 绑定与动态框架打包）**：
   - `bindings/mobile` 增加 `ConfigureRuntimeMemory`、`FreeOSMemory` 与 `extension_mode` 装配选项，补齐 Go 单测与 race 校验；
   - 更新 `bindings/scripts/build-mobile.sh` 与 Expo iOS Config Plugin / Podspec，产出并正确嵌入签名 `UnicCoreRuntime.framework`，双端（App + `.Keyboard`）动态链接；验证 `npm run verify` 与模拟器/真机编译通过。
2. **Phase 2（App Group 共享 `DataDir`、存量迁移与主 App 锁交接）**：
   - `UnicEngineModule.swift` 实现私有目录到 App Group 的一次性安全迁移、`OnAppEntersBackground` 在 `beginBackgroundTask` 内关闭 Engine 释放锁、`OnAppBecomesActive` 自动重开并发射 `onEngineResync("reopened")`；
   - 修复 `app/expo/src/engine/store.ts` 的 `instance_id` 事件过滤；
   - 实现跨进程共享 `pasteboard-state.json` 防回环状态模块。
3. **Phase 3（键盘扩展实时 Engine 接入）**：
   - 在 `app/expo/targets/keyboard` 接入 `UnicCoreRuntime.framework`：首屏快照秒开 + 后台启动 Engine（`ConfigureRuntimeMemory(25, 18)` + `extension_mode: true`）、主 App 前台 `in_use` 降级态；
   - 接入自动剪贴板读取（文本 + 缩放/限额图片）、建连待发队列补发、激活期实时接收刷列表 + 自动写 `UIPasteboard.general`、左侧同步状态指示与关闭释放锁。
4. **Phase 4（设置页、权限引导与多语言文案）**：
   - 扩展 `KeyboardPreferences` / `KeyboardBridge` / `KeyboardPage.tsx` 的 `auto_sync_enabled`（默认开启）开关、iOS「从其他 App 粘贴」引导与四语隐私文案。
5. **Phase 5（真机端到端验收）**：
   - 在物理 iPhone 真机 + Mac/桌面端完成全链路实测：
     - 存量升级迁移无丢数据；
     - 主 App 退后台 → 第三方 App 唤起键盘自动捕获文本/图片发到桌面（含冷建连补发）；
     - 键盘激活期间桌面复制文本/图片 → 键盘卡片实时刷新并自动写入系统剪贴板；
     - 连续多轮开关键盘（验证跨会话防回环不重复外发、内存稳定无警告、无 `0xdead10cc`）；
     - 主 App 前台试打输入框双活降级正常；
     - 关闭「自动读取并同步剪贴板」开关后停止自动读剪贴板但仍可手动浏览/接收。

## 范围边界（本阶段不做）

- 键盘收起后的 iOS 后台保活重传或后台静默监听剪贴板；
- 非 `builtin.tailcat` 的外部同步后端（如 WebDAV / SyncClipboard）；
- 普通文件或文件夹自动写入 iOS 系统剪贴板；
- Android 自定义输入法（IME）扩展。
