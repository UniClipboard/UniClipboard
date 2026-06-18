# UniClipboard 领域术语表（CONTEXT.md）

本文件记录 UniClipboard 跨 crate 复用、且对领域专家有意义的统一语言（Ubiquitous
Language）。只收录本项目语境特有的概念，不收通用编程概念。随设计讨论惰性增长。

> 约定：术语名用英文（与代码标识符一致），定义用中文且尽量一句话——说清它**是
> 什么**，而非它**做什么**。

## Language — 文件传输（接收侧）

**Tracked inbound file transfer**：
接收设备本地为「一个正在/已经收下的文件」维护的一条投影记录（id、来源设备、
缓存路径、状态、时间戳）。
_Avoid_: download、file record

**Receiver-side file transfer projection**：
接收侧把传输生命周期落到本地的投影表，与 domain event 总线解耦——它是接收方的
本地上下文，不是同步的真相源。
_Avoid_: transfer DB、file table

**In-flight transfer**：
尚未终结的传输，即状态为 `Pending`（已收元数据、等数据）或 `Transferring`
（已收首块、传输中）。`Completed` / `Failed` / `Cancelled` 均 **不** 属于 in-flight。
_Avoid_: active transfer、ongoing

**Entry transfer summary**：
把一个剪贴板 entry 名下所有 transfer 的状态聚合成一个对外状态的视图，聚合优先级
为 `Failed > Transferring > Pending > Cancelled > Completed`。
_Avoid_: transfer status、entry status

**Timeout sweep**：
周期任务，找出超过时限的 in-flight transfer 并逐条终结（transferring 行需先拆掉
iroh-blobs 抓取与 QUIC 连接，再标记失败）。
_Avoid_: cleanup job、GC

**Startup reconcile**：
进程启动时的一次性重整，把「上次运行残留的孤儿 in-flight transfer」批量标记失败
并清理缓存。
_Avoid_: recovery、startup cleanup

## Relationships — 关系

- 一个剪贴板 **Entry** 拥有零或多个 **Tracked inbound file transfer**
- 多个 transfer 的状态聚合成该 Entry 的一个 **Entry transfer summary**
- **Timeout sweep** 与 **Startup reconcile** 都把 **In-flight transfer** 终结为
  `Failed`，区别只是触发时机（周期 vs 启动）与粒度（逐行 vs 批量）

## Example dialogue — 示例对话

> **Dev**：mobile `PUT /file` 进来时，是不是马上就有真实 entry_id？
> **领域专家**：没有。先用占位 id seed 一条 **Tracked inbound file transfer**，
> 等 SyncDoc apply 阶段生成真实 entry 后再 relink 过去。所以这条投影行的
> entry_id 是会被改写的——这正是 `RecordReceiverTransferPort` 要 relink 的原因。

> **Dev**：那「接收进度百分比」算不算领域概念？
> **领域专家**：目前不算。我们只跟 **In-flight transfer** 的状态枚举，不跟逐块
> 进度——历史上预留过逐块投影方法，但从未接线，已在 ADR-009 删除。

## Flagged ambiguities — 已澄清的歧义

- `mark_completed` / `mark_failed` 曾在两个无关 trait 上同名（文件传输投影 vs
  `RepresentationCachePort` 打在 `rep_id` 上）——已确认是两个不同概念，文件传输投影
  侧的 `mark_completed` 实为死代码，ADR-009 已删。
- 「receiver-side 分块进度投影」一族方法（`mark_transferring` / `refresh_activity`
  / `backfill_announce_metadata` 等）曾被误认为是活功能——已澄清为未接线的预留面，
  ADR-009 删除；将来若需进度功能须按 `uc-core/AGENTS.md §2.3` 另立新意图端口。
