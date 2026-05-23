---
created: 2026-04-17T15:34:15.629Z
title: 给 FileTransferEvent::Cancelled 接入真实发射方
area: file-transfer
files:
  - src-tauri/crates/uc-daemon/src/workers/file_sync_orchestrator.rs:184-210
  - src-tauri/crates/uc-platform/src/adapters/file_transfer/service.rs
  - src-tauri/crates/uc-bootstrap/src/file_transfer_lifecycle.rs
  - src-tauri/crates/uc-application/src/file_transfer/usecases/cancel_transfer.rs
---

## Problem

`FileTransferEvent::Cancelled` 在领域模型里已经建好、`CancelTransferUseCase` 也已经存在并接入 `FileTransferLifecycle`（`file_transfer_lifecycle.rs` 的 `cancel` 字段），
但 **没有任何代码路径真正产出 `Cancelled` 事件**：

- `uc-platform` 的 `FileTransferService` 从未构造 `FileTransferEvent::Cancelled`
- 也没有 daemon / Tauri API 让 UI 主动触发取消
- 当前 `FileSyncOrchestratorWorker::handle_event` 对 `Cancelled` 事件的处理方式是 **兜底走 Failed**（见 `file_sync_orchestrator.rs` 行 184-210 的注释：
  "No adapter currently emits Cancelled ... Route through the fail path"），把 `reason` 写进 `detail` 字符串里当文本保留

影响：
- 用户发起"取消传输"这一业务动作，目前没有办法流到领域时间线上
- Projection 把取消折叠成 failed，UI 无法在展示上区分"失败"与"取消"（只能靠 `failure_reason` 文本前缀区分）
- `CancelTransferUseCase` 是死代码

## Solution

TBD — 需要成体系地接通 UI → daemon → use case → 平台层关停流程，大致方向：

1. 给 daemon API 增加 cancel 入口（Tauri command / WS 消息）
2. 入口内部调用 `lifecycle.cancel.execute(CancelTransfer { ... })`
3. `FileTransferService` 的 send/receive 任务需要支持外部中断：携带 `CancellationToken`，被取消时清理 stream + tmp 文件，然后由发起侧发 `FileTransferEvent::Cancelled`
4. 去掉 `file_sync_orchestrator.rs::handle_event` 对 `Cancelled` 走 `fail_transfer` 兜底的逻辑，改为真正经由 `CancelTransferUseCase`
5. 同步评估 projection：目前 `Cancelled` 折叠为 `failed`（见 checklist Phase 2 决议），如果前端要区分两者，可以引入 `failure_reason` 前缀约定或新增状态枚举

参考 `FILE_TRANSFER_USECASE_INTEGRATION_CHECKLIST_ZH.md` 关于 `Cancelled` 的既有决议（Phase 1 第 8 条、Phase 2 第 13 条）。

## Resolution (2026-05-22 ~ 2026-05-23)

`Cancelled` 不再是死代码。完整的取消链路接通了 UI → IPC → application facade → 平台层 fetch task 强制关停，并且通过反向 progress 通道把"取消"事件传回 sender，最终 receiver / sender 双端 UI 都按"cancelled"中性灰色展示而非"failed"红色。

落地拆成 7 个 commit：

| Commit | 范围 |
|---|---|
| `12c9b388` | vendor patch：iroh-blobs `Downloader::shutdown_endpoint`（暴露 `ConnectionPool::close`，让 caller 能从外部撕 in-flight fetch 的 QUIC connection） |
| `a3895a1f` | `BlobTransferPort::shutdown_inflight_fetch` + iroh adapter 实现 + fake |
| `08b247a7` | `BlobTransferFacade::cancel_inbound_transfer` + `fetch_blob_to_path` 用 `select!` 包 CancellationToken |
| `9c573faf` | Tauri command `cancel_file_transfer` + `AppFacade::cancel_inbound_transfer` |
| `827f7f9a` | 前端取消按钮：`TransferProgressBar` X 按钮 + `ClipboardPreview` 接线 + `cancelFileTransfer` thin wrapper |
| `26689c4f` | timeout sweep 走同一 cancel 通道（reason=Timeout）—— 见配套 todo `2026-04-17-move-file-transfer-timeout-sweep-off-repository-port` |
| `ff12a72f` | cancelled 语义全链路恢复：wire status bytes 0x04..0x08 编码 5 个 cancel reason；projection 落 `Cancelled`（不再折叠 Failed）；sender 端通过反向 `OutboundProgressReporterPort` 收到 cancel 终态；UI 灰色徽章 + i18n |

关键设计点：

- **不新建 iroh 控制通道**：复用现有反向 `OutboundProgressReporterPort` 单向流（独立 ALPN，与 fetch QUIC 物理隔离），扩展 status byte 表达 cancel + reason，不动 34 字节定长帧、不跳 ALPN 版本
- **撕 connection 前先 await report(Cancelled)**：避免 receiver 拆完 QUIC 后 sender 永远收不到终态帧的 race
- **`FileSyncOrchestratorWorker::handle_event` 不再 fallback 走 Failed**：projection 直接落 `TrackedFileTransferStatus::Cancelled`，host publisher 发 wire `status="cancelled"`
- **DB 不迁移**：旧行保留 `failed + cancelled:*` reason，前端 `resolveEntryTransferStatus` 加 fallback 识别 `cancelled:` 前缀，老数据按 cancelled 渲染
- **视角翻转**：cancel reason 的 `LocalUser` / `RemotePeer` 沿反向通道发回 sender 时对调，sender UI 收到的就是它自己视角的 reason

配套用例 5 修复（commit `6922e632`）：cancel 时 entry 落库 + `uniclip-missing://` URI scheme 表达未完成 file refs + 前端 `removePendingEntry` 兜底，避免取消后 placeholder 永远置顶 + 重启消失。

调研事实 / 设计推导 / advisor 复核细节归档在 `.planning/archive/2026-05-23-direct-transfer-fixes/`。
