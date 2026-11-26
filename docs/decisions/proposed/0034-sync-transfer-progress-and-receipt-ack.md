# 0034 · 同步发送进度上报与对端接收完成回执

- 状态：已采纳，实现中
- 日期：2026-11-26
- 关联：[0004 剪贴板流水线 + 插件](0004-clipboard-pipeline-lua.md)、[0007 多同步后端](0007-multiple-sync-backends.md)、[0020 Tailcat 同步可观测性](0020-tailcat-sync-observability.md)、[0029 可扩展同步后端集成](0029-extensible-sync-backend-integration.md)
- 落地目标：[同步后端业务契约](../../contracts/sync-backend-contract.md)、[插件 RPC 核心协议](../../contracts/plugin-rpc-contract.md)、[Engine 公共契约](../../contracts/engine-contract.md)、[SDK 扩展类型契约](../../contracts/sdk-extension-contract.md)、[本地服务契约](../../contracts/local-service-contract.md)、[流水线与本机历史](../../architecture/pipeline.md)

## 背景与用户故事

用户故事：用户复制一个文件（或较大剪贴板内容）后，希望看到该内容传输到每一台目标设备的实时进度，并在传输结束时明确看到「对端已接收完成」。

对照当前契约与实现，存在四处结构性缺口：

1. **同步后端协议无进度上报通道**：[同步后端业务契约 §4](../../contracts/sync-backend-contract.md#4-单目标发送与最后授权) 的 `peer_sessions` 路径只有 `backend.send` → `sync_host.authorize_send` → `sync_host.open_send_part` → 终态 `SendResult{state: "sent"}`；`external_channels` 的 `backend.report` 也只接受离散枚举 `queued/sending/sent/superseded/failed`。发送期间 Engine 与宿主只能看到目标处于 `sending`，无字节或阶段进度。
2. **Wire v1 无对端接收确认**：[Engine 公共契约 §1.8](../../contracts/engine-contract.md#1-边界与通用约定) 与 `internal/wire` 当前没有任何接收完成 ACK；`sent` 仅表示本地传输层已写完字节（含尚未排空的内核/隧道发送缓冲），不能证明对端完整收到、通过校验或完成提交。
3. **Engine 公开 DTO 缺少进度与历史关联**：`Operation` 未公开关联的 `HistoryID`，`TargetStatus` 仅有 `State` 和 `Error`，没有字节进度与回执阶段字段；`HistoryItem` 只汇总到后端级 `SyncState`。
4. **本地服务 API 无法发现自动复制触发的活跃传输**：[本地服务契约](../../contracts/local-service-contract.md#operation-observation) 仅提供按已知 `OperationID` 查询的 `GET /v1/operations/{id}`，缺少活跃传输列表/按历史条目查询入口；同时 Engine 事件队列（128）与本地 SSE 队列（32）容量有限，不能直接承载未节流的高频进度事件。

## 核心决策

### 1. 采用后端可选节流上报（方案 A），不以本地读取冒充网络进度

遵循 [同步集成契约 §1](../../contracts/sync-integration-contract.md#1-ownership) 的职责划分：**同步后端插件负责提供传输进度与回执证据，Engine 负责校验、聚合、节流与向上暴露**。

放弃「仅在 Engine `OpenPart` 读取端统计字节」的估算方案：对于进程外后端（存在 1 MiB RPC 流窗口与进程内发送缓冲）以及未来的 HTTP/对象存储后端，读取完成远早于真实网络传输完成，容易提前跳到 100% 造成误导。

#### 1.1 `peer_sessions` 新增子调用 `sync_host.send_progress`

- **方向与作用域**：后端 → Engine，嵌套在活跃的 `backend.send` 根调用下（`parent_request_id` 必须指向该 `backend.send`），要求携带本次有效 `grant_id`。
- **载荷字段**（无正文、无文件名、无路径、无设备地址）：
  - `grant_id`：本次 `authorize_send` 签发的授权 token（常量时间校验）。
  - `stage`：有限枚举 `"sending"`（传输中）或 `"awaiting_receipt"`（本地字节已写完，正等待对端确认）。
  - `bytes_sent`：`u64` 十进制字符串，本次发送视图已写出到传输层的累计有效载荷字节数。
  - `total_bytes`：`u64` 十进制字符串，必须等于本次 `Clip.Parts` 声明的总字节数。
  - `part_index`（可选 int）：当前写入的 part 序号（`0..len(Parts)-1`），便于多文件批次展示“第 N/M 项”。
- **校验与约束**：
  - `bytes_sent <= total_bytes` 且在同一次 `grant_id` 内**单调非递减**；`stage="awaiting_receipt"` 要求 `bytes_sent == total_bytes`；阶段只能从 `sending` 向 `awaiting_receipt` 单向推进。
  - 后端按节流频率上报（建议最小间隔 200 ms 或累计新增 ≥ 256 KiB，以及首次进入 `awaiting_receipt` 时立即上报一次）；Engine 同时做速率/并发保护，防止异常插件通过高频进度帧耗尽控制预算。
  - **完全可选**：不调用 `sync_host.send_progress` 的 `peer_sessions` 后端继续合法，UI 自动降级为无百分比的 `sending` 状态。
- **`builtin.tailcat` 实现方式**：直接复用 `internal/wire/body_observation.go` 的 `payloadWriter` 写路径做原子字节累加与节流回调，不改变 wire body framing，也不引入额外拷贝。

#### 1.2 `backend.send` 返回结果扩展回执阶段

`SendResult`（`sync_send_result`）在既有 `state: "sent"` 基础上增加可选字段 `receipt_stage`（默认缺省等价于 `"local_written"`）：

- `"local_written"`：本地已完整写出，且当前会话/对端不支持远端确认（兼容旧版对端或无回执能力的后端）。
- `"remote_received"`：对端已确认完整接收、校验通过并完成接收提交。
- `"remote_discarded"`：对端已完整接收传输流，但按对端策略、去重或配额干净丢弃（未进入对端可用剪贴板/历史）。

仅在后端声明支持远端回执且会话协商通过时，才允许返回 `"remote_received"` / `"remote_discarded"`；不声明该能力的后端返回越权阶段会被宿主拒绝为协议错误。

#### 1.3 `external_channels` 的预留兼容

后续 `external_channels` 接入文件流传输时，可在 `backend.report` 增加可选的 `bytes_sent` / `total_bytes` / `receipt_stage` 字段（例如区分 `"server_accepted"` 与 `"local_written"`），共享同一 Engine `TargetStatus` 模型；本决策首个实现切片聚焦 `peer_sessions`（含 `builtin.tailcat` 与进程外 P2P 后端）。

---

### 2. Wire v1 接收完成回执（`clip_ack`）与双向防死锁设计

#### 2.1 保持 Wire v1 与向后协商

继续遵守「内建 `peer_sessions` 保持 wire version = 1」约定，沿用 `ping`/`pong` 认证控制帧模式扩展，不升级版本号也不破坏与旧版 v2 客户端的互操作：

1. **握手能力通告**：`wire.Hello` 增加可选字符串数组字段 `"caps"`（例如 `["clip_ack"]`）。旧版客户端 JSON 反序列化自动忽略未知字段，且不会回传 `"clip_ack"`；仅当双方 `Hello.caps` 均包含 `"clip_ack"` 时，该会话启用 `clip_ack` 协议。
2. **回执控制帧**：在已认证会话上新增轻量控制帧：
   ```json
   {"t":"clip_ack","id":"<wire_id>","status":"received"}
   ```
   - `id`：对应 `wire.Clip.ID`（1..128 字节）。
   - `status`：有限枚举 `"received"` | `"discarded"` | `"failed"`。**不含**自由文本错误、策略原因、设备路径或任何剪贴板内容，避免泄露对端本地策略或配置。
   - 旧版客户端若意外收到未知控制帧类型，其 `recvLoop` 已有前向兼容跳过逻辑；且因为握手能力门控，对不支持的对端默认不发送 `clip_ack`。

#### 2.2 精确语义边界（绝不把“收到”冒充“已粘贴”）

| `clip_ack.status` | 接收端触发时机 | 含义与边界 |
|---|---|---|
| `received` | 接收端 `rcv.Receive` 返回 `outcome == "committed"` 且流已干净结束 | 对端 Engine 已收齐全部字节、通过 hash 校验、跑完 `received` 处理器并完成提交（`Received` / 可选 `History` / 去重记录）。**不代表**对端已写入系统剪贴板或用户已粘贴。 |
| `discarded` | `OpenReceive` 返回 `discard` 且 `s.w.Discard(m)` 干净排空；或 `rcv.Receive` 返回 `discarded` 且流干净排空 | 对端完整收到了流并按自身策略/去重/限额丢弃，连接保持同步。让发送方立即结束等待并诚实展示，避免误报「对端已收到」或死等超时。 |
| `failed` | 接收端校验/提交失败且连接尚可发帧（若流已失步则直接关闭连接，发送方通过断连感知） | 对端未能成功接纳该传输。 |

#### 2.3 防死锁与超时纪律（关键不变量）

在同一条双向全双工 `wire.Conn` 上，如果两端同时发送大文件，必须严格避免写锁互锁：

1. **发送方写完正文立即释放 `sess.wlock`**：`Send` 只在写 `Clip` 头和 body 期间持有 `sess.wlock`；写完最后一字节后**立刻释放 `sess.wlock`**，先上报 `sync_host.send_progress(stage="awaiting_receipt")`，然后在**不持有 `sess.wlock`** 的前提下等待该 `wire_id` 对应的 `clip_ack`。
2. **接收方异步有界发送 `clip_ack`**：`recvLoop` 在 `receive()` 完成并确定状态后，向会话内有界 ACK 通道入队（容量有界，复用 `pongSem`/专用队列模式），由独立 goroutine 获取 `sess.wlock` 并设置短写超时（如 5 秒）写出 `clip_ack`，**绝不在 `recvLoop` 同步阻塞等待 `sess.wlock`**。
3. **ACK 等待超时与断连处理**：
   - 等待 `clip_ack` 使用有界超时（默认 30 秒，与 body I/O 空闲预算一致；且若对端是大型剪贴板提交/落盘耗时较长，可根据声明大小或配置给出明确上限，具体值在契约冻结前确认）。
   - 等待期间若发生 **连接断开** 或 **ACK 超时**：发送方不能将其标为“对端已接收”，必须诚实反映**远端结果不确定**（`State = failed`，`Error.Code = unavailable` 或 `timeout`，同时保留 `ReceiptStage = local_written` 与 `BytesSent == TotalBytes`），且**禁止自动重发**。
   - 等待期间若收到 **取消 / Stop / 挂起 / 全局关闭同步 / 撤销设备信任**：`backend.send` 的 `ctx` 立即取消并退出等待，满足 [同步后端业务契约 §2](../../contracts/sync-backend-contract.md#2-状态快照修订与撤销) 的同步撤销屏障，不拖延 `UnpairDevice` 或 `Stop`。
4. **旧版对端降级**：若对端未在 `Hello.caps` 声明 `clip_ack`，发送方在本地写完后直接返回 `SendResult{state: "sent", receipt_stage: "local_written"}`，保持与旧版设备 100% 兼容。

---

### 3. Engine 公共 DTO、历史摘要与事件节流

#### 3.1 `Operation` 与 `TargetStatus` 扩展

- `Operation` 新增字段：
  - `HistoryID HistoryID`（`json:"history_id,omitempty"`）：与本次发送关联的持久历史条目 ID（`persist=false` 或配对操作时为空）。
- `TargetStatus` 保留既有 `State`（`queued | sending | sent | failed | superseded`）以维持既有消费者兼容，新增正交只读字段：
  - `ReceiptStage ReceiptStage`（`json:"receipt_stage,omitempty"`）：
    - `""`（未写完 / 尚未到达回执阶段）
    - `"local_written"`（本地字节已写完；当 `State == sending` 时表示“等待对端确认”，当 `State == sent` 时表示“已发送但对端不支持回执”，当 `State == failed` 时表示“本地已写出但未获对端确认，结果不确定”）
    - `"remote_received"`（对端已接收并提交完成）
    - `"remote_discarded"`（对端已收到流但按策略/去重丢弃）
  - `Progress *TransferProgress`（`json:"progress,omitempty"`）：
    ```go
    type TransferProgress struct {
        BytesSent  int64 `json:"bytes_sent"`
        TotalBytes int64 `json:"total_bytes"`
        PartIndex  int   `json:"part_index,omitempty"`
        PartCount  int   `json:"part_count,omitempty"`
    }
    ```
    - 仅在内存态 `Operation` 保留，**不写入 SQLite**，不含文件名、路径、正文或地址。

> **已确认终态归类**：当对端明确返回 `clip_ack{status: "discarded"}` 时，目标终态为新增枚举 `TargetDiscarded = "discarded"`（`ReceiptStage = "remote_discarded"`），不计入 `OutcomeSuccess`（全 `sent` 才为 `success`；含 `sent` 与其他终态混合为 `partial`；全 `superseded` 为 `superseded`；其余归 `failed`）。等待 `clip_ack` 的默认超时上限固定为 **30 秒**。

#### 3.2 历史摘要与事件背压保护

- **不写库抖动**：传输进度更新**不写入** SQLite，不递增 `HistoryRevision`，不触发 `EventHistory`；仅在任务创建（`pending`）与全部目标终态（`sent/partial/failed`）时写一次历史摘要（与现有行为一致）。
- **事件节流**：Engine 对同一 `OperationID` 的纯进度更新做合并节流（最小发射间隔 200 ms），仅在目标离散状态切换、进入 `awaiting_receipt` 或操作终态时立即发射 `EventOperations`，防止大文件传输把 `EventBuffer(128)` 与本地 SSE 缓冲（32）打满触发 `resync_required`。

---

### 4. 本地服务 API 与展示接入

1. **新增活跃/关联操作列表接口**：
   - `GET /v1/operations`：返回当前服务代次内存保留的 `Operation[]`（按时间倒序），支持查询参数：
     - `active=true`：仅返回未终态（`queued` / `sending`）及最近短窗口内刚完成的操作；
     - `history_id=<id>`：过滤关联到指定历史条目的发送操作。
   - `GET /v1/operations/{id}`：沿用既有路由，返回包含 `history_id`、`progress`、`receipt_stage` 的完整快照。
2. **SSE 事件通知**：
   - `GET /v1/events` 在节流后的 `EventOperations` 触发时推送轻量通知（只含 `operation_id` / `history_id` / `kind`，不含文件名与正文），Desktop 据此增量拉取活跃操作，无需高频轮询或重载整页历史。
3. **UI 语义映射**（面向用户故事）：
   - 复制文件后，在该历史项或传输面板按目标设备展示：
     - 排队中：`queued`
     - 传输中：`sending` + 进度条（如 `42 MB / 100 MB · 42%`）
     - 等待确认：`sending` + `receipt_stage=local_written`（`等待对端确认…`）
     - 对端已接收完成：`remote_received`（`对端已接收`）
     - 兼容旧设备：`sent` + `local_written`（`已发送（对端不支持回执）`）
     - 对端丢弃：`remote_discarded`（`对端未接纳`）
     - 失败/结果不确定：`failed`（区分中途失败与“已写出但未收到对端确认”）

---

### 5. 接收端实时进度与中途放弃（Toast + Discard）

在接收端同样提供实时接收进度与用户主动放弃能力，**无需新增插件 RPC 方法**（`builtin.tailcat` 与外部 `peer_sessions` 插件的入站字节均经 Engine `peerProcBackend.Receive` 流式读取）：

1. **Engine 接收操作模型**：
   - `OperationKind` 新增 `OperationReceive = "receive"`，`Phase` 新增 `PhaseReceiving = "receiving"`。
   - `Operation.Receive *ReceiveOperation`（仅内存态，不写 SQLite，不含文件名/路径/正文/地址）：含 `BackendID`、`PeerID`、`State`（`receiving | committed | discarded | failed`）、`ContentKind`、`FileCount`、`Progress`（`BytesReceived`、`TotalBytes`、`PartIndex`、`PartCount`）。
   - 流式读盘期间按与发送端相同的 **200 ms** 合并节流规则更新 `Operation` 并发布 `EventOperations`；状态切换与终态立即发布。
2. **中途放弃（`DiscardReceive`）与会话保持**：
   - Engine 新增 `DiscardReceive(ctx, id OperationID) error`；本地服务新增 `POST /v1/operations/{id}/discard`。
   - 触发放弃后，Engine 立刻停止本地写盘、删除未发布暂存文件、释放去重与配额预留，将 `Operation` 立即转为 `PhaseCompleted` + `Receive.State = discarded` 并向后端返回 `outcome = "discarded"`。
   - `builtin.tailcat` 沿用既有 `OutcomeDiscard` 路径：在内存中 `drain` 剩余在途 wire 字节（不落盘、受 30 s 空闲预算约束），随后回发 `clip_ack{status: "discarded"}` 并**保持当前会话不断开**；发送端收到回执后诚实呈现 `TargetDiscarded`（「对端未接纳」）。
3. **Desktop 接收进度 Toast**：
   - 扩展 `boardui.ToastOptions` 支持可选 `Progress *float32`（复用 BoardUI `Notification` 底部 3px 进度条）与单条 `CloseLabel`。
   - 仅当入站为文件/目录或声明总字节 `>= 256 KiB` 时弹出持久 Loading Toast（避免短文本闪屏），展示来源设备、进度百分比与字节比；点击右上角 `×` 调用 `POST /v1/operations/{id}/discard` 放弃本次接收。
   - 接收成功（`committed`）后进度 Toast 自动收起，新条目直接出现在历史列表中。

---

## 受影响的公开契约清单（待审批后同步修订）

| 契约文件 | 受影响章节与变更内容 |
|---|---|
| [sync-backend-contract.md](../../contracts/sync-backend-contract.md) | §4 新增 `sync_host.send_progress`、扩展 `SendResult.receipt_stage`；§5 定义接收端 `clip_ack` 发送时机与不变量；新增 `Hello.caps` 与 `clip_ack` wire v1 控制帧规格 |
| [plugin-rpc-contract.md](../../contracts/plugin-rpc-contract.md) 与 `docs/contracts/protocol/plugin-rpc-v1.{frame,schema,vectors}.json` | 方法表/Schema/向量新增 `sync_host.send_progress` 与 `sync_send_result.receipt_stage` |
| [engine-contract.md](../../contracts/engine-contract.md) & [sdk-extension-contract.md](../../contracts/sdk-extension-contract.md) | 修正 §1.8 / §6.2 语义说明（区分本地写出与对端接收确认，仍保持无远端应用确认、无断点续传）；扩展 `Operation.HistoryID`、`TargetStatus.Progress`、`TargetStatus.ReceiptStage` 及事件节流约束 |
| [local-service-contract.md](../../contracts/local-service-contract.md) | 新增 `GET /v1/operations` 过滤查询、`Operation` 扩展字段与 SSE 进度节流约定 |

## 备选方案对比

| 方案 | 优点 | 缺点与取舍 |
|---|---|---|
| Engine `OpenPart` 读取字节估算（不改后端协议） | 零后端 RPC 改动 | 进程外后端与外部上传后端存在缓冲失真，提前 100%，违反 0029 职责边界；且无法承载对端回执 |
| 异步 `send_receipt` 回调（`backend.send` 写完即返回） | 释放 per-peer 发送队列略早 | `Operation.Done()` 后仍需回写状态，破坏内容释放、历史终态汇总与 CLI 等待模型，需额外维护跨调用回执表 |
| **本方案：`sync_host.send_progress` + `backend.send` 内有界等待 `clip_ack`** | 全生命周期收敛于单次 `backend.send` 根调用与 `grant_id`；真实反映写出进度与对端提交确认；向后兼容旧版 v2 对端与无进度后端 | 写完后额外等待一个 RTT + 对端提交耗时；需严格遵守写锁释放与异步发送 ACK 纪律防死锁 |

## 验收要求（实现阶段必须覆盖）

1. **端到端大文件进度与对端接收完成**：真实双节点（`builtin.tailcat`）发送大文件，观测到 `Progress.BytesSent` 单调递增、`awaiting_receipt` 过渡、最终 `remote_received` 仅在对端 `Receive` commit 完成后出现。
2. **并发双向大文件传输无死锁**：A→B 与 B→A 同时传输大文件，双方均能正常收到进度与 `clip_ack`，无写锁互锁或误超时。
3. **故障与不确定结果诚实性**：模拟发送中途断连、本地写完后对端未回 ACK 断连、ACK 超时三种故障，验证绝不标为 `remote_received` 且不自动重发。
4. **对端丢弃场景**：对端关闭文件同步或触发去重时返回 `clip_ack(status="discarded")`，发送方立即结束并报告 `remote_discarded`，连接保持可用。
5. **新旧互操作降级**：模拟未声明 `Hello.caps=["clip_ack"]` 的旧对端，发送方自动降级为 `local_written` 完成，不卡顿不报错。
6. **进程外 Go `peer_sessions` 插件子进程互操作**：覆盖 `sync_host.send_progress` 正常上报、乱序/倒退/超额/伪造 `grant_id` 拒绝、不实现进度上报的插件降级、未声明回执能力却返回 `remote_received` 被拒绝。
7. **撤销屏障与生命周期**：等待 `clip_ack` 期间触发 `UnpairDevice` / `SetRuntimeState(suspended)` / `Sync.Enabled=false` / `Stop`，验证发送立即取消且撤销屏障不阻塞。
8. **事件背压与隐私哨兵**：大文件高频写路径下 `EventOperations` 经节流不触发 `resync_required`；进度 DTO、事件、日志与错误中绝不出现文件名、路径、正文、地址或 token。
