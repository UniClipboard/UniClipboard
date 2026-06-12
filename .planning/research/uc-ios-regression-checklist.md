# uc-ios 零回归验收清单

> 配套 `uc-ios-feature-inventory.md`。用途：Rust crate 替换原生逻辑后，**逐条勾选**；全绿才算「无回归」达标。
> 验证手段图例：
> - 🧬 **golden** = 跨语言黄金向量单测（Rust 输出须与 iOS/桌面字节相等）
> - 🔬 **unit** = Rust 单元测试可覆盖
> - 🔗 **e2e** = 必须连**真实桌面 daemon** 跑端到端（字节兼容只能这样证）
> - 📱 **device** = 真机/模拟器手动验证（涉及系统 API/UI/扩展）
>
> 🔴 = 字节级关键项，错一字节即回归，**优先级最高**。

---

## A. 协议与编解码（Rust 共享核心 · 字节关键）

### A1. connect-uri
- [ ] 🧬🔴 解析 `uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url>`，golden vector 与 iOS/桌面字节相等
- [ ] 🧬🔴 base64url-no-pad：`-`↔`+`、`_`↔`/`，解码前补 `(4-len%4)%4` 个 `=`
- [ ] 🔬 required 字段缺失/空/null → `missingField`；非 http(s) → `invalidURL`；svc≠mobile-sync → `unsupportedService`；v≠1 → `unsupportedVersion`
- [ ] 🔬 `urls` 缺省时回落 `[url]`；`o` 中未知字符串键保留、非字符串值丢弃
- [ ] 🧬 错误码/文案与 spec §4.2 表一致（**文案是跨语言契约**）

### A2. SyncClipboard 线模型（Clipboard / HistoryRecord）
- [ ] 🧬🔴 `Clipboard` JSON 字段名：`type/hash/text/hasData/dataName/size`，nil 字段**整字段省略**（不写 null）
- [ ] 🔬 `type` 枚举原值 `Text/Image/File/Group`
- [ ] 🧬🔴 `HistoryRecord` composite id = `"<type>-<hash>"`（大写）
- [ ] 🔴 §2.10 PATCH 用 split id `<type>/<hash>`（**不同于** composite）
- [ ] 🔴 PATCH body 用 `isDelete`（无 d）；读/创建用 `isDeleted`——封装 helper 防写错
- [ ] 🔬 `hasData/starred/pinned/isDeleted` 无条件编码；`text` 仅非空时编码
- [ ] 🔬 ISO-8601 日期：能读 `Z` 与 `+00:00`、含/不含小数秒四种组合
- [ ] 🔬 version 生命周期：创建=0，每次 PATCH +1，stale 版本 server 返 409

### A3. 哈希
- [ ] 🧬🔴 SHA-256 **大写** hex；文本 hash = sha256(utf8(text))；文件/图片 hash = sha256(原始字节)，**文件名不参与**
- [ ] 🔬 hashMatches：expected 为 null/空 → 永真；否则大小写无关相等

### A4. 长文本溢出（§3.4）
- [ ] 🧬🔴 阈值 **10240 字符**（`String.count` 字素，**非字节**）
- [ ] 🔴 溢出：`text`=前 10240 字符预览，`hasData=true`，`dataName="text_{HASH}.txt"`，payload=全文 utf8，`size`=全文长度，hash over 全文
- [ ] 🔬 publishImage：`dataName="image.{ext}"`、`text=dataName`、hash=bytes
- [ ] 🔬 publishFile：文件名经 `sanitizedFilename`（剥 `/`、`\`，空回落 "file"）

### A5. multipart（§2.7 查询 / §2.9 创建）
- [ ] 🧬🔴 行终止符一律 `\r\n`，边界 `--{b}\r\n`、结束 `--{b}--\r\n`
- [ ] 🔴 quoted：`\`→`\\`、`"`→`\"`，丢弃 CR/LF
- [ ] 🔬 字段编码：page/types 十进制串，日期 ISO-8601，bool `"true"/"false"`；**nil 字段不发**
- [ ] 🔬 TypeMask 位：Text=1 Image=2 File=4 Group=8

### A6. HTTP 客户端
- [ ] 🔗🔴 Basic Auth = `base64(utf8(user + ":" + pwd))`
- [ ] 🔗 base URL 归一：trim、补尾 `/`、校验 http(s)+非空 host
- [ ] 🔗 端点：GET/PUT SyncClipboard.json、PUT/GET file/{name}、POST api/history/query、GET api/history/{profileId}/data
- [ ] 🔬 文件名校验：空/含 `/`/含 `\` → 网络前即拒
- [ ] 🔗 状态映射：200/201/204=成功，401=authFailed，404=notFound，5xx=serverError，其余 4xx=protocolError
- [ ] 🔬 重试：仅首次遇 `.networkConnectionLost`/`.timedOut`，sleep 300ms 重试一次；401/404 不重试
- [ ] 🔬 取消：`cancelInFlight` 后续请求立即抛 `.cancelled`

### A7. 连通性探测（§5.3 Layer 2）
- [ ] 🔬 单 URL test：200/404→success，401→authFailed，其余→unreachable
- [ ] 🔬 多 URL probe：2s 超时并发，404/401=可达，`waitsForConnectivity=false`
- [ ] 🔬 `firstReachable` 按 orderedURLs 顺序取首个可达（确定性，非竞速）

---

## B. 网络分类与多服务器（§5.1–5.3）

- [ ] 🔬🔴 URL 分类网段：LAN=10/8·172.16–31/12·192.168/16·169.254/16；TS=100.64.0.0/10；`*.ts.net`→TS；`*.local`→LAN；其余→WAN
- [ ] 🔬 SSID 归一：trim、剥外层引号、`<unknown ssid>`/`0x` → nil
- [ ] 🔬 Layer 1 形态排序确定性（无 I/O，稳定排序保留同类内发布序）
- [ ] 🔬 try-order：Wi-Fi=[lan,ts,wan]；非Wi-Fi+TS=[ts,wan,lan]；蜂窝=[wan,ts,lan]；无信号=保持原序
- [ ] 🔬 `activeConfig` 解析：stale id 回落 configs[0]；空列表→nil
- [ ] 🔬 `preferredURLs(live:)`：live 有效且在当前 urls → 提头；失效 → 忽略回落形态序
- [ ] 🔬 旧格式迁移：legacy 单 `url`、`manualOverrideConfigId` 一次性提升为 activeConfigId；不回写旧键

---

## C. 同步编排（SyncEngine · 行为关键）

- [ ] 🔗 server-wins：每 tick 先处理 server，再 device
- [ ] 🔗 auto-apply ON（默认）：server hash 新 → 取字节验 §4.4 hash → 写 pasteboard → 进 watermark
- [ ] 📱 auto-apply OFF：暂存 `.hasNewUnwritten`，不取字节，显 banner
- [ ] 🔗 push：仅当 server hash==synced 且 device hash 新于 `lastSyncedContentHash`/`lastAppliedContentHash`
- [ ] 🔬🔴 去重守卫三件套：`lastSyncedContentHash`（防重 pull）、`lastAppliedContentHash`（防刚写内容被 push）、history 同 hash 去重并升级 direction
- [ ] 🔗 历史增量：冷启仅取 page 1 播种 watermark；增量用 `modifiedAfter`（严格 `>`）分页至空数组
- [ ] 🔬 loop guard：同 hash apply/push 翻转 ≥3 次（30s 窗口）→ trip；reset 后恢复
- [ ] 🔬 网络 epoch：路径变更自增，probe 结论仅 epoch 未变时有效
- [ ] 📱 tick 频率：前台 1Hz、inactive 5s、后台暂停、离线退避 5→60s+±20% jitter、历史节流 30s
- [ ] 📱 网络变更：取消在途、清退避、清 lastApplied、nil liveURL、reconcile server、重 probe

---

## D. 剪贴板 I/O（留原生，但行为须不变）

- [ ] 📱 两级访问：免提示层（changeCount+has*）vs 内容层（可能弹"允许粘贴"）
- [ ] 📱🔴 图片优先级 PNG>HEIC>JPEG>GIF，用 `data(forPasteboardType:)` 保 §4.2 hash（不经 UIImage）
- [ ] 📱 echo 守卫：lastWriteChangeCount / lastWrittenContentHash / lastAppliedContentHash / lastConsumedChangeCount
- [ ] 📱 consent-push（默认，PasteButton 免提示）vs auto-push（opt-in，tick 读剪贴板弹窗）
- [ ] 📱 `activate()` 推迟首次真实读，冷启不弹窗

---

## E. 设置项（§5.4）

- [ ] 🔬 默认值：`autoApplyServerChanges=true`、`autoPushDeviceChanges=false`、`trustInsecureCert=false`、`prefetchAttachments=true`、`prefetchOnCellular=false`、`payloadCacheMaxBytes=200MB`、`appearance=system`、键盘音/触感=true
- [ ] 🔬 前向兼容：缺失键填默认、未知键容忍、未知 appearance 回落 system
- [ ] 📱 各 toggle 实际行为：trustInsecureCert 影响 TLS 校验；autoApply 门控写入分支；autoPush 门控读剪贴板路径；prefetch* 门控预取

---

## F. 持久化与跨进程（App Group）

- [ ] 🔬 持久化键名与桌面/Android 共用：`server_config_list`、`app_settings`、`clipboard_history` 等
- [ ] 🔗 文件原子写跨进程：`last_synced_hash`、`last_known_ssid`、`live_urls`（JSON map）
- [ ] 🔬 history 去重 append：同 hash 在头不重插、`.local` 升级为 pushed/pulled、cap 200、newest-first
- [ ] 🔬 watermark：`loadHistoryWatermark`/`saveHistoryWatermark`、节流时间戳
- [ ] 🔬 损坏策略：缺失/不可解码 blob 返默认，永不阻塞启动
- [ ] 📱 PayloadCache：LRU 按 mtime 驱逐、200MB cap、原子写、backup-excluded、并发 fetch 去重（semaphore=3）

---

## G. 生命周期

- [ ] 📱 启动：load servers/settings/history/watermark → pasteboard observer 推迟读 → SSID provider → engine → 升级守卫 → 发布 SSID
- [ ] 📱 scenePhase：active（合并扩展历史/refresh SSID/强制重探/恢复 1Hz）、inactive（节流保活）、background（stop）
- [ ] 📱 冷启分支：空配置→SetupFlow；空配置且未 onboard→Onboarding；老用户直达 home

---

## H. 主 App UI（留原生 · 表层回归）

- [ ] 📱 Home：两列网格 newest-first、搜索（文本/文件名）、类型/日期筛选、多选批量（复制/分享/删除）、下拉刷新、context menu、tap 重应用、长按预览
- [ ] 📱 Settings：服务器列表、各 toggle、缓存档位（50/200/500/1000MB）+ 清理、主题、功能引导回看
- [ ] 📱 服务器管理：增/删/改、多 URL（去重）、shuffle 名、测试连接（并发 probe 取首达）、QR 扫描、滑删+切换
- [ ] 📱 Setup/Onboarding：QR 或手填、测试连接 gate、首run 走查、post-pairing 解锁卡片
- [ ] 📱 ConnectImportSheet：掩码预览、追加为新服务器

---

## I. 键盘扩展

- [ ] 📱 门控 `.ok`/`.needsFullAccess`（去设置）/`.noServer`（去主程序加服务器）
- [ ] 📱 上行：读 pasteboard→上传，**watermark 先于 metadata PUT 写**，图片入 App Group
- [ ] 📱 下行：GET 最新→入历史去重
- [ ] 📱 卡片：text/link/image（file/group 过滤）；link 检测 http(s)+host；图片走 ImageIO 缩略图（~48MB 预算）
- [ ] 📱 动作：文本 insertText 直插；图片复制到 pasteboard + "已复制长按粘贴" toast；text 溢出先取文件验 hash 再插
- [ ] 📱 changeCount ~1.2s 轮询自动上行；NWPathMonitor 自动切换；行内服务器切换
- [ ] 📱 键盘：空格/回车（按 returnKeyType 变标签）/退格 hold 加速重复/地球键；音+触感受设置门控
- [ ] 📱 需 Full Access（RequestsOpenAccess），否则 URLSession/App Group/UIPasteboard 全失效

---

## J. 分享扩展

- [ ] 📱 接受 url>text>image>file（优先级）；file URL 检测图片扩展名
- [ ] 📱🔴 上传序 §3.5：先 PUT 文件后 metadata，watermark 在中间
- [ ] 📱 >1 server 显 picker；Sharing Suggestions tile（recipient=server.id）pre-fill 直达上传
- [ ] 📱 stale server tile → 提示已删除 + 显 picker；捐赠 + 写历史
- [ ] 📱 错误态：noInputItems/noUsableAttachment/loadFailed/上传错误对话框

---

## K. App Intents / Shortcuts / 主屏

- [ ] 📱 SendClipboardIntent：server?/text?/file? 参数，优先级 file>text>pasteboard，openAppWhenRun=false，捐赠，watermark 先写
- [ ] 📱 ReceiveClipboardIntent：server?/copyToDevice(默认 true)，hash 校验，**仅 copyToDevice 时写 watermark**
- [ ] 📱 ServerEntity 解析：App Group 读 + §5.3（live_urls + 网络上下文 + preferredURLs）
- [ ] 📱 Siri 短语中英文均含 `.applicationName` 占位，自动注册
- [ ] 📱 主屏快捷：`ShortcutAction{push,pull}` raw value 稳定；冷启/运行时两路径 → runShortcut（走原生 push/pull，非 Intent 路径）

---

## L. Sharing Suggestions 捐赠

- [ ] 📱 分享/自动同步成功 → `donateSend`（INSendMessageIntent，groupIdentifier=server.id）
- [ ] 📱 删服务器 → `deleteAllDonations(forServerId)` 移除该服务器全部捐赠
- [ ] 📱 ServerPersonFactory/ServerAvatarRenderer：handle=server.id、确定性 initials+hue（FNV-1a）

---

## 验收执行建议

1. **A 区（字节关键）先行**：把 iOS 现有 golden vector（connect-uri/multipart/hash）移植成 Rust 测试，A 区全绿是动 UI 的前置闸门。
2. **A6/A2/C 用真实桌面 daemon 跑 🔗 e2e**：单测自洽不足以证字节兼容。
3. **D–L 的 📱 项**在迁移收尾阶段真机过一遍；过渡期保留原生/Rust 双路径 feature-flag，回归可 A/B 定位来源。
4. 每条勾选附「验证者 / 日期 / 证据（测试名或截图）」，避免口头达标。
