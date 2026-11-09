# 0004 · 事件驱动的剪贴板处理流水线 + Go / Lua 插件

- 状态：已采纳并实施：流水线、历史、Action、Go/Lua 插件、builtin.tailcat 与桌面配置已实现；GitHub 安装、真机双设备基线未完成；SwiftUI 接入已取消，桌面界面采用 mygo + 自定义 UI Kit，原型待定（见[执行计划](../../exec/0004-clipboard-pipeline-lua.md)）。
- 日期：2026-11-07（2026-11-08 修订：取消自动写回，增加 Action；同步后端按 0007/0008 调整为统一设备身份与实例关联）
- 关联：[0002 配置](0002-config-file.md)、[0003 Engine SDK](0003-engine-sdk.md)、[0007 多后端](0007-multiple-sync-backends.md)、[0008 设备身份与后端关联](../archive/0008-device-identity-and-backend-bindings.md)
- 参考：架构图（Capture → Processing → Commit → Sync → Use → End，事件总线，Lua 插件 API）

## 背景与目标

现在本机复制后，桌面宿主轮询到变化（`app/desktop/internal/host/clipboard.go` 的 `tick`）就交给 `Engine.ObserveClipboard`（`engine/clipboard.go`）直接同步，中间没有可介入的位置；接收远端内容同理（`applyLoop` → `Engine.BeginApply`）。

目标：用户复制后，内容先经过一条**处理流水线**，Go 或 Lua 插件可在其中自动打标签、控制是否同步/持久化、设置过期时间（只标注，不改写正文）；用户另可主动触发插件声明的 Action 来变换内容；同步通过 Go 后端插件执行，默认交付进程内 builtin 插件 **`builtin.tailcat`**（源码位于 `internal/plugins/tailcat/`）；Engine 统一管理设备身份、信任、配对审批和设备后端关联，各插件提供自身接入机制。设备管理按设备归并，并展示其各插件实例的授权与同步状态。

## 决策

### 1. 生命周期

```text
Capture/Receive → Processing → Commit → Sync → Use → End
   (捕获/接收)     (自动化)    (入历史)  (同步)  (使用)  (删除/过期)
```

| 阶段 | 事件 | 类型 | 说明 |
|---|---|---|---|
| Capture | `clipboard.captured`（本机）、`clipboard.received`（远端） | **可干预** | 同步串行调用插件，只能改标签与策略（过期、persist、sync），不能改正文 |
| Commit | `clipboard.committed` | 通知 | 最终内容与策略已确定；按 persist 决定是否写历史，此后只读 |
| Sync | `clipboard.sync.requested/started/completed/failed` | 通知 | 由 Engine 根据同步插件操作状态发出 |
| Receive | `clipboard.receive.started/completed/failed` | 通知 | 远端内容接收；completed 表示本机 commit 成功，不表示应用或远端 ACK |
| Use | `clipboard.pasted` | 通知 | 用户经 UI 使用；系统直接粘贴无法观察，不承诺 |
| Action | （无事件订阅） | 用户触发 | 用户在界面对某条历史运行插件声明的 Action，结果作为新条目走显式提交；不自动运行 |
| End | `clipboard.deleted`、`clipboard.expired` | 通知 | 用户删除或到期 |
| 其他 | `clipboard.updated`、`app.started/shutdown`，以及设备/关联状态事件（具体枚举待定义） | 通知 | 设备与路径状态分开；连接事件仅适用于 P2P，不能把仓库可访问表示成设备在线 |

“可干预”只有 captured/received 两个，其余事件只读；干预也仅限标注。这样同步与存储始终基于一份确定的最终结果。

### 2. 流水线与同步

- 流水线是 Engine 内的有序阶段链；每条内容在阶段间传递一个 `Item` 和一个 `Policy`：
  - `Policy{ persist bool, sync bool, expires_at *time, tags []string }`
  - 默认 `persist=true, sync=true`；全局授权与每后端策略在派发时分别检查，Policy 不能扩大它们。
- **同步由 Go 后端插件执行，默认交付 builtin `builtin.tailcat`**：Engine 在 commit 后通过专用后端接口派发，普通 committed 通知不负责可靠调度；派发读取 `Policy.sync`。按[0007](0007-multiple-sync-backends.md)与[0008](../archive/0008-device-identity-and-backend-bindings.md)，全局 sync.enabled、实例启用、设备信任、关联授权/同步开关及每实例 text/images/files/自动文件阈值共同约束发送，set_sync(true) 不能扩大授权。多个后端独立派发、记录结果和占用资源；零后端允许本机 commit。
- `persist=false` 的内容不入历史，但仍可按 `sync` 同步。
- 远端内容走 received 阶段，插件同样只能改标签、过期；接收方的 `set_sync` 只影响本机是否**再转发**，首版不转发（现有语义），所以对 received 实际只用 `persist/tags/expires`。

新增 `CommitContent/GetSubmission` 分开表达 commit 与发送结果；原 `SubmitContent` 保留兼容门面，观察结果增加提交信息。具体状态、失败缓存与生命周期见[流水线提交契约](../../contracts/pipeline-contract.md)，待实现。

三类入口的状态转换、提交屏障、Received 槽位/去重记录原子发布和资源释放见[流水线契约 §7](../../contracts/pipeline-contract.md#7-状态转换与资源交接)。Sync、Apply、历史 End 各自结束，不能用历史删除或一次网络成功代替整条内容的资源回收。

### 3. 防回环与 Action

**取消自动写回（2026-11-08）**：早期设计让插件改写正文并自动写回系统剪贴板。产品上否决了它：用户复制的是 A，粘贴出来变成 B，无法预期；历史里存 B 而剪贴板是 A 同样让人困惑。因此自动阶段只做标注（标签、过期、persist、sync），内容原样入历史并同步，剪贴板始终保持用户复制的内容。随之不再需要写回 guard、待办任务、自动化深度、插件来源链。

改写内容是**用户主动**的 Action：

- 插件在 Spec 中静态声明 Action（ID、Title、Accepts，首版无参数、只支持文本），用户在界面选择某条历史后触发 `RunAction`；Action 不订阅事件，不会自动运行。
- 结果作为 origin=`action` 的新历史条目走显式提交（captured 处理器照常标注它，同步按普通策略），来源条目不变；宿主随后用 `BeginApplyHistory` 写入系统剪贴板，回声由既有应用票据 AppliedToken 抑制。
- 因为处理器改不了正文，处理器与 Action 之间不存在循环，唯一的回声来源就是宿主自己的写入，沿用 0003 的 apply token / 基线 hash。
- 契约见 [Action 契约](../../contracts/clipboard-action-contract.md)。Action 与历史使用在同步关闭时仍可工作。

### 4. 插件实现方式与 Lua 沙箱

- 插件允许使用 **Go 或 Lua** 实现，共享同一套事件、Item/Policy 能力、执行顺序与错误策略；Lua 是插件接口的脚本适配层。
- Go 插件采用独立常驻进程 + RPC，不编译进宿主、不使用 Go 动态库；通过 `Options.PluginProcesses` 传入安装描述，详见[插件契约](../../contracts/plugin-contract.md)。
- Go 插件为受信任的本机程序，进程隔离不等于文件/网络权限沙箱。RPC 超时先取消，宽限后可终止插件进程；失败回滚 Item/Policy 暂存变更，外部副作用不能回滚。
- Go/Lua 混用时须有统一、确定的执行顺序，不能由注册时机隐式决定；使用显式 `plugins.order`，详见插件契约。

#### Lua 运行时


- 使用基于 `gopher-lua v1.1.2` 的纯 Go 增强 VM；上游没有公开指令计数 hook，SetContext 不能抢占原生 Go 函数。实现与验收见 [Lua 运行时契约](../../contracts/lua-runtime-contract.md)，当前仅完成上游行为探针。
- 基础函数及 string/table/math 逐项白名单注册，原生循环/分配须有界；utf8 由宿主提供。不开放动态加载、环境/元表修改、文件、网络或进程能力，不能只从 OpenBase 删除三个函数。
- 所有脚本执行边界默认 200ms 与 1,000,000 条指令；编译、原生操作另有预算。调用栈/注册表、文本及累计逻辑分配分别限制，不承诺精确堆内存隔离，禁止使用会退出宿主进程的 SetMx。
- 每个插件独立的 Lua 状态；回调串行执行，不共享全局表。执行失败销毁该状态，后续调用按错误策略处理，等待显式重载；不重放失败内容。
- 插件错误：丢弃该插件本次对 Item/Policy 的改动，继续后面的插件，不阻断流水线。**注意**：这意味着“敏感信息检测”类插件出错时内容会照常同步。提供 `plugins.on_error = "continue" | "block_sync"`，默认 `continue`，写入配置模板注释说明风险。
- 日志与事件**不含剪贴板正文**，只含插件名、事件名、错误码；Lua 错误信息在记录前脱敏（可能夹带正文）。

### 5. 统一插件接口与模板

[插件契约 v0.1](../../contracts/plugin-contract.md) 确定标识、能力、注册、Init/Start/Close 生命周期、配置、执行顺序、处理上下文、错误回滚和同步后端边界，并提供 Go/Lua 最小模板。接口为目标设计，尚未实现。

- 统一描述：ID、Version、APIVersion、Capabilities（processor/observer/action/sync_backend）；action 额外静态声明 Actions；builtin Go 插件由宿主显式装配到进程内，外部 Go 插件使用独立程序与 RPC 握手，Lua 返回插件表，不再使用全局 `events.on`。
- 普通插件分 processor（自动标注）、observer（只读通知）与 action（用户触发的内容变换）；sync_backend 是仅 Go 可用的扩展能力，默认由 builtin `builtin.tailcat` 提供。该实现放在 `internal/plugins/tailcat/`，保留独立 ID `builtin.tailcat`，不并入现有 `builtin.text` 的处理/Action 能力；源码同目录不表示共用配置、生命周期或后端实例。builtin 随宿主构建和升级，不经 GitHub 安装，也不要求单独的 tailcat 可执行文件。
- 标签、持久化、同步与过期通过 ProcessContext 方法暂存（无改正文方法），回调成功后原子采纳。通知不带正文，不能修改已提交内容。
- Lua 首版不提供网络、本地存储、Quick Panel；Go 为受信任代码。同步后端具有专用网络与接收接口，不能将这些能力授予 Lua 或普通通知插件。

Go 独立进程的核心传输规则见[RPC 核心协议](../../contracts/plugin-rpc-contract.md)，含机器可读帧表、Schema 和互操作向量。[同步后端契约](../../contracts/sync-backend-contract.md) 的初版 P2P DTO 尚须按 0008 修订；统一身份握手、后端实例标识、关联授权 DTO 及对应 Schema/向量须先补齐，不能以现有本机 RPC 通过代替设备身份认证验收。

#### 同步后端、设备身份与配对

- Engine 持有独立于 tailcat 网络密钥的设备身份与私钥，统一管理远端身份、公钥绑定、信任、候选、配对操作、票据和审批。插件持有本实例的传输密钥或仓库凭据，不打开 Engine 数据库，不取得设备私钥，不另建授信系统。
- 插件按能力提供发现、地址接入、连接或仓库访问，并传送 Engine 可验证的身份材料；不要求所有插件实现相同的发现或 P2P 拨号。具备相应能力的其他插件应能独立完成设备添加，不依赖 tailcat。
- 首次配对由 Engine 验证持钥证明、配对上下文和身份核对值，再按用户审批建立设备信任与所选关联授权。配对码只帮助发现目标，通过配对码进入的一方始终需要出码方批准。同名、自报 DeviceID 或共享仓库不能直接授信。
- 同一已验证设备只建一条设备记录，各后端实例分别建立 DeviceBackendBinding；已有设备增加路径仍须证明相同身份、绑定本次连接/仓库上下文并由用户确认，不因设备已信任而自动启用所有路径。插件实现与配置实例标识分离，同一实现可配置多个实例。
- 设备列表展示每台设备及其各插件实例的授权、同步开关、可达性和最近活动。P2P 报告连接状态；对象存储报告仓库/成员与同步状态，仓库可访问不表示设备在线。插件禁用/卸载保留设备与关联记录，其他路径继续工作。
- 暂停一条关联同步或移除该关联只影响该路径；解除设备配对撤销全部路径。Engine 在撤销提交点禁止新授权，成功返回前需取得相关后端屏障；外部后端可终止并回收不响应进程，builtin 后端须关闭端点并等待任务退出，不能把取消请求当作已完成撤销；无法确认停止时 Engine 必须报告失败并停止同步服务，迟到响应不得恢复旧权限。

上述概念、权限交集与状态语义以[设备契约](../../contracts/device-contract.md)为准；密码学编码与具体 SDK/RPC 类型仍是实现前置任务。

### 6. 插件加载与配置

- 完整配置形状与顺序见插件契约：所有启用实例在 `plugins.order` 中显式排列，不能以文件名或注册时机决定顺序。
- 支持 `unic plugin install https://github.com/owner/repo` 从仓库清单安装，版本锁定到 commit；Lua 下载加载，Go 优先匹配平台预编译，缺失时可 --build 构建插件自身，无需重建宿主，详细命令、清单、安装锁与回滚见[插件契约 §8](../../contracts/plugin-contract.md#8-从-github-仓库安装)。
- Lua 脚本由宿主从 `$UNIC_HOME/plugins/*.lua` 或安装锁指定的已安装目录发现、读取并传给 Engine；监听与防抖留在宿主，Engine 不依赖文件系统监听。
- Lua 脚本与专属配置热替换失败保留上一有效实例，启动时错误则拒绝启动；启用列表、顺序、订阅与 Go 实现变更需重启。
- Go 插件由 Engine 按显式装配启动：builtin 每个配置实例创建独立进程内对象，外部插件每实例独立常驻进程；两者均独立管理数据目录、运行锁与授权。tailcat 的 Send 复用长连接。桌面发行版内置 `builtin.tailcat` 并在首次默认配置中显式启用；SDK 零装配和用户明确禁用全部后端时为本机模式，不隐式启动 tailcat。默认只交付 tailcat，其他后端通过同一装配与设备关联契约接入，WebDAV 等具体实现另定。
- 插件不能通过业务接口修改 `limits.*` 与设备配对状态；Go 代码必须遵守同样的契约。

builtin 显式装配、外部程序的发行清单/摘要校验、私有目录/锁、Engine 与后端各自的身份初始化，以及 Options.Network 同批移除，按[装配契约](../../contracts/backend-migration-contract.md)执行。产品未上线，不导入旧 tailcat 密钥、地址或配对记录，不保留旧 SDK/设备协议兼容层。已有身份损坏须报错，不静默重建；启用后端启动失败须清理并报告失败，不绕过插件另启备用网络。

### 7. 数据模型与存储

SQLite 新增历史表（本机，不进 wire）：

```text
clip_items(id, hash, origin, origin_device, kind, created_at, updated_at,
           expires_at, persist, sync_state, source_app)
clip_item_parts(item_id, format, data | blob_ref)
clip_item_files(item_id, name, size, blob_ref)
clip_tags(item_id, tag)
```

- 大于阈值的内容与文件存私有目录，表里只存引用；不要求公共调用者提供路径（0003 §2）。
- 过期由定时清理与启动清理处理；只删除 Engine 自己拥有的资源（0002：绝不整目录删除 inbox）。
- 历史容量设上限（条数与字节），超限按最旧、未收藏优先淘汰。
- 全文索引、收藏、集合不在首版。

### 8. 标签与过期不同步

标签和过期时间只在本机历史里，不加入同步内容协议；接收方由自己的插件决定自己的标签与过期。设备协议按 0008 的统一身份与关联要求调整，不要求旧 wire v2 互通。

## 历史与流水线归属：Engine

历史由 Engine 管理，历史与流水线的业务实现都放在根目录 `engine/` 包内，不采用先放根 `internal/`、以后再决定是否公开的方案。各平台通过同一套 Engine API 复用历史与处理语义。

- Engine 负责历史入库、查询、内容读取、更新、删除、过期与容量淘汰，以及流水线调度、插件执行和同步策略。底层 SQLite 等基础设施仍可通过内部边界复用，公开签名不泄漏内部类型。
- 公开 SDK 增补历史查询与操作能力（如 `ListHistory`、`GetItem`、`DeleteItem`），事件只携带对象标识，正文通过显式读取获取。具体签名、筛选分页、跨重启历史 ID、内容租约、容量/到期和历史应用语义已在[本机历史契约](../../contracts/history-contract.md) 确定，实现与契约测试仍待完成。
- 历史是跨重启持久化的本机数据，区别于现有实例内的 `Received` 接收列表；不增加历史跨设备同步或重启后的待应用可靠投递保证。
- 宿主负责系统剪贴板读写、权限、UI 和通知。桌面 daemon 的历史 HTTP API 只适配 Engine，不另建历史存储或流水线；移动端绑定沿用同一业务契约。
- 0003 与公共契约中的“不承诺历史”仅描述既有 v0.1 接收列表；本决策扩展 Engine 的职责，实现进度仍以契约登记为准。

## 实施顺序

1. **流水线骨架**：在 `engine/` 内把现有观察、提交与接收应用流程重构为阶段链 + 同步插件派发，行为与现在完全一致（回归测试先行）。
2. **历史存储**：在 `engine/` 内实现历史管理，新增 `clip_items` 等表、commit 阶段写入、过期清理；`unic history` 查看。
3. **事件总线**：把现有 `Event` 扩为带阶段与 origin 的结构，增加设备与关联状态通知；SDK 与桌面 API 同批调整，不保留旧 `/v1` 事件兼容形状。
4. **插件接口与运行时**（Lua 运行时、适配、热替换与模板已实现；Go RPC 同步后端业务层见第 5 步）：统一 Go RPC/进程管理接口与 Lua 适配（含 Action 注册/运行），提供两种语言的最小模板；Lua 沙箱、超时、插件表与 `ctx` API、热加载；错误与脱敏。
5. **统一设备与同步后端**：先补齐统一身份/配对网络协议、实现与实例标识、设备关联 DTO、RPC Schema/向量；再实现 Engine 设备目录、信任、关联审批、设备列表与撤销屏障。在 `internal/plugins/tailcat/` 实现 builtin `builtin.tailcat`，由宿主显式创建实例并装配，只复用适合新边界的网络/传输基础设施，重构 session 对身份、信任和数据库的直接依赖，不保留旧 wire 互通要求；以多个假后端验证统一设备与独立路径。
6. **示例扩展**（Lua 示例已在 `plugins/examples/lua/`：emailtag 模板、sensitive、textcase，由 `engine/lua_test.go` 验证）：邮箱/URL 检测自动打标签、敏感信息检测、自动过期、内容分类（标注型），以及“转大写”等 Action，用同一套 Lua/内部接口实现，作为示例与测试。

## 验收

多后端、全局开关、每后端类型/自动文件阈值与跨后端去重按[0007](0007-multiple-sync-backends.md)验收；统一身份、配对和设备后端关联按[0008](../archive/0008-device-identity-and-backend-bindings.md)及[设备契约](../../contracts/device-contract.md)验收。

- `go build ...` 与 `go test -race ./...` 通过。
- 无用户处理插件、使用默认 `builtin.tailcat` 时，新协议双节点的文本/HTML/PNG/文件同步、防回环与配置热加载符合契约；不要求旧身份/地址或旧 wire v2 对端互通。
- 相同设备经多个后端发现和验证后只显示一条设备记录，各关联授权/状态独立；同名不同身份不合并，伪造 ID、密钥不符和重放不能继承信任。已信任设备的新路径未经确认不能同步，配对码入站始终需要批准。
- 单路径暂停/移除不影响其他路径，解除设备配对在全部关联后端生效；禁用/卸载插件保留设备。用假后端验证不装 tailcat 的设备添加、多实例隔离和对象存储状态不冒充设备在线。
- 全新设备/传输身份独立初始化并持久化，坏身份不静默重建；通过正常装配的 builtin tailcat 实例验证常驻会话、故障恢复、撤销屏障和资源回收；外部插件另以真实子进程验证 RPC、崩溃恢复和回收。
- 插件：加标签（例如含邮箱自动打 `email`）、`set_sync(false)` 不发送、`set_expire` 到期删除；Action 由用户触发，改文本只能经 Action；Lua 插件超时/死循环/抛错不拖垮 Engine，不影响其他插件。
- 沙箱：Lua 插件拿不到 `os/io/require`，无法读写文件或联网。
- 防回环：宿主应用历史或 Action 结果的回声不产生第二条历史；处理器不能改正文。
- 日志、事件、错误中无剪贴板正文。

## 本阶段不做

- 普通处理/通知/Action 插件的通用网络与存储扩展权限、Quick Panel、带参数的 Action、非文本 Action、WASM 插件、原生动态库加载、可视化规则编辑；同步后端所需的身份验证、关联授权与私有凭据隔离属于本阶段范围。
- 全文检索、收藏、集合、历史跨设备同步、标签/过期随内容同步。
- WebDAV/Iroh/LAN/Relay/BLE 的具体实现；本次实现统一设备/关联、多后端装配与独立策略，以 tailcat 和假后端验收；对象存储具体成员/内容保护协议另定，不实现任意脚本路由。
- 清空系统剪贴板的“到期清除”：先只删除历史记录，是否清空系统剪贴板另立决策。
