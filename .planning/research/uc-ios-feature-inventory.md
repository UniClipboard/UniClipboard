# uc-ios 功能与行为清单（Rust 共享 crate 迁移基线）

> 来源：公开仓库 https://github.com/UniClipboard/uc-ios（shallow clone 于 2026-06-12，90 个 Swift 文件）。
> 用途：把 iOS app 原生业务逻辑迁移到 iOS+Android 共享 Rust crate，**验收底线 = 零回归**。本清单即「行为基线」，迁移后逐条对照打勾才算无回归。
> 阅读顺序：先看「迁移分层」决定边界，再看「字节级不变量」（最易回归），最后用各模块清单 + 回归 checklist 验收。

---

## 0. 目标架构（4 个 target + Shared 层）

| Target | 目录 | 职责 | 迁移取向 |
|---|---|---|---|
| 主 App | `UniClipboard/` | UI、SyncEngine 编排、剪贴板 I/O、生命周期 | 大部分**留原生**，编排逻辑可下沉 |
| 键盘扩展 | `UniClipboardKeyboard/` | 自定义键盘，把同步内容贴进任意 app | UI 留原生，同步逻辑共享 |
| 分享扩展 | `UniClipboardShare/` | 系统分享面板上传文本/图片/文件 | UI 留原生，上传逻辑共享 |
| App Intents | `UniClipboard/Intents/` + `Shortcuts/` | Siri/快捷指令/主屏快捷操作发送·接收 | 壳留原生，send/receive 逻辑共享 |
| **共享层** | `Shared/` (Network/Models/Cache) | **协议、加密、编解码、配置、缓存** | **核心 Rust 共享候选** |

---

## 1. 迁移分层：Rust 共享 vs 留原生

### ✅ Rust 共享 crate 候选（纯逻辑，无 UIKit/SwiftUI 耦合）

| 模块 | 文件 | 说明 |
|---|---|---|
| connect-uri 解析 | `Shared/Network/ConnectURI.swift` | `uniclipboard://connect` 配对负载解析，有跨语言 golden vector |
| SyncClipboard HTTP 客户端 | `Shared/Network/SyncClipboardClient.swift` | §2 全部端点；Basic Auth；重试；取消 |
| 连通性探测 | `Shared/Network/ConnectionTester.swift` | 单 URL test + 多 URL 并发 probe（§5.3 Layer 2） |
| 历史查询参数+multipart | `Shared/Network/HistoryQuery.swift`、`MultipartBody.swift` | §2.7 分页/增量；RFC 7578 字节级 |
| 错误模型 | `Shared/Network/SyncError.swift` | HTTP/URLError → 语义错误 |
| 剪贴板线模型+哈希+发布 | `Shared/Models/Clipboard.swift` | §3/§4，SHA-256、长文本溢出阈值、发布助手 |
| 历史记录线模型 | `Shared/Models/HistoryRecord.swift` | §3.6，乐观锁版本、软删除命名陷阱 |
| 网络上下文+URL 分类 | `Shared/Models/NetworkContext.swift`、`ServerConfig.swift`、`ServerConfigList.swift` | §5.1–5.3 SSID 归一、LAN/TS/WAN 分类、自动切换排序 |
| 配置+设置模型 | `Shared/Models/AppSettings.swift` | §5.4 字段+默认值，前向兼容 |
| 持久化层 | `Shared/Models/SettingsStore.swift` | §5.5 键名（iOS/Android 共用导入导出）、watermark、SyncLoopGuard 状态 |
| 循环守卫 | `Shared/Models/SyncLoopGuard.swift` | apply↔push 振荡检测状态机 |
| 内容寻址缓存 | `Shared/Cache/PayloadCache.swift` | LRU 字节缓存（逻辑可共享，文件 I/O 需平台桥） |
| 名称/头像生成 | `ServerNameGenerator.swift`、`ServerAvatar.swift` | 确定性派生（FNV-1a hue） |

### 🚫 留原生（平台 API / UI / 系统集成，不进 Rust）

- 所有 `UniClipboard/Views/**`（Home、Settings、Onboarding、Setup、QRScanner 等 SwiftUI）
- 剪贴板 I/O：`DevicePasteboardObserver`、`PastedItemExtractor`（UIPasteboard、"允许粘贴" 提示、changeCount）
- 网络感知：`CurrentSSIDProvider`（NWPathMonitor、NEHotspotNetwork、CLLocationManager 授权）
- 键盘/分享扩展的 UI 与系统钩子（UIInputViewController、NSExtensionContext、textDocumentProxy）
- App Intents 壳、Siri 短语、Sharing Suggestions 捐赠（INSendMessageIntent、INPerson）、主屏快捷操作（AppDelegate）
- App Group 容器、Keychain、entitlements、缩略图 ImageIO 解码
- URL 元数据/OG 抓取、Sentry 引导

> ⚠️ 边界判断：凡是「需要操作系统 API 或渲染 UI」的留原生；凡是「给定输入算出确定输出（字节/JSON/哈希/排序）」的进 Rust。`SettingsStore`/`PayloadCache` 跨界——逻辑进 Rust，文件读写经平台桥接。

---

## 2. 🔴 字节级不变量（跨 iOS/Rust/桌面 daemon 必须逐字节一致——最易回归）

| 不变量 | 位置 | 关键细节 |
|---|---|---|
| Base64url-no-pad | ConnectURI | `-`↔`+`、`_`↔`/`，解码前补 `(4-len%4)%4` 个 `=` |
| Multipart CRLF | MultipartBody | 一律 `\r\n`(0x0D0A)，永不用 `\n`；边界、头/体分隔、结束符 |
| Multipart quoted | MultipartBody | `\`→`\\`、`"`→`\"`，丢弃 CR/LF（RFC 7578 §4.2） |
| SHA-256 | Clipboard §4.1/4.2 | **大写** hex；文本 hash=utf8(text)，文件/图片 hash=原始字节（文件名不参与） |
| 长文本阈值 | Clipboard §3.4 | **10240 字符**（`String.count` 字素，非字节）；溢出：预览 inline + 全文存 `text_{HASH}.txt` |
| ISO-8601 日期 | HistoryQuery/HistoryRecord | 含小数秒，`Z` 与 `+00:00` 两种都要能读 |
| composite profileId | HistoryRecord | `"<type>-<hash>"`（大写），用于 §2.8/§2.11 路径 |
| split id | §2.10 PATCH | `<type>/<hash>` 分段，**不同于** composite——别搞混 |
| `isDeleted` vs `isDelete` | HistoryRecord | 读/创建用 `isDeleted`，**PATCH body 用 `isDelete`（无 d）**，发错静默忽略 |
| Basic Auth | §1.2 | `base64(utf8(user + ":" + pwd))`，冒号分隔，UTF-8 |
| URL 分类网段 | §5.1 | LAN: 10/8、172.16/12、192.168/16、169.254/16；Tailscale: 100.64.0.0/10 |
| connect-uri golden vector | ConnectURITests | 跨 Rust/TS/iOS 字节相等；**错误信息文案也是契约**，改动需三端同步 |
| JSON 省略 nil | Clipboard §3.1 | `hash`/`dataName`/`size` 为 nil 时**整字段省略**，不写 `null` |

---

## 3. 同步编排行为（SyncEngine——回归高发区）

- **状态机**：`.idle/.succeeded`、`.hasNewUnwritten`（server 有新内容但 auto-apply 关）、`.offlineRetrying`、`.authFailed`、`.loopDetected`
- **tick 频率**：前台 1.0 Hz；inactive（控制中心/来电）5.0s；后台暂停；离线指数退避 5s→60s + ±20% jitter；历史同步节流 30s
- **每 tick 逻辑**：① 剪贴板观测（auto-push 开=读内容可能弹窗，关=仅读 changeCount 免提示）② GET §2.1（404=空，继续 push）③ server-wins 冲突（hash != lastSynced 时：auto-apply 开→取字节验 §4.4 hash 写入；关→暂存进 `.hasNewUnwritten`）④ push §2.2（仅当 server hash==synced 且 device hash 新）⑤ 历史 §2.7（detached、节流、冷启仅取 page 1 播种 watermark，增量用 `modifiedAfter`）
- **网络变更**：取消在途请求 → 清退避+清 `lastAppliedContentHash` → nil liveURL → reconcile 有效 server（Wi-Fi 自动切换）→ 重新 probe
- **去重守卫**：`lastSyncedContentHash`（防重复 pull）、`lastAppliedContentHash`（防刚写入内容被 push）、history 同 hash 去重并升级 direction
- **网络 epoch**：路径变更自增；probe 结论仅在 epoch 未变时有效，否则整体丢弃
- **loop guard**：同 hash apply/push 翻转 ≥3 次（30s 窗口）→ trip → `.loopDetected` → 用户确认 banner 后清空恢复

---

## 4. 剪贴板行为（DevicePasteboardObserver）

- **两级访问**：免提示层（changeCount + has*）vs 内容层（read 可能弹 "允许粘贴"）
- **echo 守卫**：`lastWriteChangeCount`、`lastWrittenContentHash`、`lastAppliedContentHash`、`lastConsumedChangeCount`
- **内容类型**：图片 PNG>HEIC>JPEG>GIF（用 `data(forPasteboardType:)` 保 §4.2 hash，不经 UIImage）；文本；URL
- **consent-push（默认）** vs **auto-push（可选）**：默认走 Home PasteButton 免提示；开 auto-push 才 tick 读剪贴板（弹窗）
- **激活门控**：`activate()` 推迟首次真实读，避免冷启弹窗
- **env 钩子**（截图/调试）：`UC_DEVICE_TEXT`、`UC_DEVICE_IMAGE`、`UC_DEVICE_IMAGE_EXT`

---

## 5. 设置项（AppSettings §5.4）

`trustInsecureCert`(false)、**`autoApplyServerChanges`(true ✅ 已核对 `AppSettings.swift:92,112`)**、`autoPushDeviceChanges`(false)、`prefetchAttachments`(true)、`prefetchOnCellular`(false)、`payloadCacheMaxBytes`(200MB)、`appearance`(system)、`downloadRelativePath`、`autoCheckUpdate`(true)、`ignoredVersion`、`keyboardSoundFeedback`(true)、`keyboardHapticFeedback`(true)、`onboardingShown`(false)、`enhancementsPromptShown`(false)、`pastePermissionHintDismissed`(false)。前向兼容：缺失填默认，未知键容忍，未知 appearance 回落 system。

> ✅ 已核对：`autoApplyServerChanges` 默认 **true**（自动写入本机剪贴板），`autoPushDeviceChanges` 默认 **false**（不自动读取上传）。之前 agent 报的 false 是误读，已订正。

---

## 6. 多服务器与自动切换（§5.1–5.3）

- `ServerConfigList{configs, activeConfigId}`；`activeConfig` 解析（stale id 回落 configs[0]）
- `ServerConfig.urls` 候选列表；`classifyURL` 纯主机形态分类（`*.ts.net`→TS、`*.local`→LAN、RFC1918→LAN、100.64/10→TS、其余→WAN）
- 两层排序：Layer 1 形态排序（无 I/O，稳定排序）；Layer 2 reachability probe（仅主 app，2s 超时并发，404/401=可达）
- live URL 跨进程缓存（`live_urls` JSON 文件）；网络变更/前台/切换 server 时强制重探
- 旧格式迁移：legacy 单 config、`manualOverrideConfigId` 一次性提升为 `activeConfigId`

---

## 7. 生命周期

- **启动**：load servers/settings/history/watermark → 初始化 pasteboard observer（推迟读）→ SSID provider → SyncEngine → 升级守卫（有 server 但 onboarding 未完则补标记）→ 发布 SSID 到 App Group
- **scenePhase**：`.active`（合并扩展历史、refresh SSID、强制重探、恢复 1Hz）；`.inactive`（节流 5s，保活）；`.background`（stop）
- **冷启分支**：`configs.isEmpty`→SetupFlow；`configs.isEmpty && !onboardingShown`→Onboarding；老用户跳过直达 home
- **env 钩子**：`UC_SETUP_STEP`、`UC_PREFILL*`、`UC_ONBOARDING`、`UC_ONBOARDING_ENHANCE`、`UC_FRESH`

---

## 8. 扩展与系统集成（留原生壳，逻辑共享）

### 键盘扩展
门控 `.ok/.needsFullAccess/.noServer`；上行（读 pasteboard→上传，watermark 先写）+ 下行（GET 最新→入历史）；卡片 text/link/image（文件/group 过滤）；图片走 ImageIO 缩略图（~48MB 预算）；文本卡 insertText 直插，图片卡复制到 pasteboard 提示长按粘贴；changeCount ~1.2s 轮询；NWPathMonitor 自动切换；行内服务器切换；空格/回车（按 returnKeyType 变标签）/退格 hold 重复/地球键；声音+触感（受设置门控）；**需 Full Access**（RequestsOpenAccess）。

### 分享扩展
接受 URL/文本/图片/文件（优先级 url>text>image>file）；上传序 §3.5（先 PUT 文件后 metadata，watermark 在中间）；>1 server 显示 picker；Sharing Suggestions tile（INSendMessageIntent recipient=server.id）pre-fill 直达上传；捐赠 + 写历史。

### App Intents / Shortcuts
`SendClipboardIntent`（server?/text?/file? 参数，优先级 file>text>pasteboard，openAppWhenRun=false，捐赠）；`ReceiveClipboardIntent`（server?/copyToDevice 默认 true，hash 校验，仅 copyToDevice 时写 watermark）；`ServerEntity`/`ServerEntityQuery`（App Group 读 + §5.3 解析）；`UniClipboardAppShortcuts`（Siri 短语，自动注册）。Siri 短语含中英文，必须带 `.applicationName` 占位。

### 主屏快捷操作
`ShortcutAction{push, pull}`（raw value 稳定不可改）；`ShortcutInbox` 单例桥接；`AppDelegate` 冷启/运行时两条路径 → ContentView 排空 → `runShortcut`（走原生 push/pull，不复用 Intent 路径）。

### App Group 共享容器
suite `group.app.uniclipboard.UniClipboard`。UserDefaults：serverConfigList、appSettings、keyboardExtensionEnabled/FullAccess、lastSyncedChangeCount、appearanceMode、keyboard 反馈偏好、keyboard_history_v1。文件（原子写跨进程）：`last_synced_hash`、`last_known_ssid`、`live_urls`。PayloadCache：`ImageData/<hash>.dat`、`payloads/`（200MB LRU）。

---

## 9. 已发布功能时间线（CHANGELOG，防止代码漏读）

- Build 10 (2026-06-11) 多 URL per-server 自动切换
- Build 9 (2026-06-10) Onboarding + home 重设计、本地历史、富链接卡/图片卡
- Build 8 (2026-06-08) 自定义键盘扩展、声音/触感、参数化 send/receive 快捷指令
- Build 7 (2026-06-01) consent-based push（PasteButton 免提示）、统一 current server、Tailscale
- Build 6 (2026-05-25) 主屏 pin、明暗主题、文件 watermark（绕 cfprefsd 延迟）、changeCount 轮询

---

## 9b. 桌面端 Rust 复用评估（2026-06-12 核对）

> 关键认知：daemon 是 SyncClipboard 协议**服务端**，手机是**客户端**。可复用的是「双方共享的纯逻辑」，不是服务端 handler。可复用代码现都在 `crates/uc-application/src/usecases/mobile_sync/`（**但该 crate 背着 uc-core+uc-infra 重依赖，复用前需抽到叶子 crate**）。

| 能力 | 现有 Rust | 复用判定 | 位置 |
|---|---|---|---|
| connect-uri 编解码 | ✅ 有，**含跨语言 golden vector** | **可直接复用** | `mobile_sync/connect_uri.rs`（`build_/parse_mobile_sync_connect_uri`、`ConnectPayload`、`ConnectUriError`） |
| SyncClipboard 线模型 DTO | ✅ 有 | **可直接复用** | `mobile_sync/clipboard_doc.rs`（`SyncClipboardMeta`、`SyncClipboardItemType`） |
| SHA-256 大写 hex | ✅ 有（~3 行纯函数） | **可直接复用** | `mobile_sync/sync_clipboard_mapping.rs`（`sha256_hex_upper`、`profile_hash_for_sync`） |
| LAN IPv4 分类（RFC1918+100.64/10） | ✅ 有 | **逻辑可复用**（接口枚举不可移植） | `mobile_sync/list_lan_interfaces.rs`（`is_lan_candidate`） |
| SyncClipboard 服务端 endpoint/multipart | ✅ 有 | **仅服务端**，客户端不复用 handler | `crates/uc-webserver/src/mobile_lan/routes/` |
| **AEAD/加密** | ❌ **mobile-sync 路径无应用层加密** | **不存在** | 加密栈在 `uc-infra/security`，仅服务于 P2P/blob，**不经 mobile-sync** |
| SSID/Wi-Fi 检测、多 URL probe 编排 | ❌ Rust 无（桌面用 network-interface） | **需新写**（且本就该留原生平台 API） | — |
| UniFFI / mobile crate 脚手架 | ❌ 完全不存在 | **从零搭** | — |

### 🔴 会改变此前判断的发现：mobile-sync 是明文（HTTP Basic Auth，TLS 可选）

之前我把「字节级一致的 AEAD 加密」当作共享 Rust 的最强理由——**对 mobile-sync 阶段不成立**。mobile-sync 协议**没有应用层加密**：内容以明文走 HTTP，仅靠 Basic Auth 认证设备身份，传输加密交给部署层 TLS。`uc-infra/security` 的 XChaCha20/MasterKey 只服务 P2P 与本地 blob，**不在 mobile-sync 链路上**。

含义：
- **mobile-sync 试水的共享价值落在 connect-uri + DTO + hash + LAN 分类**——这些 Rust **已经写好且有 golden vector**，复用是实打实的（不必从零）。
- **「加密字节兼容」这个最强理由要留到 P2P 阶段**才兑现（届时 AEAD/MasterKey 必须三端一致，且 iroh 是 Rust-only）。这进一步印证：试水的真正目的是铺 FFI 管道，而非省 mobile-sync 这点代码。
- 别在 mobile-sync 阶段引入加密栈——会无谓拖重 mobile crate。

### 复用的现实约束
可复用代码现居 `uc-application`（依赖 uc-core+uc-infra，太重）。要给 mobile crate 用，必须先把 `connect_uri.rs`/`clipboard_doc.rs`/`sha256_hex_upper`/`is_lan_candidate` **抽到一个 leaf crate**（如 `uc-mobile-proto`），桌面 daemon 与 mobile crate 共依赖之。这与 VISION「薄中间层隔离重依赖」一致，是迁移的第一块实在工作量。

---

## 10. 待办：迁移前必须钉死的点

1. **核对 `autoApplyServerChanges` 默认值**（调研报告 true/false 冲突）——读源码定死。
2. **加密/线格式必须用真实桌面 daemon 跑端到端**，单测自洽不够（字节兼容是 #1 回归风险）。
3. **`isDelete`/`isDeleted` 命名陷阱**封装成 helper，防调用点写错。
4. **PayloadCache/SettingsStore 的文件 I/O 边界**：逻辑进 Rust，读写经平台桥（App Group 容器路径由原生注入）。
5. **golden vector 测试**（connect-uri、multipart、hash）作为跨语言契约移植进 Rust 测试。
6. 把本清单逐条转成**可勾选验收表**（见各模块 + §8 扩展 checklist），替换后全绿才算无回归。
