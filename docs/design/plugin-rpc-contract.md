# 插件 RPC 核心协议 v1

> 0008 状态说明：[设备契约](device-contract.md)新增的统一身份证明、实例关联与审批字段/方法尚未纳入现有业务 Schema/向量。本文本机帧与控制协议不因此失效，但现有互操作用例不能证明统一设备认证完成；具体业务协议须版本化更新并校验后再实现。

- 状态：核心传输、生命周期、processor/observer/action 协议以及 peer_sessions 同步后端方法已实现，真实子进程端到端测试通过（`engine/peere2e_test.go`）；同步后端业务方法与 DTO 见[同步后端契约](sync-backend-contract.md)。非 Go 语言的互操作实现尚无。
- 日期：2026-11-07。
- 依据：[插件契约](plugin-contract.md)、[提交契约](pipeline-contract.md)。本协议只用于 Engine 与本机插件子进程。设备网络协议由 0008 重定，不要求保持 wire v2。
- 机器可读定义：[帧与方法表](protocol/plugin-rpc-v1.frame.json)、[控制载荷 JSON Schema](protocol/plugin-rpc-v1.schema.json)、[互操作向量](protocol/plugin-rpc-v1.vectors.json)。二进制规则由帧表与本文共同定义，Schema 不能替代状态、授权、整型范围和字节预算验证。

## 1. 通道、帧与标识

Engine 创建子进程，通过 stdin/stdout 单向管道组成双向可靠字节流，不监听 TCP。stdout 专供协议，插件日志不得写 stdout；stderr 默认丢弃，不作为错误或宿主日志转发。管道 EOF、错误 magic、版本、保留位、长度或无效状态使本次会话失败，清理所有在途资源，按插件契约处理崩溃恢复。

固定 32 字节头，大端序：magic=`UNRP`，u16 version=1，u8 type，u8 flags=0，u64 request_id，u64 stream_id，u32 payload_bytes，u32 reserved=0。长度只计 payload，必须在分配内存前检查。流式解析必须支持头和 payload 被任意拆读或多帧合并，不能以一次 Read 作为一帧。未知 type 或非零保留位是会话协议错误。

| 类型 | 数值 | request_id | stream_id | payload |
|---|---|---|---|---|
| REQUEST | 1 | 非零，新调用 | 0 | UTF-8 JSON request |
| RESPONSE | 2 | 对应在途调用 | 0 | UTF-8 JSON response |
| CANCEL | 3 | 对应本端发起的调用 | 0 | 空，payload_bytes=0 |
| DATA | 4 | 内容所属根调用 | 非零 | 原始字节，不是 JSON/base64 |
| WINDOW | 5 | 同上 | 非零 | JSON，增加发送方信用 |
| END | 6 | 同上 | 非零 | JSON，声明成功结束及总字节数 |
| HELLO | 7 | 0 | 0 | JSON hello |
| HELLO_ACK | 8 | 0 | 0 | JSON hello_ack |

Engine 发起的 REQUEST 使用单调递增奇数 ID，插件发起使用偶数 ID；各方向 stream ID 同样奇/偶分配。ID 从 1/2 开始，会话内不复用，溢出前结束会话。回应、取消和流帧复用关联 ID，不受发送方向的奇偶限制。JSON 中 u64 使用规范十进制字符串（0 或无前导零正整数），语义校验必须拒绝超过 2^64-1；头部 u64 不转换为浮点。

JSON 只允许一个顶层对象、有效 UTF-8，拒绝重复键、尾随内容、未知字段、NaN/Infinity；解析嵌套深度最多 32。Config 是唯一允许插件自定义业务字段的对象，仍受 JSON/大小限制；未知 Config 键由插件 Init 拒绝为 invalid_config。version 的语义版本格式仍需业务校验，Schema 的普通 string 验证不足以证明版本有效。

## 2. 握手、描述与限制

父端复核程序摘要并启动后，先发 HELLO：协议版本、随机 32 字节 nonce 的小写十六进制、期望 Spec 和可接受上限；插件回 HELLO_ACK，回显 nonce、实际 Spec 和协商上限。在 ACK 前不接受其他帧。首次握手硬限制 16 KiB、默认 5 秒超时，失败关闭进程，不用 processor 的 continue 掩盖启动错误。

实际 ID、Version、APIVersion 与安装描述一致，Capabilities 作为集合一致，拒绝重复/未知能力；不能协商获得安装与配置未授予的权限。nonce 仅防止会话串用，不证明二进制来源。重启使用新 nonce、全新 ID 空间与短期句柄，旧响应/句柄一律无效。

协商值由插件取自身支持值与父端报价的较小值，必须符合 Schema 最小值；父端仍可拒绝不足以支持业务的限制。默认报价与硬上限：control 1 MiB、DATA 单帧 64 KiB、单流窗口 1 MiB、会话同时流 32、每端发起的在途 REQUEST 128。所有入站流累计未消费 DATA 预算为 8 MiB，超过则停止发 WINDOW；所有待处理控制载荷累计预算 8 MiB，超过预算的失控对端结束会话，不能无界排队。

流窗口不能突破会话总预算，即使逐流协商值之和更大。生命周期和取消帧有独立有界调度空间，不等待业务 worker 或数据窗口；写端每帧最多 64 KiB 数据后重新检查控制队列。不以这些上限承诺操作系统调度的硬实时期限。

## 3. 调用、反向调用与取消

REQUEST 包含 method、timeout_ms、parent_request_id 和 params。timeout_ms 是相对预算，范围 1..300000 毫秒，收到时启动单调计时；0 无效。发送方从入队前就计时，网络/排队不延长本地 deadline，双方 deadline 先到者生效。根调用 parent_request_id="0"；嵌套 Host/stream.open 调用指向仍有效的父调用，最多 4 层，不授权递归再入提交或数据库。

一条调用只发一个 RESPONSE，ok=true 带方法定义的 result，ok=false 仅带稳定 error（code/retryable/retry_after_ms），不带自由文本或调用栈。语义错误返回该请求错误，不必断会话；越权能力、格式不合法、未声明流或帧预算违规属于协议错误，断会话并回收。未知方法返回 unsupported，不执行任意字符串命令。

发起方取消后发送 CANCEL，本地立即禁止该请求 patch 提交并撤销相关授权，接收方取消 context、停止生成内容、尽快返回 cancelled。已排入管道的 DATA/响应可能继续到达：在有限 drain 状态下按上限读取丢弃，绝不采纳。取消默认给 1 秒宽限，未收敛则由 Engine 终止并 wait 插件进程；取消流也不能阻止 Close 调度。重复取消和已终态响应按已关闭请求处理，不触发再次回调。

每端记录已完成单调 ID 的高水位及有界在途集合；未知未来 ID 的 RESPONSE/DATA 为协议错误，已终态旧 ID 的迟到帧只允许在会话流控与 drain 总预算内丢弃。这样不需要永久 tombstone；持续向已终态 ID 灌流会耗尽预算并终止会话。

Process/OnEvent 同实例串行，协议读写与 Host 反向调用使用独立调度，不得同步阻塞等持有的业务锁。Close 使用独立清理 context；Engine Stop 的总退出窗口仍为 5 秒，不能被单次 REQUEST 的较长 timeout 延长。父管道 EOF 时插件取消所有回调与后台任务并退出。

## 4. 核心方法与处理 patch

机器可读方法表规定 request params 和成功 result 的 Schema；调用方向与能力仍须按下表校验：

| 方法 | 方向 | 条件 |
|---|---|---|
| plugin.init | Engine→插件 | ACK 后一次；验证专属配置与订阅，不联网 |
| plugin.start | Engine→插件 | Init 成功后一次；后端 Bind 在 Start 前完成 |
| plugin.process | Engine→插件 | Start 后，processor 能力、配置订阅 captured/received |
| plugin.run_action | Engine→插件 | Start 后，action 能力且 action_id 在 Spec.actions 中；只在用户经 RunAction 触发时调用，不订阅事件 |
| plugin.on_event | Engine→插件 | Start 后，observer 能力与对应订阅，只读 |
| plugin.close | Engine→插件 | 幂等，可清理部分 Init 成功实例，不和新业务并发启动 |
| host.notify | 插件→Engine | 有效父调用内；不作为日志，长度最多 1024 UTF-8 字节，每实例每秒 5 次，不积攒突发额度 |
| backend.submit | Engine→插件 | Start 后，sync_backend 且 Spec.external_channels；父调用携带 `sync_part` 流 |
| backend.leave_channel | Engine→插件 | 同上且 Spec.remote_revocation；成功返回后 Engine 才删除渠道记录 |
| backend.register / backend.report | 插件→Engine | 有效的 backend.submit 期间或之后，针对该 transfer 已登记且仍未终态的目标 |
| backend.receive | 插件→Engine | Start 后，external_channels；根请求携带 `receive_part` 流 |
| stream.open | 内容发送方→接收方 | 有效父调用，purpose 与调用方向/能力匹配，配额可预留 |

process 参数包含 event、generation、Item 快照和 Policy 快照；正文只有 text/plain 通过短期流提供，image/files 只有格式、名称/大小等授权元数据，普通 processor 不获得文件路径或原始图像/文件字节读取能力。没有 text/plain 时 text=null。processor 只做标注，不返回正文。

成功返回 patch 包含同一个 generation，以及可选 persist/sync/expire_after_ms/tags，未出现字段表示保持；空 patch 只有 generation。Tags 为最终完整替换列表，SDK 将 Add/RemoveTag 暂存为该列表。expire_after_ms 必须是正的、可表示为 time.Duration 的整数毫秒，Engine 在最终 commit 起算；Policy.ExpiresAtMs 是已有绝对期限快照，不能直接冒充该 patch。SetExpire 小于 1ms 或非整毫秒返回 invalid_argument，不能跨语言悄悄四舍五入。

patch 不含正文字段：processor 不能改写内容，出现 text 或 origin 等未知字段整份拒绝。字符串/标签限制按 UTF-8 字节校验，不仅按 Schema 的字符数。

plugin.run_action 参数为 action_id、generation 和只读 Item 快照（text/plain 通过 process_text 流提供，与 process 相同）；成功结果为同一 generation 加 `text` 流描述，purpose 为 action_text，由插件经反向 stream.open 声明，parent_request_id 指向 run_action 根请求。Engine 校验 UTF-8、非空、64 MiB 硬上限及用户限制后才创建新条目，字节在根请求结束前不进入历史；未返回文本、重复返回或流未 END 视为失败。action 没有 Policy，也不能返回标签。Go 默认期限 10 秒，超时先取消再终止，规则同 process。

generation 来自 Engine 本次回调，每次重试/热替换重新分配；请求会话、generation、插件实例代次、父提交代次全部匹配才原子采纳全部变更。某个字段无效、流未完成、过期、取消、panic 或方法失败则整份 patch 回滚，后续 continue/block_sync 按插件契约处理。

### 4a. 外部渠道与身份方法

身份方法 `sync_host.identity_hello`、`identity_prove`、`check_peer` 的 DTO 与错误码见同步后端业务契约 §3；新增稳定错误码 `bad_proof`、`bad_pair_proof`、`replay`、`binding_mismatch`。

外部渠道方法（0009）：

Spec 增加布尔字段 `external_channels`、`remote_revocation`（默认 false，参与握手 Spec 相等比较）；`remote_revocation` 要求 `external_channels`。其余 DTO 见 Schema 中的 `ext_*` 定义与向量，业务语义见[同步后端业务契约](sync-backend-contract.md) §6a：

- `backend.submit`：`transfer_id`、`timestamp_ms`、`targets`（已启用渠道 ID，去重、≤1024）、`parts`（1..8，`mime`、`size` 与流描述 size 一致，purpose 必须为 `sync_part`，总量 ≤ 16 MiB）。流以该请求 ID 为根，仅内存；文件 part 不下发。结果 `{queued}`。
- `backend.register`：登记该 transfer 实际会发送的目标，必须是 submit 的子集。`backend.report`：状态只取 `queued/sending/sent/superseded/failed`，未登记目标或未知 transfer 拒绝；没有自由文本原因字段，未知字段整份拒绝。全部登记目标终态后 transfer 回收，进程退出时清空。
- `backend.receive`：`channel_id`、`transfer`、`view`、`parts`（1..8，purpose 必须为 `receive_part`，总量 ≤ 16 MiB）。Engine 拒绝（渠道停用、重复、无空间）返回 `invalid_state`，插件丢弃且不重试。
- ID 字段为 1..128 字节 UTF-8，MIME ≤ 128 字节。Go 实现的校验函数是 `plugin/rpc` 中各 DTO 的 `Validate`，Host 与 SDK 两侧共用，向量测试直接调用它们。

## 5. 流描述、信用与释放

Stream 描述包含 stream_id、size、purpose，不含本机路径。流绑定当前会话、发送方、根请求与用途，不能由其他回调或旧会话使用。流 ID 非零，size 不超过授权内容和硬上限；控制 JSON 不内联正文，文本也不因大小不同出现两套编码语义。

Engine→插件的 process_text 描述随 process 参数声明；插件先校验并预留有界输入资源，再通过 WINDOW 授予信用。插件→Engine 的 action_text 先以反向 stream.open 声明，parent_request_id 指向 run_action 根请求；Engine 返回接受并发 WINDOW 后才能传 DATA。stream.open 自身的请求结束不释放根 process 所属流。

起始信用为 0；每个 WINDOW.credit_bytes 必须 >0，允许累计但不得超过单流协商窗口或接收方会话总预算。DATA 长度必须 >0、不超过帧上限、剩余信用和声明 size。接收方只在字节已消费或移入独立有界存储后补信用；发送方不能把 WINDOW 当作新的内容大小声明。超大内容使用文件/管道逐块处理，不整体 JSON/base64。

发送方在恰好 size 字节后发 END{ok:true,size}，零字节正文只发 END；短流、超量、重复 END、END 后 DATA 都失败。接收方保留已完成流的暂存资源直到根请求提交/回滚；最终 process RESPONSE 只能在所有其使用的输入/输出流 END 后发出。成功 action_result.text 必须引用该根请求的已完成 action_text 流，不能引用未消费输入或 sync_part。

取消、失败响应、根请求完成、会话断开立即撤销流授权，关闭句柄并回滚预留；只保留处理/历史/操作已经正式接管的资源。正常结束不发送额外流关闭 RPC。sender 出错通过根调用 error/cancel 结束，不发送一个“成功 END”掩盖截断。未被 patch 引用的暂存输出同样回收。

process_text 仅 text/plain；action_text 仅在 plugin.run_action 中由插件发起，且为 UTF-8 文本。sync_part/receive_part/sync_state 只授予 sync_backend，对应方法、短期授权、完整状态快照和接收准入见[同步后端业务契约](sync-backend-contract.md)，普通插件不得打开。已 END 的流释放活跃流槽；根请求可只保留完成记录及必要暂存引用，因此 4096 个 part 不必同时占用 32 个流槽。所有权保持到业务根请求结束，不能由短暂子调用响应提前撤销。

## 6. 互操作验证与后续落地

向量文件给出固定 magic/头字节、合法 payload、非法 patch 和跨帧分块样例；后续 `plugin/` 编解码器及真实子进程必须共享这些向量。本文文档检查只验证定义与向量一致，不代表运行时测试已通过。

实现必须覆盖：任意拆读/粘帧、大端序、超长分配前拒绝、未知位、重复 JSON 键、大整数范围；握手 nonce/Spec/API 不一致；同实例串行但反向 Host 无死锁；不同流公平调度、零信用禁止发送、总预算背压；取消同时收到迟到 DATA/patch、旧会话串用、根请求资源回收；UTF-8/标签字节/过期单位；畸形、超限与模糊输入不泄密。完整发送/接收互操作测试须按同步后端业务契约实现，当前尚未执行。
