# ADR-009：拆分 `FileTransferRepositoryPort` 为意图端口，并删除未接线的接收侧投影方法

- **状态**：Accepted（已采纳）
- **日期**：2026-06-18
- **相关文档**：[`ports.md`](./ports.md)、[`module-boundaries.md`](./module-boundaries.md)、`crates/uc-core/AGENTS.md`（§5.4 Port 文档纪律、§2.3 新需求=新小端口）

## 1. 背景

### 1.1 现状

`uc-core/src/ports/file_transfer_repository.rs` 定义了一个 14 方法的
`FileTransferRepositoryPort`，负责「接收侧文件传输跟踪」。单一适配器
`DieselFileTransferRepository`（`uc-infra`）实现它，另有一个
`NoopFileTransferRepositoryPort` 占位 stub。

该端口同时混入四类职责方向：

- 接收侧投影写入（seed / relink）
- 状态迁移（mark_transferring / mark_completed / mark_failed / refresh_activity / bulk_fail）
- 元数据回填（backfill_announce_metadata）
- 查询 / 投影（get_transfer / list / summary / get_entry_id）

不同消费者依赖其中 **不同的子集**，却都被迫持有整张 14 方法表面。

### 1.2 这违反项目自己的端口规范

`ports.md` 已锁定以下规则，本端口逐条命中：

- **§2.2**：use case 不得依赖「catch-all 仓储接口」。
- **§4.1**：「若一个 trait 的方法被不同 use case 以不同组合依赖，则它过大。」
- **§6.1**：`RepositoryPort` 等宽泛命名被明令禁止作为上层依赖名——它「天然鼓励方法堆积」。
- **§3.1 / §3.2**：查询与命令不得混在同一 trait。

### 1.3 触发本次复查的关键证据

对 14 个方法逐一 grep 生产调用点后发现：**7 个方法没有任何生产消费者**——
`insert_pending_transfers`、`mark_transferring`、`refresh_activity`、
`backfill_announce_metadata`、`get_transfer`、`mark_completed`、
`list_transfers_for_entry`（最后一个仅 `#[cfg(test)]` 用；`mark_completed`
的疑似调用实为另一 trait `RepresentationCachePort` 打在 `rep_id` 上的同名方法）。

这 7 个方法是 2026-04~05「direct-transfer-fixes / iroh-migration」时期设计的
**接收侧分块进度投影**（逐块更新接收进度）的预留面，其写入侧驱动从未接线，
或在流程演进后被摘除。`Noop` stub 注释里「Plan 02 接上真 adapter 前的占位」
亦已过时——真 adapter 早已在 `assembly.rs` 接上。

## 2. 决策

### 2.1 删除 7 个未接线的接收侧投影方法（删除测试先行）

删掉上列 7 个方法，连同 `NoopFileTransferRepositoryPort` 死 stub 与 Diesel
适配器中对应的实现。这些方法零消费者，删除不会把复杂度转移到任何地方——纯粹
缩小接口表面、零行为变化。

> **重要（防止幽灵知识回流）**：将来若真要做「接收侧分块进度 / 断点续传」
> 功能，**不得** 把这些方法加回老端口。按 `uc-core/AGENTS.md §2.3`，应针对
> 那个具体 use case **重新定义贴合其意图的新小端口**。本 ADR 的存在就是为了
> 拦住「这看起来像误删，加回来吧」这一类改动。

### 2.2 将剩余 7 个活方法拆为 5 个意图端口（无 catch-all Store）

所有活方法的 port 级直接消费者都在应用层 / 组合根，**没有 infra 内部消费者**，
因此不需要保留 `ports.md §5` 意义上的低层 `Store`；Diesel 适配器直接实现这 5 个
意图端口即可。

| 端口 | 类别 | 方法 | port 级消费者 |
|---|---|---|---|
| `RecordReceiverTransferPort` | Command | `upsert_pending_transfer`、`link_transfer_to_entry` | `FileTransferFacade` |
| `GetEntryTransferSummaryPort` | Query | `get_entry_transfer_summary` | clipboard_history 投影 |
| `FindEntryIdForTransferPort` | Query | `get_entry_id_for_transfer` | host_event publisher |
| `ListExpiredInflightTransfersPort` | Query | `list_expired_inflight` | file-transfer lifecycle（超时清扫） |
| `FailInflightTransfersPort` | Command | `mark_failed`、`bulk_fail_inflight` | file-transfer lifecycle |

要点：

- **查询与命令分离**（§3.1/§3.2）：3 个查询端口 + 2 个命令端口。
- **`RecordReceiverTransferPort` 捆绑 upsert+link 不违反 §4.2**：`apply_incoming`
  对 `link_transfer_to_entry` 的调用走的是 `FileTransferFacade` 方法（入参为
  `LinkTransferToEntry` 结构体），并非直连端口；故 port 级唯一直接消费者是
  facade，它两个方法都真用。
- **`FailInflightTransfersPort` 捆绑 mark_failed+bulk_fail_inflight**：二者是
  同一状态迁移（把 in-flight 终结为 `Failed`）的两种访问方式（逐行 vs 批量），
  同变化方向、同消费者（lifecycle），满足 §4.1 的合并条款。
- **命名合规**（§6.1）：原文件 `file_transfer_repository.rs` 改名为
  `file_transfer.rs`，去掉被禁止的 `repository` 上层含义。
- **错误类型天生合规**：新端口返回领域错误 `FileTransferProjectionError`，
  取代原 `anyhow::Result`（消费者仅 `warn!` / 映射，不 match 具体变体，迁移成本低）。

### 2.3 适配器形态

保留单个 `DieselFileTransferRepository` struct，实现全部 5 个 trait；`assembly.rs`
建一个实例后 clone `Arc` 分发到 5 个 `Arc<dyn XxxPort>` 槽位——SQL 仍集中一处。

## 3. 考虑过的备选

- **档位 0（只删死方法 + 改名）**：剩一个 7 方法端口，仍违反 §4.1。被否：
  没解决核心问题。
- **档位 2（最严字面，一方法一端口，~7 个）**：忽略 §4.1 自身的合并条款，
  类型与 `assembly.rs`（已知复杂度热点）wiring 膨胀最大。被否：locality 反而更差。
- **保留 String id**：`transfer_id` / `entry_id` 当前为裸 `String`（注释称「避免
  跨 crate 耦合 uc_ids」，但本端口与 uc_ids 同在 uc-core，理由不成立）。改为
  `TransferId` / `EntryId` 会波及 DTO 序列化边界，属另一意图，**留作 follow-up**，
  不并入本次变更。

## 4. 后果

- **正面**：每个 use case 仅持有所需能力面，测试 fake 表面骤减；查询/命令清晰
  分离；接口表面减半；命名与返回类型合规。
- **代价**：一次性触及 uc-core / uc-infra / uc-application / uc-bootstrap 四层；
  `assembly.rs` deps 字段从 1 个膨胀为按需分发的若干 `Arc<dyn>`（热点区，按最小
  安全编辑推进）。
- **遗留 follow-up**：String → 领域 id（`TransferId`/`EntryId`）单独立项。

## 5. 实施切片

1. `refactor(file-transfer)`：删 7 个死方法 + Noop stub（core + infra prune，原子，零行为变化）
2. `feat(uc-core)`：新增 5 意图端口 + `FileTransferProjectionError`（加法，旧 trait 暂存）
3. `feat(uc-infra)`：在 `DieselFileTransferRepository` 上实现 5 个新 trait
4. `refactor`：消费者迁移到窄端口 + 更新 deps/assembly wiring + 删除旧 trait（收口，消除 old/new 并存）
