# push/pull 同步 SDK · 移动端 (RN) 对接指南

面向 `uniclipboard-android`(RN/Expo，iOS + Android 共用 TS) 团队。
对应设计：`.planning/2026-07-05-mobile-push-pull-sdk-design.md`（PR-A 已在本仓实现，
`crates/uc-mobile/src/engine.rs`）。

> **2026-07-06 补丁（Gap 2 修复，追加 FFI 变更，务必重新生成 binding）**：为关闭"push
> 后服务端重编码内容、下一次 pull 误判成新内容重新下载"的窗口（原设计 §6.4 遗留的一处
> 未测场景），`PUT /SyncClipboard.json` 的成功响应从裸 200 空 body 改为可选携带
> `{"contentId": "..."}`；`client.putClipboard()` 与旧版逐函数 `reducer.commitPush()`
> 的签名都因此变化（BREAKING）。详见 §3.1 与 §5 的更新。**这轮改动只动了本仓
> （`uc-mobile`/`uc-mobile-proto`/`uc-webserver`），突破了设计文档 §10 "PR-B 预期零
> 改动"的假设**——设计文档状态头已同步更新，见其文件顶部说明。

**取代上一轮**：`.planning/2026-07-05-content-availability-rn-integration-guide.md`
（`isContentAvailable`/`computeSnapshotHash`）**已被本轮工作取代并从 FFI 删除**——
push 路径根本不再有"存在性检查"这一步，误判跳过上传那类 bug 结构性消失。如果你们还没来得及
接入上一轮的指南，直接跳过它，从本指南开始接入即可。

---

## 1. 这次改了什么（一句话）

把"决策 + 网络 I/O + 去重/防回环/watermark"整个收进 Rust 的 `MobileSyncEngine`，RN 侧对客户端
只留 `push(content)` / `pull(trigger, deviceHash)` 两个原语（+ `applyStaged` 和几个生命周期方法）。
RN 不再需要：逐函数驱动 reducer、自己维护 `SyncRuntimeState`、手写 watermark 比较、手写
"存在性检查决定要不要上传"。

---

## 2. Rust core 新增了什么

新增一个 uniffi Object：`MobileSyncEngine`（`crates/uc-mobile`），长生命周期单实例。

```rust
// 构造：一次性，长期持有
MobileSyncEngine::new(server, config, settings, store, client) -> Result<Arc<Self>, SyncError>

// 两个原语
async fn push(&self, content: LocalContent) -> SyncOutcome;
async fn pull(&self, trigger: PullTrigger, current_device_hash: Option<String>) -> SyncOutcome;

// staged 流（auto_apply 关时）
async fn apply_staged(&self) -> SyncOutcome;

// 生命周期（注意：全部是 async，见 §4 末尾说明）
async fn set_server(&self, server: ServerConfig);
async fn handle_network_route_changed(&self);
async fn set_settings(&self, settings: SyncSettings);
async fn acknowledge_loop_detected(&self);
```

新增的数据类型：`LocalContent`、`SyncSettings{auto_apply}`、`PullTrigger`
（`Routine`/`Explicit`/`SseHello`/`SseResync`/`SseUpdate{contentId}`）、`SyncOutcome`
（`Uploaded`/`Applied`/`Staged`/`UpToDate`/`BackingOff`/`LoopDetected`/`Failed`）、`SyncedMeta`、
`UpToDateReason`、`StagedPreview`。全部在 `crates/uc-mobile/src/engine.rs`，具体字段直接看生成的
TS 类型定义（binding 重生成后）。

`ClipboardKind`、`ServerConfig`、`SyncError` 复用现有类型，没有变化。

---

## 3. FFI 面变更（BREAKING —— 必须重新生成 binding）

- **新增**：`MobileSyncEngine` 整个 Object + 上面列的所有新类型。
- **删除**：`computeSnapshotHash(bytes)` 自由函数、`client.isContentAvailable(server, hash)`
  方法——上一轮加的，这次同 PR 删除（未 push、RN 从未真正接入过，是"脚枪"）。如果你们已经在
  RN 里写了调用这两个 API 的代码，删掉；改用本指南 §4。
- 服务端 `GET /api/mobile-sync/content-availability` 端点本身 **暂时保留但转入 dormant**——
  没人调用它了，去留是独立的后续问题，不影响这次接入。

重生成后确认 TS 类型里能看到 `MobileSyncEngine`、`LocalContent`、`SyncOutcome` 等，且
`computeSnapshotHash`/`isContentAvailable` 已经消失。

---

### 3.1 2026-07-06 追加的 FFI 变更（BREAKING —— 同样必须重新生成 binding）

- **`client.putClipboard(server, meta, payload)`**：返回类型从 `void`/`Unit` 改为
  `String?`（服务端在 PUT 响应里回显的 `contentId`；旧 daemon / 第三方 SyncClipboard
  服务器没有这个字段时是 `null`，不是错误）。**只有当你们的代码直接调用
  `client.putClipboard()`（绕开 `MobileSyncEngine.push()`）时才需要改调用点**——走
  `engine.push()` 的正常路径不受影响，引擎内部已经把这个返回值接进了 watermark 逻辑
  （§5）。
- **`reducer.commitPush(state, pushedHash, nowMs, cfg)`**（旧版逐函数 reducer 镜像，
  本设计要淘汰的模式，见文件头"取代上一轮"说明）：新增一个位置参数
  `contentId: String?`，签名变成
  `reducer.commitPush(state, pushedHash, contentId, nowMs, cfg)`。**如果 Share
  Extension（或任何还没切到 `MobileSyncEngine` 的旧代码路径）还在直接调这个函数，必须
  同步改调用点**；没有独立 push 路径的话可以忽略这条。
- **`PUT /SyncClipboard.json` 的 wire 响应体**：从裸 200 空 body 改成可选携带
  `{"contentId": "..."}`（新增字段，向后兼容，忽略即可）。只有自己手写 HTTP 请求、
  没有走 `MobileSyncClient` 的代码才需要关心这一层。

---

## 4. RN 接线步骤

### 步骤 0 · 实现 `KeyValueStore` 并构造一次性 `MobileSyncEngine`

```ts
interface KeyValueStore {
  get(key: string): Uint8Array | null;
  set(key: string, value: Uint8Array): void;
  remove(key: string): void;
}
```

用 App Group 容器路径（`PlatformBridge.appGroupDir()`，未变）做底层存储，键名沿用
`uc_mobile_proto::persist_keys`（见 §5——**这里有一个新键，务必读完**）。

引擎构造一次，长期持有（不是每次 tick 都新建）：

```ts
const engine = MobileSyncEngine.new(server, config, { autoApply }, myKeyValueStore, client);
```

### 步骤 1 · `ClipboardMonitor` 检测到本地写 → 调 `push`

原来的"算 hash → 查存在 → 也许上传"手动编排整个删掉，替换成：

```ts
const outcome = await engine.push({ kind, text, dataName, payload });
switch (outcome.tag) {
  case "Uploaded":
    // 追加 .local 历史行（outcome.meta：kind/hash/contentId/text/size）
    break;
  case "Applied":
    // 服务端其实有更新内容——写入 outcome.content 到 UIPasteboard/Files，
    // 追加 .pulled 历史行；本地那次写入让位，没有被上传（Q10 stale-clobber 保护）
    break;
  case "UpToDate":
    // 什么都没做（已同步/自写/consent 模式关），outcome.reason 供遥测
    break;
  case "LoopDetected":
    // 出 banner，等用户调 acknowledgeLoopDetected
    break;
  case "Failed":
    // outcome.error，正常错误处理
    break;
  // Staged 理论上不会从 push() 出现在 auto_apply=false 场景之外，但类型上仍需处理
}
```

**不再需要**：自己算 `computeSnapshotHash`、自己调用任何"存在性检查"API。引擎内部会自己决定
"这次要不要真的上传"，你不可能因为手写错一个存在性判断而把这类 bug 带回来。

### 步骤 2 · SSE 回调 → 调 `pull(trigger)`

```ts
sseListener.onHello = () => engine.pull({ tag: "SseHello" }, currentDeviceHash());
sseListener.onResync = () => engine.pull({ tag: "SseResync" }, currentDeviceHash());
sseListener.onUpdate = (contentId) =>
  engine.pull({ tag: "SseUpdate", contentId }, currentDeviceHash());
// 兜底 tick（原来的固定 cadence 轮询）
setInterval(() => engine.pull({ tag: "Routine" }, currentDeviceHash()), tickIntervalMs);
// 用户下拉刷新
onPullToRefresh(() => engine.pull({ tag: "Explicit" }, currentDeviceHash()));
```

`BackingOff{retryAfterMs}` 是例行 tick 被同步操作退避挡下——按 `retryAfterMs` 排下一次
`Routine` tick，不要重试；`Explicit`/`SseHello`/`SseResync`/未短路的 `SseUpdate` 都会穿透退避。

### 步骤 3 · `auto_apply` 关闭时的 staged 流

`Staged{preview}` → 出банner（用 `preview.kind`/`text`/`size` 渲染，不含字节）。用户点"应用"：

```ts
const outcome = await engine.applyStaged();
// 同 push/pull 的 Applied 分支处理
```

### 步骤 4 · history 列表同步保持不变

引擎的 KV 键集 **不包含** history 相关键（`historyModifiedAfter`/`lastHistorySyncAt`）——这块
100% 留在 RN 侧，按 `SyncOutcome` 里的 `meta`（`Uploaded`/`Applied`）往 `HistoryStorage` 追加行，
周期性 `queryHistory` 拉列表的逻辑不用改。

### ⚠️ 生命周期方法全部是 `async`（design 草稿里画的是同步签名）

设计文档 §4 的接口草图把 `setServer`/`handleNetworkRouteChanged`/`setSettings`/
`acknowledgeLoopDetected` 画成普通同步方法。实现时发现一个真实的 panic 风险：Rust 侧用
`tokio::sync::Mutex` 持有引擎状态，若这几个方法用阻塞式加锁（`blocking_lock`），一旦调用方
恰好处于某个 tokio 异步上下文里（本仓自己的测试就踩中过），会直接 panic
（"cannot block the current thread from within a runtime"）。没有可移植的"仅在不在 runtime
里时才阻塞"的写法，所以这 4 个方法在最终实现里全部改成了 `async fn`。uniffi 会自动把 async
Rust 方法桥接成 Swift `async` / Kotlin `suspend` 函数——对 RN/TS 侧就是这几个调用点多加一个
`await`，机械改动，不影响调用时机或语义。

---

## 5. ⚠️ 新增的 App Group 键（务必接线，否则跨进程一致性会静默失效）

引擎持久化的耐久键（沿用 `uc_mobile_proto::persist_keys::files`，与 Share Extension 直写互通）：

| 键名 | 状态 |
|---|---|
| `last_synced_hash` | **已有**——Share Extension 已经在直写这个键，行为不变 |
| `last_synced_content_id` | **全新**——本轮新增的常量（`persist_keys.rs` 里之前根本没有这个键；旧的 `last_synced_content_hash` 是完全不同的东西，是历史遗留的 hash 键，**不要** 把 contentId 存到那个键上） |

`last_synced_content_id` **没有任何既有的原生写入方——你们的 `KeyValueStore` 实现是这个键的
第一个生产者/消费者**。

> **2026-07-06 更新**：push 路径对这个键的处理不再是无条件清空。`MobileSyncEngine.push()`
> 内部会把 `client.putClipboard()` 返回的 `contentId`（PUT 响应里服务端回显的，见 §3.1）
> 直接学进这个键——**服务端给了就写，没给（`null`）就清空**，与 `last_synced_hash` 的更新
> 同步进行。这是为了让"服务端对刚上传的内容做了无损再编码（Image/File 的 hash drift）"
> 这种情况下，下一次 `pull` 能靠 `contentId` 认出"这就是我刚传的东西"，不会误判成新内容
> 重新下载、把用户刚拷贝的内容覆盖掉。如果 Share Extension 也需要在推送时写这个键（比如它
> 自己也走了独立于 `MobileSyncEngine` 的 push 路径），需要遵守同一套"服务端给了就写、没给
> 就清空"的契约，而不是旧版"无条件清空"。
>
> 纯读值的场景（`pull`/`applyStaged` 学到 `contentId` 后写回）不受影响，直接原样存字符串
> 即可。

其余状态（`last_applied_hash`、staged 槽位、loop 事件、同步操作退避）**只在会话内存里**，
**不落盘**——重启后自然从下一次 `pull`/`push` 重新收敛，不需要你们做任何事。

---

## 6. 验证方法

复现原始 bug 场景（本设计要解决的核心问题）：
1. 连续拍两张不同照片，快速触发两次 `push`。
2. 第二次 `push` 必须真的走了 `PUT /file/{name}` + `PUT /SyncClipboard.json`（观察网络日志，
   或看返回的 `SyncOutcome` 是 `Uploaded` 而不是被静默吞掉）。

其余建议验证点（对应 Rust 侧的单测场景，具体见 `crates/uc-mobile/src/engine.rs` 测试模块）：
- `pull` 应用内容后立刻 `push` 同样的内容 → 应该是 `UpToDate`（自写/已收敛，两者皆可能，取决
  于服务端是否已经变化），**不会** 真的重新上传。
- SSE `onUpdate` 传来的 `contentId` 与已同步的一致 → 不应该看到任何网络请求，直接
  `UpToDate{reason: "SseShortCircuit"}`。
- 跨进程：Share Extension 直写 `last_synced_hash`/`last_synced_content_id` 后，主 App 下一次
  `pull` 应该识别为"已同步"，不重新下载。
- 连续断网多次 `pull(Routine)` 后应该收到 `BackingOff`；此时 `pull(Explicit)`（下拉刷新）应该
  仍然真正发起网络请求（不会被退避挡住）。
- **（2026-07-06 新增）** push 一张图片后，模拟服务端对它做了无损再编码（同 `contentId`、
  不同 `hash`——真实场景对应服务端的 HEIC/JPEG 兼容处理）：紧接着的 `pull` 应该是
  `UpToDate`（认出是同一份内容，靠 PUT 响应里学到的 `contentId`），**不会** 重新下载、
  不会把用户刚拷贝的内容覆盖成服务端的再编码版本。对应 Rust 侧单测
  `engine::tests::pull_after_push_recognizes_reencoded_content_via_content_id_learned_from_put_response`。

---

## 7. 一句话给到团队

> Rust core 新增了 `MobileSyncEngine`，把去重/防回环/watermark/冲突解析全部收进引擎内部，
> RN 侧只调 `push`/`pull`/`applyStaged` 三个原语 + 几个生命周期方法（现在全是 `async`，多加
> `await` 即可）。上一轮的 `computeSnapshotHash`/`isContentAvailable` 已删除，不用管。**唯一
> 需要你们主动做的持久化接线**：实现 `KeyValueStore` 时新增 `last_synced_content_id` 这个键
> （App Group 文件存储，与既有 `last_synced_hash` 同一套读写路径），这是本轮真正的新增契约，
> 其余状态引擎自己管，不需要 RN 侧关心。
>
> **2026-07-06 追加**：`client.putClipboard()` 现在会返回服务端回显的 `contentId`（用于修
> 复"push 后服务端重编码内容、下一次 pull 误判成新内容"的 Gap 2），走 `engine.push()` 的
> 正常路径无感；只有直接调 `client.putClipboard()` 或旧版 `reducer.commitPush()` 的代码
> 才需要改调用点（BREAKING，见 §3.1）——都要重新生成 binding。
