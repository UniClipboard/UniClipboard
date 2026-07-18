# T3 / #1329：全类型远端入站 Receive Attempt 统一方案

Status: ready_for_implementation（已完成两轮冷读对抗评审，无剩余计划阻塞项）
Branch baseline: `16606fe43`
Scope: 当前分支单 PR；多个原子提交；覆盖所有远端入站 entry 类型
Supersedes in scope: `.planning/2026-07-17-t3-receive-attempt-statemachine-impl.md` 的“仅目录 entry”限制

> 原目录计划保留为实现历史和目录发布细节参考；本计划是当前 PR 的范围与验收权威。

---

## 0. 决策摘要

本 PR 将 `entry_receive_attempt` 从目录专用状态机升级为所有 **远端入站** clipboard entry 的接收生命周期权威。

纳入：

- P2P 入站文本、链接、富文本、representation blob、图片、单文件、平铺多文件、目录。
- Mobile LAN 经 `ApplyInboundClipboardUseCase` 落库的文本、图片和文件。
- 新 sender delivery 对已有 partial entry 的显式升级。
- entry 级取消、聚合进度、启动收敛、daemon API、WS 事件、Tauri HUD adapter、前端 hydrate 与 stale-event 拒绝。

排除：

- 本地 clipboard capture。
- `LocalRestore`。
- 已完整持有 entry 的 resurface；它不接收新 payload，不创建 attempt。
- 自动重发、离线队列、最终一致性。
- 单成员取消 UI。
- sender 侧 per-member 状态机。

核心判断：

1. attempt 状态是 entry 接收结果的唯一权威。
2. transfer/event 行只是 attempt-scoped item 投影，不能推导 entry 终态。
3. artifact journal 只是文件系统补偿/恢复元数据，不能推导 entry 终态。
4. entry 完整度与 attempt 结果是两个事实：非目录 partial entry 继续支持；目录继续全有或全无。
5. terminal attempt 表示该 attempt 的 DB 提交和 artifact settlement 已完成；不得在后台补偿未完成时提前进入终态。

---

## 1. 类型矩阵与行为约束

| 入站类型                     | attempt | item 投影                             | artifact journal    | partial 行为                             | 进度                             |
| ---------------------------- | ------- | ------------------------------------- | ------------------- | ---------------------------------------- | -------------------------------- |
| 纯文本 / 链接 / 全内联富文本 | 必须    | 无                                    | 无                  | 不适用                                   | 状态 spinner，无字节百分比       |
| representation blob / 图片   | 必须    | 每个 blob 一行                        | 无本地用户路径时无  | 保留：删除未完成 rep，可落 partial entry | 聚合 blob bytes/items            |
| 单文件                       | 必须    | 每个 blob 一行                        | 必须，加密路径      | 保留已成功 item，缺失 item 占位          | 聚合 bytes/items                 |
| 平铺多文件                   | 必须    | 每个 blob 一行                        | 必须，加密路径      | 保留已成功 item，缺失 item 占位          | 聚合 bytes/items                 |
| 目录                         | 必须    | 每个 file member 一行                 | 必须，加密 root map | 全有或全无                               | 聚合 bytes/items + publish phase |
| Mobile LAN file/image        | 必须    | provisional transfer 被收编进 attempt | 文件需要            | 与对应 image/file 类型一致               | 收编后进入 attempt 聚合          |

不变行为：

- fully-held dedup 继续 resurface，不创建 attempt。
- partial hash match 继续复用原 entry ID；新 sender delivery 创建新 attempt。
- partial entry 不写入系统 clipboard，也不进入 rapid dedup。
- OS clipboard write 仍是 entry commit 后的 best-effort side effect，不属于 attempt 成功条件。

---

## 2. 通用状态机

### 2.1 状态

```rust
pub enum AttemptState {
    Receiving,
    Committing,
    Cancelling,
    Failing,
    Completed,
    Cancelled,
    Failed,
}
```

| 状态         | 含义                                                   | 终态 | 可被新 delivery 替换        |
| ------------ | ------------------------------------------------------ | ---- | --------------------------- |
| `Receiving`  | decode/dedup 后，正在物化、下载、校验                  | 否   | 否                          |
| `Committing` | 已赢得 commit CAS，正在执行不可逆发布与组合事务        | 否   | 否                          |
| `Cancelling` | cancel 赢得 CAS，正在停止 item 并处理 partial/rollback | 否   | 否                          |
| `Failing`    | 失败裁决已确定，正在处理 partial/rollback              | 否   | 否                          |
| `Completed`  | 完整 entry 与 attempt 已在同一事务提交                 | 是   | 否；fully-held 走 resurface |
| `Cancelled`  | 取消 settlement 已完成                                 | 是   | 是，仅新 sender delivery    |
| `Failed`     | 失败 settlement 已完成                                 | 是   | 是，仅新 sender delivery    |

目录/文件系统子协议保留独立状态：

```rust
pub enum ArtifactPhase {
    Preparing,
    Publishing,
    Landed,
    RollingBack,
}
```

`ArtifactPhase` 不是 UI entry 状态，也不能替代 `AttemptState`。

### 2.2 CAS 矩阵

| 动作                     | 守卫                                              | 目标                        | CAS 失败语义                                                              |
| ------------------------ | ------------------------------------------------- | --------------------------- | ------------------------------------------------------------------------- |
| begin first              | 无 attempt row                                    | `Receiving`                 | 已存在则按当前状态分类                                                    |
| begin redelivery         | exact current attempt 且 state∈`Cancelled/Failed` | 新 attempt ID + `Receiving` | 非终态返回 `AlreadyReceiving`；ID 已变化返回 `Superseded`                 |
| claim commit             | exact attempt + `Receiving`                       | `Committing`                | `Cancelling` 赢则停止提交；新 attempt 赢则旧 attempt 丢弃                 |
| request cancel           | exact attempt + `Receiving`                       | `Cancelling`                | `Committing/Completed` 返回 too late；terminal 幂等；ID 不符为 superseded |
| begin failure settlement | exact attempt + state∈`Receiving/Committing`      | `Failing`                   | `Cancelling` 赢则取消优先；terminal/新 attempt no-op                      |
| finalize complete        | exact attempt + `Committing`                      | `Completed`                 | **只能在 entry 组合事务内执行**                                           |
| finalize cancel          | exact attempt + `Cancelling`                      | `Cancelled`                 | 与 optional partial entry + artifact landed/rollback 同事务或同恢复裁决   |
| finalize failure         | exact attempt + `Failing`                         | `Failed`                    | 与 optional partial entry + artifact landed/rollback 同事务或同恢复裁决   |

禁止：

- `Receiving -> Receiving(new_attempt_id)`；活动 attempt 不允许被 retry 抢占。
- member `Completed/Failed/Cancelled` 修改 attempt state。
- `Cancelled/Failed` 后继续异步落 partial entry 而不做 exact attempt guard。
- 仅凭“entry row 存在”判断 attempt 成功；Replace 场景中 entry 可能早已存在。

### 2.3 窄 intent ports

不扩张现有 `AdvanceEntryAttemptPort`。按用例权限拆分：

```rust
pub trait BeginReceiveAttemptPort { /* first + terminal redelivery */ }
pub trait ClaimReceiveCommitPort { /* Receiving -> Committing */ }
pub trait RequestReceiveCancellationPort { /* Receiving -> Cancelling */ }
pub trait BeginReceiveFailurePort { /* Receiving/Committing -> Failing */ }
pub trait GetReceiveAttemptPort { /* current attempt query */ }
pub trait ListUnsettledReceiveAttemptsPort { /* startup query */ }
```

terminal finalize 不暴露在通用 advance port；只能由 §5 的组合 settlement port 执行。同一个 SQLite adapter 可以实现全部小 port，但 cancel、apply、startup use case 只注入各自需要的能力。

### 2.4 Retry 定义

本 PR 不增加“接收方凭旧 ticket 自动重试”。有效 retry 只有一种：**新的 sender delivery 到达**。

- fully-held entry：resurface，无 attempt。
- partial entry + current state `Cancelled/Failed`：CAS 创建新 attempt，复用 entry ID。
- current state `Receiving/Committing/Cancelling/Failing`：返回 `AlreadyReceiving`，不并发下载。
- 没有 entry 的旧 terminal provisional attempt：新 delivery 可分配新 entry ID；旧 attempt 由 orphan retention 清理。

这符合 `VISION.md` 的“失败即报告，用户手动重发”锁定决策。

---

## 3. ReceiveWorkPlan：先计划，后执行

在任何 fetch 或持久 transfer seed 前，将 decoded envelope 规范化为不可变工作计划：

```rust
struct ReceiveWorkPlan {
    entry_id: EntryId,
    attempt_id: AttemptId,
    snapshot_hash: SnapshotHash,
    items: Vec<ReceiveItem>,
    directory_manifest: Option<InboundFileSetManifest>,
}

struct ReceiveItem {
    item_id: ReceiveItemId,
    original_blob_index: usize,
    role: ReceiveItemRole,
    declared_size: u64,
    sender_blob_entry_id: EntryId,
    filename: Option<String>,
}

enum ReceiveItemRole {
    Representation { representation_index: usize },
    FlatFile { file_index: usize },
    DirectoryMember { member_index: usize },
    MobileProvisional,
}
```

约束：

- `original_blob_index` 永不因 `partition()` 重排而变化。
- 每个 blob-bearing item 使用唯一 attempt-scoped projection ID。
- receiver item lifecycle 与 sender outbound batch lifecycle 拆成两个 context；不再用一个 `FetchTransferContext` 同时表达两种身份。
- 所有 item 必须在首次 fetch 前 fallible pre-seed；seed 失败则 attempt 进入 failure settlement，不能继续下载。
- representation blob 必须携带 attempt ID，并在 fetch 前后检查 attempt state。
- empty directory 不创建 byte-transfer item；manifest 仍记录其成员语义。

推荐 transfer ID：

```text
{entry_id}:attempt:{attempt_id}:item:{item_index}
```

不从 ID 前缀解析业务字段；查询始终使用结构化 `entry_id + attempt_id` 列。

---

## 4. 持久化与隐私门禁

### 4.1 `entry_receive_attempt`

保留“一 entry 一 current attempt”结构，不加 `clipboard_entry` 外键，因为 attempt 在 entry 创建前存在。

字段：

```text
entry_id
current_attempt_id
attempt_state
failure_code       nullable, stable enum only
updated_at_ms
```

- `failure_code` 只允许稳定、非用户内容枚举，如 `network_unavailable`、`integrity_failed`、`storage_unavailable`、`interrupted`。
- 原始错误文本不进入明文列；需要保留时进入 AEAD ciphertext。
- 删除 entry 前先通过应用层 deletion settlement 阻止/收敛活动 attempt，解密并清理 trusted-root 下的未落地 artifacts；cleanup 失败则保留 journal 并中止删除。
- artifact settlement 成功后，才在一个 DB 事务删除 attempt、item projection、event log 和 journal。landed user-owned artifacts 不随 history entry 删除。
- 无 entry 的 terminal provisional attempt 在 settlement 完成后保留 7 天，之后由 orphan reconciliation 删除。

### 4.2 加密 receive artifact journal

将当前目录专用 `directory_publish_log` 泛化/迁移为 receive artifact journal；目录 root map 与平铺文件 staging/final path 使用同一加密能力。

```text
receive_artifact_log (
  entry_id,
  attempt_id,
  phase,                    -- enum, plaintext
  artifact_map_ciphertext,  -- AEAD
  partial_publication,      -- non-sensitive flag
  partial_artifact_count,   -- non-sensitive count
  resolution,               -- pending|landed|rolled_back
  updated_at_ms,
  PRIMARY KEY(entry_id, attempt_id)
)
```

密文载荷：

```rust
struct ReceiveArtifactMap {
    artifacts: Vec<ReceiveArtifact>,
}

struct ReceiveArtifact {
    item_id: String,
    staged_path: PathBuf,
    final_path: PathBuf,
    ownership: ArtifactOwnership,
}
```

- 使用独立 subkey info，例如 `receive-artifact-log-v1`。
- AAD 绑定 `(entry_id, attempt_id)`。
- native path bytes 编码，不做 lossy UTF-8。
- path/root/filename 不得出现在 tracing、错误文本或明文列。
- 普通 text/inline representation attempt 不创建 artifact journal。
- `resolution=pending` 表示 FS/DB settlement 未完成；valid terminal attempt 必须对应 `landed`、`rolled_back` 或无 journal。
- `terminal + pending` 不允许根据 entry 是否存在猜测结果：它是持久化损坏/历史中断，必须保留 metadata、阻止 receive readiness 并发出可观察错误，直到受控 repair。

### 4.3 加密 transfer projection metadata

当前 `file_transfer.filename`、`cached_path`、自由文本 `failure_reason` 违反持久化密文底线。迁移后：

```text
file_transfer (
  transfer_id,
  entry_id,               -- nullable only while Mobile provisional
  attempt_id,             -- nullable only while Mobile provisional
  binding_state,          -- provisional|attempt_bound
  item_role,              -- stable enum
  file_size,
  status,                 -- stable enum
  source_device,
  failure_code,           -- stable enum only
  metadata_ciphertext,    -- filename/cached path/detail under AEAD
  created_at_ms,
  updated_at_ms
)
```

- `metadata_ciphertext` AAD 至少绑定 `transfer_id`；attempt-scoped IDs 天然绑定 attempt。
- `binding_state=provisional` 是唯一允许的新 remote attemptless projection；它没有 entry authority，不能驱动 entry UI 状态，且必须被 §6.4 原子 adopt/discard。
- `Started` projection 不再把 event filename 写入明文列。
- timeout/startup cleanup port 返回 adapter 解密后的 path；bootstrap 不读数据库明文路径。
- API DTO 不返回底层 cached path。

### 4.4 加密 `file_transfer_events`

当前 `payload_json` 明文包含 filename 和 failure detail，必须迁移为：

```text
file_transfer_events (
  id,
  transfer_id,
  sequence,
  event_type,            -- stable enum
  payload_ciphertext,    -- AEAD(serialized FileTransferEvent)
  occurred_at_ms
)
```

- subkey 与 projection/artifact journal 分离。
- AAD 绑定 `(transfer_id, sequence, event_type)`。
- append 必须在确定 sequence 后加密，并与 projection update 保持同一 SQLite 事务。
- load 遇到损坏/错误 key 必须显式报错，不允许 default 空 timeline。

### 4.5 旧明文数据处理

MasterKey 不在 Diesel SQL migration 中可用，因此不尝试 SQL 内“加密迁移”。transfer projection/event log 是可重建派生数据，采用以下一次性安全迁移：

1. migration 将旧 transfer/event tables 重命名为明确的 plaintext legacy tables，创建 encrypted v2 tables，不复制 plaintext payload，并写 `pending_physical_purge` marker。
2. daemon 解锁后、网络 worker 启动前读取 legacy in-flight rows，只清理经过 trusted managed-cache/configured-save-root 验证的 attempt-owned placeholder/staging artifacts；路径缺失幂等，landed/user-owned 文件禁止删除。
3. 执行 bounded trusted-root staging sweep，覆盖无法安全采用 legacy arbitrary path 的升级残留。
4. 删除 legacy tables，执行 `PRAGMA wal_checkpoint(TRUNCATE)` + `VACUUM`。
5. byte-scan DB/WAL/SHM 确认 sentinel 不存在后，才把 marker 改为 `completed`。
6. 任一步失败或进程退出都保持 `pending_physical_purge`；下次解锁幂等重跑，receive readiness 不开放。
7. 复用 `search/sqlite_index.rs` 的 bounded busy retry 范式。

这是本 PR 的 merge gate，不允许以“legacy 行兼容”为由保留新 plaintext writes。

---

## 5. 通用组合事务

### 5.1 Port

用通用 inbound commit intent 替代目录专用组合端口：

```rust
pub trait CommitInboundReceivePort {
    async fn commit_inbound_receive(
        &self,
        settlement: &InboundReceiveSettlement,
    ) -> Result<(), InboundReceiveCommitError>;
}

pub enum InboundReceiveSettlement {
    Complete {
        record: InboundReceiveRecord,
        attempt_id: String,
        file_set: Option<EntryFileSet>,
        artifacts: CompletedArtifacts,
        now_ms: i64,
    },
    Partial {
        record: InboundReceiveRecord,
        attempt_id: String,
        terminal: PartialTerminal, // Cancelled or Failed
        file_set: Option<EntryFileSet>,
        artifacts: PartialArtifacts,
        now_ms: i64,
    },
    NoEntry {
        entry_id: EntryId,
        attempt_id: String,
        terminal: PartialTerminal,
        artifacts: NoEntryArtifacts,
        now_ms: i64,
    },
}

pub enum CompletedArtifacts { None, Landed }
pub enum PartialArtifacts { None, Landed, RolledBack }
pub enum NoEntryArtifacts { None, RolledBack }
pub enum PartialTerminal { Cancelled, Failed }
```

组合事务原子包含：

1. `Complete/Partial` Create：event + representations + entry + selection。
2. `Complete/Partial` Replace：new event + representations + selection + entry summary，删除被替换的旧内容。
3. `NoEntry` 不写 entry/event/representation/selection/file-set。
4. optional encrypted `EntryFileSet`；类型禁止 `NoEntry + file_set`。
5. exact `(entry_id, current_attempt_id, expected_state)` guard。
6. attempt terminal transition。
7. optional artifact journal resolution 写为 `landed` 或 `rolled_back`。

任何 guard 失败，整个事务 rollback。尤其：新 attempt 已开始后，旧 attempt 的迟到 partial commit 不能覆盖 entry。三个 artifact enum 在类型层排除 `Complete + RolledBack`、`NoEntry + Landed user files` 等非法组合。

### 5.2 Capture 边界

- local capture 继续走现有 capture 端口，不创建 attempt。
- remote inbound capture 必须提供 `InboundReceiveCommitContext`，不得回落到旧的 event-first Create 路径。
- normalization/spool 可以复用 `CaptureClipboardUseCase`，但 remote persistence 必须统一进入组合端口。
- 删除目录专用 public commit 分支；目录成功通过 `Complete + file_set + Landed`，取消/失败通过 `NoEntry + RolledBack` 使用通用端口。
- SQL insert/replace 继续复用 connection-level helper，不复制 persistence SQL。

### 5.3 文件系统与数据库边界

无法跨 SQLite 与文件系统做真正原子事务，采用 journal + CAS：

- 所有 attempt-owned path 在可见前先写加密 artifact map。
- 完整接收：`Receiving -> Committing` 后 publish artifacts，再执行组合事务 `-> Completed + landed`。
- 目录取消：`Receiving -> Cancelling`，rollback 全部 artifacts，事务 `-> Cancelled`，不落 entry。
- 非目录 partial 取消/失败：settler 根据现有行为保留已成功 items、回滚未完成 items，组合事务落 partial entry 并 `-> Cancelled/Failed + landed`。
- crash 在 publish 与 DB commit 之间：attempt 仍为 nonterminal、journal 未 landed；startup rollback 后进入 `Failed`。
- valid terminal transition 与 artifact resolution 在同一 DB 事务发生，因此正常运行不会产生 `terminal + pending`。
- `terminal + pending` 被视为持久化损坏/历史中断：保留 journal、阻止 receive readiness、发出可观察错误；禁止按 entry 是否存在猜测 complete/rollback。

---

## 6. 应用层编排

### 6.1 P2P `ApplyInboundClipboardUseCase`

新顺序：

1. decode envelope。
2. acquire snapshot identity lock。
3. persistent dedup。
4. fully held：resurface 并返回，不创建 attempt。
5. choose receiver entry ID；partial match 复用 ID。
6. begin first/redelivery attempt。
7. build immutable `ReceiveWorkPlan`。
8. fallible pre-seed all items。
9. emit `IncomingPending(entry_id, attempt_id, ...)` 与 attempt `Receiving` event。
10. materialize with exact attempt checkpoints。
11. complete：claim commit，publish artifacts，common commit。
12. cancel/fail：进入 `Cancelling/Failing`，执行 partial/rollback settlement，finalize terminal。
13. commit 后 emit attempt terminal event + remote `NewContent(entry_id, attempt_id)`。
14. best-effort search index、active register、OS clipboard write 保持在 commit 之后。

每个 error path 必须明确回答：

- attempt 当前状态是什么？
- artifacts 是否 landed/rolled back？
- partial entry 是否已 guarded commit？
- 向 UI 发的是 attempt 状态还是 item 状态？

### 6.2 Materializer

- `materialize()` 接收 `ReceiveWorkPlan`，不再接收松散 `blob_refs + optional attempt_id`。
- representation/file/directory item 都使用 exact attempt context。
- 每 item fetch 前后查询 attempt；仅 `Receiving` 可继续。
- cancel error 不写 item Failed。
- member failure 只更新 item projection；attempt 由 orchestrator 进入 `Failing`。
- `MaterializeResult` 改为显式：

```rust
enum MaterializeOutcome {
    Complete(MaterializedReceive),
    PartialCancelled(MaterializedReceive),
    PartialFailed { materialized: MaterializedReceive, code: FailureCode },
}
```

禁止继续用一个 `is_partial: bool` 混合 cancel 与 failure。

### 6.3 Generic cancellation

Facade API 必须包含调用方观察到的 attempt ID：

```rust
cancel_entry_receive(entry_id, expected_attempt_id)
```

不能只传 entry ID 后重新读取 current attempt；否则旧 UI 点击可能取消刚开始的新 attempt。

流程：

1. exact CAS `Receiving -> Cancelling`。
2. `Committing/Completed` 返回 `TooLate`。
3. `Cancelling` 幂等重试 compensation。
4. `Cancelled/Failed` 返回 terminal outcome。
5. current attempt ID 不匹配返回 `Superseded`，绝不触碰新 attempt。
6. 终止 registry 中 exact attempt 的 fetch。
7. bulk-cancel exact attempt 的 pending/transferring item projections。
8. 调用 settler 完成 partial/rollback 和 `Cancelling -> Cancelled`。

公开结果：

```text
cancellation_requested | cancelled | too_late | already_terminal | superseded | not_receiving
```

### 6.4 Mobile LAN adoption

Mobile `PUT /file` 早于 clipboard document/entry attempt：

- provisional bytes 只能写 trusted managed staging，不得在 dedup/attempt 前发布到用户目录。
- provisional transfer 可以发 item progress，但 `entry_id/attempt_id` 为空、`binding_state=provisional`，不得更新任何 entry 状态。
- SyncDoc dedup 后通过封闭的原子 port 二选一：

```rust
pub trait FinalizeProvisionalReceivePort {
    async fn finalize(
        &self,
        provisional_transfer_id: &str,
        action: ProvisionalReceiveAction,
    ) -> Result<(), ProvisionalReceiveError>;
}

pub enum ProvisionalReceiveAction {
    AdoptIntoAttempt {
        entry_id: EntryId,
        attempt_id: String,
        item_id: ReceiveItemId,
        role: ReceiveItemRole,
    },
    DiscardAsFullyHeld,
}
```

- `AdoptIntoAttempt` 守卫目标 attempt 是 exact current `Receiving`，且 provisional 尚未被认领；写 association、item role、encrypted metadata，必要时 reseal ciphertext，然后才允许用户目录 publication。
- `DiscardAsFullyHeld` 删除 provisional artifact、关闭 projection，但不创建 attempt；解决 fully-held resurface 与 provisional lifecycle 的冲突。
- 已完成 provisional transfer 收编后计入 aggregate completed bytes。
- 旧 fake `mobile-pending:<id>` entry、`link_then_complete` best-effort 两步和 dedup 后直接 complete 必须删除。
- adopt/discard 失败是可观察流程错误，不得 warn 后伪成功；crash 后由 provisional orphan reconciliation 收敛。
- mobile text 没有 provisional transfer，但仍创建快速 attempt 并原子 commit。

---

## 7. Startup reconciliation

daemon 层新增唯一 `ReceiveReadinessCoordinator`，它拥有下列顺序并暴露幂等 `ensure_ready()` gate：

1. Space/MasterKey 解锁。
2. 完成 `pending_physical_purge` maintenance。
3. `reconcile_receive_attempts_on_startup()`。
4. reconcile Mobile provisional orphans。
5. 仅对保留的 `attempt_id IS NULL` legacy projection 运行兼容 sweep。
6. sweep trusted receive staging roots。
7. open receive readiness gate。
8. 启动/放行 P2P inbound、Mobile LAN listener、file-sync orchestrator、timeout sweep 和读取 encrypted cleanup metadata 的 worker。

自动解锁、手动解锁、`/lifecycle/ready` retry 都必须 await 同一个 coordinator，不能各自 notify worker。现有 detached startup recovery 必须改为 gate-owned await；失败时 gate 保持关闭，health/API 暴露可观察 degraded reason，不得 best-effort 放行。

若应用以 locked 状态启动：

- 不得在无法解密 artifact metadata 时把 attempt 标终态后遗忘。
- 记录 pending reconciliation；解锁后通过同一 coordinator 重跑。
- ciphertext 损坏、AAD/key 不匹配时保留 attempt+journal、关闭 gate 并返回明确 security/recovery error；禁止删除 metadata 后宣告 settlement 完成。

恢复矩阵：

| attempt      | journal  | 处理                                                                                     |
| ------------ | -------- | ---------------------------------------------------------------------------------------- |
| `Receiving`  | none     | item rows 失败收敛，attempt `Failing -> Failed`                                           |
| `Receiving`  | unlanded | rollback artifacts，finalize Failed                                                      |
| `Committing` | unlanded | rollback 已发布 artifacts，finalize Failed                                                |
| `Cancelling` | any      | 继续 cancel settlement，finalize Cancelled                                               |
| `Failing`    | any      | 继续 failure settlement，finalize Failed                                                 |
| terminal     | pending  | 视为持久化损坏，保留 metadata、关闭 readiness、要求受控 repair；不猜测 complete/rollback |
| `Completed`  | landed   | no-op                                                                                    |

组合事务保证“entry 已写但 attempt 仍 Committing”不可出现；Replace 也不需要用 entry 存在性猜测成功。

reconciliation 必须幂等，可在同一启动中重跑。

---

## 8. 查询、事件与 API

### 8.1 Generic progress projection

将 `DirectoryReceiveProgress` 改为：

```rust
pub struct EntryReceiveProgress {
    pub entry_id: String,
    pub attempt_id: String,
    pub state: AttemptState,
    pub failure_code: Option<FailureCode>,
    pub total_bytes: i64,
    pub completed_bytes: i64,
    pub items_total: u32,
    pub items_completed: u32,
}
```

- state 来自 attempt authority。
- bytes/items 来自 exact current attempt rows。
- item 全 completed 不得推导 `Completed`。
- inline entry 合法返回 `0/0` items 和 attempt state。

### 8.2 Host events

新增独立 authority event：

```rust
ReceiveAttemptHostEvent::StateChanged {
    entry_id,
    attempt_id,
    state,
    failure_code,
}
```

以下 remote inbound event 全带 `attempt_id: Some(...)`：

- `ClipboardHostEvent::IncomingPending`
- remote `ClipboardHostEvent::NewContent`
- `TransferHostEvent::StatusChanged`
- `TransferHostEvent::Progress`

local/outbound event 为 `None`。member event 只更新 item，不写 entry state。

`HostEvent` bus 是 `clipboard.new_content` 的唯一发布权威。删除 `InboundClipboardSyncWorker` 的直接 WS `new_content` 发布和私有 payload；worker 只调用 facade 并记录 outcome。Resurface 由应用层发布合法的 attemptless refresh event。集成测试断言一次 Applied 只产生一条 WS event，且新入站 entry 带 exact attempt ID。

### 8.3 Daemon contract

列表/详情 DTO 墂可选通用对象：

```json
{
  "receiveAttempt": {
    "attemptId": "...",
    "state": "receiving",
    "failureCode": null,
    "totalBytes": 123,
    "completedBytes": 45,
    "itemsTotal": 3,
    "itemsCompleted": 1,
    "canCancel": true
  }
}
```

取消 endpoint：

```text
POST /clipboard/entries/{entry_id}/receive-attempts/{attempt_id}/cancel
```

body 只接受稳定 reason enum；当前只有 `local_user`。

涉及：

- `crates/uc-daemon-contract`
- `crates/uc-webserver`
- `crates/uc-daemon-client`
- daemon OpenAPI/codegen
- `src-tauri/crates/uc-tauri/src/activity_hud/{emitter,state,actions}.rs` 与装配入口：HUD 行以 `(entry_id, attempt_id)` 为身份，删除 `transfer_id == entry_id` 假设；item events 只更新 attempt aggregate，cancel 调 daemon client exact endpoint；GUI 不实例化 AppFacade
- `src/api/daemon`

兼容策略：

- JSON 新字段使用 optional，旧 client 可忽略。
- 当服务器查询到 authoritative attempt 时，缺少 attempt ID 的 remote member event 一律不能更新 entry state。
- 兼容 `attempt_id=NULL` 仅用于迁移前 legacy rows；本 PR 完成后所有新 remote inbound writes 必须非 NULL。

---

## 9. 前端单一状态来源

Redux 目标结构：

```ts
interface ReceiveAttemptView {
  attemptId: string
  state: ReceiveAttemptState
  failureCode: string | null
  totalBytes: number
  completedBytes: number
  itemsTotal: number
  itemsCompleted: number
}

interface FileTransferState {
  activeItems: Record<string, TransferProgressInfo>
  itemIdsByAttempt: Record<string, string[]>
  receiveAttemptByEntry: Record<string, ReceiveAttemptView>
}
```

Reducer 规则：

1. hydrate `receiveAttemptByEntry` 后才接受 realtime item event。
2. incoming pending 为 entry 建立 current attempt。
3. event attempt ID 不等于 current attempt：丢弃。
4. current attempt 已 terminal：迟到 item progress/status 不得回退 attempt state。
5. member all completed：只更新 bytes/items，不设置 entry Completed。
6. `ReceiveAttemptStateChanged` 是 realtime entry state 唯一来源。
7. remote `NewContent` 只移除 exact attempt 的 pending card。
8. cancel command 携带 UI 当前 `attemptId`；`Superseded` 后 refetch，不重试取消。

UI：

- 所有 remote inbound Receiving entry 使用同一 pending/history card 状态。
- blob-bearing 显示 aggregate percentage 和 item count。
- inline 0-byte attempt 显示 spinner，不显示 `0%`。
- `Cancelling` 显示中性“正在取消”，按钮 disabled。
- `Committing` 显示“正在完成”，按钮 disabled。
- `Cancelled` 中性样式；`Failed` 错误样式。
- card 与 preview 共用 entry-attempt cancellation action，不再调用单 transfer cancel。

---

## 10. 分阶段实施与硬门禁

单 PR 不等于无门禁的大提交。每阶段必须满足 gate 才能继续。

### Phase 0：计划与 characterization

- 固化 text/image/single/flat/directory/mobile 当前行为测试。
- 特别锁定 partial entry、dedup、OS write suppression、save-dir 行为。
- 完成计划冷读评审。

Gate：现有 targeted tests 全绿；计划无未决状态/隐私/恢复语义。

### Phase 1：隐私与当前目录 blocker

- 加密 transfer event payload/projection metadata。
- 清理旧 plaintext rows + WAL residue。
- representation blobs 加入 directory attempt、pre-seed、cancel/progress。
- `seed_pending_transfer` 改为 fallible。

Gate：DB/WAL/SHM sentinel byte-scan；目录 mixed representation cancel contract；无新 plaintext writes。

### Phase 2：通用领域与 SQLite authority

- content-neutral states + CAS matrix。
- generic progress query。
- artifact journal 泛化。
- deletion/orphan cleanup ports。

Gate：真实 SQLite 覆盖所有合法/非法迁移、winner race、strict parse、orphan retention。

### Phase 3：通用组合事务

- common Create/Replace commit port + adapter。
- complete/partial exact attempt guard。
- directory adapter 收敛到 common port。
- remote capture 不再走 event-first path。

Gate：Create/Replace/partial rollback 注入测试；新 attempt 阻止旧 partial commit；entry/event/reps/selection/file-set/attempt/journal 原子性。

### Phase 4：所有执行路径接入

- `ReceiveWorkPlan`。
- P2P inline/rep/file/flat/directory.
- Mobile provisional adoption.
- per-item attempt projection.
- typed materialize outcomes.

Gate：六类 type matrix 成功/取消/失败/新 delivery；首次 fetch 前 durable seed；除 `binding_state=provisional` 且无 entry authority 的 Mobile staged upload 外，无 attemptless 新 remote writes；fully-held provisional discard、adopt CAS 与 adopt-crash 测试通过。

### Phase 5：取消与启动收敛

- exact `(entry, attempt)` cancellation facade。
- cancel/failure settlement。
- `ReceiveReadinessCoordinator` + startup reconciliation。
- trusted-root artifact cleanup。
- entry deletion 先 settle、后 DB delete。

Gate：cancel-vs-commit、old-partial-vs-new-delivery、same-attempt late-completed、delete-vs-commit 四类 deterministic race test；locked/unlocked restart test；purge/recovery 失败时所有 receive workers 保持 gated。

### Phase 6: daemon/client/Tauri/frontend

- generic DTO/query.
- authority event + attempt-bearing member events.
- cancel endpoint/client/HUD adapter.
- frontend hydrate, aggregate, stale filter, card action.

Gate: contract serialization tests, reducer stale/terminal monotonic tests, component cancel tests, reload hydration tests.

### Phase 7：最终验证与 cleanup

- 删除目录专用/attemptless remote compatibility path。
- 严格 review 隐私、事务、竞态、跨平台 path。
- atomic commit 整理。

Gate：§12 全部命令与测试通过；不存在 P0/P1 reviewer finding。

---

## 11. 建议原子提交序列

一个 PR，建议 15 个可独立构建提交：

1. `test: characterize inbound entry receive behavior`
2. `arch: define encrypted transfer persistence ports`
3. `impl: encrypt transfer projection and event persistence`
4. `fix: bind directory representation blobs to receive attempts`
5. `arch: add generic receive attempt states and narrow ports`
6. `impl: add generic sqlite receive attempt adapters`
7. `arch: define atomic inbound receive settlement`
8. `impl: commit inbound entries and attempts atomically`
9. `refactor: build attempt-scoped receive work plans`
10. `feat: cut over inbound receive paths to generic attempts`
11. `refactor: remove directory-only attempt authority`
12. `feat: reconcile and cancel entry receive attempts`
13. `feat: gate daemon receive readiness`
14. `feat: expose receive attempts through daemon contracts`
15. `feat: render attempt-authoritative inbound progress`

为满足“每提交可构建”与“PR 内不保留双权威”：提交 5/6 只增加未接线的 generic contracts/adapters；提交 10 是唯一 runtime cutover；提交 11 紧随其后删除旧 states、目录 ports、parse/write 路径和 compatibility tests。禁止 feature flag、双写或在提交 11 之后保留 V1 runtime path。

Core port 与 infra adapter 保持分提交；API contract 传播作为一个 transport intent，可跨 contract/webserver/client，但不混入 persistence。若实际编译依赖使 10/11 无法独立构建，先调整 additive contract 边界，不得临场合并 core+infra 来绕过提交规则。

---

## 12. 测试矩阵与验证命令

### 12.1 类型测试

每类至少覆盖：

- complete success.
- cancel before first item.
- cancel during item.
- failure。
- new sender delivery after Cancelled/Failed.
- stale old-attempt event.
- restart while nonterminal.

类型：

1. inline text/link.
2. representation blob/image.
3. single file.
4. flat multi-file.
5. directory mixed roots + representation blob.
6. Mobile LAN provisional file/image + text.

### 12.2 竞态定点测试

- cancel CAS 与 claim commit CAS 同时开始，恰有一个 winner。
- old attempt partial commit 暂停，新 delivery 尝试开始：settlement 未终态前 retry 不得开始；terminal 后旧 commit 不得再运行。
- attempt Cancelled 后同 attempt member Completed 晚到：entry state 保持 Cancelled。
- new attempt Receiving 后旧 attempt progress/status/new_content 晚到：全部丢弃。
- publish collision 更新 journal 后 crash：startup 使用新 final mapping rollback。
- DB commit 成功后 crash：attempt 已 Completed、journal Landed，不 rollback。

### 12.3 隐私测试

- sender filenames、native paths、failure detail sentinel 不出现在 DB/WAL/SHM。
- event ciphertext 交换 transfer/sequence 后解密失败。
- artifact ciphertext 交换 entry/attempt 后解密失败。
- locked session 无法读取 metadata，且不会错误终结 attempt。
- ciphertext 损坏/AAD 不匹配保留 metadata 并关闭 receive readiness。
- `pending_physical_purge` 在 checkpoint/VACUUM 前 crash 后会重跑，只有 raw scan 通过才变 `completed`。
- tracing 捕获不含 filenames/paths/payload。

### 12.4 事务测试

- Create 任一步失败全 rollback。
- Replace 任一步失败保留旧 entry/event/reps/selection。
- Complete guard 不是 `Committing` 时全 rollback。
- Partial guard attempt ID/state 不匹配时全 rollback。
- optional file set 与 artifact landed 同事务。
- entry delete 先 settle active/unlanded artifacts，再清理 attempt/projection/event journal，不触碰 landed user-owned artifacts。
- cleanup 失败时 journal 与 DB record 保留，delete 返回错误。
- delete 与 claim commit/cancel 并发时 exact CAS 只有一个所有者。

### 12.5 命令

```bash
cargo fmt --all -- --check
cargo check -p uc-core -p uc-infra -p uc-application -p uc-bootstrap
cargo test -p uc-infra --test entry_receive_attempt_contract
cargo test -p uc-infra --test receive_artifact_log_contract
cargo test -p uc-infra --test inbound_receive_commit_contract
cargo test -p uc-infra file_transfer
cargo test -p uc-application apply_inbound
cargo test -p uc-application mobile_sync
cargo test -p uc-webserver clipboard
cargo test -p uc-daemon-client
bun run test -- src/hooks src/store src/components/clipboard
bun run lint
bun run build
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
```

workspace 命令若本地时限不足，必须由 CI 完整跑完；不得把 timeout 记作通过。

---

## 13. 风险清单

| 风险                                   | 级别 | 控制                                                                      |
| -------------------------------------- | ---- | ------------------------------------------------------------------------- |
| 普通 Create 共享持久化路径改造         | 高   | contract-first；connection helper 单一来源；local capture 不切换          |
| partial settlement 与 retry 竞态       | 高   | `Cancelling/Failing` 非终态 + exact guard + deterministic pause tests     |
| transfer event/projection 明文迁移     | 高   | 丢弃派生历史、checkpoint+VACUUM、raw byte scan                            |
| Mobile provisional transfer 无 attempt | 高   | adoption port；adoption 失败使 attempt 失败，禁 best-effort               |
| 前端 member 状态覆盖 attempt           | 高   | 独立 authority event；terminal monotonic；成员只供进度                    |
| flat file FS/DB crash window           | 高   | encrypted artifact journal；unlanded rollback；partial landed transaction |
| 单 PR review 面积大                    | 高   | hard gates、atomic commits、每阶段 reviewer、最终 strict review           |
| inline entry 增加 DB 写                | 低   | attempt 与 commit 同连接事务；不创建 item/journal rows                    |
| legacy API/event compatibility         | 中   | optional wire fields + hydrate gate；PR 内删除新写 attemptless path       |

---

## 14. 完成定义

只有以下全部成立才可请求合并：

- 所有新 remote inbound entry 都有非空 attempt ID；fully-held resurface 除外。
- local capture/restore 没有 attempt row。
- attempt 是 entry receive state 唯一权威；member 全完成不产生 entry Completed。
- complete 与 partial commit 都有 exact attempt/state transaction guard。
- cancellation endpoint 以 `(entry_id, attempt_id)` 为目标，不会取消 newer attempt。
- representation blob、flat file、directory member、mobile provisional item 都进入当前 attempt 聚合。
- startup 在网络 worker 前完成可重入 reconciliation；locked 状态不会遗忘 pending settlement。
- filename/path/failure detail/event payload 不以明文持久化；DB/WAL/SHM byte-scan 通过。
- 所有 FS artifact 在 journal 中先记录后暴露；terminal 时 landed 或 rollback 已确定。
- daemon/client/Tauri/frontend 使用 generic `receiveAttempt`，拒绝 stale/same-terminal member 回退。
- 自动 retry/offline resend 不存在。
- 目录专用 attempt authority 与新 generic authority 不并存。
- targeted、workspace、frontend、clippy、format、diagnostics 全部通过。
