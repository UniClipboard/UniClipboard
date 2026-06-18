# UniClipboard 领域术语表（CONTEXT.md）

本文件记录 UniClipboard 跨 crate 复用、且对领域专家有意义的统一语言（Ubiquitous
Language）。只收录本项目语境特有的概念，不收通用编程概念。随设计讨论惰性增长。

> 约定：术语名用英文（与代码标识符一致），定义用中文且尽量一句话——说清它**是
> 什么**，而非它**做什么**。

## Language — 核心同步域

**Space**：
用户拥有的一组互信设备构成的「本地密钥信任组」，由 `SpaceId` 标识、创建时强制
设定 passphrase。它是信任边界，不是云账户——无登录、无注册、无中心认证。
_Avoid_: account、workspace、room、team

**ActiveSpace**：
「该 Space 已在当前进程解锁」的不透明句柄，领域层只看得到内部的 `SpaceId`，
真正的密钥物料由 adapter 侧会话存储以 `SpaceId` 为键维护。拿到它即解锁担保，
领域代码不应直接构造。
_Avoid_: unlocked key、session、handle

**MasterKey**：
Space 的 32 字节对称根密钥，直接加密所有本地历史与传输负载；由 passphrase 经
KEK 解包得到，`Debug` 输出 `[REDACTED]`、drop 时内存清零。已物理下沉到
`uc-infra`，但仍是跨层领域概念。
_Avoid_: password、secret、AES key

**Passphrase**：
用户设定的解锁口令，仅在 unlock / initialize 流程内用于派生 KEK，不长期持有。
是「人记得住的输入」，区别于由它派生出的 `MasterKey`（机器持有的根密钥）。
_Avoid_: pin、key、token

**DeviceId**：
系统中一台设备的稳定身份值对象（`Copy`、≤64 字节、超限即拒绝而非截断）。
是机器可比较的标识，区别于用户可读的 `device_name`。
_Avoid_: device name、machine id、peer id

**SpaceMember**：
被接纳进本地 Space 的一台设备记录（device_id、device_name、身份指纹、加入时间、
同步偏好）。撤销建模为「直接从仓库移除」而非状态位翻转，故一条记录存在即代表
活跃成员。
_Avoid_: peer、trusted peer、user

**ClipboardEvent**：
一次剪贴板捕获动作的领域事件（`event_id`、捕获时刻、源设备 `DeviceId`、
`snapshot_hash`）。它记录的是「在某台设备上发生了一次复制」，是同步与去重的
事实单元。
_Avoid_: change、update、message

**ClipboardEntry**：
落到本地历史、对用户可见的一条剪贴板记录（一个 `entry_id` 关联一个 `event_id`）。
是历史列表与 Quick Panel 展示的单位，区别于触发它的 `ClipboardEvent`。
_Avoid_: item、clip、record

**SystemClipboardSnapshot**：
从系统剪贴板一次性读到的原始多格式快照（一个时刻、多条 representation）。是
捕获管线的入站原料，尚未做内联 / blob 落盘决策。
_Avoid_: clipboard data、payload

**Representation**：
一条 entry 负载的某一种格式表示（如 `public.utf8-plain-text`、`image/png`），各自
持有 `RepresentationId`、逻辑大小与负载来源（inline / blob / 本机文件路径）。一条
entry 通常有多条 representation。
_Avoid_: format、payload、blob

**EntryDeliveryRecord**：
发送侧为「某条 entry 投递到某台对端设备」维护的结果记录，状态为 `Delivered`
（对端 ack）/ `Duplicate`（对端已另有同内容）/ `Failed`（带失败分类）。是发送方的
本地投递视图，区别于接收侧的 **Tracked inbound file transfer**。
_Avoid_: ack、receipt、transfer

**delivery_tracked**：
`ClipboardEntry` 上的布尔标志，区分「投递追踪机制启用后新建的 entry」（`true`，
缺投递行即代表尚未尝试）与「机制启用前的历史 entry」（`false`，投递信息未知，
视图不应把它合成为 `Pending`）。
_Avoid_: synced、has delivery

**Transient sync semantics**：
本项目的同步契约——内容仅在设备在线时尽力投递，失败即报告，**不排队、不重发、
不追求最终一致**。离线是预期状态而非错误；自动恢复属协作工具语义，与「多设备
服务一个人」定位冲突。
_Avoid_: eventual consistency、message queue、store-and-forward

### 关系

- 一台设备以 `DeviceId` 立身，被接纳进 **Space** 后成为一条 **SpaceMember**
- 一次复制产生一个 **ClipboardEvent**，落地为一条对用户可见的 **ClipboardEntry**；
  原始字节来自该 event 的 **SystemClipboardSnapshot**，拆成多条 **Representation**
- **MasterKey** 由 **Passphrase** 经 KEK 解包，是 **Space** 内一切密文的根密钥；
  解锁后以 **ActiveSpace** 句柄表达
- 发送侧每条 entry 对每台对端设备各记一条 **EntryDeliveryRecord**；接收侧对应的
  是 **Receiver-side file transfer projection**（两侧各自为本地投影，不互为真相源）
- 上述投递全部遵循 **Transient sync semantics**——失败不重试，由用户手动重发

## Language — 文件传输（接收侧）

**Tracked inbound file transfer**：
接收设备本地为「一个正在/已经收下的文件」维护的一条投影记录（id、来源设备、
缓存路径、状态、时间戳）。
_Avoid_: download、file record

**Receiver-side file transfer projection**：
接收侧把传输生命周期落到本地的投影表，与 domain event 总线解耦——它是接收方的
本地上下文，不是同步的真相源。
_Avoid_: transfer DB、file table

**In-flight transfer**：
尚未终结的传输，即状态为 `Pending`（已收元数据、等数据）或 `Transferring`
（已收首块、传输中）。`Completed` / `Failed` / `Cancelled` 均 **不** 属于 in-flight。
_Avoid_: active transfer、ongoing

**Entry transfer summary**：
把一个剪贴板 entry 名下所有 transfer 的状态聚合成一个对外状态的视图，聚合优先级
为 `Failed > Transferring > Pending > Cancelled > Completed`。
_Avoid_: transfer status、entry status

**Timeout sweep**：
周期任务，找出超过时限的 in-flight transfer 并逐条终结（transferring 行需先拆掉
iroh-blobs 抓取与 QUIC 连接，再标记失败）。
_Avoid_: cleanup job、GC

**Startup reconcile**：
进程启动时的一次性重整，把「上次运行残留的孤儿 in-flight transfer」批量标记失败
并清理缓存。
_Avoid_: recovery、startup cleanup

## Relationships — 关系

- 一个剪贴板 **Entry** 拥有零或多个 **Tracked inbound file transfer**
- 多个 transfer 的状态聚合成该 Entry 的一个 **Entry transfer summary**
- **Timeout sweep** 与 **Startup reconcile** 都把 **In-flight transfer** 终结为
  `Failed`，区别只是触发时机（周期 vs 启动）与粒度（逐行 vs 批量）

## Example dialogue — 示例对话

> **Dev**：mobile `PUT /file` 进来时，是不是马上就有真实 entry_id？
> **领域专家**：没有。先用占位 id seed 一条 **Tracked inbound file transfer**，
> 等 SyncDoc apply 阶段生成真实 entry 后再 relink 过去。所以这条投影行的
> entry_id 是会被改写的——这正是 `RecordReceiverTransferPort` 要 relink 的原因。

> **Dev**：那「接收进度百分比」算不算领域概念？
> **领域专家**：目前不算。我们只跟 **In-flight transfer** 的状态枚举，不跟逐块
> 进度——历史上预留过逐块投影方法，但从未接线，已在 ADR-009 删除。

## Flagged ambiguities — 已澄清的歧义

- `mark_completed` / `mark_failed` 曾在两个无关 trait 上同名（文件传输投影 vs
  `RepresentationCachePort` 打在 `rep_id` 上）——已确认是两个不同概念，文件传输投影
  侧的 `mark_completed` 实为死代码，ADR-009 已删。
- 「receiver-side 分块进度投影」一族方法（`mark_transferring` / `refresh_activity`
  / `backfill_announce_metadata` 等）曾被误认为是活功能——已澄清为未接线的预留面，
  ADR-009 删除；将来若需进度功能须按 `uc-core/AGENTS.md §2.3` 另立新意图端口。
