# 剪贴板自动写回契约 v0.1

- 状态：契约已确定，待实现；不表示宿主或 Engine 已支持这些接口。
- 日期：2026-11-07。
- 依据：[0004](decisions/0004-clipboard-pipeline-lua.md)、[Engine 契约](engine-contract.md)、[提交契约](pipeline-contract.md)、[历史契约](history-contract.md)、[插件契约](plugin-contract.md)。

多后端的全局同步开关按[0007](decisions/0007-multiple-sync-backends.md) 执行；自动写回/历史使用属于本机行为，不受各后端类型或自动文件阈值限制，远端自动应用则在全局关闭时作废。

## 1. 写回的含义与时序

插件改写本机捕获或显式提交的内容后，Engine 先 commit 最终内容，再安排系统剪贴板写回。历史、发送和写回各有独立结果：写回失败不撤销历史、不重发网络、不重跑处理器，也不把已经 commit 的提交改成 failed。SDK 不直接访问系统剪贴板，宿主仍负责串行写入、回读、识别来源和报告。

自动写回只针对本机内容正文或表示实际变化：标签、期限、persist/sync 策略变化不触发写回；SetText 最终恢复原文但移除了旧 HTML，仍属于表示变化。以处理前后完整内容的内部比较判定，不能仅比较 text/plain 或 wire 的文件名/大小哈希。

远端 received 的插件改写直接进入 Received 最终内容，随后通过原 BeginApply 应用，不另建本机写回任务。历史使用通过 BeginApplyHistory，不重新处理；三种流程共享唯一应用槽位。

## 2. API 与状态

新增不透明、实例内有效的 WritebackID，以及下列目标 API：

```go
func (e *Engine) BeginWriteback(ctx context.Context, req WritebackRequest) (ApplyTicket, error)
func (e *Engine) DismissWriteback(ctx context.Context, id WritebackID) error
```

`WritebackRequest{ID WritebackID, ExpectedEpoch string, ExpectedSequence uint64}`。BeginWriteback 返回现有 ApplyTicket，结果用同一 ReportApplyResult 报告；不得通过 HistoryID 或 ReceiptID 冒充写回任务。

`State.PendingWriteback` 为可空 `WritebackSummary{ID, ItemID, HistoryID, ExpiresAtMs}`，只表示当前可领取任务；不含内容句柄、租约、正文或路径。任务已领取后从 PendingWriteback 移除，由 ActiveApply 展示。ActiveApply 扩展 Kind=received/history/writeback 和可空对应源 ID；这些字段不授予内容访问能力，已有 State 不暴露 LeaseID 的约定保持。

SubmissionResult 新增 `Writeback *WritebackResult`，nil 表示尚未 commit，字段类型见[SDK 扩展类型契约](sdk-extension-contract.md)：

| 字段 | 语义 |
|---|---|
| State | not_needed/unavailable/pending/applying/applied/failed/deferred/superseded/expired/dismissed |
| ID | 仅实际建立过任务时非空；终态仍可用于关联，不等于可再次领取 |
| ErrorCode | unavailable/failed 时可选稳定原因：unsupported/permission_denied/conflict/limit_exceeded/unavailable；不包含平台自由文本 |

processing 或 commit 前 failed 的 SubmissionResult 中 Writeback 为空；committed 时写回字段确定为 not_needed/unavailable/pending，后续仅此字段随写回状态更新，Phase、HistoryID、SyncDisposition/OperationID 不改。GetSubmission 和同键重试返回最新写回快照，不得据此再执行提交。缓存被驱逐不取消 pending/active 任务；任务状态仍以 State 为准，不要求重新建立已驱逐 submission。

通过 `writeback` 事件通知待领取任务增删与状态变化；领取、结束同时生成 apply 事件，保留 submission 时另发 submissions 事件。通知仅携带公开 ID 与稳定状态，可靠入口是 State，不依赖事件必达。订阅快照必须包含 PendingWriteback/ActiveApply，溢出后重建订阅即可恢复当前待办。

## 3. 防止覆盖较新的复制

ObserveClipboard 的有效变化在进入处理前记录 `Guard{Epoch, Sequence}`；处理期间不允许因为耗时而长期阻塞后续真实观察。commit 时只有 Engine 仍处于同一观察位置、running/active 且宿主 Observe/Apply 能力 available，才建立 pending 任务。创建任务不代表系统剪贴板已经写入。

观察接纳与处理完成须分开串行化：Engine 维护“最新已接纳观察位置/代次”和“成功处置的内容基线”，接纳新观察立即推进前者，使旧写回 guard 失效；commit 前失败不推进成功基线。旧观察处理完成只能在代次仍匹配时更新成功基线，不能把更晚的观察覆盖回旧位置。较旧内容即使最终成功 commit 仍可入历史/按提交契约同步，但写回为 unavailable/conflict。幂等结果按原 epoch/sequence 返回；不得持有当前实现的 obsMu 跨插件等待而阻止新变化进入。

显式 CommitRequest 新增可空 `WritebackGuard{Epoch, Sequence}`：宿主先写入原始内容、回读并建立 baseline，然后把该位置作为 guard 传入；guard 与可选 BackendIDs 都属于幂等语义参数，换 guard 必须换 RequestID。不给 guard 时仍正常处理、入库与发送，但若正文改变则 Writeback=unavailable/conflict，不安排自动写回。无 Observe 能力的宿主不自动写回，结果为 unavailable/unsupported。原 SubmitRequest 不新增隐式 guard，兼容门面不靠“最新基线”猜测本次写入归属。

Engine 同时校验 guard 位置与该位置的原始内容身份；guard 指向不相同的内容即 conflict，不允许拿旧提交覆盖最新剪贴板。显式 guard 若已过时或不匹配原始内容，只拒绝写回，不阻止提交。新 epoch、真实本地变化或其他成功应用更新基线时，所有 guard 不再匹配的 pending 任务变 superseded；已领取任务按原应用竞争规则失效，不能成功确认。

BeginWriteback 要求请求、任务 guard 与当前观察位置三者一致，且没有其他 ActiveApply；不一致返回 conflict 并结束过时任务，并发占槽返回 in_use 且保留待办。宿主实际写入前仍需检查平台变更计数；无法避免的“检查与写入之间发生复制”竞争必须在平台说明和测试中记录，SDK 不声称可以跨进程原子写剪贴板。

## 4. 有界任务、领取与资源

每实例最多一个 pending 写回和一个共享 active 应用票据。新的有效写回替换旧 pending，旧任务变 superseded 并释放待办引用；active 票据不可静默替换或复用 token。新真实观察使旧 active 失效，但槽位仍保留至宿主报告结束；新 pending 可以等待该槽位释放。

pending 从 commit 起 30 秒过期，使用单调计时，不支持热配置；到期立即不可领取并变 expired，释放待办引用。BeginWriteback 先检查到期，成功领取后 pending 的 30 秒计时不再适用；active 遵循现有票据无隐式超时的协议，宿主必须报告结束。该超时限制只用于自动写回待办，不是内容/历史过期，也不提供重启恢复。

任务在 commit 时取得最终内容引用，persist=false 也可以写回。BeginWriteback 原子创建独立租约并转交 ApplyTicket，然后释放待办引用；宿主在写入文件后必须保留 ticket 租约直到文件被新剪贴板替换。ReportApplyResult 不替宿主 ReleaseContent。删除/到期历史不取消已创建写回，任务依赖独立内容引用。

建立任务前校验最终内容是否有宿主可写的表示，按既有文件优先规则选用；无可应用表示返回 unavailable/unsupported，当前权限不足为 unavailable/permission_denied。不强行为当前系统补造支持，也不延后等权限恢复自动写回。任务本身不复制第二份正文，但仍遵守总配额、租约和读取资源限制。

DismissWriteback 仅取消 pending，结束为 dismissed；已领取任务返回 in_use，需通过 ReportApplyResult 结束。当前实例未知/已终结任务返回 not_found，旧实例 ID 返回 resource_expired；不宣称取消接口永久幂等。context 取消可能发生在取消已提交之后，宿主查 State 恢复。

## 5. 写入、回读与失败

领取后宿主按既有应用协议写入最终内容，回读实际系统内容并导入。只有确认回读来自本次写入，才以 applied + BaselineLeaseID/Epoch/Sequence 报告；Engine 原子建立回读基线并结束任务为 applied。系统补充 HTML/厂商格式时按平台适配规范化，不要求回读逐字节等于 commit 内容，也不得悄悄修改历史最终内容。

已知来自当前 token 的观察通过 Observation.AppliedToken 返回 echo，不进入 captured，不产生第二条历史、不重发网络。若回读来源不能确认、发生新复制、token 失效或成功报告冲突，宿主重新读取并上报真实观察，不能给疑似变化加 token，也不能为修复冲突回滚系统剪贴板。

ReportApplyResult failed/deferred 都结束当前任务，不推进成功基线、不自动重新排队；下一次自动任务必须来自新的有效提交。deferred 表示此次放弃，不承诺以后自动重试，与 Received 仍可显式再次 BeginApply 的行为不同。若希望再次使用处理结果，用户可从仍存在的历史显式使用；persist=false 内容在最后持有者释放后无法恢复。

平台错误只传稳定 ErrorCode；写回失败不调用新 CommitContent“补偿”。AppliedToken 观察到回声后仍需 ReportApplyResult 结束槽位，宿主不能因为观察已推进基线就漏掉报告。

自动写回成功只产生 writeback/apply 完成事件，不产生 clipboard.pasted；它不是用户在 UI 使用历史，也不能证明外部应用发生粘贴。

## 6. 来源与深度

Item 在初始本机观察中为 origin=local_system、automation_depth=0，显式提交为 explicit/0，远端为 remote_device/0。处理器不能通过普通 patch 修改 origin 或 depth，也不能用 Host 再入提交。

一轮串行处理里，多个插件成功改正文仍只计一次自动化深度。若最终完整内容确实改变，commit 的来源记为 plugin，PluginID 为最后一个成功且实际改变正文/表示的处理器，depth 为本轮输入 depth+1；原始来源在内部保留供 guard/审计关联，不把地址写进公开 DTO。只改 Policy 或最终内容完整恢复原样时保留原来源/深度。普通接收条目即使来源变为 plugin，仍保留“远端进入”内部标记，不得因此再转发。

深度默认上限 3，定义为同一可证明自动化链最多执行三轮处理：输入 depth>=3 时不执行任何 processor，直接 commit 原内容与默认策略；全局同步过滤和已存在的不可撤销禁发约束仍生效。修正 0004 早期“超过上限”措辞，不能允许第 4 轮运行后才阻止。

已知 token 回声直接抑制，既不增加深度也不再次 commit。只有 Engine 能关联并证明来源的自动化派生才继承 plugin/depth；未知本地变化一律作为新的 local_system/0，不能按时间或相同文本猜测来源。当前公开 API 不允许制造任意自动化链，深度上限属于内部防御，需通过内部测试直接构造链验证，不能为了触发它而将每次正常回声重新送处理器。

## 7. 生命周期与配置

BeginWriteback 仅 running/active 且观察/应用能力 available 可用；无后端、断网、sync.enabled=false 或类型同步关闭不阻止本机写回。已领取 writeback/history 票据也不因同步暂停而失效，原 Received 的自动应用仍遵守接收策略。

Stop、挂起、新 epoch、能力撤销使 pending 失效并释放引用：Stop/挂起/能力变化为 dismissed，更新基线为 superseded；active 按现有 token 作废规则处理，直到宿主报告才释放共享槽位。Close 中止读写工作并按资源契约回收，不能在回收未完成时报告完成。所有待办只在实例内，不保存到数据库，不在恢复网络/重启后重新应用。

DismissWriteback 在 created/running（含 suspended）可用；stopping/stopped 不接受新的取消写入，Stop 已负责收敛 pending。GetState、事件查询与结束 active 的 ReportApplyResult 沿用既有生命周期清理约定。

## 8. 待实现的契约测试

- 观察/显式提交改正文、仅改策略、恢复原文但清除 HTML、远端改写各走正确路径。
- guard 的位置/内容绑定，处理期间新复制、同内容新 epoch、领取后被覆盖，不能吞掉真实变化。
- 显式提交无 guard/旧 guard/无 Observe 时仍可 commit，返回明确不可写回结果。
- 单 pending 替换、共享 active 占槽、30 秒过期、过期与领取竞争、dismiss 与领取竞争。
- persist=false 待办与 ticket 持有引用；删除历史不破坏活动任务，结束后释放自有资源。
- AppliedToken 回声不重处理、不重复历史/发送，回读补全不改历史，成功确认冲突不回滚系统内容。
- failed/deferred 不重新排队，不产生 pasted；submission 缓存驱逐不让任务丢失或复活。
- 本机写回/历史使用在同步暂停时可用，Received 仍按原策略；挂起/停止/权限撤销正确作废。
- 三轮深度上限、混合插件只增一次、完整恢复保留来源、来源未知不伪造插件归属。
- 初始快照/事件溢出可恢复待办，状态/事件/日志/错误不泄露正文、路径或票据秘密。
