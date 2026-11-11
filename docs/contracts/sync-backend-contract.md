# 同步后端业务 RPC 契约 v1 · P2P profile

> **统一设备身份修订边界（2026-11-08）**：统一身份与关联授权以[设备契约](device-contract.md)为准。下文 wire_version=2、自报 hello、ID 指纹、全量 peers/rejected 及对应 Schema 是待修订的 P2P 业务草案，不能直接实现为新架构的完整认证；“不增加密码学身份声明”的原约束取消。必须补齐可由 Engine 验证的持钥证明、上下文、防重放与关联修订；插件不能用自报 ID 或布尔结论授信。无旧协议兼容要求。具体业务 DTO/Schema/向量待版本化同步更新，本机 RPC 帧规则继续有效。

- 状态：peer_sessions 的业务方法与 DTO 已实现，经真实子进程配对、发送、介绍的端到端测试验证；对象存储（WebDAV）DTO 未做，也不是本文强制接口。密钥/Network 方案见[迁移契约](backend-migration-contract.md)；多后端公共边界以[0007](../decisions/proposed/0007-multiple-sync-backends.md) 为准。
- 日期：2026-11-07。
- 依据：[插件契约](plugin-contract.md)、[RPC 核心协议](plugin-rpc-contract.md)、[流水线提交](pipeline-contract.md)、[历史契约](history-contract.md)、[身份与信任](../architecture/identity-and-trust.md)。
- 机器可读定义：沿用并扩展[帧/方法表](protocol/plugin-rpc-v1.frame.json)、[Schema](protocol/plugin-rpc-v1.schema.json) 与[向量](protocol/plugin-rpc-v1.vectors.json)。所有方法只授予已安装、握手且获配置授权且声明 peer_sessions 的 Go sync_backend 实例；同一 Engine 可有多个独立实例；Lua 和普通 processor/observer 调用属于越权。

Engine 统一身份密钥仅由 Engine 初始化和持有；后端只加载自己实例私有传输身份/凭据，子进程持 runtime.lock。首次初始化原子持久化，坏身份拒绝，不导入旧密钥、地址或配对记录。builtin 显式装配、外部程序发现、实例锁竞争、零装配与测试入口遵循[装配契约](backend-migration-contract.md)。

本文的 backend.* RPC 方法、握手、子进程锁和 EOF 规则适用于 external 适配。builtin tailcat 使用进程内接口承载相同业务语义，无本机 RPC 握手，仍须执行实例隔离、授权修订、资源预算与撤销屏障；具体 Go 接口待 SDK 契约补齐。builtin 关闭须确认端点和旧任务退出，不能用终止插件子进程作为恢复手段；无法确认停止时 Engine 停止同步服务并报告失败。

## 1. 职责与生命周期

builtin `builtin.tailcat`（`internal/plugins/tailcat/`，由宿主显式装配）持有 tailcat 端点、设备连接、按设备契约修订的 wire 编解码和传输循环；Engine 持有身份、配对审批、rendezvous 票据、同步策略、队列、内容引用、操作结果与历史。后端不打开 Engine SQLite，不调用宿主剪贴板，不存第二套批准/拒绝状态。

外部后端启动顺序为 HELLO/ACK → plugin.init → backend.bind → 初始 backend.apply_state → plugin.start；builtin 对应 Init → Bind → 初始 ApplyState → Start。Bind 只配置本次实例绑定，不联网；初始快照可以在 Start 前安装。Start 后持有常驻端点与长连接，不能为每个 Send 启进程或重连。后台断网/重连不依附某次 Process 或 Send 调用 context。

`binding_id` 为 Engine 每次后端会话生成的随机 256 位 token，区别于握手 nonce；Bind 包含 backend_id、transport_kind=peer_sessions、self_id、self_name、wire_version=2。后端 ID 与握手 Spec.ID 一致；每个实例只见自身快照与 token，不能借另一个后端的成功授权传输。所有 session/send/receive token 至少 256 位随机、只在本次 binding 内有效，不进入 State、普通事件、错误或日志。它们限制 RPC 代理的可用范围，不是对恶意本机 Go 程序的权限沙箱。

| 方法 | 方向 | 结果/作用 |
|---|---|---|
| backend.bind | Engine→后端 | 空成功结果；一次有效绑定 |
| backend.apply_state | Engine→后端 | 安装完整状态，返回实际 revision，形成状态生效屏障 |
| backend.pairing_address | Engine→后端 | 仅 export/pairing_code 场景返回秘密地址 |
| backend.connect | Engine→后端 | 显式配对尝试（attempt_id、地址、可选 expected_peer_id；不含配对令牌），返回 peer_id/awaiting_remote_approval |
| backend.send | Engine→后端 | 单 dispatch、单目标，sent 或稳定错误 |
| sync_host.identity_hello | 后端→Engine | 取得本机 Hello 并登记本连接转录 |
| sync_host.identity_prove | 后端→Engine | 提交对端 Hello，取得本机 Proof（及可选 PairProof） |
| sync_host.check_peer | 后端→Engine | 提交对端 Proof，由 Engine 验证后产生 allow/pending/reject 与会话授权 |
| sync_host.peer_event | 后端→Engine | 上报已验证连接的 connected/disconnected |
| sync_host.introduce | 后端→Engine | 可信会话介绍候选，仅产生待批准记录 |
| sync_host.authorize_send | 后端→Engine | 真正传输前再次授权和校验引用文件 |
| sync_host.open_send_part | 后端→Engine | 获取当前发送授权下某个 part 的短期流 |
| sync_host.receive_open | 后端→Engine | 完整元数据准入、资源预留或丢弃 |
| sync_host.receive | 后端→Engine | 承载一个已准入接收，等待全部流与处理完成 |
| sync_host.receive_part | 后端→Engine | 为该接收声明一个 part 的数据流 |

sync_host.identity_hello/identity_prove/check_peer/peer_event/introduce/receive_open/receive 可由 Start 后后台连接任务发起，parent_request_id="0"，不要求存在 Send/Process。authorize_send/open_send_part 必须指向有效 backend.send 根调用；receive_part 必须指向有效 sync_host.receive 根调用。它们遵守核心协议的并发/流控上限，不通过假造永不结束的父请求维持授权。

## 2. 状态快照、修订与撤销

`SyncState{backend_id, binding_id, revision, runtime, device_name, sync_enabled, policy, limits, network_generation, peers, rejected}` 为该后端完整快照。sync_enabled 是全局能力开关，policy 是本后端 Text/Images/Files/AutoFileMaxBytes；不是另一套全局类型过滤。peers 含 ID、名称、秘密 address 和 trust_revision；rejected 只含 ID/request_revision。不得遗漏仍有效的批准/拒绝项并把它们误解为默认允许。未知设备仍须 check_peer，后端不能单凭自己的缓存授信。

快照编码为 UTF-8 JSON，通过 purpose=sync_state 的流随 apply_state 参数传输，解决 1 MiB 控制帧装不下完整状态的问题。上限 64 MiB、4096 个 peers、4096 个 rejected；地址最多 8192 UTF-8 字节，名称仍受 Engine 的设备名规则约束。超过边界不能截断后生效，启动/更新返回 resource_exhausted；Engine 的新增配对/拒绝写入也须在达到边界前返回该错误，不先形成无法同步的状态。

后端先完整接收、校验、暂存，再原子安装；失败保留上一有效快照。Engine 为每次状态提交递增实例内 revision，ACK 前新状态不视为已在后端生效。相同 revision/相同规范化内容可幂等确认，相同 revision/不同内容或较旧修订返回 conflict。快照流和父 apply_state 请求完成后立即释放暂存引用。

Engine 串行派发状态更新，不让旧 ACK 覆盖新状态；本地信任/运行策略从自身提交点立即禁止新的授权，不等待快照传完。network_generation 增加表示重新评估网络、重置重连退避，不生成新身份、不重发旧内容。

删除信任、拒绝身份或 suspended 需要立即取消相应发送/接收授权、关闭旧会话；apply_state ACK 必须表示旧会话不可继续执行且在途根调用已终结。Engine 不持业务锁等待 ACK；后端不响应时按取消宽限终止并 wait 进程。撤销 API 只有在屏障确认或进程回收后才返回成功；调用者超时不回滚已持久化撤销，后续清理继续。此处是独立后端迁移时对原“会话代次失效、网络资源随后关闭”的补充，不宣称已经写出的字节可以撤回。

实现状态（§1–2）：DTO 与校验在 `plugin/rpc/state.go`，SDK 接口 `plugin.PeerBackend`（Init 后、Start 前或之后均可 Bind/ApplyState），宿主侧 `internal/pluginproc/peer.go`。代理保存最新期望状态；`ApplyState` 区分两类失败：后端明确拒绝（invalid_argument/conflict/limit_exceeded/resource_exhausted/invalid_state，表示保留上一快照）且新快照不撤销任何授权时，返回该错误、保留进程并丢弃被拒修订；无响应、超时、崩溃、协议错误，或被拒的快照含撤销（删除/改动对端、新增拒绝、挂起、关闭同步、收紧策略或限额、网络代次变化）时，终止进程并视回收为屏障，重启时先重放 Bind 与最新快照再 Start，期间到达的新修订在发布会话前补发。已有真实子进程测试；Engine 侧装配（由 Engine 数据构造 State、撤销 API 等待屏障）待地址与信任移出会话核心后接入。

普通后端类型/阈值与 limits 变化只影响之后开始的传输，不追溯已授权任务；全局 sync.enabled=false 是立即撤销，取消所有后端内容发送/接收与远端自动应用，控制连接/配对可保留，失步内容连接须关闭重建。接收最终交付仍按交付时策略过滤，过滤后为空则丢弃。全局关闭/挂起/信任撤销与普通策略变更不能混为一类取消策略。跨后端撤销须收敛所有相关实例，不能只等一个 ACK。

## 3. hello、配对与连接授权

后端的传输握手与身份证明合并为以下顺序（identity-protocol §4，Schema 为 `sync_identity_*`、`sync_check_peer`）。二进制字段一律 base64url 无填充；Engine 持有设备私钥、Hello 的 nonce 与转录，后端只搬运字节：

1. 后端为本条连接生成唯一 `connection_id`，并算出双方一致、对本连接唯一的 `context_binding`（≤256 字节）。调用 `sync_host.identity_hello{connection_id, direction, attempt_id, context_binding}`；outgoing 为发起方 I，incoming 为响应方 R。Engine 返回自己的 Hello（含新鲜 nonce），并保存转录状态；同一 `connection_id` 再次调用返回 `conflict`，每实例并发未完成握手 ≤ 8（`resource_exhausted`），超过 60 秒未完成 `resource_expired`。
2. 后端与对端交换 Hello，调用 `sync_host.identity_prove{connection_id, peer_hello}`。Engine 选定版本（无公共版本 `protocol_mismatch`），按角色计算转录 T，返回 `proof`（Ed25519，64 字节）；仅当 outgoing 且当前 attempt 带配对票据时附 `pair_proof`（HMAC，32 字节），否则为 null。Engine 只对本连接转录签名，没有任意载荷签名方法，配对令牌也不会交给后端。
3. 后端收到对端 Proof（和可选 PairProof）后调用 `sync_host.check_peer{connection_id, direction, attempt_id, hello{wire_version,name,address}, peer_proof, peer_pair_proof}`。Engine 用对端 Hello 里的公钥验证；`peer_id` 由证明过的公钥导出，线路上不再有自报的 peer_id，也不再有 `pair_ticket`。验证失败返回稳定错误：`bad_proof`、`bad_pair_proof`、`replay`、`binding_mismatch`、`resource_expired`、`protocol_mismatch`、`identity_mismatch`（ID 与已登记公钥冲突，或不等于 attempt 的 expected_peer_id/票据 dev）；失败时丢弃握手状态，不留下可复用的东西。成功才进入下面的授权决策，结果 `{peer_id, decision, session_token, trust_revision}`。

后端宣称的"已验证"一律不被接受；Engine 同时校验版本、目标统一身份的持钥证明、连接上下文、关联授权及活跃显式尝试，按设备契约决策。不承诺远端应用确认。

incoming 带有效 `peer_pair_proof`（对本地保存的未过期配对令牌验证通过）只标记“通过配对码发现”，仍返回 pending，绝不自动 allow；过期、撤销票据不授信。介绍候选也不授信。outgoing 的显式 PairByAddress/JoinByCode 可以沿用主动配对的本机信任语义，但必须由 Engine 当前 attempt_id 授权并持久化；普通后台重连不能冒充该尝试。

backend.connect 的 attempt_id 由 Engine 创建，绑定目标秘密地址、可选已知 peer_id、配对票据、原配对 Operation 和生命周期；连接取消/结束即不再允许用其授予新信任。后端不能只因 direction=outgoing 或握有地址就替 Engine 批准设备。预期身份不符返回 identity_mismatch，不覆盖旧设备记录。两端核对值由 Engine 按待定义的统一身份协议计算并展示，不采用自报 ID 哈希或插件提供的任意“匹配”结论。

allow 返回 session_token 与对应 trust_revision；pending/reject 必须返回 null token。后端只有在双方 wire verdict 都接受后，才可持 token 上报 connected 并收发内容；对方未批准时 connect 返回 awaiting_remote_approval=true，不注册内容连接。已知设备信息更新、候选落库及拒绝去重均由 Engine 执行。

peer_event 包含每连接从 1 起递增的 sequence；重复相同序号/状态幂等，不同内容冲突，旧序号不能把新连接改为离线。Engine 同时验证 connection_id/token 与当前代次，断开的 token 不得重新 connected；重新连接先 check_peer 获取新 token。disconnected 的 remote_accepted 为 false；connected 必须 true。确定双向同时连接的胜出规则仍沿用 session 的既有规则，被替换连接的旧通知不能改变新连接状态。

builtin.tailcat 通过认证会话上的 `{"t":"introduce","candidates":[{"id":"…","name":"…","address":"…"}]}` 控制消息发送介绍，与剪贴板共用串行写锁，每条最多 64 个；会话建立、状态变化及每 24 小时重发当前列表。发送写入设 5 秒截止，失败关闭会话；接收方使用本地会话 ID/token 调用 `PeerAuthority.Introduce`。禁用 `SyncState.Introduce` 同时停止发送与接收；介绍无需全局内容同步开启。

introduce 仅限已 connected、仍批准的会话，每次最多 64 个候选；名称/地址按上限校验，绝不修改已批准状态或跳过指纹核对。pairing_address 只用于显式地址导出或 Engine 创建配对码，不能放入健康状态、事件或普通初始化结果。配对票据与 rendezvous 服务仍由 Engine 管理，后端不自行创建、续期或消费配对码。

## 4. 单目标发送与最后授权

Engine commit 后为每个目标创建独立 backend.send：backend_id、mode（automatic/explicit）、dispatch_id、operation_id、peer_id、connection_id、session_token、content_handle 和 clip 元数据。content_handle 是 Engine 签发的只读短期 token，绑定该根调用/目标与最终发送视图，不是 ContentID、租约或文件路径；不能保留给下次 Send。

clip 包含 wire_id/origin/at_ms/hash 和按原 wire 顺序排列的 parts。part.index 从 0 连续，每项含 kind/mime/name/size；内存表示映射 wire fmt，文件映射 file。mime/name 的组合遵循 wire v2，不改对端字段。origin/at_ms 来自发送构造，不用于接收方授信或历史时间；wire_id/origin 承载共用 TransferID/SourceDeviceID，其认证与有界跨后端去重遵循 0007，不能仅凭远端声称建立去重授权；hash 沿用当前 clip.Snapshot.Hash 算法（文件仅按名称/大小），不能宣称文件内容完整性校验。

每个目标实际占用 writer、准备输出 wire header 之前，后端反向调用 authorize_send。Engine 检查根调用、dispatch/handle/session/peer 的绑定，当前仍批准且 running/active、全局授权、本后端类型策略、automatic 模式下的逐文件阈值及该发送视图大小限制、引用源文件的版本。所有引用文件须在此阶段打开/校验并保留读取句柄，资源不足为 resource_exhausted；不能一边写 header 一边才发现尚未校验的文件引用已经变化。

成功得到 grant_id 和 state_revision，授权仅该次传输、该连接。开始前状态更新与授权串行确定先后；普通后端策略不追溯在途授权，全局关闭/撤销/挂起使其立即失效。mode 来自 Engine 提交来源，不接受后端把 automatic 改成 explicit。没有有效 grant，后端不得输出该 clip 的首字节。返回 source_changed/limit_exceeded/sync_disabled 等传输前失败只让此目标失败，连接保持。

后端逐 part 调用 open_send_part(grant_id,index)，Engine 返回 purpose=sync_part 的流描述，所有流属于 backend.send 根调用；随后 Engine 向后端发送 DATA，后端发 WINDOW。一次可按需打开少量流，不要求 4096 个 part 同时占满 32 个活跃流位；已接收 END 的流释放活跃槽，根调用仅保留其完成元数据/必需资源。

后端将流写入现有长连接，全部 part END 且 wire 字节完整写完后才回应 sent。传输前失败返回稳定 error；写了 header/正文后失败必须丢弃该设备连接，禁止继续读写已失步的 wire。成功仅为本地写完，不代表对端接收或应用。RPC 中断/超时可能已经写出部分甚至全部，目标失败，不自动重发。

实现状态（§4 RPC 层）：DTO 在 `plugin/rpc/send.go`，SDK 接口 `plugin.PeerSender`/`SendHost`，Engine 侧 `plugin.PeerDispatcher`/`SendSource`，宿主实现在 `internal/pluginproc/send.go`。宿主按所属 `backend.send` 的 dispatch_id/connection_id/session_token/content_handle（常量时间比较）校验 `authorize_send`，每次发送只发一个 grant（再次请求 conflict，Engine 拒绝后可重试），`open_send_part` 须带该 grant、索引在范围内、每个 part 只开一次、同时打开的流 ≤ 4。空 part 不建流：返回 `stream_id=0, size=0` 的描述，后端据此得到空内容（零字节流没有信用额度，END 会抢在宣告它的响应之前）。真实子进程测试覆盖多 part 流式（含超过一个窗口）、授权被拒无任何 part 被打开、part 单次打开与越权请求。

每 (BackendID, device, DeviceID) 一个在途加一个待发由 Engine 维护，后端不另建无界发送队列；Send 只有 Engine 指派可启动，不自行重放历史。多个目标可并发，但不允许同一连接的两个 wire 写交错。根调用结束后 content_handle/grant/流全部撤销，Operation 持有的内容引用按目标终态释放。

## 5. 接收准入、流与交付

后端从可信 connected 会话读到 clip header 后，在读正文之前调用 receive_open，提交会话绑定与完整 clip 元数据。Engine 校验身份、生命周期、全局开关、本后端类型策略、跨后端传输身份、part 数/总字节和每项限制，先取得跨后端去重占位再预留完整声明大小；允许接收的最大 part 数仍 4096、内存单表示 64 MiB、单文件 2 GiB、总计 4 GiB。无符号大小转 int64 前检查，求和检测溢出。

不满足用户策略、当前限额或容量但 header 仍符合协议硬边界时，返回 discard/null；后端按声明长度读完丢弃，保持连接。身份已撤销、畸形/超协议硬上限的头不授予 drain 权限，关闭连接。不能用“为了保持连接”来接收无界长度。

准入成功返回 receive_id；绑定本次连接/token、元数据和预留。该 token 从响应起 10 秒内必须用一次 sync_host.receive 根调用消费，未使用到期清理预留；不能因重试 receive_open 复用或重复消费，不能以 wire_id 充当幂等授权。默认每 binding 最多 32 个未消费或正在处理的 receive，超过返回 discard 或资源错误，不无界占额。

sync_host.receive 参数只有 receive_id，其 RESPONSE 等所有 part 和 received 处理/commit 完成后发出。后端在该根调用期间按 part 顺序，以 receive_part(receive_id,index,stream) 声明 purpose=receive_part 的流；Engine 接受后发 WINDOW，后端才读网络正文并发送 DATA/END。流属于 receive 根调用，不属于短暂的 receive_part 子调用；子调用成功仅表示允许打开流，不表示正文接收成功。

每个 part 恰好一次、大小等于准入声明，按顺序推进；用 End 释放活跃流槽以容纳大于 32 个 part。Engine 只写自身私有暂存资源，不要求后端先在其目录保存文件。保留完整准入内容以按原算法验证 wire hash，再按交付时策略过滤；不能先丢掉某些内存格式却声称已验证原始全内容 hash。

全部流结束后 Engine 再检查信任/生命周期/交付策略，运行 received 处理器并 commit；成功才建立 Received/可选 History，根结果 outcome=committed。Received 槽位须在持久事务前预留，去重成功记录与可选历史同事务提交（含 persist=false），不能先存历史再发现无法发布 Received。处理期间策略变化、Stop/撤销与提交的竞争、占位及资源释放遵循[流水线状态转换](pipeline-contract.md#7-状态转换与资源交接)。此回应是本机 RPC 的交付结果，不能转成 wire v2 不存在的远端 ACK。策略变化导致过滤后为空，或处理后的提交屏障发现输入选择已不合新策略时为 discarded，不重跑 received；存储/校验/处理不可恢复错误返回稳定 error，清理半成品并发脱敏 receive.failed。

网络中断、取消、重复 part、短读、hash 不符或撤销授权均清理暂存与预留，不能形成可应用条目。若后端仍可按声明精确 drain 剩余字节，策略/存储丢弃可保持网络连接；RPC 故障、超限或无法确定剩余边界时关闭对应设备连接。父进程退出或 binding 更换不恢复半成品，不承诺对端重发。

接收完成只以 validated connection 的 peer_id 记录原始远端来源，首版要求 wire origin 与它一致；共用 TransferID 的跨后端接收按 0007 有界持久去重，同一来源/TransferID/ViewHash 的元数据冲突不得覆盖记录，不同发送视图独立接收、不合并历史。去重占位/成功记录分别随失败回收/commit 原子落库，不把 wire origin 当作未经认证的身份。标签/期限由本机插件决定，不从 wire 接收；首版永不再转发，即使处理后来源变为 plugin 或 SetSync(true)。

实现状态（§5 RPC 层）：DTO 在 `plugin/rpc/receive.go`，SDK 侧 `plugin.PeerHost`（`ReceiveOpen`/`Receive`），Engine 侧 `plugin.PeerReceiver`/`ReceiveParts`，宿主实现在 `internal/pluginproc/receive.go`。`receive_open` 只凭头部决定 accept/discard；接受后的 receive_id 单次有效、10 秒过期、每个绑定最多 32 个未用（第 33 个不询问 Engine，直接 discard）。`sync_host.receive` 是根调用，`sync_host.receive_part` 是它的子调用，part 按序到达且大小必须等于头部声明；空 part 同样不建流（`stream_id=0, size=0`）。头部超出硬限制为 invalid_argument（不可 drain）。`CallOptions.Sent` 在请求帧写出后回调，part 声明由此开始。连接层：子请求的父请求已在途中结束时，对端回复 cancelled，而不是视为协议错误断开连接（拒绝根调用与 part 声明的竞态）。真实子进程测试覆盖顺序交付（含超窗口与空 part）、丢弃/discarded、id 单次使用、大小不符与处理器的授权/顺序检查。

实现状态（Engine 侧装配，`engine/peer*.go`，进程外 peer_sessions 后端）：状态快照由 Engine 数据构造（`TrustRevision` = 设备修订 + 关联修订，因此改名也会结束会话），apply_state 之后 `pruneSessions` 结束失去授权的会话；握手由 Engine 持钥完成（并发 ≤ 8、TTL 60 秒、nonce 记忆 10 分钟、attempt ≤ 16 且单连接一次性），授权复用 `recordPeer`，经配对码进入的设备仍为待批准；peer_event 按序号/令牌/信任修订/网络代次检查，双向同时连接按较小设备 ID 发起的连接胜出；发送按对端各一个在途 + 一个排队（新视图取代排队者）；接收由 Engine 生成 receive_id 并在 10 秒后释放预留，按后端实例检查对端授权（`allowedPeerOn`）。配对：票据带 `be`（后端实例 ID），出码方由调用者指定实例（`CreatePairingCodeVia`/`ExportPairingTicketVia`，默认 tailcat）；进程外后端经 `backend.pairing_address` 提供 `acc`，入码方按 `be` 找实例（找不到返回 unsupported，不回退），由 Engine 创建 attempt（地址、预期 peer、令牌）后调用 `backend.connect`；出码方的配对证明只对签发它的实例有效。介绍：`SyncState.Introduce`（设置 `sync.introductions`，默认开）为真时后端把已知对端列表告诉已连接会话，接收方后端调用 `sync_host.introduce`；Engine 校验会话后经 `ReplaceVouches` 存为候选（仅建议，不授信；规则见 [身份与信任](../architecture/identity-and-trust.md)）。进程内回环测试见 `engine/peerflow_test.go`，真实双子进程互通尚无。

## 6a. 外部渠道 profile（0009，进程外 RPC 子集已实现）

声明 `ExternalChannels` 的后端不走 §3 的 hello/连接授权：没有设备身份、指纹或关联审批，授权只来自 Engine 里渠道“已启用”。后端在其私有目录保存凭据，向 Engine 提供渠道的远端上下文（不透明、≤ 256 字节、秘密）和自报名称，目标是一个组；Submit 的 `Targets` 只含已启用渠道，一次发送每渠道一条目标状态。回执语义随后端声明（如微信输入法只表示服务端已接受，界面不得写“对方已收到”）。接收经 `Receive` 一次交付内存中的文本/图片（暂不含文件），由 Engine 做准入与去重。退出组请求（`LeaveChannel`）仅在声明 `remote_revocation` 时存在。组成员、回执语义字段待定。

**进程外 RPC 子集**（`plugin/rpc` 的 `Spec.ExternalChannels`/`RemoteRevocation`，`internal/pluginproc/backend.go`，`plugin/serve_backend.go`）：

- Engine→插件：`backend.submit`（`transfer_id`、`timestamp_ms`、已启用渠道 ID 的 `targets`、`parts`；每个 part 是以请求 ID 为根的 `sync_part` 流，仅内存，文件 part 跳过，总量 ≤ 16 MiB）、`backend.leave_channel`（仅声明 `remote_revocation` 时存在，成功后 Engine 才删除记录）。
- 插件→Engine：`backend.register`（登记本次 transfer 的目标，必须是已启用渠道）、`backend.report`（`queued/sending/sent/superseded/failed`，未登记目标或未知 transfer 被拒绝；失败原因文本被丢弃，只保留状态）、`backend.receive`（`receive_part` 流，准入、去重与历史来源 `external` 同进程内路径）。
- Engine 为每个 transfer 保存目标表（≤ 256），全部目标终态或进程退出后清除；进程崩溃时未终态目标按失败结束，重启后重新装载。
- 真实子进程测试：`engine/pluginchannel_proc_test.go`。P2P 的撤销屏障、`authorize_send`、接收准入与身份方法仍未实现。

## 6. 失败恢复、权限与待实现测试

后端重启后重新 Bind、完整 ApplyState、Start，重新握手获取 session_token；旧 attempt、grant、content_handle、receive_id、流和连接事件全部失效。其他后端的 binding 与在途任务不因此失效。本地已保存身份与信任不因插件崩溃清空，未完成发送按失败结束，不重试结果不确定的 dispatch；本后端 pending 历史同步摘要按历史契约转 interrupted，不覆盖其他后端的成功/失败。

秘密字段只出现在必要的专用 RPC（地址、票据、状态快照、授权 token），不得使用通用 payload dump 日志。RPC 自由文本 error 不允许透传，日志/普通事件只写插件 ID、公开对象 ID、方法与稳定错误码。不因 Go 程序受信任而省略 Engine 端的句柄、能力、代次和配额校验。

后续必须使用本地假网络/真实子进程验证：

- 普通插件越权、旧 binding/token/connection、伪造 outgoing、缺失 attempt、配对码仅发现和介绍不授信。
- 状态大于控制帧的流式传输、旧 revision、同修订不同内容、坏快照保留旧状态、撤销等待屏障与卡死后端回收。
- 同时连接替换、延迟 disconnected 不影响新连接、断网重连不重发、sync.enabled=false 取消全部内容工作但可保留控制连接/配对。
- 多目标长连接复用、授权前无 wire 字节、源文件变化/限额变化、传输中断丢连接、成功不冒充远端应用。
- receive_open 拒绝后 drain 保持连接，硬上限/溢出关闭，预留过期/重复消费，超过 32 个 part 的顺序流回收。
- 交付前撤销/挂起/过滤、hash 校验后处理、persist=false、历史失败不建立 Received、半成品和配额全部回收。
- 每种错误/取消时序的原子状态与资源清理，以及地址/票据/正文/token 不进入日志事件错误。

## Global file permission (0016)

The effective backend file policy is `Settings.Sync.Files && BackendSyncPolicy.Files`.
External peer backends receive this effective policy in `SyncState.Policy.Files`; protocol version remains 1.
Engine still checks file permission and receive limits at admission.
The 100 MB default file limit applies to manual, automatic, and restore transfers.
Default-cache reservations do not apply to custom receive files. All receives still obey transient resource limits.
See [desktop runtime](../architecture/desktop-runtime.md).

## Callback and state-publication ordering

Backend authority callbacks must not synchronously invoke `ApplyState`. A backend may wait for those callbacks to return while applying a suspended state. Reviving a revoked device remains a pending identity operation; it queues a coalesced Engine-owned state refresh. Stop rejects new refresh work and drains the refresh worker before releasing storage. User-requested revocations retain the synchronous state-application barrier.
