# Engine 公共契约 v0.1

- 状态：设计稿，正在分阶段实现；实现进度见第 14 节，未列为已实现的部分当前代码尚未符合本规范。
- 日期：2026-11-07
- 依据：[0003 独立 Engine](decisions/0003-engine-sdk.md)、[0001 配对](decisions/0001-pairing-code.md)、[0002 配置](decisions/0002-config-file.md)。
- 范围：公开 Go 包 `unic/engine` 的业务契约及桌面宿主接入；移动端沿用业务语义，具体语言绑定后续定义。

本文中的“必须”是实现和契约测试要求。“宿主”指调用 SDK 的平台集成层，不是 GUI。SDK 不直接访问系统剪贴板，不创建 UI，不提供后台常驻保证。

## 1. 边界与通用约定

1. 公开签名只能使用标准库和公开 engine 类型，不暴露 internal、SQLite、tailcat、mygo 类型。Engine 默认拥有身份、数据库、网络与内容资源；宿主负责权限、剪贴板、主线程、通知及配置文件。
2. 每个成功返回的 DTO 都是独立快照；调用者修改切片或字节不得改变核心状态。输入在方法返回后可由调用者复用；流的例外见资源章节。
3. ID 是区分大小写的不透明字符串，禁止从中推导地址、路径或时间。DeviceID 跨进程稳定；InstanceID 每次 New 更新；其余 ID 仅在所属实例有效。旧实例 ID 的请求返回 `resource_expired`。
4. 时间统一 Unix 毫秒，仅供显示；截止时间内部使用单调时钟。序号是实例内 uint64，0 表示尚无变化；跨语言序列化为十进制字符串，防止数值精度丢失。
5. 方法允许并发调用。状态变更在明确提交点原子生效；核心不在持锁期间调用宿主代码。SDK 不指定 UI 线程。
6. context 只控制本次调用和其直接 I/O，不充当实例生命周期。context 取消后，已提交操作继续存在；取消不回滚信任与成功导入的资源。
7. 普通查询和事件不含设备地址、票据、剪贴板正文、文件内容或绝对路径。内容只能显式读取，地址和配对码只能显式获取。日志和错误必须脱敏，包括第三方错误。
8. v0.1 保留 wire v2，不新增远端 ACK、断点续传、历史搜索或可靠离线投递。

## 2. 接口总览

以下是目标 Go 签名。DTO 字段由后文规定；标识类型均为独立 string 类型，枚举也是独立 string 类型。所有带 context 的方法拒绝 nil context。`Subscription` 与 `ContentReader` 是 SDK 管理的句柄，不跨语言直接导出。

```go
func New(ctx context.Context, opts Options) (*Engine, error)

func (e *Engine) Start(ctx context.Context) error
func (e *Engine) Stop(ctx context.Context) error
func (e *Engine) Close(ctx context.Context) error
func (e *Engine) SetRuntimeState(ctx context.Context, state RuntimeState) error
func (e *Engine) NotifyNetworkChanged(ctx context.Context) error
func (e *Engine) GetState(ctx context.Context) (State, error)
func (e *Engine) GetCapabilities(ctx context.Context) (Capabilities, error)
func (e *Engine) UpdateHostCapabilities(ctx context.Context, caps HostCapabilities) error
func (e *Engine) UpdateSettings(ctx context.Context, req UpdateSettingsRequest) (SettingsState, error)

func (e *Engine) CreatePairingCode(ctx context.Context) (PairingCode, error)
func (e *Engine) GetPairingCode(ctx context.Context) (PairingCode, bool, error)
func (e *Engine) CancelPairingCode(ctx context.Context) error
func (e *Engine) ExportPairingAddress(ctx context.Context) (SensitiveAddress, error)
func (e *Engine) JoinByCode(ctx context.Context, req JoinRequest) (OperationID, error)
func (e *Engine) PairByAddress(ctx context.Context, req PairRequest) (OperationID, error)
func (e *Engine) ListPeers(ctx context.Context) ([]Peer, error)
func (e *Engine) ListPending(ctx context.Context) ([]PendingPeer, error)
func (e *Engine) Approve(ctx context.Context, req ApprovalRequest) (Peer, error)
func (e *Engine) Reject(ctx context.Context, req PendingRequest) error
func (e *Engine) Unpair(ctx context.Context, id DeviceID) error

func (e *Engine) ImportContent(ctx context.Context, in ContentInput) (ContentHandle, error)
func (e *Engine) RetainContent(ctx context.Context, id ContentID) (ContentHandle, error)
func (e *Engine) ReadContent(ctx context.Context, lease LeaseID, part PartID) (*ContentReader, error)
func (e *Engine) ReleaseContent(ctx context.Context, lease LeaseID) error
func (e *Engine) SubmitContent(ctx context.Context, req SubmitRequest) (OperationID, error)
func (e *Engine) GetOperation(ctx context.Context, id OperationID) (Operation, error)
func (e *Engine) ListReceived(ctx context.Context) ([]ReceivedContent, error)
func (e *Engine) DismissReceived(ctx context.Context, id ReceiptID) error

func (e *Engine) ObserveClipboard(ctx context.Context, req Observation) (ObservationResult, error)
func (e *Engine) BeginApply(ctx context.Context, req ApplyRequest) (ApplyTicket, error)
func (e *Engine) ReportApplyResult(ctx context.Context, result ApplyResult) error
func (e *Engine) SubscribeEvents(ctx context.Context) (*Subscription, error)

func (s *Subscription) Snapshot() State
func (s *Subscription) Next(ctx context.Context) (Event, error)
func (s *Subscription) Close() error
```

`ContentReader` 实现 `io.ReadCloser`，只提供指定 part 的字节；与文件路径、URI 无关。以上签名属于设计稿，调整须同步更新本文及契约测试。

## 3. 创建、配置与运行条件

### 3.1 创建参数

| Options 字段 | 约束 |
|---|---|
| DataDir | 必填、本地私有目录，存放身份与数据库；持有进程级独占锁，冲突报 `in_use` |
| InboxDir | 可选，默认 DataDir/inbox；仅用于核心拥有的资源，不授予任意路径清理权限 |
| Settings | 完整有效配置；不从环境变量、TOML 隐式加载 |
| HostCapabilities | 初始平台能力与当前可用状态，默认所有剪贴板能力关闭 |
| ResourcePolicy | 有界磁盘、列表和操作保留配置；见第 7 节 |
| Logger | 可选结构化日志接收器；默认不输出内容与秘密，接收器不得阻塞核心 |

目录和端点 URL 是本地配置，不是内容 DTO。New 可创建目录、打开存储并初始化身份，但不访问网络；失败释放已获得的锁和资源。身份读取失败不得悄悄生成新身份。

### 3.2 Settings

字段为 `DeviceName`、`RendezvousURL`、`Limits{MaxFileBytes, MaxTotalBytes}`、`Sync{Enabled, Text, Images, Files}`。默认值与验证规则沿用 0002。空设备名使用创建时生成并稳定保存的默认名；不在每次查询读取宿主名。

`UpdateSettingsRequest{ExpectedRevision, Value}` 替换完整配置，修订不匹配报 `conflict`；验证失败无字段生效。返回 `SettingsState{Revision, Value}`。名称仅影响之后的握手；端点仅影响之后的新配对码请求；大小限制在新传输开始时取快照。在途传输不追溯。同步开关过滤规则沿用 0002。

DataDir、InboxDir 和 ResourcePolicy 只在 New 设置；桌面配置中的 inbox 修改继续显示需重启。文件加载失败信息由桌面配置层持有，不能作为 SDK 配置状态的底层错误直接转发。

`RuntimeState` 为 `active` 或 `suspended`，默认 active。suspended 禁止新网络操作并关闭现有监听和会话；active 后重新建立监听与重连。`Sync.Enabled=false` 仍保留连接，新的提交返回 `sync_disabled`，新接收读完丢弃。已开始传输不因同步开关变化回滚，应用前还须重新检查开关。

`NotifyNetworkChanged` 只触发网络重新评估并重置重连退避，不意味着网络一定可用，也不生成新身份或自动重发旧剪贴板。

### 3.3 能力模型

`HostCapabilities` 包含 `ReadFormats`、`WriteFormats`（MIME 集合）、`ReadFiles`、`WriteFiles`，以及观察与应用状态 `available | permission_required | unavailable | unsupported`。能力描述宿主当前能做什么，不代表远端支持什么。

`Capabilities` 返回宿主声明、核心支持格式和有效交集，以及因配置/挂起导致不可用的原因。初版核心支持 text/plain、text/html、image/png 和普通文件；未知格式返回 `unsupported`。能力变化不改变信任，不清空已收到的内容，也不自动触发系统读写。

## 4. 生命周期

```text
created ─Start─> starting ─成功─> running ─Stop─> stopping ─完成─> stopped
   │                └─失败─> stopping ─回收完成─> stopped              │
   └─Stop─> stopped                                                 │
created / stopped ─Close─> closed <────停止并释放完成───────────────────┘
```

- Start 仅允许 created；成功表示本地服务已启动，远端连接仍可能全部离线。重复调用报 `invalid_state`。在 starting 时 Stop 可取消启动。
- Start 期间使用调用 context 完成启动；成功后实例不依赖这个 context。启动失败必须收敛到 stopped；context 到期时若尚未回收则仍处于 stopping。
- Stop 一旦提交就持续回收，取消调用只停止等待；超时后保持 stopping，后续 Stop 可继续等待。Stop 不关闭数据库、不释放有效内容租约，允许查询和释放。
- Close 同样可取消等待；提交后拒绝新工作，最终关闭存储并释放锁。完成前不得返回成功，不得关闭仍被工作任务使用的资源。完成后仅 Stop/Close 幂等成功，其余方法报 `closed`。
- 停止后不能再次 Start；重新运行创建新实例。挂起/恢复使用 SetRuntimeState，生命周期仍为 running。
- created 允许状态/配置/能力/订阅、设备列表、内容导入读取释放。网络、观察、提交和应用要求 running；stopped 仅允许查询、内容读取释放、订阅和关闭。stopping 只允许查询、释放、Stop/Close。
- suspended 时允许查询、配置、内容导入读取释放及离线信任管理；网络配对、内容提交返回 `suspended`。停止或挂起使未完成发送失败，不保留待恢复的发送队列。
- SDK 必须跟踪全部后台任务。Stop 完成后无网络/发送任务；Close 完成后无 SDK 工作任务，不再调用日志接收器。

## 5. 配对与设备

| DTO | 必需字段/语义 |
|---|---|
| Peer | ID、Name、TrustRevision、Connection（disconnected/connecting/connected）、LastSeenMs；已配对不等于已连接 |
| PendingPeer | ID、Name、RequestRevision、Source（direct/code/introduction）、IntroducedByID（可空）、Fingerprint、SeenAtMs |
| PairingCode | Code（NNN-NNN）、ExpiresAtMs；敏感，只在显式方法返回 |
| PairRequest / JoinRequest | RequestID 与 Address / Code；敏感输入不得进入 Operation DTO |
| ApprovalRequest | DeviceID、RequestRevision、ExpectedFingerprint；用于校验批准的是用户看到的请求 |
| PendingRequest | DeviceID、RequestRevision；拒绝过期请求返回 conflict |

- CreatePairingCode 成功才替换旧码；失败保留旧码。配对码修改串行化；Cancel 返回时本地已失效，远端 consume 是尽力操作，不影响本地安全。有效期、token、指纹和限速分类按 0001。
- GetState 仅显示 `PairingCodeState{Active, ExpiresAtMs}`，不包含码本身。GetPairingCode 过期时返回 exists=false。
- Join/Pair 接受请求后返回 OperationID；operation 的 type 为 pairing，phase 为 resolving/connecting/handshaking 或终态 succeeded/failed。成功结果为 PeerID、Fingerprint、AwaitingRemoteApproval。等待批准是成功的配对结果，不意味着已经建立同步会话；此 operation 随后不变，后续状态查询 Peer。
- 入码方保留主动配对信任语义；出码方必须形成待批准请求。未批准和仅被介绍的设备不得接收内容。批准操作要求指纹一致；不一致报 conflict，宿主必须重新展示，而非自动重试。
- Approve 的提交点是信任持久化成功；不会等待联网。Reject 持久化拒绝并按 0001 屏蔽重复介绍，直接再次连接可产生新请求修订。
- Unpair 对未配对 ID 幂等成功；持久化移除信任并使旧会话/重连代次失效后返回，旧会话不得继续接收、发送新内容，网络资源随后关闭。已完整接收的本地资源保留；未来直接连接仍须重新审批。
- 身份不一致、协议版本不兼容均失败，不覆盖原有 Peer 身份；失败可能已产生对方待审批项，不能声称跨设备事务回滚。

## 6. 内容模型与发送

### 6.1 输入与描述

`ContentInput` 包含 `Representations[]` 与 `Files[]`。每个输入项提供 MIME（文件可缺省为 application/octet-stream）、声明 Size 及 io.Reader；文件还含展示 Name。Reader 由调用者关闭，Import 返回前调用者不得修改或并发读取；SDK 不保留 Reader。

- 文本和 HTML 是有效 UTF-8；PNG 须为可解码 PNG。一个内容最多包含每种 MIME 一个内存表示，重复 MIME 或空内容报 invalid_argument。不同表示被视为同一内容的替代表示。
- 不遍历文件夹，不接受目录输入。文件名称只用作展示和安全落盘名称，不用作路径；消除目录分隔、`.`、`..` 等路径语义，重复名可存在且 PartID 不同。
- Import 按声明大小读取并验证流恰好结束，短读、额外字节和读取错误均失败；硬上限沿用 wire（内存单表示 64 MiB、单文件 2 GiB、总计 4 GiB、part 数 4096）。用户限制在导入和开始传输时也验证。
- Import 返回 `ContentHandle{Content: ContentDescriptor, LeaseID}`。Descriptor 包含 ID、各 part 的 PartID、Kind（representation/file）、MIME、Name、Size 和总大小；不含字节、路径和 hash。
- 文件与表示可同时导入/传输；自动应用沿用桌面“文件优先”规则。宿主不支持的表示可跳过；无任何可应用表示时报告 unsupported，不假装成功。

### 6.2 提交与操作

`SubmitRequest{RequestID, LeaseID, Origin}`：Origin 为 `explicit` 或 `clipboard`。clipboard 提交由 ObserveClipboard 产生，外部调用 SubmitContent 只接受 explicit，避免绕过观察顺序。当前目标是提交瞬间已配对且 connected 的设备，不含随后上线设备。

- 无在线目标报 `no_targets`，不创建排队任务，不做离线补发。
- 提交点为创建 operation 并持有内部内容引用；返回后调用者可释放 LeaseID。显式提交即使内容相同也可发送一次。
- `Operation` 含 ID、RequestID、Kind、CreatedAtMs、UpdatedAtMs、Phase、Targets、Outcome、Error。发送 Targets 中每项有 PeerID、state、结构化错误；state 为 queued/sending/sent/failed/superseded。运行中总 Phase 为 queued/sending，全部目标终止后为 completed；Outcome 为 success/partial/failed/superseded，只有全部 sent 才为 success，全部 superseded 才为 superseded，sent 和其他终态混合为 partial，其余为 failed。
- 每个设备最多一个在途、一个待发内容；新提交替换同设备未开始的待发项，被替换目标标记 superseded。不能静默丢任务。
- sent 仅表示完整字节写出本地传输。不能显示为“对方已复制”或“已应用”。连接中断的当前发送失败，重连不自动重发。
- Submit、Join、Pair 的 RequestID 必填，实例内幂等：保留期内相同类型和语义参数返回相同 OperationID，不重复执行；参数不同报 conflict。重复识别先于租约有效性验证；内部仅保留敏感参数的不可逆校验值，不存储到公共 DTO。超出保留期后调用者须用新 RequestID，不承诺去重。
- 其他方法除明确声明幂等外，不提供隐式重试；context 超时不能被解释为“操作一定未提交”。

## 7. 资源租约与接收列表

ContentID 标识资源，LeaseID 表示一个持有者。Import 和 Retain 每次创建新的 LeaseID；ReadContent 必须使用有效租约，读取句柄另持有内部引用直至 Close。文件清理不能使已打开的读取句柄中途失效；实例 Close 会中止读取，调用者仍应 Close 读取句柄。

ReleaseContent 按 LeaseID 幂等，释放已知且已释放的租约成功；未知 ID 报 not_found，旧实例 ID 报 resource_expired。不能用 ContentID 模拟引用计数，避免重复释放扣减其他调用者引用。释放最后一个外部引用不影响仍在进行的传输。

`ResourcePolicy` 为创建选项，默认值：

| 字段 | 默认值 | 规则 |
|---|---|---|
| MaxStoredBytes | 8 GiB | 核心内容的逻辑字节总额，导入和接收均占用；必须至少容纳 Settings 的 MaxTotalBytes |
| MaxReceived | 32 | 接收列表条目上限，必须 > 0 |
| ReceivedTTL | 30 分钟 | 从收到完整内容起计时；不是可靠投递承诺 |
| MaxOperations | 1024 | 同时保留的操作上限；未终止操作不得驱逐 |
| OperationTTL | 30 分钟 | 终态后最多保留时间；容量压力可提前驱逐最早终态操作及其 RequestID |
| MaxSubscriptions | 16 | 超出报 resource_exhausted |
| EventBuffer | 128 | 每订阅队列容量，必须 > 0 |

- 导入先按声明总大小预留配额，失败释放；接收在完整分配前检查。配额不足只清理已过期或可驱逐资源；不能删除租约、读取句柄或在途传输持有的资源。仍不足则失败；入站读完丢弃并保持协议流同步。
- `ReceivedContent{ReceiptID, Content, PeerID, ReceivedAtMs, ExpiresAtMs, ApplyState}`。完整接收且验证成功、经同步过滤后非空才入列表，列表持有内部引用；部分文件和失败接收不得暴露。
- 接收列表满时驱逐最早条目；到期或 DismissReceived 移除列表引用。正在应用的条目不能驱逐，容量被全部占用时丢弃新接收并报告资源不足。
- RetainContent 将仍有效的 ContentID 转为宿主持有租约；列表移除不影响该租约。若已清理返回 resource_expired。DismissReceived 对不存在条目幂等成功。
- 桌面已写入剪贴板的文件须持有租约，直到宿主确认被替换。ReadContent 不导出路径；桌面适配可在自己拥有的目录物化文件，并负责清理，不能要求 SDK 返回私有路径。
- 资源、接收列表、操作和租约不承诺跨进程恢复。Close 使租约失效；桌面应先结束剪贴板适配。后续启动只能清理核心自己的临时资源和明确迁移的旧 clip-*，不清理用户任意文件。
- State 和列表不保证永久保留终态记录；被移除对象的查询返回 not_found/resource_expired，消费者不得据此判断是否曾成功。

## 8. 剪贴板观察、应用与防回环

### 8.1 本地观察

`Observation{Epoch, Sequence, Mode, LeaseID, AppliedToken}`。宿主每次启动/恢复观察创建新 Epoch（不透明随机 ID），Sequence 从 1 严格递增。Mode 为 baseline/change；首次必须 baseline，且只能在上一个 epoch 停止观察后切换。

- baseline 只建立本地内容基线，不发送；空剪贴板以空 LeaseID 表示。恢复不广播挂起期间遗留内容。
- change 由 SDK 比较规范化内容标识；无变化或清空只更新基线。变更且允许同步时按提交规则创建操作。结果为 `ObservationResult{Disposition, OperationID}`；Disposition 为 baseline/unchanged/empty/echo/submitted/disabled/no_targets。
- 同 epoch 的重复 Sequence 与相同参数幂等返回原结果（仅保证最近一次，比较先于租约有效性检查）；更旧序号或重复序号但内容不同报 conflict。序号跳跃允许，但不能据此推导丢失内容。
- disabled/no_targets 仍推进观察基线，不在重新启用/联网后自动补发。导入失败和观察非法则不推进。
- 去重标识内部沿用 wire 规范；文件同名同大小可能被视为相同，不宣称文件字节校验。显式发送不受这项观察去重限制。

### 8.2 应用流程

1. 宿主查到接收条目后调用 `BeginApply{ReceiptID, ExpectedEpoch, ExpectedSequence}`。SDK 检查条目、同步策略、能力与当前观察版本，返回 `ApplyTicket{Token, ContentID, LeaseID}`；版本不符报 conflict，禁止用过期请求覆盖较新的本地复制。
2. 同一宿主实例只能有一个未完成 ticket；并发 BeginApply 报 in_use。ticket 持有独立租约，锁住接收条目不被驱逐；宿主始终串行化应用和观察分发。
3. 宿主在平台线程写入内容；若平台支持变更计数/来源标记，须检查写入和回读之间是否出现其他复制。无法确认来源的回读不得标记为本次远端应用。
4. `ApplyResult{Token, Outcome, BaselineLeaseID, Epoch, Sequence, ErrorCode}`：Outcome 为 applied/failed/deferred。applied 需提供实际写入后回读基线（空剪贴板可为空 LeaseID）和新的观察序号；SDK 原子更新观察基线并完成 ticket，返回后宿主释放回读租约。只有经宿主确认来自本次写入的结果才能推进基线。
5. failed/deferred 不推进成功基线；错误只传稳定 code，不传平台原始错误。所有结果结束 ticket 并解锁条目；再次应用必须重新 BeginApply。ticket 租约仍由宿主显式释放，文件继续在剪贴板时必须保留。

ReportApplyResult 对同 token、相同结果幂等；不同结果或过期代次报 conflict。ticket 未完成期间，宿主可携带 AppliedToken 报告确认属于本次写入的观察，SDK 返回 echo 且不广播；其他本地变化正常处理，并使旧 ticket 的成功确认冲突。宿主不可仅因“时间接近”就给变化附 token。

挂起、停止、拒绝相应能力或接收策略关闭时，未完成 ticket 失效，已给宿主的租约仍需释放。ticket 不设置隐式超时；宿主必须在退出应用流程时报告 failed/deferred，避免长期占用应用槽位。已实际写入但确认冲突时，不擅自回滚系统剪贴板，宿主重新读取并提交真实观察。

在缺少系统来源标记、无法原子写入并回读的平台，不保证完全消除外部应用竞争；应缩小串行窗口，遇到歧义保留为本地变化，不能吞掉用户新复制。此限制必须进入平台适配测试与说明。

CLI“写入并发送”由宿主执行本地写入、回读、建立抑制回传基线，再调用 SubmitContent(explicit)；失败阶段分别呈现。SDK 不提供一个假装跨系统剪贴板和网络原子提交的方法。

## 9. 状态与事件

`State` 包含 InstanceID、Revision、Lifecycle、RuntimeState、Self{ID, Name}、SettingsState、Capabilities、Peers、Pending、PairingCodeState、保留的 Operations、Received 和 ActiveApply。DTO 不含任何 LeaseID；租约必须显式获取。State 内容来自一个一致性提交点。

- 每个对外可观察变更递增 Revision，并生成一个 Event；一次原子变更可包含多个对象引用。`Event{InstanceID, Revision, AtMs, Kind, Objects}`，Objects 为类型与 ID，消费者查询详情。
- Kind 为 lifecycle/runtime/settings/capabilities/peers/pending/pairing_code/operations/received/apply/resource_warning。事件不作为可靠投递任务；真正工作项保存在查询状态中。resource_warning 的非持久提示丢失不影响任务恢复。
- SubscribeEvents 在同一提交点注册并复制初始 State；Snapshot 返回该快照，Next 只返回 revision 大于该快照的变化，避免查询与订阅间隙。传入 context 仅控制创建订阅；后续由订阅 Close 管理生命周期。
- 队列溢出后原子丢弃该订阅积压，标记失步；Next 必须返回一次 `resync_required` 并终止订阅。消费者新建订阅，读取新 Snapshot；不提供旧事件重放。通知不能阻塞核心。
- 每个 Subscription 只允许一个并发 Next，违反报 in_use；Close 幂等并唤醒 Next，后续返回 io.EOF。Engine Close 也结束所有订阅。Stop 后仍可订阅/查询直至 Close。
- 配对码到期、列表到期等变更必须主动更新状态，不能只靠 GUI 自行推断。相同实例内完整连续订阅的 Revision 可用于检测遗漏，跨实例禁止比较。

## 10. 错误契约

`Error{Code, Message, Retryable, RetryAfterMs}` 可用 errors.As 识别；Message 已脱敏、可展示但不能用于程序分支。取消与超时同时支持 errors.Is(context.Canceled/DeadlineExceeded)。Retryable 仅表示将来条件可能改善，不授权自动重复非幂等操作。

| code | 场景 |
|---|---|
| invalid_argument | DTO、格式、大小声明、UTF-8 等无效 |
| invalid_state / closed | 生命周期不允许 / 实例关闭 |
| in_use / conflict | 目录或应用/读取者被占用 / 版本、幂等参数或代次不匹配 |
| not_found / resource_expired | 对象未知或已驱逐 / 旧实例资源或租约已失效 |
| unsupported / permission_denied | 平台或核心不支持 / 当前系统拒绝权限 |
| sync_disabled / suspended / no_targets | 当前策略禁用 / 宿主挂起 / 无在线发送目标 |
| limit_exceeded / resource_exhausted | 协议或用户大小限制 / 磁盘及有界队列配额不足 |
| unavailable / timeout / cancelled | 外部服务、网络或存储暂不可用 / 超时 / 取消 |
| identity_mismatch / protocol_mismatch | 设备身份或 wire 版本不符 |
| incomplete / invalid / expired / rate_limited | 0001 的配对码分类（桌面 HTTP 可映射为旧带连字符形式） |
| resync_required | 订阅溢出，必须重新订阅 |
| internal | 无法安全细分的内部错误 |

本地已知失效与过期资源可返回 resource_expired；无法区分“从未存在”和“已清理”时允许 not_found。错误中的对象标识仅可使用公开 ID，不包含秘密或文件路径。必要内部诊断另用不含用户内容的关联 ID。

## 11. 桌面迁移映射

| 现有入口 | 新职责 |
|---|---|
| daemon 打开 Store、创建身份、启动 tailcat | Engine New/Start/Close 接管默认实现和所有权 |
| engine.watch / Board.Read / Board.Write | 桌面剪贴板适配负责轮询、串行写入和 Observe/Apply 协议 |
| SetLocal 等待轮询广播 | 宿主显式写入、回读基线、SubmitContent，一次用户请求只发送一次 |
| api.Handler 直接读 Store/engine 内部类型 | 转换公开 DTO，保留 /v1 的旧响应；地址通过显式 Export 获取 |
| config.Manager | 保留 TOML 与热加载，映射 Settings 的完整更新与错误展示 |
| 直接导出接收文件路径 | 桌面适配物化受控副本并持有/释放租约 |
| sent / received 人类摘要事件 | 映射操作状态及显式内容视图，事件不得携带正文摘要 |

旧 /v1/status 的 addr 兼容字段只在现有显式查询中获取，禁止复制进新 State。旧事件格式可映射 kind，但不能为了兼容恢复正文或秘密日志。文件物化副本的字节开销由桌面层另行限制，不包含在 SDK 的逻辑配额内。

## 12. 契约验收清单

每项需要测试结果支撑；文档完成不代表实现已通过。

- 生命周期：重复 Start、启动失败回收、Start/Stop 竞争、Stop 超时后继续等待、Close 唤醒订阅并释放目录锁。
- 并发与数据隔离：DTO 修改不影响核心、流取消清理、Settings CAS 冲突、同目录重复实例失败。
- 信任：码仅发现、双方指纹一致、过期审批冲突、介绍不授信、Unpair 后旧会话失效、重启保留身份与拒绝记录。
- 发送：RequestID 幂等与冲突、离线不排队、每设备替换待发、部分成功、网络挂起失败、sent 不冒充远端应用。
- 资源：读取和传输期间释放租约、重复释放、配额预留回滚、列表过期/驱逐、应用锁定条目、重启只清理自有资源。
- 剪贴板：启动/恢复基线、远端格式补全、防重复显式发送、写入失败、新本地复制穿插、过期 token、系统来源不明时不吞变化。
- 事件：订阅初始快照无窗口、序号连续、队列满必报 resync_required、慢消费者不阻塞、关闭结束等待。
- 配置与隐私：沿用 0002 的热加载及过滤语义，超限读完丢弃，事件/错误/日志不泄露地址、票据和正文。
- 完成桌面构建、`go test -race ./...` 和双节点文本/HTML/PNG/文件回归；移动端编译与真机不属于本阶段验收。

## 13. 版本与后续修订

本文 v0.1 是尚未实现的业务契约；公开稳定前可根据桌面落地调整，但必须同步文档和测试。0003 描述架构与实施顺序，本文描述具体调用语义；发现矛盾时先修订文档再实现，不能默认以代码覆盖约定。

SDK、桌面 HTTP、wire 分别版本化。未来移动端绑定需额外规定数值/字节映射、句柄关闭、取消、线程投递、分块读取和平台来源标记的可用性；这些工作不改变已定义的信任与成功语义。引入 ACK、摘要、可靠投递或跨进程内容恢复须另写设计决策。

## 14. 实现进度与落地时的补充约定

公开包 `unic/engine` 分两步落地。每一步完成后更新本节；契约测试位于 `engine/engine_test.go`。

**第一步（已实现并有测试）**：创建与目录独占锁、`Start`/`Stop`/`Close`、`SetRuntimeState`、`NotifyNetworkChanged`、`GetState`、`GetCapabilities`、`UpdateHostCapabilities`、`UpdateSettings`、全部配对与设备方法（`CreatePairingCode` … `Unpair`）、`GetOperation`、`SubscribeEvents`/`Subscription`、错误模型。

**第二步（未实现）**：内容导入与租约（`ImportContent`、`RetainContent`、`ReadContent`、`ReleaseContent`）、`SubmitContent`、接收列表、`ObserveClipboard`/`BeginApply`/`ReportApplyResult`，以及 `State` 中对应的 `Received`、`ActiveApply` 和发送类操作；随后迁移桌面 daemon。在此之前，公开 Engine 不同步剪贴板内容，收到的内容会被读完并丢弃；桌面仍使用旧入口。

落地时确定的细节（以此为准，必要时回改前文）：

1. **实例目录**：`DataDir/unic.db` 与桌面现有数据库同名同表，设备身份沿用 `device.id`；独占锁是 `DataDir/unic.lock`（flock / LockFileEx），进程退出由系统释放。默认设备名在首次创建时取主机名并保存（`device.default_name`），之后不随主机名变化。
2. **配置输入**：`Options.Settings` 全零值等同 `DefaultSettings()`；会合服务地址只来自 `Settings.RendezvousURL`，空则用生产服务，不读环境变量（桌面宿主自行把 `UNIC_RENDEZVOUS_URL` 填入）。`Settings` 不含 inbox，它是创建选项 `InboxDir`。
3. **日志**：`Options.Logger` 是 `*slog.Logger`。底层（tailcat、会话层）的日志会包含地址和设备名，因此在 SDK 模式下被丢弃；SDK 只记录自己的脱敏行（生命周期与错误码）。
4. **待批准项修订号**：`PendingPeer.RequestRevision` 由引擎在内存中维护，设备首次提出请求或被拒绝后再次提出请求时变化；已在等待的设备在后台重连不改变修订。`Peer.TrustRevision` 取配对时间（Unix 秒）。因此同一秒内“拒绝后立刻再次请求”可能得到相同修订，这一窗口接受。
5. **`PendingPeer.IntroducedByName`**：新增字段，介绍人的名称（非身份），桌面界面需要展示“由谁介绍”。`IntroducedByID` 暂不提供，存储里只有名称。
6. **连接状态**：`Peer.Connection` 目前只产生 `disconnected` / `connected`；`connecting` 和操作阶段 `handshaking` 保留，尚未发出。`JoinByCode` 的阶段是 `resolving → connecting → succeeded|failed`，`PairByAddress` 从 `connecting` 开始。
7. **审批输入**：`Approve` 要求 `ExpectedFingerprint` 非空且与当前一致，空值按不一致处理。`Reject` 只校验修订。
8. **运行条件**：`CreatePairingCode`、`JoinByCode`、`PairByAddress`、`NotifyNetworkChanged` 要求 running 且非 suspended，分别报 `invalid_state` / `suspended`；重复的 `RequestID` 即使在 suspended 时也返回原操作。`GetPairingCode`、`CancelPairingCode`、设备列表与审批在任何未关闭状态可用。`ExportPairingAddress` 要求 running（挂起时仍可用）。
9. **事件合并**：会话层的“设备上线/下线”只产生 `peers` 事件；“配对或批准”产生 `peers` 与 `pending` 两个事件（各占一个修订）。事件的 `Objects` 目前只用于操作。
10. **挂起恢复**：恢复时重新监听并重连全部已配对设备；`NotifyNetworkChanged` 让未连接设备跳过退避立即重试。
