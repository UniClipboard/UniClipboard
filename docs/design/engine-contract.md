# Engine 公共契约 v0.1

> 2026-11-08 目标修订：[0008](decisions/0008-device-identity-and-backend-bindings.md) 与[设备契约](device-contract.md) 统一设备身份、设备级信任与按后端实例授权。下文 Peer 单 Connection、配对请求/结果、ID 指纹及旧实现登记不构成完整目标 API；须同批替换 SDK/宿主/测试，不保留兼容层。普通设备列表每设备一行并展开多个关联，秘密仍仅显式导出。

- 状态：设计稿，正在分阶段实现；实现进度见第 14 节，未列为已实现的部分当前代码尚未符合本规范。
- 日期：2026-11-07
- 依据：[0003 独立 Engine](decisions/0003-engine-sdk.md)、[0001 配对](decisions/0001-pairing-code.md)、[0002 配置](decisions/0002-config-file.md)。
- 范围：公开 Go 包 `github.com/uniclipboard/unic/engine` 的业务契约及桌面宿主接入；移动端沿用业务语义，具体语言绑定后续定义。

本文中的“必须”是实现和契约测试要求。“宿主”指调用 SDK 的平台集成层，不是 GUI。SDK 不直接访问系统剪贴板，不创建 UI，不提供后台常驻保证。

## 1. 边界与通用约定

1. 公开签名只能使用标准库和公开 engine 类型，不暴露 internal、SQLite、tailcat、mygo 类型。Engine 默认拥有身份、数据库、网络与内容资源；宿主负责权限、剪贴板、主线程、通知及配置文件。
2. 每个成功返回的 DTO 都是独立快照；调用者修改切片或字节不得改变核心状态。输入在方法返回后可由调用者复用；流的例外见资源章节。
3. ID 是区分大小写的不透明字符串，禁止从中推导地址、路径或时间。DeviceID 与 0004 扩展的 HistoryID 跨进程稳定；InstanceID 每次 New 更新；其余 ID 仅在所属实例有效。旧实例 ID 的请求返回 `resource_expired`。
4. 时间统一 Unix 毫秒，仅供显示；截止时间内部使用单调时钟。序号是实例内 uint64，0 表示尚无变化；跨语言序列化为十进制字符串，防止数值精度丢失。
5. 方法允许并发调用。状态变更在明确提交点原子生效；核心不在持锁期间调用宿主代码。SDK 不指定 UI 线程。
6. context 只控制本次调用和其直接 I/O，不充当实例生命周期。context 取消后，已提交操作继续存在；取消不回滚信任与成功导入的资源。
7. 普通查询和事件不含设备地址、票据、剪贴板正文、文件内容或绝对路径。内容只能显式读取，地址和配对码只能显式获取。日志和错误必须脱敏，包括第三方错误。
8. v0.1 保留 wire v2，不新增远端 ACK、断点续传、全文检索或可靠离线投递；本机历史基础筛选见历史扩展契约。

### 0007 多后端同步扩展（待实现）

[0007](decisions/0007-multiple-sync-backends.md) 确定全局 sync.enabled、各实例启用、各后端独立的 text/images/files/自动文件阈值三层控制，允许多个后端并发，结果/目标/资源按后端隔离。目标 Settings.Sync 只保留 Enabled，新增 BackendPolicies 映射；下文旧 SyncSettings 类型字段及过滤/在途语义描述既有实现，接入时须同批迁移 SDK、宿主与测试。全局关闭立即取消所有后端内容工作，不能被手动发送绕过；本机历史、处理与 Action 保留。P2P 连接 API 仅为 peer_sessions 扩展，WebDAV 对象协议另定。

### 外部渠道扩展（已实现，后端接入待定）

[0009](decisions/0009-external-channels.md) 的数据模型与接收路径已实现：`Channel` DTO（无地址/指纹/远端上下文）、`AddChannel / ListChannels / GetChannel / RenameChannel / SetChannelEnabled / SetChannelAutoApply / RemoveChannel`（均带 ExpectedRevision 做 CAS，变化发 `ObjectRef{Type:"channel"}` 与 `EventDevices`）和混排的 `ListPeers`（`Peer{Kind: device|channel}`）。发送目标类型 `group`，一次发送一条目标状态；提交时只把已启用渠道交给后端，后端登记未启用渠道返回 invalid_argument。接收经 `plugin.Receiver` 进入与设备相同的准入、received 流水线、历史与去重，来源为 `external`，`ReceivedContent` 带 `ChannelID`（`PeerID` 为空）与到达时的 `AutoApply`；引擎不写剪贴板，宿主据 `AutoApply` 决定。`RemoveChannel`：后端未声明 `remote_revocation` 时只删本地；声明时（须实现 `plugin.ChannelLeaver`）先停用、请求退出，成功才删除，失败保留并标记 `remove_failed`。

### 0004 扩展边界（待实现）

[0004](decisions/0004-clipboard-pipeline-lua.md) 已确定：本机持久历史和处理流水线均由 Engine 管理，业务实现放在 `engine/`；宿主只经 SDK 访问，不另建历史或流水线。[本机历史契约](history-contract.md) 已确定历史 API、筛选分页、稳定 ID、内容租约、更新/删除、容量与过期、接收列表映射、历史应用和事件恢复。该契约为本文的目标扩展，尚未实现；其明确声明的历史语义优先于本文既有实例内接收列表约定。临时资源启动清理不得删除有效历史；标签与过期不进 wire，不承诺重启后的待应用可靠投递。

### 流水线提交扩展（待实现）

[流水线提交契约](pipeline-contract.md) 确定新增 `CommitContent/GetSubmission`、提交结果与网络结果分离、独立请求幂等表、旧 SubmitContent 门面和 ObservationResult 扩展。无目标、同步关闭或无后端仍可本机 commit；旧发送错误不能用于判断历史是否保存。以下既有发送/观察章节描述原 v0.1 API，接入 0004 时按该扩展同步迁移代码、宿主和测试。[Action 契约](clipboard-action-contract.md) 取代早期的自动写回设计：处理器只标注，ListActions/RunAction 由用户主动触发，结果成为 origin=action 的新历史条目并经 BeginApplyHistory 应用；RPC 核心帧与控制载荷见[RPC 核心协议](plugin-rpc-contract.md)，[后端业务 DTO 与撤销屏障](sync-backend-contract.md) 已定义，实现及互操作测试仍待完成。

三类入口的状态转换、Stop/策略与 commit 的序列化屏障、Received 槽位与成功去重记录的发布、引用/配额交接及重启边界见[流水线契约 §7](pipeline-contract.md#7-状态转换与资源交接)。公开 submission Phase 不增加内部准备状态；接收完成、应用和历史删除不能混作一个终态。

### 后端装配与身份初始化扩展（待实现）

按[装配契约](backend-migration-contract.md) 生成并持久化 Engine 设备身份与各实例独立传输身份；不导入旧 tc.*，不保留旧地址或配对记录。Engine 持有全局设备/信任与关联；插件持有传输凭据，不能取得设备私钥。移除公开 Network 并显式装配程序，零后端为本机模式，默认 tailcat 由桌面发行清单提供。

### Lua 运行时扩展（待实现）

[Lua 运行时契约](lua-runtime-contract.md) 确定增强 VM、逐项白名单、执行/编译/原生操作/逻辑分配预算及失败状态处置。四项上游探针已通过，不代表增强预算或完整沙箱通过验收；不使用 SetMx，不宣称精确堆内存隔离。运行时细节不进入公开 SDK 类型。

### SDK 类型与错误扩展（待实现）

[SDK 扩展类型契约](sdk-extension-contract.md) 确定历史/提交/Action DTO、可选值、插件装配及 Lua 源码输入、标准策略唯一存储、严格 Config 校验和错误分类。目标新增 invalid_config/plugin_panic，与已定义 RPC 对齐；既有 errors.go 尚无这两个码，不能宣称已实现。各阶段同步代码、适配和实现登记，内部类型不进入公开 SDK。

### 插件与同步后端扩展（待实现）

插件化后端的状态快照、审批与会话 token、传输前授权、接收准入/流和撤销生效屏障见[同步后端业务契约](sync-backend-contract.md)。该扩展要求撤销成功前取得后端确认；外部后端可终止回收进程，builtin 须关闭端点并确认任务退出，不能确认则停止同步服务并报告失败；下文既有 Unpair 的异步资源关闭说明只描述当前进程内实现。

[插件契约 v0.1](plugin-contract.md) 定义 Go/Lua 统一插件与最小模板。builtin Go 插件由宿主显式传入进程内实例，外部 Go 插件通过独立常驻进程/RPC 接入，安装优先平台预编译，缺失时可 --build 构建插件自身，不重建宿主；Engine 拥有注册、生命周期、配置、串行处理、commit 和操作状态；tailcat 网络、会话与传输封装为默认 builtin Go 插件 `builtin.tailcat`（`internal/plugins/tailcat/`，与 `builtin.text` 独立），公共配对/发送/接收 API 保持门面，审批、信任和策略最终由 Engine 控制。通知总线不承担同步调度。默认后端按 0008 实现统一设备身份，不保留旧 wire v2 兼容要求；无后端允许本机历史工作。插件 API 和同步流式 DTO 尚未实现，不计入第 14 节已完成功能。

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
func (e *Engine) CreatePairingCodeVia(ctx context.Context, backend PluginID) (PairingCode, error)
func (e *Engine) ExportPairingTicketVia(ctx context.Context, backend PluginID) (SensitiveTicket, error)
func (e *Engine) ListCandidates(ctx context.Context) ([]Candidate, error)
func (e *Engine) PairCandidate(ctx context.Context, req PairCandidateRequest) (OperationID, error)
func (e *Engine) IgnoreCandidate(ctx context.Context, deviceID string, ignored bool) error

func (e *Engine) ImportContent(ctx context.Context, in ContentInput) (ContentHandle, error)
func (e *Engine) RetainContent(ctx context.Context, id ContentID) (ContentHandle, error)
func (e *Engine) ReadContent(ctx context.Context, lease LeaseID, part PartID) (*ContentReader, error)
func (e *Engine) LocalFilePath(ctx context.Context, lease LeaseID, part PartID) (string, error)
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

`ContentReader` 实现 `io.ReadCloser`，只提供指定 part 的字节；与文件路径、URI 无关。以上签名属于设计稿，调整须同步更新本文及契约测试。历史扩展的 `ListHistory/GetItem/RetainHistoryContent/UpdateItem/DeleteItem/BeginApplyHistory` 签名与 DTO 见[本机历史契约 §2](history-contract.md#2-标识描述与-api)。

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

DataDir、InboxDir 和 ResourcePolicy 只在 New 设置；历史扩展另有 `Options.History`，见历史契约。桌面配置中的 inbox 修改继续显示需重启。文件加载失败信息由桌面配置层持有，不能作为 SDK 配置状态的底层错误直接转发。

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
- `CreatePairingCodeVia` / `ExportPairingTicketVia` 针对指定 peer_sessions 后端实例签发配对码/完整票据（票据携带该实例 ID `be`，入码方只在同一实例上连接）；无 Via 的方法使用默认 tailcat。
- 设备介绍（[0012](decisions/0012-device-introductions.md)）：`ListCandidates` 返回已配对设备推荐的候选（`Candidate{DeviceID, BackendID, Name, Introducers, LastSeenMs, Ignored, Requested, RequestedUntilMs}`，名称未经验证，不含地址与指纹）。`PairCandidate` 返回 pairing 类 OperationID，立即尝试一次并在 24 小时内退避重试，仍须对方批准、双方核对指纹；`IgnoreCandidate` 隐藏/恢复。候选变化发 `ObjectRef{Type:"candidate"}` 与 `EventDevices`。设置 `sync.introductions`（默认开）关闭时不接受介绍也不发送介绍。介绍只是建议，不授信。
- 身份不一致、协议版本不兼容均失败，不覆盖原有 Peer 身份；失败可能已产生对方待审批项，不能声称跨设备事务回滚。

## 6. 内容模型与发送

### 6.1 输入与描述

`ContentInput` 包含 `Representations[]` 与 `Files[]`。每个内存表示提供 MIME、声明 Size 及 io.Reader。每个文件提供展示 Name、MIME（可缺省为 application/octet-stream）、声明 Size，以及二选一的来源：`Reader`（流，复制进核心）或 `Source`（引用）。`Source` 是宿主实现的 `FileSource{Open(ctx) (io.ReadCloser, FileMeta, error)}`，`FileMeta{Size, Version}`，Version 是宿主给的不透明变更标记（桌面可用修改时间）。两者都给或都不给报 invalid_argument。Reader 由调用者关闭，Import 返回前调用者不得修改或并发读取；SDK 不保留 Reader。公开 API 不接收路径。

引用语义：导入时只调用一次 Open 校验大小等于声明值，不读取内容，不计入 MaxStoredBytes。开始传输和 ReadContent 时再次 Open，大小或 Version 与导入时不同、文件已不存在，均以 `source_changed` 失败，不重试、不补发。宿主必须在内容被租约、操作或读取句柄引用期间保持文件可打开。

- 文本和 HTML 是有效 UTF-8；PNG 须为可解码 PNG。一个内容最多包含每种 MIME 一个内存表示，重复 MIME 或空内容报 invalid_argument。不同表示被视为同一内容的替代表示。
- 不遍历文件夹，不接受目录输入。文件名称只用作展示和安全落盘名称，不用作路径；消除目录分隔、`.`、`..` 等路径语义，重复名可存在且 PartID 不同。
- Import 按声明大小读取并验证流恰好结束，短读、额外字节和读取错误均失败；硬上限沿用 wire（内存单表示 64 MiB、单文件 2 GiB、总计 4 GiB、part 数 4096）。用户限制在导入和开始传输时也验证。
- Import 返回 `ContentHandle{Content: ContentDescriptor, LeaseID}`。Descriptor 包含 ID、各 part 的 PartID、Kind（representation/file）、MIME、Name、Size、是否引用（Referenced）和总大小；不含字节、路径和 hash。
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

实例内的 ID 都带有实例前缀，以便区分旧实例（resource_expired）与未知（not_found）。ContentID 标识资源，LeaseID 表示一个持有者。Import 和 Retain 每次创建新的 LeaseID；ReadContent 必须使用有效租约，读取句柄另持有内部引用直至 Close。文件清理不能使已打开的读取句柄中途失效；实例 Close 会中止读取，调用者仍应 Close 读取句柄。

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
- 桌面已写入剪贴板的文件须持有租约，直到宿主确认被替换。ReadContent 不导出路径。需要把接收的文件交给系统剪贴板的宿主调用 `LocalFilePath(lease, part)`，它只对核心拥有的文件返回绝对路径（引用的文件宿主本来就知道路径，返回 unsupported），路径仅在租约有效期间有效，不进入任何 DTO、事件或日志。宿主据此可以不再复制一份；也可以自行物化并负责清理。
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
| source_changed | 引用的文件已被修改或不存在 |
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
| 直接导出接收文件路径 | 桌面适配持有租约，经 `LocalFilePath` 取得路径写入系统剪贴板，不再复制 |
| sent / received 人类摘要事件 | 映射操作状态及显式内容视图，事件不得携带正文摘要 |

旧 /v1/status 的 addr 兼容字段只在现有显式查询中获取，禁止复制进新 State。旧事件格式可映射 kind，但不能为了兼容恢复正文或秘密日志。桌面用引用导入本地文件、用 `LocalFilePath` 使用接收文件，所以不产生额外副本；只有宿主自行物化时的开销才由桌面层另行限制，不包含在 SDK 的逻辑配额内。

## 12. 契约验收清单

每项需要测试结果支撑；文档完成不代表实现已通过。历史扩展另须通过[历史契约测试清单](history-contract.md#8-必须补充的契约测试待实现)。

- 生命周期：重复 Start、启动失败回收、Start/Stop 竞争、Stop 超时后继续等待、Close 唤醒订阅并释放目录锁。
- 并发与数据隔离：DTO 修改不影响核心、流取消清理、Settings CAS 冲突、同目录重复实例失败。
- 信任：码仅发现、双方指纹一致、过期审批冲突、介绍不授信、Unpair 后旧会话失效、重启保留身份与拒绝记录。
- 发送：RequestID 幂等与冲突、离线不排队、每设备替换待发、部分成功、网络挂起失败、sent 不冒充远端应用。
- 资源：读取和传输期间释放租约、重复释放、配额预留回滚、列表过期/驱逐、应用锁定条目、重启只清理自有资源。
- 引用的文件：导入后被修改、截断、删除，发送与读取以 source_changed 失败；引用不占存储配额；Open 的权限错误被分类。
- 剪贴板：启动/恢复基线、远端格式补全、防重复显式发送、写入失败、新本地复制穿插、过期 token、系统来源不明时不吞变化。
- 事件：订阅初始快照无窗口、序号连续、队列满必报 resync_required、慢消费者不阻塞、关闭结束等待。
- 配置与隐私：沿用 0002 的热加载及过滤语义，超限读完丢弃，事件/错误/日志不泄露地址、票据和正文。
- 完成桌面构建、`go test -race ./...` 和双节点文本/HTML/PNG/文件回归；移动端编译与真机不属于本阶段验收。

## 13. 版本与后续修订

本文 v0.1 是尚未实现的业务契约；公开稳定前可根据桌面落地调整，但必须同步文档和测试。0003 描述架构与实施顺序，本文描述具体调用语义；发现矛盾时先修订文档再实现，不能默认以代码覆盖约定。

SDK、桌面 HTTP、wire 分别版本化。未来移动端绑定需额外规定数值/字节映射、句柄关闭、取消、线程投递、分块读取和平台来源标记的可用性；这些工作不改变已定义的信任与成功语义。引入 ACK、摘要、可靠投递或跨进程内容恢复须另写设计决策。

## 14. 实现进度与落地时的补充约定

公开包 `github.com/uniclipboard/unic/engine` 分两步落地。每一步完成后更新本节；契约测试位于 `engine/engine_test.go`。

**第一步（已实现并有测试）**：创建与目录独占锁、`Start`/`Stop`/`Close`、`SetRuntimeState`、`NotifyNetworkChanged`、`GetState`、`GetCapabilities`、`UpdateHostCapabilities`、`UpdateSettings`、全部配对与设备方法（`CreatePairingCode` … `Unpair`）、`GetOperation`、`SubscribeEvents`/`Subscription`、错误模型。

**第二步 2a（已实现并有测试）**：内容导入与租约（`ImportContent`、`RetainContent`、`ReadContent`、`ReleaseContent`、`LocalFilePath`），含文件引用（`FileSource`）、配额预留与回滚、读取句柄持有引用、`Close` 中止读取并清理。

**第二步 2b（已实现并有测试）**：`SubmitContent`（仅 explicit）、发送类 `Operation`（`Targets`、`Outcome`、阶段 queued/sending/completed）、每设备一个在途加一个待发并替换为 `superseded`、挂起/断线使在途与待发失败、提交时与开始传输时的同步开关与大小限制检查、引用文件在传输前重新校验。

**第二步 2c（已实现并有测试）**：接收列表（`ListReceived`、`DismissReceived`、`State.Received`、`received` 与 `resource_warning` 事件）；接收内容存为内容对象，经 `RetainContent` 读取、`LocalFilePath` 取文件。

**桌面迁移（已完成）**：`app/desktop/cmd/unicd` 只装配 `app/desktop/internal/host`，由它持有 SDK 引擎、系统剪贴板适配和 HTTP 后端；内部 `internal/session`（原 `internal/engine`）退化为会话层，旧的剪贴板轮询、`apply`、`broadcast`、`lastHash`、`SetLocal`、`Board` 已删除，只剩一套业务状态与信任逻辑。

**第二步 2d（已实现并有测试）**：`ObserveClipboard`、`BeginApply`、`ReportApplyResult`、`State.ActiveApply`、`apply` 事件。公开 Engine 本身不读写系统剪贴板，由宿主适配调用这些方法。

**0004 提交扩展（提交结果与幂等契约已确定、实现与测试待完成）**：见[流水线提交契约](pipeline-contract.md)。Action（插件主动变换）见[Action 契约](clipboard-action-contract.md)，待实现。

**0004 P1 首批内部处理（已实现并有测试，P1 未完成）**：三类入口共用私有处理核心，提供策略暂存、成功采纳/失败回滚、逐实例串行、协作超时/挂起取消与内容引用交接（早期的文本改写 SetText、自动化深度与 plugin 来源已按取消自动写回的决定移除）；默认处理链为空。旧网络适配读取最终内容，入站处理后再次校验运行/策略/信任；并发同键 legacy 提交合并，独立缓存成功/失败（MaxSubmissions 默认 1024、SubmissionTTL 默认完成后 30 分钟），处理中的记录不驱逐，缓存不持有正文。调用者取消只停止等待，工作随 Engine 运行 context 收敛；观察接纳已与处理分离，旧结果不覆盖新位置/基线。CommitContent/GetSubmission、持久历史与多后端分发此后已实现（见执行计划 P1/P2 登记）；Action 及标注型处理的收尾尚未实现。

**0004 历史扩展（契约已确定、实现与测试待完成）**：见[本机历史契约](history-contract.md)。现有代码仍采用下列临时资源/接收列表语义；历史实现时必须同步调整清理、持有关系与应用规则，不能以已有测试通过代表历史验收通过。

落地时确定的细节（既有 v0.1 实现；0004 历史扩展以历史契约为准，必要时回改前文）：

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
11. **ID 前缀**：实例内的 ID（内容、租约、操作）都以实例 ID 加 `-` 开头。他实例的 ID 返回 `resource_expired`，本实例未知的返回 `not_found`。已释放的租约和已回收的内容各记住最近 8192 个，超出后退化为 `not_found`。
12. **PNG 校验**：只校验文件头和尺寸信息（`png.DecodeConfig`），不做整图解码，避免 64 MiB 图片导入时的内存放大；损坏的像素数据会在应用端暴露。
13. **清理范围**：`New` 只删除 `InboxDir` 下以 `unic-content-` 开头的目录（数据目录锁保证它们属于已死的实例）。旧的 `clip-*` 由桌面适配在迁移时处理，SDK 不碰，因为 `InboxDir` 可能是用户自己的目录。
14. **引用的校验**：`FileMeta` 全等比较（大小和 Version）；`Open` 返回不存在或无权限一律报 `source_changed`，其他错误报 `unavailable`，宿主错误文本不转发。
15. **发送的过滤与限制**：提交时按当前 `Sync` 设置裁剪内容（文本/HTML 属于 Text，PNG 属于 Images，文件属于 Files），裁剪后为空报 `sync_disabled`；哈希按裁剪后的内容计算。大小限制在提交时检查，开始传输时用当时的设置再检查一次，超限或同步已暂停则该目标失败而不是发送。
16. **失败不一定断线**：开始写之前的失败（文件已变化、超限、同步暂停）只让这一次发送失败，连接保持；写了一半的失败会丢弃连接，因为流已不同步。
17. **操作持有内容**：创建发送操作时增加内容引用，所有目标终态后释放；调用者此后可立即释放租约。没有目标时不创建操作，引用立即归还。
18. **旧式对端**：发送侧的测试曾用基于会话层的旧式对端验证字节与哈希，现保留它作为与旧 daemon 的线路兼容测试。
19. **接收的准入**：会话层读取一份内容之前先问引擎。不在 running/active、无内容、或存储放不下时，该内容被读完丢弃，连接保持。存储不够时先按从旧到新淘汰接收列表中“只有列表持有”的条目；被租约、读取句柄或操作持有的条目淘汰后也不会释放空间，因此不淘汰；仍放不下则丢弃新内容并发 `resource_warning`。
20. **接收的过滤**：交付时再按当时的同步设置过滤（图片属 Images，其他格式属 Text，文件属 Files）；过滤后为空则丢弃并归还配额，保留的部分只按保留的字节计配额。
21. **接收列表**：最新在前，上限 `MaxReceived`，条目 `ReceivedTTL` 后过期并发 `received` 事件；条目被驱逐后，内容在无其他持有者时立即删除。接收的文件写在 `InboxDir/unic-recv/` 下，该目录整个归引擎所有，启动和 `Close` 时清空。接收文件的 MIME 固定为 `application/octet-stream`（wire 不携带文件类型）。
22. **应用状态**：条目的 `ApplyState` 目前恒为 `available`；`applying/applied/failed/deferred` 与条目锁定（`locked`）已预留，由 2d 的应用流程使用。
23. **观察协议**：新 epoch 必须以 `baseline`、序号 1 开头；同一 epoch 内只有第一条可以是 baseline。重复最近一条序号且参数相同返回原结果（先于租约检查），参数不同或更旧序号报 `conflict`，序号跳跃允许。挂起清空观察状态，恢复必须开新 epoch，因此挂起期间复制的内容不会被发送。
24. **处置与基线**：除错误外的所有处置都会推进基线，包括 `no_targets`、`disabled`，以及新增的 `too_large`（导入后限制被调低时才会出现，因为导入本身已按限制检查）。内容标识是未裁剪内容的哈希，文件只计名称和大小。提交发生硬错误（如存储不足）时不推进，宿主可重报同一变化。
25. **回声**：应用成功后基线取宿主读回的内容，因此系统补充格式不会造成回传。`AppliedToken` 只对未完成的当前 ticket 有效，否则 `conflict`。ticket 未完成期间出现不带 token 的真实本地变化，视为用户新复制：照常处理，并使该 ticket 的成功报告 `conflict`（ticket 随之以 failed 结束、接收条目回到可再次应用）。
26. **ticket 作废**：挂起、停止、同步暂停、宿主应用能力不再 `available` 都会作废当前 ticket：条目解锁，但槽位保持占用直到宿主报告结果（成功报告会 `conflict`，failed/deferred 正常结束）。无观察能力的宿主（`Observe` 不可用）可以不带 epoch 调用 `BeginApply` 和报告 applied，不更新基线。
27. **应用条目的生存**：被应用的接收条目不受容量淘汰、TTL 和 `DismissReceived`（`in_use`）影响，即使已过期仍在列表中；应用结束后立即按 TTL 处理。
28. **Network 选项**：`Options.Network` 允许替换默认的 tailcat 网络（接口只用标准库类型），用于测试和特殊嵌入；`Start`/`Close` 各调用一次，挂起恢复时 `Listen` 会再次调用。
29. **桌面适配的约定**：适配器把剪贴板快照只保留 text/html/png 与普通文件，文件以引用导入；无法表示的内容（文件夹、超限、只有厂商格式）按“剪贴板已清空”上报，因此之后复制别的内容仍是真实变化。应用成功后适配器会把条目移出接收列表（`DismissReceived`），接收文件的生命周期只由剪贴板上的租约决定，被新内容替换后立即释放。显式发送（CLI/GUI 的写入并发送）先写入、回读，再以新 epoch 的 baseline 建立基线，最后 `SubmitContent`，一次请求只发送一次。
30. **HTTP /v1 的兼容**：响应形状不变。`POST /v1/pending/{id}/approve` 新增可选请求体 `{"fingerprint": "..."}`：给出时若设备当前指纹不同则返回 409；GUI 发送它展示的指纹，旧客户端不发则按当前指纹批准。事件的 `Info` 不再携带正文摘要，只写 `text`、`image`、`N files`；`pending` 事件的 `Info` 仍是 `code` 或介绍人名称。


### 本机旧历史迁移

[0011](decisions/0011-v1-history-migration.md) 的独立 CLI 使用 `Options.MigrationMode` 获取目标目录锁并离线导入。`ImportHistory`、`LookupHistoryImport`、`ImportHistoryTags` 与 `ListHistoryTags` 的契约见[本机历史契约](history-contract.md)。该模式禁止 Start，不启动网络或插件，不应用剪贴板。
