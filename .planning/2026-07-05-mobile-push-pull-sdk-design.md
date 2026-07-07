# 移动端 push/pull 同步 SDK 设计（逻辑闭环 · 防回环）

状态：**v0.4（PR-A 已实现，2026-07-05；PR-B 2026-07-06 追加实现，见下）**。用户拍板跳过
codex 对抗评审直接批准 PR-A 实现。
`MobileSyncEngine` 落地在 `crates/uc-mobile/src/engine.rs`（单测在同文件的 `tests` 模块）。
RN 交接指南（PR-C）：`.planning/2026-07-05-mobile-push-pull-sdk-rn-integration-guide.md`。
实现期发现并修正的两处与本文档描述不符之处：persist_keys 的 contentId 键（§7 表格误指向
`LAST_SYNCED_CONTENT_HASH`，实为需要新增 `LAST_SYNCED_CONTENT_ID`）、4 个生命周期方法从
草图的同步签名改为 `async`（`blocking_lock()` 在 tokio 上下文里会 panic）。

> **2026-07-06 补丁：§10 的"PR-B 预期零改动"假设被推翻，且改动范围超出了 PR-B 的界定
> （不只 `uc-mobile-proto`，还动了 `uc-webserver`）。** 起因：测试覆盖 PR-A 遗留的两个
> 空白点（`File` 类型 push/pull 零覆盖；push 后服务端对 Image/File 做无损再编码——同
> `contentId`、不同 `hash`——的 drift 场景零覆盖）时，写出的特征测试证实了一个真实、可
> 复现的问题：`commit_push` 无条件清空 `last_synced_content_id`（§7 原设计），叠加
> `PUT /SyncClipboard.json` 的响应本来就是裸 200 空 body，导致 push 后的下一次 pull
> 完全没有 `contentId` 可以 dedup，会把服务端的再编码结果误判成"服务端有新内容"重新
> 下载、覆盖用户刚拷贝的内容。修复方案（唯一安全的方案——纯客户端追加一次 verify-GET
> 会引入与其他设备并发写入的竞态误判窗口，比现状更危险，故排除）：
> `PUT /SyncClipboard.json` 成功响应从裸 200 改为可选携带
> `{"contentId": "..."}`（`uc-webserver`，新增字段、向后兼容）；`commit_push` 新增
> `content_id: Option<&str>` 形参，有值就学、`None` 保持旧的清空行为
> （`uc-mobile-proto`）；`client.putClipboard` 返回值从 `()` 改为 `Option<String>`
> 把这个值带出来（`uc-mobile`）。三处都已实现 + 全绿（含 2 条 reducer 级 + 2 条 engine 级
> 新测试直接验证 drift 场景），FFI 面变更详见 RN 交接指南 §3.1。

范围：**只做移动端**（用户 2026-07-05 拍板）。桌面侧（`uc-application` 入站机器）不动。
落点：**本仓 `crates/uc-mobile`**。RN 侧 `uniclipboard-android` 的接线属他仓，本设计只产出接口契约 + 交接指南。

关联：
- 现状机器：`crates/uc-mobile-proto/src/sync_engine.rs`（纯 reducer）、`crates/uc-mobile/src/client.rs`（reqwest 网络原语）、`crates/uc-mobile/src/reducer.rs`（reducer 的 FFI 镜像，逐函数导出、state 按值传入传出）。
- SSE 推送：`.planning/2026-06-30-mobile-sync-sse-design.md`（notify-then-pull 红线；重连/生命周期归 native 的决策 5 已 codex 10 轮锁定）。
- contentId 去重：`.planning/2026-06-23-contentid-mobile-dedup-design.md`。
- content-availability 探针：`.planning/2026-07-05-content-availability-rn-integration-guide.md`（上一轮工作，§6.3 见其在 push 路径退役 + §12 处置）。

> **一句话**：把「决策 + 网络 I/O + 去重/防回环/watermark」收进一个 Rust deep module，对客户端只留 `push` / `pull` 两个原语。客户端保留它 **必须** native 做的：何时触发、读/写 `UIPasteboard` 的字节、UI/生命周期、SSE 连接调度。防回环与去重从此「客户端调 push/pull 就无法搞错」。

---

## 0. 决策记录

### 0.1 方向性决策（用户拍板）

| # | 决策点 | 结论 | 依据 |
|---|---|---|---|
| 1 | 引擎落点 | **Rust `uc-mobile`**（新 `MobileSyncEngine` uniffi Object） | 本仓唯一能真正 build 出来的东西；iOS/Android 一次写对；复用已在本仓的 reducer + client；错误逻辑进不了客户端手里 |
| 2 | 状态归属 | **有状态引擎 + 注入 `KeyValueStore` 外部 port**（读写 App Group，键名沿用 `persist_keys.rs`） | 唯一能把接口缩到「push/pull 只收内容」的选项；load-before-op 天然吃下跨进程 resync；沿用键名 ⟹ 与 share extension 直写互通 |
| 3 | SSE / 计时塞入量 | **薄 - 中：引擎拥有「是否 pull / 冲突如何解」的决策，不拥有 SSE 连接生命周期**；native 继续 `start_sse_subscription` + 重连 + 兜底 tick + 生命周期 | iOS 生命周期/网络 epoch 本质 native；SSE 决策 5 已 codex 10 轮锁定，不重开 |

### 0.2 grilling 拍板（2026-07-05，Q1–Q10 全部「接受」）

| Q | 决策点 | 结论 |
|---|---|---|
| Q1 | pull 是否收当前设备剪贴板 | **收**。`pull(trigger, current_device_hash)`；保留 reducer 的 truth-gate（收敛检测 + watermark 修复），零多余 apply |
| Q2 | `auto_push` / consent 语义 | `push(content)` 映射 **`commit_push` 语义（不设 `last_applied`）**；`auto_push` 从引擎拿掉，「何时调 push」归客户端；引擎 `SyncSettings` **只留 `auto_apply`**；iOS consent-push 特例延后为独立变体 |
| Q3 | 同步操作退避归谁 | **引擎拥有**（本就持失败态）。例行调用在退避窗内返回 `BackingOff{retry_after_ms}` 不打网络；explicit / SSE 触发 / push punch through。native 的 SSE 重连退避另算、留 native |
| Q4 | history 列表同步 | **100% 留 native**（列表回填，正交）。引擎 outcome 带够元数据（hash/kind/text/size/content_id）供 native 往 `HistoryStorage` 追加行；引擎 KV 键集不含 history 键 |
| Q5 | `last_applied` 何时置 | **乐观**：`Applied` 返回的同时引擎即置 `last_applied`（假定 native 随即写），客户端不可能忘；写失败的罕见边角接受（与现 reducer 一致） |
| Q6 | SSE 转发粒度 | **折进 `pull(trigger, …)`**，不设三个 `handle_sse_*`；对外真的只剩 `push` + `pull`；`on_disconnected` 不进引擎 |
| Q7 | `is_content_available`/`compute_snapshot_hash` | 落引擎的 PR 里 **从 FFI 删掉**（未 push、never integrated、脚枪）；保留 `uc-content-hash` crate + `uc-core` 重构（引擎内部用）；服务端端点暂留 dormant；上一轮 RN 指南标记为被取代 |
| Q8 | 切服务器 / 网络路由 | **长生命周期引擎 + `set_server`**（与上次不同即 `handle_active_server_changed` 清 KV watermark + reset runtime）+ **`handle_network_route_changed`**（清同步操作退避）；避开「每实例读旧 KV」陷阱 |
| Q9 | `auto_apply` 关（staged）流 | 加 **`apply_staged()`（async）**：此刻 `get_file` 下载字节 + `mark_staged_applied` 语义 + 返回 `Applied`；staged 只存元数据、会话内、重启丢失即下次 pull 重 staged |
| Q10 | push 是否先 `get_latest` | **先 get_latest，与 pull 共用内部 sync-tick**：server-new 就 apply（push 也可能返回 `Applied`）、本地内容让位；server 没变才 `put_clipboard`。避免服务端接收时刻 LWW 下的 stale-clobber；网络成本与现 `forceTickNow` 一致；冲突解析逐位一致 |

---

## 1. 问题与目标

### 1.1 用户诉求（原话转译）
做一个 **逻辑闭环、防回环** 的同步 SDK：客户端 **只调 `push` 和 `pull`**，其余——去重、探测服务端有没有新内容、防回环 echo——都在 SDK 内部 **自动完成**，把它塌缩成客户端 **搞不错** 的东西（消灭 RN 今天那套「算 hash → 查存在 → 也许上传」的手动编排）。

### 1.2 目标
1. 客户端面缩到 `push(content)` / `pull(trigger, device_hash)`（+ 少量生命周期）。
2. 去重、server-new 探测、防回环、冲突解析全部在 SDK 内，客户端无从绕过或写错。
3. 不回归上一轮铁律（active register 只有完整 `put_clipboard` 才前进）。
4. 不重开已 codex 锁定的 SSE 决策（重连/生命周期归 native）。
5. 不引入 stale-clobber 等新回归（Q10）。

### 1.3 非目标
桌面侧机器不改；SSE 连接生命周期不搬进 Rust；history 列表同步留 native；后台/APNs 不在范围；RN/TS 代码在他仓。

---

## 2. 现状：今天「壳」在哪、错误怎么发生

- **`MobileSyncClient`（client.rs）= 纯网络原语层**。`get_latest`(=拉)、`put_clipboard`(=推，永远真上传)、`get_file`/`get_history_payload`(=下载字节)、`start_sse_subscription` 等。client **不持有「当前 server」**。
- **reducer（sync_engine.rs）= 纯决策核**。单个 `tick`：`plan_preamble → getLatest → plan_after_server_get → I/O → commit_*`。`SyncRuntimeState` 由外壳持有。`reducer.rs` 逐函数 FFI 导出、state 按值进出——**引擎无常驻状态**。
- **真正的编排壳 100% 在 RN TypeScript**（`SyncEngine.ts`，本仓不可见）：tick 循环 + 退避、SSE 重连/降级/生命周期/epoch、**所有持久化**（App Group 文件 + UserDefaults，见 `persist_keys.rs`）、`ClipboardMonitor` 本地轮询、逐函数驱动 reducer。**这就是要塌缩成 push/pull 的那层。**

### 2.1 错误今天怎么发生（要根治的类）
RN 的 `SyncClipboardClient.putContent` 上传前调 `getRecord(profileId)` 探「服务端是否已有」，对 Image/File 故意放行 hash 漂移 ⟹ 误判已存在 ⟹ 字节从未上传却标 `Synced` ⟹ 静默丢数据。**根因是「客户端自己手写了一条不可靠的存在性检查并据此决定是否上传」**——正是本设计要消灭的那类手动编排（§6.3：reducer 驱动的 push 里根本没有这条检查，bug 结构性消失）。

### 2.2 持久化现状（关键约束，recon 已核实）
**Rust 侧没有任何持久化 port**：reducer 无状态、proto crate 明确把 I/O 留 native、`persist_keys.rs` 只钉键名、唯一 host hook `PlatformBridge::app_group_dir()` 只返回目录路径。**share extension 是独立进程**，push 时 **直写** `LAST_SYNCED_HASH` 等键（不经引擎）。⟹ 引擎自持状态必须新增读写 App Group 的 port，且沿用同一批键名以与 share extension 互通。

---

## 3. seam 设计（deep module）

```text
┌─────────────────────────── native（RN / Swift / Kotlin，他仓）────────────────────────────┐
│  · 何时 push：ClipboardMonitor 1s 本地轮询检测到本地写 → 读 UIPasteboard → engine.push(content)  │
│  · 何时 pull：SSE 通知 / 兜底 tick / 前台 resume → engine.pull(trigger, 当前剪贴板 hash)          │
│  · 把 Applied 返回的字节写回 UIPasteboard（文本/图→剪贴板，文件→Files）                          │
│  · SSE 连接：start_sse_subscription + 重连退避 + 降级轮询 + 生命周期/epoch（SSE 决策 5，不动）     │
│  · 按 push/pull 的 outcome 元数据往 HistoryStorage 追加行；周期性 history 列表同步（query_history）│
│  · UI：banner、staged 高亮、设置开关（auto_apply）                                                │
└───────────────────────────────────────┬───────────────────────────────────────────────────┘
              push(content) / pull(trigger, device_hash) / apply_staged() / set_server / …   ← 小接口（seam）
┌───────────────────────────────────────▼─────────────── Rust SDK：MobileSyncEngine ─────────┐
│  内部一个 sync-tick（复用 reducer）：get_latest → 路由（converged / server-new / push-side）    │
│  去重（watermark + contentId 短路） · 冲突解析（server-new 优先 apply，本地让位）                 │
│  字节上传/下载（put_clipboard / get_file / get_history_payload） · 推进 active register           │
│  防回环（last_applied 自写 guard + loop_guard 翻转检测） · 同步操作退避（backoff）                 │
│  watermark 持久化（经注入的 KeyValueStore port，读写 App Group；load-before-op / save-after-op）  │
│  内部件（internal seams，各自可测）：纯 reducer(sync_engine.rs) · MobileSyncClient · loop_guard    │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

**留 native 的本质复杂度**：触发时机（OS 无剪贴板推送 API，本地检测必须轮询）、`UIPasteboard` 字节读写（平台 API + 主线程；故 push 收字节、pull 返字节，不引入 pasteboard port）、SSE 连接生命周期（决策 5）。
**进引擎的意外复杂度**：去重/防回环/冲突解析/字节 I/O/register 推进/退避/watermark 持久化——凡「跨平台一致 + 容易写错」的。

---

## 4. 接口（拟定 FFI 面）

新增一个 uniffi `Object`：`MobileSyncEngine`（uc-mobile）。**长生命周期单实例**（Q8），对客户端唯一要学的面。

```rust
// —— 构造 ——
#[uniffi::constructor]
fn new(
    server: ServerConfig,          // 复用现有类型；当前服务器（可后续 set_server 切换）
    config: SyncConfig,            // 复用现有 reducer 配置（cadence/backoff/loop 阈值）
    settings: SyncSettings,        // 只含 auto_apply（Q2）
    store: Arc<dyn KeyValueStore>, // 新增 with_foreign port（§7）
    client: Arc<MobileSyncClient>, // 复用现有网络原语层
) -> Result<Arc<Self>, SyncError>;

// —— 两个原语（push/pull 共用一个内部 sync-tick，Q10）——

/// 客户端观察到本地剪贴板变化后调用。内部：load 状态 → get_latest → 路由：
///   · server-new（服务端自上次同步后变过）→ 下载并返回 Applied（本地内容让位，防 stale-clobber）
///   · 否则按 watermark/自写 guard 去重 → 命中即 UpToDate；未命中即完整 put_clipboard（真上传 +
///     推进 active register）→ 记 loop_guard → 落 watermark → Uploaded
/// push 视为 punch-through（用户内容不因退避窗被丢）。
async fn push(&self, content: LocalContent) -> SyncOutcome;

/// 探测并（按 auto_apply）应用服务端新内容。`current_device_hash` 供 truth-gate 收敛检测（Q1）。
/// trigger 决定退避门控与 contentId 短路（Q3/Q6）。内部：load 状态 → get_latest → 路由。
async fn pull(&self, trigger: PullTrigger, current_device_hash: Option<String>) -> SyncOutcome;

// —— staged 流（auto_apply 关，Q9）——
/// 用户在 banner 上点「应用」时调。此刻 get_file 下载字节 + mark_staged_applied 语义 + 返回 Applied。
async fn apply_staged(&self) -> SyncOutcome;

// —— 生命周期 ——
fn set_server(&self, server: ServerConfig);   // 与上次不同即清 watermark + reset runtime（Q8）
fn handle_network_route_changed(&self);       // 清同步操作退避（Q8）
fn set_settings(&self, settings: SyncSettings);
fn acknowledge_loop_detected(&self);          // 用户消 banner 后清 loop 缓冲
```

**输入/输出类型（uniffi Record/Enum）**：

```rust
/// 客户端读 pasteboard 得到的本地内容。File/Image 带字节 + 名字；纯文本走 text。
struct LocalContent { kind: ClipboardKind, text: String, data_name: Option<String>, payload: Option<Vec<u8>> }

struct SyncSettings { auto_apply: bool }   // auto_push 归客户端（Q2）

/// push 触发省略；pull 的触发源决定退避门控 + contentId 短路（Q3/Q6）。
enum PullTrigger {
    Routine,                       // 兜底 tick：受同步操作退避门控
    Explicit,                      // 用户下拉刷新：punch through
    SseHello,                      // 连接刚活：无条件 pull（punch through）
    SseResync,                     // 服务端 lagged：无条件 pull（punch through）
    SseUpdate { content_id: String }, // content_id==last_synced_content_id 则短路 UpToDate，否则 pull
}

/// push / pull / apply_staged 统一返回（Q10 后二者共用内部 tick，outcome 同型）。
enum SyncOutcome {
    /// 完整 put_clipboard 成功（推进 active register）。带元数据供 native 追加 .local 历史行（Q4）。
    Uploaded { meta: SyncedMeta },
    /// 服务端有新内容，字节已下载：native 请写入剪贴板（文本/图）或 Files（文件）。
    /// 引擎已乐观置 last_applied（Q5）。带元数据供 native 追加 .pulled 历史行。
    Applied { content: LocalContent, meta: SyncedMeta },
    /// 服务端有新内容但 auto_apply 关：暂存（会话内），native 出 banner。
    Staged { preview: StagedPreview },
    /// 什么都没流动（已同步 / 自写 / 已收敛 / 无本地变化 / SSE contentId 短路）。reason 供遥测。
    UpToDate { reason: UpToDateReason },
    /// 例行调用被同步操作退避挡下（Q3）；native 据 retry_after_ms 排下一次例行 tick。
    BackingOff { retry_after_ms: i64 },
    /// loop_guard 翻转跳闸，已暂停；native 出 banner，用户 ack 后调 acknowledge_loop_detected。
    LoopDetected,
    Failed { error: SyncError },
}

struct SyncedMeta { kind: ClipboardKind, hash: Option<String>, content_id: Option<String>, text: Option<String>, size: Option<i64> }
enum UpToDateReason { AlreadySynced, SelfWritten, Converged, NoLocalChange, SseShortCircuit, ConsentMode }
```

**接口深度自检**：客户端要学的 = 2 个原语 + `apply_staged` + 4 个小生命周期 + 几个数据类型。没有一处要求它理解 watermark、contentId、loop_guard、active register、两段式上传、冲突解析——全在实现里。相较今天客户端要逐个驱动 `plan_preamble`/`plan_after_server_get`/`commit_*` 并自持 `SyncRuntimeState`，接口显著变小、实现显著变深。

---

## 5. 三个方向性分叉（DESIGN-IT-TWICE，已拍板）

（Fork 1/2/3 详见 §0.1；此处存档推荐理由）
- **Fork 1 → Rust 引擎**：本仓唯一能 build 的东西；跨端一次写对；"can't get it wrong" 最强。
- **Fork 2 → 有状态 + KV port**：唯一兑现「push/pull 只收内容」；load-before-op 反过来解决跨进程；沿用键名与 share extension 互通。
- **Fork 3 → 薄 - 中**：防回环/去重/冲突解析（易错）进引擎；计时/连接（平台）留 native，不重开 SSE 决策 5。

---

## 6. 关键正确性点

### 6.1 防回环（用户强调的「防回环」如何兑现）
回环链：`Applied` 后 native 写内容 X 到剪贴板 → ClipboardMonitor 把 X 当「本地变化」→ 若 `push(X)` → 桌面当新内容 → …。两道 guard（都在 reducer 内，引擎包住）：
1. **自写 guard**：`Applied` 返回时引擎即置 `last_applied = hash(X)`（Q5 乐观）。下次 `push(X)` 内部 tick 见 `hash(X) == last_applied` → `UpToDate{SelfWritten}`。链断。
2. **loop_guard 翻转检测**：窗口内数 pushed/pulled 翻转，A→B→A→B 跳闸 `LoopDetected` 暂停。
→ 客户端只要调 push/pull 就自动获得两道防线——「逻辑闭环」的兑现点。

### 6.2 push 不盲推 + active register 铁律（Q10）
`push` **先 get_latest 走内部 sync-tick**（与 pull 同一条）。两个后果：
- **server-new 优先 apply**：若服务端自上次同步后变过，push 返回 `Applied`（本地 X 让位），**不** 用旧内容盖新内容——避免服务端「接收时刻 LWW」下的 stale-clobber（离线积压的旧 push 冲掉桌面新内容）。冲突解析与现 reducer 逐位一致。
- **一旦决定 push 就走完整 `put_clipboard`**，永远真上传字节 + 推进 register。`find_entry_id_by_snapshot_hash` 命中只证明「曾存在」不证明「当前活跃」，active register 只有完整 put 才前进（上一轮 `put_clipboard_deduped` 被毙即此因）。绝不用「内容已存在」去跳过上传。

### 6.3 `is_content_available` 在 push 路径退役（把两轮工作接上）
原 bug 只因 RN 手写 `getRecord` 存在性检查。reducer 驱动的 push 里 **根本没有存在性检查**：靠内部 tick 的路由 + watermark gate 决策，一旦 DoPush 就完整 `put_clipboard`。⟹ **误判跳过上传那类 bug 结构性消失，无需 `is_content_available`**。它作为 client 原语在本 PR 从 FFI 删除（§12）。这也回答「为什么自动去重需谨慎」：它是 **watermark 去重**，不是 **内容存在性去重**。

### 6.4 跨进程 App Group 一致性
share extension（独立进程）push 时直写 `persist_keys` 键。引擎 `KeyValueStore` port **读写同一批键名**（不是不透明 blob），两进程 watermark 互通。引擎 **load-before-op** 即读到 share extension 刚写的值——**吸收并简化** reducer 今天 `plan_preamble` 的跨进程 resync fold。

### 6.5 并发
一进程内 `push`（本地写触发）与 `pull`（SSE 触发）可能并发，load-before/save-after 有读改写竞态。**引擎内部一把 async 锁串行化 push/pull/apply_staged**，把 SSE 决策 5 里 TS 的「单个在跑 + 至多一个 pending」协调下沉一半到 Rust（native 仍负责触发合并的上层调度）。

---

## 7. 状态持久化设计（新增 port）

```rust
/// App Group（跨 app / keyboard ext / share ext）键值存储。native 实现，注入引擎。
/// 键名沿用 uc_mobile_proto::persist_keys（与 share extension 直写互通）。
#[uniffi::export(with_foreign)]
pub trait KeyValueStore: Send + Sync {
    fn get(&self, key: String) -> Option<Vec<u8>>;
    fn set(&self, key: String, value: Vec<u8>);
    fn remove(&self, key: String);
}
```

- **引擎持久化的键**（沿用 `persist_keys.rs`）：`LAST_SYNCED_HASH` / `LAST_SYNCED_CONTENT_HASH`(contentId)。**仅 watermark 是耐久的**；`last_applied_hash` / `staged_*` / `loop_events` / backoff 按 reducer 语义 **仅会话内**（引擎内存持有）。**history 键（`LAST_HISTORY_SYNC_AT` / `HISTORY_MODIFIED_AFTER`）不属引擎**（Q4，native 自管）。
- **load-before-op**：每次 push/pull/apply_staged 前，引擎从 port 读耐久键 → fold 进内存 `SyncRuntimeState`（等价并简化跨进程 resync）。
- **save-after-op**：`advance_synced` 变动后经 port 写回。
- **契约留交接**：native 的 KV 实现负责原子写 + App Group 容器路径（用现有 `PlatformBridge::app_group_dir()`）。
- **切服务器（Q8）**：`set_server` 检测到不同即经 port `remove` watermark 键 + reset 内存 runtime（新服务器有自己的内容时间线）。

---

## 8. 与现有 M5 reducer / SSE 决策的关系

### 8.1 我们 **重画** 了 M5 的一条线（用户已确认接受）
M5（用户 2026-06-14）把「决策核（Rust）」与「执行外壳（native）」拆开。本设计把外壳里「server 往返 + 去重/防回环/冲突解析/watermark」这半用 Rust 重写进引擎；**scenePhase、UIPasteboard 字节、banner、SSE 连接** 仍留 native。即 **沿「平台绑定 vs 跨平台一致」重新切一刀**，纯 reducer 仍原样是内部 seam。**用户 2026-07-05 已确认接受此重画。**

### 8.2 我们 **不动** 的（已 codex 锁定）
- SSE notify-then-pull 红线：`update` 只带 contentId，引擎收到（`pull(SseUpdate)`）仍走 `get_latest` 拉权威 snapshot。
- SSE 重连/降级/生命周期/epoch 归 native（决策 5，10 轮评审）。引擎只在 `pull` 的 trigger 里吃 SSE 语义。
- 桌面事件源、服务端 SSE 端点：不动。

### 8.3 我们 **复用** 的
纯 reducer（`sync_engine.rs`）逐路由/commit 逻辑（含内部 sync-tick 的 get_latest→route）、`MobileSyncClient` 全部网络原语、`loop_guard`、`persist_keys` 键名、`Clipboard` wire 模型。

---

## 9. 测试策略

**push/pull 就是测试面**（deep-module 可测性红利）：注入 fake `KeyValueStore`（内存 HashMap）+ 把 `MobileSyncClient` 指向现有 mock-server（`client.rs` 已有 `spawn_mock`）→ 驱动 push/pull/apply_staged → 断言 `SyncOutcome` + 持久化键 + mock 收到的上传字节。纯 reducer 继续独立单测。

重点用例：
- 连续两张不同图 push（第二张必须真上传——复现原 bug 场景）。
- pull 应用后立刻 push（自写 guard → `UpToDate{SelfWritten}`）。
- ping-pong（loop_guard 跳闸 `LoopDetected`）。
- SSE update 短路（`content_id==last_synced_content_id` → 不发网络 `UpToDate{SseShortCircuit}`）。
- 跨进程（fake store 预置 share-ext 写的键 → 引擎 load-before-op 读到，`UpToDate{AlreadySynced}`）。
- **Q10 stale-clobber**：本地候选 X + 服务端已是更新的 Y → `push(X)` 返回 `Applied{Y}`，mock 未收到 X 的上传。
- 退避（连续失败后 `pull(Routine)` 返回 `BackingOff`，`pull(Explicit)` punch through）。
- 切服务器（`set_server` 后 watermark 键被 remove，下条 server-new 正常 `Applied`）。

---

## 10. 分 PR 计划（本仓 vs 他仓）

**本仓（`uniclipboard`）——本设计的实现范围：**
- **PR-A**（uc-mobile）：`KeyValueStore` port + `MobileSyncEngine`（push/pull/apply_staged/set_server/handle_network_route_changed/set_settings/acknowledge_loop_detected）+ `SyncOutcome`/`PullTrigger`/`LocalContent`/`SyncedMeta` 类型；内部复用 reducer + client。含 Rust 单测（§9）。**同 PR 删** `is_content_available`/`compute_snapshot_hash` 两个 FFI（§12）。
- **PR-B**（uc-mobile-proto + uc-webserver）：**原预期零改动的假设未成立**（2026-07-06
  追加实现，见文件头补丁说明）——`commit_push` 新增 `content_id: Option<&str>` 形参，
  `uc-webserver` 的 `PUT /SyncClipboard.json` 成功响应新增可选 `contentId` 字段，
  `client.putClipboard` 返回值随之变化。修复 push 后服务端再编码内容被误判成新内容的
  drift 窗口（原 §6.4 遗留场景，测试驱动发现）。
- **PR-C**（文档）：RN 交接指南，给出 `push`/`pull(trigger)`/`apply_staged` 调用序列、`KeyValueStore` 实现契约、从「逐函数驱动 reducer」迁移到「调 push/pull」的对照，并标注上一轮 content-availability 指南被取代。

**他仓（`uniclipboard-android`）——本设计不实现，仅交接：**
- RN 瘦身：删 TS 里的 reducer 驱动 + 手动 watermark 线程 + `getRecord` 存在性检查；`ClipboardMonitor` → `engine.push`；SSE 回调 → `engine.pull(SseHello/SseResync/SseUpdate)`；`Applied` 返回字节 → 写 UIPasteboard/Files；实现 `KeyValueStore`（App Group）；history 列表同步 + `HistoryStorage` 追加保持 TS（按 outcome 元数据）。

---

## 11. 开放问题（grilling 已全部拍板，见 §0.2）

Q1–Q10 全部解决并折入 §0.2 / §4 / §6 / §7。**无遗留结构性开放问题。** 实现期待确认的微观项：
- ~~PR-B 是否需要（reducer 微调）——预期零改动~~：**2026-07-06 已确认需要**，见文件头
  补丁说明与 §10。
- consent-push（iOS paste control 特例）是否本期就加独立变体——默认延后（Q2）。
- 服务端 content-availability 端点最终去留——独立 follow-up（Q7）。

---

## 12. 上一轮工作（commit `270f5715b`）的处置

| 产物 | 处置 |
|---|---|
| `uc-content-hash` crate | **保留**（引擎内部算 snapshot_hash 用） |
| `uc-core` 用其重构（纯提取） | **保留** |
| 服务端 `GET /api/mobile-sync/content-availability` 端点 | **暂留 dormant**，去留作独立 follow-up |
| `MobileSyncClient::is_content_available`（FFI） | **PR-A 删除**（脚枪，push 路径已退役，未 push、never integrated） |
| `compute_snapshot_hash`（FFI 自由函数） | **PR-A 删除**（RN 不再自算 hash） |
| RN 对接指南 `2026-07-05-content-availability-rn-integration-guide.md` | **标记被 push/pull 取代** |

---

*本文件为 v0.2 设计（grilling 10 问已折入）。进 codex 对抗评审 / 用户最终批准后进 PR-A；批准前不写引擎代码（交接文件最高风险项：不在错误 crate / 错误状态模型上白干）。*
