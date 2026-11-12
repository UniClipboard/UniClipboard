# 本机历史契约 v0.1

- 状态：已实现并有测试：Engine 历史 API（查询/分页、内容租约、更新/删除、过期与配额、接收列表映射、历史应用），桌面 `/v1/history` 与 `unic history`；SwiftUI 客户端路线已取消，桌面界面采用 mygo + 自定义 UI Kit，原型待定。
- 日期：2026-11-07。
- 依据：[0004](../decisions/proposed/0004-clipboard-pipeline-lua.md)、[Engine 契约](engine-contract.md)、[插件契约](plugin-contract.md)。本文是 Engine 公共契约的历史扩展，公开包仍为 `github.com/uniclipboard/unic/engine`。

## 1. 所有权与记录单位

历史、查询、过期、淘汰和使用流程由 Engine 管理，业务实现放在 `engine/`；SQLite 和文件操作可复用基础设施。宿主不直接读写历史数据库，不自行维护第二份历史或搜索索引。

一条历史对应一次通过处理流水线的 commit，保存处理后的最终内容；多个同步后端只建立独立派发，不重复写历史。`persist=true` 才入库；相同内容的不同 commit 具有不同 HistoryID，可共享内部 blob，但不能按 hash 合并用户记录。观察基线、未变化、清空、应用回声和同一请求的幂等重试不产生新记录。标签和过期仅本机生效。

历史内容不可变；提交后只能修改标签和过期时间。修改正文必须作为新的显式提交走处理流水线，不能原地修改已提交内容。删除或到期不清空系统剪贴板，不撤回已经发送的字节，不删除用户源文件。

## 2. 标识、描述与 API

HistoryID 为不透明 string 类型，至少 128 位随机性，不含正文、hash、路径或设备地址，在同一 DataDir 内跨重启稳定、永不复用；不属于实例前缀 ID。其他 DataDir 的 HistoryID 与未知/已删除 ID 一样返回 not_found，不提供跨库探测。ContentID、LeaseID、ReceiptID、OperationID、ApplyToken 仍仅当前实例有效。

目标公开签名如下，类型在 engine 定义：

具体 DTO 字段类型、可选更新值及错误分类见[SDK 扩展类型契约](sdk-extension-contract.md)。

```go
func (e *Engine) ListHistory(ctx context.Context, req ListHistoryRequest) (HistoryPage, error)
func (e *Engine) GetItem(ctx context.Context, id HistoryID) (HistoryItem, error)
func (e *Engine) RetainHistoryContent(ctx context.Context, id HistoryID) (ContentHandle, error)
func (e *Engine) UpdateItem(ctx context.Context, req UpdateItemRequest) (HistoryItem, error)
func (e *Engine) DeleteItem(ctx context.Context, req DeleteItemRequest) error
func (e *Engine) BeginApplyHistory(ctx context.Context, req ApplyHistoryRequest) (ApplyTicket, error)
```

`HistoryItem` 是无正文的元数据快照：

| 字段 | 语义 |
|---|---|
| ID | HistoryID |
| Revision | 持久化 uint64，创建为 1；每次有效元数据或同步摘要变更递增，CAS 使用此值 |
| Kind | `directory_reference` for deferred selections; `directory` for immutable trees; otherwise files take priority over PNG/image and text. |
| Origin | Kind 为 local_system/remote_device/explicit/action/external/migration；DeviceID、PluginID 仅对应来源有值（external 来源的 PluginID 是后端实例，没有 DeviceID）；migration 不关联设备，不含秘密地址 |
| SourceApp | 可空的不透明应用标识，不包含窗口标题、路径或自由文本摘要 |
| CreatedAtMs / UpdatedAtMs | 本机 commit 时间 / 最后一次有效变更时间；迁移例外保留旧库创建/活动时间，普通网络接收不采用远端时间 |
| ExpiresAtMs | 0 为永不过期，否则为绝对 Unix 毫秒时间 |
| Tags | 最多 1000 个，每个最多 64 个 Unicode 字符，须为有效 UTF-8，去重、按 UTF-8 字节顺序返回 |
| Parts | 只含 Kind、MIME、Size；不含文本预览、文件名、路径、PartID 或实例内 ContentID |
| TotalBytes | 所有 part 的逻辑字节总和 |
| SyncState | not_requested/pending/sent/partial/failed/interrupted；各后端汇总，不代表远端收到或应用 |
| BackendSyncStates | 按 BackendID 保存本条内容的策略跳过、传输与中断摘要，不含秘密目标 URL/token；跨重启保留 |

文件名与具体 PartID 在显式 RetainHistoryContent 返回的 ContentDescriptor 获取。Ordinary State, metadata lists, events and errors contain no body. Explicit SearchHistory hits may contain only the bounded previews defined by the [search contract](history-search-contract.md).HistoryItem 只描述持久记录，不返回租约；所有返回值遵守 Engine 的快照隔离约定。

多后端摘要按[0007](../decisions/proposed/0007-multiple-sync-backends.md) 记录，因阈值/类型跳过不得计为成功；存在失败或策略跳过而另有成功时整体为 partial；无实际任务且只有策略跳过为 not_requested。同步不请求时为 not_requested（含远端首版不转发）；已建发送操作为 pending，终态按目标结果汇总为 sent/partial/failed，全部 superseded 归 failed。重启发现 pending 改为 interrupted，不恢复或重发。OperationID 不作为持久可查询凭证；历史摘要保留但操作仍按实例保留期回收。

## 3. 基础查询与分页

`ListHistoryRequest{Limit int, Cursor string, Filter HistoryFilter}`；Limit=0 使用 50，合法范围 1..200。`HistoryFilter{Kinds []HistoryKind, Tags []string, OriginKinds []HistoryOriginKind, DeviceID DeviceID, CreatedFromMs int64, CreatedBeforeMs int64}`：各字段之间 AND，同字段 Kinds/OriginKinds 内 OR，Tags 为全部匹配，时间为 `[from, before)`，0 表示不设该边界。DeviceID 只匹配 remote_device 来源；OriginKinds 可含 external。未知枚举、非法时间区间或超出标签限制返回 invalid_argument；重复筛选项规范化去重。

The metadata-list API does not search content or return total counts. Explicit content search, file-name matching, bounded previews, candidate catalogues and batch counts use the separate [history search contract](history-search-contract.md). Hosts must not download all content to implement search.

`HistoryPage{Items []HistoryItem, NextCursor string, HistoryRevision uint64}`。顺序使用私有 `(order_ms, seq)` 降序；seq 是事务内递增且持久化的提交序号。普通提交的 order_ms 为当前创建时间与已有最大排序时间的较大值，迁移为旧创建时间，以便与既有历史合并。两字段持久化后不变，相同毫秒和系统时间回拨不会造成重复/漏页。序号不是公开 ID。第一页固定 seq 上界，后续页使用复合 keyset 游标，不使用 offset，不保留长事务。

游标绑定 DataDir、当前 InstanceID、规范化 Filter、第一页上界和上页末尾序号。后续请求须带相同 Filter，Limit 可变。畸形或篡改游标报 invalid_argument，变更 Filter 报 conflict，跨实例游标报 resource_expired。游标不得携带正文、路径或秘密；必须校验来源与完整性。

分页是有上界的实时遍历，不是数据库快照：新 commit 不进入当前遍历；删除/到期条目跳过；标签更新可使尚未遍历的旧记录加入或退出结果，已越过的记录不回补，元数据取每次查询时快照。同一遍历不会重复返回记录。NextCursor 为空表示该查询时没有后续匹配项，不保证未来不再变化。需要最新完整视图时从第一页重新查询。

HistoryRevision 是数据库内持久化的历史集合修订号，每次历史可见变更递增；页面内条目和该修订号在同一读事务取得。它用于检测变更，不用于跨页锁定快照，不与实例 State.Revision 混用。不提供总数以免暗示多页快照一致性。

## 4. 内容读取、更新与删除

GetItem 不分配内容租约。RetainHistoryContent 在单一提交点检查记录存在且未到期，再返回当前实例的 ContentHandle；多个调用各自取得新租约，实例内可复用 ContentID。后续使用已有 ReadContent/LocalFilePath/ReleaseContent。历史持有引用与外部租约分开，ReleaseContent 不删除历史。

`UpdateItemRequest{ID HistoryID, ExpectedRevision uint64, Tags *[]string, ExpiresAtMs *int64}`：两个可选字段至少给一个；非 nil Tags 替换全部标签（空数组清空）；ExpiresAtMs 为 0 清除期限，为正则必须晚于提交时本机当前时间，负数无效。插件的 SetExpire(duration) 仍从原 commit 起算，管理 API 使用绝对时间。全部字段校验成功且 Revision 一致才原子更新；相同值为成功无变化，不递增修订、不发事件、不改变排序。

`DeleteItemRequest{ID HistoryID, ExpectedRevision uint64}`。更新和删除都要求非零 ExpectedRevision；存在但修订不符报 conflict，未知/已删除/已到期报 not_found，不保留永久墓碑。删除不是幂等成功接口；发生超时可重新 GetItem 判断，不承诺跨重启请求去重。context 取消不撤销已提交变更。

删除、过期和容量淘汰在事务提交点立即使条目对 List/Get/Retain/Update/BeginApplyHistory 不可见，并释放历史引用；已取得的租约、读取、发送和应用继续持有内容直至结束，不能因删除而使其失效。活动应用允许删除其历史元数据，应用票据依赖独立内容引用；这不同于锁定的 Received 条目不能 DismissReceived。所有剩余引用释放后才删除物理 blob；Close 中止实例读取和工作，但保留仍属有效历史的资源。

## 5. 历史、接收列表与使用

Received 是实例内、有 TTL 的待应用视图，History 是跨重启的本机记录。接收 commit 可同时建立二者；`ReceivedContent` 新增可空 `HistoryID`，persist=false 时为空。二者独立持有内容引用：DismissReceived/ReceivedTTL/应用后移出列表不删除历史；删除历史不自动 dismiss、撤销或失效已接收内容。历史链接可能返回 not_found，宿主必须容忍。重启不重建 Received，也不自动应用历史。

`ApplyHistoryRequest{HistoryID, ExpectedEpoch, ExpectedSequence}` 经 BeginApplyHistory 取得现有 ApplyTicket，共享 BeginApply 的唯一槽位、宿主能力校验、观察版本竞争和 ReportApplyResult 协议。仅 running/active 可调用；不要求联网、同步启用或仍信任原设备。历史已本地化，撤销配对不删除它。停止/挂起/应用能力撤销会使票据失效；仅同步暂停不使历史应用票据失效。

历史使用写入的是已提交最终内容，不重新运行处理器、不产生新历史、不自动同步；回读用同一 token 建立基线，已知回声不触发捕获。成功报告产生只读 clipboard.pasted 通知，语义仅为宿主成功使用，不能断言其他应用已经粘贴。若 UI 要“再次发送”，显式 RetainHistoryContent 后 SubmitContent，按新的提交运行处理器并可生成新历史，原条目保持不变。

Action（插件主动变换）的结果是 origin=action 的新历史条目，由[Action 契约](clipboard-action-contract.md)定义；宿主随后对它调用 BeginApplyHistory，与普通历史使用一致，不凭时间接近判断回声。

## 6. 存储、配额与过期

SQLite 存元数据、小正文和 blob 引用；大正文/文件存 `DataDir/history/` 的 Engine 私有目录，不能使用会在启动/关闭时清空的临时接收目录。每个内存表示不大于 256 KiB 时可内联，超过时必须外置；引擎自有的文件（Reader 导入、远端接收）作为自有 blob。目录与引用只在内部可见。

**宿主就地保存的文件不复制进历史**（2026-11-07 决定，取代早先“一律复制为自有快照”的设计）：来源实现可选接口 `PersistentFileSource{PersistentRef() string}` 时，历史只持久化该不透明引用（桌面为绝对路径）以及 commit 时的 Size 与 Version，不拷贝字节、不占历史配额。引用与路径一样是秘密，仅存数据库，不进 DTO、事件、日志或错误。重启后 `RetainHistoryContent` 通过 `Options.ResolveFileRef(ref)` 重建来源；读取时按原规则比对 Size/Version，文件被移动、删除或修改报 source_changed，未提供 resolver 同样如此。这类条目的文件内容因此不保证长期可读，元数据仍保留。不实现 PersistentFileSource 的来源（如无法稳定寻址的移动端 URI）仍回退为复制 blob。历史条数/字节配额只统计历史自己存储的内联数据与 blob，就地引用的文件按 0 计，HistoryItem.TotalBytes 仍报告逻辑大小。

### Unified cleanup and storage budgets

`Options.History.MaxBytes` is a startup storage budget. Zero selects 4 GiB, capped by
`ResourcePolicy.MaxStoredBytes`; negative values are invalid.
`HistoryOptions.MaxItems` and `DefaultTTL` are removed. Count and age are live policy,
not per-record expiry defaults.

`Settings.History` contains `HistoryCleanupSettings`:

| Field | Default | Rule |
|---|---|---|
| AutoCleanup | true | One switch for age/count cleanup and capacity-driven history deletion. |
| RetentionDays | 180 | 0..36500; 0 disables age cleanup. Age uses stored CreatedAtMs. |
| MaxItems | 0 | 0..1000000; 0 means unlimited eligible records. |
| ExcludedTags | [] | Existing tag validation; exact case-sensitive OR matching. |

Excluded records do not count toward MaxItems. Empty ExcludedTags excludes nothing.
Tag updates, policy updates, candidate selection, and deletion serialize under the history mutex.
Cleanup processes oldest creation time first, then commit sequence. Age and count share this order.
It runs at startup (after the host reconciles clipboard files), on policy changes, after commits,
and every 30 seconds while running. Each pass deletes at most 256 records; large backlogs drain in later passes.
A new record is not evicted by its own commit's count pass.
Reading and restoring do not reset record age. Policy changes apply to existing records.

Manual deletion and explicit user/processor deadlines remain independent of AutoCleanup and ExcludedTags.
The existing read-time expiry check still applies only to those explicit deadlines.
Automatic age is a maintenance policy, not an ExpiresAtMs value.

All automatic storage reclamation respects AutoCleanup and ExcludedTags.
History byte budgets count inline/blob data; persistent host references remain zero-cost.
The default file-cache quota counts each retained payload and pending reservation once.
If cleanup is off, protected history fills storage, or active holders prevent enough reclamation,
reject admission with resource exhaustion. Do not bypass protection or claim held bytes are freed.
A single item larger than the history byte budget is rejected without deleting old history.

Deleting managed-file history releases every managed-file reference in that record.
There is no independent file-age worker, file-cleanup switch, or payload-only quota eviction.
Cache capacity reclamation selects eligible history with an unheld payload whose other history references are also eligible, and deletes the record.
Shared files survive while any history reference or content/clipboard holder remains.
Records containing multiple managed files are deleted as a whole.
Logical deletion and history revision commit before physical reclamation.
Owned orphan payloads remain charged in the cache ledger until physical deletion succeeds;
release, maintenance, and restart retry reclamation. Never unlink host-managed source/custom files.

commit 须先完成自有 blob 暂存与持久化，再在数据库事务内发布内容引用、历史、淘汰和修订号；数据库提交成功后才发布通知与网络派发。事务失败不删除既有历史，已落盘但未被数据库引用的孤儿 blob 留待安全清理。提交成功后崩溃不会自动恢复待发送工作。不得宣称跨系统剪贴板、数据库与网络原子提交。

接收 commit 还须预留 Received 槽位并在同一事务写成功去重记录；persist=false 接收也必须完成去重事务。满表无法预留则不发布历史或成功去重记录。提交屏障、实例状态发布及所有引用的交接遵循[流水线契约 §7](pipeline-contract.md#7-状态转换与资源交接)，不以历史已经落盘代替接收提交成功。

到期定义为当前本机墙钟 >= ExpiresAtMs；跨重启保存绝对时间，进程内定时器只负责唤醒，唤醒后重新比较墙钟。回拨可延后尚未删除条目到期，前拨可提前触发，已删除不会复活。New 返回前清理到期元数据与仅由 Engine 拥有的孤儿资源，运行时每 30 秒扫描且查询/读取/变更时再次判定，禁止在扫描间隙返回已到期条目。物理清理失败可重试，不恢复逻辑记录；错误必须脱敏。

## 7. commit、生命周期与事件恢复

本机有效变化和 explicit 提交先处理/commit，再决定是否发送；同步关闭、无在线目标、插件禁发或无后端不应阻止 persist=true 写历史。baseline/empty/unchanged/echo 仍不入历史。远端先通过身份、同步过滤和资源准入，再进入 received/commit；persist=true 入库失败则不建立可应用条目。

此约定扩展 Engine 契约中旧 Submit/Observe 的发送语义：[流水线提交契约](pipeline-contract.md) 已确定新增 CommitContent/GetSubmission、失败结果幂等缓存及观察结果扩展，原 SubmitContent 签名保留为兼容门面。旧 no_targets/sync_disabled/unsupported 返回值仅说明未安排发送，不能推断历史没有入库；新宿主使用结构化提交结果，避免重试重复 commit。

历史 List/Get/Retain 在 created/running/stopped 可用，stopping 仅允许 List/Get 和已有内容读取/释放；closed 返回 closed。Update/Delete 在 created/running 可用，包括 running/suspended；stopped/stopping 返回 invalid_state。Stop 后不再产生定时历史变更，查询仍按当前时间隐藏到期项，物理清理留给下次 New；Close 不删除有效持久历史。BeginApplyHistory 的运行条件见第 5 节。

State 新增 `HistoryRevision`，不嵌入全量历史。历史增删改在持久事务内更新 HistoryRevision，随后更新实例 State.Revision 并发 `history` 事件（Objects 类型 history，ID 为 HistoryID）。流水线另发 committed/updated/deleted/expired 通知，容量淘汰按 deleted；这些事件只含 ID 和稳定原因，不带正文、路径、文件名或原始插件错误。

历史读取/变更与事件注册须统一序列化可见提交点，不允许看到新数据库记录却取得旧 State.HistoryRevision。订阅初始快照含 HistoryRevision，沿用 resync_required；消费者先订阅，再分页加载并消费变更，加载期 HistoryRevision 改变可重载。分页不是快照，持续写入时不保证有限重试可得到全量静态副本，UI 可用历史事件提示刷新。删除事件后 GetItem 返回 not_found 是正常情况。

重启后持久 HistoryRevision 保留，InstanceID/State.Revision 重建，不重放旧历史事件。启动清理产生的结果包含在首个快照中；无持久事件回放或可靠投递承诺。

## 8. 必须补充的契约测试（待实现）

- 稳定 ID 重启可用，旧内容租约/游标失效；不同库和已删除 ID 返回 not_found。
- 同毫秒/时间回拨分页不重复，新插入不进入旧遍历，筛选变更与游标篡改被拒绝，删除/更新遵循实时遍历语义。
- List/Get 无正文、文件名/路径/实例内容句柄；显式租约读取返回准确正文与文件名；就地引用的文件读取时校验 Size/Version。
- 相同内容独立记录、共享计费不重复；观察/请求幂等不重复 commit，关闭同步/无目标仍入历史。
- CAS 更新/删除竞争、过期竞争、清除期限、无变化更新；正文不可原地改写。
- 删除/过期与读取、租约、发送、应用交错，现有持有者可完成，新请求不可取得已删除历史。
- Received dismiss 不删历史，历史删除不撤销 Received；重启不重建待应用列表。
- 历史应用复用唯一槽位，离线/同步暂停可用，新复制导致确认冲突，回声不入历史、不转发。
- 引用文件持久化前变化、配额预留失败、磁盘满、事务回滚、崩溃后孤儿清理，不能误删源文件或有效历史。
- 条数/字节淘汰、单条超限无破坏、活动引用不虚报可回收空间，Stop/Close 保留持久资源。
- 到期读时不可见、时钟变化、重启清理；history 修订与订阅快照无窗口，溢出可重新查询。
- pending 在重启后变 interrupted，无隐式重发；日志/事件/错误不含正文、地址、票据和路径。


## 离线归档导入（0011，已实现）

`Options.MigrationMode=true` 的 Engine 可以在 `New` 后调用 `ImportHistory`，但不能 `Start`。目标目录沿用全局独占锁。普通会话调用导入返回 `invalid_state`。

`HistoryImportRequest` 包含不透明 `Source`、`RecordID`、`CreatedAtMs`、`UpdatedAtMs`、`Tags` 与 `ContentInput`。来源标识和记录 ID 各为 1..256 字节，时间非负；内容要求与本机支持的标准格式一致，文件必须提供 Reader，不接受外部 Source 引用。内容写入目标自有存储，文件在发布数据库引用前同步落盘；条目、标签关联、来源映射同事务提交。失败不发布映射，可重试。导入不走普通处理、发送、系统剪贴板与容量淘汰流程。

`ImportHistory` 返回 `HistoryImportResult{HistoryID, AlreadyImported}`。`LookupHistoryImport` 查询持久映射；相同来源和记录再次导入返回原 ID，不以正文去重。目标条目被主动删除后仍保留映射，重跑不会复活记录；源后续变更不覆盖已导入内容与标签。`ImportHistoryTags` 在迁移会话中保存无关联名称，`ListHistoryTags` 返回定义与条目关联的并集。

容量下限、旧格式解锁、目录 ZIP 与完成校验见[0011](../decisions/proposed/0011-v1-history-migration.md)。旧密码与旧源目录均不进入 SDK 请求。

存储共享预算压力触发的自有历史淘汰，在删除提交成功后与配额淘汰一样发布每条 ID 的 `clipboard.deleted` 只读通知；删除失败不发。通知仍遵守 observer 有界队列与尽力交付语义。

## Received-file storage classes and restore sends (0016)

Default-cache files use internal `cache:` references in storage. The references never appear in ordinary history responses.
`HistoryPartSummary.Available` and desktop `parts[].available` expose default-cache content availability.
Unified cleanup deletes eligible history metadata and releases its default-cache payloads.
No independent cache policy removes a payload from a retained history record.
Missing or externally altered payloads can still make reads fail with `resource_expired`.
Active holders keep payloads alive. The file cache counts a payload once even when history and Received share it.
Custom received files use persistent external references supplied by `Options.StoreReceivedFile`.
`Options.ResolveFileRef` rebuilds their `FileSource` after restart.
Engine stores and returns the reference unchanged and never treats the reference as a filesystem path.
Hosts obtain a checked reference with `FileReference`; `LocalFilePath` does not expose host references as paths.
History deletion, expiry, and content release must not unlink host-managed files.
Imported archive payloads remain under their existing history ownership rules.
A generic history apply stays local. The desktop explicit restore can resend after successful apply through `SyncRestoredHistory`.
Restore resend respects the stored processor sync prohibition and does not rerun processors or create a new record.
See [desktop runtime](../architecture/desktop-runtime.md).

## Local headless host

The [local service contract](local-service-contract.md) defines explicit submissions, transient Received reads,
independent Action application and exclusive desktop adapter generations.
A missing clipboard adapter does not restrict Engine import, commit, receive, history reads or export.
Service restart invalidates local sessions and content leases.


## History source analytics (0020)

Successful explicit application uses the selected record's stored origin for the
finite `history_applied.source_kind` property. Local system and explicit origins map
to `local`; remote_device maps to `remote`; external, action and migration retain their
categories. Unknown origins map to `unknown`. The application ticket retains this
classification, so deleting the record after ticket creation does not change it.
No source-device identity or history identifier enters analytics. This does not
observe another application's paste or create an automatic-application event.
See the [observability contract](observability-contract.md#111-product-value-and-daily-admission).

## Directory extension

Directory selection references, preparation, manifests, file-policy grouping, bounds, and acceptance follow the [directory content contract](directory-contract.md). The 0024 execution receipt distinguishes specified behavior from verified implementation.
