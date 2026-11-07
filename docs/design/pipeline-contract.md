# 流水线提交契约 v0.1

- 状态：提交结果与幂等契约已确定，待实现；自动写回票据、RPC 帧与运行时资源预算仍按执行计划补齐。
- 日期：2026-11-07。
- 依据：[0004](decisions/0004-clipboard-pipeline-lua.md)、[Engine 契约](engine-contract.md)、[历史契约](history-contract.md)、[插件契约](plugin-contract.md)。本文件是 Engine 的目标扩展，不表示已有代码支持下述 API。

## 1. 将 commit 与发送结果分开

一次提交可能已完成处理和历史入库，却因策略、无目标或无后端没有发送。SDK 必须明确返回这两个结果，不能让宿主根据一个发送错误推断“没有入库”，更不能因此自动重跑处理器。

新增 API，保留原 SubmitContent 签名作为兼容门面：

```go
func (e *Engine) CommitContent(ctx context.Context, req CommitRequest) (SubmissionResult, error)
func (e *Engine) GetSubmission(ctx context.Context, id RequestID) (SubmissionResult, error)
```

`CommitRequest{RequestID, LeaseID}` 仅用于宿主显式提交，Origin 固定 explicit。自动捕获和接收使用同一内部提交器，不允许宿主伪造 remote_device、plugin 或跳过观察协议。调用者没有“跳过插件”或单独强制 persist/sync 的参数；策略由配置和处理器决定。

`SubmissionResult` 是无正文的独立快照：

| 字段 | 语义 |
|---|---|
| RequestID | 本次提交的调用标识，不包含正文或秘密 |
| Phase | processing/committed/failed；CommitContent 成功返回时必为 committed，GetSubmission 可观察其余状态 |
| ItemID | 实例内不透明处理对象 ID，接纳请求时分配；与 HistoryID、ContentID 分开 |
| HistoryID | 仅成功 persist=true 的 commit 非空，跨重启稳定 |
| SyncDisposition | not_requested/disabled/no_backend/no_targets/queued/failed；processing 或 failed 阶段为空 |
| OperationID | 仅 queued 非空，引用原有发送 Operation；后续发送进度在 GetOperation 获取 |
| SyncError | 仅 SyncDisposition=failed 非空，为稳定脱敏 Error |
| Error | 仅 Phase=failed 非空，表示 commit 前的终态错误 |

此结果不携带可直接读正文的句柄或写回票据，persist=false 也不能依靠 ItemID 重新获取已回收资源。HistoryID 可能在结果缓存保留期内已被删除/到期，再查历史返回 not_found 属于正常情况。

GetSubmission 的 RequestID 只用于当前实例，不是跨重启恢复凭证；未知或已回收返回 not_found。重启不恢复 processing、缓存错误或未决网络派发；历史按自身契约保留。

## 2. 接纳、处理与 commit 的原子边界

1. 先校验请求形状、RequestID 保留命名空间与幂等键；同键重试优先于租约和运行条件检查。
2. 新请求要求 running/active，允许无后端和同步关闭。检查原始内容租约、导入限制和有界请求表容量，取得内部内容引用并创建 processing 记录。此前失败不接纳请求，不产生缓存或处理副作用。
3. 接纳后以 Engine 工作 context 执行 captured 处理器，按顺序暂存与采纳 patch，再准备最终资源；取消调用者 context 只停止等待，不取消已接纳工作。Stop/挂起取消仍未提交的工作，结果缓存为 failed；不得迟到 commit。
4. persist=true 时，按历史契约原子提交历史、必要淘汰、历史修订与资源引用；同时将提交结果变为 committed。persist=false 时以同一可见提交边界接受最终不可变 Item/Policy，并保留操作期资源。commit 前任何终态失败都为 failed，不产生 committed 通知、历史或发送。
5. commit 时决定同步是否请求并预留必要操作/调度状态；网络派发只能在 commit 成功后开始。结果、历史与发送任务的对外可见性统一序列化，不允许查询到“同一请求未提交”却已看到发送。

处理器失败按 continue/block_sync 回滚该处理器 patch，本身不必导致 Phase=failed；仍可能成功 commit。自由文本错误、stderr、正文和源文件路径不能进入 SubmissionResult。

commit 成功后不再把网络调度失败作为 CommitContent 的 error 返回；以 SyncDisposition/SyncError 或已创建 Operation 的终态表示。调用者可在 CommitContent 等待中收到 context 超时，此时必须用原 RequestID 查询/重试，不能推断工作失败。提交前后同时发生 Stop 或配置更新时，通过同一序列化提交点确定谁先生效。

## 3. 同步决策

在 commit 可见提交点，按以下顺序确定首次同步决策，结果一旦形成就不因后来联网或策略变化自动补发：

1. 远端接收、Policy.Sync=false 或 block_sync 禁发：not_requested。
2. 全局 sync.enabled=false 或按类型过滤后无内容：disabled；插件无法突破该过滤。
3. 未配置同步后端：no_backend；配置了后端但运行期不可用：failed/unavailable。
4. 无当前已批准且连接的目标：no_targets。
5. 传输大小/授权复核或操作容量不足：failed，SyncError 为稳定错误码。
6. 其余创建发送 Operation：queued，绑定此时已批准且连接的目标，不加入之后上线的设备。

原始内容的导入/处理限制仍属于 commit 前验证；这里的大小失败指发送视图在调度时不满足当前传输限制。历史保存未经过网络类型裁剪的最终内容；只对发送视图过滤，禁止为了同步过滤而从本机历史删除未发送格式。

queued 只表示任务建立，sent 仍仅为字节写完。真正开始每个目标传输前重新校验当前授权、同步开关和限制；变化使该目标失败，不回滚 commit、不重新运行处理器。保留每设备一个在途、一个待发与 superseded 语义。通知队列、普通 observer 和日志不承担派发。

无后端的本机模式中 CommitContent 仍正常入历史；“配对/发送 unsupported”是网络门面的结果，不能用来拒绝整个本地提交。

## 4. 幂等、并发与保留

新增独立 submission 表，不能仅依赖发送 Operation 的 RequestID 索引：没有网络任务的 commit 和失败也必须去重。显式 CommitContent 和 SubmitContent 共用同一命名空间与记录，语义参数为原始 LeaseID 与 explicit 来源；改变参数使用同一键返回 conflict，不用已处理内容 hash 代替原始参数比较。

同键并发只接纳一次，其他调用等待或查询同一结果；不会并发执行两遍插件。processing 返回给 GetSubmission；CommitContent 等待终态。重复识别早于租约校验，因此调用者释放原租约后仍可取得原结果。外部文件变化也不得导致已提交请求重跑。

已接纳请求的终态成功和失败都缓存。相同键重试失败仍返回原错误，不再处理；修改参数或希望重新尝试必须使用新 RequestID。插件可能产生无法回滚的外部副作用，幂等只保证 Engine 在保留期内不重复调用，不能提供外部事务保证。

创建选项 ResourcePolicy 新增 MaxSubmissions（默认 1024，>0）与 SubmissionTTL（默认 30 分钟，>0），独立于操作表。processing 不淘汰；终态从完成时计 TTL，容量压力可提前淘汰最旧终态记录。无可淘汰槽位则接纳前 resource_exhausted。查询和重试不延长保留期，驱逐不删除历史或中止发送。

GetSubmission 结果中的 OperationID 可能先于 submission 被驱逐，GetOperation 返回 not_found 不会让 committed 失效。超出保留期后不再承诺去重，宿主不得将旧键当成永久幂等凭证，需遵守原 Engine 契约使用新 RequestID。

RequestID 禁止宿主使用 `obs:` 与 `recv:` 前缀，它们由 Engine 保留。观察键是实例/epoch/sequence，接收键是本次已完成接收的内部标识，不采用对端可控 wire ID 作为跨设备或跨重启去重保证。

## 5. 原 API 与观察协议迁移

SubmitContent 保留原 `(OperationID, error)` 签名、explicit 限制与网络返回码，通过同一提交器取得已缓存结果再映射：queued 返回 OperationID；not_requested/disabled 返回 sync_disabled；no_backend 返回 unsupported；no_targets 返回 no_targets；failed 返回 SyncError；commit 前失败返回 Error。这些网络错误可发生在历史已保存之后，宿主展示结果应迁移到 CommitContent/GetSubmission，不能自动创建新请求重试。

同一 RequestID 从 CommitContent 转到 SubmitContent 仍读取同一结果，反之亦然，门面差异不能重复 commit。旧 HTTP `/v1` 响应形状保持；如需展示历史结果，使用新增适配接口，不能往旧响应隐式混入正文或破坏其字段含义。

ObservationResult 增加可空 Submission 字段（SubmissionResult 快照），原 Disposition/OperationID 保留。baseline/empty/unchanged/echo 不建立 submission；有效变化即使无目标、同步关闭或无后端也完成处理/commit，并在结果中返回历史标识。

commit 后的观察处置映射：queued→submitted，disabled/not_requested→disabled，no_targets→no_targets，no_backend→新增 local_only，failed/limit_exceeded→too_large，其余 failed→新增 sync_failed。commit 已完成的处置均推进原始观察基线，不因网络失败重报；插件最终内容写回后的基线仍由单独 apply token 协议确认，不能直接假定已写系统剪贴板。

commit 前失败不推进基线，但已接纳失败的同一 epoch/sequence 仍命中失败缓存。需要主动重试时宿主使用新的、更大 sequence 与有效租约；不得把旧 sequence 改参数重报。尚未接纳的失败可用同一请求重试。观察重复窗口至少覆盖当前 epoch 最近一次已接纳结果（含失败），独立于普通 submission 表 TTL；新 epoch 和 Stop/挂起使其按观察协议失效。

没有 Observe 能力的显式提交仍可处理与入库；SDK 不假定宿主已写入处理后的正文。本机自动写回的票据、失败处置与来源深度仍须单独补齐后才能实现改写回填。

## 6. 接收、生命周期与资源

接收必须先通过身份/信任、同步策略与配额准入，再以收到的完整资源运行 received 处理。首版不转发；commit 后才创建 Received 视图和允许 BeginApply。persist=false 仍可接收应用，Received 独立持有资源。持久化失败不返回一个假装可用的接收条目；清理半成品并以无正文的 receive.failed 通知结束，不增加 wire ACK。

提交器从接纳到 commit/失败持有原始和暂存内容；成功后历史、Received、Operation、待写回任务分别取得自己的引用，再释放处理引用。任一失败、取消或崩溃恢复均遵循历史资源清理范围，不能删除用户源文件。submission 元数据缓存本身不长期持有正文；persist=false 且无操作/应用/租约持有后即可回收内容，结果缓存仍可查询。

GetSubmission 在 created/running/stopping/stopped 可查询；closed 返回 closed。新 CommitContent 只在 running/active 接纳，重复终态在其他未关闭生命周期也可查询返回；processing 的重试仍可等待已有工作。Stop/挂起取消未 commit 工作并写入终态失败，已 commit 的历史保持，已创建网络任务按原契约终止。Close 等工作回收后才释放数据库和目录锁。

State 不包含整张 submission 表。新增 `submissions` 事件，仅用 Objects 引用公开 RequestID，接纳与终态各产生修订；消费者用 GetSubmission 查询，事件溢出沿用 resync_required。宿主应保留自己发起的 RequestID，GetSubmission 的查询保留期是恢复范围，不提供全量任务重放。历史与 submission 同批 commit 的事件不能让消费者观察到半提交状态。

## 7. 待实现的验收用例

- 相同请求并发/跨门面/释放原租约后重试，只执行一次 processor、commit 与派发；不同参数冲突。
- persist=true/false × 同步允许/禁止 × 有目标/无目标/无后端，结果字段与所有权一致。
- commit 前校验、插件、落盘、数据库失败均不发送；已接纳失败缓存，未接纳失败可重试。
- commit 成功后操作容量不足、后端故障、限额变化，以独立同步失败表示，不回滚历史或重跑处理器。
- 调用者超时后查回同一结果；Stop/挂起与 commit 竞争、迟到 patch、并发读取结果没有半提交。
- 观察 commit 后同步失败仍推进基线；commit 前失败不推进，旧序号命中失败，新序号可主动重试。
- submission/operation 独立驱逐、processing 不淘汰、满表接纳前失败；缓存不泄露或无限持有正文。
- 远端不转发，接收 commit 失败不创建 Received，接收通知不承诺远端可靠投递。
