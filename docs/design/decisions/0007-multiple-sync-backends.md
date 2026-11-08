# 0007 · 多同步后端与分层同步策略

> 0008 补充：[统一设备与后端关联](0008-device-identity-and-backend-bindings.md) 要求实现标识与配置实例分离，设备级信任和关联授权分别校验；目标/策略按实例区分。无旧 tailcat 身份/地址/配对记录及配置兼容要求，原密钥导入要求取消，详见[装配契约](../backend-migration-contract.md)。本文件实现登记是阶段事实，不能视为 0008 已实现。

- 状态：已采纳；Engine 内部多后端抽象与派发已实现（见文末“实现登记”），per-backend 设置、热更新、跨后端去重与外部后端装配待实现。
- 日期：2026-11-07。
- 关联：[0002 配置](0002-config-file.md)、[0004 流水线与插件](0004-clipboard-pipeline-lua.md)、[提交契约](../pipeline-contract.md)、[同步后端契约](../sync-backend-contract.md)。
- 本决策替代上述目标设计中的“最多一个后端”和“全局按类型过滤”；现有代码仍保留旧模型，文档变更不表示多后端已实现。

## 1. 三层控制

“同步到其他设备”是 Engine 统一提供的能力，各同步插件是该能力的实现。最终发送必须同时满足：全局允许、后端实例启用、Item/Policy 允许、该后端策略允许、目标授权和资源/协议限制。任何插件不能扩大上一层授予的权限。

1. `sync.enabled` 是唯一全局内容同步开关。false 时所有后端停止自动/手动内容发送、接收和远端自动应用，不得借轮询、重试、离线队列或其他入口继续传输；本机捕获、历史、处理与本机 Action/历史使用继续工作。
2. `plugins.instances.<id>.enabled` 控制单个插件实例。可以同时启用 P2P、WebDAV 等多个 Go sync_backend，默认发行版仍只提供 builtin.tailcat；0 个后端为本机模式。启用列表变化仍需重启，全局关闭不等于卸载或必须杀掉插件进程。
3. 每个后端有独立的 `sync` 标准策略：`text`、`images`、`files`、`auto_file_max_bytes`。这部分由 Engine 解析、校验与执行，不由插件自行决定是否遵守；连接地址、凭据引用等实现专属选项仍在 `config`，不进入普通日志和事件。

全局开关关闭是立即的授权撤销：取消尚未开始和正在进行的内容传输/接收，清理半成品和配额；已发出的字节或已写入远端存储的对象不能撤回。需要终止失步的 P2P 内容连接时允许断开后重建控制连接。配对、健康检查及连接维护可保留，WebDAV 内容上传/下载/内容轮询必须停止。切回 true 不自动补发关闭期间或已失败的提交，不重新应用旧 Received。

## 2. 配置与阈值

```toml
[sync]
enabled = true

[plugins]
order = ["builtin.tailcat", "example.webdav"]

[plugins.instances."builtin.tailcat"]
enabled = true
events = []
[plugins.instances."builtin.tailcat".sync]
text = true
images = true
files = true
auto_file_max_bytes = "100MB"

[plugins.instances."example.webdav"]
enabled = true
events = []
[plugins.instances."example.webdav".sync]
text = true
images = true
files = true
auto_file_max_bytes = "10MB"
# WebDAV 实现专属设置放在 .config；具体凭据与对象协议另定。
```

这里的 example.webdav 仅演示可并存的后端，不表示已经提供 WebDAV 插件。标准策略仅允许 sync_backend 声明，普通 processor/observer 配置该段应被拒绝。所有布尔项默认 true，auto_file_max_bytes 默认 0，含义为不额外设自动文件阈值，仍受 Engine limits 和后端硬限制约束。负数非法，正值须不超过协议支持的单文件硬上限；大小单位沿用 0002。

阈值按每个文件判断，`size <= threshold` 允许；有任一文件超阈值，则该后端本次**整个自动提交**被跳过，原因 auto_file_too_large，不悄悄只发送部分文件或同条目的文本。其他后端独立判断；例如 50MB 文件可以自动走 100MB 阈值的 P2P，而跳过 10MB 阈值的 WebDAV。总大小限制仍单独检查，不把单文件阈值当总额。

类型开关适用于发送与接收，文本含 HTML 等非图像内存表示，图片为 image/*，文件为文件 part；按后端过滤，过滤后为空则跳过，未发送的格式仍可留在本机历史。阈值仅限制自动发送，不限制接收；接收由类型开关、limits 和配额控制。

显式 CommitContent/SubmitContent 为手动发送，可越过自动文件阈值，但不能越过全局开关、实例禁用、Policy 禁发、类型开关或大小硬限制；不得通过 UI 自动生成 explicit 请求来偷偷绕过自动阈值。

同 ID 后端的标准策略允许热更新，需 CAS 原子替换并按各后端传播；坏配置保留上一有效值。类型/阈值/limits 影响之后开始的传输，普通策略变化不追溯已开始传输；全局关闭、撤销信任和挂起属于立即取消。重启项仍是实例启用、顺序、Go 二进制及后端种类变更。

## 3. Engine 设置与严格配置

目标 `Settings.Sync` 只保留 Enabled；`Settings.BackendPolicies map[PluginID]BackendSyncPolicy` 保存 Engine 生效的各后端标准策略，BackendSyncPolicy 为 Text/Images/Files/AutoFileMaxBytes。只允许引用已装配的 sync_backend；省略时取默认值，返回 SettingsState 时给出完整有效映射，DTO 深拷贝。实例安装/启用描述仍由插件装配设置管理，不让普通处理插件改后端授权。

仅接受新的实例标准策略；旧 `sync.text/images/files` 为未知键，启动和热加载均拒绝，不提供映射或兼容读取。坏配置保留上一份有效值；模板、config 输出、SDK、桌面和测试使用新形状。

Engine 的旧 SyncSettings.Text/Images/Files 在迁移提交中显式弃用/替换，所有 SDK 调用者、桌面适配、测试和契约同批调整；不能宣称原 Go 结构签名无需变化。当前 v0.1 已实现行为仍见 Engine 实现登记。

## 4. 多后端派发与结果

captured/received 处理器各运行一次，历史 commit 一次，随后 Engine 基于同一最终内容为每个已启用后端独立构造发送视图。plugins.order 只保证处理与生命周期顺序，后端的传输可以并发，不要求等前一后端网络完成。失败、断网、阈值跳过互不阻断，也不重复 commit。首个后端成功不能提前释放其他后端正在持有的内容。

CommitRequest 新增可选 BackendIDs：省略为全部启用后端；非空列表可选择子集，重复、未知、未启用或非同步插件 ID 均 invalid_argument；显式空列表非法，避免与省略混淆。选择项属于幂等语义参数。自动观察默认尝试所有启用后端，普通 SetSync(bool) 控制整个条目，不授予插件绕过全局或指定隐藏后端的能力。

SubmissionResult 增加按 BackendID 排序的 BackendResults，各项含 BackendID、Disposition、稳定 Error；Disposition 为 not_requested/disabled/policy_filtered/auto_file_too_large/no_targets/queued/failed。全局禁发或未配置后端可在聚合层直接表示，未尝试的后端不得伪装发送成功。

保留单个聚合 OperationID，其 Targets 扩展为 `(BackendID, TargetKind, TargetID)`，TargetKind 为 device 或 repository。P2P 对每个在线设备建立目标；对象存储按该后端的已配置 repository 建立目标，不把 WebDAV 假装成一个已在线设备。TargetID 是公开不透明标识，不是 URL、路径、凭据或秘密地址。同一设备经两个后端发送时是两个独立目标。

任一后端有实际任务时，聚合 SyncDisposition=queued，其他后端的失败/跳过仍在 BackendResults 表示；Operation 仅涵盖实际建立的目标，其成功不表示所有配置后端都同步。无任务时优先级为：全局关闭→disabled，Policy 禁发→not_requested，无启用后端→no_backend，任一后端错误→failed，全部实际通过策略但无目标→no_targets，其余→新增 policy_filtered。宿主必须展示按后端结果，不能仅以聚合字段宣称全面成功。

每设备“一个在途加一个待发”扩展为每 `(BackendID, TargetKind, TargetID)` 一组，保留替换/superseded 语义。对象存储的持久队列、游标、重试与留存另定协议；默认不因为 WebDAV 存在就承诺可靠离线投递。关闭全局开关后不得继续消耗任何重试任务，恢复也不得自动重试结果不确定的上传。

## 5. P2P 与对象存储的接口边界

SyncBackend 必须声明 TransportKind=peer_sessions 或 object_store；初始 tailcat 属于 peer_sessions。通用边界包含生命周期、标准策略快照、单目标传输、准入与流式内容；P2P 的地址/配对/hello/session_token 扩展只授予 peer_sessions，不能强迫 WebDAV 实现虚假的设备连接和 PairingAddress。

[同步后端业务契约](../sync-backend-contract.md) 中现有连接 DTO 是 P2P profile 的目标定义；该 profile 的身份/关联与设备协议按 0008 修订，不要求旧 wire v2 互通。object_store 的远端对象格式、设备身份验证/授权、内容保护、对象命名、游标及完成语义须在 WebDAV 实现前补齐；不能让“共享一个 WebDAV 目录”自动等于信任其中所有内容。普通 HTTP 成功不代表其他设备已接收/应用。

每个后端实例独立运行对象、binding_id、状态修订、短期 token 和资源目录；builtin tailcat 在宿主进程内，外部插件每实例独立进程。SyncState 只含该后端的标准策略及必要目标信息；不同 binding 的句柄不可交叉使用。Engine 持有全局授权，撤销一个设备时需在所有相关后端取得撤销屏障；外部实例可回收不响应的进程，builtin 须关闭端点并等待任务退出，不能确认则停止同步服务并报告失败；只停掉 P2P 不能算撤销成功。全局 MaxStoredBytes 覆盖所有后端共享的内容与接收预留，不给每个插件复制一份无限预算。

多个启用后端按既有启动语义全部成功后 Engine 才算启动成功；其中一个启动失败逆序清理，不能静默变成另一个后端成功的降级模式。运行期单实例可恢复故障只使其任务失败并按自己的预算恢复，其他后端与本机历史继续工作。外部后端进程崩溃按预算重启；builtin 的进程级崩溃会影响整个宿主。同步后端数量按创建选项 MaxSyncBackends 限制，默认 8、合法 1..32，0 个启用后端仍允许本机模式。

插件目录、锁、身份初始化、默认装配与 Network 测试入口见[装配契约](../backend-migration-contract.md)。Engine 新建统一设备身份，各实例新建自己的传输身份，不导入旧 tc.*；程序版本升级复用实例私有数据，不把密钥复制给其他实例。

## 6. 跨后端去重与防回环

Engine 为一次本机 commit 分配一个随机 128 位 TransferID，与原始 SourceDeviceID 组成传输身份，所有后端及目标共用；P2P 在现有 wire Clip.ID/Origin 字段承载，不改 wire v2 格式。不能为每个后端/设备重新生成 ID，也不能用正文 hash 去重掉用户两次明确复制/提交。

由于类型策略可产生不同发送视图，去重键为 `(经过认证的 SourceDeviceID, TransferID, ViewHash)`，ViewHash 为该最终发送视图的现有 wire hash。相同视图跨后端到达才去重；不同视图独立接收/入历史，首版不合并或补写已提交历史，不能为了“只显示一条”丢掉另一路独有格式。文件 hash 仍只有名称/大小，保留既有精度限制，不冒充文件字节指纹。原始来源不可直接信任对端声称的 wire origin。首版不转发，因此要求 origin 等于已认证的发送设备；接收插件改变本地 Item 来源不能改变去重身份。支持对象存储前必须定义可验证的设备来源，不能用 repository ID 当设备身份。

Engine 在接收准入时按上述去重键取得并发占位；一个副本处理/commit 成功后其他副本只 drain 或丢弃，不再处理、建历史、应用或再转发。首个副本失败可释放占位，让后续新到达副本尝试；已经丢弃的副本不承诺缓存和重新恢复。同一键但可检出的规范化元数据不一致属于冲突，不覆盖已有内容；已判定重复后 drain 的正文不作增强校验，不承诺识别 wire hash 本身无法识别的文件变化。

去重创建选项 ResourcePolicy.MaxSyncDedupEntries（默认 100000，>0）与 SyncDedupTTL（默认 7 天，>0）只在 New 设置。去重记录跨重启持久化，但首版有界：默认最多 100000 个去重键、成功接收后 7 天过期，容量压力提前淘汰最旧项；占位只实例内，重启清理。只有成功 commit 才写成功去重记录（含 persist=false），与对应 commit 同一事务提交，不把 wire hash 当文件字节校验。超过保留范围不能保证避免跨后端重复，仓库型后端须使用自己的游标/留存规则进一步约束，不能宣称已有永久去重保证。

## 7. 验收与交付范围

- 全局关闭时所有后端自动/显式发送、接收、内容轮询和重试均停止，本机历史/Action 仍可用；关闭/撤销竞争不能泄出后续新任务。
- 两个假后端分别启用/禁用、按类型过滤、文件阈值等于/超过边界；同一 50MB 文件按 100MB/10MB 策略独立决策，手动只跳过阈值。
- 处理与历史只执行一次；多后端结果、共享资源引用、目标替换与失败隔离正确；总配额不因多实例放大。
- 同一传输同一视图经两个后端并发到达只 commit 一次，不同类型视图不被错误吞掉，原始两次提交不被内容去重吞掉，重启/失败占位/去重过期边界明确。
- 旧配置键拒绝、坏配置保留、策略热加载/实例重启项，以及 SDK/桌面新接口有测试。
- 首版实际交付 tailcat 与多假后端契约测试；WebDAV 具体实现不在本决策中冒充完成，但接口不得固化成仅一个 P2P 后端。

## 实现登记（2026-11-08）

已实现（`engine/backend.go`，有 `backend_test.go` 用多个假后端验收）：

- 包内私有 `syncBackend` 接口（ID/Kind/Policy/Submit）与 `backendHost`（Register/Report）；tailcat 是包装现有 session 的内置适配器 `builtin.tailcat`。接口在 P4 的 RPC 能承载前不公开。
- 提交处理一次、历史 commit 一次，再按后端独立构造发送视图：类型过滤、`auto_file_max_bytes`（自动提交受限，显式提交越过）、大小硬限制；所有后端共用同一个 TransferID/时间戳（`backendView.id`）。
- `SubmissionResult.BackendResults`（按 BackendID 排序）与聚合 `SyncDisposition`：任一 queued → queued；否则 failed > disabled > no_targets > policy_filtered；全局关闭 → disabled，Policy 禁发 → not_requested，无后端 → 新增 `no_backend`。单个后端失败/跳过不影响其他后端和 commit。
- 单个聚合 Operation 汇总各后端目标；`TargetStatus` 新增 `BackendID/TargetKind/TargetID`（`PeerID` 仅 device，保持兼容）；注册窗口内（后端仍可能登记目标）Operation 不会提前 completed。
- `CommitRequest.BackendIDs`：省略 = 全部；空列表、未知、重复 → invalid_argument；规范化排序后参与幂等比较。
- 旧 `SubmitContent`/观察路径在只有 tailcat 时行为不变（既有全部测试通过）。

**策略迁移已完成（2026-11-08，不保留兼容）**：`Settings.Sync` 只剩 `Enabled`；新增 `Settings.BackendPolicies map[PluginID]BackendSyncPolicy`（Text/Images/Files/AutoFileMaxBytes），由 Engine 持有并执行，`New`/`UpdateSettings` 校验（未知后端、阈值越界 → invalid_argument）并补全默认，读取返回完整深拷贝，CAS 热更新对之后的提交与传输前复核生效；发送、接收过滤、能力限制（`text_disabled` 等：只有所有后端都不允许某类型才受限，无后端时不限制）和 `appliable` 都读它。桌面 `config.toml` 删除了 `sync.text/images/files`，改为 `[plugins.instances."builtin.tailcat".sync]`（省略的布尔项为 true，`auto_file_max_bytes` 默认 0），旧键现在是未知键错误，不做映射。后端接口不再有 `Policy()`，策略归 Engine。

**接收与去重已完成（2026-11-08）**：后端无关的 `admitReceive`（`engine/receive.go`）在读正文前检查运行状态、全局开关、wire origin 等于认证来源、本后端策略至少放行一个 part、跨后端去重键 `(认证来源, TransferID, ViewHash)`（实例内占位 + 持久 `sync_dedup`，默认 10 万条/7 天，`ResourcePolicy.MaxSyncDedupEntries/SyncDedupTTL`）和容量；成功记录与历史同事务，persist=false 也记；不同视图独立接收；Received 记录所属后端，应用时按该后端策略判断。发布前被回滚则一并删除去重记录。

仍是过渡形态、未实现：

- 实例启用/顺序/`events` 等插件装配配置（`[plugins].order`、`.enabled` 仍未支持，桌面配置只接受 `[plugins.instances."builtin.tailcat".sync]`）。
- 历史条目的各后端同步状态摘要（`SyncState` 仍恒为 not_requested）。
- `MaxSyncBackends`、撤销屏障、对象存储（`repository` 目标）实际协议与外部进程后端。
