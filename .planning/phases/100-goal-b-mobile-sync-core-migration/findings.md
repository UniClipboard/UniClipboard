# Findings & Decisions — 目标 B mobile-sync 迁移

> 外部记忆。权威细节在 commit message + `.planning/research/` + 代码 doc comments；本文存「接手必知」的浓缩。

## Requirements

<!-- 来自迁移方案 + 回归清单 -->
- 把 uc-ios `Shared/`（Network/Models/Cache）纯逻辑 + HTTP 客户端下沉到共享 Rust crate，iOS/Android 经 UniFFI 调用。
- UI / 剪贴板 I/O / SSID 探测 / 系统 API / 扩展壳 **永留原生**。
- 只做 mobile-sync（SyncClipboard 兼容 LAN HTTP，明文 + Basic Auth），**不做 P2P/iroh/加密栈**。
- 验收唯一标准 = `.planning/research/uc-ios-regression-checklist.md` 逐条全绿（A 字节关键 → L 捐赠）。
- 零回归：迁移期保留原生/Rust 双路径 feature-flag，A/B 定位来源；全绿后删原生路径。

## 纲领文档（接手按序读）

1. `.planning/research/uc-mobile-spike-plan.md` — spike B0–B2，FFI 管道 + runtime 执行模型 + 三个工程缝。
2. `.planning/research/uc-mobile-goal-b-migration-plan.md` — **主线方案**，M0–M6 + oracle 策略 + 风险表（M0–M3 已 ✅）。
3. `.planning/research/uc-ios-regression-checklist.md` — **验收唯一标准**，逐条勾选（A1–A7 + B 编解码已勾）。
- uc-ios 源码：`/tmp/uc-ios`（可能被清理，重 clone：`git clone --depth 1 https://github.com/UniClipboard/uc-ios /tmp/uc-ios`）。Swift 实现 + `Tests/` 是规范源。

## crate 拓扑

```
crates/
├── uc-mobile-proto   ← 纯编解码叶子（零内部依赖，仅 sha2/unicode-segmentation/chrono）。
│                       connect_uri/hash/clipboard_doc/history_record/multipart/net_class。140 测试。
│                       是 JSON/字节唯一真相。net_class 含 ordered_urls/preferred_urls/classify_url。
└── uc-mobile         ← UniFFI 边界。client.rs = async HTTP 客户端（M2 全端点）
                        + ConnectionTester（M3：test_connection/probe/first_reachable，ProbeResult/ProbeReport）。
                        53 测试。deps：proto + reqwest(ring rustls) + tokio(current_thread on 1 thread) + chrono。
```

## Research Findings（关键技术事实）

- **执行模型**：`MobileSyncClient` 在 1 条专用线程上跑 current_thread tokio runtime（iOS 扩展 ~48MB jetsam 预算排除多线程 runtime）。每个请求 `spawn` 到该 runtime，导出 fn 只 await JoinHandle（reactor-free，UniFFI 机器可 poll）。
- **缝 3（drop 语义）**：丢弃导出 future 不打断在途请求（detached task 跑完），只有 `cancel_in_flight` 显式 abort。put_clipboard 的 file→metadata 窗口因此对调用方 drop 原子。
- **reqwest 0.12.28**：`read_timeout`(idle) + `connect_timeout` + `timeout`(total)。M3 probe 用 total=timeout_ms 短超时；生产用 idle/connect=10s。`danger_accept_invalid_certs(true)` 与 rustls-tls 可用（trustInsecureCert）。reqwest 默认不等连通性（= Swift waitsForConnectivity=false）。
- **A7 状态语义 ≠ A6 主客户端**：probe/test 把 **404 当可达**（Success）、401=AuthFailed；A6 主客户端把 404 当 NotFound 错误。新 `ProbeResult` 枚举集中此语义。
- **map_status 字节级**：成功 = 严格 {200,201,204}；401→Unauthorized、404→NotFound、5xx→ServerError、其余（含 202/206/3xx/非401-404的4xx）→ProtocolError。
- **firstReachable 确定性**：按 ordered_urls 顺序取首个可达，**非竞速**（两个可达取排序靠前者）。无 results 条目的 URL 永不选中。
- **uniffi `=0.31.1` 钉死**（bindgen 与 runtime 同版本）。HashMap 可作 Record 字段/返回值；u64/枚举/Record 跨 FFI OK。iOS target 已装（aarch64-apple-ios + -sim），Xcode 26.2。

## Oracle 策略（字节兼容是 #1 回归风险）

| 端口 | oracle | 手段 |
|---|---|---|
| doc get/put、file get/put、Basic Auth | ✅ 真实 daemon | B2 编排脚本 `run-b2-daemon-demo.sh` |
| connect-uri/hash/溢出/multipart/HistoryRecord 编码 | ✅ iOS golden vector | M0 原样移植成 Rust 测试 |
| history version/409、PATCH isDelete、modifiedAfter | ⚠️ 二者皆不可靠 | 抓真机/官方 server 字节 fixture；daemon 兼容壳修复另开 issue |

## 已记录、未阻塞的遗留（另开 issue，不混进迁移线）

1. **单一真相收敛 #1b（daemon 侧）**：`uc-webserver/.../sync_doc.rs::SyncClipboardDoc` 改依赖 proto `Clipboard`。client 侧 M2 已收敛；daemon 侧因 PascalCase 别名 + size 恒序列化 + hash 不归一三处语义有回归风险，用户拍板独立 issue。
2. **新端点真实 daemon e2e**：M2 file/history 端点用 in-process mock 覆盖（字节兼容由 proto golden 锁）；补真机 e2e 扩 `run-b2-daemon-demo.sh`。
3. **A2 version 409 / PATCH**：服务端行为，Swift SyncClipboardClient 无 PATCH 方法——不属客户端范围。
4. **daemon history 兼容壳功能补全**（version/409/modifiedAfter）：服务端工程，另开 issue。

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| ProbeResult（Success/AuthFailed/Unreachable/MissingFields）+ ProbeReport{network_epoch,results} | 404→可达语义集中；epoch 盖戳供 M5 校验 |
| build_http_client 加 trust 参数 → danger_accept_invalid_certs | probe/test 各自构建客户端时接 trust（M3）；生产客户端 trust 留 M4 |
| 提取 get_latest_with（http,server）自由 fn | get_latest 与 test_connection 共用（后者可传 trust-override 客户端） |
| probe 用 tokio::task::JoinSet 单 runtime 线程并发扇出 | 不引入 futures crate；parent task abort → JoinSet drop → 子任务全 abort，取消自然传播 |
| dedup_preserving_order（Swift Array(Set(urls))） | 去重保留首见序；结果是 map，顺序不影响但稳定 |

## 工作方式偏好（用户记忆 + 本线观察）

- 对话用 **中文**，代码注释/commit/PR 用 **英文**（`.planning/` 豁免，可中文）。
- 契约/会与既定计划冲突的决策 **先问后做**；用 AskUserQuestion 批量问、给推荐项（M2/M3 验证有效）。
- 简单任务先做后报告；复杂任务走完整 recon→design→implement。
- 文档引用文件用反引号路径 + 行号，**不用 Markdown 链接**（项目 CLAUDE.md 硬要求，Zed）。

## M4 recon（2026-06-14，E/F 区状态与持久化）

### 规范源（uc-ios `/tmp/uc-ios`，已重在）
- `Shared/Models/AppSettings.swift` — `AppSettings`(Codable, 17 字段) + `AppearanceMode`(system/light/dark) + `PersistenceKey`(键名常量)。前向兼容靠手写 `init(from:)`：每字段 `decodeIfPresent ?? default`，未知 appearance→system，未知键 serde 默认容忍。`ignoredVersion` 用 `encodeIfPresent`(nil 省略)，其余恒编码。键全 camelCase（= Swift 字段名逐字）。
- `Shared/Models/ServerConfig.swift` — `ServerConfig`(id/name?/urls/username/password，encode 同时写 `url`==urls[0] **和** `urls`；decode urls 优先、回退 legacy 单 `url`；丢弃 `autoSwitchWifiNames`/`autoSwitchStrategy`)；`ServerConfigList`(configs/activeConfigId，**B 区迁移**：`manualOverrideConfigId` 可解析则提升为 activeConfigId、不回写旧键)；`LegacyServerConfig`(url/user/pwd → `migrated(idProvider)` 新 UUID 小写、置 active)。形态分类 (classifyURL/orderedURLs/preferredURLs/normalizeSSID) 已在 proto `net_class`(M1/M3)，本区只补 Codable + 迁移。
- `Shared/Models/ClipboardHistoryItem.swift` — id(UUID)/entry(Clipboard)/timestamp(Date)/direction(pulled/pushed/local)。**非 wire 协议，client 本地观测日志**。
- `Shared/Models/SyncLoopGuard.swift` — 纯值类型状态机：`record(dir,hash?,at)`(hash 大写归一、空/nil 忽略、record 时淘汰 window 外事件)、`tripped()`(按 hash 分组数方向翻转 flips≥threshold=3、window=30s、幂等)、`reset()`。**运行时态，不持久化**。
- `Shared/Cache/PayloadCache.swift` — actor。**M4 只迁驱逐决策**：给 {key,size,mtime} 列表 + maxBytes → 按 mtime 升序删到 ≤cap 的 key 列表；`isValidKey`(非空/无 `/`、`\`、非 `.`/`..`)。文件原子写/backup-excluded/semaphore=3/fetchAndStore 去重 **留原生**。
- `Shared/Models/SettingsStore.swift` — UserDefaults + App Group 文件壳。迁移后 **原生退化为字节搬运 + 文件 I/O**，纯逻辑下沉 Rust。
- 测试规范源：`Tests/.../SettingsStoreTests.swift`(35 例)、`SyncLoopGuardTests.swift`(8 例)、`PayloadCacheTests.swift`(LRU/setMaxBytes/invalidKey 决策相关数例)。

### 字节兼容约束（#1 风险）
- AppSettings / ServerConfigList / live_urls map：**纯 JSON，无日期** → serde `rename_all="camelCase"` + `#[serde(default)]` + 自定义 appearance 回落，**容易**。
- watermark / lastHistorySyncAt：UserDefaults 存 ISO-8601 字符串（fractional 优先、回退 plain）→ 复用 proto `parse/format_iso8601_utc`，**容易**。
- last_synced_hash：纯文本文件、大写 hex、trim。last_known_ssid：纯文本、`normalize_ssid`(复用 proto)。**容易**。
- **clipboard_history 是唯一难点**：Swift `JSONEncoder` 默认 `id:UUID`→大写字符串、`timestamp:Date`→**Double 秒-since-2001**(timeIntervalSinceReferenceDate, 2001-01-01 UTC, epoch 偏移 978307200)。Rust 需自定义 serde 才能与现存原生 blob 双向 round-trip。FFI 侧时间戳拟用 epoch-millis i64（同 M2 约定），serde 层转 Double-since-2001。

### M4 拆解（snapshot in → decision/bytes out；I/O 留原生）
- proto 新增纯模块：`app_settings`、`server_config`(+迁移)、`history_log`(append 去重 + cap + direction 升级 + blob 编解码)、`loop_guard`、`payload_cache`(plan_eviction + is_valid_key)。watermark 复用 iso8601。
- uc-mobile：FFI 镜像 Record/Enum + `#[uniffi::export]` 自由 fn（byte-in/out 或 transform）。proto 保持无 uniffi derive（既定）。
- trustInsecureCert 补生产客户端（M3 只接 probe/test）。
- history append 算法在 Swift 重复两处 (SettingsStore + AppViewModel)→ 迁 Rust = 单一真相收益。

### M4 三个待拍板（已问用户，见 task_plan Decisions）
1. 持久化所有权：Rust 拥有 blob 字节 (decode/encode，原生纯搬运) vs 原生留 Codable、Rust 只做内存 transform。
2. clipboard_history 字节忠实度：忠实匹配 Swift Date-since-2001/大写 UUID(迁移保留本地历史) vs 接受一次性历史重置。
3. trustInsecureCert：构造期固定 (+setter 换 reqwest client 不重启 runtime) vs per-call 传入。

## M5 recon（2026-06-14，C 区 SyncEngine 决策核）

### 规范源 `/tmp/uc-ios/UniClipboard/Sync/SyncEngine.swift`（968 行，已通读）
`@MainActor @Observable final class SyncEngine`。结构：
- **State 枚举**：idle/succeeded/hasNewUnwritten/offlineRetrying/authFailed/loopDetected。
- **UI 可观察态**：state/lastSyncedAt/lastError/stagedEntry/isExplicitlyRefreshing。
- **runtime 决策态（8+ 字段）**：`lastSyncedContentHash`(守卫 1，持久化)、`lastAppliedContentHash`(守卫 2)、`loopGuard:SyncLoopGuard`(M4 已迁)、`stagedServerHash`、`consecutiveFailures`(退避计数)、`nextNetworkAttemptAt`(退避门)、`lastHistorySyncAt`(历史节流，持久化)、`isTicking`/`isHistorySyncing`(并发锁)。
- **配置**：normalCadence=1.0、inactiveCadence=5.0、isSceneInactive、offlineBackoff=5.0、offlineBackoffMax=60.0、historySyncInterval=30.0、historySyncMaxPages=50、loopGuard window=30s/flip=3。

### tick(explicit) 主流程（决策↔I/O 交织——拆层关键难点）
1. isTicking 守卫 / explicit 等待在途 → **执行壳**（调度）。
2. pasteboard 观测（autoPush ON: poll+seedCache+appendHistory(.local)；OFF: pollDetection）→ **执行壳**（UIPasteboard），但「device.hash≠lastApplied && !isHashInRecentHistory → 记 local」是 **决策**。
3. activeServer guard / authFailed|loopDetected guard → **决策**（早退）。
4. 退避门 `!explicit && now<nextNetworkAttemptAt → return` → **决策**（需 now 入参）。
5. cross-process re-sync：读持久化 lastSyncedHash 比对刷新内存 → **决策**+I/O。
6. 构造 client + `getClipboard()` → **执行壳**（网络）。
7. **truth-gate**：serverHash==deviceHash → 已收敛，repair watermark+advanceSynced → **决策**。
8. else `serverHash≠lastSyncedContentHash` → `processServerNew` → **决策（路由）**。
9. else → `maybePush` → **决策**。
10. history sync（detached Task）→ isDue 节流是 **决策**，分页 walk 是 **执行壳**。
11. 错误处理：authFailed→stop、cancelled→no-op、SyncError→backoff+kickProbe、其他→capture → **决策**（状态转移 + 退避计算）。

### 各方法决策/执行分界
- **processServerNew**：dedup(alreadyStaged) → !staged 时 serverLatest/appendHistory(.pulled)/prefetch（执行壳）；autoApply&&hasHash → applyServerToDeviceThrowing（执行壳：写板 + 验 hash）成功后 advanceSynced+lastApplied+loopGuard.record(.pulled)+tripped；失败 park staged+throw；else 暂存 .hasNewUnwritten；alreadyStaged&&(off|hashless)→noop（不动 lastSyncedAt）。
- **maybePush**：!autoPush→consent 模式 noop succeeded；device==nil→succeeded(不动 lastSyncedAt)；device==synced→已同步；device==lastApplied→**防刚写被 push**；else pushReturningEntry（执行壳）成功后 advanceSynced+appendHistory(.pushed)+loopGuard.record(.pushed)+tripped+donation。
- **consentPush**：用户授权 push（PasteButton 字节在手）→ appendHistory(.local)+adopt → pushSnapshot 成功后 advanceSynced+lastApplied+updateHistoryDirection(.pushed)+loopGuard.record+tripped。
- **advanceSynced**：nil/empty 跳过；否则 uppercase+ 持久化（纯逻辑+I/O）。
- **runHistorySyncIfDue**：isDue 节流（last+interval）；cold-start(watermark==nil) 只取 page1 播种 watermark；增量用 modifiedAfter(严格>) 分页至空数组；maxModified 推进 watermark。
- **currentBackoffSeconds**：`2^min(failures-1,6)*base` capped at max，× jitter(0.8~1.2 random)。
- **cadenceSeconds**：authFailed/loopDetected→∞；else isSceneInactive?5:1。
- **tripLoopBreaker**：state=.loopDetected+stop（幂等）。
- **hashesEqual**：nil==nil、uppercase 比较。**isHashInRecentHistory**：history.first.hash==hash。

### 可纯函数化的决策清单（→ proto 单测覆盖）
1. **退避/节奏**：`backoff(failures,base,max,jitter) -> f64`、`cadence(state,inactive,n,i) -> f64`、退避门 `now < next`。
2. **去重守卫三件套（🔬🔴 核心）**：`hashes_equal`、push 前提 `device≠synced && device≠applied`、truth-gate `server==device`、server-new `server≠synced`、history 同 hash 去重升级 direction（M4 `history_log::append` 已有）。
3. **loop guard**：M4 `loop_guard::{record,tripped}` 已有；M5 接线（apply/push commit 时 record + tripped → trip）。
4. **网络 epoch 校验（🔬）**：`probe 结论仅 report.epoch==current_epoch 时有效`（M3 ProbeReport 盖戳，M5 校验）。
5. **processServerNew dedup**：`already_staged(stagedHash,stagedEntry,entry)`。
6. **history sync 决策**：`is_due(last,now,interval)`、`is_cold_start(watermark)`、watermark 推进 `max(wm, max_lm)`。

### 拆层难点（拍板抉择见 task_plan Key Questions M5）
迁移方案字面是 `decide(tick_input) -> Vec<SyncAction>`，但 tick 内 I/O 交织（getClipboard 结果决定路由 → apply/push 又是后续 I/O → I/O 后才 commit 守卫/loopGuard）。单次纯 decide 覆盖不了。需拆成「plan（路由）→ 原生 I/O → commit（转移守卫/loopGuard）」reducer 形态，且 8+ runtime 字段宜聚合成 `SyncRuntimeState` plain struct（caller 持有，同 loop_guard 风格但避免散装签名爆炸）。

### M5 交付边界（既定，迁移方案行 75/80-84）
- proto 实现纯决策逻辑 + 单测（同 M4，**不暴露 uniffi**，FFI 镜像延后 M6）。
- 执行壳（tick 调度/scenePhase/UIPasteboard/网络 cancel/banner/prefetch）留原生。
- 验收：C 区 🔬 条目以决策核单测覆盖；🔗 条目过 daemon e2e（留 M6/真机）；📱 留 M6。

## M6 recon（2026-06-15，uc-ios 接入与灰度）

### iOS repo 结构（`/Users/mark/MyProjects/iOSApp/UniClipboard`）
- **两个消费方**：① `Package.swift` 的 SwiftPM library/test target（`UniClipboardModels`/`Network`/`Cache`，path 直指 `Shared/{Models,Network,Cache}`）——只为 `swift test`；② Xcode app target——经 `PBXFileSystemSynchronizedRootGroup` **直接** 吸收 `Shared/` 文件，**不经 SwiftPM 包**。
- ⚠️ **关键约束**：`Shared/` 下的文件被两个消费方同时编译。往 `Shared/` 加 `import UniClipboardCore`（Rust binding 模块）会让 **app 构建在 Xcode 侧接线前就断**。所以 M6-0a 的 Rust 消费只放在 **测试目标**（`Tests/UniClipboardCoreTests`），不碰 `Shared/`。
- `SyncEngine.swift` 在 `UniClipboard/Sync/`（app target），**不在 `Shared/`** → `swift test` 覆盖不到，验证是 📱 级。
- `swift test` 跑在 macOS host（arm64-apple-macosx），用 Swift Testing(`@Test`/`#expect`) + XCTest 混合。Swift 6.2.3，Package tools-version 5.9（= Swift 5 语言模式，uniffi binding 不踩严格并发）。
- `ConnectURI.Payload`(native)：url/urls([url]回落)/user/pwd/other。Rust FFI `ConnectPayload`：v/url/urls(单候选为空)/user/pwd/other(`o`)。`parseConnectUri(uri:)` throws `ConnectUriError`(case PascalCase)。

### xcframework 交付（D2：脚本构建 + gitignore）
- 产物 **139M/3-slice 209M**（含 debuginfo，70M/slice）→ **不 check-in**。
- iOS `Scripts/build-rust-core.sh`：`UC_RUST_REPO`(默认 `~/MyProjects/uniclipboard`) → 跑 Rust 的 `build-ios-xcframework.sh` → 拷 xcframework + `uc_mobile.swift` 进 gitignored `RustCore/`。binding 也 gitignore（与 xcframework 由脚本同步再生，零漂移；FFI surface 在 Rust repo review，CI 跑脚本）。
- xcframework 现含 3 slice：ios-arm64 / ios-arm64-simulator / **macos-arm64**（host `swift test` 链接用；app 不发布 macOS，Xcode 按平台选 slice）。
- `Package.swift` 用 `FileManager.fileExists(RustCore/...xcframework)` **条件** 加 `binaryTarget`(UniClipboardCoreFFI) + `target`(UniClipboardCore，path RustCore，**exclude xcframework**，sources `uc_mobile.swift`) + test target。缺 RustCore 时 `swift test` 行为完全不变（Rust core opt-in）。
- 坑：UniClipboardCore target path=RustCore 会把 xcframework 的 .a 当散文件 → emit-module 失败；必须 `exclude: ["UniClipboardCore.xcframework"]`。

### connect-uri 防御式解析缺口（tracer-bullet 核心产出，已修）
- proto parse **信任 desktop encoder**（strict）；native Swift **防御式**（容忍手改/未来 QR）。3 处不符：
  1. `o` 非字符串值：serde `BTreeMap<String,String>` 遇数字/bool **整条 PayloadDecodeFailed**；native `if let s = v as? String` 静默丢弃。
  2. `urls` 不过滤非 http(s) 条目；native `filter(isHTTPURL)`。
  3. `urls` 不丢非字符串条目（serde 会报错）；native `compactMap(String)`。
- M0/M1 清单 A1 误标这些已 🔬 覆盖（golden vector 只测 well-formed，没测防御式 decode）。
- 修法（用户拍"修 proto 保零回归"）：proto `de_lenient_string_map`/`de_lenient_url_list`（`serde_json::Value` 入手，容忍 null/非对象/非数组，丢非字符串，urls 还 trim + http 过滤）。`[url]` 回落留 consumer/shim（FFI 契约返回过滤后列表，单候选为空）。encode 路径 + golden vector 不变。
- A/B harness 形态：`normalizedURLs(rust) == native.urls`（shim 补 [url] 回落）；错误用共享 `ConnectErrorKind` 分类比对（两 enum 字段不同，只比类别）。

### M6 拆解（当前进度）
- **M6-0a done**：管道打通 (xcframework→SPM→Swift→FFI) + connect-uri A/B 全平价 + proto 防御式修复。
- **M6-0b done**：Part 1=`MobileCoreFlags`(App Group A/B flag，默认 OFF)+`ConnectURIRouter`(canImport 守卫+Rust→native 类型/错误映射)+`AppViewModel` 用 router+Package.swift 条件依赖；Part 2(W1)=.pbxproj 加 XCLocalSwiftPackageReference "."+app 依赖 `UniClipboardCore` product。connect-uri 已在真 app 可灰度（默认 native）。⏳ 运行时翻转待模拟器+QR 验 (📱)。
- **M6-1+ 下一步**：逐模块 M2/M3(client，FFI 已暴露但需补 client router/A/B + 路由 `SyncClipboardClient` 调用点)→M4(持久化，需暴露 FFI)→M5(reducer，需暴露 FFI 镜像 Record/Enum + `#[uniffi::export]`)。每切过 📱/🔗 清单。
- 三进程 TLS 验收 + 删原生路径。

### Xcode 接线关键事实（W1，M6-0b Part 2）
- 项目 objectVersion 77，已有 sentry-cocoa(XCRemoteSwiftPackageReference) 可镜像 product-dependency 接法。
- W1 = app target `packageProductDependencies` 加 `UniClipboardCore`(来自 XCLocalSwiftPackageReference relativePath ".") + PBXBuildFile(productRef) 进 app Frameworks phase + PBXProject `packageReferences`。router 无需改码（canImport 一套）。
- **硬前置（已处理）**：`UniClipboardCore` product 仅 `hasRustCore`(RustCore 在盘) 时定义 → app 构建前必须先 `Scripts/build-rust-core.sh`，否则 SPM 解析失败。CI：`testflight.yml`（仅 tags:v* + 手动触发）已加 checkout Rust 仓库 (`rust_core_ref` 输入默认 main) + 加 iOS targets + build-rust-core.sh，置于 swift test + archive 前。本地：CLAUDE.md Commands 段已注明。⚠️ military-muscle 未并 main 前，dispatch 需把 rust_core_ref 设该分支；Rust 仓库私有则需 checkout PAT。
- **模拟器 slice 必须通用 (arm64+x86_64)**：generic/Release 编双架构，arm64-only 链接 x86_64 失败 → 脚本 lipo `aarch64-apple-ios-sim`+`x86_64-apple-ios`（需 `rustup target add x86_64-apple-ios`）。xcframework 现 3 slice：ios-arm64 / ios-arm64_x86_64-simulator / macos-arm64（280M，gitignored）。
- **🔴 canImport 陷阱（已修，M6-1+ 复用）**：`Shared/` 被 app + 两扩展同时编译，但只有 app 链接 core。`#if canImport(UniClipboardCore)` 在不链接 core 的 Share 扩展里 **可能为真**（模块在共享 build 目录可见，依 build 顺序）→ 扩展编译 Rust 分支 → undefined symbol 链接失败（flaky）。**改用 `#if UC_RUST_CORE`**：只在链接 core 的 target 上定义——app target `SWIFT_ACTIVE_COMPILATION_CONDITIONS`(Debug+Release，pbxproj) + SwiftPM `UniClipboardNetwork` 的 `.define`(hasRustCore 时)。`import UniClipboardCore` 同 `#if` 守卫。已写进 iOS CLAUDE.md。
- **真机 A/B 测试入口（M6-0b）**：`MobileCoreFlags`(默认 OFF) → SettingsView 诊断 section 有 `#if DEBUG` toggle "connect-uri 走 Rust 核心"；`ConnectURIRouter` 每次 parse 打 `log.notice`("via Rust core"/"via native Swift"，Console.app subsystem `app.uniclipboard`/category `network`；结果字节相同只能靠日志分辨)；应用内扫码 (QRScannerView/ServerQRPayload) 与 `.onOpenURL` 深链两条都已走 router。⚠️ DEBUG toggle 在 TestFlight(Release) 不显示。
- ⚠️ iOS 分支 `mobile-sync-rust-core` 本地 **7** commit，**未推送/未 PR**（用户未要求）。

## M6-1 recon（2026-06-15，M2/M3 client 切换）

### 原生 `SyncClipboardClient`（`Shared/Network/SyncClipboardClient.swift`，336 行）
- `final class ... @unchecked Sendable`，**每 server 一实例**：`init(server:ServerConfig, trustInsecureCert:Bool, session:URLSession?=nil) throws`（session 仅测试注入 MockURLProtocol）。
- 异步端点面：`getClipboard()->Clipboard`、`putClipboard(_:Clipboard)`、`getFile(name:)->Data`、`getHistoryPayload(profileId:)->Data`、`queryHistory(_:HistoryQuery)->[HistoryRecord]`、`putFile(name:body:)`。+ `cancelInFlight()`（path/profile 变时 §5.3 abort 在途 + **poison** 该实例，新请求抛 .cancelled）。+ static `normalizeBaseURL`/`basicAuthHeader`。
- **构造时机 = 每操作新建**：AppViewModel ~12 处方法各自 `let client = try SyncClipboardClient(...)`，无长生命周期单例。sync tick（active 时 1s 一次）也是每 tick 新建。

### Rust `MobileSyncClient`（`crates/uc-mobile/src/client.rs`，FFI 已暴露）
- `#[uniffi::constructor] new(bridge: Arc<dyn PlatformBridge>, trust_insecure_cert: bool) -> Arc<Self>`。**构造即 spawn 一条专用 runtime 线程**（`RuntimeHost::spawn`，drop 时 join 关闭）。
- **server 每方法传入**（无状态 re: server）：`get_latest(server)->ClipboardMeta`、`put_clipboard(server,meta,payload:Option<Vec<u8>>)`、`put_file(server,name,body)`、`get_file(server,name)->Vec<u8>`、`query_history(server,query)->Vec<HistoryRecord>`、`get_history_payload(server,profile_id)->Vec<u8>`。+ `cancel_in_flight()`（abort 全部在途，**不 poison**，2026-06-12 拍板）+ `set_trust_insecure_cert`(热切换)。
- **缝 2**：构造要 Swift 实现 `PlatformBridge { fn app_group_dir()->String }`（foreign trait via `with_foreign`）。
- **类型差异**（需 iOS 适配层）：Rust `ServerConfig{base_url,username,password}`/`ClipboardMeta`/`HistoryRecord`/`HistoryQuery`(Rust Records) ↔ 原生 `ServerConfig`/`Clipboard`/`HistoryRecord`/`HistoryQuery`(Swift)。

### M6-1 关键设计分叉（**比 connect-uri 复杂得多**）
1. **生命周期错配**：原生每操作新建 client（含 sync tick 每 1s）；Rust 构造即 spawn+join runtime 线程 → 每操作新建 Rust client = 每 tick spawn/join 线程（重）。Rust API 故意设计成 server-per-call 正是为支持长生命周期共享 client。→ 共享单例 (高效，cancel 语义变全局) vs 每操作新建 (对齐原生最简，但 tick 级线程开销)。
2. **路由机制**：工厂+protocol（调用点全改 `make(...)`，类型变 `any 协议`） vs `SyncClipboardClient` 内部路由（调用点零改，每方法 `#if UC_RUST_CORE` 分支，文件变脏）。
3. **A/B 验证手段断档**：原生 client 测试靠注入 `MockURLProtocol`；reqwest **不走 URLProtocol** → host `swift test` 无法字节级 A/B 平价。备选：(a) 信 Rust 侧 29 M2 测试 + golden vector 当 oracle，iOS 只 router + 真机/Console A/B（同 connect-uri）；(b) 起真 loopback HTTP server 双打对比（重）；(c) 真 daemon e2e。
4. **扩展约束（非选项，是硬约束）**：Share/Keyboard 扩展 + ConnectionTester 内部用 `SyncClipboardClient`；扩展 **不链接 core**（`UC_RUST_CORE` 仅 app target）→ 扩展恒留原生，路由层必须 `#if UC_RUST_CORE` 守卫优雅降级（同 ConnectURIRouter）。
5. **scope**：一次切全部 7 方法 × ~15 调用点（大 diff）vs tracer-bullet 先切读路径（getClipboard）验证再铺开（对齐既定谨慎节奏）。

### M6-1 step 1 实现（iOS `188b991`，2026-06-15）
- **新文件**（`Shared/Network/`，被 SwiftPM `UniClipboardNetwork` + Xcode app 双吸收）：
  - `SyncClipboardClienting.swift`：协议 (getClipboard/putClipboard/getFile/putFile/queryHistory/getHistoryPayload/cancelInFlight)；`extension SyncClipboardClient: SyncClipboardClienting {}` 结构化 conform。
  - `SyncClientFactory.swift`：`make(server:trustInsecureCert:flags:) -> any SyncClipboardClienting`；Rust 分支 `#if UC_RUST_CORE`，扩展无 flag 恒返 native。
  - `RustSyncClient.swift`（整文件 `#if UC_RUST_CORE`）：`RustSyncCore.shared` 单例 (持 `MobileSyncClient` 一条 runtime 线程 + `ucMobileInit()` + `AppGroupBridge` + trust setter 热切)；`RustSyncClientAdapter`(progressive：getClipboard→Rust，其余→内部 native fallback `SyncClipboardClient`)；纯映射器 (`rustServer`/`clipboard`/`syncError`)。
- **flag**：`MobileCoreFlags.syncClientUsesRustCore`(key `mobileCore.syncClientUsesRustCore`，默认 OFF)。
- **调用点改动**：SyncEngine(`inFlightClient` + 3 helper 签名 → `any SyncClipboardClienting`；484 构造改 `SyncClientFactory.make`)、AppViewModel.refresh:1353、ReceiveClipboardIntent:55。
- **🔴 dual-build 名字消歧（重要，step 2/3 复用）**：native 与 Rust binding 都叫 `ServerConfig`/`HistoryRecord`。adapter 文件同时 import 两模块 → 裸名歧义。用 `typealias Native*`：`#if canImport(UniClipboardModels)`(SwiftPM) 用 `UniClipboardModels.X` 限定，`#else`(app 单模块) 用裸名 (当前模块优先 import 的)。⚠️ typealias 不能 `private`(被 internal 方法签名用→访问级别报错)，要 internal。`Clipboard`(native 唯一，Rust 是 `ClipboardMeta`)/`HistoryQuery`/`SyncError`(同模块在 Network) 不冲突，免别名。
- **映射事实**：Rust 错误 enum case **PascalCase**(`.NotInitialized`/`.InvalidInput(reason:)`/`.Network`/`.Unauthorized`/`.NotFound`/`.ServerError(status:)`/`.ProtocolError(status:)`/`.DecodingFailed`/`.Cancelled`/`.Internal(reason:)`)；`ClipboardKind` 普通 enum **lowercase**(`.text/.image/.file/.group`)。Rust `Network` 折叠 native connectTimeout/receiveTimeout/networkUnreachable(引擎都退避,中性)。`base_url` 去尾斜杠。**已知良性 gap**：server 省略 size 时 native→nil、Rust→0(size 不参与同步决策，hash 才是)。
- **A/B 验证**：host swift test 无法字节平价 (reqwest 不走 URLProtocol)；新测只锁 flag 选后端 + 纯映射器 (8 测，用 `@testable import UniClipboardNetwork` 访问 internal adapter/mapper)。真机靠 `SyncClient.getClipboard via Rust core` log.notice。
- **生命周期成本**：flag ON 时每 tick 仍建 1 个 cheap adapter + 1 个 native fallback(URLSession，同 baseline)；贵的 Rust runtime 线程经 `RustSyncCore.shared` 复用。cancelInFlight 同时取消 shared Rust client(全局 abort,path 变正确，不 poison)+ 本 adapter 的 native fallback。

### M6-1 step 1 真机诊断坑（iOS `d725817`）
- 首次真机"toggle 开了、同步成功、但 Console 搜不到 `SyncClient.getClipboard via Rust core`"。根因排查：路由逻辑单测已证实 (flag on→必返 Rust adapter)；"同步成功"区分不了后端；**没日志=adapter getClipboard 没被调到**（翻错开关/App 后台无 tick/扩展走 native/搜索词错）。
- 修法：工厂 `SyncClientFactory.make` 加 `#if DEBUG` `log.notice("SyncClientFactory: backend = Rust core/native Swift")`——每次构造 (每 tick) 都打实际后端，不管走哪端点，一锤定音。**经验：per-endpoint log 只在该端点被调到才打；想确认"flag 是否生效"要在分叉点 (工厂) 打，且两分支都打**。getClipboard 的 1Hz notice 不 #if DEBUG(flag 生产恒 off 故不触发)；工厂 log 两分支都打会在生产 native 路径 1Hz 刷屏，故 `#if DEBUG`。

### M6-1 step 2 实现（iOS `53175aa`）
- putClipboard/putFile/getFile 从 native fallback 搬到 Rust(adapter 内，调用点不变)。仅 history(queryHistory/getHistoryPayload) 留 native fallback 待 step 3。
- **putClipboard 字节兼容 (写路径 #1 风险，已核对)**：native `putClipboard(_ entry:)` 只发 metadata(payload 经 putFile 另发，§3.5)→ Rust `put_clipboard(server,meta,payload: nil)`。`ClipboardMeta::into_proto()` 恒发 `Some(size)`；native 上传 entry(publishText/Image/File/fromText) **size 恒非 nil** → 两侧都发 size，无失配。into_proto 重做 hash 空→省略归一 (同 Clipboard.init)。字节由 M2 锁。
- 映射器 `clipboardMeta(from: Clipboard)`(step 1 `clipboard(from:)` 反向)+`rustKind`；`size: UInt64(max(0, entry.size ?? 0))`(?? 0 仅防御，上传路径不触发)。
- helper：`client()`(取 shared client 带 trust)+`mappingErrors`(Rust SyncError→native)。getFile/putFile 是 Data⇄Vec<u8> 透传。
- 测试 +2(上传向映射 + Clipboard→meta→Clipboard 往返)；现 220 XCTest + 34 Swift Testing。

### M6-1 step 3 实现（iOS `6af01ed`）
- queryHistory/getHistoryPayload 从 native fallback 搬到共享 Rust client（adapter 内，调用点零改）。**Rust client FFI 已 M2 暴露 history 端点，无需改 Rust**——纯 iOS adapter 加映射。**全 7 端点经 Rust**；native fallback 仅剩 `cancelInFlight` 在用，step 4 删。
- **history 类型映射（step 4+ / M6-2 复用）**：
  - `rustQuery(from: native HistoryQuery) -> UniClipboardCore.HistoryQuery`：page/types `Int?`→`Int64?`、before/after/modifiedAfter `Date?`→epoch millis `Int64?`、searchText/starred/sortByLastAccessed 1:1。**nil = 字段从 multipart body 省略**，两侧语义一致。
  - `historyRecord(from: UniClipboardCore.HistoryRecord) -> NativeHistoryRecord`：hash/text/hasData/starred/pinned/isDeleted 1:1、kind 复用 `kind(from:)`、size/version `Int64?`→`Int?`(64-bit 无损)、create/lastModified/lastAccessed epoch millis→Date。
  - **时间戳约定**：Rust FFI 用 Unix epoch millis(i64)（同 M2/M4 约定），Swift 侧 `Int64((date.timeIntervalSince1970*1000).rounded())` ⇄ `Date(timeIntervalSince1970: Double(ms)/1000)`。`.rounded()` 取最近 ms——输入是 millis-精度（服务器时间戳由 millis ISO-8601 解析来），无损，wire 字节与 native fractional-seconds ISO-8601 一致。字节兼容由 Rust 侧 M2 `query_history` multipart golden 锁（决策③信 Rust oracle）。
- **类型消歧**：native `HistoryQuery` 在 `UniClipboardNetwork`（adapter 当前模块，裸名即 native）；Rust 的需 `UniClipboardCore.HistoryQuery` 限定。native `HistoryRecord` 用既有 `NativeHistoryRecord` 别名（`UniClipboardModels`）。测试里构造 native HistoryQuery 用 `UniClipboardNetwork.HistoryQuery(...)`、构造 Rust 用 `UniClipboardCore.HistoryRecord(...)` 限定。
- 测试 +4（HistoryQuery 全字段→Rust + nil 保 nil、HistoryRecord 全字段→native + nil 时间戳/flag 忠实）；现 **220 XCTest + 38 Swift Testing**。app build SUCCEEDED（扩展跳过 `#if UC_RUST_CORE`）。

## M6-2 recon（2026-06-15，M4 持久化 + M5 reducer 的 iOS 接入面）

> 两 Explore agent 摸 iOS 两块 + 我摸 Rust 侧。行号属 agent recon，用时复核。

### 🔑 性质判断（影响 scope）
- **验收清单的 M4/M5 核心逻辑已全绿**：E/F 区（持久化/设置）、C 区（同步编排决策核）的 🔬/🔴 条目全部由 proto 单测覆盖标 `[x]`/`[~]`（M4/M5 commit）。**逻辑下沉的验收已达成**。
- **M6 剩余验收 ≈ 真机过 D–L 的 📱 条目**：本质是「原生执行壳（剪贴板 I/O / UI / 扩展 / Intent / 捐赠 / 调度 / banner）行为不变」，**不是「每项都 routing 到 Rust」**。C/E/F 的 📱 条目都明确标注「决策核已 Rust，I/O/调度/UI 留原生」。
- 因此 M6-2 有两条 **正交** 的线：①**验收线**=真机过 D–L 📱（不依赖更多 routing，现有 client+connect-uri 已够把 app 跑起来）；②**routing 深度线**=M4/M5 还要把多少 Swift 逻辑真正切到 Rust（删原生重复、奔单一真相）。

### Rust FFI 暴露面（uc-mobile，现状 + M6-2 要做的）
- 现有暴露模式（`crates/uc-mobile/src/lib.rs`）：FFI 镜像类型（`uniffi::Record`/`uniffi::Error`）+ `From` impl 转换 + `#[uniffi::export]` 自由 fn；**proto 永远无 uniffi derive**（叶子纯净）。M2 client 的 ClipboardMeta/ServerConfig/HistoryRecord/HistoryQuery/SyncError 也在 client.rs 这么做。
- proto 已 re-export 全部 M4/M5 纯逻辑（`crates/uc-mobile-proto/src/lib.rs`）：
  - M4：`{encode,decode}_app_settings`/`AppSettings`/`AppearanceMode`；`{encode,decode}_server_list`/`load_servers`/`ServerConfig`/`ServerConfigList`/`LegacyServerConfig`/`ServerLoad`；`{encode,decode,append,touch}_history`/`ClipboardHistoryItem`/`HistoryDirection`；`loop_guard_{record,tripped}`/`LoopDirection`/`LoopGuardEvent`；`plan_eviction`/`is_valid_cache_key`/`CacheEntry`；`{encode,decode}_live_urls`/`{format,parse}_watermark`/`normalize_synced_hash`/`update_live_url`；`persist_keys`。
  - M5：`plan_preamble`/`plan_after_server_get`/`commit_*`(11 个)/纯函数`backoff_secs`/`cadence_secs`/`hashes_equal`/`is_history_sync_due`/`is_cold_start`/`advance_watermark`/`is_probe_conclusion_valid`/转移`mark_staged_applied`/`acknowledge_loop_detection`/`reset_runtime_state`/`handle_*_changed`；类型 `SyncRuntimeState`/`SyncConfig`/`SyncState`/`ServerRoute`/`PushDecision`/`Preamble*`/`ServerGetSnapshot`/`ServerNewPlan`/`CommitOutcome`/`StopReason`/`TickErrorKind`/`TickFailureOutcome`。
- **M6-2 的 Rust 工作 = 给要 routing 的模块加 FFI 镜像**（同 connect_uri 的 ConnectPayload 模式）。镜像类型多（AppSettings 17 字段 / 全套 M5 reducer 类型），是真实工作量。**无消费方前 FFI 易漂移**——现在 M6-2 就是消费方，可以暴露了（既定原则）。

### iOS M4 持久化接入面（`Shared/Models/SettingsStore.swift` ~495 行）
- 持久化项 → 后端/编解码/消费方（agent 1）：
  | 项 | 后端 | 编解码 | 消费方 | routing 价值 |
  |---|---|---|---|---|
  | server_config_list | UserDefaults | JSON | app(VM init) | 中（迁移逻辑已在 proto load_servers，但 Codable 已够好） |
  | app_settings | UserDefaults | JSON | app + Keyboard 扩展 | 中（前向兼容默认值在 proto，但 Codable 已够好） |
  | clipboard_history | UserDefaults | JSON 数组 | app + Share + Keyboard | **高（dedup 逻辑 Swift 重复两处，见下）** |
  | last_synced_hash | App Group 文件 | UTF-8 纯文本 | app + Share + Keyboard | 低（字符串归一） |
  | last_known_ssid | App Group 文件 | UTF-8 归一 | app + Keyboard | 低 |
  | live_urls | App Group 文件 | JSON dict | app | 低 |
  | history_modified_after(watermark) / last_history_sync_at | UserDefaults | ISO-8601 串 | app | 低 |
  | image_data 缓存 / 驱逐决策 | App Group 文件 | 二进制 / 纯决策 | PayloadCache | 中（plan_eviction 已 Rust，I/O 留原生） |
- **history dedup 重复（routing 最高价值点）**：同一 dedup+direction 升级+cap200 算法在 `SettingsStore.appendHistory`（扩展用）和 `AppViewModel.appendHistory`（app 用）**重复实现**。proto `append_history` 已是单一真相纯转移（load Vec→dedup append→回 Vec），routing 两处都调它即消重复。
- **扩展约束（硬）**：Share/Keyboard 调 `SettingsStore.appendHistory`/`loadHistory`/`loadAppSettings` 等，但 **不链接 core**（UC_RUST_CORE 仅 app）。任何 routing 必须 `#if UC_RUST_CORE`+native fallback，扩展恒走 native。**字节兼容已由 M4「忠实匹配 Swift」决策保证**（timestamp Double-since-2001/UUID 大写），所以 app(Rust) 与扩展 (native) 读写同一 blob 无缝——这是 M4 routing 可行的关键前提。
- pure-vs-IO 边界（与既定一致）：Rust 拥有 blob 字节（encode/decode/append/touch/plan_eviction）；native 做 UserDefaults/文件原子写/App Group 路径解析/UUID/Date.now/PayloadCache actor+semaphore。

### iOS M5 SyncEngine 接入面（`UniClipboard/Sync/SyncEngine.swift` ~968 行，**app target，不在 Shared/**）
- State 枚举（idle/succeeded/hasNewUnwritten/offlineRetrying/authFailed/loopDetected）+ UI 可观察态 + runtime 决策字段（lastSyncedContentHash/lastAppliedContentHash/loopGuard/stagedServerHash/consecutiveFailures/nextNetworkAttemptAt/lastHistorySyncAt(持久化)/isTicking/isHistorySyncing/inFlightClient）——**全部对应 M5 proto 已建模**。
- tick 决策↔I/O↔commit 分界（agent 2）：plan_preamble(早退/退避门/cross-process resync) → getClipboard(I/O) → plan_after_server_get(truth-gate/server-new/push 路由) → apply/push(I/O) → commit_*(守卫/loopGuard 转移折回 state)。完全是 M5 reducer 形态。
- **client 构造已 M6-1 接 `SyncClientFactory.make`**（tick:484 等）；reducer routing 与 client routing 可 **共用 syncClientUsesRustCore flag**（一个开关控整个 M6 Rust rollout）。
- 纯函数已可直接 routing：`backoff_secs`/`cadence_secs`/`hashes_equal`（proto 已有 + 测过）。
- **🔴 验证风险**：SyncEngine 在 app target，`swift test`(macOS host SwiftPM) **覆盖不到** → 决策核 routing 只能 📱 验。**缓解**：把 state-assembly + 动作-dispatch 的纯映射抽到 `Shared/`（如 `SyncReducerAdapter.swift`，可 swift test，同 SyncLoopGuard 在 Shared/Models 可测的先例），SyncEngine 只留 I/O 编排。
- **M5 routing = big surgery + 最高风险**（968 行 tick 深度改 + 📱-only），价值也最高（决策核是最易错最该单一真相的逻辑）。

### M6-2 关键张力（待用户拍 scope）
1. **routing 深度**：全项 routing（M4 全部+M5）奔单一真相/删原生，工作量大（大量 FFI 镜像 + 每项 router+ 扩展双后端）；vs 只挑高价值（M5 决策核 + M4 history dedup），低价值项（app_settings/server_config JSON/watermark/ssid 字符串）留原生 Codable（已够好）。
2. **顺序/风险**：M4 history_log 最小可测（tracer-bullet 验 M4 FFI 管道）→ M4 其余 → M5 reducer（最难最险，📱-only，最后）；vs 先攻 M5 决策核（价值最高但最险）。
3. **验收线 vs routing 线**：D–L 的 📱 真机过一遍（确认原生行为不变）其实独立于更多 routing——可以现在就排期真机过，与 routing 深度解耦。

### ✅ scope 拍板（2026-06-15）：价值优先渐进
- 只 routing 高价值逻辑：**① M4 history_log**（消 Swift dedup 重复，最小 tracer-bullet，可 swift test）→ **② M5 决策核**（big surgery，抽 `Shared/SyncReducerAdapter.swift` 可测，📱-only，最后）。
- 低价值持久化项（app_settings/server_config JSON、watermark/ssid 字符串归一）**留原生 Codable**（已够好，ROI<成本）。
- D–L 📱 验收线独立排期；每步真机验。
- ① 实现前置：看 proto `history_log` 公开签名（`append_history`/`touch_history`/`encode_history`/`decode_history` 入出参 + `ClipboardHistoryItem`/`HistoryDirection` 定义）设计 FFI 镜像。

### ⚠️ ① 实现发现（2026-06-15，ROI 复核——recon 低估的成本）
设计 ① 的 Rust FFI 时撞到三点，合计把 ① 的成本/价值比拉低：
1. **entry 的 size Optional 失真（字节风险）**：`ClipboardHistoryItem.entry` 是 proto `Clipboard`（`size: Option<i64>`，serde 忠实 None vs 0）。复用现成的 client `ClipboardMeta`（`size: u64` 非 Option，M2 为 upload 路径设计）作 entry 会把 `None→0` → **Rust 写的 history blob 与扩展 (native) 写的字节不一致**，破坏 M4「忠实匹配 Swift」零回归保证（findings 上面那句「blob 由 M4 忠实保证两后端无缝」当时没考虑 FFI 镜像会引入失真——更正）。要忠实就得 **新建第二个 size-Option 的 Clipboard FFI 镜像**（与 ClipboardMeta 高度重复，iOS adapter 多一组 native Clipboard⟷Rust 映射），或走纯 blob 边界但 decode 仍要 record。
2. **扩展恒 native 削弱「消重复」**：Share/Keyboard 调 `SettingsStore.appendHistory` 但不链接 core → 该 native 实现必须保留。routing 只切 app 侧的 `AppViewModel.appendHistory` → Swift dedup 算法仍留一份（SettingsStore），「消重复」只达成一半。
3. **AppViewModel observable 重构**：`AppViewModel.appendHistory` 直接 mutate observable `history` 数组 + didSet 持久化；routing 要重构这条更新路径。
- **FFI 管道验证价值也不大**：connect_uri（`parse_connect_uri`）+ M2 client（Record/Enum/export）已验证「proto→uc-mobile FFI 镜像→iOS 调用」管道，① 的「验管道」论证不成立。
- **M5 的 size 失真良性（更正 2026-06-15）**：M5 reducer **确实接收 Clipboard**（`ServerGetSnapshot.server_entry` + `SyncRuntimeState.staged_entry: Option<Clipboard>`），我先前「不传 Clipboard 字节」的说法不准。但关键差异：这些 Clipboard **不写持久化 blob**（runtime 字段 / 临时输入），size 失真（None→0）**良性**——staged_entry 与 server_entry 都经同一 native→Rust 映射，hashless dedup equality 两边一致、不产生 blob 字节漂移。→ **M5 可复用现成 `ClipboardMeta`**（不必新建 size-Option 镜像）。M5 避开的是「写 blob 字节回归」（M4 history 的真问题），不是「不传 Clipboard」。M5 仍是真高价值（决策核单一真相）+ 迁移核心。→ ✅ 用户拍板（2026-06-15）：**跳过 ①，M6-2 = ② M5 决策核**（M4 history 归低价值留原生）。

### M6-2 ② 子步 1：M5 reducer FFI 暴露面（✅ done 2026-06-15，commit `84ffdf32b`）
> 落地：uc-mobile `reducer.rs`（1044 行）。15 类型镜像 + From 双向 + 24 wrapper + 21 单测。形态 (i) 值传已实现。复用 ClipboardMeta（size 失真良性，不入 blob）。client.rs `{into,from}_proto`→pub(crate)。Swift binding 验过（带数据 enum 正确）。⚠️ 改了 Rust → iOS 接入前需重 stage RustCore（`build-rust-core.sh`）。
完整暴露面（同 connect_uri/client 的「FFI 镜像 Record/Enum + From 双向 + `#[uniffi::export]` wrapper」模式，proto 仍无 uniffi）：
- **类型镜像（15）**：`SyncState`(enum6)/`SyncConfig`(record7+Default)/`SyncRuntimeState`(record，嵌 `Vec<LoopGuardEvent>`+`Option<ClipboardMeta>` staged_entry)/`LoopGuardEvent`(record)+`LoopDirection`(enum)/`PreambleSnapshot`/`Preamble`+`PreambleProceed`(enum 带数据 Stop(StopReason))/`StopReason`/`ServerGetSnapshot`(嵌 `Option<ClipboardMeta>`)/`ServerRoute`(enum 带数据 Converged{hash}/ServerNew(ServerNewPlan)/Push(PushDecision))/`ServerNewPlan`/`PushDecision`/`CommitOutcome`/`TickErrorKind`/`TickFailureOutcome`。+ 复用 `ClipboardMeta`。
- **函数 wrapper（~24）**：`plan_preamble`/`plan_after_server_get`；commit：`commit_converged`/`commit_apply`/`commit_apply_failed`/`commit_stage`/`commit_push`/`commit_push_skipped`/`commit_consent_push`/`commit_tick_success`/`commit_tick_failure`/`commit_history_sync_done`；转移：`mark_staged_applied`(→bool)/`acknowledge_loop_detection`/`reset_runtime_state`/`handle_active_server_changed`/`handle_network_route_changed`；纯函数：`hashes_equal`/`backoff_secs`/`cadence_secs`/`is_history_sync_due`/`is_cold_start`/`advance_watermark`/`is_probe_conclusion_valid`。
- **🔑 设计抉择：mut state 怎么过 FFI**。proto 是 `fn(st: &mut SyncRuntimeState, ...) -> Output`（原地改 + 返回 plan）；uniffi Record 值语义无 `&mut`。**定 (i) 值传 + 返回 new state**（每个 mut fn wrapper 接 `SyncRuntimeState` 值、返回含「更新后 state + 输出」的 result Record；Swift `state = r.state`）。理由：忠实 proto caller-holds-plain-struct 哲学；`SyncReducerAdapter`(Shared/) 纯映射易测；不引入 uniffi::Object 的隐藏内部状态（mock 难）。代价：每 tick clone state 进出 FFI 两次（state 小、1Hz，可忽略）。否决 (ii) uniffi::Object 持 state。
- **体量**：M2/M3 量级（~15 镜像 + From 双向 + 24 wrapper + 单测，约 800–1200 行 Rust）。是独立大 milestone。
- ✅ **子步 2 done（iOS `288e10c`）**：`Shared/Network/SyncReducerAdapter.swift`——snapshot 构造 + native Clipboard⟷ClipboardMeta（复用 step2 `RustSyncClientAdapter.clipboardMeta`）+ reducer 转发（planPreamble/planAfterServerGet/commitStage/commitApplyFailed）。**实现收窄**：① State⟷SyncEngine.State 映射不在 adapter（State 嵌在 app-target engine，Shared/ 不可达）→ 留子步 3 engine 自己映射；② hash-only commit + 纯函数无 Clipboard 映射 → 子步 3 SyncEngine 直调 `UniClipboardCore` binding，不经 adapter（避免 passthrough）。10 swift test（220 XCTest + 48 Swift Testing）。
### M6-2 ② 子步 3 设计（recon+ 设计 done 2026-06-16，✅ 拍 A1 双路径）
> `UniClipboard/Sync/SyncEngine.swift` 968 行 refactor，**📱-only**（app target，swift test 覆盖不到）。完整字段/调用点映射见 agent recon。

**字段迁移**（→ Rust `SyncRuntimeState` / 留 native）：
- → Rust state：`lastSyncedContentHash`(86)→last_synced_hash、`lastAppliedContentHash`(96)→last_applied_hash、`loopGuard`(102,native SyncLoopGuard)→loop_events、`stagedServerHash`(107)→staged_server_hash、`stagedEntry`(70,Clipboard?)→staged_entry(ClipboardMeta)、`consecutiveFailures`(167)→consecutive_failures、`nextNetworkAttemptAt`(383,Date?)→next_attempt_ms(epoch-ms)、`lastHistorySyncAt`(192,Date?)→last_history_sync_ms。
- 留 native：`state`(63,UI observable，从 Rust SyncState 同步)、`lastSyncedAt`/`lastError`/`isExplicitlyRefreshing`(UI)、`isTicking`/`isHistorySyncing`/`loopTask`(锁/调度)、config(cadence/backoff/historySyncInterval/maxPages/isSceneInactive)、`viewModel`/`store`/`inFlightClient`(deps/IO)。

**调用点映射**（决策→plan / I/O 留 native / commit）：preamble(434-477)→`plan_preamble`；getClipboard→native I/O；truth-gate/server-new/push(497-521)→`plan_after_server_get`；apply/push/consent I/O→native；advanceSynced+loopGuard.record+tripped(660-668/741-758/797-806)→`commit_apply`/`commit_push`/`commit_consent_push`(内置 record_and_check + 返 tripped)；stage(669-679)→`commit_stage`；apply 失败 (656-658)→`commit_apply_failed`；tick 成功 (539-544)→`commit_tick_success`；错误 (545-598)→`commit_tick_failure`(SyncError.kind→TickErrorKind)；markStagedApplied(260)→`commit_apply`；acknowledge(276)→`acknowledge_loop_detection`；reset(286)→`reset_runtime_state`；handleActiveServerChanged(323)→`handle_active_server_changed`+`store.saveLastSyncedHash(nil)`；handleNetworkRouteChanged(344)/handleEndpointChanged(357)→`handle_network_route_changed`；runHistorySyncIfDue(881) 节流/冷启/watermark→`is_history_sync_due`/`is_cold_start`/`advance_watermark`+`commit_history_sync_done`，分页 walk 留 native。

**🔑 抉择 1：A/B 策略 → ✅ A1（用户拍板 2026-06-16）**——M6 灰度原则是 flag-gated native↔Rust 双路径（同 M6-1 client）：
- **A1 flag-gated 双路径，散字段共享真相（推荐）**：state 真相留散字段+SyncLoopGuard；flag on 时 tick 决策点「组装散字段→`SyncRuntimeState`→调 reducer→拆回散字段」，flag off 走原 native inline 决策（**零改动=零回归**）。真 A/B（共享 state）、易 revert（flag off）、native fallback 在、📱-only 更需 native 对照。代价：每 tick 组装/拆解 (state 小可忽略)、reducer 路径新增~镜像 tick 结构、SyncLoopGuard 要暴露 events 做组装/拆解、native+reducer 决策暂双份 (收尾删 native)。
- **A2 两套 state 并存**：flag on 持 `SyncRuntimeState`、flag off 持散字段，切 flag 重 init。少组装/拆解但两套 state 字段。
- **B 单向重写**：删散字段+SyncLoopGuard，engine 全持 `SyncRuntimeState`。彻底单一真相，但 **非 A/B(失 native fallback)+近全文重写 big diff+📱-only 无单测网风险最高 + 难 revert**。✗
- **持久化副作用边界**：Rust commit 改 in-state hash，native 在 commit 后 `store.saveLastSyncedHash`/`saveLastHistorySyncAt`(持久化留 native)。
- **Date⟷epoch-ms**：next_attempt/last_history_sync 转换，集中 helper `nowMs()`/`dateFromMs()`。
- **SyncLoopGuard window/threshold**→`SyncConfig`(loop_window_secs/loop_flip_threshold 已在 proto)。
- **UI 字段**(lastSyncedAt/lastError) 每 commit 后 native 同步 (Rust 无)。

**实施分阶段（A1 下，每阶段独立 flag 分支 + 📱 验 + revert）**：① pull(preamble+converged+server-new)→② push(maybePush)→③ consent/markStagedApplied→④ history sync→⑤ 公开方法 (acknowledge/reset/handle_*)。
> ⚠️ 子步 3 是目标 B **最高风险一步**：968 行深改、📱-only 无 swift test 网（仅子步 2 adapter 4 方法有）。

#### 子步 3 阶段 ① 实现级决策（2026-06-16，coding 前 recon 确认）
读全文 SyncEngine.swift(978 行) + reducer.rs(FFI 镜像) + proto sync_engine.rs(语义) + SyncLoopGuard.swift 后定的 4 个非显然点：
1. **🔴 loop-trip guard 陷阱（最关键）**：proto `record_and_check`(sync_engine.rs:735) tripped 时 **已置 `st.state=LoopDetected`**。若先把 reducer 返回的 state 整体映回 engine.state（含 .loopDetected）再调 native `tripLoopBreaker()`，后者 `guard state != .loopDetected else { return }` 会 **提前返回 → stop() 不执行 → loop 不停**（破坏 loop 检测）。**解法**：`applyReducerRuntime(_:)` **只同步非-UI runtime 字段**（lastSyncedContentHash/lastAppliedContentHash/loopGuard/stagedServerHash/consecutiveFailures/nextNetworkAttemptAt/lastHistorySyncAt），**不碰 engine.state**；engine.state + lastSyncedAt/lastError 由 shell 每决策点 **显式设**，逐行镜像 native（apply 路径先 `state=.succeeded` 再 `if outcome.tripped { tripLoopBreaker() }`，guard 此时为 .succeeded 正常放行）。reducer 返回的 state 字段不映回（信息性）。
2. **stagedEntry 保真**：`stagedEntry`(observable Clipboard?) 经 ClipboardMeta 往返会 size None→0 失真（M5「良性」仅对 reducer 内部 dedup 成立，对 observable 是 cosmetic 回归）。**解法**：native `stagedEntry` 保持全保真，由 shell 显式设（stage/apply-failed 时 = 手里的 native `entry`，apply/converged 时 = nil）；assemble 时映射进 SyncRuntimeState(Clipboard→ClipboardMeta) 供 reducer hashless dedup，但 `applyReducerRuntime` **不** 从 rs.stagedEntry 反写。`stagedServerHash`(String 无损) 走 reducer，与 stagedEntry 成对一致。
3. **SyncLoopGuard 重建**：`events` private、只读 `snapshot`、无从 events 构造的 init。assemble 读 `loopGuard.snapshot`→`[LoopGuardEvent]`；unpack 需从 `[LoopGuardEvent]` 重建 → 给 `SyncLoopGuard` 加 `init(window:flipThreshold:events:)`（同文件，可访问 private events；Shared/ 纯值类型 swift-testable）。Date⟷ms 用 `.rounded()`(同 step3 约定，30s window 下 sub-ms 无影响)。
4. **engine.state 显式镜像（非 reducer 映回）**：preamble Stop(NoActiveServer)→`.idle`、Stop(Paused/BackoffGate)→不动、ToNetwork→不动；converged→`.succeeded`；apply→`.succeeded`(+trip)；stage→`.hasNewUnwritten`；apply-failed→不动 (error handler 设)。`enterLoopDetected()` 从 `tripLoopBreaker()` 抽出无-guard 主体备复用（本阶段仍走 tripLoopBreaker，因 shell 先 .succeeded）。
- 阶段 ① 范围：preamble(434-477)+truth-gate(497-514)+processServerNew(601-684)。push(518-521 maybePush) 留 native(阶段 ②)。每决策点 `if flags.syncClientUsesRustCore { reducer } else { 原 native inline }`（复用 M6-1 `syncClientUsesRustCore` flag）。helper 全 `#if UC_RUST_CORE`(扩展不链接 core)。

## 删原生路径 recon + 计划（2026-06-19）

> 子步 3 全 5 阶段 + 诊断 harness（含 happy-path）已坐实 → 收口不可逆清理。**核心认识：「删原生路径」≠「删所有 native」**。

- **🔴 边界铁律 = `#if UC_RUST_CORE` 是扩展边界**：Share/Keyboard 扩展 **不链接 core**，共编 `Shared/` 文件时只编 native 分支，且 **直接实例化 native `SyncClipboardClient` + `SettingsStore`**（`UniClipboardKeyboard/{KeyboardModel,KeyboardUploader,KeyboardViewController}`、`UniClipboardShare/{ShareRootView,ShareUploader}`），**不经** 工厂/router/flag/adapter。→ native `SyncClipboardClient`/`SettingsStore`/`ConnectURI.parse`/`ConnectionTester` **永久保留**。
- **删除面分三类**：
  1. **app-only 文件 → 彻底单一路径**：`UniClipboard/Sync/SyncEngine.swift`（仅 app target 编、app 硬依赖 core SPM product）。10 个 dispatcher triple（287/315/337/386/420/446/664/720/945/1057）形态 `func foo(){ #if UC_RUST_CORE if flag {fooViaReducer();return} #endif fooNative() }` + `private func fooNative(){verbatim}` + `fooViaReducer` 在底部 `#if UC_RUST_CORE` 扩展 (1168+)。塌缩=删 dispatcher+ 全部 `*Native()`，公开/内部方法直接走 reducer 逻辑，**恒真的 `#if UC_RUST_CORE` 一并去掉**，`import UniClipboardCore` 转无条件。
  2. **Shared/ 共编文件 → 只删 flag 分叉，保留 `#if UC_RUST_CORE`(Rust)/`#else`(native) 结构**：`SyncClientFactory.make`（→`#if UC_RUST_CORE return RustSyncClientAdapter #else return SyncClipboardClient`，删 `flags` 参数）、`ConnectURIRouter.parse`（同形，删 `flags`）、`RustSyncClientAdapter`（整文件已 `#if UC_RUST_CORE`，删内部 `fallback: SyncClipboardClient`(93)+init(98)+`cancelInFlight` 的 `fallback.cancelInFlight()`(170)；cancelInFlight 只 cancel 共享 Rust）。
  3. **整删**：`Shared/Network/MobileCoreFlags.swift`（两 flag 删后零引用）、`SettingsView.swift` 两 DEBUG toggle(147/156)。
- **保留不动**：`SyncClipboardClienting`(协议 seam，两后端 conform)、`SyncReducerAdapter`(reducer 桥，SyncEngine 1182/1224/1274/1301 在用，**非死代码**)、`RustSyncClientAdapter` 的映射器（rustServer/clipboard/clipboardMeta/syncError/rustQuery/historyRecord）。
- **测试要更新**：`Tests/UniClipboardCoreTests/SyncClientRouterTests.swift`(flag→backend 路由用例，如 `client is RustSyncClientAdapter`)、`ConnectURIRouterTests.swift`(flag on/off 用例)、任何注入 `MobileCoreFlags` 的测试——flag 删后改为「UC_RUST_CORE 下恒 Rust」单态。
- **验证**：SyncEngine app-target-only → `swift test`(macOS host) 覆盖不到，每 commit 只能 `xcodebuild 模拟器 build`(禁签名) + `swift test`(Shared/+adapter+ 映射器不回归)。flag 删后无 A/B，reducer 是唯一路径，靠 `ios-log-diagnose` skill 自查 + 用户 📱 终验。**不可逆**：删后无 native 对照，但子步 3 全路径 + happy-path 已 📱/agent 验过、风险有界。
- **commit 序列（每步 build+test）见 task_plan**。⚠️ 全在 iOS repo `mobile-sync-rust-core` 分支；本会话前 iOS 工作树 clean（无用户并行 WIP）。RustCore 不变（纯删 iOS 代码，无 Rust 改动）。
