# 任务规划：大文件直连传输四个问题

> 临时会话规划文件（task_plan / findings / progress），不入库。正式 todo 沉淀到 `.planning/todos/` 后清理。

## 目标

修复大文件直连传输暴露的 5 个用户可见问题：

1. **接收方 timeout，发送方仍在传输** —— 单边 timeout，缺双向取消信号
2. **缺少取消按钮** —— `CancelTransferUseCase` 已就绪，缺前端 + Tauri command
3. **Webview 进程内存异常** —— 与传输路径无关；嫌疑在缩略图 / 预览 / dev 模式
4. **删除未完成传输卡在"删除中"** —— DB 实际已删，前端不收回执
5. **取消后 placeholder 一直置顶 + 重启消失** —— cancel 时 entry 未落库，前端 pendingItems 无清理

## 关键背景（来自 findings）

详细见 `findings.md`。简要：

- 项目已有两个 pending todo 直接对应问题 1 和 2：
  - `2026-04-17-move-file-transfer-timeout-sweep-off-repository-port.md`
  - `2026-04-17-wire-real-filetransferevent-cancelled-emitter.md`
- 问题 1 + 2 + 4 共享同一架构方向：**接收方 fetch task 可被外部 cancel + 单一 cancel 通道**
- 问题 3 与传输路径无关（progress wire 是 34 字节定长，不带 bytes），独立处理
- **关键事实**：sender 端没有"send loop"，`publish_path` 只是把文件 add 进本地 blob store；之后是被动 provider。持续传输的是 **receiver fetch task**。因此修复焦点是让 receiver fetch task 可中止，而非 sender 端
- iroh-blobs `Downloader::download` 把任务挂在内部 actor 的 JoinSet 上，**caller drop 不会取消**。必须 `ConnectionPool::close(id)` 撕 QUIC connection。Downloader 没暴露这个 API → **改 vendor 加 `shutdown_endpoint`**

## 推荐顺序（按依赖）

1. ✅ **P1-0** vendor patch: `Downloader::shutdown_endpoint`
2. ✅ **P1-3** `BlobTransferPort::shutdown_inflight_fetch` + iroh adapter 实现 + fake
3. ✅ **P1-4** `BlobTransferFacade::cancel_inbound_transfer` + `select!` 包 `fetch_blob_to_path`
4. ✅ **P1-5** `cancel_file_transfer` Tauri command（含 `AppFacade::cancel_inbound_transfer`）
5. ✅ **P1-6** 前端：取消按钮 + thunk + reducer
6. ✅ **P1-8** timeout sweep 走同一 cancel 通道（commit `26689c4f`，2026-05-22）
7. ✅ **P1-10** cancelled 语义全链路恢复（双端不再把"用户取消"显示为"传输失败"）
8. 🟡 **P1-7** 复现验收：CASE 1（cancel 路径）✅ session 5；CASE 2（成功路径）依赖用户重跑 daemon 模式
9. ✅ **Phase 5** 问题 5：取消后 entry 落库 + 前端去 placeholder（commit `6922e632`，2026-05-23）
10. ⏸ **Phase 3** 问题 4：删除流程修复（依赖用户实机复现以定位卡点）
11. ✅ **Phase 4** 问题 3：Webview 内存（commit `77ac2832`，2026-05-23）—— 双层节流（blobs.rs receiver 端 + space_setup.rs sender 端 translator），5/sec 硬上限；2GB 传输实测 webview 稳在 ~500MB（之前 2-5GB）
12. 🧹 **收尾** 关闭 pending todos（`move-file-transfer-timeout-sweep-off-repository-port`、`wire-real-filetransferevent-cancelled-emitter`）

## 阶段

### 阶段 0：调研与定位（✅ 已完成）
见 `findings.md`。

### 阶段 1：接通 cancel 全链路（7/8 已完成）

**关键转向**：sender 端 **不需要** 主动中止机制，也 **不需要** 新建 iroh 双向控制通道。所有 4 个用户报告问题都是 **receiver 视角**，焦点是让 receiver 端的 fetch task 持有 `CancellationToken` + 撕 QUIC connection。

#### 已完成 commit（adaptable-finch 分支）

| Commit | 任务 | 说明 |
|---|---|---|
| `12c9b388` | P1-0 | chore(vendor): bump iroh-blobs to add Downloader::shutdown_endpoint |
| `a3895a1f` | P1-3 | feat(blob-transfer): add BlobTransferPort::shutdown_inflight_fetch |
| `08b247a7` | P1-4 | feat(blob-transfer): cancel_inbound_transfer + select! cancel in streaming fetch |
| `9c573faf` | P1-5 | feat(file-transfer): Tauri command cancel_file_transfer |
| `827f7f9a` | P1-6 | feat(file-transfer): receiver-side cancel button + thunk（5 月 22 日完成，挂在 `TransferProgressBar` + `ClipboardPreview`，走 `src/api/tauri-command/file_transfer.ts` thin wrapper） |
| `ff12a72f` | P1-10 | feat(file-transfer): treat cancelled distinct from failed across full stack（2026-05-23；wire status 0x04..0x08 + projection + publisher + UI 灰色徽章 + i18n） |
| `26689c4f` | P1-8  | feat(file-transfer): route Transferring timeouts through cancel_inbound_transfer（2026-05-22；sweep 在 Transferring 行走 cancel 通道，Pending 行保留 mark_failed 旧路径） |
| `bcd41143` | bug1  | fix(file-transfer): batch-aware lifecycle in multi-blob fetch（2026-05-23；防止 PNG 缩略图 fetch 完成把整批 transfer 提前标 completed） |
| `6922e632` | P5    | feat(file-transfer): persist partial entry when inbound transfer is cancelled（2026-05-23；问题 5 全链路修复） |

vendor submodule 也有一个对应 commit `56b29c25 feat(api/downloader): expose shutdown_endpoint for external cancel` 在 `uniclipboard/0.100.0-patched` 分支上。

#### 剩余任务

##### P1-6 ✅ 已完成（commit `827f7f9a`，2026-05-22）
- 实现位置：`TransferProgressBar` 的 X 按钮 + `ClipboardPreview` 接线 + `src/api/tauri-command/file_transfer.ts` thin wrapper
- 本 session 2026-05-23 曾误以为 P1-6 待做，重复实现了一套（FilePreview + redux thunk），发现冲突后回滚
- 教训：跨 session 必须先核对 `git log --oneline` 与 task_plan 的 ⏸/✅ 状态是否一致，再动手

##### P1-8 timeout sweep 走同一 cancel 通道
- 改 `src-tauri/crates/uc-bootstrap/src/file_transfer_lifecycle.rs:114-154` 的 `spawn_timeout_sweep`
- 现状：timeout 触发后直接 `repo.mark_failed` + emit `TransferHostEvent::StatusChanged failed`，**不撕 receiver fetch task** —— 发送方继续传，bandwidth 浪费
- 改造：把 mark_failed 路径改成调 `BlobTransferFacade::cancel_inbound_transfer(transfer_id, FileTransferCancellationReason::Unknown)`（或新增 `Timeout` 变体）
- 需要：lifecycle 现在没有 `BlobTransferFacade` 引用；要在 `build_file_transfer_assembly` 装配期把 facade 注入进 `FileTransferLifecycle`，或者通过 `AppFacade` 调用
- 注意 lifecycle 是在 bootstrap 装配的，FileTransferFacade 也是 bootstrap 装配的，建议把 `Arc<BlobTransferFacade>` 也传进 `FileTransferLifecycle::new`
- 关闭 pending todo `move-file-transfer-timeout-sweep-off-repository-port`

##### P1-10 ✅ 已完成（commit `ff12a72f`，2026-05-23）

cancelled 语义全链路恢复 —— 双端 UI 不再把"用户取消"显示为"传输失败"。

**关键设计点**（落进代码 + commit message）：
- 复用 `OutboundProgressReporterPort` 反向通道（独立 ALPN，与 fetch connection 物理隔离），不新建任何信令
- wire status byte 扩展 `0x04..0x08` 编码 5 个 cancel reason，不动 34 字节帧长、不跳 ALPN
- `cancel_inbound_transfer` 撕 QUIC 之前先 `reporter.report(Cancelled).await`，独立 ALPN 保证撕 fetch connection 不影响 cancel 帧
- 视角翻转：receiver 视角的 `LocalUser`/`RemotePeer` 沿反向通道发给 sender 时对调，sender UI 拿到的就是自己视角的 reason
- DB 不迁移：旧行保留 `failed + cancelled:*`，前端 `resolveEntryTransferStatus` 加 fallback 识别 `cancelled:` 前缀

**事实链** 详见 `findings.md` 2026-05-23 章节。

<details><summary>历史实施步骤（仅供回溯，不再需要执行）</summary>

1. **`uc-core`** (`core/src/file_transfer/outbound_progress.rs`):
   - `OutboundProgressStatus` 加 `Cancelled { reason: FileTransferCancellationReason }` 变体
   - reason 使用现有 `FileTransferCancellationReason` 枚举（5 变体）

2. **`uc-core`** (`core/src/ports/file_transfer_repository.rs:11-20`):
   - `TrackedFileTransferStatus` 加 `Cancelled` 变体
   - `as_str()` / `from_str_value` 同步：`"cancelled"`

3. **`uc-infra` wire** (`infra/src/network/iroh/transfer_progress_wire.rs:63-69, 128-132`):
   - status 字节分配：`0x04=LocalUser / 0x05=RemotePeer / 0x06=Replaced / 0x07=Timeout / 0x08=Unknown`
   - `ProgressFrame::status_byte()` + decode match 同步扩展
   - **不动帧长**（仍 34 字节），ALPN 不跳版本
   - 注释里更新 frame layout doc-comment

4. **`uc-infra` reporter** (`infra/src/network/iroh/transfer_progress_adapter.rs`):
   - 对 `OutboundProgressStatus::Cancelled { reason }` 写帧时映射到对应 status byte
   - 单元测试加 round-trip：Cancelled 五个变体

5. **`uc-infra` projection** (`infra/src/file_transfer/projection/sqlite.rs:64-76`):
   - `FileTransferEvent::Cancelled` 落 `status=Cancelled`（而非 Failed）
   - reason 列存子原因（`"local_user"` / `"remote_peer"` / ...），不带 `cancelled:` 前缀
   - **DB 不迁移**：旧行保留 `failed + cancelled:*`，前端 fallback 兼容

6. **`uc-application` host publisher** (`application/src/facade/host_event/publisher.rs:185-192`):
   - `FileTransferEvent::Cancelled` 发 wire `status="cancelled"`（而非 `"failed"`）
   - `cancellation_reason_label` 去掉 `cancelled:` 前缀（reason 字段已经隐含了"是 cancel"的语义）

7. **`uc-application` blob_transfer facade** (`application/src/facade/blob_transfer/facade.rs:483+ cancel_inbound_transfer`):
   - 撕 connection **之前** 先 `reporter.report(target, transfer_id, bytes, total, Cancelled { reason }).await`
   - 必须 await 完成才调 `shutdown_inflight_fetch`，避免 race
   - 同时核对：fetch error 路径触发的 `report_outbound_terminal(..., Failed)`（`facade.rs:653, 789, 803`）仍发 Failed —— 那是真正的 fetch 失败，不是 cancel

8. **`uc-bootstrap` sender translator** (`bootstrap/src/space_setup.rs:153-167`):
   - `match event.status` 加 `OutboundProgressStatus::Cancelled { reason }` 分支
   - emit `StatusChanged { status: "cancelled", reason: Some(reason_str) }`，reason_str 用 `FileTransferCancellationReason` 的 snake_case 名（与 publisher.rs 对齐）

9. **前端 reducer** (`src/store/slices/fileTransferSlice.ts:21-24, 245-265`)：
   - `EntryTransferStatus['status']` 加 `'cancelled'`
   - 新增 reducer `markTransferCancelled({ transferId, reason })`
   - `resolveEntryTransferStatus` 加 `'cancelled'` 分支
   - **fallback 兼容旧 DB 行**：`if entryStatus.status === 'failed' && entryStatus.reason?.startsWith('cancelled:')` → 视为 cancelled

10. **前端 event handler** (`src/hooks/useTransferProgress.ts:78-95`)：
    - validStatuses 白名单加 `'cancelled'`
    - status === 'cancelled' 时调 `markTransferCancelled`
    - reason 字符串不带 `cancelled:` 前缀（后端已剥离）

11. **前端 UI** (`FilePreview.tsx:79-84, 152-157` + `ClipboardItemRow.tsx`)：
    - 加 `effectiveStatus === 'cancelled'` 分支：灰色 `XCircle` / `MinusCircle` 徽章 + `t('clipboard.transfer.cancelled')`
    - reason 横幅：用 `t('clipboard.transfer.cancelReason.' + reason)` 映射（5 个 i18n key）
    - **不再** 用 destructive 红色

12. **i18n 文案**（`src/locales/zh.json` + `en.json`）：
    - `clipboard.transfer.cancelled`: "已取消"
    - `clipboard.transfer.cancelReason.localUser`: "你取消了此次传输"
    - `clipboard.transfer.cancelReason.remotePeer`: "对方取消了此次传输"
    - `clipboard.transfer.cancelReason.timeout`: "传输超时已自动取消"
    - `clipboard.transfer.cancelReason.replaced`: "已被新内容替换"
    - `clipboard.transfer.cancelReason.unknown`: "传输已取消"

13. **TS bindings 重生成**：`cd src-tauri && cargo test -p uc-tauri --test specta_export`

14. **测试**：
    - wire round-trip Cancelled 五个变体（`transfer_progress_wire.rs::tests`）
    - projection Cancelled 落 status=Cancelled（`projection/sqlite.rs::tests`）
    - publisher Cancelled 发 status="cancelled"（`publisher.rs::tests` 如存在）
    - reporter race：cancel_inbound_transfer 先发 Cancelled 帧再 shutdown

**Scope 提醒**：~14 处改动，跨 4 crate + 前端。要么 user 自己实施，要么委托。

</details>

##### P1-7 复现验收
- 起 dev profile：`pnpm tauri dev`（或仓库内特定脚本）
- 用一台 Mac + 一台 Windows / 第二台 Mac 配对，传一个 ≥500MB 文件
- 用例 A：传到 30% 接收方点取消 → 接收方 UI 立即 cancelled、发送方 UI 也变 cancelled、dev 日志能看到 `cancel_inbound_transfer` + `blob fetch: shutdown_endpoint dispatched`、发送方 ActivityMonitor 上的 daemon 网络流量立即归零
- 用例 B：传到 30% 拔接收方网线 5 分钟 → 接收方 timeout 触发 → 双端都 cancelled，发送方流量归零（P1-8 实施后）
- 关闭 pending todo `wire-real-filetransferevent-cancelled-emitter`

### ~~阶段 2~~（已并入阶段 1 P1-8）

### 阶段 3：删除流程修复（问题 4）

**当前状态**：调研完成，等复现定位卷点（详见 progress.md session 6）。

**已知事实**：
- 删除走 daemon HTTP（不是 Tauri command），路径：
  `DELETE /clipboard/entries/:id` → `clipboard.rs:152 delete_entry` → `clipboard_history::facade::delete_entry` → `DeleteClipboardEntryUseCase::execute`
- 前端 reducer 健全：thunk fulfilled 解锁 `isDeleting`，根因在后端 HTTP 不返回
- use case 当前 **不在 delete 前 cancel in-flight transfer**，可能在 untag / cleanup_cache_files / delete_event 之一阻塞
- cancel 链路已就绪：`BlobTransferFacade::cancel_inbound_transfer(transfer_id, reason)`，需查 `FileTransferRepositoryPort::list_transfers_for_entry(entry_id)` 拿 transfer_id

**Plan（待复现确认后实施）**：
1. 在 uc-core 加 `trait InflightTransferCancellation { cancel_inflight_for_entry(entry_id) -> Result<()> }`
2. `BlobTransferFacade` 实现该 trait（内部查 entry → 拿 transfer_id 列表 → 对每个调 `cancel_inbound_transfer(_, LocalUser)`）
3. `DeleteClipboardEntryUseCase::with_inflight_cancel(Arc<dyn InflightTransferCancellation>)`，execute 开头先 cancel
4. 装配点：`clipboard_history` facade 构造时注入
5. **不新增** `FileTransferCancellationReason::EntryDeleted`，复用 `LocalUser`（advisor 建议）

**待复现产出**：哪个 span enter 后不 exit，决定优先修哪里。

### 阶段 5：取消后 placeholder 持久化（问题 5）

**状态**：调研完成，用户已选定方案，待实施。

#### 用户决策
- **Entry 形态**：落库 + 已成功的 representations 保留 + 未成功的 file refs 标"missing"
- **置顶问题**：cancel 时 placeholder 立即转为真 entry 行（按 createdAt 排序）

#### 根因（核实代码）

代码路径 `src-tauri/crates/uc-application/src/usecases/clipboard_sync/apply_inbound/usecase.rs:192-224`：

```
1) emit IncomingPending          → 前端 addPendingEntry，placeholder 置顶
2) materializer.materialize(...)  → fetch 跑 N 秒
   ↓ 用户点取消
   ↓ fetch_blob_to_path → BlobTransferError::Cancelled
   ↓ materializer 用 anyhow ? 早退出（materializer.rs:177, 277）
   ↓ apply_inbound .map_err 分支触发 (usecase.rs:211-224)
3) emit StatusChanged status="failed"  (P1-10 之后是 cancelled)
   ❌ 没调 self.capture.capture(...) → DB 没有 entry
   ❌ 前端没收到 clipboard.new_content → removePendingEntry 永不触发
4) pendingItems 是 in-memory + unshift → 永远置顶；重启即丢
```

下游链：
- `useTransferProgress.ts:104-110` 收 cancelled 只更新 fileTransferSlice，**不动 pendingItems**
- `clipboardSlice.ts:217-224` `addPendingEntry` 用 `unshift` 永远进列表头
- `clipboardSlice.ts:231-233` `removePendingEntry` 只在 `clipboard.new_content` 时调（`useClipboardEventStream.ts:122`）

#### 设计要点（advisor 复核后修订版）

**关键 hazard（advisor #1）**：partial entry 不能让 OS clipboard write 把 `uniclip-missing://` URI 推到系统剪贴板（用户 cmd-V 出垃圾比当前 bug 更糟）。

**关键边界（advisor #2）**：rep_refs 阶段 cancel 时，已声明但未 fetch 的 representation 残留 envelope stub bytes —— 必须 drop（不能 capture 半残 rep）。drop 后如果 snapshot 没有 supported rep，`CaptureClipboardUseCase::has_supported_representation` 返回 false → `Ok(None)` 不落 entry。需要 envelope text rep 兜底（envelope 本身的 title/text rep 不依赖 fetch，始终有效）。

##### 后端改动

**5.1 `InboundBlobMaterializer` 返回类型**（uc-application/usecases/clipboard_sync/apply_inbound/materializer.rs）

```rust
pub struct MaterializeResult {
    pub snapshot: SystemClipboardSnapshot,
    pub missing: Vec<MissingFileRef>,  // empty = complete
}

pub struct MissingFileRef {
    pub filename: String,
    pub size_bytes: u64,
    pub reason: FileTransferCancellationReason,  // 复用 uc-core 枚举
}
```

advisor 建议过 struct + missing list 而不是 enum —— 一个 caller 不需要枚举分支，`missing.is_empty()` 就够了。

**5.2 materializer 实现**

- 暴露 `InboundBlobFetcher::is_cancel_error(&AnyhowError) -> bool` 或让 fetcher 返回结构化错误（`FetcherError::Cancelled`）—— cancel detection 是必要的信令，与方案选择无关
- rep_refs 循环：cancel 时 break，**不要** `set_inline_bytes`（保留 rep 的原 envelope bytes 不安全 → 把当前 + 后续未完成的 rep 从 `snapshot.representations` 中删除）
- file_refs 循环：cancel 时 break，把已完成的 path 加进 file:// URI 列表，未完成的 file_refs 加进 missing list 并写成 `uniclip-missing:///{filename}?size={N}&reason={r}` URI
- rewrite file-list rep：file:// + uniclip-missing:// 混合 uri-list

**5.3 apply_inbound 处理 partial**（usecase.rs:199-245）

```rust
let MaterializeResult { snapshot, missing } = materializer.materialize(...).await
    .map_err(|e| { /* 真错误 path，emit failed，return Err */ })?;

let is_partial = !missing.is_empty();
let snapshot_for_write = snapshot.clone();  // 仍 clone，但下方按 is_partial 分支
let entry_id = self.capture.capture(...).await?;

// 关键修复（advisor #1）：partial 跳过 OS write
if !is_partial {
    tokio::spawn(async move {
        write_port.write(snapshot_for_write).await ...
    });
} else {
    info!(entry_id=%entry_id, missing_count=missing.len(),
          "inbound: partial entry persisted, skipping OS clipboard write");
}

emit_host_event(ClipboardHostEvent::NewContent { origin: Remote, .. });  // 前端 removePendingEntry
```

**5.4 file_transfer projection**：P1-10 已经把 Cancelled event 落 `status='cancelled'`，无需改。`untag` (`uc-infra/network/iroh/blobs.rs:449`) 只删 iroh tag store，**不动 sqlite file_transfer 行** → file_transfer 行作为"这条 entry 是 partial"的额外信号是稳健的（Option C 仍可用作辅助查询）。

##### 前端改动

**5.5 `useTransferProgress.ts:104-110`** —— cancelled 分支兜底清 placeholder：

```ts
} else if (status === 'cancelled') {
  dispatch(removePendingEntry(entryId))  // 兜底，与 clipboard.new_content 幂等
  dispatch(markTransferCancelled({...}))
}
```

**5.6 URI 渲染解析**

- 候选 A（推荐）：renderer 解析 file-list rep bytes，识别 `uniclip-missing://` scheme → 渲染 missing 态
- 候选 C 兼容（可叠加）：渲染时附带查 file_transfer.status='cancelled' 做 missing 信号兜底

`uniclip-missing://` 优势：自包含、跨设备稳定、不依赖 join。

**5.7 UI 组件**（ClipboardItemRow / FilePreview / FileContextMenu）

- missing file：灰色 icon + "文件未传输完成 / 已丢失" 文案
- 点击打开 / 拖出 / 复制：disabled，加 tooltip

**5.8 i18n**：`clipboard.fileMissing.cancelled.localUser` 等（reason 维度 5 个 key）

##### Schema 变更

**无**。`clipboard_entry` 不加列，`file_transfer` 表已有 cancelled 状态。partial 信号靠 representation bytes 内的 `uniclip-missing://` URI 表达。

##### 边界情况清单

| 情况 | 处理 |
|---|---|
| cancel 在第一个 fetch 都没开始 | snapshot 只剩 envelope rep（text/title 等）；若已有 supported rep → 照常走 partial capture；若全无 → materializer 在 partial 退出前 mint 一个 `text/plain` rep 兜底，内容形如 `"[Cancelled transfer from {device}]\n{filename_1}\n..."`，**不需要 fetch**，从 advertised_filenames + from_device 直接构造（用户决策 2026-05-23 session 7） |
| cancel 在 rep_refs 阶段（PNG 缩略图） | 该 rep 从 snapshot.representations 删除；如果 envelope 还有其他 supported rep，照常 capture；如果全删光，同上 |
| cancel 在 file_refs 阶段 | 最常见路径：已完成的 file 用 file://，未完成的用 uniclip-missing://；总有 rep 可 capture |
| dedup（`find_recent_duplicate`） | content_hash 是 envelope hash（不依赖 fetch），visible_key 也来自 input → partial entry 不会被误判为 dup |
| Timeout 路径（P1-8 实施后） | materializer cancel detection 不分 reason；is_cancel_error 对所有 `BlobTransferError::Cancelled` 命中即可 |

##### 实施顺序

1. **InboundBlobFetcher cancel 识别**：fetcher trait / 实现里加结构化错误或 `is_cancel_error`
2. **MaterializeResult 类型扩展**：trait 签名改，所有实现（含 fake）跟进
3. **materializer 实现**：rep_refs / file_refs cancel handling + URI 重写
4. **apply_inbound 分支**：is_partial 跳过 OS write，统一 emit NewContent
5. **前端 useTransferProgress**：cancelled removePendingEntry 兜底
6. **前端 URI 解析 + UI**：missing file 渲染 + 操作 disable
7. **i18n 文案**
8. **测试**：materializer 单测（cancel mid-rep / mid-file）、apply_inbound 单测（partial 落 entry 且 OS write 不被调）、前端 reducer 单测
9. **E2E 真机**：800MB 取消后 (a) entry 还在列表 (b) 不在第一行 (c) 重启后仍在 (d) cmd-V 系统剪贴板不带 missing 占位

### 阶段 4：Webview 内存（问题 3）

未开始。建议先复现取证：
1. 起 dev profile，传 ≥1GB 文件
2. 看 Activity Monitor + Chromium Task Manager 的 webview 进程内存曲线
3. grep 前端代码所有读取传输文件内容的位置（`convertFileSrc` / `readBinaryFile` / 缩略图 / 预览组件）
4. 根据复现结果分支处理（缩略图限制 / 预览懒加载 / dev 模式独有不修）

## 决策记录

- 阶段顺序按"问题 2 → 1 → 4 → 3"，因为前 3 个共享 cancel 链路、问题 3 独立可后置
- 阶段 1 拆成 7 个 commit（P1-0/1-3/1-4/1-5 已完成；P1-6/1-7/1-8 待做）
- vendor 改动直接打补丁，后续给 iroh-blobs upstream 提 PR（见 `src-tauri/vendor/iroh-blobs/UNICLIPBOARD_PATCH.md` Patch 3）
- **没有** 新建 iroh 双向控制通道：advisor 复核后确认是过度设计；用 `ConnectionPool::close` 拆 receiver 端 connection 后，sender 端 iroh provider 自然 EOF 退出
- 三件套规划文件用完后归档（移动到 `.planning/archive/2026-05-22-direct-transfer-fixes/` 或并入 PR 描述后删除）

## 下一个 session 接手指引

1. **先确认 git 状态**：`git branch --show-current` 应该是 `adaptable-finch`；如果不是，跑：
   ```bash
   git -C ../dirt-ceder switch dirt-ceder
   git switch adaptable-finch
   ```
   或重新做我做过的 swap dance（见 `progress.md` 末尾"branch 错乱事件"）
2. **检查最近 4 个 commit**：`git log --oneline d1415efe..HEAD` 应该看到 P1-0 / P1-3 / P1-4 / P1-5 四个 commit
3. **从 P1-8 开始**（timeout 走 cancel 通道）—— 这个比 P1-6 更影响日常体验（用户被 timeout 打断的频率高于主动点取消）
4. **或者从 P1-6 开始** —— 想先看到 UI 效果就先做这个，P1-8 跟着做
5. **P1-7 验收两个都做完后再跑**

## 重要约束（容易踩坑）

- `uc-core` 不允许依赖 `tokio`：所有 cancel token 类型在 uc-core 之上的层使用，port 上不暴露 `CancellationToken`
- `uc-application` 对外只暴露 `src/facade/*` 下的 facade：新增 Tauri command 必须经过 facade，**不允许** 直接 `use uc_application::usecases::*`
- 三件套 markdown 文件不入库（`task_plan.md` / `findings.md` / `progress.md` 没在 `.gitignore` 但应只在本地）
- Lint-staged 会在每次 commit 时跑 Rust 格式化（不改逻辑），看到 "modified by linter" 是正常的
- specta TS bindings 是 CI gate：新增 Tauri command 必须跑 `cargo test -p uc-tauri --test specta_export` 重生 `src/lib/ipc-bindings.generated.ts`
