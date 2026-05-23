---
created: 2026-04-17T15:34:15.629Z
title: 让 timeout sweep 走事件存储而非直接操作 projection
area: file-transfer
files:
  - src-tauri/crates/uc-bootstrap/src/file_transfer_lifecycle.rs:96-231
  - src-tauri/crates/uc-core/src/ports/file_transfer_repository.rs
  - src-tauri/crates/uc-application/src/file_transfer/usecases/fail_transfer.rs
---

## Problem

Phase 3 已经把 lifecycle 写路径收敛到了 `FileTransferEventStorePort`，但 `FileTransferLifecycle` 里的两条后台任务 **仍然绕过事件存储，直接操作 projection 表**：

- `spawn_timeout_sweep`：循环调用 `file_transfer_repo.list_expired_inflight(...)` + `mark_failed(...)`，然后手动发 `TransferHostEvent::StatusChanged`
- `reconcile_on_startup`：调用 `file_transfer_repo.bulk_fail_inflight(...)`，同样手动发 host event

`file_transfer_lifecycle.rs` 的注释已经记录了保留这条路径的原因：

> `FailTransferUseCase` requires a `peer_id`, which a pending-timeout transfer
> does not yet have (no `Started` event occurred). Re-threading this through
> the event store would require domain-model changes to support a peer-less
> failure scenario, which is deferred to the Phase 5 cleanup.

这是 `FILE_TRANSFER_USECASE_INTEGRATION_CHECKLIST_ZH.md` Phase 4 / Phase 5 的直接对象：
- Phase 4 说 orchestrator 应退化为"运行时协调器"，不该再直接推进状态
- Phase 5 说系统里只应剩一套生命周期推进路径

当前状态违反了两条，导致：
- Projection 表既是 read model，又是 write target（至少对 timeout / reconcile 路径而言）
- 领域事件日志里看不到"这笔传输是被 timeout 打死的"这条事实，排障只能反查 projection 的 `failure_reason` 字段
- 任何未来想复制 projection、或切换 projection 存储的改动，都会被这条侧路径拖住

## Solution

TBD。两条潜在方向：

**方向 A：扩展领域模型以表达 peerless failure**

- 给 `FailTransfer` / `FileTransferEvent::Failed` 允许 `peer_id: Option<String>`，或引入 `FileTransferEvent::TimedOut { transfer_id, reason }` 这种新的领域事件
- timeout sweep / reconcile 改为 append 领域事件，projection 在 apply 时处理 peerless 情况
- 配套：projection apply 逻辑需要知道如何在没有 Started 事件的情况下落 `failed`

**方向 B：让 timeout 路径通过查 projection 补 peer_id，再走 FailTransferUseCase**

- `list_expired_inflight` 返回结果里带 peer_id（如果有的话）
- 没有 peer_id 的 pending 行（还没收到 Started 的）用一个合成值或留空字符串
- 优点：改动小；缺点：把 projection 的 peer_id 当成输入反馈到领域事件，本质上还是在两个来源之间耦合

倾向方向 A，与 Phase 4 + Phase 5 的收尾一起做，同时把 `FileTransferOrchestrator` 的 timeout 调度从 `FileTransferLifecycle` 里单独抽出来，`FileTransferLifecycle` 只暴露 use case 聚合。

参考 checklist Phase 4（"把 `FileTransferOrchestrator` 降级为运行时协调器"）、Phase 5（"删除旧 receiver-side tracking path"）。

## Resolution (2026-05-23)

走了一条更小的"方向 C"：sweep 不再直连 projection write path，而是把 `Transferring` 行的 timeout 重路由到统一的 `BlobTransferFacade::cancel_inbound_transfer(_, Timeout)` 通道——撕 fetch task + 撕 QUIC connection + 落 `FileTransferEvent::Cancelled`，与用户主动取消走同一套领域事件。仅 `Pending` 行（无 peer_id，尚未收到 Started）保留旧 `mark_failed` 路径，因为它没有可撕的 in-flight fetch。

落地代码（commit `26689c4f` `feat(file-transfer): route Transferring timeouts through cancel_inbound_transfer`）：

- `uc-core/file_transfer/cancellation.rs` — `FileTransferCancellationReason` 加 `Timeout` 变体
- `uc-application/facade/blob_transfer/facade.rs` — `cancel_inbound_transfer` 返回值改为 `Result<InboundCancelOutcome>`，区分 `Cancelled` / `NotInflight`
- `uc-bootstrap/file_transfer_lifecycle.rs:spawn_timeout_sweep` —
  - `Transferring` 行调 `cancel_inbound_transfer(transfer_id, Timeout)`，`Cancelled` 即收口；`NotInflight`/`Err` 回退 `mark_failed` 兜底
  - `Pending` 行（仍无 peer_id）走原 `mark_failed` 路径
- `uc-bootstrap/build_file_transfer_assembly` + `uc-desktop/FileSyncOrchestratorWorker::new` — 把 `Arc<BlobTransferFacade>` 串进 lifecycle 装配
- projection / host event publisher 加 `cancelled:timeout` reason label（与其他 4 个 reason 对齐，前缀 `cancelled:` 后续被 P1-10 全链路改造剥离）

副产物：领域时间线现在能区分"timeout cancel"和"真错误 failed"，projection 不再是 timeout 的 write target。`Pending` 行的 peerless failure 仍未走事件存储，作为已知遗留留待未来如需 projection 切换时再处理（届时应走方向 A）。
