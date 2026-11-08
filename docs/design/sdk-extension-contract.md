# 0004 / 0007 SDK 类型与错误契约

- 状态：目标定义完成，待实现；本文不是已有可调用 API。现有实现基线见[测试记录](../exec/baselines/0004-2026-11-07.md)。
- 日期：2026-11-08（取消自动写回，改为 Action）。
- 公开业务类型属于 `github.com/uniclipboard/unic/engine`；插件进程 SDK 属于 `github.com/uniclipboard/unic/plugin`，不依赖 engine。行为分别遵循[提交](pipeline-contract.md)、[历史](history-contract.md)、[Action](clipboard-action-contract.md)、[插件](plugin-contract.md)及[0007](decisions/0007-multiple-sync-backends.md)。本文补齐具体字段、可选值与错误映射，不取代它们的状态机。

## 1. 公共边界与表示

以下 Go 类型是目标声明，分阶段加入 engine；未重复列出的既有类型保持原定义。不得在公开签名、字段、导出泛型参数中出现 internal、SQLite、tailcat、mygo 或 VM 类型。公开 engine 与 plugin 的同义 DTO 独立定义并在 Engine 内转换，不使用类型别名形成反向依赖。

新枚举使用各自的命名 string 类型，不能复用网络 Operation 的 Phase 或入参 Origin。ID 均不透明；PluginID 同时是首版插件实例 ID 和后端 ID，不再引入一个意义相同的 BackendID 类型。实例不支持同一 ID 多次装配；若未来需要同一实现多配置实例，另定标识迁移。

nil 指针表示字段尚无值/不适用，不能用零枚举猜测状态；非空字段必须是已定义枚举，不能传空字符串。切片、映射、指针及嵌套 Config 全部深拷贝。Go SDK 不强制经过 JSON；对外新增 JSON 适配使用 snake_case，uint64 修订/序号按十进制字符串编码，时间为 Unix 毫秒 int64；RPC 仍按自身 Schema 编码字节/时间，不复用 SDK JSON 标签。旧桌面 `/v1` 形状由兼容适配保持。

## 2. 提交与 Action DTO

```go
type PluginID string
type ItemID string
type HistoryID string
type SubmissionPhase string
type SyncDisposition string
type BackendDisposition string

type CommitRequest struct {
    RequestID RequestID
    LeaseID LeaseID
    BackendIDs []PluginID
}
type SubmissionResult struct {
    RequestID RequestID
    Phase SubmissionPhase
    ItemID ItemID
    HistoryID HistoryID
    SyncDisposition SyncDisposition
    OperationID OperationID
    BackendResults []BackendSubmissionResult
    SyncError *Error
    Error *Error
}
type BackendSubmissionResult struct {
    BackendID PluginID
    Disposition BackendDisposition
    Error *Error
}
type ActionSpec struct {
    ID string
    Title string
    Accepts []string
}
type ActionInfo struct {
    Key string // pluginID/actionID
    PluginID PluginID
    ID string
    Title string
    Accepts []string
}
type RunActionRequest struct {
    RequestID RequestID
    Key string
    HistoryID HistoryID
}
type ActionResult struct {
    RequestID RequestID
    ItemID ItemID
    HistoryID HistoryID
    Phase SubmissionPhase
    SyncDisposition SyncDisposition
    OperationID OperationID
    BackendResults []BackendSubmissionResult
    SyncError *Error
}
```

SubmissionPhase 为 processing/committed/failed；SyncDisposition 与 BackendDisposition 的不同取值及聚合遵循提交契约和 0007。BackendResults 按 BackendID 排序且不重复。processing/failed 时 SyncDisposition 为空，OperationID/HistoryID 为空；failed 必有 Error。committed 时 Error=nil；仅同步 failed 有 SyncError，只有 queued 有 OperationID。HistoryID 仅成功持久化时非空，不能以其空值判断是否 committed。

ActionInfo/ActionSpec 的字段限制见[插件契约 §1](plugin-contract.md#1-标识能力与入口)；Accepts 取值 text/image/files，首版仅 text 可用。RunAction 失败返回 engine.Error，不创建条目；成功的 ActionResult 只在已 commit 后返回，故无 failed 阶段，Phase 恒为 committed。语义见[Action 契约](clipboard-action-contract.md)。

BackendIDs=nil 表示全部启用后端；非 nil 空切片非法，JSON 适配须保留“省略/null”和 `[]` 的区别，不能用 omitempty 丢掉显式空数组校验。非空选择规范化排序参与幂等比较，重复项拒绝。

新增/扩展字段：`ObservationResult.Submission *SubmissionResult`、`State.HistoryRevision uint64`、`ReceivedContent.HistoryID HistoryID`。State 不嵌入整张 submission/历史表。

既有 ActiveApply 保留 ContentID，新增 `Kind ApplyKind`（received/history）、`HistoryID HistoryID`，已有 ReceiptID 改为按 Kind 可空。只有对应源 ID 非空；历史删除后仍可保留关联 ID。ApplyTicket 的 Token/ContentID/LeaseID 不变，不在 State 中加入租约。ReportApplyResult 的 ErrorCode 沿用稳定码输入，不接收原始平台错误文本。

发送 TargetStatus 的目标结构如下；ApplyKind 和 SyncTargetKind 都是独立命名 string 类型。TargetKind 为 device/repository，TargetID 是不透明 string：device 时等于 DeviceID 的字符串，repository 时为实例内稳定仓库标识，禁止用 URL/凭据作 ID。PeerID 仅 device 时非空以便兼容旧展示。没有新增 canceled 目标枚举，取消仍以 failed + cancelled/suspended 等稳定错误表达。

```go
type ApplyKind string
type SyncTargetKind string
type TargetStatus struct {
    BackendID PluginID
    TargetKind SyncTargetKind
    TargetID string
    PeerID DeviceID
    State TargetState
    Error *Error
}
```

## 3. 历史 DTO

```go
type HistoryKind string
type HistoryOriginKind string
type HistorySyncState string
type HistoryOrigin struct {
    Kind HistoryOriginKind
    DeviceID DeviceID
    PluginID PluginID
}
type HistoryPartSummary struct {
    Kind PartKind
    MIME string
    Size int64
}
type HistoryBackendSyncState struct {
    BackendID PluginID
    State HistorySyncState
    Disposition BackendDisposition
    ErrorCode Code
}
type HistoryItem struct {
    ID HistoryID
    Revision uint64
    Kind HistoryKind
    Origin HistoryOrigin
    SourceApp string
    CreatedAtMs int64
    UpdatedAtMs int64
    ExpiresAtMs int64
    Tags []string
    Parts []HistoryPartSummary
    TotalBytes int64
    SyncState HistorySyncState
    BackendSyncStates []HistoryBackendSyncState
}
type HistoryFilter struct {
    Kinds []HistoryKind
    Tags []string
    OriginKinds []HistoryOriginKind
    DeviceID DeviceID
    CreatedFromMs int64
    CreatedBeforeMs int64
}
type ListHistoryRequest struct {
    Limit int
    Cursor string
    Filter HistoryFilter
}
type HistoryPage struct {
    Items []HistoryItem
    NextCursor string
    HistoryRevision uint64
}
type UpdateItemRequest struct {
    ID HistoryID
    ExpectedRevision uint64
    Tags *[]string
    ExpiresAtMs *int64
}
type DeleteItemRequest struct {
    ID HistoryID
    ExpectedRevision uint64
}
type ApplyHistoryRequest struct {
    HistoryID HistoryID
    ExpectedEpoch string
    ExpectedSequence uint64
}
type HistoryOptions struct {
    MaxItems int
    MaxBytes int64
    DefaultTTL time.Duration
}
```

HistoryKind 为 text/image/files，HistoryOriginKind 为 local_system/remote_device/explicit/action，HistorySyncState 为 not_requested/pending/sent/partial/failed/interrupted。HistoryOrigin 只在 remote_device 携带 DeviceID、只在 action 携带 PluginID；内部远端进入标记另存，不能靠公开来源重新推断是否允许转发。PartKind 沿用既有 representation/file，摘要不暴露 Name、PartID、路径或正文。

BackendSyncStates 按 BackendID 排序且不重复；State 描述实际目标汇总，Disposition 保存首次提交决策，ErrorCode 仅保存稳定失败类别。重启把 pending 改为 interrupted，不把原 queued 决策改成“已重发”。禁发/无后端/远端不转发可只有聚合 not_requested 而无后端摘要；删除插件装配不删旧历史摘要。

UpdateItem 的 nil Tags 表示不变，非 nil 且空切片表示清空；nil ExpiresAtMs 表示不变，指向 0 表示移除期限。Created 时间边界不得为负，两个非零边界须 from < before；0 是无边界。其余上限、排序、游标与 CAS 错误按历史契约。

HistoryOptions 的两个容量字段 0 使用默认值（10000、4GiB），负数非法；DefaultTTL=0 表示无限期。Resources 新增 MaxSubmissions int/SubmissionTTL time.Duration、MaxSyncDedupEntries int/SyncDedupTTL time.Duration，零值分别取 1024/30分钟、100000/7天，负数非法。默认化后历史 MaxBytes 不得大于总 MaxStoredBytes。

## 4. 插件装配与配置所有权

```go
type PluginCapability string
type TransportKind string
type PluginErrorPolicy string
type PluginConfig map[string]any
type PluginInstanceSettings struct {
    Enabled bool
    Events []string
    Config PluginConfig
}
type PluginSettings struct {
    Order []PluginID
    Instances map[PluginID]PluginInstanceSettings
    OnError PluginErrorPolicy
}
type PluginProcessSpec struct {
    ID PluginID
    Version string
    APIVersion int
    Capabilities []PluginCapability
    TransportKind TransportKind
    Executable string
    SHA256 string
}
type LuaPluginSpec struct {
    ID PluginID
    Version string
    APIVersion int
    Capabilities []PluginCapability
    Source []byte
}
type BackendSyncPolicy struct {
    Text bool
    Images bool
    Files bool
    AutoFileMaxBytes int64
}
type SyncSettings struct {
    Enabled bool
}
```

目标 Options 新增 `History HistoryOptions`、`Plugins PluginSettings`、`PluginProcesses []PluginProcessSpec`、`LuaPlugins []LuaPluginSpec`；Resources 新增 `MaxSyncBackends int`（0 取默认 8，合法 1..32）。Network 的移除时点仍是 P5，不在文档阶段删除现有接口。LuaPlugins 是宿主读完后传入的源码，不是路径、加载回调或 VM 对象；New 深拷贝并校验最大 256KiB，启用脚本语法/执行校验在启动时有界进行。热替换由后续 P6 定义的入口承接，不能因装配使用 []byte 允许调用方原地修改已运行状态。

PluginCapability 为 processor/observer/action/sync_backend，TransportKind 为 peer_sessions/object_store；普通插件 TransportKind 必须为空，Lua 不支持 sync_backend；Spec 的 Actions 仅 action 能力携带。PluginErrorPolicy 为 continue/block_sync，空值默认 continue。Version/API/ID/能力/摘要的校验和生命周期仍按插件、RPC 与迁移契约。装配存在 object_store 标识不代表已定义其后端协议；首版启动不支持的 profile 返回 unsupported。

同 ID 只能有一个 Go 或 Lua 描述；启用实例须有描述且在 Order 恰好一次；Order 不能包含禁用/未知实例。禁用描述不启动、不要求其本机程序当前可访问，但元数据仍须有效，不能保留重复/冲突 ID。无插件装配是本机模式，不默认开启 tailcat。Stop 后再 Start 不替换装配；重启项指关闭旧 Engine 后用新 Options 创建实例。

Settings 新增 `BackendPolicies map[PluginID]BackendSyncPolicy`，是标准策略的唯一生效存储。PluginInstanceSettings **没有第二份 Sync 字段**；桌面 TOML 的 `.sync` 映射到该表。只允许键指向已装配的 sync_backend（含禁用实例），返回 SettingsState 时填满默认策略。缺少某键才默认 Text/Images/Files=true、阈值0；显式给出的全 false 是合法禁用，不能被默认化覆盖。阈值范围 0..2GiB。

DefaultSettings 仍提供有效大小限制及全局 Enabled=true；后端默认值因装配而在 New 规范化。SDK 调用方要表达全局关闭，使用 DefaultSettings 后设置 Enabled=false；完整 Settings 零值仍沿用既有“使用默认设置”的约定。UpdateSettings 是全量 CAS，缺省策略重新按默认规范化，不表示保留旧值；不得借更新更改装配/启用/顺序。日志和公开 State 不包含 Executable、Source 或专属 Config；含秘密的后端配置不得通过完整装配快照泄漏。

Config 限于 JSON 对象，最大编码 64KiB、嵌套深度32；数字保留 json.Number（以 `encoding/json` 定义为准），拒绝 NaN/Inf、重复键、未知 Go 对象/指针、循环结构及不能无损往返的数值。核心先做结构/限额校验，再由插件 Init 按完整允许键集合递归校验专属配置；未知键返回 invalid_config，不可静默忽略。SDK 提供严格解码辅助，Go 模板必须启用未知字段拒绝；Lua Init 使用同等严格的配置工具。无需在安装阶段执行插件来校验专属键，清单默认配置也不能证明某个任意键受支持。

启动任一 Init 拒绝即启动失败；热替换候选拒绝则保留旧实例。配置/校验错误只返回稳定类别及公开插件 ID，不能回显错误值、源码或嵌套配置秘密。普通插件 Config 不得出现用来替代 Engine 标准同步策略的授权字段。

## 5. 统一错误分类

engine.Error 的既有字段、errors.As 及 context errors.Is 行为保持。目标新增 `CodeInvalidConfig = "invalid_config"`、`CodePluginPanic = "plugin_panic"`，与既有 RPC Schema 对齐；不引入第三套 Lua 自由文本错误。其余复用现有 Code。插件 SDK 的 Error/Code 独立定义为 RPC 允许集合，不能依赖 engine.Error；父端只接纳允许码，未知码是协议错误。

| 场景 | SDK 稳定 code / 结果 | 处理与重试边界 |
|---|---|---|
| DTO/枚举/UTF-8/大小声明/装配形状非法 | invalid_argument | 接纳前拒绝；不缓存提交，不泄漏输入 |
| Config 未知键/类型/必需项不符 | invalid_config | Init/候选失败，不能靠 continue 掩盖启动失败 |
| Spec 版本/API/协议不匹配，非法帧或越权 RPC | protocol_mismatch | 拒绝会话并回收；身份不符用 identity_mismatch |
| 源码语法或插件表结构非法 | invalid_config | 启动/重载失败，不把语法诊断片段转发 |
| Go 回调 panic | plugin_panic | SDK 恢复并脱敏；处理回滚 patch，按 on_error 继续/禁发 |
| Lua 常规抛错或无法细分的插件内部失败 | internal | 回滚 patch，丢弃失败状态；不能重放当前提交 |
| 墙钟超时 / 工作取消 | timeout / cancelled | 回滚未采纳 patch；挂起取消的工作可用 suspended，Stop 用 cancelled |
| 指令/原生工作/逻辑分配或有界槽位耗尽 | resource_exhausted | 释放所有预留，Lua 丢弃状态；不是精确 RSS 保证 |
| 用户/协议内容大小超限 | limit_exceeded | 与存储/队列容量不足区分；合法入站按后端规则 drain |
| 数据库/磁盘 I/O 错误、插件进程退出 | unavailable | SQLite busy 超时用 timeout；可识别的容量耗尽用 resource_exhausted；不转发底层错误 |
| 历史 CAS/幂等参数/游标过滤不符 | conflict | 查新快照或换请求；同键失败缓存不重跑 |
| 历史已删/到期或请求结果已驱逐 | not_found | 不能把配对码 expired 用于历史到期 |
| 旧实例游标/票据/租约 | resource_expired | 新实例重新取得，不重建旧授权 |
| 应用槽被占用 | in_use | 宿主稍后重试；平台能力/权限用 unsupported/permission_denied |
| Action 的 Key 未知 / 来源类型不符 | not_found / unsupported | 不调用插件，不创建条目 |
| Action 未恰好一次 SetText / 结果为空或非法 | invalid_argument（超限 limit_exceeded） | 失败即失败，不适用 on_error，不创建条目 |
| 没有后端 / 无目标 / 策略禁发 | 提交成功后的 disposition | 不作为新 CommitContent 错误；旧 SubmitContent 按兼容门面映射 |
| 自动文件阈值跳过 | BackendDisposition=auto_file_too_large | 策略结果，不新增同名 Error.Code |

处理器失败通常由 on_error 消化，不能把 callback 错误无条件设为 SubmissionResult.Error。只有 commit 前不可恢复终态才 Phase=failed；commit 后网络错误进 SyncError/目标状态，历史已保存不能被改为失败。

Retryable 只表达未来条件可能改善；已接纳失败使用原键返回原结果，不能因 Retryable=true 自动创建新请求。timeout/cancelled 可能是调用者停止等待，须按原 RequestID 查询；不向 SDK Error 附带第三方 cause，只有宿主本次 context 错误保留标准取消/超时 cause。

## 6. 分阶段交付与校验

P1 加提交/状态 DTO，P2 加历史 DTO，P3 加 Action DTO 与 ListActions/RunAction，P4 加插件 DTO/错误与严格配置辅助，P5 移除 Network 并迁移标准策略，P6 加 Lua 装配/重载实现；依赖字段可先声明，但不得把未实现方法标为可用。公开 DTO 不得为了绕过编译依赖而直接引用内部实现。

各阶段须验证零值/显式空值、未知枚举、深拷贝、错误分类和事件隐私；RPC 与公开 DTO 转换须验证大整数与可选字段。新增接口前同步 SDK 契约实现登记，旧 HTTP `/v1` 适配和新接口分开验收。本文完成仅关闭 P0 类型/错误设计项，不关闭真实双设备基线或任一运行时验收。
