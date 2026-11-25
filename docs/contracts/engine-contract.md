# Engine 公共契约 v0.1

> The [device contract](device-contract.md) and [identity architecture](../architecture/identity-and-trust.md) govern device identity, approval, pairing and per-instance bindings. Use them for device and pairing DTOs. A device list presents one row per device with its bindings. Export pairing material only through the explicit export operation.

- Status: public Go SDK contract. The linked feature contracts specify the implemented history, submission, Action, plugin and device APIs. Section 14 records implementation scope.
- 日期：2026-11-07
- 依据：[0003 独立 Engine](../decisions/proposed/0003-engine-sdk.md)、[身份与信任](../architecture/identity-and-trust.md)、[0002 配置](../decisions/proposed/0002-config-file.md)。
- Scope: the public Go package `github.com/uniclipboard/unic/engine` and its host-facing semantics. The [mobile binding](../architecture/overview.md#expo-mobile-client-and-in-process-engine) embeds the same Engine in the Expo host.

本文中的“必须”是实现和契约测试要求。“宿主”指调用 SDK 的平台集成层，不是 GUI。SDK 不直接访问系统剪贴板，不创建 UI，不提供后台常驻保证。

## 1. 边界与通用约定

1. 公开签名只能使用标准库和公开 engine 类型，不暴露 internal、SQLite、tailcat、mygo 类型。Engine 默认拥有身份、数据库、网络与内容资源；宿主负责权限、剪贴板、主线程、通知及配置文件。
2. 每个成功返回的 DTO 都是独立快照；调用者修改切片或字节不得改变核心状态。输入在方法返回后可由调用者复用；流的例外见资源章节。
3. ID 是区分大小写的不透明字符串，禁止从中推导地址、路径或时间。DeviceID 与 0004 扩展的 HistoryID 跨进程稳定；InstanceID 每次 New 更新；其余 ID 仅在所属实例有效。旧实例 ID 的请求返回 `resource_expired`。
4. 时间统一 Unix 毫秒，仅供显示；截止时间内部使用单调时钟。序号是实例内 uint64，0 表示尚无变化；跨语言序列化为十进制字符串，防止数值精度丢失。
5. 方法允许并发调用。状态变更在明确提交点原子生效；核心不在持锁期间调用宿主代码。SDK 不指定 UI 线程。
6. context 只控制本次调用和其直接 I/O，不充当实例生命周期。context 取消后，已提交操作继续存在；取消不回滚信任与成功导入的资源。
7. 普通查询和事件不含设备地址、票据、剪贴板正文、文件内容或绝对路径。内容只能显式读取，地址和配对码只能显式获取。日志和错误必须脱敏，包括第三方错误。
8. The built-in peer-session wire uses version 1. It does not provide remote application acknowledgments, resumable transfers or reliable offline delivery. See the [history contract](history-contract.md) for local queries.

### Multiple sync backends

Engine applies global sync settings, backend instance enablement and each backend's `Settings.BackendPolicies` for text, images, files and automatic file size. Backend results, targets and resource ownership are independent. Disabling global sync cancels backend content work, including explicit sends. History, processing and Actions remain available. See the [pipeline architecture](../architecture/pipeline.md#sync-settings-and-file-ownership) and [backend contract](sync-backend-contract.md) for the current policy and peer-session interfaces.

### External channels

[0009](../decisions/proposed/0009-external-channels.md) 的数据模型与接收路径已实现：`Channel` DTO（无地址/指纹/远端上下文）、`AddChannel / ListChannels / GetChannel / RenameChannel / SetChannelEnabled / SetChannelAutoApply / RemoveChannel`（均带 ExpectedRevision 做 CAS，变化发 `ObjectRef{Type:"channel"}` 与 `EventDevices`）和混排的 `ListPeers`（`Peer{Kind: device|channel}`）。发送目标类型 `group`，一次发送一条目标状态；提交时只把已启用渠道交给后端，后端登记未启用渠道返回 invalid_argument。接收经 `plugin.Receiver` 进入与设备相同的准入、received 流水线、历史与去重，来源为 `external`，`ReceivedContent` 带 `ChannelID`（`PeerID` 为空）与到达时的 `AutoApply`；引擎不写剪贴板，宿主据 `AutoApply` 决定。`RemoveChannel`：后端未声明 `remote_revocation` 时只删本地；声明时（须实现 `plugin.ChannelLeaver`）先停用、请求退出，成功才删除，失败保留并标记 `remove_failed`。


`PairChannel` delegates provider pairing to the selected external-channel
backend. A confirmed provider group is passed to `AddChannel`. An empty code
explicitly resumes the group's existing provider pairing. The result omits
the code and private group context. Existing channels preserve their settings.

`ContentInput.FileImagePNG` optionally carries the clipboard-supplied image
alternative of a single selected file. Engine copies the bytes under its
content quota when an eligible backend declares image file projection. The alternative
does not appear as a content part or in history. See the
[external-channel contract](sync-backend-contract.md#6a-外部渠道-profile0009进程外-rpc-子集已实现).

`WantsClipboardFileImage` reports current demand for the optional alternative
from an eligible automatic clipboard observation.

`ReplaceExternalPlugins` replaces active external Go Action-only plugins or
external-channel object-store backends of one implementation. The replacement
keeps the same capability class. The Engine closes each old process before
starting a replacement in its private directory. It publishes the group only
after all replacements start and the host commits the installation. A failure
restarts the previous processes with their running configurations. The Engine
preserves channel authorization for unchanged effective configuration and
applies the existing revocation rule for changes in one transaction.

### Persistent history

Engine owns the persistent history and processing pipeline. Hosts use the SDK for history and processing. The [history contract](history-contract.md) defines stable history IDs, queries, leases, updates, deletion, retention, received-item mapping, application and event recovery. Startup cleanup preserves retained history. Local tags and expiry stay outside the wire protocol.

### Submission and Actions

The [submission contract](pipeline-contract.md) defines `CommitContent` and `GetSubmission`, request idempotency, and the legacy `SubmitContent` facade. A commit can save history when sync is disabled or no backend or target is available. Check the commit result to determine whether history was saved.

Processors can annotate content. The host calls `ListActions` and runs a selected Action with `RunAction`. A successful Action produces a new history entry with origin `action`. The host can apply that entry through `BeginApplyHistory`. See the [Action contract](clipboard-action-contract.md) and [RPC core protocol](plugin-rpc-contract.md).

三类入口的状态转换、Stop/策略与 commit 的序列化屏障、Received 槽位与成功去重记录的发布、引用/配额交接及重启边界见[流水线契约 §7](pipeline-contract.md#7-状态转换与资源交接)。公开 submission Phase 不增加内部准备状态；接收完成、应用和历史删除不能混作一个终态。

### Backend assembly and identity initialization

按[装配契约](backend-migration-contract.md) 生成并持久化 Engine 设备身份与各实例独立传输身份；不导入旧 tc.*，不保留旧地址或配对记录。Engine 持有全局设备/信任与关联；插件持有传输凭据，不能取得设备私钥。移除公开 Network 并显式装配程序，零后端为本机模式，默认 tailcat 由桌面发行清单提供。

### Lua runtime

The [Lua runtime contract](lua-runtime-contract.md) defines the enhanced VM, allowlist, execution, compilation and allocation budgets, and failure handling. `internal/luavm` implements the runtime budget checks. The VM accounts for logical work and allocations. Runtime internals stay outside the public SDK.

### SDK extension types and errors

The [SDK extension contract](sdk-extension-contract.md) defines the history, submission and Action DTOs, optional values, plugin assembly, and configuration validation. Use the public `engine.Code` set for Engine errors and the [RPC code list](plugin-rpc-contract.md) for plugin protocol errors.

### Plugins and sync backends

The [sync backend contract](sync-backend-contract.md) governs the implemented peer-session state snapshot, approvals, session tokens, send authorization, receive admission and revocation barrier. Revocation must reach the backend before Engine reports success.

The [plugin contract](plugin-contract.md) covers Go and Lua registration, built-ins, external Go processes and GitHub installation. The host explicitly assembles default sync through `builtin.tailcat`. Engine owns plugin lifecycle, processing, commit and operation state. Engine also owns device approval, trust and sync policy. Running with zero backends still provides local history.

### 0014 Logs 与 Metrics 诊断扩展（已接入工作区）

[可观测性契约](observability-contract.md) 确定 `Options.Diagnostics func(context.Context, DiagnosticRecord)`，以及公开的 `DiagnosticStage`、`DiagnosticResult`、`DiagnosticBackend`、`DiagnosticContentKind` 和 `DiagnosticRecord` 类型。现有 `Options.Logger` 保留。类型、有限枚举、统计边界和调用约束以该契约第 2 节为准，已接入当前工作区；验收范围见该契约关联的执行计划。

诊断仅传递阶段、结果、后端大类、内容大类、稳定 Code 和耗时。Engine 在业务锁外调用，宿主必须非阻塞接纳。诊断失败和回调 panic 不改变业务结果。公开签名不依赖 OTel 或 internal 类型，不暴露内容、路径或身份。The diagnostic hook adds no tracing, sampling or propagation fields. The service owns the [explicit-submission trace slice](observability-contract.md#13-sampled-explicit-submission-traces) outside the public Engine API.

### 1.5 Product event observation

`Options.ProductEvents func(context.Context, ProductEvent)` is an optional finite-field product-action hook.
`ProductEvent` contains `Name`, `Product`, `Result`, `Code`, `ContentKind` and `ActionCategory`; it has no identity, business IDs, content or arbitrary properties.
Engine emits authoritative Action/send/trust/apply completions once at their existing idempotency boundaries.
The host owns enablement, analytics identity, SDK queues and delivery.
The callback runs outside the Engine state mutex (`e.mu`); lifecycle/commit guards may remain held. It must return immediately, must not call Engine methods or wait for lifecycle work, and must not retain the caller context. Panic does not change business results.
`ApplyHistoryRequest.UserRestore` distinguishes explicit history restoration from Action application; the host sets it from the operation purpose, independently of sync settings.
Names, finite values and exact emission semantics are defined in [the observability contract](observability-contract.md#10-posthog-product-events).

## 2. 接口总览

Selected public Go signatures follow. The feature contracts specify their DTOs. Context arguments must be non-nil. The SDK owns the `Subscription` and `ContentReader` handles.

```go
func New(ctx context.Context, opts Options) (*Engine, error)

func (e *Engine) Start(ctx context.Context) error
func (e *Engine) Stop(ctx context.Context) error
func (e *Engine) Close(ctx context.Context) error
func (e *Engine) SetRuntimeState(ctx context.Context, state RuntimeState) error
func (e *Engine) NotifyNetworkChanged(ctx context.Context) error
func (e *Engine) RefreshConnections(ctx context.Context, deviceID string) ([]Device, error)
func (e *Engine) GetState(ctx context.Context) (State, error)
func (e *Engine) GetCapabilities(ctx context.Context) (Capabilities, error)
func (e *Engine) UpdateHostCapabilities(ctx context.Context, caps HostCapabilities) error
func (e *Engine) UpdateSettings(ctx context.Context, req UpdateSettingsRequest) (SettingsState, error)

func (e *Engine) CreatePairingCode(ctx context.Context) (PairingCode, error)
func (e *Engine) GetPairingCode(ctx context.Context) (PairingCode, bool, error)
func (e *Engine) CancelPairingCode(ctx context.Context) error
func (e *Engine) ExportPairingTicket(ctx context.Context) (SensitiveTicket, error)
func (e *Engine) JoinByCode(ctx context.Context, req JoinRequest) (OperationID, error)
func (e *Engine) JoinByTicket(ctx context.Context, req JoinTicketRequest) (OperationID, error)
func (e *Engine) ListDevices(ctx context.Context) ([]Device, error)
func (e *Engine) ListPeers(ctx context.Context) ([]Peer, error)
func (e *Engine) ApproveDevice(ctx context.Context, req ApproveDeviceRequest) (Device, error)
func (e *Engine) UnpairDevice(ctx context.Context, req UnpairDeviceRequest) error
func (e *Engine) CreatePairingCodeVia(ctx context.Context, backend PluginID) (PairingCode, error)
func (e *Engine) ExportPairingTicketVia(ctx context.Context, backend PluginID) (SensitiveTicket, error)
func (e *Engine) ListCandidates(ctx context.Context) ([]Candidate, error)
func (e *Engine) PairCandidate(ctx context.Context, req PairCandidateRequest) (OperationID, error)
func (e *Engine) IgnoreCandidate(ctx context.Context, deviceID string, ignored bool) error

func (e *Engine) ImportContent(ctx context.Context, in ContentInput) (ContentHandle, error)
func (e *Engine) RetainContent(ctx context.Context, id ContentID) (ContentHandle, error)
func (e *Engine) ReadContent(ctx context.Context, lease LeaseID, part PartID) (*ContentReader, error)
func (e *Engine) LocalFilePath(ctx context.Context, lease LeaseID, part PartID) (string, error)
func (e *Engine) FileReference(ctx context.Context, lease LeaseID, part PartID) (string, error)
func (e *Engine) ReleaseContent(ctx context.Context, lease LeaseID) error
func (e *Engine) CommitContent(ctx context.Context, req CommitRequest) (SubmissionResult, error)
func (e *Engine) GetSubmission(ctx context.Context, id RequestID) (SubmissionResult, error)
func (e *Engine) SubmitContent(ctx context.Context, req SubmitRequest) (OperationID, error)
func (e *Engine) GetOperation(ctx context.Context, id OperationID) (Operation, error)
func (e *Engine) ListReceived(ctx context.Context) ([]ReceivedContent, error)
func (e *Engine) DismissReceived(ctx context.Context, id ReceiptID) error
func (e *Engine) ListHistory(ctx context.Context, req ListHistoryRequest) (HistoryPage, error)
func (e *Engine) SearchHistory(ctx context.Context, req HistorySearchRequest) (HistorySearchPage, error)
func (e *Engine) BeginApplyHistory(ctx context.Context, req ApplyHistoryRequest) (ApplyTicket, error)
func (e *Engine) ListActions(ctx context.Context) ([]ActionInfo, error)
func (e *Engine) RunAction(ctx context.Context, req RunActionRequest) (ActionResult, error)

func (e *Engine) ObserveClipboard(ctx context.Context, o Observation) (ObservationResult, error)
func (e *Engine) BeginApply(ctx context.Context, req ApplyRequest) (ApplyTicket, error)
func (e *Engine) ReportApplyResult(ctx context.Context, r ApplyResult) error
func (e *Engine) SubscribeEvents(ctx context.Context) (*Subscription, error)

func (s *Subscription) Snapshot() State
func (s *Subscription) Next(ctx context.Context) (Event, error)
func (s *Subscription) Close() error
```

`ContentReader` implements `io.ReadCloser` and returns bytes for the selected part. See the [history API](history-contract.md#2-标识描述与-api) for the full history interface and DTOs.

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

`Settings` includes the device name, rendezvous URL, transfer limits, global `Sync`, `BackendPolicies`, history cleanup and file-cache settings. Global `Sync` controls enablement, automatic and restore sends, file and directory policy, and introductions. Backend policies contain the per-backend text, image and file switches and automatic file threshold. An empty device name uses the saved default name chosen at creation.

`UpdateSettingsRequest{ExpectedRevision, Value}` 替换完整配置，修订不匹配报 `conflict`；验证失败无字段生效。返回 `SettingsState{Revision, Value}`。名称仅影响之后的握手；端点仅影响之后的新配对码请求；大小限制在新传输开始时取快照。在途传输不追溯。同步开关过滤规则沿用 0002。

DataDir、InboxDir 和 ResourcePolicy 只在 New 设置；历史扩展另有 `Options.History`，见历史契约。桌面配置中的 inbox 修改继续显示需重启。文件加载失败信息由桌面配置层持有，不能作为 SDK 配置状态的底层错误直接转发。

`RuntimeState` 为 `active` 或 `suspended`，默认 active。suspended 禁止新网络操作并关闭现有监听和会话；active 后重新建立监听与重连。`Sync.Enabled=false` 仍保留连接，新的提交返回 `sync_disabled`，新接收读完丢弃。已开始传输不因同步开关变化回滚，应用前还须重新检查开关。

`NotifyNetworkChanged` 触发网络重新评估：重置离线设备的重连退避，并对已连接会话异步发起有界应用层探活预热（若旧会话已半开僵死则在探活超时内立即关闭并重连）；它不意味着网络一定可用，也不生成新身份或自动重发旧剪贴板。

`RefreshConnections(ctx, deviceID)` 由用户显式触发，对目标设备（`deviceID` 为空表示全部已信任设备）执行有界连接检测：对已连会话测量应用层往返延迟与传输路径分类（`direct` / `derp` / `peer_relay` / `unknown`，不含原始端点或地址），对离线设备立即重置退避重拨，更新 `DeviceBinding` 的 `Connection` / `ConnectionPath` / `LatencyMs` / `LastCheckedMs` 并发布 `EventDevices`。该操作按需执行，不引入后台周期性主动探测。

### 3.3 能力模型

`HostCapabilities` 包含 `ReadFormats`、`WriteFormats`（MIME 集合）、`ReadFiles`、`WriteFiles`，以及观察与应用状态 `available | permission_required | unavailable | unsupported`。能力描述宿主当前能做什么，不代表远端支持什么。

`Capabilities` 返回宿主声明、核心支持格式和有效交集，以及因配置/挂起导致不可用的原因。初版核心支持 text/plain、text/html、image/png 和普通文件；未知格式返回 `unsupported`。能力变化不改变信任，不清空已收到的内容，也不自动触发系统读写。

Host format and file declarations describe system clipboard integration. They do not restrict `ImportContent`,
explicit commit, received-content admission, history reads or export. `CoreFormats` describes supported representations;
the core also supports regular files. A headless host declares no clipboard formats or file abilities and sets
Observe/Apply to `unavailable`. Import and explicit commit do not require an observation epoch.

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
| `Device` | Stable device identity, fingerprint, trust, revision and current bindings. |
| `DeviceBinding` | One device path through a backend instance. Carries authorization, sync state, connection and revision. |
| `PairingCode` | Sensitive `Code` (`NNN-NNN`) and `ExpiresAtMs`. Returned only by explicit calls. |
| `JoinRequest` / `JoinTicketRequest` | Caller request ID, optional backend selection and a code or ticket. Pairing material stays out of the operation projection. |
| `ApproveDeviceRequest` | Device ID, device revision, expected fingerprint and complete pending binding ID/revision set. |

- CreatePairingCode 成功才替换旧码；失败保留旧码。配对码修改串行化；Cancel 返回时本地已失效，远端 consume 是尽力操作，不影响本地安全。有效期、token、指纹和限速分类见[身份与信任](../architecture/identity-and-trust.md)。
- GetState 仅显示 `PairingCodeState{Active, ExpiresAtMs}`，不包含码本身。GetPairingCode 过期时返回 exists=false。
- `JoinByCode` and `JoinByTicket` return a pairing operation ID. A successful result can still require approval. Query the device and its bindings for the current trust and connection state.
- The code and ticket identify a pairing attempt. Each side must approve the peer. `ApproveDevice` atomically checks the fingerprint, device revision and pending path revisions before persisting trust and authorizing the reviewed paths.
- `UnpairDevice` records a revoked device and removes its bindings. The revocation barrier invalidates affected sessions before success. A later pairing attempt requires new approval. Previously received local history remains available.
- `CreatePairingCodeVia` / `ExportPairingTicketVia` issue a code/ticket for a selected peer-sessions instance. Ticket `be` identifies the implementation. The receiver selects a compatible local instance through `JoinRequest.BackendID` or `JoinTicketRequest.BackendID`. Methods without `Via` select the default Tailcat instance.
- 设备介绍（[身份与信任](../architecture/identity-and-trust.md)）：`ListCandidates` 返回已配对设备推荐的候选（`Candidate{DeviceID, BackendID, Name, Introducers, LastSeenMs, Ignored, Requested, RequestedUntilMs}`，名称未经验证，不含地址与指纹）。`PairCandidate` 返回 pairing 类 OperationID，立即尝试一次并在 24 小时内退避重试，仍须对方批准、双方核对指纹；`IgnoreCandidate` 隐藏/恢复。候选变化发 `ObjectRef{Type:"candidate"}` 与 `EventDevices`。设置 `sync.introductions`（默认开）关闭时不接受介绍也不发送介绍。介绍只是建议，不授信。
- 身份不一致、协议版本不兼容均失败，不覆盖原有 Peer 身份；失败可能已产生对方待审批项，不能声称跨设备事务回滚。

## 6. 内容模型与发送

### 6.1 输入与描述

`ContentInput` 包含 `Representations[]` 与 `Files[]`。每个内存表示提供 MIME、声明 Size 及 io.Reader。每个文件提供展示 Name、MIME（可缺省为 application/octet-stream）、声明 Size，以及二选一的来源：`Reader`（流，复制进核心）或 `Source`（引用）。`Source` 是宿主实现的 `FileSource{Open(ctx) (io.ReadCloser, FileMeta, error)}`，`FileMeta{Size, Version}`，Version 是宿主给的不透明变更标记（桌面可用修改时间）。两者都给或都不给报 invalid_argument。Reader 由调用者关闭，Import 返回前调用者不得修改或并发读取；SDK 不保留 Reader。公开 API 不接收路径。

引用语义：导入时只调用一次 Open 校验大小等于声明值，不读取内容，不计入 MaxStoredBytes。开始传输和 ReadContent 时再次 Open，大小或 Version 与导入时不同、文件已不存在，均以 `source_changed` 失败，不重试、不补发。宿主必须在内容被租约、操作或读取句柄引用期间保持文件可打开。

- 文本和 HTML 是有效 UTF-8；PNG 须为可解码 PNG。一个内容最多包含每种 MIME 一个内存表示，重复 MIME 或空内容报 invalid_argument。不同表示被视为同一内容的替代表示。
- 不遍历文件夹，不接受目录输入。文件名称只用作展示和安全落盘名称，不用作路径；消除目录分隔、`.`、`..` 等路径语义，重复名可存在且 PartID 不同。
- A regular file may contain zero bytes. A nonempty set of zero-byte files is content and must follow the same receive, history and export path as other files. Empty representations and a snapshot without parts remain invalid.
- Import reads exactly the declared size and requires end of stream. Short reads, extra bytes and read errors fail. Protocol caps are 64 MiB per representation, 2 GiB per file, 4 GiB total and 4096 parts. Import enforces the configured total limit. The configured per-file limit applies when transfer starts, so a larger local source can still be imported.
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

### 6.3 Device selection and temporary transfer

- `CommitRequest.DeviceIDs` limits a send to named paired devices. `nil` selects all devices. Empty, unknown, revoked or repeated selections return `invalid_argument`. Selection is part of request idempotency; changing it under the same RequestID returns `conflict`. Only peer-session backends dispatch selected sends. Offline targets retain the `no_targets` disposition.
- `CommitRequest.LocalOnly` skips dispatch for this request and returns
  `not_requested` without a send operation. Local processing and history
  policies apply. The request must omit both target selections. Changing
  `LocalOnly` under the same request ID returns `conflict`.
- `AdmitTransfer(ctx, TransferOffer) (*TransferInbox, error)` admits content from a peer without a unic device identity. Admission precedes payload reads. Engine must be running; sync, backend policy, duplicate detection and storage admission apply. The caller writes received files beneath the private `TransferInbox.Dir`, then calls exactly one of `Complete(TransferContent)` or `Abort()`. Completion enters the received pipeline, including processors, deduplication, quota, history and the received list. Origin is `external`, PluginID is `unic.transfer`, PeerID is empty and ChannelID is `transfer`. This peer creates no device, stored channel or trust relationship. Public signatures expose neither tailcat nor internal types. The host owns the transfer protocol. See the [headless architecture](../architecture/headless.md#command-surface-and-temporary-transfer).

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
| incomplete / invalid / expired / rate_limited | 配对码分类（桌面 HTTP 可映射为旧带连字符形式） |
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

The SDK, local service API and peer wire have separate version boundaries. The [mobile binding](../architecture/overview.md#expo-mobile-client-and-in-process-engine) adapts the same SDK semantics through a platform-specific interface. Update the corresponding contract and verification when a public boundary changes.

## 14. 实现进度与落地时的补充约定

The public Engine provides lifecycle, clipboard observation and application, device trust, content leases, operations and event subscriptions. The production desktop service owns a single Engine instance. The [headless architecture](../architecture/headless.md) describes service and clipboard adapter ownership.

Engine implements durable history, local commit results, request idempotency, multiple backend dispatch and user-triggered Actions. The [history contract](history-contract.md), [submission contract](pipeline-contract.md) and [Action contract](clipboard-action-contract.md) specify the current behavior and link to verification.

The numbered notes below record the initial SDK implementation. Use the linked feature contracts above for current device, history, submission and backend behavior.

1. **实例目录**：`DataDir/unic.db` 与桌面现有数据库同名同表，设备身份沿用 `device.id`；独占锁是 `DataDir/unic.lock`（flock / LockFileEx），进程退出由系统释放。默认设备名在首次创建时取主机名并保存（`device.default_name`），之后不随主机名变化。
2. **配置输入**：`Options.Settings` 全零值等同 `DefaultSettings()`；会合服务地址只来自 `Settings.RendezvousURL`，空则用生产服务，不读环境变量（桌面宿主自行把 `UNIC_RENDEZVOUS_URL` 填入）。`Settings` 不含 inbox，它是创建选项 `InboxDir`。
3. **日志**：`Options.Logger` 是 `*slog.Logger`。底层（tailcat、会话层）的日志会包含地址和设备名，因此在 SDK 模式下被丢弃；SDK 只记录自己的脱敏行（生命周期与错误码）。
4. **待批准项修订号**：`PendingPeer.RequestRevision` 由引擎在内存中维护，设备首次提出请求或被拒绝后再次提出请求时变化；已在等待的设备在后台重连不改变修订。`Peer.TrustRevision` 取配对时间（Unix 秒）。因此同一秒内“拒绝后立刻再次请求”可能得到相同修订，这一窗口接受。
5. **`PendingPeer.IntroducedByName`**：新增字段，介绍人的名称（非身份），桌面界面需要展示“由谁介绍”。`IntroducedByID` 暂不提供，存储里只有名称。
6. **连接状态**：`Peer.Connection` 目前只产生 `disconnected` / `connected`；`connecting` 和操作阶段 `handshaking` 保留，尚未发出。`JoinByCode` 的阶段是 `resolving → connecting → succeeded|failed`，`PairByAddress` 从 `connecting` 开始。
7. **审批输入**：`Approve` 要求 `ExpectedFingerprint` 非空且与当前一致，空值按不一致处理。`Reject` 只校验修订。
8. **运行条件**：`CreatePairingCode`、`JoinByCode`、`PairByAddress`、`NotifyNetworkChanged` 要求 running 且非 suspended，分别报 `invalid_state` / `suspended`；重复的 `RequestID` 即使在 suspended 时也返回原操作。`GetPairingCode`、`CancelPairingCode`、设备列表与审批在任何未关闭状态可用。`ExportPairingAddress` 要求 running（挂起时仍可用）。
9. **事件合并**：会话层的“设备上线/下线”只产生 `peers` 事件；“配对或批准”产生 `peers` 与 `pending` 两个事件（各占一个修订）。事件的 `Objects` 目前只用于操作。
10. **Network recovery**. `NotifyNetworkChanged` resets backoff for disconnected peers and runs bounded probes on live sessions. See [device connection refresh](../architecture/identity-and-trust.md#device-connection-refresh).
11. **ID 前缀**：实例内的 ID（内容、租约、操作）都以实例 ID 加 `-` 开头。他实例的 ID 返回 `resource_expired`，本实例未知的返回 `not_found`。已释放的租约和已回收的内容各记住最近 8192 个，超出后退化为 `not_found`。
12. **PNG 校验**：只校验文件头和尺寸信息（`png.DecodeConfig`），不做整图解码，避免 64 MiB 图片导入时的内存放大；损坏的像素数据会在应用端暴露。
13. **清理范围**：`New` 只删除 `InboxDir` 下以 `unic-content-` 开头的目录（数据目录锁保证它们属于已死的实例）。旧的 `clip-*` 由桌面适配在迁移时处理，SDK 不碰，因为 `InboxDir` 可能是用户自己的目录。
14. **引用的校验**：`FileMeta` 全等比较（大小和 Version）；`Open` 返回不存在或无权限一律报 `source_changed`，其他错误报 `unavailable`，宿主错误文本不转发。
15. **发送的过滤与限制**：提交时按当前 `Sync` 设置裁剪内容（文本/HTML 属于 Text，PNG 属于 Images，文件属于 Files），裁剪后为空报 `sync_disabled`；哈希按裁剪后的内容计算。大小限制在提交时检查，开始传输时用当时的设置再检查一次，超限或同步已暂停则该目标失败而不是发送。
16. **失败不一定断线**：开始写之前的失败（文件已变化、超限、同步暂停）只让这一次发送失败，连接保持；写了一半的失败会丢弃连接，因为流已不同步。
17. **操作持有内容**：创建发送操作时增加内容引用，所有目标终态后释放；调用者此后可立即释放租约。没有目标时不创建操作，引用立即归还。
19. **接收的准入**：会话层读取一份内容之前先问引擎。不在 running/active、无内容、或存储放不下时，该内容被读完丢弃，连接保持。存储不够时先按从旧到新淘汰接收列表中“只有列表持有”的条目；被租约、读取句柄或操作持有的条目淘汰后也不会释放空间，因此不淘汰；仍放不下则丢弃新内容并发 `resource_warning`。
20. **接收的过滤**：交付时再按当时的同步设置过滤（图片属 Images，其他格式属 Text，文件属 Files）；过滤后为空则丢弃并归还配额，保留的部分只按保留的字节计配额。
21. **接收列表**：最新在前，上限 `MaxReceived`，条目 `ReceivedTTL` 后过期并发 `received` 事件；条目被驱逐后，内容在无其他持有者时立即删除。接收的文件写在 `InboxDir/unic-recv/` 下，该目录整个归引擎所有，启动和 `Close` 时清空。接收文件的 MIME 固定为 `application/octet-stream`（wire 不携带文件类型）。
22. **应用状态**：条目的 `ApplyState` 目前恒为 `available`；`applying/applied/failed/deferred` 与条目锁定（`locked`）已预留，由 2d 的应用流程使用。
23. **观察协议**：新 epoch 必须以 `baseline`、序号 1 开头；同一 epoch 内只有第一条可以是 baseline。重复最近一条序号且参数相同返回原结果（先于租约检查），参数不同或更旧序号报 `conflict`，序号跳跃允许。挂起清空观察状态，恢复必须开新 epoch，因此挂起期间复制的内容不会被发送。
24. **处置与基线**：除错误外的所有处置都会推进基线，包括 `no_targets`、`disabled`，以及新增的 `too_large`（导入后限制被调低时才会出现，因为导入本身已按限制检查）。内容标识是未裁剪内容的哈希，文件只计名称和大小。提交发生硬错误（如存储不足）时不推进，宿主可重报同一变化。
25. **回声**：应用成功后基线取宿主读回的内容，因此系统补充格式不会造成回传。`AppliedToken` 只对未完成的当前 ticket 有效，否则 `conflict`。ticket 未完成期间出现不带 token 的真实本地变化，视为用户新复制：照常处理，并使该 ticket 的成功报告 `conflict`（ticket 随之以 failed 结束、接收条目回到可再次应用）。
26. **ticket 作废**：挂起、停止、同步暂停、宿主应用能力不再 `available` 都会作废当前 ticket：条目解锁，但槽位保持占用直到宿主报告结果（成功报告会 `conflict`，failed/deferred 正常结束）。无观察能力的宿主（`Observe` 不可用）可以不带 epoch 调用 `BeginApply` 和报告 applied，不更新基线。
27. **应用条目的生存**：被应用的接收条目不受容量淘汰、TTL 和 `DismissReceived`（`in_use`）影响，即使已过期仍在列表中；应用结束后立即按 TTL 处理。
29. **桌面适配的约定**：适配器把剪贴板快照只保留 text/html/png 与普通文件，文件以引用导入；无法表示的内容（文件夹、超限、只有厂商格式）按“剪贴板已清空”上报，因此之后复制别的内容仍是真实变化。应用成功后适配器会把条目移出接收列表（`DismissReceived`），接收文件的生命周期只由剪贴板上的租约决定，被新内容替换后立即释放。显式发送（CLI/GUI 的写入并发送）先写入、回读，再以新 epoch 的 baseline 建立基线，最后 `SubmitContent`，一次请求只发送一次。
30. **桌面 HTTP /v1**：`GET /v1/status` 不含地址/票据，也不触发签发。显式 `POST /v1/pairing-ticket` 返回 `{"ticket":"unic-pair:…"}`，响应禁止缓存，CLI `unic device addr` 与 GUI 生成票据使用它。`POST /v1/pending/{id}/approve` 必须提交 `{"fingerprint":"…","revision":1,"bindings":[{"id":"…","revision":1}]}`，修订均来自待批准列表；遗漏/坏 JSON 报 400，指纹不符或设备/待批准关联集合变化报 409。`ApproveDeviceRequest{ID, ExpectedRevision, ExpectedFingerprint, Bindings []BindingRequest}` 由 `Engine.ApproveDevice` 在一个事务中批准设备及完整待批准关联；已信任设备也不可跳过指纹。CLI `unic device approve <id> '<另一端指纹>'`，GUI 回传所展示的完整快照。事件的 `Info` 不携带正文摘要，只写 `text`、`image`、`N files`；`pending` 事件的 `Info` 仍是 `code` 或介绍人名称。


### 本机旧历史迁移

[0011](../decisions/proposed/0011-v1-history-migration.md) 的独立 CLI 使用 `Options.MigrationMode` 获取目标目录锁并离线导入。`ImportHistory`、`LookupHistoryImport`、`ImportHistoryTags` 与 `ListHistoryTags` 的契约见[本机历史契约](history-contract.md)。该模式禁止 Start，不启动网络或插件，不应用剪贴板。

## Sync settings and file ownership (0016, CLI implemented)

`SyncSettings` includes Enabled, Automatic, OnRestore, Files, and Introductions.
`FileCacheSettings` includes MaxBytes. Unified history cleanup controls retention.
The sync defaults are true, true, false, true and true respectively. The default file-cache quota is 500 MB.
The default per-file transfer limit is 100 MB. Source import uses the protocol file cap instead of the transfer limit.
`SyncRestoredHistory` requires the last successful history apply token and a retained content lease.
The method consumes the token's resend permission once, preserves stored processor policy, and creates no duplicate history.
Its result separates local apply from sync disposition, operation ID, and sync error.
`Options.CacheDir` is an app-private directory selected by the host for Engine-owned payloads.
`Options.StoreReceivedFile` is an optional host callback, separate from private-cache storage.
When the callback is nil, Engine reserves and retains files in its private cache.
When the callback is present, Engine passes `ReceivedFile{Name, MIME, Size, Reader}` and adopts the returned `PersistentFileSource`.
The callback must consume exactly Size bytes synchronously, honor context cancellation, and return a readable source with the declared size.
The reader remains Engine-owned and is valid only during the callback.
The persistent reference must be non-empty, at most 65536 bytes, and must not use the reserved `cache:` prefix.
Engine validates the source without holding state, history, or content locks.
Callback errors and panics produce stable errors without forwarding host text, references, or paths.
A failed callback cannot publish history, Received, or fallback cache content.
Files that the host already created remain host-owned, including after a later receive commit fails.
Engine receives no destination path or URI and no authority to delete host files.
`FileReference` explicitly returns a persistent host reference after validating the leased source.
Engine does not interpret that value; a host can resolve it as a URI, handle, or path.
`LocalFilePath` returns `unsupported` for host-managed sources.
`ReconcileClipboardFiles` establishes default-cache clipboard pins before deferred startup cleanup.
`GetFileCacheState` exposes only IDs, byte counts, completion times, availability, and held flags.
Cache deletion authority never follows a custom file reference.
See [desktop runtime](../architecture/desktop-runtime.md) for the CLI, configuration revision, and complete deletion boundary.

## Unified automatic cleanup

`Settings.History` exposes live `HistoryCleanupSettings{AutoCleanup, RetentionDays, MaxItems, ExcludedTags}`.
`Settings.FileCache` retains only `MaxBytes`. Independent file-age cleanup and its settings are removed.
`Options.History` retains only the startup `MaxBytes` budget.
Settings snapshots deep-copy the excluded-tag slice. UpdateSettings validates the whole policy and applies it with revision checking.
Age/count deletion and default-cache capacity reclamation share history deletion and tag exclusion rules.
See the [history contract](history-contract.md) and [desktop runtime](../architecture/desktop-runtime.md).

## Local headless host

The [local service contract](local-service-contract.md) defines explicit submissions, transient Received reads,
independent Action application and exclusive desktop adapter generations.
A missing clipboard adapter does not restrict Engine import, commit, receive, history reads or export.
Service restart invalidates local sessions and content leases.

## Receive admission and temporary startup cleanup

All receive adapters provide the exact byte size of every part before admission.
Admission rejects a total that differs from the sum of the part sizes.
Representations must contain at least one byte. Regular files may contain zero bytes.
External-channel adapters derive part sizes from the supplied data before entering the shared admission path.

After acquiring the profile lock, New removes plain directories named `unic-recv` or prefixed `unic-content-` inside InboxDir.
The scan preserves other entries and does not follow symbolic links during recursive deletion.
Cleanup failure prevents startup. Retained file-cache payloads remain governed by their ledger and history holders.
Dismissing Received or closing Engine does not delete a payload still referenced by history.


## Product result classification (0020)

`ProductEvent.SourceKind` is a finite category: `local`, `remote`, `external`,
`action`, `migration`, or `unknown`. Explicit history application freezes this
category from the selected stored origin in its application ticket. It emits one
`history_applied` event only after an accepted successful application result.
Successful remote-device receipt application emits `remote_content_applied` instead.
History application never emits that receipt event. Duplicate reports emit neither
again. These callbacks preserve the existing outside-state-lock and panic-isolation
rules. The service owns daily admission, persistence and operational volume.
See the [observability contract](observability-contract.md#11-tailcat-sync-diagnostics-0020).

## Explicit history search

Engine owns `SearchHistory`, `ListHistorySearchCandidates` and `CountHistorySearches`.
They use typed queries and a derived projection in the same history database.
SearchHistory is an explicit bounded content read; ordinary metadata lists and events remain content-free.
Matching, source selectors, tag all/any mode, cursor rules, read isolation, limits and incomplete-index behavior are defined in the [history search contract](history-search-contract.md).
The implementation is in `engine/history_search.go` and `engine/history_search_types.go`.

## Directory extension

Directory selection references, preparation, manifests, file-policy grouping, bounds, and acceptance follow the [directory content contract](directory-contract.md). The 0024 execution receipt distinguishes specified behavior from verified implementation.

## Source application metadata

Clipboard content and history preserve optional source application identity and cached PNG icons.
See the [source application contract](source-application-contract.md) for capture attribution, resource bounds, and transport semantics.

## Go Action input streams

Go Actions may accept text, image and file history items and return a new text
item. The Engine retains source content while the invocation is active and
revokes readers when it ends. `ActionSpec.TimeoutMS` selects a deadline up to
300000ms; zero preserves 10 seconds. `RunActionRequest.CancelOnContext` opts into
cancelling admitted Action work when its caller context ends. The default keeps
admitted work running. See the [Action contract](clipboard-action-contract.md#go-action-source-content)
for indexed source access, idempotency and external-effect boundaries.

## Configured sync backend instances

`PluginInstance.ID` is the local runtime instance ID. Empty uses the implementation ID as the singleton convenience. A distinct ID is supported for external Go `sync_backend` instances. `PluginProcess.Spec.ID` remains the implementation identity verified by the process handshake. Lua and ordinary processor/observer instances cannot acquire this capability or use runtime aliases.

Engine keys standard policies, private directories, locks, authority, operations, receive provenance and restart selection by runtime instance ID. Program identity is sticky for the lifetime of that instance. Reassigning an existing instance to another implementation returns `conflict`; create another instance instead. Engine atomically records private configuration identity before starting a backend. A changed Go backend configuration revokes that instance's old device bindings and external channels on the next service start. Device trust and other instances remain intact. This conservative rule also applies to non-secret settings because the portable Schema does not classify transport/account context. No private configuration value or digest is projected publicly.

`ListSyncBackends(ctx)` returns assembled instance IDs, implementation IDs, transport kinds, device-pairing support, external-channel classification and local runtime state (`running`, `restarting`, `unavailable`, `suspended`, `stopped`). Runtime state does not imply remote connectivity. Disabled instances are not assembled. No transport address, credential, process token or absolute path appears in this projection.

`JoinRequest.BackendID` and `JoinTicketRequest.BackendID` select a local compatible instance and participate in request idempotency. The selected implementation must match the ticket descriptor. Omission uses only an exact singleton ID match; no alias guessing or Tailcat fallback occurs. The issuer checks its own issuing instance when redeeming PairProof. RPC frames, identity proofs, peer metadata and ticket envelopes use protocol version 1. No previous ticket-shape compatibility is provided.


### Live external plugin start


`StartExternalPlugin` admits an inactive external Go Action-only instance or
external-channel object-store backend while the Engine is running. The Engine
verifies and starts the candidate child process, calls the host's persistence
callback, then publishes its Action or backend registration. The callback
may read or update settings and must return without waiting for service
shutdown or another plugin startup. Callbacks from a candidate backend have no
channel access before publication. A failed request publishes no runtime.
Existing plugin instances and the host lifecycle remain in place.
