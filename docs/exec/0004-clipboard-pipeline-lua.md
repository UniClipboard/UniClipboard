# 0004 执行计划：剪贴板流水线、历史与 Go / Lua 插件

- 状态：契约定义阶段；标注“定义/方案”的勾选仅表示该文档交付及文档检查完成。实现任务须代码、契约与对应验证均完成才可勾选，不能以文档完成代替运行验收。
- 日期：2026-11-07。
- 依据：[0004](../design/decisions/0004-clipboard-pipeline-lua.md)、[插件契约](../design/plugin-contract.md)、[Engine 契约](../design/engine-contract.md)、[0002 配置](../design/decisions/0002-config-file.md)、[0005 布局](../design/decisions/0005-monorepo-layout.md)。
- 范围：本计划覆盖 0004 与插件契约的桌面首版，包括 GitHub 安装及默认 builtin `builtin.tailcat`；不包含移动端子进程支持。

## 目标与边界

将本机观察、显式提交和远端接收统一为处理流水线：`Capture/Receive → Processing → Commit → Sync/Apply → Use → End`。只有 `captured/received` 可修改 Item/Policy，其他事件只读；commit 后同步通过专用后端接口派发，不能依赖可能丢失的通知。

历史、流水线调度、插件执行、信任、操作状态与资源策略的业务实现放在 `engine/`。`internal/store` 等仅承担基础设施；`plugin/` 提供不依赖 Engine 的公开 SDK、DTO 与 RPC；`internal/plugins/builtintext/` 提供随宿主编译的 builtin `builtin.tailcat`（与 `builtin.text` 独立）；外部插件通过独立程序/RPC 接入。桌面宿主负责剪贴板、TOML、文件监听、安装管理和 API 适配，核心不得反向依赖桌面。

设备网络协议按 0008 重定，不保留旧 wire v2 互通要求；不同步标签/过期，不增加离线可靠投递或远端应用确认；不做全文索引、收藏、集合、Quick Panel、Lua 网络/存储、WebDAV 等后端的具体对象协议与实现、任意脚本路由。过期只删除历史，不清空系统剪贴板。

## 当前基线与必须先解决的问题

历史部分的设计已在[本机历史契约](../design/history-contract.md) 确定（API、分页、租约、Received 映射、历史应用、容量/到期与事件）；下面第 1、2 项和第 5 项的历史配置部分已完成契约定义，第 4 项的提交结果/幂等已在[流水线提交契约](../design/pipeline-contract.md) 确定；第 3 项原自动写回已于 2026-11-08 取消，改由[Action 契约](../design/clipboard-action-contract.md) 取代并已确定；[RPC 核心协议](../design/plugin-rpc-contract.md) 的帧、握手、patch、流描述与互操作向量也已定义；[后端业务 DTO 与授权/撤销/接收流程](../design/sync-backend-contract.md) 已定义；[身份初始化/外部后端装配方案](../design/backend-migration-contract.md) 已确定；0008 新增的统一身份协议、关联 DTO/Schema 和实例标识仍待补齐，[Lua 运行时契约](../design/lua-runtime-contract.md) 及四项上游隔离探针已完成，[SDK 类型与错误定义](../design/sdk-extension-contract.md) 已补齐；自动化基线通过，增强 VM/沙箱验收、真实双设备基线及新功能实现验证仍未完成。

当前 `engine/clipboard.go` 管理观察和应用 token，`engine/send.go` 直接提交发送，`engine/receive.go` 管理实例内接收列表；`engine/engine.go` 直接装配 session/tcnet。`internal/store/store.go` 保存身份与信任，尚无历史表；公开 `Options.Network` 是进程内接口，不能跨 RPC 原样迁移。当前接收资源会在启动/关闭时清理，不能直接作为持久历史文件。

实现前必须在契约中确定下列细节；本计划不将候选 API 当成已经实现的接口：

1. 历史 ID 跨重启稳定；区别于实例内 ContentID、ReceiptID、LeaseID。明确分页顺序、游标一致性、过滤、读取租约、更新/删除的并发语义，以及删除后活动读取/应用/发送的资源保留规则。
2. 历史与 Received 的映射：接收列表仍是待应用视图，dismiss 不等于删除历史；历史到期不新增待应用可靠投递保证。明确历史条目如何进入已有 BeginApply/ReportApplyResult 协议。
3. ~~本机插件改写后的宿主写回协议~~（2026-11-08 取消：自动写回会让用户困惑）。改为用户主动触发的 Action：插件静态声明，`RunAction` 生成 origin=action 的新历史条目，宿主经 BeginApplyHistory 写入；自动阶段只标注。
4. 无目标、同步关闭或无后端时仍可本机 commit。补齐原发送 API 返回值与新增处理结果之间的关系，避免为保留原 disposition 而跳过历史；基线导入、回声和重复请求仍不得重复处理。
5. 历史容量、blob 阈值、清理周期、插件资源限制的默认值及配置归属；收藏不在首版，因此淘汰只按最旧且满足资源约束的条目进行。明确历史与临时内容共用配额，避免重复计数。
6. RPC 帧格式、patch 与流式 DTO；插件未知配置键的校验机制。确定 `Options.Network` 弃用/移除和builtin 默认后端与外部程序的装配方式，同步公开契约与测试。
7. 验证 gopher-lua 是否能直接支持指令预算、utf8 工具与所需取消语义；若需封装/扩展则先形成可验证方案，不能用墙钟超时冒充指令上限。

上述问题在 P0 形成确定契约。若改变 0004 的既定边界，先修订相关设计决策，不在实现中静默更改。

## 阶段与依赖

主线：`P0 → P1 → P2 → P3 → P4 → P5 → P6 → P7 → P8`。此顺序让存储、处理语义、RPC、网络迁移分别可评审。P1 暂时使用旧发送实现作为内部过渡适配；P5 完成后删除生产路径，不能作为最终交付的进程内备用后端。

[0007](../design/decisions/0007-multiple-sync-backends.md) 补充范围：首版要支持多后端装配、独立策略与结果/资源隔离，实际只交付 tailcat，使用多个假后端验收；WebDAV 实现前另补对象协议。

### P0 · 固化契约与回归基线

- [x] 确定历史契约；文档检查完成，实现与契约测试仍待 P2/P3 验证。
- [x] 确定提交结果与幂等契约（第 4 项）：新增 CommitContent/GetSubmission，保留旧门面，定义失败缓存、观察处置与无网络本机 commit；实现与测试待完成。
- [x] 确定 action 契约（取代自动写回，第 3 项）：Spec 静态声明、ListActions/RunAction、结果为新历史条目并经 BeginApplyHistory 应用、失败不留条目、同一幂等命名空间；实现与测试待完成。
- [x] 定义 RPC 核心帧、握手、生命周期/processor/observer/action 方法、patch 与内容流，提供机器可读 Schema 和互操作向量；后端业务 DTO 和运行时互操作测试仍未完成。
- [x] 定义 SyncBackend/SyncHost 业务 DTO：完整状态流、撤销屏障、配对与会话授权、传输前授权、接收预留/内容流与完成结果；实现与真实子进程互操作待完成。
- [x] 核实 gopher-lua v1.1.2 的取消、原生调用、基础库与 utf8 行为（四项隔离探针通过），确定增强 VM/白名单/预算方案；不是增强 VM 或完整沙箱验证。
- [x] 补齐上文其余契约问题，更新 Engine/插件契约与实现进度登记；[SDK 类型与错误契约](../design/sdk-extension-contract.md) 定义历史/提交/Action DTO、插件/Lua 装配、可选值、严格 Config 校验及稳定错误映射，不暴露内部类型；代码与契约测试仍待各阶段实现。
- [x] 明确捕获/接收/显式提交的处理、commit、同步和应用状态转换，以及每一步的资源取得与释放；流水线契约 §7 定义提交屏障、Received 槽位与去重发布、引用/配额账本及崩溃恢复边界，实现与故障注入仍待完成。
- [x] 记录现有测试和自动化双节点基线：107 个测试非缓存竞态运行通过，两个桌面入口构建通过；文本、HTML、PNG、文件、过滤、幂等、防回声、挂起恢复、撤销、超限丢弃的用例映射及覆盖限制见[基线记录](baselines/0004-2026-11-07.md)。
- [ ] 完成真实双设备 tailcat/系统剪贴板基线；自动化使用 loopback/fakeBoard，不能替代真实隧道和原生剪贴板实测，同机两个 UNIC_HOME 仍共享系统剪贴板。
- [x] 形成持久资源目录、全新身份初始化/原子持久化、运行锁与损坏拒绝方案；原旧 tailcat 密钥/地址迁移方案取消，不提供兼容导入；实现及故障注入仍待 P5。
- [x] 确定 Options.Network/engine.Network 在 P5 移除、PluginProcesses/Plugins 显式装配、SDK 零装配本机模式、桌面 builtin 默认后端装配及外部插件真实子进程测试入口；实现与发行验证仍待完成。

落点：`docs/design/engine-contract.md`、`docs/design/plugin-contract.md`、现有 `engine/*_test.go` 与桌面 host 测试。

退出条件：新增 API 和协议可以据此编写测试；已知基线失败有记录，不能将其误报为此次回归或默认忽略。

### P1 · 统一流水线与提交边界

- [x] 首批内部处理核心：快照/暂存上下文、文本/策略 patch、逐处理器串行与回滚、禁发锁、深度及协作取消（其中文本 patch `SetText`、自动化深度与 plugin 来源已按 2026-11-08 取消自动写回的决定，正在代码中移除，随后的提交完成）；captured 接入显式提交/观察，received 接入完整接收，旧网络适配只读取最终内容。新增处理用例及既有全量竞态测试通过；尚不是 P1 全部出口。
- [x] legacy 提交独立结果缓存与异步观察接纳：接纳前查重/容量校验，成功和失败缓存，调用者取消不取消工作，processing 不淘汰；最新观察独立保留结果，旧处理不覆盖新基线。公开提交 API 和持久 commit 仍待完成。
- [ ] 在 `engine/` 实现 Item、Policy、处理上下文、阶段链与 commit 结果；初始处理器为空，以旧网络实现适配专用派发边界。
  - 进度：已加公开 `CommitContent`/`GetSubmission`/`SubmissionResult`（Phase、ItemID、SyncDisposition、OperationID、SyncError）；无目标/同步关闭/策略不发送时仍 committed 而非错误，旧 `SubmitContent`/观察由同一记录映射回原错误码。`HistoryID`、`BackendResults`、`BackendIDs` 已随 P2/0007 加入；`no_backend` 尚无来源。因此该项不勾选。
- [ ] ObserveClipboard、SubmitContent、接收入口统一进入流水线。（现状：观察与显式提交共用 submission 记录，请求在处理前查重，并发合并；接收入口已共用处理核心，但没有 submission 记录，也没有 `recv:` 请求键，等跨后端去重/Received 槽位预留（P2 去重表、提交屏障）一起做，避免内部接收占用宿主提交容量。）RequestID 在处理前查重，覆盖同一请求并发到达，重复请求不重复执行插件或写历史。
- [ ] 区分历史内容与网络过滤后的发送视图；（现状：发送视图 `buildSendSet` 与提交内容已分开，过滤后内容不被改动，提交时与传输前均复核，已有测试；历史库本身属 P2，故未勾选。）保留提交时和实际传输前两次授权、配置及大小检查。
- [ ] 将后端派发与通知分离；保留每 (BackendID, TargetKind, TargetID) 一个在途加一个待发、替换、离线不排队、部分成功语义。
  - 进度（2026-11-08）：内部多后端抽象已落地（见 [0007 实现登记](../design/decisions/0007-multiple-sync-backends.md)）：`syncBackend`/`backendHost`、tailcat 适配器、各后端独立视图/策略/阈值、`BackendResults`、聚合 Operation 与目标三元组、`BackendIDs` 选择，并用多个假后端测试。策略已迁到 `Settings.BackendPolicies`（热更新、校验、桌面 `[plugins.instances."builtin.tailcat".sync]`，旧 `sync.text/images/files` 已删除不兼容）。仍未做：每 (BackendID, TargetKind, TargetID) 的“一个在途加一个待发”目前只由 tailcat 的 session 层按设备保证，通用层尚无；全局关闭时对在途内容传输的统一取消也仍依赖 tailcat 现有实现，故不勾选。
- [ ] 处理调用采用快照/暂存/校验后提交；（现状：快照/暂存/回滚、锁外执行、Stop/挂起后丢弃迟到结果已有测试；统一提交协调器与提交屏障尚未实现。）不持 Engine 业务锁执行插件。取消或代次失效后禁止迟到提交。

退出条件：无插件的网络行为与原来一致；新增本机 commit 路径可独立测试，事件丢失或慢观察者不影响派发。

首批实现说明：处理核心与适配器在 `engine/pipeline.go`、`send.go`、`receive.go`；处理器注册仅为包内适配入口，生产默认空列表，不提供进程内第三方执行 API。并发同键 legacy 提交合并，成功/失败由独立有界 submission 缓存保留，不持有正文，并与 Operation 独立驱逐。观察只在接纳阶段持 obsMu，处理/等待在锁外运行；新复制和新 epoch 可在旧处理期间接纳，旧完成结果不回退观察位置或覆盖新基线。同步关闭/无目标仍经过内部处理，但没有历史持久化和可查询的本机 commit；Persist/标签/期限目前只在内部暂存。公开 CommitContent/GetSubmission、提交事件、原子 commit/持久引用和多后端派发仍待后续步骤。接收发布前复查运行状态、当前同步策略和信任；处理期间策略发生变化时保守丢弃本次接收，不重跑处理器。

### P2 · 持久历史与资源所有权

- [x] 在 `internal/store` 增加版本化 SQLite 迁移和底层读写能力；在 `engine/` 实现历史业务及公开查询、读取、更新、删除 API。
  - 进度：`internal/store` 已完成（`history.go`）：`PRAGMA user_version` 版本化迁移（拒绝更新版本的库）、`clip_items`/`clip_item_parts`/`clip_tags`/`clip_item_sync`/`history_state` 表、AUTOINCREMENT 提交序号（不复用）、插入/读取/keyset 分页（同一读事务返回修订号）/CAS 更新/删除/到期删除/最旧淘汰候选/blob 引用查询，均有测试。文件名并入 parts 表（`kind=file`），不单设 `clip_item_files`。`engine/` 已有首批（`history.go`、`history_types.go`）：`Options.History`、128 位随机 HistoryID、提交时写入（小于 256KiB 的表示内联，其余表示与引擎自有文件落 `DataDir/history/<blob>/`；宿主就地文件只存引用，经 `Options.ResolveFileRef` 重建，读取时校验 Size/Version）、`GetItem`/`ListHistory`（HMAC 游标，绑定实例与过滤器，首页固定上界）/`RetainHistoryContent`（blob 在内容释放前不被删除）、插入后按条数/字节淘汰最旧、启动清理孤儿 blob，并有测试。`UpdateItem`（CAS、标签规范化、相同值无新修订）/`DeleteItem`（持有中的内容读完才删 blob）、启动清理和 30 秒到期扫描、`history` 事件与 `State.HistoryRevision`、容量淘汰事件均已实现并有测试。仍未做：同步状态摘要（含重启后 pending→interrupted）、与 `MaxStoredBytes` 的共用配额、`stopping` 等生命周期细节的专项测试，故本项暂不勾选。`BeginApplyHistory` 已实现：共用唯一应用槽与宿主能力/观察位置校验，不要求联网、同步启用或信任，暂停同步不使票据失效，应用中删除记录不影响已取得的内容；`ActiveApply` 增加 `Kind`/`HistoryID`；有测试。
- [x] 建立 `clip_items/clip_item_parts/clip_item_files/clip_tags` 及必要索引；稳定 ID、时间与分页规则遵循 P0 契约。标签、过期和同步状态仅本机保存。
- [x] 小内容入库，大内容进入 Engine 专属持久目录。宿主就地保存的文件不复制：实现 `PersistentFileSource` 的来源只持久化不透明引用（桌面为路径）与 Size/Version，读取时校验；不能稳定寻址的来源和引擎自有文件才复制为 blob（2026-11-07 决定，见历史契约 §6）。
- [x] 明确 blob 落盘、数据库事务、commit 发布与异常恢复顺序。启动可清理孤儿临时文件；不得先发送再发现历史写入失败。
- [x] `persist=true` 入库失败不派发；`persist=false` 不入历史，仍可同步，操作/租约释放后可回收。（现状：提交与接收均已接入，入库失败则 commit 失败且不派发；接收在发布前若策略/信任变化则回滚已写记录；有测试。）
- [x] 实现启动清理、定时过期与条数/字节容量淘汰。删除条目与实际释放 blob 分开，活动租约/读取/发送/应用持有的内容不得提前删除。
- [x] 将历史与临时接收目录隔离，修正 New/Close 清理流程；升级不丢身份/信任，不触碰用户文件或整目录删除自定义 inbox。

退出条件：重启可查可读历史；磁盘满、事务失败、提交中断恢复、共享内容引用、过期与操作竞争均有测试；无目标和同步关闭不妨碍历史入库。

- [x] 接收侧实现跨后端 TransferID/认证来源/ViewHash 的并发占位、有界持久去重（默认 7 天/100000 条）及失败/重启清理，persist=false 也必须记成功去重记录。（`engine/receive.go` 的 `admitReceive` 是后端无关的准入入口，tailcat 经 `receiver` 适配；去重键 `(认证来源, TransferID, ViewHash)`，`sync_dedup` 表持久、按条数/时间有界，占位只在实例内；persist=true 的成功记录与历史同一事务，persist=false 单独写入，发布前被回滚时一并删除；准入还要求 wire origin 等于认证来源、后端策略至少放行一个 part。有测试：双后端同一视图只提交一次、不同视图/来源独立、首份失败后可重试、重启保留、容量淘汰。Received 槽位在持久写入前预留（`reserveRecvSlot`），满表且无可淘汰项时不产生历史或去重记录。）

**P2 现状小结（2026-11-08）**：Engine 侧历史完整可用——持久化、分页查询、读取、更新、删除、容量淘汰、到期扫描、`history` 事件、`State.HistoryRevision`、`BeginApplyHistory`、各后端同步状态摘要（重启后 pending→interrupted）；复制的本机文件只存路径引用不拷贝；历史与 `MaxStoredBytes` 共用同一预算（`reserveStored`：导入/接收/历史提交都计入，空间不足时先淘汰无人使用的接收项，再淘汰最旧的、自己存有字节的历史项，纯引用条目不计也不被为此淘汰；提交的源内容在宿主释放租约前与历史副本会短暂同时存在，按“即将释放”计）；接收侧跨后端去重与槽位预留；历史上限可在 `config.toml` 配置；桌面 daemon 暴露 `/v1/history`。故障注入覆盖数据库失败、blob 目录不可写、磁盘满（`createNewFile` 接缝）、时钟前拨/回拨、超限、并发、Stop 后、崩溃残留。**P2 已完成**，已知取舍：被租约/读取持有的已淘汰 blob 在释放前仍占磁盘但不再计入预算；固定/搜索/清空历史未纳入首版。

### P3 · 事件、标注型处理与 Action

2026-11-08 产品决定：取消自动写回（用户复制 A 却粘贴出 B 会困惑）。自动阶段只标注（标签、过期、persist、sync），内容原样入历史与同步；改写正文是用户主动触发的 Action。

- [ ] 从引擎移除文本改写（`SetText`/文本 patch）、自动化深度、`plugin` 来源链及相关历史字段；处理器只保留 AddTag/RemoveTag/SetPersist/SetSync/SetExpire，历史来源改为 local_system/remote_device/explicit/action。
- [ ] 增加阶段事件，保留订阅修订号、初始快照与溢出恢复协议；事件只给公开标识与安全元数据，正文显式读取。
- [ ] 实现统一 ProcessContext（标注型）：标签限制、persist/sync/expire 暂存，Item.Text 只读；过期从 commit 起算。示例：含邮箱自动打 `email` 标签。
- [ ] 单次失败回滚全部 patch；`continue` 继续处理，`block_sync` 设置后续插件不能解除的禁发标记。普通 SetSync(false) 可被后续插件覆盖，但不能突破全局配置和信任。
- [ ] Engine 实现 Action 注册与运行：Spec 静态声明（ID/Title/Accepts）校验、`ListActions`、`RunAction`（来源校验、串行门与期限、恰好一次 SetText、新条目 origin=action 走显式提交、RequestID 幂等、失败不留条目），Go 与 Lua 适配器共用同一语义；契约测试见 [Action 契约 §6](../design/clipboard-action-contract.md)。
- [ ] 桌面宿主与 daemon：`GET /v1/actions`、`POST /v1/history/{id}/actions/{key...}` 运行 Action 后经 BeginApplyHistory 写入系统剪贴板，回声由 AppliedToken 抑制，不产生第二条历史；SwiftUI 在历史条目菜单展示。
- [ ] 明确 pasted 仅表示经宿主 UI 使用，不声称可观察所有系统粘贴；实现 deleted/expired/updated 等只读通知。

退出条件：用测试处理器验证标注、回滚、策略；用测试 Action 验证新条目、应用回声和失败不留条目；`/v1` 旧事件映射兼容且无正文，通知失败不影响已完成 commit。

### P4 · plugin SDK、RPC 与常驻进程管理

- [x] 新增 `plugin/` 的公开接口、稳定错误码、机器可读协议定义与可编译 Go 模板；Engine 使用代理，SDK 不依赖 engine。（`plugin/`、`plugins/examples/emailtag`、`plugins/README.md`）
- [x] 实现版本化长度帧、JSON 控制载荷、有界二进制流、请求/流 ID、多路复用、取消、deadline、反向 Host 调用与窗口流控；大整数用十进制字符串。（`plugin/rpc`，向量与真实子进程测试通过）
- [x] 实现 PluginProcessSpec、摘要复核、Spec/能力/API 握手和会话 nonce；最小环境启动，不对外监听 TCP，不转发自由文本错误或 stderr。（`internal/pluginproc`；摘要在启动前复核，未防文件替换竞态）
- [x] 实现 spawn/handshake/Init/Start/Close/wait 和失败逆序清理；同实例 Process/OnEvent 串行，跨目标 Send 可并发，控制读写不被业务回调堵住。
- [x] 处理默认 200ms 超时，先取消、1 秒宽限后终止；Stop 默认 5 秒退出窗口，回收进程树并 wait。处理 panic、EOF、迟到响应与父进程退出。（Unix 终止进程组；Windows 使用 job object，`KILL_ON_JOB_CLOSE`，已通过 `GOOS=windows go vet`，未在 Windows 上实跑）
- [x] 按 1/2/4 秒退避重启，60 秒最多 3 次；超限 unavailable。新会话重传最新状态，旧句柄失效，未决同步不自动重发。（`Engine.RestartPlugin` 显式恢复；新会话重传同步状态属 P5 同步后端，当前处理器无状态可传）
- [x] 统一 Order/Instances/OnError 校验：启用项恰好出现一次，拒绝未知事件/能力/配置/API，支持 0..MaxSyncBackends、TransportKind 区分 P2P/对象存储，每实例独立进程/binding；标准同步策略与全局授权由 Engine 校验。（以 `Options.Plugins` 列表顺序作为 Order；`Disabled`、`Events` 校验、`Resources.MaxSyncBackends`、TransportKind；sync_backend 描述通过校验后返回 unsupported，装配随 P5）

P4 剩余（均不阻塞 P5）：`device.*`/`sync.*` 通知（会话事件不带对端 ID，随 P5/设备契约一起做）；Windows job object 尚未在真机验证；摘要复核后改为执行私有临时副本，但与可执行文件同目录的旁路文件不在校验范围内（插件应经 `UNIC_PLUGIN_DIR` 找资源）。

退出条件：用真实测试子进程覆盖错误帧、超大帧、握手冲突、取消、死锁、崩溃、重启、资源回收与流控；Go 标注结果与 Action 结果和 P3 测试处理器语义一致。

### P5 · 统一设备与 builtin builtin.tailcat 后端

以[0008](../design/decisions/0008-device-identity-and-backend-bindings.md)及[设备契约](../design/device-contract.md)为准；下述新增项目为前置定义/实现任务，现有 RPC 通过不表示设备认证通过。

- [ ] 定义统一设备身份、持钥证明、连接/仓库绑定、防重放、配对票据和身份核对算法，版本化更新网络协议、RPC Schema/向量及互操作测试。
- [ ] 分离插件实现与配置实例标识，定义并实现 Device/DeviceBackendBinding、候选/配对与关联授权 DTO、CAS 和持久状态。
- [ ] 提供统一设备列表和各实例状态/控制；验收同设备多路径归并、新关联审批、无 tailcat 配对、单路径隔离与设备全路径撤销。

- [ ] 在 `internal/plugins/builtintext/` 新增 builtin tailcat 实现，保留独立 ID `builtin.tailcat`、配置和生命周期，由宿主显式创建实例并装配，只复用适合新身份协议的 tcnet/传输基础设施；重构 session/wire 与身份、信任和数据库的边界，不保留旧 wire v2 互通要求，插件不打开 Engine SQLite。
- [ ] 定义 builtin 与 external 共用的 SyncBackend/SyncHost 业务接口，并为外部后端实现 RPC 适配：按实例隔离且带修订的授权快照、Engine 可验证的身份材料、设备/关联授权检查、准入、传输前授权、流式内容与关联状态上报；只在显式导出返回秘密地址。
- [ ] Engine 管理统一设备身份、配对票据/操作、审批与关联授权，并保留过滤、队列和资源所有权；Unpair 撤销全部关联，单关联移除/暂停只影响该路径。配对码只发现目标，入站始终须出码方批准并核对身份；新路径绑定须验证同一身份和本次上下文，再单独确认。
- [ ] 持续持有 tailcat 端点和长连接，连续 Send 复用 builtin 实例和会话；断线、挂起、崩溃恢复按契约执行。
- [ ] 按装配契约在 New 初始化 Engine 统一身份，后端实例独立初始化传输凭据；覆盖原子持久化、中途失败、锁竞争、坏身份拒绝与重启恢复；不导入旧密钥、地址或配对记录。
- [ ] 删除 Engine 直接装配 session/tcnet 的生产路径，落实 Options.Network 的契约变更，测试注入点迁入后端。
- [ ] 提供 builtin tailcat 的显式装配和随宿主构建/分发，不查找独立 tailcat 程序；启用后端启动失败应报启动失败，无后端为显式本机模式，配对/发送返回 unsupported。sync.enabled=false 立即取消所有内容工作，控制连接与配对可保留；本机历史/Action 继续。

退出条件：双节点按新身份协议互通，设备关联授权及身份攻击用例通过；确认长连接复用、超限读完丢弃、撤销竞争和中断不重发；后端运行期故障不停止本机历史。

### P6 · Lua 适配与原子热替换

- [ ] 按 Lua 运行时契约纳入固定版本及最小增强补丁、许可证和来源记录，每插件独立状态，映射统一插件表、生命周期、ctx 和只读通知；Go/Lua 混合严格按显式顺序运行。
- [ ] 逐项开放基础函数/string/table/math 与有界 utf8 工具；禁止动态加载、环境/元表修改、文件、网络和普通 Host 再入提交，不能直接加载整库。
- [ ] 实现 P0 方案中的粘滞指令中止、墙钟取消、原生操作/编译/逻辑分配预算及调用栈/注册表和正文限制（增强实现尚未验证）；覆盖初始化、启动、处理、通知、关闭等可执行脚本边界，防止在 process 之外死循环。
- [ ] 桌面读取和监听脚本；新实例验证/Init/Start 成功后在回调边界切换，再关闭旧实例。失败保留旧实例，启动无有效实例时拒绝启动。
- [ ] 只允许同 ID/版本/API/能力的脚本与专属配置热替换；启用列表、顺序、订阅、Go 程序和后端切换报告需重启。
- [ ] 提供规范化、URL 标签、敏感内容禁发、自动过期/分类示例及可运行 Go/Lua 模板；示例不得改变默认无用户插件的行为。

退出条件：死循环、超时、抛错、沙箱逃逸尝试、热替换失败不拖垮 Engine；两种语言共享语义用例，错误/日志/事件不出现正文。

### P7 · 桌面配置、历史 CLI 与 GitHub 安装

- [ ] 桌面 TOML 映射插件设置，保留目录监听、防抖、坏配置保留上一有效值；模板解释 continue 风险、block_sync 和需重启项。
- [ ] 映射全局 sync.enabled 和每后端 .sync 类型/阈值，拒绝旧类型键，不做兼容映射；自动/手动策略与热加载测试覆盖。
- [ ] 增加 `unic history` 和 Engine 历史 API 的薄 HTTP 适配；查询/读取/删除不另建历史存储，桌面 API 与 SDK 同批更新，不保留旧响应兼容层。原生 macOS UI 不在此阶段实现。
- [ ] 实现 install/list/enable/disable/uninstall，公共 GitHub HTTPS URL、ref→完整 commit、子目录和清单校验；不递归安装依赖、不执行安装脚本。
- [ ] Go 优先平台匹配预编译，核对摘要/架构/归档边界；仅无匹配项或确认资产不存在时允许显式 --build。网络/限流/摘要失败不得触发源码兜底。
- [ ] 源码固定执行 `go build -mod=readonly` 仅构建插件，不重建宿主、不自动安装工具链；安装时不执行下载程序，首次启用启动时再握手。
- [ ] 实现 staging、下载/解包限额、防路径逃逸、installed.lock 和原子提交。相同 commit 幂等、跨仓库同 ID 不静默覆盖、升级需 --replace。
- [ ] 安装默认不启用，--enable 追加顺序而不覆盖用户设置；list 区分安装/配置/运行版本和 restart_required。禁用卸载待进程回收、引用释放后删除，不碰历史，不允许第三方占用 builtin.tailcat。

退出条件：本地假 GitHub 服务覆盖默认分支/tag/commit/子目录、缺资产、限流、损坏、构建失败和回滚；实际完成安装→启用→重启→处理/发送→停用→回收，不能以下载成功代替交付。

### P8 · 集成验收与交付登记

- [ ] 两个假后端验证全局关闭、独立类型/自动文件阈值、手动阈值豁免、部分失败、目标键隔离、并发/重启去重和统一资源配额。
- [ ] 核心、插件 SDK、后端与桌面测试通过，边界测试确认 engine 不引入 GUI、TOML 或监听依赖。
- [ ] 运行下方检查并记录结果；将 builtin tailcat 纳入宿主构建与 CI，验证干净安装可显式装配默认后端；外部测试程序另验 RPC 装配。
- [ ] 双节点实测文本/HTML/PNG/文件；插件改写、禁发、标签、过期；挂起恢复、断线、进程崩溃、重启历史和身份持久化。
- [ ] 隐私回归在正文/地址/票据中放置哨兵，检查日志、事件、公开错误及 stderr 处理；允许正文仅出现在显式内容读取和授权处理输入。
- [ ] 更新 README、桌面说明、示例、两份契约的实现登记和 0004 状态，区分已验证能力与后续平台工作。

## 验证命令与记录

每阶段先运行受影响包的测试；涉及竞态与资源生命周期时启用 `-race`。集成出口执行：

```sh
go test -race ./...
go build -o /tmp/unic-0004 ./app/desktop/cmd/unic
go build -o /tmp/unicd-0004 ./app/desktop/cmd/unicd
# builtin builtin.tailcat 随 unic/unicd 构建，无独立可执行文件
./bin/golangci-lint run ./...
./bin/golangci-lint fmt --diff
```

builtin 后端验收在 P5 接入后适用，外部 RPC 测试使用专门的测试程序；lint 使用仓库固定版本。双节点使用独立 UNIC_HOME 和插件私有目录，测试不访问真实第三方仓库。每个阶段完成时在本文件补充对应提交、运行命令、结果与未解决项；当前为契约与计划阶段，未执行实现验收。

进度记录：2026-11-08，产品决定取消自动写回（用户复制 A 却粘贴出 B 会困惑）：自动阶段只标注，改写内容改为用户主动触发的 Action，写回契约由 [Action 契约](../design/clipboard-action-contract.md) 取代，SDK/插件/RPC Schema 与向量、0004、0006 同步更新；引擎中已有的 `SetText`、自动化深度与 plugin 来源代码随后移除。

进度记录：2026-11-07，`1e00097` 提交历史/提交/插件契约与执行计划；随后补齐当时的自动写回契约（现已由 Action 契约取代）、RPC 核心协议、同步后端业务 DTO、0007 多后端、后端迁移、Lua 预算方案、流水线状态/资源账本及 SDK 类型/错误映射。文档差异、本地链接、帧/Schema 引用和固定向量检查通过；上游 Lua 四项隔离探针通过。现有实现 `go test -race -count=1 -json ./...` 非缓存运行通过（107 个测试，0 失败/测试跳过），桌面两个入口构建通过，详见[回归基线](baselines/0004-2026-11-07.md)。新功能实现、真实子进程互操作、真实双设备/原生剪贴板仍待验证；未运行 lint。

多后端更新：按 0007 同批调整 SDK Settings、提交 BackendResults/聚合 Operation、P2P RPC 快照与 mode 字段；对象存储 DTO 不冒充已实现。

## 发布与失败恢复

按阶段提交，避免将历史落盘、网络迁移和安装器一次合并。每个阶段的契约与实现同批更新；阶段出口未通过不宣告完成。

历史迁移采用可重复执行的增量方案，迁移前保留可恢复备份；不能通过降级程序直接删除新历史表或持久资源。设备和后端身份首次初始化须原子持久化，已有身份损坏停止启动，不静默重建；不提供旧密钥/地址迁移。安装升级失败保留旧锁、旧版本与用户配置，Lua 热替换失败保留旧实例。运行期后端崩溃依契约退避恢复，不回退到第二条生产网络路径，不重发结果不确定的同步。

进度记录：2026-11-08，进程内 Go 插件已落地：公开包 `plugin`（Spec/Processor/Actor/ProcessContext/ActionContext），`engine.Options.Plugins` 装配，校验 ID/能力/动作声明，Init+Start 在 `New`、逆序 Close 在收尾；内置插件 `builtin.text`（`internal/plugins/builtintext`）自动标注 email/url 并提供 upper/lower/trim/plain 动作；桌面新增 `[plugins.instances."builtin.text"] enabled`（需重启）、`GET /v1/actions`、`POST /v1/history/{id}/actions/{key...}`（运行后写入剪贴板）。未做：外部 Go/Lua 插件、`on_error`、Observer 能力、`PluginNotify` 在桌面的展示。

进度记录：2026-11-08，P4 主体落地（未提交）：`plugin/rpc`（帧、严格 JSON、Conn、流控、握手）、`plugin/serve.go`（插件侧 `Serve`）、`internal/pluginproc`（进程管理代理、退避重启、unavailable）、`engine.PluginInstance.Process/OnError`；Action 来源条目 origin 改为 `explicit`（此前为空，被插件进程校验拒绝）。命令：`go test -race -count=1 ./engine ./plugin/... ./internal/pluginproc` 全部通过；`go vet ./...` 通过；`go test -race ./...` 中 `internal/session` 的 `TestJoinByCodeStillNeedsApproval` 在全量运行时出现测试结束后写日志的竞态（该包未改动，单独 `-count=5` 通过），待另行处理。未运行 golangci-lint。

进度记录：2026-11-08，P4 收尾：Observer 能力与 `plugin.on_event`（SDK、RPC 代理、引擎有界异步派发，慢/崩溃观察者不阻塞引擎）、`Engine.RestartPlugin`、`PluginInstance.Disabled`、事件订阅校验、`Resources.MaxSyncBackends`（默认 8，上限 32）与 TransportKind 校验、`plugins/README.md`。

进度记录：2026-11-08，P4 收尾二：Observer 增加 `clipboard.pasted`（仅经宿主 UI 应用历史并成功回报时）与 `app.started`/`app.shutdown`（关闭时先派发并限时排空队列）；插件可执行文件摘要校验改为“边复制边哈希、执行私有 0700 副本”，消除校验与 exec 之间的替换竞态；Windows 进程树回收用 job object。`go vet ./...`、`go test -race -count=1 ./...` 通过。
