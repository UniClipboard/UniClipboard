# 0004 · 事件驱动的剪贴板处理流水线 + Go / Lua 插件

- 状态：草案（历史与流水线归属已确定；未实现）
- 日期：2026-11-07
- 关联：[0002 配置](0002-config-file.md)、[0003 Engine SDK](0003-engine-sdk.md)
- 参考：架构图（Capture → Processing → Commit → Sync → Use → End，事件总线，Lua 插件 API）

## 背景与目标

现在本机复制后，桌面宿主轮询到变化（`app/desktop/internal/host/clipboard.go` 的 `tick`）就交给 `Engine.ObserveClipboard`（`engine/clipboard.go`）直接同步，中间没有可介入的位置；接收远端内容同理（`applyLoop` → `Engine.BeginApply`）。

目标：用户复制后，内容先经过一条**处理流水线**，Go 或 Lua 插件可在其中修改内容、加标签、控制是否同步、设置过期时间；同步本身由默认的 **`sync.tailcat` Go 插件**执行，不再是写死的出口。

## 决策

### 1. 生命周期

```text
Capture/Receive → Processing → Commit → Sync → Use → End
   (捕获/接收)     (自动化)    (入历史)  (同步)  (使用)  (删除/过期)
```

| 阶段 | 事件 | 类型 | 说明 |
|---|---|---|---|
| Capture | `clipboard.captured`（本机）、`clipboard.received`（远端） | **可干预** | 同步串行调用插件，可改内容与策略 |
| Commit | `clipboard.committed` | 通知 | 最终内容与策略已确定；按 persist 决定是否写历史，此后只读 |
| Sync | `clipboard.sync.requested/started/completed/failed` | 通知 | 由 Engine 根据同步插件操作状态发出 |
| Receive | `clipboard.receive.started/completed/failed` | 通知 | 远端内容接收；completed 表示本机 commit 成功，不表示应用或远端 ACK |
| Use | `clipboard.pasted` | 通知 | 用户经 UI 使用；系统直接粘贴无法观察，不承诺 |
| End | `clipboard.deleted`、`clipboard.expired` | 通知 | 用户删除或到期 |
| 其他 | `clipboard.updated`、`device.connected/disconnected`、`app.started/shutdown` | 通知 | |

“可干预”只有 captured/received 两个，其余事件只读。这样同步与存储始终基于一份确定的最终结果。

### 2. 流水线与同步

- 流水线是 Engine 内的有序阶段链；每条内容在阶段间传递一个 `Item` 和一个 `Policy`：
  - `Policy{ persist bool, sync bool, expires_at *time, tags []string }`
  - 默认 `persist=true, sync=true`；全局授权与每后端策略在派发时分别检查，Policy 不能扩大它们。
- **同步是默认 Go 插件 `sync.tailcat`**：Engine 在 commit 后通过专用后端接口派发，普通 committed 通知不负责可靠调度；派发读取 `Policy.sync`；按[0007 多后端策略](0007-multiple-sync-backends.md)，全局 sync.enabled、各实例启用及每后端 text/images/files/自动文件阈值共同约束发送，set_sync(true) 不能越过它们。默认 tailcat 之外允许多个同步后端并存。
- `persist=false` 的内容不入历史，但仍可按 `sync` 同步。
- 远端内容走 received 阶段，插件同样可以改内容、标签、过期；接收方的 `set_sync` 只影响本机是否**再转发**，首版不转发（现有语义），所以对 received 实际只用 `persist/tags/expires`。

新增 `CommitContent/GetSubmission` 分开表达 commit 与发送结果；原 `SubmitContent` 保留兼容门面，观察结果增加提交信息。具体状态、失败缓存与生命周期见[流水线提交契约](../pipeline-contract.md)，待实现。

三类入口的状态转换、提交屏障、Received 槽位/去重记录原子发布和资源释放见[流水线契约 §7](../pipeline-contract.md#7-状态转换与资源交接)。Sync、Apply、历史 End 各自结束，不能用历史删除或一次网络成功代替整条内容的资源回收。

### 3. 防回环

插件改内容意味着需要把结果写回系统剪贴板，这会再次触发捕获。沿用 0003 的 apply token / 基线 hash 思路，并增加：

- 每个 `Item` 带 `origin`（`LocalSystem | RemoteDevice(id) | Plugin(name) | …`）和 `automation_depth`。
- 深度默认上限 3：输入深度达到上限时直接 commit，不再执行处理器；同一轮多个插件改写只增加一次深度。最终改写内容的来源记为 Plugin（最后一次成功改正文的插件），原始来源内部保留。只有可证明的自动化链继承深度，未知真实本地变化作为新的 LocalSystem/0。
- 已知写回回声通过 token/回读基线抑制，不再次 captured、commit 或同步，不为计算深度而重跑处理器。
- [自动写回契约](../clipboard-writeback-contract.md) 定义 BeginWriteback、显式提交 guard、单 pending 与共享应用槽、30 秒待办期限、失败不重试及资源释放。自动写回与历史使用在同步关闭时仍可工作。

### 4. 插件实现方式与 Lua 沙箱

- 插件允许使用 **Go 或 Lua** 实现，共享同一套事件、Item/Policy 能力、执行顺序与错误策略；Lua 是插件接口的脚本适配层。
- Go 插件采用独立常驻进程 + RPC，不编译进宿主、不使用 Go 动态库；通过 `Options.PluginProcesses` 传入安装描述，详见[插件契约](../plugin-contract.md)。
- Go 插件为受信任的本机程序，进程隔离不等于文件/网络权限沙箱。RPC 超时先取消，宽限后可终止插件进程；失败回滚 Item/Policy 暂存变更，外部副作用不能回滚。
- Go/Lua 混用时须有统一、确定的执行顺序，不能由注册时机隐式决定；使用显式 `plugins.order`，详见插件契约。

#### Lua 运行时


- 使用基于 `gopher-lua v1.1.2` 的纯 Go 增强 VM；上游没有公开指令计数 hook，SetContext 不能抢占原生 Go 函数。实现与验收见 [Lua 运行时契约](../lua-runtime-contract.md)，当前仅完成上游行为探针。
- 基础函数及 string/table/math 逐项白名单注册，原生循环/分配须有界；utf8 由宿主提供。不开放动态加载、环境/元表修改、文件、网络或进程能力，不能只从 OpenBase 删除三个函数。
- 所有脚本执行边界默认 200ms 与 1,000,000 条指令；编译、原生操作另有预算。调用栈/注册表、文本及累计逻辑分配分别限制，不承诺精确堆内存隔离，禁止使用会退出宿主进程的 SetMx。
- 每个插件独立的 Lua 状态；回调串行执行，不共享全局表。执行失败销毁该状态，后续调用按错误策略处理，等待显式重载；不重放失败内容。
- 插件错误：丢弃该插件本次对 Item/Policy 的改动，继续后面的插件，不阻断流水线。**注意**：这意味着“敏感信息检测”类插件出错时内容会照常同步。提供 `plugins.on_error = "continue" | "block_sync"`，默认 `continue`，写入配置模板注释说明风险。
- 日志与事件**不含剪贴板正文**，只含插件名、事件名、错误码；Lua 错误信息在记录前脱敏（可能夹带正文）。

### 5. 统一插件接口与模板

[插件契约 v0.1](../plugin-contract.md) 确定标识、能力、注册、Init/Start/Close 生命周期、配置、执行顺序、处理上下文、错误回滚和同步后端边界，并提供 Go/Lua 最小模板。接口为目标设计，尚未实现。

- 统一描述：ID、Version、APIVersion、Capabilities；Go 使用独立程序与 RPC 握手，Lua 返回插件表，不再使用全局 `events.on`。
- 普通插件分 processor（可变处理）与 observer（只读通知）；sync_backend 是仅 Go 可用的扩展能力，默认由 `sync.tailcat` 提供。
- 修改文本、标签、持久化、同步与过期通过 ProcessContext 方法暂存，回调成功后原子采纳。通知不带正文，不能修改已提交内容。
- Lua 首版不提供网络、本地存储、Quick Panel；Go 为受信任代码。同步后端具有专用网络与接收接口，不能将这些能力授予 Lua 或普通通知插件。

Go 独立进程的核心传输规则见[RPC 核心协议](../plugin-rpc-contract.md)，含机器可读帧表、Schema 和互操作向量；同步后端业务 DTO 已在[同步后端契约](../sync-backend-contract.md) 定义，运行时实现与验证仍待完成。

### 6. 插件加载与配置

- 完整配置形状与顺序见插件契约：所有启用实例在 `plugins.order` 中显式排列，不能以文件名或注册时机决定顺序。
- 支持 `unic plugin install https://github.com/owner/repo` 从仓库清单安装，版本锁定到 commit；Lua 下载加载，Go 优先匹配平台预编译，缺失时可 --build 构建插件自身，无需重建宿主，详细命令、清单、安装锁与回滚见[插件契约 §8](../plugin-contract.md#8-从-github-仓库安装)。
- Lua 脚本由宿主从 `$UNIC_HOME/plugins/*.lua` 或安装锁指定的已安装目录发现、读取并传给 Engine；监听与防抖留在宿主，Engine 不依赖文件系统监听。
- Lua 脚本与专属配置热替换失败保留上一有效实例，启动时错误则拒绝启动；启用列表、顺序、订阅与 Go 实现变更需重启。
- Go 插件由 Engine 启动常驻子进程，默认附带并启用 `sync.tailcat` 独立程序，Send 复用长连接。允许多个同步后端独立派发，也允许无后端的本机历史模式；当前仍只交付 tailcat，WebDAV 等实现另定，见[0007](0007-multiple-sync-backends.md)。
- 插件不能通过业务接口修改 `limits.*` 与设备配对状态；Go 代码必须遵守同样的契约。

默认程序的显式装配、插件持久目录/锁、旧 tailcat 密钥与地址保留，以及 Options.Network 同批移除，按[后端迁移契约](../backend-migration-contract.md) 执行。SDK 零装配进入本机模式，桌面仍显式默认启用随包 tailcat；密钥迁移和地址核对失败不生成新身份或回退旧网络。

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

标签和过期时间只在本机历史里。不改 wire，v2 设备互通不受影响；接收方由自己的插件决定自己的标签与过期。

## 历史与流水线归属：Engine

历史由 Engine 管理，历史与流水线的业务实现都放在根目录 `engine/` 包内，不采用先放根 `internal/`、以后再决定是否公开的方案。各平台通过同一套 Engine API 复用历史与处理语义。

- Engine 负责历史入库、查询、内容读取、更新、删除、过期与容量淘汰，以及流水线调度、插件执行和同步策略。底层 SQLite 等基础设施仍可通过内部边界复用，公开签名不泄漏内部类型。
- 公开 SDK 增补历史查询与操作能力（如 `ListHistory`、`GetItem`、`DeleteItem`），事件只携带对象标识，正文通过显式读取获取。具体签名、筛选分页、跨重启历史 ID、内容租约、容量/到期和历史应用语义已在[本机历史契约](../history-contract.md) 确定，实现与契约测试仍待完成。
- 历史是跨重启持久化的本机数据，区别于现有实例内的 `Received` 接收列表；不增加历史跨设备同步或重启后的待应用可靠投递保证。
- 宿主负责系统剪贴板读写、权限、UI 和通知。桌面 daemon 的历史 HTTP API 只适配 Engine，不另建历史存储或流水线；移动端绑定沿用同一业务契约。
- 0003 与公共契约中的“不承诺历史”仅描述既有 v0.1 接收列表；本决策扩展 Engine 的职责，实现进度仍以契约登记为准。

## 实施顺序

1. **流水线骨架**：在 `engine/` 内把现有观察、提交与接收应用流程重构为阶段链 + 同步插件派发，行为与现在完全一致（回归测试先行）。
2. **历史存储**：在 `engine/` 内实现历史管理，新增 `clip_items` 等表、commit 阶段写入、过期清理；`unic history` 查看。
3. **事件总线**：把现有 `Event` 扩为带阶段与 `origin/automation_depth` 的结构，旧 `/v1` 事件形状保留。
4. **插件接口与运行时**：统一 Go RPC/进程管理接口与 Lua 适配，提供两种语言的最小模板；Lua 沙箱、超时、插件表与 `ctx` API、热加载；错误与脱敏。
5. **同步插件与示例扩展**：将现有 tailcat 网络、会话和传输封装为 `sync.tailcat`，保留 Engine 的信任、策略与资源控制；URL 检测、敏感信息检测、自动过期、内容分类，用同一套 Lua/内部接口实现，作为示例与测试。

## 验收

多后端、全局开关、每后端类型/自动文件阈值与跨后端去重的补充验收以[0007](0007-multiple-sync-backends.md) 为准。

- `go build ...` 与 `go test -race ./...` 通过。
- 无用户插件、使用默认 `sync.tailcat` 时，同步行为、防回环、0002 配置语义与现在一致。
- 插件：改文本、加标签、`set_sync(false)` 不发送、`set_expire` 到期删除；Lua 插件超时/死循环/抛错不拖垮 Engine，不影响其他插件。
- 沙箱：Lua 插件拿不到 `os/io/require`，无法读写文件或联网。
- 防回环：插件改文本写回后不无限循环；深度上限生效。
- 日志、事件、错误中无剪贴板正文。

## 本阶段不做

- 插件网络/存储/权限模型、`register_action` 与 Quick Panel、WASM 插件、原生动态库加载、可视化规则编辑。
- 全文检索、收藏、集合、历史跨设备同步、标签/过期随内容同步（需改 wire）。
- WebDAV/Iroh/LAN/Relay/BLE 的具体实现；本次实现多后端装配/独立策略与契约测试，默认只迁移 tailcat，不实现任意脚本路由。
- 清空系统剪贴板的“到期清除”：先只删除历史记录，是否清空系统剪贴板另立决策。
