---
paths:
  - engine/pipeline.go
  - engine/submission.go
  - engine/backend.go
  - engine/receive.go
  - engine/clipboard.go
  - engine/action.go
  - engine/history.go
  - engine/history_sync.go
  - engine/history_types.go
  - engine/history_import.go
  - engine/plugins.go
  - engine/observers.go
  - plugin/plugin.go
  - internal/store/history.go
  - internal/plugins/builtintext
verified: a19152214865e4393e781b627244e39b3e6c1c2a
---
# 剪贴板处理流水线与本机历史

内容从捕获/接收到入历史、同步、使用的内部流程，以及 Engine 管理的本机历史与用户触发的 Action。

## 现状

阶段链：Capture/Receive → Processing → Commit → Sync → Use → End。

- **入口**：自动捕获（`ObserveClipboard`，请求键 `obs:<实例>:<epoch>:<序号>`，origin=`local_system`；基线、回声、未变化、空内容的观察只返回处置，不进流水线）、显式提交（`CommitContent`/旧门面 `SubmitContent`，origin=`explicit`）、Action（`RunAction`，origin=`action`）共用一个提交器（`startSubmissionLocked` → `runSubmission`）；远端接收（`delivered`，origin=`remote_device` 或 `external`）走自己的路径，不建 submission 记录。
- **Processing**：已注册的 processor 串行执行，顺序为内置/Go 插件按装配顺序（`Options.Plugins`），之后是 `[plugins].lua` 列表顺序的 Lua 插件。本机/显式/Action 结果触发 `clipboard.captured`，接收触发 `clipboard.received`。processor 只能通过 `processingContext` 暂存标签（≤32 个、每个 ≤64 字节）、`persist`、`sync`、过期时长，**不能改正文**。每次调用的 200ms 预算含等待实例闸；成功才整体采纳，出错、超时、等闸超时、panic 都丢弃该次改动并继续；任一方法调用返回错误（非法标签/超限）即使被插件忽略也使整次改动作废。`on_error=block_sync` 的 processor 失败（含等闸超时）则强制 `sync=false`，后续 processor 不能解除。每个 processor 实例有独立串行闸，跨入口共用。默认 `persist=true, sync=true`。
- **Commit**：`persist=true` 时 `persistHistory` 先检查单条大小与共享存储预算（必要时淘汰最旧的自有历史），再在一个 SQLite 事务内写记录（及接收去重记录），事务后另行按条数/字节淘汰最旧（不淘汰刚写入者）；失败则整次提交 failed，不发送。`persist=false` 不入历史但仍可同步（接收则仍写去重记录并建立 Received）。提交结果为无正文的 `SubmissionResult`（Phase processing/committed/failed，HistoryID，同步处置，各后端结果），按 RequestID 幂等缓存，`GetSubmission` 查询。提交成功后才派发同步，并发 `clipboard.committed` 通知。
- **Sync**：`dispatchProcessed` 先查全局 `sync.enabled` 与条目 `sync`，再对每个选定后端独立做类型过滤、自动文件阈值（仅 origin=explicit 不受阈值约束）与限额检查，调用后端 `Submit`（后端按 ID 排序）；所有有任务的后端合并为一个 Operation。仅当有后端 queued/failed 时才按后端记录同步摘要（`clip_item_sync`）；重启后 pending 变为 interrupted，不续发。后端本身见各后端页面/契约。
- **接收**：认证、策略、去重占位与配额准入通过后，对经后端接收策略过滤后剩余的内容跑 `received` processor（过滤后为空则静默丢弃，不写去重）；`sync` 对接收无效（不转发）。先预留 Received 槽位，再写历史与成功去重记录，最后在提交点复查生命周期/同步开关/信任，失败则撤回历史与去重记录。
- **Use**：`BeginApply`（Received）与 `BeginApplyHistory`（历史）应用不重跑 processor、不新建历史、不同步。二者共用唯一应用槽、宿主 Apply 能力检查与 `ReportApplyResult`；`BeginApplyHistory` 不要求同步开启或来源设备受信。仅经 `BeginApplyHistory` 应用成功才发 `clipboard.pasted`（载荷为 HistoryID），`BeginApply` 不发。剪贴板内容不会被 processor 改写或自动写回。
- **Action**：插件静态声明（ID/Title/Accepts，当前仅文本、无参数）；动作键为 `pluginID/actionID`，结果必须是非空 UTF-8 且恰好一次 `SetText`。`ListActions`/`RunAction` 由用户触发，对一条文本历史产生**新的** origin=`action` 条目（强制 persist，仍经 captured processor 标注和正常同步决策），来源条目不变，10s 超时只覆盖等闸与动作本身，同一 RequestID 命名空间幂等（与 `CommitContent` 共用，参数不同返回 conflict）；插件动作失败对外只报不透明的 `internal`。宿主随后对新条目调用 `BeginApplyHistory`。`builtin.text` 提供 upper/lower/trim/plain，并在 processor 阶段给文本打 `email`/`url` 标签。桌面 API：`GET /v1/actions`、`POST /v1/history/{id}/actions/{key}`。
- **历史**：由 Engine 持有，SQLite 表 `clip_items`/`clip_item_parts`/`clip_tags`/`clip_item_sync`，≤256KiB 的内容内联，其余存 `DataDir/history/` 私有 blob；宿主就地保存的文件（`PersistentFileSource`）只存引用与版本，不复制。HistoryID 为随机不透明串，跨重启稳定。API：`ListHistory`（keyset 游标，Limit 默认 50、1..200）、`GetItem`、`RetainHistoryContent`、`UpdateItem`（仅标签/过期，`ExpectedRevision` CAS；用户标签 ≤1000 个、每个 1..64 字符，过期须为 0 或未来时间）、`DeleteItem`、`BeginApplyHistory`；`HistoryRevision` 持久化；列表游标为 HMAC 密封，绑定过滤条件与引擎实例。迁移模式下 `engine/history_import.go` 离线导入（origin=`migration`），不跑 processor、不同步。默认上限 10000 条、4GiB，超限淘汰最旧；`DefaultTTL` 默认不过期，processor 设置的过期优先；过期按绝对时间，启动清理加每 30s 扫描并在读时再判定。标签与过期不同步。桌面 `/v1/history*`，CLI `unic history`（list/show/delete/tag/expire/action）。
- **通知**：observer 只收无正文事件（committed/updated/deleted/expired/pasted/app.*，仅含 Kind/ItemID/时间），不能改内容。每个 observer 独立队列（容量 64，满则丢最新），回调 200ms 超时，不保证送达。存储压力淘汰自有历史与条数/字节配额淘汰一致，在成功删除后发 `clipboard.deleted`，每个被淘汰 ID 一次。

## 不变量

- processor 与 observer 不能改正文；改写内容只能走用户触发的 Action，且产物是新条目。
- 一个 RequestID 只 commit 一次；`obs:`、`recv:` 前缀保留给 Engine；失败也缓存，改参数用同 ID 返回 conflict。
- 网络派发只发生在 commit 成功之后；commit 前失败不发送，commit 后同步失败不回滚历史、不重跑 processor。
- 同步无后端、同步关闭或无目标时本机内容仍 commit 并入历史。
- Stop/挂起与 commit 通过 `commitGate` 排序：先生效则不得迟到 commit。
- 历史内容不可变，提交后只能改标签与过期；删除/到期立即对 List/Get/Retain/Update/Apply 不可见，已持有的租约/读取/发送/应用继续有效。
- 接收不得留下半条目：历史、去重记录、Received 槽位同成同败；persist=false 接收也必须写去重记录。
- 历史与 Received 各自持有内容引用，互不删除；历史存储配额与 `MaxStoredBytes` 共享，就地引用的文件按 0 计。
- 事件、日志、错误、SubmissionResult 不含正文、路径、文件名或原始插件错误。
- 就地引用文件被移动/改动后读取报 `source_changed`，不得复制替代或删除用户源文件。

## 代码地图

- `engine/plugins.go`：processor/Action 注册与校验、`on_error` 策略。
- `engine/pipeline.go`：processor、`processingContext`、`processContent`（暂存/回滚/超时）。
- `engine/submission.go`：提交器、幂等缓存、`CommitContent`/`GetSubmission`。
- `engine/backend.go`：`dispatchProcessed`、多后端结果聚合。
- `engine/clipboard.go`：观察协议、`BeginApply`、`ReportApplyResult`。
- `engine/receive.go`：接收入口 `delivered`、Received 列表与槽位。
- `engine/action.go`：Action 注册、`RunAction`。
- `engine/history.go`、`history_types.go`、`history_sync.go`：历史 API、blob、淘汰/过期、同步摘要。
- `engine/observers.go`：只读通知分发。
- `internal/store/history.go`：历史与去重表结构及查询。
- `internal/plugins/builtintext`：内置 processor + Action 插件。
- `app/desktop/internal/api/api.go`、`app/desktop/cmd/unic/history.go`：桌面 HTTP 与 CLI 适配。

## 规格与背景

- 契约：[pipeline-contract](../contracts/pipeline-contract.md)、[history-contract](../contracts/history-contract.md)、[clipboard-action-contract](../contracts/clipboard-action-contract.md)、[engine-contract](../contracts/engine-contract.md)、[sdk-extension-contract](../contracts/sdk-extension-contract.md)
- 插件注册与配置：[plugin-contract](../contracts/plugin-contract.md)；同步后端：[sync-backend-contract](../contracts/sync-backend-contract.md)

## 缺口

- 插件 GitHub 安装（`unic plugin install`）未实现。
- 真机双设备基线实测未做。
- macOS SwiftUI 未接入历史与 Action 菜单。
- `CommitRequest.WritebackGuard` 非空返回 `unsupported`（自动写回已取消，无计划）。
