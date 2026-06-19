# Task Plan: 目标 B — mobile-sync 共享逻辑迁移（uc-ios → Rust）

> 工作记忆层。持久权威文档在 `.planning/research/`（迁移方案 + 回归清单 + spike 计划），
> 本文只做「我在哪 / 去哪 / 决策 / 错误」的快照，细节不重复正文。
> 分支 `military-muscle`（PR 目标 main）。Rust toolchain 钉死 1.95.0，cargo 从仓库根跑。

## Goal

把 uc-ios `Shared/`（Network/Models/Cache）里「给定输入 → 确定输出」的纯逻辑 + HTTP 客户端
下沉到共享 Rust crate（`uc-mobile-proto` 纯编解码叶子 + `uc-mobile` UniFFI 边界），iOS（未来
Android）经 UniFFI 调用；**只做 mobile-sync（明文 HTTP + Basic Auth），不做 P2P/iroh/加密栈**。
验收标准 = `.planning/research/uc-ios-regression-checklist.md` 逐条全绿。

## Current Phase

**Phase 6（M6）· uc-ios 接入与灰度** — 进行中。connect-uri 全 done。**M6-1 全 7 端点 📱真机通过**（step 1 getClipboard + step 2/3 put/file/history，2026-06-16 用户真机验过）：getClipboard/putClipboard/putFile/getFile/queryHistory/getHistoryPayload + cancelInFlight 全经 Rust core，native fallback 仅剩 cancelInFlight、待 step 4 删（step 4 = 删 adapter native fallback，待 M4/M5 全切完 + 真机后做）。220 XCTest + 38 Swift Testing 全绿、app build SUCCEEDED。4 设计决策见 Decisions。**M6-2 子步 3 阶段 ①（M5 决策核 pull 路径）代码完成（iOS `f8ddf79`）**：SyncEngine tick 的 preamble + truth-gate/server-new 经 Rust M5 reducer（flag-gated 双路径，flag off 零回归）；push/history/公开方法仍 native（阶段 ②+）。220 XCTest + **50** Swift Testing 全绿、app build SUCCEEDED。阶段 ① 4 实现级决策（含 🔴 loop-trip guard 陷阱）见 findings。**阶段 ① 📱 真机验通过（2026-06-17 用户翻 toggle，Console 实测 reducer 逐 tick 日志 + 拉取/收敛/server-new/loop 正常）**。**阶段 ② done（push，iOS `2c2711f`）**：`route` 的 `.push(decision)` 捕获 reducer 算好的 PushDecision → 新 `maybePushViaReducer`（skip→`commit_push_skipped`+shell 补 lastSyncedAt 细微差；doPush→native PUT→`commit_push`+native appendHistory/donation；trip stick loopDetected）；flag off 逐字节不变=零回归；app build SUCCEEDED + 220 XCTest + 50 Swift Testing 不回归。**阶段 ② 📱 真机验通过（2026-06-18 用户翻 toggle，push 路径正常）**。**阶段 ③ done（consentPush + markStagedApplied，iOS `89918f3`，📱 验通过）+ 阶段 ④ done（history sync `runHistorySyncIfDue` 节流/冷启/watermark/done-commit 经 reducer，iOS `761d1b4`）+ 阶段 ⑤ done（余下公开方法 acknowledge/reset/handleActiveServerChanged/handleNetworkRouteChanged/handleEndpointChanged，iOS `157a626`）**：全 dispatcher/native/viaReducer 三件套，flag off 逐字节不变=零回归；app build SUCCEEDED + 220 XCTest + 50 Swift Testing 不回归。**🎉 子步 3 收口（SyncEngine 决策核全 5 阶段全迁，flag on 时 pull+push+consent+staged-apply+history-sync+ 公开转移全经 Rust M5 reducer）**。**✅ 子步 3 整体 📱 终验通过（2026-06-18 用户「全部验证通过」，flag on 全路径真机跑通）**。用户拍板 **现在删原生路径**，但前置：删后 reducer 路径必须能靠日志诊断、不黑盒。**生产诊断日志 harness done（iOS `e94ffd1`）**：去掉全部 reducer 日志的 `#if DEBUG`（原先 Release 零输出=黑盒）；1Hz 分级（逐 tick 决策 preamble/route/skip→`debug` 仅 live-stream 不刷持久库；状态变化 apply/stage/push/consent/staged-apply/history/5 公开转移→`notice` 持久留痕；失败→`error`）；补全 push 结果/history 计数/5 公开转移/consent 错误分支缺口；隐私只打状态/bool/计数/决策 + 8 位 hash 前缀 (`hashTag`，单向不可逆，绝不打内容)。23 条日志 subsystem `app.uniclipboard`/category `sync`。app build SUCCEEDED + 220+50 不回归。**✅ 诊断闭环实测通过（2026-06-18，agent 自主）**：boot 模拟器 + `simctl spawn defaults write` 注入 flag+server(App Group) + launch + `log stream`/`log show` 拉日志——实测抓到 reducer `sync preamble: stop(backoffGate)`(live debug) + `sync route-changed`(持久 notice) + `tick: SyncError`(持久 error)。**删原生路径后我能自主诊断、不黑盒（删后连 flag 都不用注，reducer 是唯一路径）**。流程存记忆 `ios-sim-log-diagnosis-harness` + **固化成 model-invoked skill `.claude/skills/ios-log-diagnose/`**（helper `ios-logs.sh` 三子命令 drive/stream/show，已实测；untracked 待用户决定是否 commit）。**当前焦点 → 删原生路径前置已满足，proceed 删原生路径（13 method-triple 塌缩 + flag + adapter native fallback + 工厂/router 双路径，多 commit 每步 build/test）**。⚠️ phase④ 期间发现用户并行提交了 manual-refresh consent-push 功能（`858a1e5` VM+View），其 SyncEngine 侧 `explicitRefresh(pushing:)` 漏提交→我补 `2aebbe8`（详见 Errors）。
iOS repo: `/Users/mark/MyProjects/iOSApp/UniClipboard`，分支 `mobile-sync-rust-core`（基线 main，本地 20 commit，未推送/未 PR；含用户并行提交的 manual-refresh 功能 `858a1e5`+我补的 `2aebbe8`）。RustCore/ gitignored 产物，改 Rust 后需 `UC_RUST_REPO=<rust-repo> bash Scripts/build-rust-core.sh` 重 stage。⚠️ Shared/ 里调 Rust core 的代码必须 `#if UC_RUST_CORE`（非 canImport，见 findings 🔴）。

## Phases

> 里程碑 ↔ 清单分区映射：M0/M1↔A、M3↔A7+B、M4↔E/F、M5↔C、M6↔D/G–L。

### Phase 0: spike B0–B2（FFI 管道证明）
- [x] connect_uri 叶子 crate、UniFFI crate、async client 对真 daemon 跑通
- [x] 三个工程缝：缝 1 rustls ring provider、缝 2 with_foreign 构造参数、缝 3 挂起打断
- **Status:** complete

### Phase 1: M0+M1 · proto 纯编解码全集（A 区 + B 区编解码）
- [x] golden vector 全量移植（M0）
- [x] `uc-mobile-proto` 扩出 hash/clipboard_doc/history_record/multipart/net_class 五模块
- [x] 140 测试全绿，逐字节对照 uc-ios Swift
- **Status:** complete

### Phase 2: M2 · uc-mobile HTTP 客户端（A6）
- [x] 全端点（doc/file/history）+ 状态映射 + 重试 + 取消 + base-url/文件名校验
- [x] client 侧 `WireDoc` 收敛到 proto `Clipboard`
- [x] 29 测试绿、iOS-sim 交叉编译通过
- **Status:** complete · commit `ad2596d9f`

### Phase 3: M3 · ConnectionTester (A7)
- [x] `test_connection`（单 URL，走完整 get_latest+ 重试 + 解码）
- [x] `probe`（多 URL，2s 短超时、不重试、status-only、JoinSet 并发、ProbeReport+epoch）
- [x] `first_reachable`（纯函数，复用 proto ordered_urls 形态序）
- [x] trustInsecureCert 为 probe/test 接线；53 测试绿、iOS-sim 通过、aws-lc-rs 仍缺席
- **Status:** complete · commit `8bb5d08a9`

### Phase 4: M4 · 状态与持久化逻辑（E/F 区）
> 边界模型（用户 2026-06-14 拍板）：**Rust 拥有 blob 字节**(decode/encode)，原生退化为
> 字节搬运 + 文件 I/O + 提供 UUID/Date.now()。proto 放纯逻辑+serde（无 uniffi）。
- [x] proto `app_settings`：AppSettings(17 字段)+AppearanceMode；前向兼容 (容器 default+ 未知 appearance→system+ 损坏→defaults)
- [x] proto `server_config`：ServerConfig(双写 url+urls)/List/Legacy；§5.5 迁移 + §5.2 pin 提升；`load_servers`→{list,migrated}
- [x] proto `history_log`：append 头去重/`.local`升级/cap 200/newest-first + touch；timestamp Double-since-2001 + UUID 大写串字节忠实
- [x] proto `loop_guard`：纯函数 record/tripped over Vec<Event>（大写归一/空忽略/window 淘汰/flip≥3）
- [x] proto `payload_cache`：`plan_eviction`(mtime LRU)+`is_valid_cache_key`
- [x] proto `file_state`：watermark(复用 iso8601)/normalize_synced_hash/live_urls map；`persist_keys` 键名单一真相
- [x] uc-mobile：trustInsecureCert 构造期参数 + `set_trust_insecure_cert`(RwLock swap)。持久化 FFI 镜像延后 M6
- [x] 测试：proto 198(58 新)/uc-mobile 55+1；镜像 Swift SettingsStore/SyncLoopGuard/PayloadCache；损坏→默认；iOS-sim+aws-lc 缺席
- **Status:** complete · commit `fa56d186e`

### Phase 5: M5 · SyncEngine 决策核（C 区，拆层后迁）
- [x] 决策核进 Rust：proto `sync_engine` reducer（plan_preamble/plan_after_server_get/commit_*）——server-wins 路由、去重守卫三件套、loop-guard 接线、退避/节奏、history due/cold-start/watermark、epoch 校验、公开转移
- [x] 执行壳留原生：网络 I/O、tick 调度、scenePhase、UIPasteboard、banner、prefetch、持久化 I/O（M6 接线）
- [x] 语义动作 enum（ServerRoute/PushDecision）；SyncRuntimeState plain struct（caller 持有）
- [x] 忠实移植 Swift maybePush push-trip-overwrite 怪异 + 标注（建议另开 issue）
- [x] proto 246 测试（48 新）/clippy/fmt clean；iOS-sim 通过；aws-lc-rs 缺席；uc-mobile 55+1 不回归
- **Status:** complete · commit `00d3612cc`

### Phase 6: M6 · uc-ios 接入与灰度（D/G–L + 📱）— 进行中
> 策略（2026-06-15）：tracer-bullet 先打通管道切首模块 connect-uri，feature-flag = 运行时 toggle，再逐模块长 FFI。
> iOS repo `/Users/mark/MyProjects/iOSApp/UniClipboard`，分支 `mobile-sync-rust-core`（基线 main）。

- [x] **前置 bug 修复**：Swift `maybePush` push-trip-overwrite → Rust `commit_push` 走 `record_and_check` + Swift maybePush early-return（push trip 现 stick `LoopDetected`，同 apply）。Rust commit `42272913b`；iOS commit `ada05b2`。
- [x] **M6-0a tracer-bullet（管道打通 + connect-uri A/B）**：
  - iOS `Scripts/build-rust-core.sh`（从 `UC_RUST_REPO` 跑 xcframework 构建 + stage 进 gitignored `RustCore/`；xcframework 139M→**不 check-in**，binding+xcframework 都再生零漂移）。
  - xcframework 加 **macOS slice**（`swift test` host 链接用）；Rust commit `41fcff662`。
  - `Package.swift` 用 `FileManager` 条件加 Rust-core targets（RustCore 缺则 `swift test` 行为不变，opt-in）。
  - `Tests/UniClipboardCoreTests` A/B 平价（native `ConnectURI.parse` ↔ Rust `parseConnectUri`：字段/错误类别/防御式解码全平价）。
  - **暴露并修复 proto 防御式解析缺口**（M0/M1 误标已覆盖）：`o` 非字符串值 serde 整条报错、`urls` 不过滤非 http/不丢非字符串 → proto `de_lenient_string_map`/`de_lenient_url_list`；Rust commit `f98429336`（proto 251 绿，5 新）。
  - 验收：`swift test` 全绿（220 XCTest + 21 Swift Testing）；uc-mobile 55+1 不回归；iOS commit `4b91c99`。
- [x] **M6-0b Part 1（运行时 toggle 路由生产代码，SwiftPM 可验）**：
  - `Shared/Network/MobileCoreFlags.swift`（App Group UserDefaults，默认 OFF=native，可注入；`connectURIUsesRustCore`）。
  - `Shared/Network/ConnectURIRouter.swift`（按 flag 路由；`#if canImport(UniClipboardCore)` 守卫；返回 native `Payload` + 抛 native `ParseError`；Rust→native 类型/错误映射 + 空 urls→[url] 回落）。
  - `AppViewModel.handleIncomingURL` 改用 `ConnectURIRouter.parse`（core 未链接/flag off 时等价 native，零行为变更）。
  - `Package.swift`：`UniClipboardNetwork` 条件依赖 `UniClipboardCore`（router Rust 分支在 `swift test` 可编译）。
  - 验收：`swift test` 220 XCTest + **24** Swift Testing 全绿（router 两 flag 态均匹配 native）；**app target BUILD SUCCEEDED**（native 路径，core 未链接）。iOS commit `1e14670`。
- [x] **M6-0b Part 2（Xcode app-target 链接 core，W1）**：`.pbxproj` 加 XCLocalSwiftPackageReference "." + app target 依赖 `UniClipboardCore` product（镜像 sentry 接法，6 处插入，plutil OK）。`canImport(UniClipboardCore)` 在 app 为真 → router Rust 分支编入 app，toggle 运行时生效。
  - 修复：模拟器 slice 原 arm64-only → generic/Release 构建编 x86_64 失败；构建脚本加 `x86_64-apple-ios` + lipo 通用模拟器 slice（`rustup target add x86_64-apple-ios`）。Rust commit `b63412a04`。
  - 验收：`xcodebuild ... generic/platform=iOS Simulator build` **BUILD SUCCEEDED**（UniClipboardCore 编入 app 图，25 处）；`swift test` 仍绿。iOS commit `d01c6db`。
  - **app 构建硬前置已处理**（iOS commit `41681a9`）：testflight.yml 在 swift test + archive 前 checkout Rust 仓库 (`UniClipboard/UniClipboard`，`rust_core_ref` dispatch 输入默认 main) + 加 iOS targets + 跑 build-rust-core.sh stage RustCore；CLAUDE.md 补本地前置说明。⚠️ Rust `military-muscle` 未并入 main 前，dispatch 要把 rust_core_ref 设成该分支；Rust 仓库若私有需 checkout PAT secret（注释已留）。
  - **真机 A/B 入口已备**（iOS commit `ba95915`/`1a32e05`）：SettingsView 诊断 `#if DEBUG` toggle + `ConnectURIRouter` log.notice（Console 分辨后端）+ 应用内扫码也走 router。**并修 canImport flaky 链接 bug**（Share 扩展误编 Rust 分支 → 改 `#if UC_RUST_CORE` 只在链接 core 的 target 定义；写进 iOS CLAUDE.md + findings `🔴`）。clean build SUCCEEDED。
  - [x] **📱 真机验收通过**（2026-06-15）：用户真机翻 toggle 扫码，Console 实测 `ConnectURIRouter: parsing connect URI via Rust core`（18:37:31，进程 UniClipboard），解析正常。connect-uri 模块灰度端到端坐实。DEBUG toggle 不进 TestFlight（如需 Release A/B 另议）。
- [ ] **M6-1（M2 client 切换，进行中）**——4 决策：①共享单例 `RustSyncCore`(一条 runtime 线程，server 每 call 传入，cancel 全局)；②工厂 + 协议 `SyncClipboardClienting`(原生 conform + Rust adapter conform,`SyncClientFactory.make` 按 flag 返回)；③Rust 侧当字节 oracle(host swift test 无法注 MockURLProtocol),iOS 只 router 单测 + 真机/Console A/B；④tracer-bullet 先切 getClipboard。
  - [x] **step 1（getClipboard 读路径）📱真机通过**（iOS `188b991` + 诊断 `d725817`）：plumbing(`SyncClipboardClienting`/`SyncClientFactory`/`RustSyncClientAdapter`/`RustSyncCore` 单例/`AppGroupBridge`/`syncClientUsesRustCore` flag) + 路由 SyncEngine tick + AppViewModel.refresh + ReceiveClipboardIntent。诊断坑：首次真机"同步成功但没日志"→ 加工厂层 `#if DEBUG` backend log(每 tick 打 `SyncClientFactory: backend = Rust core/native Swift`) 定位；用户重测确认走 Rust。
  - [x] **step 2（put/file）代码完成**（iOS `53175aa`）：putClipboard(native Clipboard→Rust ClipboardMeta 上传向，into_proto 恒发 size 与 native publish 路径一致字节不变)/putFile/getFile(Data 透传) 全搬 Rust；`client()`+`mappingErrors` helper 收敛。验收 220+34(+2 映射测试)、app build SUCCEEDED。**📱可与 step 3 合并验**。
  - [x] **step 3（history）代码完成**（iOS `6af01ed`）：queryHistory/getHistoryPayload 搬到 Rust（adapter 内，调用点零改）+ 纯映射器 `rustQuery`(native HistoryQuery→Rust，Date→epoch millis `.rounded()` 无损)/`historyRecord`(Rust→native，millis→Date) +4 测试。**全 7 端点经 Rust**，native fallback 仅 cancelInFlight 在用。验收 220 XCTest + 38 Swift Testing、app build SUCCEEDED。**📱可与 step 2 合并验**。
  - [ ] **step 4（待 step 2/3 📱验后）**：删 adapter native fallback（cancelInFlight 改只 cancel 共享 Rust client）；工厂 native↔Rust 双路径要等 M4/M5 也切完 + 真机验后才删原生路径（不可逆方向，谨慎）。
- [ ] **M6-2（价值优先渐进，2026-06-15 拍板；起点 2026-06-15 调整=直接攻 ② M5）**——只 routing 最高价值的 M5 决策核；M4 持久化（**含 history_log**）+ 低价值项全留原生 Codable（ROI 复核见 findings「① 实现发现」）：
  - [~] ~~① M4 history_log~~ **跳过（ROI 低）**：entry `size`-Option 失真要双 Clipboard FFI 镜像 + 扩展恒 native 只消一半 dedup 重复 + FFI 管道已由 connect_uri/client 验过 → 归入低价值留原生。
  - [ ] **② M5 决策核 reducer（迁移核心，最高价值）**：reducer 只传 hash/config/state，**不传 Clipboard 字节**，避开 size 问题。拆三子步降风险：
    - [x] **子步 1（done，commit `84ffdf32b`）**：uc-mobile `reducer.rs` 暴露 reducer FFI 镜像（15 类型 + ~24 wrapper；mut state 值传 + 返回新 state 捆 `*Step`；复用 ClipboardMeta，size 失真良性）。+21 单测；proto 251 不回归；clippy/fmt clean；iOS-sim/aws-lc 缺席/Swift binding(带数据 enum `case converged(serverHash:)`) 全验。
    - [x] **子步 2（done，iOS `288e10c`）**：`Shared/Network/SyncReducerAdapter.swift`——snapshot 构造 + native Clipboard⟷ClipboardMeta（复用 step2 `RustSyncClientAdapter.clipboardMeta`）+ reducer 转发（planPreamble/planAfterServerGet/commitStage/commitApplyFailed）。**收窄范围**：State⟷SyncEngine.State 映射（State 在 app target 不可达 Shared/）+ hash-only commit/纯函数（无 Clipboard 映射）留子步 3 让 SyncEngine 直调 binding。10 swift test（220 XCTest + 48 Swift Testing），app build SUCCEEDED。
    - [~] **子步 3（设计 done 2026-06-16 拍 A1；implementing，📱-only big surgery）**：A1=flag-gated 双路径、散字段共享真相（flag on 时 tick 决策点组装散字段→`SyncRuntimeState`→reducer→拆回；flag off 原 native inline 零改动=零回归）。分 **5 阶段**，每阶段独立 flag 分支 + 📱 验 + revert。复用 `syncClientUsesRustCore` flag。完整字段/调用点映射 + 4 实现级决策见 findings「子步 3 设计 / 阶段 ① 实现级决策」。
      - [x] **阶段 ①（pull：preamble+truth-gate/converged+server-new）done（iOS `f8ddf79`）**：poll 设共享 I/O；`preambleProceeds`/`route` 派发器；`preambleNative`/`routeNative` 抽 native 原文 verbatim 作 A/B 基线；`preambleViaReducer`/`routeViaReducer`/`processServerNewViaReducer` 走 reducer；`assembleRuntimeState`/`applyReducerRuntime` 桥散字段⟷FFI state（散字段仍真相）。**🔴 loop-trip guard 陷阱解法**：`applyReducerRuntime` 不碰 engine.state（record_and_check 已预置 .loopDetected，映回会 defuse tripLoopBreaker guard→loop 不停）+ 不碰 stagedEntry（保全保真）；engine.state 由 shell 每决策点显式设（apply 先 .succeeded 再 trip）。`SyncLoopGuard` 加 `init(window:flipThreshold:events:)` 重建。验收 220 XCTest + 50 Swift Testing(+2 重建)、app build SUCCEEDED。**📱 真机验通过（2026-06-17）**。诊断已备（iOS `ba3a76b`）：SyncEngine reducer 路径加 DEBUG-only 逐 tick 日志（`preamble proceed/stop` + `route = converged/serverNew/push`），Console 可确认 reducer 真跑 + 走哪条分支（同 M6-1 工厂 backend log 法）；Release flag off 不触发。
      - [x] **阶段 ②（push：maybePush）done（iOS `2c2711f`）**：`route` 的 `.push(decision)` 捕获 reducer 算好的 `PushDecision`，改调新 `maybePushViaReducer`（弃 native `maybePush` 委托）。skip 分支走 `commit_push_skipped` + shell 补 native `lastSyncedAt` 细微差（consent/no-device 不动；already-synced/self-written 设 .now）；`doPush` 跑 native PUT I/O→`commit_push`(advance synced+`.pushed` loop event+trip)→native appendHistory(.pushed)+donation；trip stick `.loopDetected`（先 .succeeded 过 idempotency guard，再 trip，跳 `lastSyncedAt`）。flag off 走 `routeNative`→native `maybePush` 逐字节不变=零回归。app-target only（swift test 编不到），验收 app build SUCCEEDED + 220 XCTest + 50 Swift Testing 不回归 + DEBUG 逐 tick reducer 日志 (push(decision)) 供 A/B。**📱 待验**。
      - [x] **阶段 ③ consentPush + markStagedApplied done（iOS `89918f3`）**：两个 tick 外用户触发的公开转移走 reducer（同 dispatcher/native/viaReducer 三件套，flag off 逐字节不变）。`markStagedApplied()`→`UniClipboardCore.markStagedApplied(state:)`（模块限定消歧同名方法）：reducer advance synced+ 清 stagedServerHash+ 报 wasStaged，shell 保留 no-op guard、清 stagedEntry、设 UI 字段。`consentPush(_:)`→`consentPushViaReducer`：本地 record+adopt/server-state guards/`pushSnapshot` PUT/history-direction flip/donation/inline 错误处理全留 native，`commit_consent_push` 折 advance-synced+last-applied+`.pushed` loop event+(非 trip) 清失败计数；trip stick `.loopDetected`（先 .succeeded 过 guard，跳 lastSyncedAt+donation）。app build SUCCEEDED + 220 XCTest + 50 Swift Testing 不回归 + DEBUG 日志 (wasStaged/tripped) 供 A/B。**📱 待验**。
      - [x] **阶段 ④ history sync done（iOS `761d1b4`）**：`runHistorySyncIfDue` dispatcher/native/viaReducer；throttle→`isHistorySyncDue`、cold-start→`isColdStart`、watermark advance→`advanceWatermark`(只取决策，应用 native Date 避 ms 往返精度损失)、defer done→`commitHistorySyncDone`；分页 walk+`queryHistory`+`mergeHistoryRecord`+空服务器 seed+`isHistorySyncing` 守卫+persist 留 native。app build SUCCEEDED + 220 XCTest + 50 Swift Testing 不回归。
      - [x] **阶段 ⑤ 余下公开方法 done（iOS `157a626`）**：acknowledgeLoopDetection/resetRuntimeState/handleActiveServerChanged/handleNetworkRouteChanged/handleEndpointChanged 全 dispatcher/native/viaReducer，调 `UniClipboardCore.<fn>(state:)`(模块限定消歧同名方法)；reducer 折状态字段，shell 留 engine.state/stagedEntry/persist I/O/start/forceTickNow/cancelInFlight。app build SUCCEEDED + 220 XCTest + 50 Swift Testing 不回归。**🎉 子步 3 全 5 阶段收口。**
  - 每步真机验。recon 见 findings「M6-2 recon」+「① 实现发现」。
- [~] **删原生路径（进行中 2026-06-19，不可逆收尾）**——recon done（见 findings「删原生路径 recon + 计划」）。**核心：删 app 侧双路径脚手架，非删所有 native**（扩展永久走 native）。commit 序列（每步 `xcodebuild 模拟器 build` + `swift test` 不回归；全 iOS repo，无 Rust 改动）：
  - [x] **C1（done，iOS `4020f60`）= 原计划 C1+C2+C3 合并**：SyncEngine 塌缩到 Rust reducer 单一路径——删全部 native 基线方法 (6 公开 + preamble/route/processServerNew/maybePush/consentPush/history native) + flag dispatcher + 全部恒真 `#if UC_RUST_CORE` 守卫 (import 转无条件)；`*ViaReducer` 重命名回规范名 (行为不变=📱 验过的 flag-on 路径)。1705→1075 行。Python 脚本 (brace-counting 删方法+rename+ 守卫) 一次完成，带残留/brace 校验。**BUILD SUCCEEDED + 220 XCTest + 50 Swift Testing 全绿**。
  - [x] **C2（done，iOS `c90e7eb`）**：`SyncClientFactory`/`ConnectURIRouter` 去 flag → `#if UC_RUST_CORE Rust #else native`，删 `flags` 参数 + A/B debug 日志；更新 `ConnectURIRouterTests`/`SyncClientRouterTests`（删 3 flag 用例，50→47）。BUILD SUCCEEDED + 220+47。
  - [x] **C3（done，iOS `56fefcf`）**：`RustSyncClientAdapter` 删 native fallback（`fallback` 属性 + init 构造 + cancelInFlight 的 `fallback.cancelInFlight()`）；cancelInFlight 只 cancel 共享 Rust client。init 保留 `throws`（工厂 native 路径仍需）。BUILD SUCCEEDED + 220+47。
  - [x] **C4（done，iOS `ec6dc86`）**：删 `MobileCoreFlags.swift` + `SettingsView` 两 DEBUG toggle + 改 2 处残留注释（SyncClipboardClienting/AppViewModel）。**全仓零 MobileCoreFlags 引用**。BUILD SUCCEEDED + 220+47。
  - [x] **删原生路径手术收口（2026-06-19）**：app 内 mobile-sync **唯一路径=Rust core**，扩展 (Share/Keyboard) 经 `#if UC_RUST_CORE #else` 保留 native client/parser。**端到端验证通过**：`ios-log-diagnose` drive 刚 build 的删除后 app（PID 64236）打 mock，实测 `sync apply hash=2014A4CB tripped=false` + `sync history: round done` + 公开转移——reducer 唯一路径端到端跑通（flag 注入现为无害 no-op）。**剩：用户 📱 真机终验 + 三进程 TLS + D–L 📱 清单**。
- [ ] 三进程 TLS 验收 + D–L 📱 清单真机过一遍（验收线，独立于 routing 深度）。
- [x] **诊断 harness happy-path 终验 + skill 清理（done 2026-06-18）**：用户「先用 `ios-log-diagnose` skill 把 happy-path 日志实跑验证一遍再删」。之前闭环只抓到 stop/error 路径（死 URL）；本次起 mock SyncClipboard 服务器（返回合法 `/SyncClipboard.json`，自算大写 SHA-256 hash 保 apply 校验过）→ `drive http://127.0.0.1:59998`（flag ON + active server，默认 autoApply=ON/autoPush=OFF）→ **两通道全验**：live debug `preamble: proceed`/`route: converged hash=CB67C116`/换内容后 `route: server-new willApply=true alreadyStaged=false`；持久 notice `sync apply: server→device hash=A0FCE822 tripped=false`/`sync history: round done`/`sync route-changed`/`sync endpoint-changed`；额外验到换服务器瞬断的 `stop(backoffGate)→proceed` 退避恢复。hashTag 关联随内容变（CB67C116→A0FCE822）、不打内容。⚠️ skill `.claude/skills/ios-log-diagnose/` **长期保留供后续诊断**（用户明确：诊断工具长期用，别删）——我一度误删后已按对话内容逐字恢复（含 `chmod +x`，系统重新识别），并 **已 commit 进 military-muscle（`5dfe301db`，ios-logs.sh 以 100755 入库）从此 tracked、不再易丢**（`.claude/` 是 AGENTS.md 豁免路径）。能力 + happy-path mock 法 + eza alias 坑见记忆 `ios-sim-log-diagnosis-harness`。
- **Status:** in_progress（connect-uri 全 done + 📱通过；**M6-1 全 7 端点 📱通过**；**M6-2 ② 子步 1 done（reducer FFI `84ffdf32b`）+ 子步 2 done（`288e10c`）+ 子步 3 **全 5 阶段 done 收口**（① pull `f8ddf79`+`ba3a76b` 📱验过 / ② push `2c2711f` 📱验过 / ③ consent+staged-apply `89918f3` 📱验过 / ④ history-sync `761d1b4` / ⑤ 余下公开方法 `157a626`，④⑤ build 全绿待 📱）**，下一步子步 3 整体 📱 终验 → 验过才做不可逆删原生路径**）

### 持续项（不单列 phase）
- [ ] CI：交叉编译 + bindgen drift + 体积预算 + aws-lc-rs 断言搬进 workflow
- [ ] uniffi `=0.31.1` / toolchain `1.95.0` 钉死不变，升级单独评估

## Key Questions

> M4 三问已答（见 Decisions Made）：①键名共用→proto `persist_keys` 单一真相；②PayloadCache 快照边界→原生采 {key,size,mtime} 传入、Rust `plan_eviction` 回带待删 key、原生删；③trust→构造期 + setter。

> M5 待答（接手时读迁移方案 §M5 + uc-ios SyncEngine.swift，968 行）
1. `decide(tick_input)` 的输入快照边界——剪贴板 hash/changeCount/网络上下文/settings/watermark/loop-guard 状态怎么打包进 FFI 入参？输出 `Vec<SyncAction>` 的动作粒度（fetch/apply/push/throttle）？
2. server-wins + 去重三守卫 + push 前提 + 退避，哪些是纯决策（进 Rust）、哪些必须执行壳即时读（如 UIPasteboard）？
3. loop-guard 事件缓冲谁持有——M4 `loop_guard` 是纯函数 over `Vec<Event>`，M5 决策核持有该 Vec 还是原生持有并每 tick 传入？

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| 移动端只做 mobile-sync，不做 P2P/iroh/加密栈 | 2026-06-12 拍板，与 VISION §63 一致；引入加密栈只拖重 crate（记忆 `mobile-no-p2p-decision`） |
| cancel **不永久 poison**（偏离 Swift） | Rust client 长生命周期、多 server、独占 runtime；poison 会逼原生每次网络切换重建 client+ 重起线程（M2，2026-06-12 拍板） |
| 单一真相收敛只做 client 侧（删 WireDoc），daemon 侧另立 issue | daemon `SyncClipboardDoc` 收敛有 PascalCase 别名/size 恒在/hash 不归一三处回归风险（M2，用户拍板） |
| M3 就为 probe/test 接 trustInsecureCert | 各自构建客户端、成本极低、忠实 A7；生产客户端 trust 留 M4（2026-06-14 拍板） |
| epoch 不透明透传，probe 返回 `ProbeReport{network_epoch,results}` | 盖戳供 M5 校验「结论仅 epoch 未变有效」；校验逻辑本身属 M5 SyncEngine（2026-06-14 拍板） |
| probe/test 作 MobileSyncClient 方法，first_reachable 作纯自由 fn | 复用 runtime+reqwest+helpers；独立 Object 会重复一套 runtime/init |
| M4 **Rust 拥有持久化 blob 字节**（decode/encode），原生纯字节搬运 | 真正迁移前向兼容/默认值/键名 + Android 单一真相；代价仅 clipboard_history 自定义 serde（2026-06-14 拍板） |
| clipboard_history **忠实匹配** Swift（timestamp=Double 秒-since-2001、UUID 大写串） | M6 灰度时本地历史无缝保留，无可见回归；约 30 行 serde（2026-06-14 拍板） |
| trustInsecureCert **构造期固定 + setter**（swap reqwest client 不重启 runtime 线程） | 与 Swift「client 按 config 构建」一致；toggle 罕见、成本低（2026-06-14 拍板） |
| M5 决策核 = **reducer（plan+commit 分阶段）** | 单次 `decide` 覆盖不了 tick 内 I/O 交织；plan 产路由、原生做 I/O、commit 折叠结果回 state（2026-06-14 拍板） |
| M5 动作建模 = **语义动作 enum**，网络 I/O 留原生执行壳 | proto 只说「做什么」+ commit「做完的状态」；忠实「I/O 留原生」边界（2026-06-14 拍板） |
| M5 交付 = **proto-only 纯逻辑 + 单测，FFI 延后 M6** | 同 M4 先例；无消费方时 FFI 接口易漂移返工（2026-06-14 拍板） |
| runtime state 聚合成 proto `SyncRuntimeState` plain struct，caller 持有 | 8+ 字段散装签名爆炸；沿用 loop_guard「caller 持 Vec」惯例（2026-06-14） |
| Swift `maybePush` push-trip-overwrite **M6 现在就修两处**（用户 2026-06-15 拍板，改 M5 的"忠实移植待开 issue"） | Rust `commit_push` 走 `record_and_check`、Swift maybePush early-return；push trip 现 stick `LoopDetected`（同 apply）。测试 `push_path_trip_shows_loop_detected` |
| M6 走 **tracer-bullet**（先打通管道切 connect-uri），非 FFI-first big-bang | 最早暴露集成风险 (modulemap/链接/签名)；契合"有消费方才暴露 FFI"原则；M1-first 对齐清单序（2026-06-15 拍板） |
| feature-flag = **运行时 toggle**（UserDefaults/隐藏设置） | 同一构建里 A/B 切 native↔Rust，最利定位回归（checklist 执行建议 #3 本意）（2026-06-15 拍板） |
| xcframework **脚本构建 + gitignore**（不 check-in），加 macOS slice | 139M/slice 太大不宜入 git；binding+xcframework 由脚本再生零漂移；macOS slice 解锁 host `swift test` A/B（2026-06-15） |
| `Package.swift` 用 `FileManager` **条件加** Rust-core targets | RustCore 缺则 `swift test` 行为不变，Rust core opt-in 不破坏默认 DX（2026-06-15） |
| connect-uri 防御式解析缺口 **修 proto 保全零回归**（非 strict、非 shim） | Rust 单一真相 + 手改 QR 也零回归 + Android 复用；缺口 C(`o` 非字符串) 只能 proto 修（用户 2026-06-15 拍板） |
| M6-0b Part 2 用 **W1**（app 依赖本地 SPM 包 `UniClipboardCore` product），由我手改 .pbxproj | router 无需改码（canImport 一套机制）；代价 RustCore 成 app 硬前置（gray rollout 本就要 core 在场）；镜像已有 sentry SPM 接法（用户 2026-06-15 拍板） |
| xcframework 模拟器 slice 必须 **通用 (arm64+x86_64)** | generic/Release 模拟器构建编双架构，arm64-only slice 链接 x86_64 失败；lipo 合并（2026-06-15 接线时发现） |
| M6-1 Rust client = **共享长生命周期单例**（`RustSyncCore.shared` 持 `MobileSyncClient`，一条 runtime 线程，server 每 call 传入） | 原生每操作新建 (含 tick 每 1s)；Rust 构造即 spawn runtime 线程，每操作新建=tick 级线程开销。server-per-call 的 Rust API 正为支持长生命周期；cancel 全局 abort 在 path 变时本就正确且不 poison（2026-06-15 拍板） |
| M6-1 路由 = **工厂 + 协议**（`SyncClipboardClienting`，原生直接 conform，`RustSyncClientAdapter` conform，`SyncClientFactory.make` 按 flag 返回 `any`） | 干净/可测/对齐六边形；扩展无 `UC_RUST_CORE` 工厂恒返 native 优雅降级（2026-06-15 拍板） |
| M6-1 A/B = **信 Rust 侧 oracle + 真机/Console**（host swift test 不做字节平价） | reqwest 不走 URLProtocol，无法注 MockURLProtocol；Rust 侧 29 M2 测试 + golden vector 已锁字节兼容；iOS 只 router 单测 (flag 选对 + 类型/错误映射)（2026-06-15 拍板） |
| M6-1 走 **progressive adapter + tracer-bullet**（step 1 只 getClipboard 走 Rust，其余委托内部 native fallback；调用点 step 1 改一次，步骤 2/3 只动 adapter） | 行为切片最薄、最早暴露集成风险 (bridge/单例/类型映射)、每步独立真机验+revert；后续步骤零调用点 churn（2026-06-15 拍板） |
| M6-2 走 **价值优先渐进**（只 routing 高价值逻辑：M5 决策核 + M4 history dedup；低价值持久化项 settings/server JSON、watermark/ssid 留原生 Codable） | 逻辑下沉验收已由 proto 单测达成；M6 剩余验收≈真机过 D–L 📱(行为不变)；低价值项 routing ROI<成本 (FFI 镜像+router+ 扩展双后端)；聚焦最易错的决策核单一真相。顺序 ① M4 history_log(最小可测 tracer-bullet)→② M5 reducer(big surgery,📱-only，最后)（2026-06-15 拍板） |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| （M3 期间无阻塞性错误；fmt --check 检出几处手写需重排） | 1 | `cargo fmt -p uc-mobile` 应用后复查 FMT-CLEAN-CONFIRMED |
| phase④ 提交时 `git add SyncEngine.swift` 误打包了用户并行写入同文件的 WIP（`explicitRefresh(pushing:)`），混入我的 commit | 1 | `git reset --mixed HEAD~1` + `git checkout 89918f3 -- SyncEngine.swift` 还原干净基线 → 重贴我的 history-sync 单独提交（`761d1b4`）→ 还原用户 explicitRefresh（与原版逐字节一致核验）。用户又并行提交了 `858a1e5`(VM+View) 但漏 SyncEngine 侧 → 我补 `2aebbe8` 让 tip 可编译。**教训**：在用户并行工作的 repo 里，commit 前必看 `git diff --stat` 是否有非我改的文件，`git add` 严格按文件 scope（见记忆 `parallel-repo-commit-scope`） |

## Notes

- 验回归：`cargo test -p uc-mobile`（现 55+1）+ `-p uc-mobile-proto`（现 **251**）；`cargo clippy -p uc-mobile{,-proto} --all-targets`；iOS-sim 冒烟 `cargo build -p uc-mobile --lib --target aarch64-apple-ios-sim`。
- iOS A/B 验收（在 uc-ios repo）：先 `UC_RUST_REPO=<rust-repo> bash Scripts/build-rust-core.sh` stage RustCore，再 `swift test`（现 220 XCTest + 21 Swift Testing 全绿）。RustCore/ 是 gitignored 产物，每次改 Rust 后需重跑脚本。
- `cargo check --workspace` 因 `uniclipboard`（tauri 壳）缺 sidecar 失败 = pre-existing，用 `--exclude uniclipboard`。
- `aws-lc-rs` 必须 **不在** uc-mobile 依赖树（`cargo tree -p uc-mobile -i aws-lc-rs` 应报 did not match）——ring-only TLS 栈。
- 提交触发 lint-staged（rustfmt/autocorrect/refresh-agents/fix-md-cjk-emphasis/prettier）——会回改 markdown CJK 间距，正常。
- 沙箱 stdout/Read 回传可能注入假数据/丢失——外发"成功"用 `git show`/`gh api` 二次核验（记忆 `untrusted-terminal-output`）。
- 每个里程碑单独 commit（本线 cadence）。本规划三文件是 **未跟踪工作记忆**，不提交（与 `.planning/research/` 持久文档分工）。
