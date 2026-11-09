# 插件契约 v0.1

> 0008 目标修订：设备身份、信任与各实例关联授权以[设备契约](device-contract.md)为准；实现标识和配置实例标识须分离，同实现可配置多个实例。后端发现/连接不授信，SyncState 仅含该实例已授权关联和必要候选。下文 P2P 接口草案须补齐身份证明/关联 DTO 后才能作为实现依据。

- 状态：目标设计，待实现；本文接口与模板当前不可直接编译运行。
- 依据：[0004](decisions/0004-clipboard-pipeline-lua.md)、[Engine 契约](engine-contract.md)。
- 范围：Go / Lua 统一模型、builtin 进程内装配与外部独立进程 RPC、默认同步插件及 GitHub 安装。历史、流水线与插件调度在 `engine/`。

## 1. 标识、能力与入口

插件描述包含 ID、Version、APIVersion、Capabilities；sync_backend 另声明 TransportKind=peer_sessions/object_store，普通插件不得声明该字段。sync_backend 还可声明 `ExternalChannels`（对端是没有 unic 身份的外部渠道，见 0009）与 `RemoteRevocation`（须同时声明 ExternalChannels，并实现 `ChannelLeaver`）；其他插件声明这两项被拒绝。外部渠道后端实现 `ReceiverAware`，Engine 在 Init 之后、Start 之前交给它 `Receiver`，收到的内容经 `Receive(channelID, Received)` 进入 Engine；被拒（渠道停用、策略、空间、重复）时返回错误，后端丢弃且不得重试。当前只支持进程内 Go 实例，外部进程后端待 RPC 业务层。ID 匹配 `[a-z][a-z0-9]*(\.[a-z][a-z0-9_-]*)+`，最多 128 字节；版本为语义版本，API 首版为 1。实例内 ID 唯一，`builtin.tailcat` 保留给发行版内置同步插件。能力为 processor（自动标注）、observer（只读通知）、action（用户主动触发的内容变换）、sync_backend（同步后端）。Lua 首版支持前三项；声明能力还须通过 Engine 校验和配置授权。

processor 只能改标签、过期、persist、sync，不能改写正文；改写正文只能通过 action，见[Action 契约](clipboard-action-contract.md)。action 能力须在 Spec 中静态声明 `Actions []ActionSpec{ID, Title, Accepts}`：ID 匹配 `[a-z][a-z0-9_-]*`、最多 64 字节且插件内唯一，对外键为 `pluginID/actionID`；Title 为 1..64 字节的纯 UTF-8；Accepts ⊆ {text, image, files}，首版仅 text 可用（来源与结果都是文本）。首版没有参数。声明了 Actions 却无 action 能力，或反之，均为 invalid_config。

**Go 插件区分 builtin 与 external 两种装配方式**。builtin 由宿主显式创建进程内实例，随宿主构建和升级，不走安装器或 RPC；默认同步后端 `builtin.tailcat` 位于 `internal/plugins/tailcat/`，与 `builtin.text` 保持独立 ID、配置和生命周期。两种方式执行相同的能力校验、实例授权和业务契约，builtin 不因内置而绕过设备审批。

**外部 Go 插件使用独立可执行程序 + RPC**。宿主传入 `Options.PluginProcesses []PluginProcessSpec`（ID、可执行路径、摘要、API 版本），Engine 启动与管理子进程。路径仅供装配，不进入普通状态与事件。每个 Engine 的每个启用插件各有一个常驻进程，跨处理/发送复用，不编译进宿主，不使用 Go 动态库或全局 init 注册。

Lua 的预算、白名单、失败状态与热替换约束见 [Lua 运行时契约](lua-runtime-contract.md)；上游行为已核实，增强 VM 尚未实现。Lua 返回插件表，由 Engine 内适配器运行。宿主负责文件读取、监听和 TOML 映射；Engine 不依赖桌面文件监听。

公开 SDK 规划为 `github.com/uniclipboard/unic/plugin`，不依赖 engine；Engine 依赖它的 DTO 和 RPC 客户端。以下是插件进程内部接口，跨 RPC 传请求/响应，不传 Go 指针、interface 或 context：

```go
type Spec struct {
    ID string
    Version string
    APIVersion int
    Capabilities []string
    Actions []ActionSpec // 仅 action 能力
}
type ActionSpec struct {
    ID string
    Title string
    Accepts []string
}
type Config map[string]any // 只接受 JSON 值

type Plugin interface {
    Spec() Spec
    Init(context.Context, Host, Config) error
    Start(context.Context) error
    Close(context.Context) error
}
type Processor interface {
    Plugin
    Process(context.Context, ProcessContext) error
}
type Observer interface {
    Plugin
    OnEvent(context.Context, Event) error
}
type Actor interface {
    Plugin
    Run(context.Context, ActionContext) error
}
```

Plugin SDK 的 Main 函数服务 RPC，Engine 使用代理；Lua 实现相同业务语义，不需要额外进程。Go 进程隔离不等于系统权限沙箱，第三方程序仍是受信任的本机代码。

## 2. 生命周期与失败

Go：spawn → handshake → Init → Start → callbacks → Close → wait。Lua：load → Init → Start → callbacks → Close。Init 校验配置并准备资源，不联网；Start 后可以运行后台任务。启动任一步失败，Engine 逆序清理所有部分初始化实例，并报告启动失败。

同一实例的 Process/OnEvent 串行，不同内容也遵守；Sync Send 允许跨目标并发。RPC 读写不能被回调阻塞，Engine 不持业务锁调用插件。处理超时默认 200ms，Go 先发取消，再给 1 秒宽限，仍不响应则终止子进程并回收；Lua 终止该次调用并丢弃失败状态，等待显式重载；后续调用仍遵循错误策略。SDK 恢复 Go 回调 panic，返回稳定错误码。迟到响应不得提交。

Stop 先取消新工作与在途请求，再 Close；默认 5 秒退出窗口，超时结束插件进程树并 wait，释放流、预留配额和句柄。Close 幂等，可清理部分初始化实例，使用独立清理 context。未完成回收不能报告资源已释放。父进程管道 EOF 时 SDK 取消后台任务并退出，避免孤儿进程继续持有网络连接。

处理失败丢弃该插件全部暂存变更，再执行后续插件。`on_error=continue|block_sync`，默认 continue；block_sync 设置本条目不可被后续插件解除的禁发标记。普通 SetSync(false) 可被后续正常插件覆盖。外部副作用无法回滚。通知失败不回滚 commit，记录的只有 ID、事件和稳定错误码，丢弃插件自由文本错误与 stderr，避免泄露正文。

崩溃或被强制终止时，处理失败按 on_error 处理；发送失败，接收半成品清理。Engine 以 1s、2s、4s 有界退避重启，60s 内最多 3 次，超过标记 unavailable，显式重启恢复。重启重新握手和初始化，传最新状态；旧会话响应失效，不自动重发未决同步，不因 RPC 中断推断远端未收到。本机历史不因运行期后端故障停止。

## 3. 配置与顺序

插件专属 Config 的静态描述与 GUI 生成见[插件设置 Schema 契约](plugin-settings-contract.md)和[0010](decisions/0010-plugin-settings-schema.md)。独立 `plugin/settings` 工具与 macOS 固定 schema 预览已实现；Spec/清单/RPC 发布、实例配置保存和安装器读取尚未接入，不将 GUI 预览视为运行期配置能力。

SDK 的 PluginSettings 包含 Order、Instances、OnError；每实例包括 Enabled、Events、Config。标准同步策略只存于 Settings.BackendPolicies（Text/Images/Files/AutoFileMaxBytes），不在实例设置重复保存；规范见[0007](decisions/0007-multiple-sync-backends.md)，具体 Go 类型、LuaPlugins 装配、严格配置和错误映射见[SDK 扩展类型契约](sdk-extension-contract.md)。宿主映射 TOML：

```toml
[plugins]
order = ["example.emailtag", "builtin.tailcat"]
on_error = "continue" # 检测失败仍可能同步，可改 block_sync

[plugins.instances."example.emailtag"]
enabled = true
events = ["clipboard.captured", "clipboard.received"]
[plugins.instances."example.emailtag".config]
tag = "email"

[plugins.instances."builtin.tailcat"]
enabled = true
events = [] # 专用同步接口，不依赖通知队列
[plugins.instances."builtin.tailcat".sync]
text = true
images = true
files = true
auto_file_max_bytes = "100MB"
```

所有启用实例在 Order 中恰好一次；不按文件名或注册时机排序。顺序作用于同阶段与生命周期，不能把同步提前到处理/commit 之前。未知事件、能力或 API 不兼容均拒绝；核心先验证 Config 的 JSON 结构/限额，插件 Init 严格递归拒绝未知专属配置键（invalid_config），启动失败或热替换保留旧版本，不能在安装时执行程序做校验。配置不能让普通插件取得同步能力。

发行版默认内置并启用 builtin.tailcat，允许 0..MaxSyncBackends 个后端实例并存（builtin 为独立对象，external 为独立进程），0 为本机历史模式。全局 sync.enabled=false 立即取消所有后端内容发送/接收/远端自动应用及重试，控制连接与配对可保留；本机历史/Action 不受影响。各后端独立类型开关和自动文件阈值由 Engine 统一执行，手动提交仅可越过自动阈值。

首版启用列表、顺序、订阅、Go 二进制升级与后端切换在 Engine 重启时生效，不重建宿主。各后端标准同步策略允许热更新；Lua 同 ID/版本/API/能力的脚本与专属配置允许原子热替换：先验证并 Init/Start 新实例，再在回调边界切换，失败保留旧实例，最后关闭旧实例。

## 4. 处理上下文

ProcessContext 提供 Event()、Item()、Policy() 快照及下列方法：

```go
func (c *ProcessContext) AddTag(string) error
func (c *ProcessContext) RemoveTag(string) error
func (c *ProcessContext) SetPersist(bool) error
func (c *ProcessContext) SetSync(bool) error
func (c *ProcessContext) SetExpire(time.Duration) error
```

Item 包含 ID、Type、Text（只读，供检测邮箱等用途）、Formats、Origin（Kind/DeviceID/PluginID）、SourceApp 和 Files（仅名称/大小）。SourceApp 可为空；不提供文件路径、票据或秘密地址。Policy 包含 Persist、Sync、ExpiresAtMs、Tags。回调内部读取反映暂存修改；Process 结束返回 patch，Engine 校验代次、权限、正文、格式与限制后原子采纳。不能跨 RPC 修改引擎内存或保留回调对象。

RPC 的 SetExpire 使用正整数毫秒，小于 1ms 或非整毫秒返回 invalid_argument；不跨语言隐式舍入。processor 不提供改正文的方法。标签最多 32 个，每个 64 UTF-8 字节、去重；过期需正时长，自 commit 起算。最终同步策略受全局配置过滤、信任和不可撤销禁发标记约束。

Host 只提供 Notify(ctx, message)，限流并限制长度，由宿主展示，不进入事件或日志，失败不阻断。PluginEvent 只有 Kind、ItemID、AtMs，不含正文；观察者不能修改 commit 后内容。普通 Host 不提供再入 Engine 提交或数据库访问。

Lua 处理器使用 ctx.item、ctx.policy 及 ctx:add_tag/remove_tag/set_persist/set_sync/set_expire；时长如 "30s"。方法错误回滚整次回调。订阅由配置控制，不使用全局 events.on。

**Action 上下文**：Go 的 `ActionContext` 提供 `Action() string`、只读 `Item()` 快照（同上，含 Text）和 `SetText(string) error`，没有标签/策略方法；`Host.Notify` 可用。必须恰好调用一次 SetText，规则、限制与失败语义见[Action 契约](clipboard-action-contract.md)。Lua 在插件表声明 `actions = {{id=..., title=..., accepts={"text"}}}` 并实现 `run_action(ctx)`，ctx.action 为 ID，ctx.item 只读，`ctx:set_text(s)`、`ctx:notify(msg)` 可用。Action 不订阅事件，只在用户触发时运行；Go 经 RPC `plugin.run_action`（见 [RPC 核心协议](plugin-rpc-contract.md)）。

## 5. Commit 与资源

提交结果、无网络任务的幂等、调用者取消和旧发送门面映射遵循[流水线提交契约](pipeline-contract.md)。接纳后使用 Engine 工作 context；处理 deadline 与 Stop/挂起仍可取消在途处理，调用者停止等待不能让已接纳请求被重复执行。

三类入口的阶段、提交屏障、接收槽位/去重记录发布与引用账本见[流水线契约 §7](pipeline-contract.md#7-状态转换与资源交接)。每个插件 patch 独立持有暂存资源，失败只释放自身暂存；回调结束使其上下文和句柄失效，迟到响应不能取得新引用或提交。

历史 ID、基础查询、持久快照、租约、删除/到期与配额遵循[本机历史契约](history-contract.md)。插件 Item.ID 是处理对象 ID，不得假定它就是 HistoryID；仅 persist=true 的成功 commit 建立历史映射。标签/期限的后续管理不重新触发 processor；使用已提交历史不重新处理或自动同步。

Commit 表示接受最终不可变 Item/Policy；persist=true 成功入历史后才派发，写入失败不发送；persist=false 也能 commit 和同步，只保留操作期资源。committed 通知只带条目 ID；不持久条目无任何操作/租约持有后返回 not_found。

ObserveClipboard 与显式 SubmitContent 均走 captured → processing → commit；显式提交不能绕过插件。RequestID 幂等检查在处理前完成，重复请求不重复运行插件。接收先校验身份与准入，再走 received、commit、宿主应用；首版不转发。

处理器不能改写内容，因此没有自动写回、自动化深度或插件来源链。Action 的结果作为 origin=`action` 的新条目提交，来源条目不变，宿主之后经 BeginApplyHistory 写入系统剪贴板，见[Action 契约](clipboard-action-contract.md)。

## 6. 常驻同步插件 builtin.tailcat

builtin.tailcat 是随宿主编译的 builtin Go 同步插件，源码位于 `internal/plugins/tailcat/`，由宿主显式装配进程内实例；保留 ID `builtin.tailcat`，不与现有 `builtin.text` 合并，不提供或查找 `unic-plugin-tailcat` 可执行文件。它负责 tailcat 网络、session/wire 会话与传输。Engine 保留身份与信任审批、过滤、操作状态、历史、资源租约和防回环；插件不直接写系统剪贴板或 Engine 数据库。

SDK 的 SyncBackend 扩展接口规划如下（builtin 通过进程内接口调用，external 由 RPC 适配承载；均待按 0008 补齐身份与关联 DTO）：

```go
type SyncBackend interface {
    Plugin
    Bind(SyncHost) error // Init 后 Start 前
    ApplyState(context.Context, SyncState) error
    Send(context.Context, SyncRequest) error
    PairingAddress(context.Context) (string, error)
    Connect(context.Context, string) error
}
```

- SyncState 为该后端独立的带修订号快照：BackendID、全局同步授权、本后端策略、运行状态/限制，以及 P2P profile 必要的已批准设备和拒绝状态；旧修订不能覆盖新状态。撤销信任立即断开并终止相应发送，ApplyState 返回表示约束已经生效。挂起关闭会话，恢复重建；全局关闭立即取消内容工作，控制连接可保留。
- Engine commit 后通过专用后端 RPC 派发 SyncRequest，不依赖会丢失的通知队列。请求包含派发/操作/设备 ID 与只读内容句柄，Engine 管理排队、替换、所有权，保留每 (BackendID, TargetKind, TargetID) 一个在途加一个待发语义；提交和实际开始传输前都检查限制与授权。
- Send 返回目标终态；成功只是字节写完，不等于远端收到或应用。RPC 中断不得自动重复发送；传输失败不回滚历史，不可靠投递离线过期内容。
- SyncHost 通过反向 RPC 提供身份验证、信任查询、准入、流式接收、传输前授权和设备状态上报。后端不能自行批准设备，不能绕过指纹核对。配对票据/rendezvous 由 Engine 管理，PairingAddress 只在显式导出边界使用，Connect 不授信。
- 接收正文以流进入 Engine 私有资源，由 received 阶段处理和宿主 BeginApply/ReportApplyResult 应用；保留超限读完丢弃与隐私边界。设备网络协议按 0008 重定，不要求与旧 wire v2 互通。
- **Start 后 builtin 实例长期持有 tailcat 端点和设备长连接，Send 复用会话。** 不为每次同步创建实例或重新连接；断网、挂起恢复、实例故障后按契约恢复。关闭等待端点和后台任务退出，不能依赖子进程强杀。
- 源码位于 `internal/plugins/tailcat/`，由宿主显式创建独立 `builtin.tailcat` 实例，按新身份边界重构并复用适合的 internal/tcnet、internal/session；Engine 不再直接装配它们。设备身份由 Engine 新建；tailcat 传输密钥在实例私有目录首次初始化，按[装配契约](backend-migration-contract.md) 原子持久化与持锁；不导入旧密钥或地址；插件不打开 Engine 的 SQLite。信任以 Engine RPC 为准，不维护第二套授信逻辑。
- Options.Network/engine.Network 在 P5 同批移除，替换为 builtin 实例或外部程序的显式装配；Transport 注入只保留在后端内部。Engine/桌面以正常装配的 builtin 实例回归，外部 RPC 另用真实测试进程，见[装配契约](backend-migration-contract.md)。SDK 零装配为本机模式，桌面默认显式启用 builtin tailcat，不另保留绕过插件的网络生产路径。

独立后端的授权、连接事件、状态快照、撤销屏障与发送/接收 DTO 以[同步后端业务契约](sync-backend-contract.md) 为准。Unpair 的成功返回要求后端确认旧会话终止或进程已回收，不能只依赖发送一条不等待确认的通知。

多个后端装配、独立策略、目标隔离及去重按[0007](decisions/0007-multiple-sync-backends.md) 实现；这里的 SyncBackend 地址/连接接口仅描述 peer_sessions profile，object_store 不实现假设备连接，具体对象协议另定。所有启用后端启动成功才报告 Engine 启动成功；运行期单后端失败不拖垮其他后端，启动失败不能用处理器 continue 掩盖。

### RPC 协议

父进程创建专用 stdin/stdout 双向管道，不对外监听 TCP。控制载荷使用 UTF-8 JSON，内容载荷使用有界二进制帧；统一长度帧头包含协议版本、类型、请求 ID、流 ID、载荷长度，长度上限握手协商且受引擎硬上限限制。

握手交换协议/API、ID、版本、能力和本次会话 nonce，必须与安装描述一致。nonce 绑定父子会话，不代替来源校验。只传最小环境变量与插件私有目录，不继承无关凭据。启动前复核安装二进制摘要；stderr 默认不转发。

请求、响应、取消、Host 反向调用与内容流多路复用，不能因 Process/Send 阻塞控制读写。deadline、取消和稳定错误码有明确字段；大整数（序号/时间/字节计数）用十进制字符串避免 JSON 精度损失。Process 正文是显式授予输入，通知不含正文。

大文件/图片以短期句柄 + 分块二进制 + 窗口流控传输，不整体编码为 JSON/base64。句柄绑定会话与请求，取消/完成/断线即失效；不传本机文件路径。SyncHost 反向调用也须校验能力、会话与授权状态。

核心帧格式、握手、process patch 与流描述已在[RPC 核心协议](plugin-rpc-contract.md) 和机器可读 Schema/向量中定义；编解码器与子进程互操作测试待实现。SyncBackend/SyncHost 业务方法、传输授权票据、状态生效屏障与接收 DTO 已在[同步后端业务契约](sync-backend-contract.md) 定义，实现及互操作测试仍待完成。移动端不默认承诺可启动子进程，运行方式按平台约束另定；不因移动端限制自动退回进程内第三方执行。

## 7. 最小模板

可编译示例见 [`plugins/examples/emailtag`](../../plugins/examples/emailtag)（`go build ./plugins/examples/emailtag`），以下为其摘要；入口为 `plugin.Serve`，处理回调接收 `plugin.ProcessContext` 接口。Go 构建独立 main 包，不重新构建 unicd：

```go
package main

import (
    "context"
    "regexp"
    "github.com/uniclipboard/unic/plugin"
)

type emailtag struct{ tag string }
func main() { _ = plugin.Serve(&emailtag{}) } // SDK 服务 RPC 并管理退出
func (*emailtag) Spec() plugin.Spec {
    return plugin.Spec{ID: "example.emailtag", Version: "1.0.0",
        APIVersion: 1, Capabilities: []string{"processor"}}
}
func (p *emailtag) Init(_ context.Context, _ plugin.Host, cfg plugin.Config) error {
    for key := range cfg {
        if key != "tag" { return plugin.ErrInvalidConfig }
    }
    p.tag = "email"
    if v, ok := cfg["tag"]; ok {
        s, valid := v.(string)
        if !valid || s == "" { return plugin.ErrInvalidConfig }
        p.tag = s
    }
    return nil
}
func (*emailtag) Start(context.Context) error { return nil }
func (*emailtag) Close(context.Context) error { return nil }

var email = regexp.MustCompile(`[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}`)

func (p *emailtag) Process(ctx context.Context, c plugin.ProcessContext) error {
    if err := ctx.Err(); err != nil { return err }
    item := c.Item()
    if item.Type == "text" && email.MatchString(item.Text) {
        return c.AddTag(p.tag) // 只标注，内容原样入历史与同步
    }
    return nil
}
```

Lua 替代实现（同 ID 不能同时安装/注册）：

```lua
local tag = "email"
return {
  id = "example.emailtag", version = "1.0.0", api_version = 1,
  capabilities = { "processor" },
  init = function(host, config)
    for key, _ in pairs(config) do
      if key ~= "tag" then error("invalid_config") end
    end
    if config.tag ~= nil then
      if type(config.tag) ~= "string" or config.tag == "" then error("invalid_config") end
      tag = config.tag
    end
  end,
  start = function() end,
  process = function(ctx)
    if ctx.item.type == "text" and ctx.item.text:find("[%w._%+-]+@[%w.-]+%.%a%a+") then
      ctx:add_tag(tag)
    end
  end,
  close = function() end,
}
```

Action 示例（用户在界面选择“转大写”后才运行，结果成为新条目）：

```lua
return {
  id = "example.upper", version = "1.0.0", api_version = 1,
  capabilities = { "action" },
  actions = { { id = "upper", title = "转成大写", accepts = { "text" } } },
  init = function() end, start = function() end, close = function() end,
  run_action = function(ctx)
    ctx:set_text(ctx.item.text:upper())
  end,
}
```

Go 与 Lua 的正则方言不同，示例只展示接口对应关系；Go 版 Action 实现 `Run(ctx, *plugin.ActionContext)`，在其中调用一次 `SetText`。

## 8. 从 GitHub 仓库安装

目标 CLI（尚未实现）：

```sh
unic plugin install https://github.com/owner/repo
unic plugin install https://github.com/owner/repo --ref v1.2.0
unic plugin install https://github.com/owner/repo --build
unic plugin install https://github.com/owner/repo --ref <commit> --path plugins/emailtag --enable
unic plugin list
unic plugin enable example.emailtag
unic plugin disable example.emailtag
unic plugin uninstall example.emailtag
```

### 清单与产物

根或 --path 子目录含 spec.toml，每次安装一个插件，不递归安装依赖插件、不执行安装脚本。Lua 清单：

```toml
manifest_version = 1
id = "example.emailtag"
version = "1.0.0"
api_version = 1
kind = "lua"
entry = "main.lua"
capabilities = ["processor"]
events = ["clipboard.captured", "clipboard.received"]
[config]
tag = "email"
```

Go 使用独立 main 包；提供匹配 module 的 go.mod/go.sum，清单示例：

```toml
manifest_version = 1
id = "example.emailtag"
version = "1.0.0"
api_version = 1
kind = "go"
module = "example.com/emailtag"
package = "./cmd/emailtag"
capabilities = ["processor"]
events = ["clipboard.captured", "clipboard.received"]

[[artifacts]]
goos = "darwin"
goarch = "arm64"
release = "v1.0.0"
asset = "emailtag_darwin_arm64.tar.gz"
executable = "emailtag"
sha256 = "<发布时替换为 64 位十六进制摘要>"

[config]
tag = "email"
```

artifacts 可为空，同一 GOOS/GOARCH 最多一项，可声明最低系统版本与运行库要求；未满足不算匹配。Release/asset 指向同仓库发布资产，不猜文件名；摘要固定内容。清单是所选 commit 中的数据，不能用 Release 名称代替 commit 锁定。清单与启动握手 Spec 必须一致。

### 安装选择与 --build

1. 规范 GitHub HTTPS URL，允许末尾 .git。分支、tag、commit 用 --ref；子目录用 --path，不解析 tree/blob 网页 URL。默认解析默认分支 HEAD，所有 ref 锁到完整 commit；获取该 commit 清单。首版仅公共仓库，不处理子模块。
2. Lua 获取脚本并校验语法/API。Go 查找当前 GOOS/GOARCH 与系统要求匹配的预编译项，并探测 Release 资产是否存在。
3. 存在匹配预编译则下载，校验 SHA-256、归档边界、可执行格式/架构，无需 Go 工具链。摘要不符、包损坏、描述冲突直接失败，不用源码构建掩盖完整性错误。
4. 没有匹配项或确认资产不存在：默认失败并提示 --build。指定 --build 才允许源码兜底。即使指定 --build，仍优先使用有效预编译；网络、限流、认证错误不能误判为“无预编译”。
5. 源码兜底仅构建插件自身：在锁定源码上执行固定 `go build -mod=readonly -o <staging-output> <manifest-package>`。需要兼容本地 Go 工具链、依赖下载和可能的 cgo/系统库；不自动安装工具链，不重建 unicd，不执行仓库 shell/install/go generate 脚本、不加载 .so。
6. 下载与构建都在 staging 完成，全部校验后原子提交；失败保留当前程序、配置与锁。安装阶段不执行新下载的第三方程序，首次启动再握手校验。

预编译与源码产物进入 `$UNIC_HOME/plugins/installed/<id>/<commit>/bin/`；Lua 文件存同一版本目录。installed.lock 记录仓库、commit、子目录、插件版本/API、来源 prebuilt/source、OS/架构、内容与二进制摘要、Release/asset 或构建工具链/依赖信息。commit 锁代码和清单，摘要锁资产；不宣称独立证明预编译是该源码构建而来。

限制下载/解包大小与数量，拒绝绝对路径、越界路径、设备文件及逃逸符号链接；--path、entry、package、executable 均限定在相应根目录内。不同仓库的相同 ID 不能静默替换；同 commit 安装幂等，升级使用显式 --replace。插件 ID/目录名严格校验。

### 启用、升级与卸载

首次安装默认不启用；--enable 等价显式 enable，采用清单配置默认值并追加到阶段顺序末尾，不覆盖用户已有配置。启用列表变更按第 3 节在重启时生效。安装器不覆盖正在运行的版本，重启切换失败可回退旧版本；Go 无需构建宿主。

list 显示 installed/ready、source、configured_enabled、running_version、restart_required，区分磁盘与实际运行状态。disable/uninstall 先标记待停用，重启时取消并回收进程，之后删除无引用文件，不碰历史。内置 builtin.tailcat 随宿主编译和升级，不经外部安装器安装/卸载，不允许第三方覆盖保留 ID；可禁用，其关联记录保留，全部后端禁用后进入本机模式。

桌面管理器负责 GitHub、文件、锁与配置；Engine 只接收已校验程序描述/脚本。锁文件不含配对或剪贴板秘密。交付必须包含资产探测、--build 兜底、原子安装/回滚、进程启动与回收，不能以下载成功代替运行成功。

## 9. 实施与验收

1. plugin SDK、描述/配置/顺序、进程生命周期与接口模板落地；补齐 RPC 帧、patch、流式 DTO 的机器可读 schema 与互操作测试。
2. 握手不匹配、帧长度、并发流控、取消/超时、迟到响应、崩溃/重启、孤儿进程清理与资源回收测试。
3. ObserveClipboard、SubmitContent、接收经统一流水线，幂等、防回环与失败回滚不变；历史 commit 写失败不发出，persist=false 仍可同步。
4. tailcat builtin 装配、统一设备与传输身份独立初始化、信任撤销竞争与状态恢复；双节点文本/HTML/PNG/文件、长连接复用、挂起重连、故障不重发回归。
5. Lua 沙箱、热替换、两语言只读/可变能力与错误策略测试。
6. GitHub 默认分支/tag/commit、子目录、资产平台匹配/缺失、网络/限流分类、摘要失败禁止兜底、--build 只构建插件、安装回滚、启用/卸载测试；使用本地假服务。
7. go test -race ./... 与桌面构建通过；日志/事件/错误不含正文、地址或票据。文档通过不表示功能已实现。
