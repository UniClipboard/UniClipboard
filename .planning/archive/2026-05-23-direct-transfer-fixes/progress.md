# 会话进度日志

## 2026-05-22 session 1

### 阶段 0：调研
- 创建规划三件套
- 启动阶段 0：并行 Explore agent 摸代码 + grep dev log
- 阶段 0 完成：
  - 代码定位确认问题 1/2/4 都是接线 + 架构问题，已有两个 pending todo 对应
  - 日志取证：今天 dev 日志无传输活动，复现验证留到各阶段验收
  - 提出阶段顺序：问题 2 → 1 → 4 → 3

### 用户决策
- 推进节奏：先做规划文档，再逐个修
- Phase 1 切片：全量 Phase 1（一次做到位）
- Vendor 策略：直接改 vendor + 后续补上游 PR
- Commit 策略：每个 slice 一个 commit

### Phase 1 重大转向（advisor 复核后）
- sender 端没有 "send loop"，`publish_path` 同步 add 文件
- 真正持续传输的是 receiver fetch task
- 不需要新建 iroh bi 流，不需要改 sender 端代码
- 单一焦点：receiver fetch_to_path 接 CancellationToken；用户取消 + timeout + delete 三个来源共同 trigger
- 任务调整：删除 P1-2（bi 协议）；新增 P1-8（timeout 走同一通道）+ P1-9（验证 iroh-blobs future 模型）

### 关键发现：iroh-blobs Downloader 是 spawn 模型
- `DownloaderActor` 把下载任务挂在内部 `JoinSet`（`vendor/iroh-blobs/src/api/downloader.rs:40-99`）
- caller drop 不会取消任务（`handle_download` 用 `tx.send().await.ok()` 吞错）
- `ConnectionPool::close(id)` 存在但 Downloader 没暴露
- → 需要改 vendor

### 已完成 commits（按时间顺序）

| Commit | 范围 |
|---|---|
| `12c9b388` | vendor: bump iroh-blobs，submodule 同步 commit `56b29c25` |
| `a3895a1f` | uc-core port + uc-infra impl + fake 实现 `shutdown_inflight_fetch` |
| `08b247a7` | uc-application facade：`cancel_inbound_transfer` 方法 + `fetch_blob_to_path` 加 select! |
| `9c573faf` | uc-tauri `cancel_file_transfer` command + `AppFacade::cancel_inbound_transfer` |

### branch 错乱事件（重要！）
- session 中间发现 git 切换到了 `dirt-ceder` 分支（reflog 显示 `HEAD@{0}: checkout`），不是我做的（可能用户在另一个终端、IDE、或 hook 触发）
- 两个 worktree 互换了：
  - `.../adaptable-finch/` worktree 当前在 `dirt-ceder` 分支
  - `.../dirt-ceder/` worktree 当前在 `adaptable-finch` 分支（持有我的 commit）
- 恢复 dance：
  1. `git switch --detach`（本 worktree 释放 dirt-ceder）
  2. `git -C ../dirt-ceder switch dirt-ceder`（另一个 worktree 切到 dirt-ceder，释放 adaptable-finch）
  3. `git switch adaptable-finch`（本 worktree 切回）
- WIP（P1-5 改动）stash 后 pop 恢复

### 已完成（P1-5 后）
- P1-0 ✅ vendor patch
- P1-1 ✅ 代码定位
- P1-3 ✅ port + adapter + fake
- P1-4 ✅ facade cancel_inbound_transfer + select!
- P1-5 ✅ Tauri command
- P1-9 ✅ Downloader 生命周期验证

## 2026-05-22 session 2 — P1-8

### 改动
- uc-core: 新增 `FileTransferCancellationReason::Timeout` 变体
- uc-application:
  - `BlobTransferFacade::cancel_inbound_transfer` 返回值由 `Result<(), _>` 改为 `Result<InboundCancelOutcome, _>`,区分 `Cancelled` / `NotInflight`
  - 新增 `InboundCancelOutcome` 枚举，从 facade 模块对外导出
  - host_event publisher 加 `cancelled:timeout` label
- uc-infra: projection/sqlite.rs 加 `cancelled:timeout` label
- uc-tauri: command 显式丢弃 outcome(IPC 边界不暴露)
- uc-bootstrap: `spawn_timeout_sweep` 新增 `Arc<BlobTransferFacade>` 入参;sweep 循环按状态分支：
  - `Transferring` 行调 `cancel_inbound_transfer(Timeout)` → 撕 fetch task + 撕 QUIC + 落 Cancelled 事件
  - `Pending` 行 (无 peer_id) 走原 `mark_failed` 路径
  - `Cancelled` outcome 收口;`NotInflight` / Err 都 fallback 到 mark_failed
- uc-desktop: `FileSyncOrchestratorWorker::new` 多收一个 `Arc<BlobTransferFacade>`,assembly 串好

### 验证
- `cargo check --workspace` ok
- `cargo test -p uc-application` 542 unit + 10 integration ok
- `cargo test -p uc-core -p uc-infra -p uc-bootstrap` ok
- 仍待 P1-7 用真实 dev 日志复现 timeout 触发 cancel 链路

## 2026-05-22 session 5 — P1-7 真机 E2E 验收

### 跑通的验收
脚本 `scripts/test_file_send_recv_cancel.sh`,双 profile (alice/bob，均 --dev，真 rendezvous),800MB 随机文件，4 秒后 SIGINT。CASE 1 通过：

```
bob recv exit=1 (expected 1 for cancel path)
JSON outcome:
  "outcome": "cancelled"
  "bytes_written": 0
  "entry_id": "ea3d16c2-..."
  "transfer_id": "c26e9db3-..."  (uuid v4, bob 端 mint)
stderr: ✗ Cancelled
target file 不在 inbox (partial 已清掉)
```

alice 端 send 日志：
```
✓ Blob published          (publish_blob_path 进了 800 MiB)
✓ 1 accepted, ...         (V3 envelope dispatch 成功,带 blob_ref)
entry_id: ea3d16c2-...    (与 bob 端一致 → 同一条 transfer)
```

→ 全链路验证：用户视角 Ctrl-C → cancel_inbound_transfer → token.cancel + Downloader::shutdown_endpoint (vendor patch) → fetch_blob_to_path 的 select! 返回 Cancelled → cleanup partial + 落 Cancelled domain event。

### CASE 2 (成功路径) 未通过
小文件 32KB 第二次 alice send + bob recv,bob 30s 内未收到任何 envelope (stderr 只到 "Probed 1 online")。怀疑双 CLI session 共用同一 rendezvous 地址，session1 cleanup 后 session2 的 peer connection 还没重新发现到 LAN。与 cancel 路径 **无关**,留作单独排查 (可用 daemon 模式跑 CASE 2 而非 CLI 单进程)。

### 未完成
- Phase 3 ⏸ 删除流程修复
- Phase 4 ⏸ Webview 内存

## 2026-05-22 session 4 — CLI send -f / recv (E2E)

### 改动
- AppFacade 新增三个直通方法：
  - `publish_blob_path` (流式 publish)
  - `fetch_blob_to_path` (流式 fetch，会注册到 inflight registry 当带 transfer_context)
  - `dispatch_clipboard_snapshot_with_blob_refs` (V3 envelope + 尾部 blob refs)
- facade module 暴露 `FetchTransferContext`、`decode_v3_bytes_to_snapshot_and_blob_refs`
- `uniclip send -f <path>`: publish_blob_path → 构造 file-uri-list rep + 单个 free-file V3BlobRef → dispatch_with_blob_refs → 等 Ctrl-C 保持 iroh router 活着
- `uniclip recv [--out <dir>]`: 新命令，默认 cwd,subscribe inbound notices，跳过无 file blob 的 envelope，挑第一个 free-file blob,fetch_blob_to_path with FetchTransferContext + Ctrl-C → cancel_inbound_transfer(LocalUser),失败/取消时删 partial 文件
- recv **不写** 系统剪贴板，与 `start` 形成对照

### 关键设计选择
- send -f 不能与 positional text 同时给 (clap 不能用 conflicts_with 因为 file 是 long opt 但 text 是 positional，手动判断)
- recv 用 Uuid::new_v4 当 transfer_id (不绑定本地 entry —— CLI 不参与剪贴板写入)
- filename 来自 remote，做了 sanitize (剥 / \ \\0)
- ctrl_c handler 通过 oneshot 实现 success path 让 spawned task 退出

### 验证
- `cargo check --workspace` ok
- `cargo test -p uc-cli` 32 tests ok
- `uniclip send --help` / `uniclip recv --help` 输出正确
- 仍待真机双 profile E2E:`uniclip send -f big.bin` 一端 + `uniclip recv --out /tmp/inbox` 另一端 + Ctrl-C 触发 cancel，观察 receiver 端 fetch task 是否真正退出

## 2026-05-22 session 3 — P1-6

### 改动
- regenerate `src/lib/ipc-bindings.generated.ts`(`cargo test -p uc-tauri --test specta_export`)
- `src/api/tauri-command/file_transfer.ts`:新增 `cancelFileTransfer(transferId)` 薄封装，reason 硬编码 `localUser`
- `src/api/tauri-command/index.ts`:re-export
- `TransferProgressBar.tsx`:新增 `onCancel` + `cancelling` 可选 prop;`compact` 和 `inline` 两个 variant 都加 X 按钮，只对 `direction === Receiving && status === active` 渲染
- `ClipboardPreview.tsx`:`useState(cancelling)` + `useCallback(handleCancelTransfer)`;调用 wrapper，失败 reportError + 复位 cancelling 让用户可重试;不做乐观更新，等 host event
- i18n:`clipboard.transfer.cancel` 中英文

### 验证
- `tsc --noEmit` 通过
- `bun run test --run`:80 files / 514 tests 全绿
- 仍待 P1-7 真机验证 (从前端按钮一直到 sender 进程退出 fetch)

### 未完成
- P1-7 ⏸ 双端 cancel 流转验收
- Phase 3 ⏸ 删除流程修复
- Phase 4 ⏸ Webview 内存

### 下一个 session 建议起手
1. 验证 git 状态在 `adaptable-finch` 分支
2. 跑 `cargo build --workspace` 确认 P1-5 之前的所有改动还能编译
3. 选择从 P1-8（后端 timeout）或 P1-6（前端按钮）开始
4. 在做 P1-7 验收前必须把 P1-6 和 P1-8 都做完，否则只能验证局部

## 2026-05-23 session 6 — Phase 3 开局（复现优先）

### 决策
- 用户选了"先复现定位卷点"路径（advisor 建议方向）
- **不在未复现前盲改代码**。已知方向（delete 前 cancel in-flight transfer）逻辑正确，但要先验证 HTTP 是真的卡在 `delete_uc.execute` 哪一步，还是卡在更外层（axum flush / 客户端 timeout）

### 已知代码事实（调研产出）
- 删除走 daemon HTTP：`DELETE /clipboard/entries/:id` → `uc-webserver/src/api/clipboard.rs:152 delete_entry` → `clipboard_history::facade::delete_entry` → `DeleteClipboardEntryUseCase::execute`
- use case 串行步骤（`src-tauri/crates/uc-application/src/usecases/clipboard_history/delete_entry.rs:65-211`）：
  1. `fetch_entry` (`info_span!`)
  2. `release_blob_tag` → `blob_transfer.untag(ClipboardEntry(entry_id))`（**目前没 cancel in-flight fetch**）
  3. `cleanup_cache_files` → `tokio::fs::remove_file` 走 file://uri-list（如果 receiver fd 还开着，macOS 不阻塞但删完留 dangling，Win 会 EBUSY）
  4. `cleanup_search_index`
  5. `delete_selection`
  6. `delete_entry` (DB) ← 用户报告"DB 已删"应该指这步已过
  7. `delete_event` (`event_writer.delete_event_and_representations`) ← 嫌疑卡点（如果 in-flight fetch 在持续 UPDATE representations / file_transfer 行，DELETE 可能竞争）
- 前端：`DeleteConfirmDialog.tsx:31-47` `isDeleting` 在 `await onConfirm()` 的 `finally` 才复位；`clipboardSlice.ts:112-122` thunk 直接 await HTTP。HTTP 不返回 → 前端永远卡 isDeleting=true。**前端逻辑健全，根因在后端**
- cancel 能力已就绪：`BlobTransferFacade::cancel_inbound_transfer(transfer_id, reason)`（Phase 1 P1-4），入参是 `transfer_id` 不是 `entry_id` —— 要先查 `FileTransferRepositoryPort::list_transfers_for_entry(entry_id)` 拿到 in-flight transfer_id

### 复现指引（用户操作）

**目标**：复现"传输中删除卡住"，从日志判断到底卡哪一步。

**配置 RUST_LOG**（接收端窗口；macOS）：
```bash
RUST_LOG='info,uc_application=trace,uc_application::usecases::clipboard_history::delete_entry=trace,uc_infra::network::iroh::blobs=debug' \
  pnpm tauri dev
```
关键 span 名（每步都有 `info_span!`，trace 级会看到 enter/exit）：
- `usecase.delete_clipboard_entry.execute`
- `fetch_entry` / `release_blob_tag` / `cleanup_cache_files` / `cleanup_search_index` / `delete_selection` / `delete_entry` / `delete_event`

**复现步骤**：
1. 双端 dev 配对（参考现有 `scripts/test_clipboard_e2e.sh` 走法 / 或用 `dual-side-sync` skill 把 macOS 改动推到 win 跑）
2. alice 端 `uniclip send -f big.bin`（≥500MB，参考 `scripts/test_file_send_recv_cancel.sh:BIG_SIZE_MB`）
3. bob 端用 GUI（pnpm tauri dev）发现新条目，在 transfer 进行到 ~30% 时从 UI 点删除
4. 观察 bob 端：
   - HTTP DELETE 在 daemon 日志里有没有看到响应日志？
   - 上面 7 个 span 哪个 enter 后没 exit？
   - iroh-blobs untag debug 日志：是直接返回还是等什么？
   - 前端 isDeleting 按钮是不是永远卡住？

**判断**：
- 若卡在 `release_blob_tag` → untag 等 in-flight tag holder；修法：先 cancel
- 若卡在 `cleanup_cache_files` → iroh 写 partial 持有 fd，Win 上 remove_file 失败重试；修法：先 cancel 让 fd 释放
- 若卡在 `delete_event` → sqlite 行级锁竞争；修法：先 cancel 让 UPDATE 停
- 若 use case 完整退出但 HTTP 仍未返回 → 不是 use case 问题，看 axum / hyper / 客户端 timeout

### Phase 3 待办（依赖复现产出）
- ⏸ 用户跑上面的复现并贴日志 / 描述卡点
- ⏸ 基于卡点确定方向：
  - 大概率方向：在 uc-core 加 `trait InflightTransferCancellation { cancel_inflight_for_entry(entry_id) }`，BlobTransferFacade 实现，注入到 DeleteClipboardEntryUseCase；execute 第一步先 cancel
  - 复用 `FileTransferCancellationReason::LocalUser`（advisor 建议）—— 不新增 `EntryDeleted` 变体除非前端真需要区分
- ⏸ 写完代码后回归：再跑一次复现脚本，看是否解锁

### 未完成
- Phase 3 ⏸ 待复现
- Phase 4 ⏸ Webview 内存

## 2026-05-23 session 3 — cancelled 语义全链路

### 用户问题
> 用户取消，算传输失败吗？... 双方都不应该认为用户主动取消是失败，发送方也显示传输失败

### 调研发现（写入 findings.md）
- cancelled 语义被三层压扁：uc-application publisher、uc-infra projection、前端 EntryTransferStatus 都把 Cancelled 当 Failed
- sender 端通过 `OutboundProgressReporterPort` 反向通道（独立 ALPN，不与 fetch QUIC 共用）接收状态；当前枚举只有 InProgress/Completed/Failed
- sender 端 wire→UI 转译在 `uc-bootstrap/src/space_setup.rs:136-181`，是轻量 translator，不经过 FileTransferEvent

### Advisor 复核要点
- 方向正确：复用反向 progress 通道，不新建信令
- ALPN 独立，撕 fetch connection 不影响 cancel 帧发送
- race：reporter.report(Cancelled).await 必须先于 shutdown_inflight_fetch
- 老 sender 单帧 UnknownStatus 不致命，accept loop 会接受下一帧
- DeliveryFailureReason 不动（小内容路径无 cancel 语义）
- DB 不迁移，前端 fallback 兼容

### 用户决策
- "一个 PR 一次到位"（拒绝拆 P1/P2）
- 14 处改动跨 4 crate + 前端

### 阶段 1 任务清单更新
- task_plan.md 加入 P1-10（完整 14 步实施方案）
- 总进度：P1-10 ⏸ 待实施


## 2026-05-23 session 4 — P1-10 实施完成

### 改动文件（15 处）
后端（7）：
- uc-core/file_transfer/outbound_progress.rs — OutboundProgressStatus 加 Cancelled { reason }
- uc-core/ports/file_transfer_repository.rs — TrackedFileTransferStatus 加 Cancelled，compute_aggregate_status 优先级 failed > transferring > pending > cancelled > completed
- uc-infra/network/iroh/transfer_progress_wire.rs — status bytes 0x04..0x08 编码 5 个 cancel reason，doc-comment 更新
- uc-infra/file_transfer/projection/sqlite.rs — Cancelled event 落 status=Cancelled，reason 去 `cancelled:` 前缀
- uc-application/facade/host_event/publisher.rs — Cancelled event 发 wire status="cancelled"，reason 去前缀
- uc-application/facade/blob_transfer/facade.rs — cancel_inbound_transfer 先 reporter.report(Cancelled) await 再撕 connection，cancel arm 不再发 outbound Failed；加 flip_cancel_reason_perspective 视角翻转；InflightFetch 加 outbound 字段；build_outbound_context helper
- uc-bootstrap/space_setup.rs — translator 加 Cancelled 分支 + cancellation_reason_label helper

前端（6）：
- src/store/slices/fileTransferSlice.ts — EntryTransferStatus.status 加 'cancelled'；TransferProgressInfo 加 status 'cancelled' + cancelReason；markTransferCancelled reducer；resolveEntryTransferStatus 加 cancelled 分支 + 旧 reason 前缀 fallback；normalizeCancelReason helper
- src/hooks/useTransferProgress.ts — validStatuses 加 cancelled，status === cancelled 走 markTransferCancelled
- src/components/clipboard/preview-renderers/FilePreview.tsx — 灰色 XCircle 徽章 + 中性 reason 横幅
- src/components/clipboard/ClipboardItemRow.tsx — isTransferCancelled 分支 + XCircle tooltip
- src/components/clipboard/FileContextMenu.tsx — 排除 cancelled 的"打开文件位置"，copyDisabled 加 cancelled 文案
- src/i18n/locales/{zh-CN,en-US}.json — clipboard.transfer.cancelled + cancelReason.* + statusBadge.cancelled + copyDisabled.cancelled

测试（2）：
- uc-infra wire — 加 frame_round_trip_cancelled_all_reasons + cancel_status_bytes_pin
- src/store/slices/__tests__/fileTransferSlice.test.ts — 加 resolveEntryTransferStatus cancelled / 旧前缀 fallback / markTransferCancelled / normalizeCancelReason 共 9 个断言

### 验证
- cargo check --workspace 全通过
- cargo test -p uc-core / uc-infra / uc-application / uc-bootstrap 全通过（542 + 109 + 9 + ... = N 个 unit + integration）
- specta_export 通过（TS bindings 未变化，因 OutboundProgressStatus 不暴露）
- pnpm tsc --noEmit 0 错误
- pnpm test 523 通过（之前 514 + 9 新）
- daemon-local 13 doc-test 失败是 pre-existing 问题，与本次改动无关（stash 验证）

### 阶段 1 进度
- P1-10 ✅ 已完成（待 commit）
- 剩余：P1-8（timeout sweep 走 cancel 通道）、P1-7（复现验收）

## 2026-05-23 session 5 — P1-6 重复实施 & 回滚

### 起点
task_plan.md 把 P1-6 标记为 ⏸ 待做，于是按规划走了一遍实施：
- src/store/slices/fileTransferSlice.ts：新增 `cancelInboundFileTransfer` thunk + `selectTransferIdByEntryId` selector
- src/components/clipboard/preview-renderers/FilePreview.tsx：在 status badge 旁加 cancel 按钮 + local `cancelRequested` state
- src/store/slices/__tests__/fileTransferSlice.test.ts：新增 4 个断言

### Commit 前发现重复
准备 commit 时 `git log --oneline` 显示 `827f7f9a feat(file-transfer): receiver-side cancel button + thunk`（2026-05-22 已合并到 origin/adaptable-finch）。该 commit 走 `TransferProgressBar` 的 X 按钮 + `ClipboardPreview` 接线 + `src/api/tauri-command/file_transfer.ts` thin wrapper，已实现 P1-6。

### 用户决策 & 回滚
用户选"Revert 这次的 P1-6 工作"，保留 827f7f9a 的实现。回滚的范围：
- fileTransferSlice.ts：移除 thunk + selector + 相关 import
- FilePreview.tsx：移除 cancel 按钮 + local state + 相关 import
- fileTransferSlice.test.ts：移除新增的 4 个断言

回滚后 `pnpm tsc --noEmit` 0 错误、`pnpm test` 523 通过（P1-10 完成态）。

### 教训（写入下次 session checklist）
跨 session 必须先核对 `git log --oneline` 与 task_plan 的 ⏸/✅ 状态是否一致再动手——task_plan 是临时文件，可能落后于实际仓库状态。

## 2026-05-23 session 7 — Phase 5 立项（取消后 entry 落库）

### 用户报告（问题 5）
取消传输后，placeholder 永远置顶在列表第一行，重启应用 entry 消失。期望：取消后 entry 写进数据库，文件标"丢失",且不再置顶。

### 调研产出（详见 findings.md "2026-05-23 取消后 placeholder 永远置顶"）

代码事实：
- `apply_inbound/usecase.rs:211-224` materialize 失败时 emit StatusChanged + 返回 Err,**never calls capture.capture** → DB 没行
- `useTransferProgress.ts:104-110` cancelled status 只更新 fileTransferSlice,**不动 pendingItems**
- `clipboardSlice.ts:223` `unshift` → placeholder 永远在列表头
- `pendingItems` 是 redux in-memory，无 persist 配置 → 重启即丢

### 用户决策
- Entry 形态：落库 + 已成功 representations 保留 + 未成功 file refs 标"missing"
- 置顶问题:cancel 时 placeholder 立即转为真 entry 行 (按 createdAt 排序)

### 倾向方案（待 advisor 复核）
- 后端:materializer 返回 `MaterializeOutcome::Complete | PartialOnCancel`,partial 分支继续走 capture
- Missing 表达：`uniclip-missing://` URI scheme 写入 file-list rep，前端解析 (候选 A，无 schema 变更)
- 前端：`useTransferProgress.ts` cancelled 分支主动 `removePendingEntry`,等 `clipboard.new_content` 渲染真 entry
- UI:`uniclip-missing://` URI 渲染灰色"文件已丢失",复制/打开 disabled

### Advisor 复核结果
Advisor 提了 4 个关键点，事实核实后落进 task_plan.md "P5 设计要点 (advisor 复核后修订版)":

| # | Advisor 提问 | 事实核实 | 落地方案 |
|---|---|---|---|
| 1 | partial entry 推到 OS clipboard | `usecase.rs:307-323` spawn write port.write(snapshot.clone()) 确实会写 | apply_inbound 加 `is_partial` 分支，partial 跳过 spawn write |
| 2 | Rep-bound blob mid-fetch 残留 stub bytes | `materializer.rs:188` set_inline_bytes 只在 fetch 成功后调用，未完成 rep 残留 envelope 声明 | cancel 时把未完成的 rep 从 snapshot.representations 删除;若全删光，依赖 envelope 自带 text/title rep 兜底 |
| 3 | Option C 验证 | `blobs.rs:449-459 untag` 只 delete iroh tag store，不动 sqlite file_transfer 行 | C 数据基础可靠;不过 A(URI scheme) 仍是主方案，C 作为辅助 |
| 4 | MaterializeOutcome enum 过度 | apply_inbound 只判断 partial vs complete | 改用 `struct MaterializeResult { snapshot, missing }`,empty=complete |

### 用户决策 (极端边界)
**强制落最小 entry**。如果 envelope 全无 supported rep,materializer 在 partial 退出前 mint 一个 text/plain rep 兜底：`"[Cancelled transfer from {device}]\n{filename_1}\n..."`。这个 rep 不需要 fetch，从 advertised_filenames + from_device 直接构造，保证"取消后 entry 总是保留"语义一致。

### 下一步
- 用户确认极端边界处理后开 P5 实施 (顺序见 task_plan.md "实施顺序" 9 步)

## 2026-05-23 session 8 — P5 实施完成

### 改动文件
后端 (4):
- `apply_inbound/materializer.rs` — MaterializeResult struct + MissingFileRef + is_cancel_error helper;BlobTransferFacade impl 用 `anyhow::Error::from` 保留 thiserror chain;rep_refs/file_refs cancel 分支 catch 后走 finalize_partial;新 helpers:`finalize_partial`、`format_missing_uri`、`encode_path_segment`、`supported_for_capture`(fallback rep 兜底)
- `apply_inbound/usecase.rs` — `let (snapshot, is_partial) = match ...`;partial 分支跳过 OS clipboard write spawn(advisor #1 hazard);partial 也跳过 `remember_recent_inbound`(advisor A retry 修复)
- `apply_inbound/tests.rs` — Mock trait 签名跟进 `Result<MaterializeResult>`;`Ok(MaterializeResult::complete(snapshot))` 替代历史 `Ok(snapshot)`
- 新增测试 (4): partial mid-batch / partial first-file / OS-write skip invariant(advisor B) / dedup-skip-on-partial invariant(advisor A)

前端 (4):
- `useTransferProgress.ts` — cancelled 分支兜底 `dispatch(removePendingEntry(entryId))`
- `clipboard-utils.ts` — UNICLIP_MISSING_SCHEME 常量 + `isUniclipMissingUri` + `parseFileItemsFromUriList` + `summarizeFileMissing`
- `clipboard-transform.ts` — 用 parseFileItemsFromUriList 填充 ClipboardFileItem.file_missing
- `clipboardItems.ts` — `ClipboardFileItem.file_missing?: boolean[]`
- `FileContextMenu.tsx` — 加 hasMissingFiles prop;copy/open file location 在 hasMissingFiles=true 时 disable;**删除按钮始终保留**(用户约束)
- `ClipboardContent.tsx` — FileContextMenu 调用点透传 hasMissingFiles

### Advisor 复核 → 全部回应
- A(blocking): `remember_recent_inbound` 移入 `if !is_partial` 块 + 加 dedup-skip-on-partial 测试 ✅
- B(blocking): 加 partial_materialize_persists_entry_but_skips_os_write 测试 (MockWrite.times(0)) ✅
- C(one-line): FileContextMenu 加 hasMissingFiles 兜底 ✅

### 验证
- cargo check --workspace --tests: 全通过
- cargo test -p uc-application --lib: 546 passed(+4 new)
- pnpm tsc --noEmit: 0 错误
- pnpm test: 523 passed(无变化，前端 helpers 是 dead-code 备用，未单测 —— 后续按需补)
- E2E 真机验收并入 P1-7

## 2026-05-23 session 9 — Phase 4 根因 + 修法

### 取证路径（从 dead-end 拉回来的)
1. T0 baseline → T1 复制 1GB zip → webview 90s 内涨到 5GB+（rust daemon 仅 ~100MB）
2. 关 Web Inspector 无变化 → 排除 Inspector 缓存
3. Console queryObjects(Uint8Array/ArrayBuffer/Blob/String>1MB) 全空 → 排除 JS 堆
4. querySelectorAll('img/video/audio/canvas') 全 0 + DOM 总 358 → 排除 DOM
5. IndexedDB/localStorage/sessionStorage 全空 → 排除 storage
6. macOS pbcopy 覆盖剪贴板内存仍涨 + 用户报告 Windows 接收方也复现 → 排除 macOS pasteboard 共享内存假象
7. daemon 日志 5 min 5480 个 `file-transfer.progress` 事件 = **18 emits/sec** → 锁定高频 WS 洪水
8. `vmmap -summary` 揭示 `WebKit Malloc` 堆 1.0GB resident / 290 万个分配 → 确认 WebKit native C++ 对象累积
9. daemon 端 capture 最大 1MB（zip 内容 NEVER 进 representation）→ 排除内容直推假设

### 根因
`uc-infra/src/network/iroh/blobs.rs:28-37` 节流条件 `due_by_bytes || due_by_time`。高带宽 wifi 下 5ms 跨过 256KB → 字节窗几乎一直 true → 时间窗失效 → 实测 18 emits/sec。

### 修法
- 删 `PROGRESS_REPORT_BYTES` 常量
- emit 条件改 time-only：`if due_by_time && total > last_reported_bytes`
- 200ms 时间窗成为硬上限（5 emits/sec），慢速传输仍每 200ms 至少更新一次
- 注释里写清楚历史 bug + Phase 4 取证背景

### 验证
- `cargo test -p uc-infra --lib network::iroh::blobs` 17/17 通过
- `cargo check --workspace` 全通过
- 待用户重启 dev profile 真机复测：1GB zip 复制后 webview 应稳在 ~700MB

### Phase 4 状态
- 调研 ✅
- 修复 ✅（commit `77ac2832`）
- 真机验证 ✅ —— 2GB 文件传输 emit 频率 4-5/sec（之前 100-180/sec），webview 稳在 ~500MB（之前 5GB+），WebKit Malloc 521MB resident（之前 1GB），下降一半

### 第二次修法（同 commit）
P1 修了 `blobs.rs` 后用户重启再测发现 sender 端 emit 还是 100-180/sec —— 因为 mac 是发送方，progress 帧从 windows peer（跑老版本，没 blobs.rs fix）反向推过来。在 `space_setup.rs::spawn_outbound_progress_translator` 加防御性节流（per-transfer 200ms 时间窗 + 终态帧绕过），即便对端版本旧/跑老代码也不影响本机前端。最终 emit 严格 ≤5/sec。

### 后续优化空间（用户决定暂不做）
490MB baseline 已合理，但仍有 ROI 排序的优化方向：
1. production build 对比（预计省 100-200MB，dev 模式 __TEXT 占 361MB）
2. entry 列表虚拟化（react-window，entry 数线性优化）
3. routes lazy load（React.lazy）
4. 后台 polling 整合（encryption/state 每 ~1.5s/instance，多组件订阅会叠加）
5. 缩略图 IntersectionObserver 懒加载

## 2026-05-23 session 6 — Commit + push P1-10

### Commit
`ff12a72f feat(file-transfer): treat cancelled distinct from failed across full stack`
- 15 文件 / +556 / -101
- 已 push 到 `origin/adaptable-finch`（bcd41143..ff12a72f）

### task_plan 更正
P1-6 状态由 ⏸ 改为 ✅（实施 commit = 827f7f9a，2026-05-22），P1-10 状态由 ⏸ 改为 ✅（实施 commit = ff12a72f，2026-05-23）。

### 阶段 1 剩余
- P1-8（timeout sweep 走 cancel 通道）
- P1-7（双端真机复现验收）
