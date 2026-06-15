# uc-ios 零回归验收清单

> 配套 `uc-ios-feature-inventory.md`。用途：Rust crate 替换原生逻辑后，**逐条勾选**；全绿才算「无回归」达标。
> 验证手段图例：
> - 🧬 **golden** = 跨语言黄金向量单测（Rust 输出须与 iOS/桌面字节相等）
> - 🔬 **unit** = Rust 单元测试可覆盖
> - 🔗 **e2e** = 必须连 **真实桌面 daemon** 跑端到端（字节兼容只能这样证）
> - 📱 **device** = 真机/模拟器手动验证（涉及系统 API/UI/扩展）
>
> 🔴 = 字节级关键项，错一字节即回归，**优先级最高**。

---

## A. 协议与编解码（Rust 共享核心 · 字节关键）

> ✅ M0/M1 完成 2026-06-12（`cargo test -p uc-mobile-proto`，140 测试全绿；4 区均经独立对抗 agent 逐字节核查）。下方 A1–A5 + B 区编解码项已由 Rust golden vector / 单测覆盖；🔗（真实 daemon）项属 M2、📱 项属 M6，仍留空。
> ✅ **A1 connect-uri 📱 M6 真机验收通过 2026-06-15**：M6-0b 把 connect-uri 解析经 `ConnectURIRouter` 灰度到 Rust core（运行时 toggle）。真机（iPhone 16 Pro / iOS 27）翻开关扫码，Console 实测 `ConnectURIRouter: parsing connect URI via Rust core`（进程 UniClipboard，18:37:31），解析正常、与原生结果一致（iOS A/B 单测 `nativeAndRustAgreeOn*` + 真机日志双证）。

### A1. connect-uri
- [x] 🧬🔴 解析 `uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url>`，golden vector 与 iOS/桌面字节相等 — B0/B1 `connect_uri.rs`
- [x] 🧬🔴 base64url-no-pad：`-`↔`+`、`_`↔`/`，解码前补 `(4-len%4)%4` 个 `=` — `connect_uri.rs`
- [x] 🔬 required 字段缺失/空/null → `missingField`；非 http(s) → `invalidURL`；svc≠mobile-sync → `unsupportedService`；v≠1 → `unsupportedVersion`
- [x] 🔬 `urls` 缺省/全过滤后回落 `[url]`（回落属调用方，FFI 契约返回过滤后列表）；`o` 中未知字符串键保留、非字符串值丢弃；非 http(s) 与非字符串 urls 候选丢弃 — **M6 补齐**：proto `de_lenient_string_map`/`de_lenient_url_list`，测试 `parse_drops_non_string_o_values`/`parse_ignores_non_object_o`/`parse_filters_non_http_urls_candidates`/`parse_urls_all_non_http_becomes_empty`/`parse_drops_non_string_urls_entries` + iOS A/B `nativeAndRustAgreeOn*`。（M0/M1 误标已覆盖，实为 strict 解析与原生防御式行为不符；M6 tracer-bullet A/B 暴露并修复，保全零回归）
- [x] 🧬 错误码/文案与 spec §4.2 表一致（**文案是跨语言契约**）

### A2. SyncClipboard 线模型（Clipboard / HistoryRecord）
- [x] 🧬🔴 `Clipboard` JSON 字段名：`type/hash/text/hasData/dataName/size`，nil 字段 **整字段省略**（不写 null） — `clipboard_doc.rs`
- [x] 🔬 `type` 枚举原值 `Text/Image/File/Group` — `clipboard_doc.rs`
- [x] 🧬🔴 `HistoryRecord` composite id = `"<type>-<hash>"`（大写） — `history_record.rs::composite_profile_id`
- [x] 🔴 §2.10 PATCH 用 split id `<type>/<hash>`（**不同于** composite） — `history_record.rs::split_patch_id`
- [x] 🔴 PATCH body 用 `isDelete`（无 d）；读/创建用 `isDeleted`——封装 helper 防写错 — `HistoryRecordPatch`（`isDelete` 仅出现一处生产代码）
- [x] 🔬 `hasData/starred/pinned/isDeleted` 无条件编码；`text` 仅非空时编码（核查订正：Swift `encodeIfPresent` 仅按 nil 判定，空串也编码——已按 Swift 对齐）
- [x] 🔬 ISO-8601 日期：能读 `Z` 与 `+00:00`、含/不含小数秒四种组合（并显式拒绝 chrono 比 Swift 宽松的小写 `t`/`z`/空格分隔）
- [ ] 🔬 version 生命周期：创建=0，每次 PATCH +1，stale 版本 server 返 409 — 创建=0/递增语义已落（`INITIAL_VERSION`+doc），**409 是服务端行为，且 uc-ios 尚无 PATCH DTO**，留 M2/服务端

### A3. 哈希
- [x] 🧬🔴 SHA-256 **大写** hex；文本 hash = sha256(utf8(text))；文件/图片 hash = sha256(原始字节)，**文件名不参与** — `hash.rs::sha256_hex_upper`
- [x] 🔬 hashMatches：expected 为 null/空 → 永真；否则大小写无关相等 — `hash.rs::hash_matches`

### A4. 长文本溢出（§3.4）
- [x] 🧬🔴 阈值 **10240 字符**（`String.count` 字素，**非字节**） — `clipboard_doc.rs`（unicode-segmentation，含 ZWJ/组合字符向量）
- [x] 🔴 溢出：`text`=前 10240 字符预览，`hasData=true`，`dataName="text_{HASH}.txt"`，payload=全文 utf8，`size`=全文长度，hash over 全文
- [x] 🔬 publishImage：`dataName="image.{ext}"`、`text=dataName`、hash=bytes
- [x] 🔬 publishFile：文件名经 `sanitizedFilename`（剥 `/`、`\`，空回落 "file"；核查修正为按字素而非字节查分隔符）

### A5. multipart（§2.7 查询 / §2.9 创建）
- [x] 🧬🔴 行终止符一律 `\r\n`，边界 `--{b}\r\n`、结束 `--{b}--\r\n` — `multipart.rs`（verdict byte-exact）
- [x] 🔴 quoted：`\`→`\\`、`"`→`\"`，丢弃 CR/LF
- [x] 🔬 字段编码：page/types 十进制串，日期 ISO-8601，bool `"true"/"false"`；**nil 字段不发**
- [x] 🔬 TypeMask 位：Text=1 Image=2 File=4 Group=8

### A6. HTTP 客户端

> ✅ M2 完成 2026-06-12（`cargo test -p uc-mobile`，29 测试全绿；逐条对照 Swift
> `SyncClipboardClient.swift` / `SyncError.swift` / `SyncClipboardClientTests.swift`
> 移植到 `crates/uc-mobile/src/client.rs`）。🔗 项以 in-process axum mock 做端到端验证
> （字节兼容已由 proto 层 golden vector 锁，HTTP 层只验状态/重试/取消/路径/认证接线）；
> 真实 daemon e2e 的 doc/put 已在 B2 跑通，新增 file/history 端点的真机 e2e 经
> `run-b2-daemon-demo.sh` 扩展跑（M2 未阻塞项，见迁移方案 §3 假 oracle 说明）。

- [x] 🔗🔴 Basic Auth = `base64(utf8(user + ":" + pwd))` — reqwest `.basic_auth`；`basic_auth_header_matches_spec`（alice:secret → `Basic YWxpY2U6c2VjcmV0`，与 Swift 同向量）
- [x] 🔗 base URL 归一：trim、补尾 `/`、校验 http(s)+非空 host — `normalize_base_url` + `endpoint`；`normalize_base_url_matches_swift`/`endpoint_normalizes_and_joins_paths`
- [x] 🔗 端点：GET/PUT SyncClipboard.json、PUT/GET file/{name}、POST api/history/query、GET api/history/{profileId}/data — 全部落地，复用 proto `Clipboard`/`HistoryQuery`/`HistoryRecord` 编解码
- [x] 🔬 文件名校验：空/含 `/`/含 `\` → 网络前即拒（profileId 同规则） — `validate_path_component`；`file_endpoints_reject_bad_filenames_before_network`/`get_history_payload_rejects_bad_profile_id`（断言 events 为空 = 未触网）
- [x] 🔗 状态映射：200/201/204=成功，401=authFailed，404=notFound，5xx=serverError，其余 4xx=protocolError — `map_status` 逐字节对齐 Swift `mapHTTPStatus`（202/206 等非 {200,201,204} 也归 protocolError）；`status_mapping_matches_swift`（全表）+ `get_latest_maps_http_statuses`（端到端）
- [x] 🔬 重试：仅首次遇 `.networkConnectionLost`/`.timedOut`，sleep 300ms 重试一次；401/404 不重试 — `is_retriable`（timeout 经 `is_timeout`；connection-lost 经 io `ConnectionReset`/`Aborted`/`BrokenPipe`/`UnexpectedEof`/`NotConnected` 源链遍历）+ `send_with_retry`；`retry_on_timeout_then_succeeds`/`retry_on_connection_reset_then_succeeds`（RST mock）/`status_errors_are_not_retried`
- [x] 🔬 取消：`cancel_in_flight` 中止在途请求（观测 `.cancelled`），300ms 重试因 task abort 自然不触发 — `cancel_in_flight_yields_cancelled`。**刻意偏离 Swift**：长生命周期、多 server、独占 runtime 的 Rust client **不永久 poison**——`cancel` 后的新请求（原生壳带新 `ServerConfig`）正常工作，避免每次网络切换重建 client+ 重起 runtime 线程；用户 2026-06-12 拍板，`cancel_does_not_poison_subsequent_requests` 守此决策（详见 `client.rs` 模块 docs）

### A7. 连通性探测（§5.3 Layer 2）

> ✅ M3 完成 2026-06-14（`cargo test -p uc-mobile`，53 测试全绿；逐条对照 Swift
> `ConnectionTester.swift` / `ConnectionTesterProbeTests.swift` 移植到
> `crates/uc-mobile/src/client.rs`）。探测路径 **刻意复用** A6 的 `endpoint`/
> `map_status`/`.basic_auth`，但 **状态语义不同**：probe/test 把 404 当「可达 - 但空」
> → `Success`（A6 主客户端把 404 当 `NotFound` 错误）。`trustInsecureCert` 用户拍板
> M3 就为 probe/test 接线（各自构建客户端时 `danger_accept_invalid_certs`；生产客户端
> 的 trust 仍属 M4/E 区）。`network_epoch` 用户拍板做不透明透传：`probe` 收 epoch 入参、
> 随 `ProbeReport` 回带（结论的有效性校验属 M5 SyncEngine，M3 只盖戳）。

- [x] 🔬 单 URL test：200/404→success，401→authFailed，其余→unreachable — `test_connection`（走完整 `get_latest_with` + 重试 + 解码；2xx 解码失败→unreachable，对齐 Swift catch-all）；`test_connection_{success_on_200,404_is_success,wrong_password_is_auth_failed,server_error_is_unreachable,decode_failure_is_unreachable,{blank_url,missing_fields}→missing_fields,malformed_url_is_unreachable}`
- [x] 🔬 多 URL probe：2s 超时并发，404/401=可达，`waitsForConnectivity=false`（reqwest 默认不等连通性，无需额外配置） — `probe`（per-request 短 total timeout、**不重试**、status-only、`JoinSet` 单线程并发扇出）；`probe_{maps_status_per_candidate,targets_syncclipboard_json_with_basic_auth,empty_credentials_all_missing_fields_without_network,empty_list_returns_empty_with_epoch,malformed_url_unreachable_blank_missing_fields,dedupes_repeated_candidates,times_out_to_unreachable,trust_insecure_still_works_over_plain_http}`
- [x] 🔬 `firstReachable` 按 orderedURLs 顺序取首个可达（确定性，非竞速） — `first_reachable`（纯函数，复用 M1 proto `ordered_urls` 形态序）；`first_reachable_{skips_unreachable_head,auth_failed_counts_as_reachable,order_decides_when_both_reachable,none_when_nothing_reachable,missing_entry_is_not_reachable}` + 端到端 `probe_then_pick_{chooses_first_reachable_in_shape_order,over_live_mocks}`

---

## B. 网络分类与多服务器（§5.1–5.3）

- [x] 🔬🔴 URL 分类网段：LAN=10/8·172.16–31/12·192.168/16·169.254/16；TS=100.64.0.0/10；`*.ts.net`→TS；`*.local`→LAN；其余→WAN — `net_class.rs::classify_url`（核查补齐 host 字符校验 + 百分号解码，对齐 Foundation；UTS-46 fullwidth 点为已记录的可接受残差）
- [x] 🔬 SSID 归一：trim、剥外层引号、`<unknown ssid>`/`0x` → nil — `net_class.rs::normalize_ssid`
- [x] 🔬 Layer 1 形态排序确定性（无 I/O，稳定排序保留同类内发布序） — `net_class.rs::ordered_urls`
- [x] 🔬 try-order：Wi-Fi=[lan,ts,wan]；非 Wi-Fi+TS=[ts,wan,lan]；蜂窝=[wan,ts,lan]；无信号=保持原序 — `net_class.rs::class_preference`
- [x] 🔬 `activeConfig` 解析：stale id 回落 configs[0]；空列表→nil — `net_class.rs::resolve_active_index`
- [x] 🔬 `preferredURLs(live:)`：live 有效且在当前 urls → 提头；失效 → 忽略回落形态序 — `net_class.rs::preferred_urls`
- [x] 🔬 旧格式迁移：legacy 单 `url`、`manualOverrideConfigId` 一次性提升为 activeConfigId；不回写旧键 — M4 落地（proto `server_config`：`load_servers`→`{list,migrated}` + `ServerConfigList` decode 提升 pin）；测试 `server_config::tests::{load_servers_migrates_legacy_only, load_servers_new_key_wins_over_legacy, load_servers_corrupt_new_key_returns_empty, promotes_resolvable_legacy_pin, unresolvable_pin_falls_back_to_active, decodes_legacy_single_url}`

---

## C. 同步编排（SyncEngine · 行为关键）

- [~] 🔗 server-wins：每 tick 先处理 server，再 device（M5 决策核：`sync_engine::plan_after_server_get` 路由顺序 truth-gate→server-new→push；测试 `route_*` + `server_wins_then_dedup_short_circuits_push`。e2e 真实 daemon 留 M6）
- [~] 🔗 auto-apply ON（默认）：server hash 新 → 取字节验 §4.4 hash → 写 pasteboard → 进 watermark（M5：`ServerNewPlan{will_apply}` + `commit_apply`；测试 `route_server_new_when_hash_differs_from_synced`/`commit_apply_advances_guards_and_succeeds`。取字节/验 hash/写板=原生 I/O，e2e 留 M6）
- [~] 📱 auto-apply OFF：暂存 `.hasNewUnwritten`，不取字节，显 banner（M5 决策核：`will_apply=false`→`commit_stage`→`HasNewUnwritten`，测试 `server_new_auto_apply_off_does_not_apply`/`commit_stage_sets_has_new_unwritten`。banner=原生，留 M6）
- [~] 🔗 push：仅当 server hash==synced 且 device hash 新于 `lastSyncedContentHash`/`lastAppliedContentHash`（M5：`plan_push`，测试 `push_already_synced`/`push_self_written_guard_blocks_reapplied_content`/`route_push_when_server_unchanged`。e2e 留 M6）
- [x] 🔬🔴 去重守卫三件套：`lastSyncedContentHash`（防重 pull）、`lastAppliedContentHash`（防刚写内容被 push）、history 同 hash 去重并升级 direction（M5：guard #1/#2 在 `plan_after_server_get`/`plan_push`，测试 `push_self_written_guard_blocks_reapplied_content`/`server_wins_then_dedup_short_circuits_push`；history 去重升级 = M4 `history_log::append`）
- [~] 🔗 历史增量：冷启仅取 page 1 播种 watermark；增量用 `modifiedAfter`（严格 `>`）分页至空数组（M5 决策核：`is_history_sync_due`/`is_cold_start`/`advance_watermark`，测试 `history_sync_due_*`/`cold_start_when_no_watermark`/`watermark_advances_only_forward`。分页 walk=原生 I/O，e2e 留 M6）
- [x] 🔬 loop guard：同 hash apply/push 翻转 ≥3 次（30s 窗口）→ trip；reset 后恢复（M5：`commit_apply`/`commit_push` 接 M4 `loop_guard`，测试 `apply_final_trip_shows_loop_detected`/`push_path_trip_shows_loop_detected`/`acknowledge_loop_detection_clears_buffer_and_idles`。✅ M6 已修 Swift `maybePush` push 路径 trip 被 line 756 覆盖回 succeeded 的怪异：Rust `commit_push` 改走 `record_and_check`、Swift 同步重排，push trip 现与 apply 路径一致 stick 成 `LoopDetected`）
- [x] 🔬 网络 epoch：路径变更自增，probe 结论仅 epoch 未变时有效（M5：`is_probe_conclusion_valid`，测试 `probe_conclusion_valid_only_for_same_epoch`；M3 `ProbeReport` 盖戳）
- [~] 📱 tick 频率：前台 1Hz、inactive 5s、后台暂停、离线退避 5→60s+±20% jitter、历史节流 30s（M5 决策核：`cadence_secs`/`backoff_secs`（jitter 入参）/`SyncConfig` 默认值，测试 `cadence_active_inactive_and_paused`/`backoff_doubles_and_caps`/`backoff_applies_jitter`。1Hz 调度循环/后台暂停=原生，留 M6）
- [~] 📱 网络变更：取消在途、清退避、清 lastApplied、nil liveURL、reconcile server、重 probe（M5 决策核：`handle_network_route_changed`（清退避）/`commit_tick_failure` kick_probe，测试 `handle_network_route_changed_clears_backoff`/`tick_failure_network_backs_off_and_kicks_probe`。取消在途/nil liveURL/reconcile=原生 I/O，留 M6）

---

## D. 剪贴板 I/O（留原生，但行为须不变）

- [ ] 📱 两级访问：免提示层（changeCount+has*）vs 内容层（可能弹"允许粘贴"）
- [ ] 📱🔴 图片优先级 PNG>HEIC>JPEG>GIF，用 `data(forPasteboardType:)` 保 §4.2 hash（不经 UIImage）
- [ ] 📱 echo 守卫：lastWriteChangeCount / lastWrittenContentHash / lastAppliedContentHash / lastConsumedChangeCount
- [ ] 📱 consent-push（默认，PasteButton 免提示）vs auto-push（opt-in，tick 读剪贴板弹窗）
- [ ] 📱 `activate()` 推迟首次真实读，冷启不弹窗

---

## E. 设置项（§5.4）

- [x] 🔬 默认值：`autoApplyServerChanges=true`、`autoPushDeviceChanges=false`、`trustInsecureCert=false`、`prefetchAttachments=true`、`prefetchOnCellular=false`、`payloadCacheMaxBytes=200MB`、`appearance=system`、键盘音/触感=true（M4：proto `app_settings::tests::defaults_match_spec_table`）
- [x] 🔬 前向兼容：缺失键填默认、未知键容忍、未知 appearance 回落 system（M4：proto `app_settings::tests::{partial_json_fills_missing_with_defaults, unknown_keys_are_tolerated, unknown_appearance_falls_back_to_system, decode_empty_or_corrupt_returns_defaults}`）
- [~] 📱 各 toggle 实际行为：trustInsecureCert 影响 TLS 校验（M4 已接生产客户端：`MobileSyncClient::new(trust)` 构造期固定 + `set_trust_insecure_cert` 热切换，测试 `production_client_built_with_trust_drives_plain_http`/`set_trust_insecure_cert_swaps_client_and_keeps_working`）；autoApply/autoPush/prefetch* 门控属引擎行为 → M5/M6 原生接入验

---

## F. 持久化与跨进程（App Group）

- [x] 🔬 持久化键名与桌面/Android 共用：`server_config_list`、`app_settings`、`clipboard_history` 等（M4：proto `persist_keys`（`keys`/`files` 常量）+ `persist_keys::tests::key_names_match_swift`）
- [~] 🔗 文件原子写跨进程：`last_synced_hash`、`last_known_ssid`、`live_urls`（JSON map）——值的 **字符串/字节形态** 已 Rust 单一真相（`file_state::{normalize_synced_hash, parse/format_watermark, decode/encode/update_live_urls}` + `net_class::normalize_ssid`，round-trip 测试）；原子写/跨进程可见性属原生 I/O → M6 真机 e2e 验
- [x] 🔬 history 去重 append：同 hash 在头不重插、`.local` 升级为 pushed/pulled、cap 200、newest-first（M4：proto `history_log::tests::{append_inserts_newest_first, append_dedups_same_hash_at_head, append_upgrades_local_head_to_directional, append_local_does_not_downgrade_directional_head, append_caps_oldest_dropped, touch_*}`；timestamp/UUID 字节忠实：`timestamp_serializes_as_seconds_since_2001`/`decodes_swift_reference_date_double`）
- [x] 🔬 watermark：`loadHistoryWatermark`/`saveHistoryWatermark`、节流时间戳（M4：proto `file_state::tests::{watermark_round_trips_to_millisecond, watermark_accepts_plain_iso_without_fractional, watermark_corrupt_or_empty_is_none}`）
- [x] 🔬 损坏策略：缺失/不可解码 blob 返默认，永不阻塞启动（M4：各 `decode_*` corruption 测试 — app_settings/server_config/history_log/file_state）
- [~] 📱 PayloadCache：LRU 按 mtime 驱逐、200MB cap、原子写、backup-excluded、并发 fetch 去重（semaphore=3）——驱逐 **决策** 已 Rust（`payload_cache::{plan_eviction, is_valid_cache_key}` + 测试镜像 Swift LRU/setMaxBytes/invalidKey）；文件 I/O、原子写、backup-excluded、semaphore 去重留原生 → M6

---

## G. 生命周期

- [ ] 📱 启动：load servers/settings/history/watermark → pasteboard observer 推迟读 → SSID provider → engine → 升级守卫 → 发布 SSID
- [ ] 📱 scenePhase：active（合并扩展历史/refresh SSID/强制重探/恢复 1Hz）、inactive（节流保活）、background（stop）
- [ ] 📱 冷启分支：空配置→SetupFlow；空配置且未 onboard→Onboarding；老用户直达 home

---

## H. 主 App UI（留原生 · 表层回归）

- [ ] 📱 Home：两列网格 newest-first、搜索（文本/文件名）、类型/日期筛选、多选批量（复制/分享/删除）、下拉刷新、context menu、tap 重应用、长按预览
- [ ] 📱 Settings：服务器列表、各 toggle、缓存档位（50/200/500/1000MB）+ 清理、主题、功能引导回看
- [ ] 📱 服务器管理：增/删/改、多 URL（去重）、shuffle 名、测试连接（并发 probe 取首达）、QR 扫描、滑删 + 切换
- [ ] 📱 Setup/Onboarding：QR 或手填、测试连接 gate、首 run 走查、post-pairing 解锁卡片
- [ ] 📱 ConnectImportSheet：掩码预览、追加为新服务器

---

## I. 键盘扩展

- [ ] 📱 门控 `.ok`/`.needsFullAccess`（去设置）/`.noServer`（去主程序加服务器）
- [ ] 📱 上行：读 pasteboard→上传，**watermark 先于 metadata PUT 写**，图片入 App Group
- [ ] 📱 下行：GET 最新→入历史去重
- [ ] 📱 卡片：text/link/image（file/group 过滤）；link 检测 http(s)+host；图片走 ImageIO 缩略图（~48MB 预算）
- [ ] 📱 动作：文本 insertText 直插；图片复制到 pasteboard + "已复制长按粘贴" toast；text 溢出先取文件验 hash 再插
- [ ] 📱 changeCount ~1.2s 轮询自动上行；NWPathMonitor 自动切换；行内服务器切换
- [ ] 📱 键盘：空格/回车（按 returnKeyType 变标签）/退格 hold 加速重复/地球键；音 + 触感受设置门控
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
3. **D–L 的 📱 项** 在迁移收尾阶段真机过一遍；过渡期保留原生/Rust 双路径 feature-flag，回归可 A/B 定位来源。
4. 每条勾选附「验证者 / 日期 / 证据（测试名或截图）」，避免口头达标。
