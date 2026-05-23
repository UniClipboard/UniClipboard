# 调研发现

> 阶段 0 输出 + 实施过程中的关键发现。代码引用全部用 `文件:行号`。

## 日志取证状态

今天（2026-05-22）与昨天（2026-05-21）的 dev profile 日志中，`uc_application::file_transfer` / `uc_bootstrap::file_transfer_lifecycle` 只出现启动相关行（"No orphaned in-flight transfers found at startup"），**没有 timeout 触发、cancel 调用、delete 流程的运行时记录**。

`cancel` 关键字仅命中 updater 和 pairing 模块；`timeout` 关键字仅命中 iroh relay ping。

含义：4 个问题的根因结论以代码事实为主。需要补一次"完整复现 + 抓 dev 日志"的回合（P1-7 验收阶段）。

## 问题 1：接收方 timeout，发送方仍在传输

- 触发点：`src-tauri/crates/uc-bootstrap/src/file_transfer_lifecycle.rs:31-35`
  - `PENDING_TIMEOUT_MS = 60_000`
  - `TRANSFERRING_TIMEOUT_MS = 300_000`
  - 判定字段：`updated_at_ms`（即 `last_progress`），见 `file_transfer_repository.rs:65`
- 触发后行为：`spawn_timeout_sweep`（同文件 `:114-154`）→ `repo.list_expired_inflight()` → `repo.mark_failed()` → 手动发 `TransferHostEvent::StatusChanged`
- **关键缺失**：只更新本地 projection 表，**不撕 receiver fetch task** —— receiver 端 iroh-blobs Downloader actor task 继续下载，发送方 iroh provider 继续响应 read request
- 已知 pending todo：`.planning/todos/pending/2026-04-17-move-file-transfer-timeout-sweep-off-repository-port.md` 描述了"timeout sweep 绕过事件存储"
- **修复方向**（P1-8）：sweep 改成调 `BlobTransferFacade::cancel_inbound_transfer`，让 timeout 走同一 cancel 通道

## 问题 2：缺少取消按钮

- `CancelTransferUseCase` 已存在：`src-tauri/crates/uc-application/src/file_transfer/usecases/cancel_transfer.rs:35-44`
- `FileTransferFacade::cancel` 已经暴露：`src-tauri/crates/uc-application/src/facade/file_transfer/facade.rs:162`
- **缺失**：
  - Tauri command（**已修**：`9c573faf`）
  - 前端按钮 + thunk（P1-6 待做）
  - **关键点**：仅调 `FileTransferFacade::cancel` 不够 —— 它只发 `Cancelled` event，不撕 receiver fetch task。所以新增 `BlobTransferFacade::cancel_inbound_transfer` 才是完整修复（08b247a7 commit）
- 已知 pending todo：`.planning/todos/pending/2026-04-17-wire-real-filetransferevent-cancelled-emitter.md`

## 问题 3：Webview 内存异常（疑似整文件加载）

- 直连传输的 progress wire：`src-tauri/crates/uc-infra/src/network/iroh/transfer_progress_wire.rs:14-27`
  - 34 字节定长帧：`magic(1) + transfer_id(16) + bytes_transferred(8) + total_bytes(8) + status(1)`
  - **不携带任何文件 bytes**
- 接收方落盘：iroh-blobs fetch 直接写到 `cached_path`，**不经 Tauri command 把 bytes 返回前端**
- 前端 progress reducer（`fileTransferSlice.ts:53-94`）只存数值指标（bps、ETA），不存 bytes
- **结论**：传输路径本身不会让 webview 内存膨胀
- 真正嫌疑（**待运行时确认**）：
  - 缩略图生成：`ThumbnailRepositoryPort` 在大文件场景下是否会把整图解码到内存
  - 预览：`ClipboardPayloadResolverPort` 在前端展示传输完成的大文件时是否拉了完整 payload
  - dev 模式特有：Vite HMR / source map / DevTools profiler 自身持有

## 问题 4：删除未完成传输卡死

- 删除入口：前端 `delete_entry`（推测）→ Tauri command → `DeleteClipboardEntryUseCase`（在 `uc-application/facade/clipboard_history/`）
- 与传输的耦合：`file_transfer_lifecycle.rs:217-249` `cleanup_cached_path` 清理文件时，若 iroh 持有句柄会触发 EACCES/EAGAIN（macOS 上 unlink 仍然可以成功，但 Windows 上不行）
- **关键缺失**：没有显式"entry removed" domain event；前端 reducer 只监听 `file-transfer.status_changed` 和 `clipboard.new_content`，没有 entry removed 通道
- "重启后 entry 在 dashboard 消失"说明持久化删除（DB row）实际已经成功；卡死的只是 **前端等待回执** 这一步
- 根因假设（**待 Phase 3 实施时定位**）：
  - 后端命令同步等待 cleanup 完成；如果传输 in-flight 持有文件句柄 / 等待 sender drop，命令本身就 await 不返回
  - 或者命令已返回但前端没监听 deleted 事件 / 没更新 reducer

## Sender 端真相（advisor 复核确认）

- `BlobTransferPort::publish_path` (`src-tauri/crates/uc-infra/src/network/iroh/blobs.rs:221-273`) 同步把文件 add 进本地 iroh-blobs store，是 `.await`
- 完成后返回 ticket 给上层，**没有 "send loop"**
- 真正持续"传输"的是 **receiver 端 fetch task** 拉 blob
- iroh-blobs Router 在 sender 端做被动 provider；receiver 端 connection drop / EOF → provider 自然结束
- **不需要改 sender 端任何代码，不需要 iroh 双向控制通道**

## Receiver 端 fetch 路径

- `IrohBlobTransferAdapter::fetch_to_path` (`blobs.rs:320-383`)
  - 内部调 `ensure_blob_in_store` (`blobs.rs:439+`)
  - `ensure_blob_in_store` 用 `self.downloader().download(...).stream()` 拿 progress 流
  - 持有 `_connection` 给 ConnectionPool 一个 warm reference（但 pool 自己持有 connection，不复用我们的）
- 被 `FetchBlobUseCase::execute_to_path` (`fetch_blob.rs:127-166`) 调用
- 被 `BlobTransferFacade::fetch_blob_to_path` (`facade/blob_transfer/facade.rs:477+`) 编排

## iroh-blobs Downloader 关键事实（P1-9 验证）

读 `vendor/iroh-blobs/src/api/downloader.rs:1-380`：

- `Downloader::new` 在 `n0_future::task::spawn(actor.run(rx))` 起常驻 actor task（line 358）
- `download()` 通过 `irpc` client 发请求 → actor `self.spawn(handle_download)` 把任务挂到内部 JoinSet（line 84, 96）
- caller drop progress receiver → `handle_download` 里 `tx.send(...).await.ok()` 吞 SendError → **actor task 继续跑直到自然完成或网络出错**
- `ConnectionPool::close(id)` (`util/connection_pool.rs:454`) 能强制关掉对端 connection，但 Downloader 没暴露
- **解决**：vendor patch 给 Downloader 加 `shutdown_endpoint(id)` 转调 pool.close（P1-0 commit `56b29c25` + 主仓库 `12c9b388`）

## 横切观察

- **问题 1 + 问题 2 + 问题 4 共享同一架构方向**：单一 cancel 通道（receiver fetch token + iroh connection shutdown + Cancelled domain event）
- **问题 3 独立**：是前端 / payload 解析侧的资源占用问题，与传输路径无关

## 已完成实现的接线图

```
用户点取消按钮（前端 P1-6 待做）
       │
       ▼
[Tauri IPC]  cancel_file_transfer(transfer_id, reason)
       │   ← uc-tauri/src/commands/file_transfer.rs (P1-5 ✅)
       ▼
AppFacade::cancel_inbound_transfer(transfer_id, reason)
       │   ← uc-application/src/facade/app_facade.rs (P1-5 ✅)
       ▼
BlobTransferFacade::cancel_inbound_transfer
       │   ← uc-application/src/facade/blob_transfer/facade.rs:383 (P1-4 ✅)
       │
       ├── (1) inflight_fetches.remove(transfer_id) → token.cancel()
       │       → fetch_blob_to_path 里的 select! 唤醒 → 返回 BlobTransferError::Cancelled
       │
       ├── (2) BlobTransferPort::shutdown_inflight_fetch(ticket)
       │       → IrohBlobTransferAdapter (uc-infra/src/network/iroh/blobs.rs P1-3 ✅)
       │       → Downloader::shutdown_endpoint(endpoint_id)
       │       → ConnectionPool::close(id) [vendor P1-0 ✅]
       │       → actor task execute_get 报 Read(Reset)/ConnectionLost → 退出
       │
       └── (3) FileTransferFacade::cancel(CancelTransfer { ... })
               → 落 Cancelled domain event → projection → host event → 前端 status_changed

需要加进同一通道的入口（P1-8 待做）：
       ┌─ Timeout sweep（file_transfer_lifecycle.rs:114-154）
       └─ Delete entry（Phase 3）—— if entry has inflight transfer 先 cancel 再 cleanup
```

## 2026-05-23 新发现：sender 端"提前显示完成" + receiver 端"延迟显示 entry" 双 bug

### 复现
mac sender 复制 `archboot-...-ARCH-local-aarch64.iso` (实际 ~950MB)，win receiver 配对运行。

### 日志证据（UTC 时间）
- **07:41:57.916** `mac` add_path completed (streaming)，path = archboot-local-...iso，blob_hash=ab967ec3c5，tag=uc-clipboard-entry:**d1378f9a**
- **07:41:57.683** `win` `inbound: decoded V3 envelope`
- **07:41:57.684 → 07:41:57.694** `win` 第一次 fetch：`materialize: fetching representation-bound blob` → `blob inlined back into representation`（10ms，PNG 缩略图 115KB）
- **07:41:57.695 → 07:42:26.770** `win` 第二次 fetch：`materialize: fetching blob` → `blob cached to local path (streaming)`（**~29 秒**，950MB ISO）
- **07:41:57.696** `win` `WARN upsert_pending_transfer: skipping — existing row is not pending, existing_status="completed", transfer_id=f780b8df-...`（第二次 seed_lifecycle 失败，因为第一次已 mark completed）
- **07:41:57.696** `win` `WARN blob fetch: start lifecycle failed`（同上根因）
- **07:42:26.770** `win` `WARN blob fetch: complete lifecycle failed`（第二次 fetch 想 mark_completed，但 row 已是 completed → no-op，warn）
- **07:42:26.783** `win` `Clipboard capture completed entry_id=f780b8df-...` → 此刻 entry 才落 sqlite
- **07:42:27.299** `mac` 第三次 emit `file-transfer.status_changed`（之前 07:41:46/48/58 已经发过，每次 fetch 完成 receiver 都反向推 Completed）

### sqlite 状态
- `mac.clipboard_entry`：entry `d1378f9a` total_size=1009070 bytes (≈985KB)，title=archboot-local-...iso
- `mac.file_transfer`：**空**（sender 不创建 projection 行）
- `win.clipboard_entry`：entry `f780b8df` total_size=1009166（与 mac 不同 entry_id，receiver 自己 mint）
- `win.file_transfer`：transfer_id=`f780b8df` status=completed，**file_size=115443**（115KB PNG），filename=NULL
- `win` 端 sqlite 用 **WAL 模式**：`uniclipboard.db-wal` 2.1MB 包含全部新数据；只读主文件会看到两天前的旧状态（坑：dual-side-debug 调研时必须连 wal+shm 一起 cp，否则误判 "win 没收到 entry"）

### 根因 1：sender 端 UI "提前显示完成"

代码路径：
- `materializer.rs:110-112` 把 envelope 的 blob_refs 按 `representation_index.is_some()` 切成 `rep_refs` (内联回 representation) 和 `file_refs` (free-standing 文件落盘)
- `materializer.rs:115-182` 顺序对每个 rep_ref 调 `fetcher.fetch_blob(...)`（含 PNG 缩略图）
- `materializer.rs:193-...` 顺序对每个 file_ref 调 `fetcher.fetch_blob_to_path(...)`（含 950MB ISO）
- **关键**：所有 fetch 共用同一个 `transfer_id = receiver_entry_id`（`materializer.rs:140 / 217`）
- `BlobTransferFacade::fetch_blob` (`facade.rs:580-606`) 和 `fetch_blob_to_path` (`facade.rs:715-729`) 在每次 fetch Ok 后无条件调 `complete_lifecycle(ctx)` + `report_outbound_terminal(ctx, ..., OutboundProgressStatus::Completed)`

效果：
1. 第一次 (PNG, 10ms) fetch 结束 → receiver 推 Completed 给 sender → `space_setup.rs:155` translator emit `StatusChanged completed` → mac 前端 `useTransferProgress.ts:96-98` 调 `setEntryTransferStatus completed` + `markTransferCompleted` → FilePreview 显示绿色"传输完成"badge
2. 第二次 (950MB ISO, 29s) fetch 还在跑，但 sender UI 已经定格 completed
3. receiver 端两次 `seed_lifecycle/start_lifecycle/complete_lifecycle` 也都触发，第二次 upsert/transition 因 row 已 completed 而 fail-soft (warn)

### 根因 2：receiver dashboard 延迟 ~29 秒才显示 entry

代码路径：
- `apply_inbound/usecase.rs:265-274` 先 `materialize` 全部 blob，**等所有 fetch 完成** 才调 `capture.capture(...)` 落 entry
- entry 落库后才通过 host event 触发前端刷新
- 950MB ISO fetch 占 29 秒 → receiver dashboard 29 秒空窗

代码里注释提到"占位卡片"机制（`apply_inbound/usecase.rs:280-282`），但实际只有 `StatusChanged transferring` 事件能触发占位，且 placeholder 卡片的 UI 渲染依赖 entry 已存在；目前 receiver 端没有在 entry 落库前先 emit 一个 placeholder entry 的机制。

### 修复方向（待 user 决策）

**Bug 1 修法选项**：
- A. 给 `BlobTransferFacade::fetch_blob{_to_path}` 加 `is_last_of_batch: bool` 参数，只有最后一个 fetch 才 emit terminal lifecycle + outbound Completed
- B. 引入新 facade method `materialize_batch(refs: Vec<...>)`，内部 sequentially fetch，统一在最后 emit
- C. materializer 改成不传 `transfer_context` 给前 N-1 个 fetch（progress 失踪但状态不会提前 completed）
- 推荐 A：最小侵入，与既有单 blob 调用方式兼容

**Bug 2 修法选项**：
- D. apply_inbound 在 decode envelope 后立即 seed 一个 placeholder entry（用 receiver_entry_id），materialize 完成后再 finalize（update content/representations）
- E. 前端基于 `transferring` 事件创建 client-side placeholder 卡片，entry 落库后 reconcile
- 推荐 D：服务端真相唯一，前端简单

## 2026-05-23 Bug 2 复核：placeholder 机制已就位，需用户重新确认

### 后端
- `apply_inbound/usecase.rs:192` 在 V3 envelope decode 之后 **第一时间** emit `ClipboardHostEvent::IncomingPending { entry_id, from_device, total_bytes, filenames }`
- 注释明确："让前端立即出现占位卡片"

### 传输
- Win 端 daemon ws log（07:41:57.684 / 07:41:57.690 / 07:42:26.785 / 07:42:26.790 / 08:09:21.334）多次 `forwarding daemon websocket event to subscribed client event_type=clipboard.incoming_pending`
- WS 转发链路工作

### 前端
- `useClipboardEventStream.ts:82-110` 收到事件后 `dispatch(addPendingEntry(...))` + `setEntryTransferStatus({status: 'transferring'})`
- `ClipboardContent.tsx:395-413` 在 list 渲染逻辑里把 `pendingItems` 转为 placeholder 行（含 filenames + 进度 overlay + Receiving... 文案）
- placeholder 行先于真 entry 出现，`clipboard.new_content` 到达时 `removePendingEntry` 切换为真行

### 结论
**Bug 2 描述的"dashboard 没展示 entry"在当前代码里不应发生**。

可能的真实原因（按怀疑度）：
1. 用户复现时确实出现过 placeholder（灰色 Receiving 卡片）但没注意到，误以为没显示
2. placeholder 显示了，但样式/位置不显眼（被 filter 过滤掉、被滚动出视野）
3. dashboard 不在 Dashboard 路由（其他页面没挂 useClipboardEvents → 不监听 incoming_pending）
4. encryption_ready=false 时段事件被丢

### 待做
- 让用户再复现一次 ≥500MB 文件传输，重点观察 receiver 端 dashboard 列表顶部 29 秒空窗里是否出现 "Receiving..." 灰色卡片
- 如果真的没有，再启动 Bug 2 修复；如果有，标 Bug 2 关闭

## 2026-05-23 新发现：取消后 placeholder 永远置顶 + 重启丢失（问题 5）

### 用户报告
> 当接收方取消了传输，placeholder 依然会是置顶的第一个，除非重启应用，重启应用这个 entry 就消失了，说明这个 entry 没有写到数据库里。期望：取消后 entry 保留，也写进数据库，但文件是丢失的。并且也不应该继续置顶展示。

### 代码事实链

**触发流**（`uc-application/src/usecases/clipboard_sync/apply_inbound/usecase.rs`）：

| 步骤 | 文件：行 | 行为 |
|---|---|---|
| 1. emit IncomingPending | `usecase.rs:192-197` | 前端 `addPendingEntry` → `pendingItems.unshift(...)` (clipboardSlice.ts:223) → placeholder 置顶 |
| 2. materialize 开跑 | `usecase.rs:199-245` | 对每个 V3BlobRef 顺序 fetch |
| 3. 用户取消 → fetch err | `materializer.rs:177` / `:277` | `anyhow ?` 早退出 |
| 4. apply_inbound catch | `usecase.rs:211-224` | emit `StatusChanged status="failed"`(P1-10 后是 cancelled),**never calls capture** |
| 5. 返回 Err | `usecase.rs:223` | `ApplyInboundError::Internal` |

**前端 placeholder 处理**：
- `clipboardSlice.ts:217-224` `addPendingEntry`：用 `pendingItems.unshift(...)`，找不到时直接放数组头 → 永远在第一位
- `clipboardSlice.ts:231-233` `removePendingEntry`：filter by entryId
- 调用 `removePendingEntry` 的唯一入口：`useClipboardEventStream.ts:122`，触发条件是收到 `clipboard.new_content`
- `useTransferProgress.ts:104-110`：收到 cancelled status 只更新 `fileTransferSlice`（`markTransferCancelled` + `setEntryTransferStatus`），**完全不动 `pendingItems`**

**重启丢失**：`pendingItems` 是 redux in-memory state（`clipboardSlice.ts:48 initial`），无 localStorage / persist 配置，进程重启即清空。

### Materializer 当前结构

`InboundBlobMaterializer::materialize`（`materializer.rs:91-326`）：
- 入参：`from_device`、`receiver_entry_id`、`snapshot`、`Vec<V3BlobRef>`
- 出参：`Result<SystemClipboardSnapshot>` —— **没有 partial 概念**
- 内部：`rep_refs`（rep-bound blob，写回 representation）+ `file_refs`（free-standing 文件，落到 cache_dir）顺序 fetch
- 任意 fetch 失败 → `?` 早退出 → 上层拿不到"前 N-1 个已成功"的信息

### 关键事实
- materializer.rs:287 `local_paths.push(path)` —— 落盘成功才入列
- 第 N 个 fetch 在 cancel 触发时 `fetch_blob_to_path` 已 **部分写入** 目标 path,facade 内部清 partial（`facade.rs:789-796` cancel 分支 cleanup）
- rep-bound blob (PNG 缩略图) 通常排在 file_refs 前面（partition 顺序），所以 cancel 大文件时缩略图通常已成功 inline 进 snapshot

### 修复设计候选（待 advisor 复核）

#### 后端：partial materialize outcome

```rust
pub enum MaterializeOutcome {
    Complete(SystemClipboardSnapshot),
    PartialOnCancel {
        snapshot: SystemClipboardSnapshot,  // 已成功的 reps + 重写的 file-list rep（含 missing 标记）
        missing: Vec<MissingFileRef>,        // filename + size + reason
        completed_paths: Vec<PathBuf>,       // 已落盘成功的文件
    },
}
```

materializer 实现需要识别"这次 err 是 cancel 不是真错误"——可能让 `InboundBlobFetcher` 暴露一个 `is_cancel_error(&AnyhowError) -> bool` 或 fetcher 自己返回结构化错误。

apply_inbound 在 `(false, Some(materializer))` 分支收到 `PartialOnCancel`：
- 继续走 `capture.capture(...)`，用 partial snapshot
- 落库后 emit `clipboard.new_content` —— 复用现有路径
- emit `StatusChanged status="cancelled"` 时机不变（在 cancel_inbound_transfer 路径已经发了）

#### Missing files 在 file-list rep 的表达

选项：

A. **URI scheme `uniclip-missing://`**：rep_bytes 是 `text/uri-list`，把 missing 文件写成 `uniclip-missing:///{filename}?reason=cancelled&size=950000000`，前端解析时识别这个 scheme 渲染 missing 态。
- 优点：无 schema 变更，向前/向后兼容旧 DB；frontend 渲染时按 URI scheme 分支
- 缺点：合约语义重，需要前端解析逻辑

B. **clipboard_entry 加 `partial_files: bool` 列**：需 sqlite migration。
- 优点：简单查询
- 缺点：schema 迁移成本，与 ClipboardEntry domain model 强耦合

C. **依赖 file_transfer.status='cancelled' 做 join**：前端查 entry 时附带查 transfer 状态，cancelled 就标 missing。
- 优点：无 schema 变更；file_transfer projection 已有此状态（P1-10 落地）
- 缺点：每次列表渲染要 join；transfer 行可能因其他原因被清理（cleanup_cache_files、untag）
- file_transfer 行 lifecycle 与 entry 行不严格一对一（一个 entry 可能多 transfer），且 transfer 行被 `release_blob_tag` cleanup 时机不确定

D. **representation 上加 `missing_blobs: Option<Vec<MissingBlobMeta>>` 元数据**：扩展 `ObservedClipboardRepresentation` 结构，专门标记这个 rep 内嵌哪些 missing files。
- 优点：domain model 表达力更强；DB 序列化时可以走 representations 现有 blob column
- 缺点：domain model 改动

**倾向 A**：实施成本最低，schema 不动，跨进程跨设备语义清晰。前端把 `uniclip-missing://` 与 `file://` 一起识别，渲染时区分。

#### 前端

- `useTransferProgress.ts:104-110` cancelled 分支：先 `dispatch(removePendingEntry(entryId))`，再做现有 transfer state 更新
- 等 `clipboard.new_content` 到达走正常 entry 渲染流程
- `ClipboardItemRow` / `FilePreview` / `FileContextMenu` 解析 `uniclip-missing://`：灰色 icon + "文件已丢失" 文案 + 复制 / 打开 / 拖出 disabled
- i18n：`clipboard.fileMissing.cancelled` 等

### 边界情况

- cancel 在第一个 blob 都没开始：`completed_paths.is_empty()` → snapshot 完全没 materialize（reps 全空 bytes）→ 还能不能 capture？需要确认 `CaptureClipboardUseCase` 对空 representation 的容忍度
- cancel 在 rep-bound blob (PNG) 阶段 → file_refs 完全未开始，partial snapshot 里 PNG rep 是 inline 成功的、但 file-list rep 完全是 missing
- dedup 逻辑（`usecase.rs:248-259` `find_recent_duplicate`）：partial entry 是否会被认为是 dup？`content_hash` 是 envelope 全包 hash，不依赖 fetch 结果，所以 dup 判定不变
- cancel 与 timeout sweep 同时触发（P1-8 实施后）：cancel reason=Timeout 也走同一 partial 路径

## 2026-05-23 Phase 4 取证：复制 1GB zip 即触发 webview 内存暴涨

### 复现条件
- 应用刚启动，无其他动作
- 在 macOS Finder 里 cmd+C **一个 1GB zip 文件**（不展开 entry，不传输，无 peer 配对）

### T0 baseline（应用刚启动）
```
webview 65413  MEM=453M  CMPRS=61M  → 实际压力 ~514M
rust    65199  MEM= 80M  CMPRS=26M  → ~106M
```
即使空载，webview 514M 也偏高（dev 模式 + Vite HMR 合理上限是 400M）。

### T1（复制 1GB zip 后立刻）
```
webview 65413  MEM=1395M  CMPRS=103M  → +942M
rust    65199  MEM= 117M  CMPRS= ??   → +37M
```

### T1+30s（无任何操作，T2 静置）
```
webview 65413  MEM=1553M  CMPRS=1276M  → 实际压力 ~2.7GB
rust    65199  MEM=  95M  CMPRS=  67M  → ~162M（基本无变化）
```
**关键观察**：
- 30s 内 webview 占用 **继续上升** 到 2.7GB（resident 1.5GB + macOS compressed 1.2GB）
- rust daemon 几乎不动 —— **数据没有持久驻留在后端 Rust 进程**
- 数据流向：file → ??? → webview 内存

### 已排查（不是根因）

| 路径 | 文件：行号 | 结论 |
|---|---|---|
| 平台 macOS pasteboard Files 分支 | `uc-platform/src/clipboard/common.rs:311-343` | 只读 file:// URI（~200B），不读文件内容 |
| 平台 macOS pasteboard Image 分支 | `common.rs:366-481` | 仅 `ctx.has(ContentFormat::Image)` 才触发；zip 文件 Finder 不会塞 Image content |
| `clipboard.new_content` WS event payload | `uc-webserver/src/api/event_emitter.rs:130-147` | 只含 `{ entry_id, preview, origin }`，不含 bytes |
| `list_entries` HTTP response | `uc-webserver/src/api/clipboard.rs:79-100` 312-333 | `EntryProjectionResponseDto` 只含 metadata（file_names、file_sizes），不含 inline_data |
| 前端 redux reducer | `src/store/slices/clipboardSlice.ts` + `clipboard-transform.ts` | 仅存 metadata，不存 bytes |

### 可疑但需要堆快照验证

| 嫌疑 | 文件：行号 | 触发条件 | 是否对 zip 生效 |
|---|---|---|---|
| `publish_oversized_inline_blob_refs` 阈值仅对 `image/*` | `clipboard_outbound/mod.rs:392` | 仅 outbound（无 peer 时不触发） | 对 zip 不生效，且本场景无 peer |
| `ClipboardPayloadResolver::resolve()` 无条件 clone inline_data | `uc-infra/src/clipboard/payload_resolver.rs:73` | 仅前端 fetch resource 时调；用户未展开 | 不该触发 |
| `entry_resource_to_dto` base64 编码 inline_data | `uc-webserver/src/api/clipboard.rs:353` | 仅前端请求 `/clipboard/entries/:id/resource` 时调 | 不该触发 |
| `representation_cache.put()` | `clipboard_capture/usecase.rs:283-286` | capture 阶段无条件 put inline_bytes | 但 rust daemon 仅涨 ~20MB，证伪 |
| `common.rs` 后续 image-from-file fallback | `uc-platform/src/clipboard/common.rs:480+` | 注释提到"opportunistic load image bytes when clipboard only contains file ref to image" | 仅图片扩展名，zip 不该触发 |

### 推理 vs 实测的矛盾

代码 grep 找不到任何明显的"复制 zip 时把全文件读到 webview"路径，但实测确实涨了 ~2.7GB。两种可能：

1. **WKWebView / DevTools / Inspector 内部缓存**：如果 Web Inspector 被 attach 到 webview，会持有所有对象快照，量级与 2.7GB 吻合。需排除：关闭所有 dev tools，重启复测
2. **某条没找到的代码路径**：Tauri IPC convertFileSrc / asset:// 协议 / 某个 React 组件在 entry 列表项渲染时读了文件 URL（即使 zip 没有 preview，可能仍 fetch 了 thumbnail URL）

### 下一步取证（待用户配合）

1. **关闭 Web Inspector**（如果开着）→ 重启 app → 重复 T1 → 看是否仍涨。若不涨：root cause 是 Inspector 缓存（dev-only 不修）
2. **打开 Safari Web Inspector** → 复制前 take heap snapshot → 复制后 take heap snapshot → diff → 找新增的 ≥100MB 对象（Uint8Array / ArrayBuffer / String）
3. **如果是 String**：定位到 base64 化的 inline_data
4. **如果是 ArrayBuffer/Uint8Array**：定位到 `URL.createObjectURL` / `fetch()` blob response 持有

### 取证更新（同一 session）

**Web Inspector 排除**：关掉 Inspector 后等 30s，webview 仍占 2.94GB（只跌 ~50MB）。Inspector 不是元凶。

**JS 堆排除**：Console 跑 `queryObjects(Uint8Array/ArrayBuffer/Blob/String>1MB)` 全部返回 0 或仅 128 字节小对象。**JS 层没有大对象**。

**DOM 排除**：`querySelectorAll('img/video/audio/canvas')` 全部 0；CSS `background-image` 没有 file://blob:/大 base64。**DOM 层没有大对象**。

**Storage 排除**：Inspector Storage tab 显示 IndexedDB / Local Storage / Session Storage 都为空。

**持续增长证据**（关键）：从 T1 到 T1+90s，webview 占用：
```
T1 后 ~30s：  MEM=1553M  CMPRS=1276M  → ~2.8GB
T1 后 ~60s：  MEM=1645M  CMPRS=1310M  → ~2.95GB
T1 后 ~90s：  MEM=2748M  CMPRS=2379M  → ~5.1GB
```
**90 秒内从 2.8GB 涨到 5.1GB**。这是真泄漏（持续增长），不是一次性占用。

**跨平台确认**（用户口头）：Windows 接收方 webview 也有相同问题。所以 **不是 macOS 共享 pasteboard 假象**，是真的 app 代码 bug，且在 sender (copy) 和 receiver (downloading) **两个方向都触发**。

### 假设排除清单

| 假设 | 状态 |
|---|---|
| WKWebView 内部图片解码缓存 | ❌ 0 个 `<img>` 元素 |
| `<video>`/`<canvas>` 持有解码帧 | ❌ 0 个 |
| JS 大对象（base64 字符串 / Uint8Array） | ❌ queryObjects 0 |
| IndexedDB / localStorage | ❌ 空 |
| Web Inspector 自身缓存 | ❌ 关掉后内存不降 |
| macOS pasteboard 共享内存假象 | ❌ Windows 接收方也复现 |
| `inline_data` 通过 HTTP API 推到前端 redux | ⚠️ 链路看起来不携带 bytes（待 RUST_LOG=trace 复测确认） |
| 某个定时器在持续 fetch 文件 / 触发渲染 | 🔍 **当前主嫌疑**：90s 涨 2GB+ 强烈暗示循环 |
| WebKit 网络层 / GPU 进程缓存 response | 🔍 待 Network tab 取证 |

### vmmap 取证落地（**根因确认**）

切到 dev profile 第二个 webview PID（98363）跑 `vmmap -summary`：

```
Physical footprint:         1.0G
Writable regions: Total=70.9G written=1.4G(2%) resident=1.1G

REGION TYPE                    VIRTUAL  RESIDENT   DIRTY  SWAPPED  COUNT
WebKit Malloc                     1.8G     1.0G   497.5M   403.4M     60   ← 这里
MALLOC ZONE WebKit Malloc_0x...   2.7G     1.0G   510.7M   406.7M     63   2,899,497 个分配 / 825.1M 已分配
```

JS heap 几乎空（JS JIT 9MB resident、Gigacage 48K resident），DOM 358 元素（正常）。

**290 万个 WebKit C++ 小对象**（平均 ~300 字节）—— 不是 DOM 不是 JS，是 WebKit native 层（MessageEvent / PerformanceResourceTiming / CSS style / Cached resource 之类的内部对象）。

### 根因

`uc-infra/src/network/iroh/blobs.rs:28-37` 节流逻辑 bug：

```rust
const PROGRESS_REPORT_BYTES: u64 = 256 * 1024;
const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_millis(200);

if (due_by_bytes || due_by_time) && total > last_reported_bytes {
    sink.report(total, None).await;
```

OR 让两个阈值"任一满足即 emit"。高带宽 wifi 下每 5ms 跨过 256KB → `due_by_bytes` 几乎一直 true → 实测 18 emits/sec（与日志统计一致：5min 5480 个 file-transfer.progress 事件）。

每个 WS frame 进 webview → JS handler → Redux dispatch → React render → WebKit 内部产生若干 MessageEvent / PerformanceEntry 之类的 C++ 对象。18/秒 持续打入，GC 跟不上 → 累积。

### 修法（commit 待做）

`uc-infra/src/network/iroh/blobs.rs`：删 `PROGRESS_REPORT_BYTES`，emit 条件改为 **time-only**（硬上限 5 emits/sec）：

```rust
let due_by_time = last_reported_at
    .map(|t| t.elapsed() >= PROGRESS_REPORT_INTERVAL)
    .unwrap_or(true);
if due_by_time && total > last_reported_bytes {
    sink.report(total, None).await;
```

时间窗 200ms = 5 emits/sec 上限，对人眼足够流畅；慢速传输也保证 200ms 至少更新一次（不会停滞）。

`cargo test -p uc-infra --lib network::iroh::blobs` 17/17 通过；`cargo check --workspace` 全通过。

### 待用户复测

跑 `cargo build` + 重启 dev profile + 复制 1GB zip，30s+90s 后 webview 内存应保持在 ~700MB 以下（baseline + 少量 React/DOM 增长）。如果还涨到 GB 级，说明还有第二条 emit 源（比如 outbound reporter 自己也在被高频调用），需要继续查 `transfer_progress_adapter.rs`。

---

## 2026-05-23 早期 Phase 4 探索（已被上面的 vmmap 结果取代）

### 下一步（用户决策：重启 app + RUST_LOG=trace 复跑）

启动命令：
```
RUST_LOG='info,uc_application::clipboard_capture=debug,uc_platform::clipboard=debug,uc_webserver=debug,uc_application::facade::host_event=debug,uc_application::facade::clipboard_history=debug' pnpm tauri dev
```

操作：
1. 杀掉当前 `pnpm tauri dev`（Ctrl-C）
2. 用上面命令重启
3. 等应用完全启动（看到 webview 出现）记 baseline 内存
4. 复制 1GB zip
5. 静置 60-90s
6. tail daemon log 后 200 行贴回来

关注的关键模式（grep 也可）：
- `clipboard_capture` 是否在循环 capture（同一个 entry_id 多次）
- `uc_platform::clipboard` 是否在持续 poll（频率应该 250ms 左右一次）
- `uc_webserver` HTTP request 路径：`GET /clipboard/entries` 是否被前端反复调
- `host_event` 是否在 emit `new_content` 或 `progress` 类事件给前端
- 任何带 zip 文件名的日志行

---

## 2026-05-23 新发现：cancelled 语义被三层压扁 + sender 端也显示 failed

### 用户原话
> 用户取消，算传输失败吗？... 双方都不应该认为用户主动取消是失败，发送方也显示传输失败

UI 现状：FilePreview 渲染 `cancelled:local_user` 作为红色"失败"徽章 + 原始 reason 字符串。

### 语义压扁链（receiver 侧）

| 层 | 状态 | 文件：行 |
|---|---|---|
| uc-core 领域 | ✅ `FileTransferEvent::Cancelled` 独立事件；`FileTransferCancellationReason` 五变体 | `core/src/file_transfer/event.rs:49,79` |
| uc-application host publisher | ❌ **Cancelled event 硬编码 wire status="failed"**，只在 reason 字段塞 `cancelled:*` | `application/src/facade/host_event/publisher.rs:185-192` |
| uc-infra projection | ❌ **Cancelled event 落 `TrackedFileTransferStatus::Failed`**，reason 列塞 `cancelled:*` | `infra/src/file_transfer/projection/sqlite.rs:64-76` |
| uc-core projection enum | ❌ `TrackedFileTransferStatus` 只有 4 档 `{Pending, Transferring, Completed, Failed}`，无 Cancelled | `core/src/ports/file_transfer_repository.rs:11-20` |
| 前端 reducer | ❌ `useTransferProgress.ts:78` 只接受 `pending/transferring/completed/failed`；status_changed 走 `markTransferFailed` | `src/hooks/useTransferProgress.ts:78-95` |
| 前端 EntryTransferStatus | ❌ `status` 只允许 4 档 | `src/store/slices/fileTransferSlice.ts:21-24` |
| 前端 FilePreview | ❌ 红色 AlertTriangle 徽章 + 直接渲染 raw reason 字符串 | `FilePreview.tsx:79-84, 152-157` |

### 关于"发送方也显示传输失败"——关键架构发现

**Sender 端的 transfer 状态完全由 receiver 反向推过来**，通过 `OutboundProgressReporterPort`（`uc-core/src/file_transfer/outbound_progress.rs`）：

- 通道：iroh 反向 ALPN 单向流，frame 定长 34 字节 (`uc-infra/src/network/iroh/transfer_progress_wire.rs`)
- 协议：`magic(1) + transfer_id_uuid(16) + bytes(8) + total(8) + status(1) + FIN`
- 状态字节当前定义：`0x01=InProgress / 0x02=Completed / 0x03=Failed`
- Receiver fetch sink 每次回调通过 `HostEventProgressSink::report` 旁路调用 reporter (`facade/blob_transfer/facade.rs:875-885`)
- Sender 收到帧后把状态映射成本地 `FileTransferEvent::{Progress, Completed, Failed}`
- 错误处理：当前 receiver 端 fetch 报 `BlobTransferError::Fetch` 时只调 `report_outbound_terminal(..., Failed)` (`facade.rs:653, 789, 803`)

**含义**：
- **不需要新建任何信令通道**——之前 task_plan 判定"不要建 iroh bi 控制通道"对 cancel 同步信号同样成立
- **复用现有反向 progress 通道** 即可让 sender 看到对方"取消"

### 完整修复方案（advisor 待复核）

#### 后端（4 处）

1. **`uc-core::OutboundProgressStatus`** 新增 `Cancelled { reason: FileTransferCancellationReason }` 变体
2. **wire 编码**（`transfer_progress_wire.rs:63-69, 128-132`）：status 字节新增 `0x04..0x08` 编码 5 种 cancel reason；不破坏帧长，不需要跳 ALPN
   - 注：旧 sender 收到新 status 会 `UnknownStatus` → 单帧丢失，sender 看到的是"InProgress 后无终态"直到 timeout sweep 兜底。可接受（桌面 app 两端同步升级）
3. **receiver cancel 路径**（`BlobTransferFacade::cancel_inbound_transfer` `facade.rs:483+`）：撕 connection 前先调一次 `reporter.report(..., Cancelled { reason })`，让 sender 收到终态
4. **sender 收到 Cancelled status** → 落 `FileTransferEvent::Cancelled`（而非 Failed）

#### 后端 cancelled 语义恢复（receiver 侧 wire / projection 解压扁）

5. **`TrackedFileTransferStatus`** 新增 `Cancelled` 变体（`core/src/ports/file_transfer_repository.rs:11-20`）
6. **`uc-infra` projection**：`Cancelled` event 落 `status=Cancelled`，reason 列去掉 `cancelled:` 前缀，只存子原因
7. **`uc-application` host publisher**：`Cancelled` event 用 wire status="cancelled"（不再压扁成 "failed"）

#### 前端（3 处）

8. **`EntryTransferStatus['status']`** 加 `'cancelled'`（`fileTransferSlice.ts:21-24`）
9. **`useTransferProgress`**：白名单加 cancelled；status_changed 走新 reducer `markTransferCancelled`
10. **FilePreview / ClipboardItemRow**：cancelled 用中性灰色 + i18n（`'clipboard.transfer.cancelled.local_user'` 等）

### 决策待 advisor 复核

- 拓展现有 OutboundProgressStatus enum vs 跳 ALPN 版本
- 是否新增 `FileTransferEvent::Cancelled { ... }` 在 sender 端的产生路径（receiver fetch 反推），还是让 sender 直接落 Cancelled status 不发 event
- `DeliveryFailureReason`（小内容广播路径）是否同步加 Cancelled 变体，还是仅大文件传输路径处理
