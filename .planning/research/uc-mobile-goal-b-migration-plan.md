# 目标 B 方案：mobile-sync 共享逻辑零回归迁移（uc-ios → Rust）

> 前置：spike B0–B2 已完成（FFI 管道证明成立，见 `uc-mobile-spike-plan.md`）。
> 输入：`uc-ios-feature-inventory.md`（行为基线）+ `uc-ios-regression-checklist.md`（验收闸门）。
> 范围拍板（2026-06-12）：**只做 mobile-sync，不做 P2P**——本方案不含任何 Transport/iroh/加密栈内容。
> 状态：M0+M1+M2+M3+M4+M5 已完成（M5 于 2026-06-14），M6 待续。语言审查豁免路径（`.planning/`）。
> 进度：M0 ✅ + M1 ✅（`uc-mobile-proto` 扩出 `hash`/`clipboard_doc`/`history_record`/`multipart`/`net_class` 五模块，从 uc-ios Swift 逐字节迁移，140 测试全绿）· M2 ✅（`uc-mobile/client.rs` 补全 A6：全端点 + 状态映射 + 重试 + 取消 + base-url/文件名校验，29 测试绿、iOS-sim 交叉编译通过，client 侧 `WireDoc` 收敛到 proto `Clipboard`）· M3 ✅（`client.rs` 补 A7：`test_connection`/`probe`/`first_reachable`，复用 proto `ordered_urls`，trustInsecureCert 接线 probe/test、epoch 透传回带 `ProbeReport`，53 测试绿、iOS-sim 通过）· M4 ✅（E/F 区：Rust 拥有持久化 blob 字节——proto 新增 `app_settings`/`server_config`/`history_log`/`loop_guard`/`payload_cache`/`file_state`/`persist_keys` 7 模块 198 测试；trustInsecureCert 补生产客户端构造期+setter，55+1 测试绿、iOS-sim 通过）· M5 ✅（C 区 SyncEngine 决策核：proto 新增 `sync_engine` 模块——reducer 形态 `SyncRuntimeState` plain struct + plan/commit 纯转移函数；server-wins 路由、去重守卫三件套、loop-guard 接线、退避/节奏、history due/cold-start、epoch 校验；网络 I/O + 1Hz 调度 + UIPasteboard 留原生；proto 246 测试绿、iOS-sim 通过，FFI 镜像延后 M6）· M6 ⏳

## 0. 一句话定位

把 uc-ios `Shared/`（Network/Models/Cache）里「给定输入 → 确定输出」的纯逻辑迁入共享 Rust crate，iOS（未来 Android）经 UniFFI 调用，**验收标准 = 回归清单全绿**。UI、剪贴板 I/O、SSID 探测、扩展壳全部留原生。

## 1. 目标拓扑（在 spike 产物上生长，不另起炉灶）

```
crates/
├── uc-mobile-proto    ← 纯编解码叶子（现有 connect_uri + 本方案 M1 扩容）
│     新增：wire 模型(Clipboard/HistoryRecord)、sha256 大写 hex、
│     长文本溢出、multipart builder、ISO-8601、URL 分类/SSID 归一/排序
│     deps 只准加：sha2、unicode-segmentation（字素计数）—— 仍零内部依赖
│
└── uc-mobile          ← FFI 边界（现有 client.rs 扩容）
      M2+：完整 HTTP 客户端(A6)、ConnectionTester(A7)、
      SettingsStore/watermark/loop-guard 逻辑、SyncEngine 决策核
      I/O 一律 snapshot 经 PlatformBridge，不在 async 内回调原生
```

**单一真相收敛（顺手但单列 commit）**：daemon 的 `sync_doc.rs::SyncClipboardDoc`（server 侧）改为依赖 `uc-mobile-proto` 的规范 wire 类型，消除 Rust 侧两份 serde 定义（spike 期间 uc-mobile 里的 `WireDoc` mirror 同时收敛）。TS / 旧 iOS 实现的漂移仍靠 golden vector 锁。

## 2. Oracle 策略（先于一切端口工作敲定）

字节兼容是 #1 回归风险，而 **真实 daemon 对一半端口是假 oracle**（history query/PATCH 是兼容壳：patch 不读 body、version 硬编码 0、无 409、无 modifiedAfter，`routes.rs:15-16`）。按端口分三类：

| 端口 | oracle | 手段 |
|---|---|---|
| SyncClipboard.json get/put、file get/put、Basic Auth | ✅ 真实 daemon | 🔗 e2e（B2 编排脚本 `run-b2-daemon-demo.sh` 直接复用扩展） |
| connect-uri、hash、长文本溢出、multipart、HistoryRecord 编码 | ✅ iOS 现有 golden vector / 单测 | 🧬 把 uc-ios 仓库的测试向量 **原样移植** 成 Rust 测试（M0），iOS 实现是规范源 |
| history version/409、PATCH `isDelete`、modifiedAfter | ⚠️ 二者皆不可靠 | 从 iOS 真机/官方 SyncClipboard server 抓字节 fixture 入库（`crates/uc-mobile-proto/tests/fixtures/`）；daemon 兼容壳修复另开 issue，不阻塞本迁移 |

## 3. 里程碑（每个独立可验收、可暂停）

### M0 · 契约先行：golden vector 全量移植（小，先做）— ✅ 完成
把 checklist A 区的跨语言向量移植成 Rust 测试：connect-uri（已有 ✅）、Clipboard JSON nil 省略、composite/split id、sha256 大写、10240 字素溢出、multipart CRLF/quoted、ISO-8601 四种组合、Basic Auth。
**验收**：A 区每条 🧬 都有对应 Rust 测试，fixture 来源注明。
**结果**：向量直接嵌入各模块测试（不走 `#[ignore]`，一次到位），fixture const 与 `/tmp/uc-ios/docs/examples/` 程序化逐字节比对。

### M1 · uc-mobile-proto 扩容：纯编解码全集（中）— ✅ 完成
A2/A3/A4/A5 + B 区纯逻辑：wire 模型、hash、长文本溢出（字素计数用 unicode-segmentation）、multipart builder、TypeMask、ISO-8601、URL 分类、SSID 归一、Layer-1 形态排序、try-order、`isDelete`/`isDeleted` 封装 helper。
**验收**：M0 测试全绿；~~daemon `sync_doc.rs` 改依赖 proto 类型~~。
**结果**：5 模块落地，140 测试绿。`ClipboardKind`/ISO-8601 跨模块重复已收敛到单一真相。**遗留单列**：daemon `sync_doc.rs` + uc-mobile `WireDoc` 改依赖 proto 规范类型（§1「单一真相收敛」）——拆到独立 commit，不阻塞 M2。核查另发现 `ServerConfig` Codable 持久化迁移属 M4，本里程碑只做形态分类纯函数（见回归清单 B 末项）。

### M2 · uc-mobile HTTP 客户端补全（中）— ✅ 完成
在 B2 `client.rs` 基础上补 A6 全集：history query（multipart POST）/history data 端点、base-url 归一、文件名前置校验、状态映射表（200/201/204、401、404、5xx、其余 4xx）、重试语义（仅首遇 connection-lost/timeout，300ms 一次，401/404 不重试）、`cancel_in_flight` 后续请求立抛 cancelled。
**验收**：A6 全条（mock 单测 + 真实 daemon e2e 跑 doc/file 端点）；缝 3 drop 测试扩展到新端点。
**结果**（2026-06-12，`cargo test -p uc-mobile` 29 绿、clippy/fmt 干净、iOS-sim 交叉编译通过）：
- 新增 FFI 端点 `get_file`/`put_file`/`query_history`/`get_history_payload`，复用 proto `Clipboard`/`HistoryQuery`/`HistoryRecord` 编解码；`get_latest`/`put_clipboard` 重构走同一套 `send_with_retry` + `check`。
- 新 FFI 镜像类型 `HistoryQuery`/`HistoryRecord`（时间戳 = epoch 毫秒 `Option<i64>`，用户拍板）。`SyncError` 重做：新增 `NotFound`/`ServerError{status}`/`ProtocolError{status}`/`DecodingFailed{reason}`，删 `Http`/`Protocol{reason}`，逐字节对齐 Swift `SyncError.Kind`。
- **单一真相收敛 #1a（client 侧，本里程碑顺手做）**：删 `uc-mobile` 的 `WireDoc`，`ClipboardMeta` 改 `into_proto`/`from_proto` 经 `uc-mobile-proto::Clipboard`（唯一 JSON 形态真相）。daemon 侧 `SyncClipboardDoc` 收敛因 PascalCase 别名 / `size` 恒在 / `hash` 不归一有回归风险，**另立 issue**（见 §1 + 下方"明确不做"）。
- **刻意偏离 Swift**：cancel **不永久 poison**（长生命周期/多 server/独占 runtime；用户 2026-06-12 拍板），`client.rs` 模块 docs + 回归清单 A6 记此决策。
- 缝 3 drop 测试保留（file→metadata 窗口原子）；新增 retry（timeout + RST mock）、状态映射全表、文件名/profileId 前置校验、no-poison、basic-auth 向量等测试。

### M3 · ConnectionTester（小）— ✅ 完成
A7：单 URL test、多 URL 并发 probe（2s 超时、404/401=可达）、`firstReachable` 按序确定性取首达（非竞速）。网络 epoch 由原生传入快照参数，Rust 不订阅系统事件。
**验收**：A7 + B 区 `preferredURLs` 全条单测绿。
**结果**（2026-06-14，`cargo test -p uc-mobile` 53 绿、clippy/fmt 干净、proto 140 不回归、iOS-sim 交叉编译通过、`aws-lc-rs` 仍不在依赖树）：
- `MobileSyncClient::test_connection`（单 URL，走完整 `get_latest_with` + 重试 + 解码，2xx 解码失败→unreachable）、`MobileSyncClient::probe`（多 URL，短 total timeout、**不重试**、status-only、`tokio::task::JoinSet` 单 runtime 线程并发扇出、`dedup_preserving_order`）、自由函数 `first_reachable`（纯，复用 M1 proto `ordered_urls` 形态序）。新增 FFI 类型 `ProbeResult`（Success/AuthFailed/Unreachable/MissingFields，404→Success 的可达语义集中于此）+ `ProbeReport{network_epoch,results}`。
- **用户拍板（2026-06-14）**：①`trustInsecureCert` M3 就为 probe/test 接线（`build_http_client` 加 trust 参数→`danger_accept_invalid_certs`；生产客户端 trust 仍 M4）；②epoch 不透明透传——`probe` 收 `network_epoch` 入参、随 `ProbeReport` 回带，**有效性校验属 M5**（M3 只盖戳）。
- API 形态：probe/test 作 `MobileSyncClient` 方法（复用 runtime + reqwest + `send_with_retry`/`check`/`map_status`），`first_reachable` 作纯自由 `#[uniffi::export]` fn——不另起独立 Object（会重复一套 runtime/init）。
- **遗留单列**：B 区「旧格式迁移」（legacy 单 `url`/`manualOverrideConfigId` 提升）仍属 M4 持久化层，本里程碑只动探测，不碰 `ServerConfig` Codable。

### M4 · 状态与持久化逻辑（中）— ✅ 完成
SettingsStore 键名/默认值/前向兼容（E 区）、watermark、history 去重 append（cap 200、direction 升级）、SyncLoopGuard 状态机、PayloadCache 的 LRU 索引决策（驱逐 **决策** 在 Rust，文件读写/原子写由原生按决策执行——snapshot in、command out）。
**验收**：E/F 区 🔬 条目全绿；损坏 blob 返默认不阻塞启动。
**结果**（2026-06-14，proto 198 测试绿 / uc-mobile 55+1 绿、clippy+fmt 干净、iOS-sim 交叉编译通过、`aws-lc-rs` 仍缺席）：
- **边界模型（用户 2026-06-14 拍板）**：Rust **拥有持久化 blob 字节**（decode/encode），原生退化为字节搬运 + 文件 I/O + 提供 UUID/`Date.now()`。同 M1 先例，proto 里程碑=纯逻辑 + 测试；FFI 镜像随 M5 决策核 Rust 内部消费 + M6 原生接入再暴露。
- proto 新增 7 模块（58 测试）：`app_settings`（17 字段 + `rename_all=camelCase` + 容器 `default` 前向兼容 + appearance 未知→system）、`server_config`（双写 `url`+`urls`、§5.5 legacy 迁移、§5.2 `manualOverrideConfigId` 提升、`load_servers`→`{list,migrated}`）、`history_log`（`append_history` 头去重/`.local` 升级/cap 200/newest-first + `touch` + **timestamp Double-since-2001 + UUID 大写串** 字节忠实 serde）、`loop_guard`（纯函数 over `Vec<Event>`：record 大写归一/空忽略/window 淘汰 + tripped flip≥3）、`payload_cache`（`plan_eviction` mtime LRU + `is_valid_cache_key`）、`file_state`（watermark 复用 iso8601 / `normalize_synced_hash` / `live_urls` map）、`persist_keys`（键名/文件名跨平台单一真相）。
- **trustInsecureCert 补生产客户端**（用户拍板「构造期 + setter」）：`MobileSyncClient::new(bridge, trust)` 构造期固定、`http` 字段改 `RwLock<reqwest::Client>`、新增 `set_trust_insecure_cert` 热切换（swap reqwest client 不重启 runtime 线程）；2 个新冒烟测试。
- **明确不做（留 M6）**：持久化 FFI 镜像（settings/history/cache 的 uniffi Record + 导出）、文件原子写/跨进程可见性 e2e、PayloadCache 文件 I/O + semaphore 去重（驱逐决策已 Rust）。

### M5 · SyncEngine 决策核（大，最后做，先拆层再迁）✅（2026-06-14）
968 行状态机不整体搬。拆两层：
- **决策核（进 Rust）**：原计划 `fn decide(tick_input) -> Vec<SyncAction>`；通读 `SyncEngine.swift` 后发现 tick 内决策与网络 I/O 深度交织（`getClipboard` 结果决定路由 → apply/push 又是后续 I/O → I/O 后才 commit 守卫/loop-guard），单次纯 `decide` 覆盖不了。**用户 2026-06-14 拍板改为 reducer 形态**：proto `sync_engine` 模块暴露 `SyncRuntimeState` plain struct（caller 持有，同 `loop_guard` 惯例）+ 纯转移函数——`plan_preamble`（早退/记 local/退避门/cross-process resync）、`plan_after_server_get`（truth-gate / server-new 路由 / push 决策，语义动作 enum `ServerRoute`/`PushDecision`）、`commit_*`（apply/push/converged/consent_push/stage/tick_success/tick_failure 折叠 I/O 结果回 state）；纯函数 `backoff_secs`(jitter 入参)/`cadence_secs`/`is_history_sync_due`/`is_cold_start`/`advance_watermark`/`is_probe_conclusion_valid`/`hashes_equal`；公开转移 `mark_staged_applied`/`acknowledge_loop_detection`/`reset_runtime_state`/`handle_active_server_changed`/`handle_network_route_changed`。
- **执行壳（留原生）**：网络 I/O（getClipboard/apply 写板验 hash/push PUT/history 分页 walk）、tick 调度（1Hz/5s/暂停）、scenePhase、UIPasteboard 读写、banner、prefetch、`last_synced_hash`/`last_history_sync_ms` 持久化 I/O。
- **交付边界**：proto-only 纯逻辑 + 单测，**不暴露 uniffi**，FFI 镜像延后 M6（同 M4 先例，用户拍板）。
- **结果**：proto `sync_engine` 模块 + 48 新测试（proto 共 246 绿），clippy/fmt clean，iOS-sim 交叉编译通过，aws-lc-rs 仍缺席。移植中识别 Swift `maybePush` push 路径 trip 被 line 756 无条件 `state=.succeeded` 覆盖的怪异（apply 路径顺序相反无此问题），M5 先忠实移植 + 标注；**M6 已修复**（用户拍板）：Rust `commit_push` 改走 `record_and_check`、Swift `maybePush` 同步重排，push trip 现与 apply 路径一致 stick 成 `LoopDetected`。
**验收**：C 区 🔬 条目以决策核单测覆盖（✅）；🔗 条目决策逻辑进 proto 并单测、daemon e2e 留 M6（[~]）；📱 条目决策部分进 proto、执行壳留 M6（[~]）。

### M6 · uc-ios 接入与灰度（跨 repo）
xcframework 经 SPM binaryTarget 进 uc-ios；**feature flag 双路径**（原生/Rust 各保完整路径，A/B 定位回归来源——checklist 执行建议 #3）；按 M1→M5 的顺序逐模块切换，每切一个模块过一遍对应 📱 清单；三进程上下文 TLS 验收（spike 遗留）在此补。全绿后删原生路径（不留无限期双实现）。
**验收**：回归清单逐条附验证者/日期/证据；双路径删除 PR 合并。

### 持续项（不单列里程碑）
- CI：交叉编译 + bindgen drift 检查 + 体积预算 + aws-lc-rs 断言（脚本已有，搬进 workflow）。
- uniffi/toolchain 版本钉死不变（=0.31.1 / 1.95.0），升级单独评估。

## 4. 明确不做

- P2P / Transport 抽象 / iroh / 加密栈（mobile-sync 是明文 HTTP + Basic Auth，引入加密栈只会拖重 crate）
- 键盘/分享/Intents 的 UI 壳与系统钩子、剪贴板 I/O、SSID 平台 API（永留原生）
- daemon history 兼容壳的功能补全（version/409/modifiedAfter）——另开 issue，是服务端工程不是迁移工程
- **daemon 侧 `SyncClipboardDoc` 收敛到 proto `Clipboard`（单一真相 #1b）**——M2 已收敛 client 侧；daemon 侧改依赖 proto 类型需逐一保住 PascalCase 别名（兼容 iOS Shortcut 大小写）、`size` 恒序列化（默认 0）、`hash` 不归一三处语义，并补 daemon 回归测试，**另立独立 issue**，不混进本迁移线（用户 2026-06-12 拍板：只收敛 client 侧）
- Android 客户端实装（crate 按 iOS+Android 共享设计，但 Kotlin binding 与 Android 接入不在本方案）

## 5. 风险与对策

| 风险 | 对策 |
|---|---|
| 字素 vs 字节 vs code point（10240 阈值） | unicode-segmentation + 专门 golden vector（emoji/组合字符用例） |
| `isDelete`/`isDeleted` 写错 | proto 层 helper 封装，clippy 禁裸字符串（grep CI 检查） |
| 假 oracle 端口拿不到 fixture | M0 阶段就抓真机字节；抓不到的端口降级为「iOS 单测向量为准」并在清单上标注 |
| SyncEngine 拆层后行为漂移 | 决策核输入输出全部可序列化，原生侧录制真实 tick 快照回放进 Rust 测试 |
| uc-ios 双路径维护拖长 | 每个模块切换后两周内删原生路径；删除是 M6 验收项不是可选项 |

## 6. 与现有文档的关系

- 验收唯一标准：`uc-ios-regression-checklist.md`（本方案的里程碑↔清单分区映射：M0/M1↔A、M3↔A7+B、M4↔E/F、M5↔C、M6↔D/G–L）。
- 行为语义查询：`uc-ios-feature-inventory.md`。
- 管道与执行模型（runtime/缝 1/2/3）：`uc-mobile-spike-plan.md`，本方案不重复。
