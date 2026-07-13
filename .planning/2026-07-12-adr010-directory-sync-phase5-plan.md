# ADR-010 目录同步 · 第五阶段规划（移动端降级、用户可见层与收尾）

Status: reviewed (2026-07-12，跨模型对抗审查 10 轮 / 33 条意见全部处置并并入，见 §11)
Issue: #875（剩余验收）· #1318（移动端泄漏，P0）· Map: #1315
ADR: `docs/architecture/adr-010-directory-sync-as-file-set-manifest.md`
前序规划：`.planning/2026-07-10-adr010-directory-sync-phase2-plan.md` ·
`.planning/2026-07-10-adr010-directory-sync-phase3-plan.md` ·
`.planning/2026-07-11-adr010-directory-sync-phase4-plan.md`
基线假设：**阶段 4 已合并**（PR #1323）。本文行号锚点相对该基线。

> 本阶段目标：修复移动端目录泄漏（ADR 承诺的降级行为）+ 补齐目录同步的
> 用户可见层（进度/取消/不可同步原因/旧端噪声抑制）+ 清 P2 技术债
> （reserver 竞态、设置界面、用户文档）。所有决策已与维护者逐项拍板
> （2026-07-12 grilling 会话），本文为定稿记录。

---

## 1. 现状盘点（阶段 4 合并后，阶段 5 的起点）

### 1.1 active-clipboard register：三方共用的单一真相源

- 单行 SQLite 表（`CHECK (id=1)`），四列 `snapshot_hash/entry_id/activated_at_ms/activated_by`
  （`crates/uc-infra/src/db/repositories/active_clipboard_register_repo.rs:17-29`）。
- 推进端口 `AdvanceActiveClipboardPort::advance(&state)`
  （`crates/uc-core/src/ports/clipboard/active_clipboard/register.rs:16`）。
- **推进位点两类**：
  - 本地写入（捕获 announce、restore）统一经 `LocalActiveRegisterAdvancer::advance_local`
    （`crates/uc-application/src/clipboard_write/active_register.rs:57`）；
    `announce_local_activation` 推进后立即 0xC3 广播
    （`crates/uc-application/src/facade/active_clipboard/mod.rs:287-306`）。
  - 入站 0xC3 收敛（`crates/uc-application/src/usecases/clipboard_sync/active_state/apply_inbound.rs:135`，
    OS 写成功才推进）。
- **消费方三个**：mobile GET（`latest_snapshot_adapter.rs:112-152`，只读当前指针，
  无 file-set 检测）；桌面 0xC3 收敛缺内容时按 hash 发起 pull
  （`apply_inbound.rs:402-412`）；peer 上线重同步 worker（`facade/active_clipboard/mod.rs:348`）。
- **结论（P0 方案取舍的依据）**：在推进侧排除目录 entry（原方案 A）会同时废掉
  本地 active 指针、0xC3 收敛、离线 peer pull 恢复三条桌面路径上的目录行为——
  为修手机误伤桌面。方案 A 出局。

### 1.2 mobile 读路径与通知通道

- mobile GET 锚定 register 当前值（`latest_snapshot_adapter.rs:118-126`），
  `MobileSyncSnapshotPorts`（`:82-89`）无 file-set 端口。目录 entry 经
  `classify_for_sync` 判为 File，以伪 File 泄漏（阶段 4 计划 §1.6）。
- **手机今天已消费单文件 entry**：`get_file.rs:151` 的
  `parse_first_uri_from_uri_list` 取 uri-list 首条 URI 经 staging 读回字节；
  平铺多文件 entry 只取第一个文件（既有边界，本阶段不动）。
- SSE 通道只发信号 `{content_id, server_time_ms}`，手机收到后无条件拉取，
  去重在拉取结果上做（`crates/uc-webserver/src/mobile_lan/routes/sse.rs` 头注释）。
  **推论**：目录激活时 SSE 照常发信号也自洽——手机拉到回退值（旧 contentId），
  去重后无操作。SSE 侧零改动。

### 1.3 pull-serve：第二条目录泄漏路径（本次勘探新发现）

- `ActiveClipboardPullServeUseCase::serve`
  （`crates/uc-application/src/usecases/clipboard_sync/active_state/serve_pull.rs:93`）
  直接走 `extract_file_paths_from_snapshot` + `publish_file_blob_refs` +
  `encode_snapshot_with_blob_refs_to_v3_bytes`（`:46-51`），**不经过
  `resolve_outbound_file_set` 的目录判定、不产 `UCDS` 清单**。
- 桌面 peer 对目录 entry 的 hash 发起 pull 会拿到无树结构的扁平帧——与 #1318
  同性质的泄漏，只是发生在桌面之间。阶段 4 只给 dispatch/resend 两条出站
  路径加了目录感知，pull-serve 是漏网的第三入口（平行逻辑漂移）。

### 1.4 出站进度：发送端零自观测，反向通道按 16 字节 id 关联

- iroh blob 是 pull-based，发送端对「被拉了多少字节」无事件可观测；字节进度
  全靠接收端沿反向 ALPN 回报（`crates/uc-core/src/file_transfer/outbound_progress.rs`
  头注释）。
- 反向帧定长 34 字节，`transfer_id` 为 16 字节 UUID，当前约定 = 发送端 entry_id
  （`crates/uc-infra/src/network/iroh/transfer_progress_wire.rs:10-24`）。
- **目录的 N 个成员共享同一 id**（`publish_file_blob_refs` 每成员都用目录
  entry_id，阶段 4 计划 §10），接收端逐成员回报会打在发送端同一行字节进度上
  互相覆盖——**百分比随成员切换反复归零重涨，是乱跳的假进度**（round 3 审查
  校正：整组「完成」终态只在最后一个成员完成时上报一次，终态本身正确；乱的
  是字节百分比）。§5.2 隐藏目录百分比的同时，补「仅最后成员完成才出现完成
  态」回归测试。
- 帧布局本身可容纳 per-member（16 字节装任意 UUID），但发送侧 id 分配 +
  reporter 按成员建模 + UI 聚合是真功能（阶段 4 计划 §10 已记录），本阶段不做。

### 1.5 接收侧进度：后端成员粒度已就位，前端聚合缺失

- 领域事件 `FileTransferEvent::{Started, Progress, Completed, Failed}` 带字节
  进度（`crates/uc-core/src/file_transfer/event.rs:79`），经 daemon WS 推前端。
- 阶段 4 已给目录成员建独立生命周期行 `{receiver_entry_id}:member:{index}`
  （`crates/uc-application/src/usecases/clipboard_sync/apply_inbound/materializer.rs:961-962`），
  `file_transfer` 表按 entry 聚合的投影（`EntryTransferSummary`）就位。
- **前端对不上**：`src/store/slices/fileTransferSlice.ts:103` 的
  `entryTransferMap[entryId] = transferId` 是 entry → 单 transferId 映射，
  目录卡片只显示最后 link 的那个成员的进度冒充整体
  （`HistoryCardTransferProgress.tsx` 渲染单 transfer）。
- ✅ 已核清（2026-07-12 审查）：目录成员 fetch **已逐成员** 发出进度事件
  （transfer_id = `X:member:N`），T3 后端侧只剩投影/关联工作，无需补事件发射。

### 1.6 Sync-ineligible：原因已持久化，缺投影

- 捕获侧整组放弃时逐行落 `EntryFileSetLineKind::Excluded { reason }`
  （`crates/uc-core/src/clipboard/file_set.rs:116,121`；reason 含
  `UnsupportedMember`/`SizeCapExceeded`/`IngestFailed` 等），已加密入库
  （`crates/uc-infra/src/db/repositories/entry_file_set_repo.rs:125`）。
- 整组同因放弃（traversal 在元凶处 break 后批量标记，
  `clipboard_capture/usecase.rs:1069-1090`），任取一行 reason 即代表整组；
  但 **元凶成员路径未单独记录**，不保证可指认（详见 §7 简/繁档取舍）。
- 展示面缺失：daemon API 无该字段投影，前端无展示。

### 1.7 v3 拒绝：逐条 Failed，无记忆

- 旧端收 v3 header → `Rejected` ack → 发送端 `Failed(PeerRejected)`（阶段 4
  §3.3）。每条目录对每个旧 peer 重复一次完整 dispatch + 失败。
- presence 元数据不含对端应用版本，「对端已升级」无可靠信号。

### 1.8 settings：字段齐备，缺界面

- `FileSyncSettings`（`crates/uc-core/src/settings/model.rs:276-306`）：
  `file_sync_enabled` / `small_file_threshold` / `max_file_size` /
  `max_file_set_total_bytes` / `max_file_set_member_count` /
  `file_cache_quota_per_device` / `file_retention_hours` / `file_auto_cleanup` /
  `auto_save_dir`。设置界面为纯暴露工作。

---

## 2. 决策总表（已全部拍板，2026-07-12）

| # | 决策点 | 结论 | 依据 |
| --- | --- | --- | --- |
| D1 | #1318 修复方案 A vs B | **方案 B：mobile 读侧回退**；register 推进侧不动 | §1.1 方案 A 误伤桌面三路径；可消费性是消费方知识，不该倒灌共享真相层 |
| D2 | 「上一个可消费值」回退源 | **register 行加影子列**，advance 时携带可消费标志维护；升级后解锁时幂等回填 | 忠实兑现 ADR「保持上一个值」；单行迁移；GET 侧零检测（§3.2）；无回填则存量用户升级后手机读空（§3.1） |
| D3 | 可消费性谓词 | **文件集含目录结构**（`has_directory_structure`），单文件/平铺多文件维持可消费 | ADR 字面「不消费文件集 entry」会砍掉手机现有单文件同步（§1.2）；需给 ADR 加澄清注记 |
| D4 | pull-serve 泄漏 | **并入 P0**：serve 与 dispatch/resend 同源走目录感知出站路径 | §1.3；消灭第三入口的平行逻辑 |
| D5 | 发送端目录进度 | **甲档：只显状态机（传输中/完成/失败/取消），抑制字节百分比** | §1.4 假进度；per-member id 建模留 backlog |
| D6 | 接收端目录进度 | **聚合单行**：Σ字节 + 「n/m 个文件」；不做成员展开 | §1.5 地基就位，聚合在前端/投影层完成 |
| D7 | 取消语义 | **entry 级取消**，不做单成员取消 | 全有或全无下单成员取消必致整体失败，按钮只造困惑 |
| D8 | Sync-ineligible 展示 | **简档**：卡片徽标 + 分类原因；不指认元凶成员 | §1.6 纯投影即可；元凶定位需捕获侧改动，由真实反馈驱动 |
| D9 | v3 拒绝噪声 | **内存级记忆**，跳过 dispatch 但仍落 Failed 投递记录；遗忘 = Offline 转换清除 + TTL 兜底 + 手动重发绕过 | presence 无连接代次、静默重连无事件（§6），「重连即遗忘」不可实现；不违反「不静默」 |
| D10 | reserver TOCTOU | 给 `ReserveInboundFileTargetPort` 加 **目录感知预留**，占位全程持有 | CodeRabbit #1323 Finding 2 |
| D11 | settings UI 范围 | **裁剪子集**：开关、单文件上限、总量上限、成员数上限、`auto_save_dir` | 运维旋钮（threshold/配额/保留）不上界面 |
| D12 | 用户文档 | docs-site「文件夹同步」一页，中文 | 平台范围、手机行为、不可同步规则、上限、失败/重发 |

---

## 3. P0-A：#1318 移动端降级（方案 B + 影子列）

### 3.1 影子列

- 迁移：`active_clipboard_register` 行加一个可空列
  `consumable_ref_ciphertext BLOB`（影子值 `(snapshot_hash, entry_id)` 的
  AEAD 密文，见下；激活时间戳/设备沿用主列语义，不重复存；影子值只回答
  「上一个可消费值是什么」，不参与 LWW）。
- **加密口径（round 7 定稿：影子值加密存储，撤回此前「明文主张留待 PR
  论证」）**：影子对 `(snapshot_hash, entry_id)` 序列化后作** 单个 AEAD 密文
  blob** 存入一列（如 `consumable_ref_ciphertext BLOB`）。理由：内容 hash
  虽不可逆，但可被字典攻击确认短文本、且影子列会 **保留已过期的旧内容指纹**，
  扩大静态可见面；「持久化即密文」底线下唯一明文例外是内容类型枚举，主列
  的存量明文不构成新增列的先例。加密要素在此定死（不留到 PR）：
  - 密钥/上下文：沿用 `entry_file_set` 行加密的同一 MasterKey 派生链路
    （`DeriveSpaceSubkeyPort`，独立 subkey 标签如 `active-register-consumable`）；
  - 读写均要求解锁会话——mobile GET 服务内容本就需解锁，无新增可用性约束；
    未解锁时 GET 返回无内容（与现状锁定行为一致）；
  - 条件原子守卫 **不受影响**：UPDATE 的 WHERE 只比较主列明文
    （`consumable_ref_ciphertext IS NULL AND snapshot_hash=? AND entry_id=?`），
    密文只是 SET 的 payload，无需在 SQL 里比较密文。
- **升级回填（防止存量用户手机读空）**：迁移后影子列为 NULL，而 GET 立即
  切读影子列——若不回填，存量用户升级后直到下一次复制可消费内容前手机一直
  404。回填是 **app 层「解锁后且影子列为空」时执行的幂等操作**（round 3
  审查修正：不能绑在启动时——`entry_file_set` 行是密文，会话未解锁时探针
  必然失败，若按「失败判不可消费」一次性跳过，空值将永久留存；改为可重复
  触发，自动/手动解锁两条路径都覆盖，SQL 迁移层不做判定）：
  加载 register 主列 → 经 §3.2 探针判定 →
  可消费则拷入影子列；是目录则 **留空**。**写入必须是条件原子操作**（round 4
  审查修正：回填是「读→判→写」多步，正常激活可能在中间插入，无条件写会把
  影子列覆盖回旧值）：单条 UPDATE 带 `WHERE consumable_ref_ciphertext IS NULL AND
  snapshot_hash = <判定时值> AND entry_id = <判定时值>` 守卫——**hash 与
  entry_id 双标识**（round 5 补强：同内容重新激活时 hash 不变而 entry_id
  变化，只比 hash 会拼出旧 entry + 新激活的错配），任一条件失效即放弃
  （说明已有新激活接管）（升级前 register 已指向目录时，
  「上一个可消费值」在旧版本中本就不存在，404 是忠实语义，且旧版本此刻
  正在泄漏伪 File，留空即止血）。
- `AdvanceActiveClipboardPort::advance` 签名扩展为携带可消费标志
  （形态建议 `advance(&state, mobile_consumable: bool)`，或包一个
  `ActiveClipboardAdvance { state, mobile_consumable }` 值对象——PR 内定稿）。
  语义：`mobile_consumable == true` 时主列与影子列同写同一值；false 时只写主列，
  影子列保留旧值。**编译器强制所有推进位点表态**。

### 3.2 谓词与调用方

- 谓词单一真相源：`EntryFileSet::has_directory_structure()`
  （`crates/uc-core/src/clipboard/file_set.rs:126-135`）/
  `EntryFileSetLine::indicates_directory_root(row_count)`。**禁止内联复制判定**。
- app 层引入一个小探针（如 `MobileConsumabilityProbe`，包
  `EntryFileSetRepositoryPort`，`crates/uc-core/src/ports/clipboard/entry_file_set.rs:23`）：
  `is_mobile_consumable(entry_id) -> bool`（无 file-set 行或无目录结构 → true）。
- **推进位点共三类，全部接探针**（审查修正：初稿漏了第 3 条）：
  1. 本地写入：`LocalActiveRegisterAdvancer::advance_local`（捕获 announce、restore）；
  2. 0xC3 入站收敛：`active_state/apply_inbound.rs:135`；
  3. **普通剪贴板推送接收**：`apply_inbound/usecase.rs:191` ——B 机收到 A 机
     push 的目录 entry 后同样推进 register，漏接则接收机的手机照旧泄漏。
  port 签名扩展由编译器强制三处（及未来新增位点）全部表态，这是选择改签名
  而非旁路装饰器的核心理由。
- 查询失败的降级方向：**判不可消费**（影子列不动，手机拿旧值）——宁可手机
  少更新一次，不可把未知形态推给手机。

### 3.3 mobile GET 切换

- 新建 **独立小端口**（如 `LoadMobileConsumableClipboardPort`，round 3 审查
  修正：不往通用 `LoadActiveClipboardPort` 上加方法——该端口有大量非移动端
  调用者，按 `docs/architecture/ports.md` 的「不同用途/调用者用独立小接口」
  纪律拆开；infra 侧同一 repo 实现两个端口），返回 **专用最小值对象**
  （如 `MobileConsumableRef { snapshot_hash, entry_id }`）——**不得** 把
  影子列包装成 `ActiveClipboardState`：影子内容标识配当前激活的时间戳/设备
  会拼出自相矛盾的「激活状态」，误导调用方把两次激活当同一次（round 2
  审查修正）。mobile GET 链路只消费这两个字段（§1.2），最小值对象即够。
  `latest_snapshot_adapter.rs` 把 `load()` 换成它。**`MobileSyncSnapshotPorts` 不加任何 file-set 端口**——原方案 B
  描述里的 `EntryFileSetRepositoryPort` 注入整个省掉，检测收敛在推进侧一处。
- SSE 零改动（§1.2 推论：信号照发，手机拉到回退值去重后无操作）。

### 3.4 ADR 澄清注记

- ADR-010 连带决策 5 补一行：「不消费文件集 entry」限缩为「不消费**含目录
  结构的**文件集 entry」；单文件与平铺多文件维持移动端可消费（现状行为）。

### 3.5 测试要点

- 影子列迁移 + advance 双写/单写矩阵（consumable true/false × 首写/覆写）。
- 目录激活后 mobile GET 返回上一个可消费值（entry/hash 均为旧值）；
  连续两个目录激活 → 仍返回更早的可消费值。
- 三类推进位点（本地 / 0xC3 收敛 / push 接收）各自维护影子列。
- **升级回填四态**：升级时 register 指向普通内容 → 解锁后回填、GET 有值；
  升级时指向目录 → 影子列留空、GET 404（止血而非泄漏）；启动时锁定、随后
  手动解锁 → 回填在解锁后仍执行（round 3 审查补充）；回填期间发生新激活
  （含同内容不同 entry 重新激活）→ 条件守卫放弃写入、不覆盖新结果
  （round 4/5 审查补充）。
- 谓词探针：无 file-set 行 / 平铺多文件 / 含目录结构 / 查询报错四态。
- 现有单文件 mobile 同步回归不变。

### 3.6 实现结果（2026-07-13）

T1 / #1326 已完成：active register 已加入加密影子值，三类推进位点统一按
文件集结构维护影子值，mobile GET 已切到独立的可消费值读取端口，自动解锁与
手动解锁都会执行带双标识守卫的幂等回填。单文件和平铺多文件行为保持不变，
含目录结构的 entry 不再作为伪 File 返回手机。

验证覆盖：迁移与密文残留检查、双写/保留矩阵、连续目录激活、回填四态、
锁定时返回空内容、可消费性四态、手机预览/文件读取回归、全工作区测试与检查。

---

## 4. P0-B：pull-serve 目录泄漏收口

### 4.1 决策落地（D4）

- `serve_pull.rs` 并入阶段 4 的同源出站路径：经 `resolve_outbound_file_set`
  判定；`DirectorySyncable` → 与 dispatch/resend 同一 `UCDS` 编码产带清单的
  v3 payload。**禁止在 serve_pull 内再抄一份目录判定或清单编码**。
- 真正不可同步态（`DirectoryNotYetSyncable` 残留语义，如密钥未解锁）维持
  `NotAvailable`。

### 4.2 旧端 puller 的失败形态（有意决策，供审查挑战）

- pull 协议无 header 版本门控（envelope 直接是加密的 payload 字节）。旧端
  puller 拉到含 `UCDS` 尾段的 payload 时，`UCBS` 解码器的严格尾字节检查
  （阶段 4 计划 §1.2）会 **响亮报错**、丢弃本次 pull——不是静默损坏，等价于
  「拉取失败」。旧端本来也无法正确重建目录，此失败形态可接受；不为 pull
  通道新建版本协商。

### 4.3 测试要点

- 新 puller 对目录 entry：pull → UCDS 解码 → 全有或全无重建（复用阶段 4
  接收主线测试基建）。
- 平铺/文本 entry 的 pull 回归不变（无 UCDS 尾段）。
- 模拟旧端 puller 解码含 UCDS payload → 硬报错不入库。

---

## 5. P1-A：目录传输进度与取消

### 5.1 接收端（D6/D7）

- **后端**：确认/补齐成员级 `FileTransferEvent` 发射（§1.5 ⚠️）；如需，daemon
  WS 事件带 `entry_id` 关联（成员 transfer_id 形如 `{entry_id}:member:{i}`，
  entry 关联已可从 id 解出或经 `FindEntryIdForTransferPort`）。
- **前端**：`fileTransferSlice` 把 `entryTransferMap` 改为 entry → transferId
  集合；选择器按 entry 聚合 Σ`bytesTransferred`/Σ`totalBytes` + 完成计数；
  `HistoryCardTransferProgress` 对目录 entry 渲染聚合进度 + 「n/m 个文件」。
- **取消（审查修正：不能笼统「复用失败流程」）**：现状取消入口只接受单个
  transfer_id，且成员逐个顺序 fetch——中止当前成员会让后续成员统一落 **Failed**
  而非 Cancelled（`materializer.rs:838` 一带），聚合优先级 failed > cancelled
  会把整体判成失败。需要新建 **entry 级取消入口**，语义：
  1. 置该 entry 的取消旗标；取消检查 **贯穿整个接收过程**（round 4 审查修正，
     不只在成员边界）：成员边界、全部成员完成后的完整性校验前、原子提升前
     各查一次。**取消与提升的终局通过同一 attempt 状态机原子裁决**（round 6
     审查修正：check-then-rename 仍有窗口）：提升与取消都必须对当前 attempt
     做一次原子状态迁移「认领」（如 Active→Promoting vs Active→Cancelled，
     CAS 语义），谁先认领谁生效，输家得到明确的 no-op 结果——取消成功则
     目录必不出现，提升已认领则取消返回「已完成，无法取消」。补一条在
     终检与 rename 之间注入暂停的确定性竞态测试；
  2. 中止当前 in-flight 成员 fetch，取消原因 `LocalUser`；
  3. 未开始成员的 `file_transfer` 行统一标 **Cancelled（LocalUser）**，禁止
     落 Failed；
  4. 聚合结果为整体 Cancelled，暂存目录清理与不提升复用全有或全无路径。
  不暴露单成员取消。
- **重试用显式 attempt_id（round 3 引入、round 4 补强——「按 entry 汇总全部
  记录」会让旧终态行持续参与聚合，新尝试成功仍显示失败/取消）**：每次接收
  分配显式 `attempt_id`，成员 transfer 行携带它（如
  `{entry}:member:{i}:attempt:{n}`），entry 侧记录 **当前 attempt_id**；
  持久化聚合查询与前端进度 **只汇总当前尝试**，旧尝试事件按 attempt_id
  丢弃、其行保留为历史。**attempt 所有权语义（round 5 补强，防重叠尝试）**：
  - **attempt 状态的持久权威（round 7 补强：现有存储无 attempt 所有权
    字段，内存旗标不构成权威）**：新增 entry 级 attempt 状态存储（迁移 +
    repository，**归入 PR-3 范围**；形态 PR 内定，如 entry 级投影表加
    `current_attempt_id` + `attempt_state` 两列），所有状态迁移经该单一
    权威以 CAS 完成；
  - **三个竞争动作（begin-retry / cancel / publish）全部经状态机原子裁决**：
    - begin-retry：仅当当前 attempt 处于可替换态（Failed/Cancelled/Active
      未认领提升）时原子切换为新 attempt 并中止旧 fetch；**当前 attempt 已
      认领 Promoting 时 retry 返回明确 no-op（或等待终局）**——不得出现
      「旧 attempt 已在发布、新 attempt 又装入」的双活；
    - cancel：携带 attempt_id，只对目标 attempt 生效，迟到取消不误伤新重试；
    - publish：认领 Promoting 后旧/新其他动作全部 no-op。
  - **重启恢复（round 8 引入、round 9 补强）**：恢复裁决 **不得** 以「root 是否
    出现在最终位置」为准——发布先于身份校验与 entry 落库，两个间隙中断都会
    被误判成功。改为 **加密发布日志**（AEAD，密钥沿 §3.1 同派生链路）作为
    恢复的单一依据：记录 attempt_id、当前阶段、每个 root 的暂存→最终路径
    映射、entry 落库完成标记。**落库标记与接收记录必须同事务写入**（round 10
    审查修正：两者分开写，间隙崩溃会让恢复误回滚已成功保存的结果）——发布
    日志与接收记录同在 SQLite 主库，标记随接收记录在同一事务提交；恢复裁决
    以「当前 attempt 的 entry 接收记录是否存在」为准，标记只是加速索引。
    重启时：落库确认 → 补终态收尾；否则按映射回滚已发布 root、判 Failed
    （可重发）。补「写入接收记录与标记之间崩溃」定点测试。**Active（下载/校验中）
    的 attempt 同样在启动时收敛**（round 9：现有启动清理只关成员传输行，
    `uc-bootstrap/src/subsystem/file_transfer.rs:209` 一带，不认识新的 attempt
    权威）——所有非终态 attempt 统一 fail-and-clean（attempt 行与成员行一致
    更新），不做隐式续传。测试覆盖：发布前中断、多 root 发布一半中断、全部
    发布但校验前中断、落库前中断、Active 态中断五种。
  - **整体状态以 attempt 状态机为唯一权威（round 8 补强）**：成员文件行只喂
    进度计算，**不得** 由「全部成员 completed」直接推导整体完成——成员下载
    完成早于校验/发布；整体状态序列 = 传输中 → 校验/发布中（UI 仍显示
    进行中）→ 全部发布且接收记录落库成功后才是完成。补「成员全完成但发布
    失败 → 整体 Failed 而非完成」测试。
  - **当前 attempt 在初始查询与实时事件中都暴露**，前端不靠事件顺序猜。
  - **重载/断线后的聚合重建（round 6 审查，部分采纳）**：初始查询返回当前
    attempt 的 **逐成员状态 + 成员大小**（大小来自清单/blob refs，本就已知），
    已完成字节 = Σ已完成成员大小，文件计数照实恢复；**不持久化流式字节
    计数**（为进度条往 SQLite 持续写计数是新的写放大，收益只是飞行中那
    一个成员的部分字节——它在下一条实时进度事件到达时即自愈）。测试补
    「若干成员完成后重连 → 聚合恢复正确」「成员传输中途重连 → 该成员从 0
    显示、随事件自愈」。
  测试六条：「取消后重发」「失败后重发」「旧尝试事件晚到不污染新尝试」
  「旧失败记录存在但新尝试成功 → 整体显示完成」「旧 attempt 存活时重试 →
  旧完成不提升」「取消与重试竞态 → 迟到取消不杀新尝试」。

### 5.2 发送端（D5）

- 目录 entry 的出站行只渲染状态（传输中/完成/失败/已取消），**不渲染字节
  百分比**：反向进度帧对目录 entry（id = 目录 entry_id 且 entry 为目录形态）
  只取终态语义，InProgress 帧折叠为「传输中」。
- 落点在前端渲染层 + （如需）daemon 投影层打目录标记；**不改反向 wire、不改
  reporter**。
- per-member 出站建模（id 分配 + reporter 重建模 + wire 携带）维持 backlog
  （阶段 4 计划 §10 已记录），不在本阶段。

### 5.3 测试要点

- 聚合选择器：多成员进度合成、成员失败/取消对聚合行的影响。
- 目录 entry 发送行无百分比、终态正确；单文件发送行为回归不变。
- entry 级取消 → 全部成员 fetch 撤销 → 整体 Cancelled + 暂存清理（复用阶段 4
  全有或全无测试）。

---

## 6. P1-B：v3 拒绝的会话级记忆（D9）

- dispatch 层内存态记忆（形态 PR 内定）：首次对某 peer 发 **v3 目录帧** 收
  `PeerRejected` → 只记事实本身「该 peer 拒绝了目录帧」。**不得** 引申为
  「版本过旧」——`PeerRejected` 还覆盖未知设备、帧损坏等不可区分原因
  （round 2 审查修正），可区分的拒绝原因是 backlog，不在本阶段。
- 记忆有效期内后续 **目录 entry** 对此 peer 跳过网络 dispatch，投递记录沿用
  **现有 `PeerRejected` 失败分类**，稳定标识（如 `peer_rejected_directory_frame`）
  放入既有补充说明通道（detail/message 字段，PR 内对齐实际类型）——**不新增
  失败分类枚举、不塞任意字符串进分类位**（round 2 审查修正）。
- UI 文案：「对端暂不接受文件夹同步」（中性表述，不断言版本原因）。
- **遗忘机制（审查修正：「重连即遗忘」不可实现）**：presence 只有
  Online/Offline 转换与时间戳，无连接代次（`crates/uc-core/src/ports/presence.rs:30`），
  且短暂重启可能不产生任何转换事件（`presence_adapter.rs:269` 一带）。改为
  双通道遗忘：
  1. **观测到该 peer 的 Offline 转换即清除**（升级重启多数会触发）；
  2. **TTL 兜底**（建议 30 分钟）：静默重连升级的最坏情况 = 抑制多存续一个
     TTL 窗口后自动重试；
  3. **手动重发绕过记忆**：用户对 entry 显式 resend 时无条件真实 dispatch
     ——用户主动动作即最强的「再试一次」信号，也是即时逃生口。
- **不静默**：每条目录对每个旧 peer 仍有可见投递记录，只省掉注定失败的往返。
- 纯平铺/文本帧（v2）不受记忆影响，照常发。
- 测试：拒绝→记忆→跳过；Offline 转换清除；TTL 过期重试；手动重发绕过；
  v2 帧不受影响；多 peer 隔离。

## 7. P1-C：Sync-ineligible 原因展示（D8，简档）

- daemon 投影：entry 查询结果增字段（如 `syncIneligibleReason`），来源 =
  该 entry `entry_file_set` 任一 `Excluded` 行的 reason（§1.6 整组同因）。
- 前端：历史卡片「不可同步」徽标 + hover/详情人话文案：
  `UnsupportedMember`→「包含符号链接或特殊文件」、`SizeCapExceeded`→
  「超过文件夹同步上限」、`IngestFailed`→「读取失败」。
  **审查修正**：现有 `EntryFileSetExcludeReason` 的 `SizeCapExceeded` 不区分
  「总量超限」与「成员数超限」（`file_set.rs:121`），简档不改持久化枚举，
  两者统一为一句文案；细分原因（及元凶指认）一并记 backlog，由捕获侧
  reason 细化驱动。
- 不做元凶成员指认（繁档记 backlog，由用户反馈驱动；需捕获侧记录元凶路径，
  §1.6）。

## 8. P2：技术债与收尾

### 8.1 reserver 目录感知预留（D10）

- `ReserveInboundFileTargetPort` 增加目录发布语义，消灭现状
  `remove_file` → `rename` 的 TOCTOU 窗口（`materializer.rs:932` 一带）。
  **具体机制（round 6 定稿：「隐藏暂存 + no-replace 原子发布」，取代此前
  两版占位方案——可见占位会让用户在传输期间看到空的「已同步」文件夹，
  崩溃还会把它永久遗留）**：
  - **发布前最终路径上不存在任何东西**：全部准备工作在**目标卷上的隐藏
    暂存目录**完成（同卷保证 rename 原子性；跨卷场景先复制 + 校验到该隐藏
    暂存，再走同一发布序列）。
  - **发布 = no-replace 原子改名**：Linux `renameat2(RENAME_NOREPLACE)`、
    macOS `renamex_np(RENAME_EXCL)`、Windows `MoveFileEx`（不带
    REPLACE_EXISTING，目标已存在即失败）——三平台原生支持「目标存在则失败」
    语义。失败（落点被抢占）→ 取下一个 `folder (2)` 后缀重试，**永不覆盖、
    永不合并**，竞态从「静默混入」转为「有界重试」。
  - **异常清理**：崩溃/失败遗留物只存在于隐藏暂存目录（可识别命名），
    启动清扫或下次接收时回收；最终可见位置在发布成功前**不出现任何中间
    产物**。
  - **多 root 发布序列（round 7 审查修正：no-replace 是单 root 原子，混合
    选择的多个顶层 root 逐个发布存在部分可见窗口）**：
    - 发布前 **预检全部 root**（名称冲突一次性解决、权限/同卷确认），把
      中途失败概率压到最低；
    - 逐 root no-replace 发布；任一 root 失败 → **补偿回滚**：把已发布的
      root rename 回隐藏暂存（同卷 rename，近乎必成）→ entry 判 Failed、
      最终位置零残留；
    - **回滚能力保留至最终提交（round 8 补强）**：发布后的完整性校验、
      接收记录落库任一失败，同样撤回已发布 root（发布≠终点，「全有」的
      判定点是接收记录成功持久化）；补「发布成功后校验失败」「发布成功后
      落库失败」两组测试；
    - 回滚本身失败（极端）→ entry 判 Failed；**已可见 root 清单只落加密
      发布日志**（round 9 CRITICAL 修正：root 名是用户内容，`file_transfer`
      失败详情字段是明文列，路径/名称不得入明文 DB 字段或日志），明文侧
      只记非敏感标记（如 `partial_publication` + 数量）；补「名称/路径不出
      现在任何明文字段与日志」断言测试；
    - 不采用「全部 root 装进单一容器目录」方案：粘贴 3 个项目却得到一层
      包装文件夹违背粘贴语义。
    - 备注：阶段 4 已上线代码即逐 root rename（同窗口存在），本设计是收紧
      而非回归。
  - 测试：发布前最终路径不可见、冲突后缀重试、崩溃遗留物清理、多 root 中途
    失败回滚归零、回滚失败的显式部分发布记录五组。
- 跨 `uc-core` 端口演进，先读 `docs/architecture/ports.md` 与
  `crates/uc-core/AGENTS.md` 端口纪律。

### 8.2 `file_sync` 设置界面（D11）

- 暴露：`file_sync_enabled`、`max_file_size`、`max_file_set_total_bytes`、
  `max_file_set_member_count`、`auto_save_dir`（目录选择器）。
- 不暴露：`small_file_threshold`、`file_cache_quota_per_device`、
  `file_retention_hours`、`file_auto_cleanup`（维持默认，避免设置页旗标仓库化）。
- **非纯前端**（round 5 审查修正）：现行公开设置 API **有意省略**
  `max_file_set_total_bytes` / `max_file_set_member_count` 两字段，入站更新
  会将其丢弃——T8 需同步扩展 webserver 设置 DTO/请求处理/校验、重新生成前端
  类型，并补 round-trip 测试；上限变更即时生效于后续捕获（捕获侧已读
  settings，无需新接线）。

### 8.3 用户文档（D12）

- docs-site 新页「文件夹同步」（中文）：支持平台（macOS/Windows/Linux 桌面）、
  手机行为（保持上一个可消费内容，不接收文件夹）、不可同步规则（符号链接/
  特殊文件/上限）、默认上限与设置入口、失败展示与手动重发（增量补缺）。

---

## 9. 子 ticket 拆分（挂 map #1325，2026-07-12 已发布）

已发布编号：T1=#1326 · T2=#1327 · T7=#1328 · T3=#1329 · T4=#1330 ·
T5=#1331 · T6=#1332 · T8=#1333 · T9=#1334。（阶段 5 新开独立 map #1325；
#1315 是阶段 4 map，随其子项收口。）

| ticket | 优先级 | 内容 | 依赖 |
| --- | --- | --- | --- |
| T1 ✅ | P0 | §3 移动端影子列降级（三推进位点 + 升级回填）+ ADR 澄清注记（关 #1318） | 无 |
| T2 | P0 | §4 pull-serve 同源目录出站 | 无（与 T1 并行） |
| T3 | P1 | §5.1 接收端聚合进度 + entry 级取消入口（后端取消语义 + 前端聚合） | **T7**（round 9：attempt 发布状态/恢复依赖 T7 的发布/回滚/暂存行为，不得在旧发布器上验证恢复保证） |
| T4 | P1 | §5.2 发送端目录状态化显示（去假百分比） | 无 |
| T5 | P1 | §7 Sync-ineligible 投影 + 徽标 | 无 |
| T6 | P1 | §6 v3 拒绝会话记忆 | 无 |
| T7 | P2 | §8.1 reserver 目录感知预留 | 无 |
| T8 | P2 | §8.2 settings 界面 | 无 |
| T9 | P2 | §8.3 用户文档 | 建议最后（内容依赖 T3-T5 的 UI 定稿） |

依赖 DAG：仅 **T7 → T3** 一条硬依赖（round 9），其余各自独立可领取；
#875 在 T3/T4/T5 合并后可关。

## 10. PR 划分

| PR | 内容 | 层 |
| --- | --- | --- |
| PR-1 | T1（迁移 + port 签名 + 探针 + GET 切换 + ADR 注记），提交按 core/infra/app 拆 | core+infra+app |
| PR-2 | T2（serve_pull 同源化） | app |
| PR-3 | T3 后端（attempt 状态存储迁移 + repository + CAS 状态机 + entry 级取消入口 + 相关 API/事件） | core+infra+app+webserver |
| PR-4 | T6（v3 拒绝记忆） | app |
| PR-5 | T5 后端投影（sync-ineligible API 字段） | app+webserver |
| PR-6 | T3 + T4 + T5 前端（进度聚合、取消按钮、状态化发送行、徽标） | frontend |
| PR-7 | T7（reserver 端口演进） | core+infra+app |
| PR-8 | T8 settings（webserver DTO/校验 + 前端界面） | webserver+frontend |
| PR-9 | T9 文档 | docs-site |

（round 5 审查修正：原 PR-3 捆绑三个行为/存储/回滚边界各异的变更，改为按
ticket 独立成 PR。）顺序约束：PR-1/PR-2 先行（P0）；**PR-7（T7 发布/回滚/
暂存）先于 PR-3**（round 9：attempt 恢复保证必须建在新发布器上，加密发布
日志与 no-replace 发布同属一套接口，两 PR 若边界难切可合并交付）；PR-3/PR-5
先于 PR-6（前端消费其 API 字段与事件）；其余无序。

## 11. 审查结论（2026-07-12 对抗审查已闭环）

初稿的四个开放问题经审查核清，另收 7 条修正（均已写回本文对应章节）：

**已核清（无需再查）：**
1. **泄漏路径穷尽性**：mobile LAN 的历史/内容/文件 route 最终都汇入同一条
   「取最新内容」路径（`latest_snapshot_adapter`）——§3 的单点回退即覆盖全部
   出口，无第二泄漏面。
2. **成员级事件**：目录成员 fetch 已逐成员报告进度（§1.5），T3 无后端事件缺口。
3. **§4.2 旧端 puller 失败形态**：旧版本解码含目录清单的 payload 确认响亮
   失败而非静默损坏，「不建 pull 版本协商」成立。

**Round 1 修正（已并入正文）：** 影子列加密口径论证与升级回填（§3.1）、第三条
推进位点 push 接收（§3.2）、entry 级取消不能复用失败流程（§5.1）、v3 遗忘
机制改 Offline 转换 + TTL + 手动重发绕过（§6）、上限原因不可细分改统一文案
（§7）、目录预留操作序列具体化（§8.1）。

**Round 2 修正（已并入正文）：**mobile 读返回专用最小值对象而非拼装
`ActiveClipboardState`（§3.3）、成员 transfer_id 重试复位规则与重发测试
（§5.1）、`PeerRejected` 不引申「版本过旧」+ 沿用现有失败分类而非任意
reason 字符串（§6）、跨卷回退撤回「写入占位内部」改为目标卷隐藏暂存 +
同卷提升（§8.1）。

**Round 10 修正（已并入正文，终轮）：** 落库标记与接收记录同事务提交，恢复
以 DB 接收记录为准（§5.1）。

**Round 9 修正（已并入正文）：** 恢复裁决改以加密发布日志为单一依据（含
阶段/映射/落库标记），非终态 attempt（含 Active）启动统一收敛（§5.1）、
部分发布 root 清单只入密文日志、明文侧仅非敏感标记（§8.1，CRITICAL）、
T7 成为 T3 前置、PR-7 先于 PR-3（§9/§10）。

**Round 8 修正（已并入正文）：**Promoting 持久态的重启恢复裁决（§5.1）、
整体状态以 attempt 状态机为唯一权威、成员行只喂进度（§5.1）、回滚能力保留
至校验 + 落库全部成功（§8.1）。

**Round 7 修正（已并入正文）：** 影子值改 AEAD 密文存储、加密要素计划内
定死（撤回 round 1 的明文主张）（§3.1）、attempt 状态持久权威 + retry 纳入
CAS 三方裁决、存储工作归 PR-3（§5.1/§10）、多 root 发布补偿回滚 + 响亮部分
发布记录（§8.1）。

**Round 6 修正（已并入正文）：** 取消/提升经 attempt 状态机 CAS 认领终局
（§5.1）、重载后聚合从成员终态 + 已知大小重建（部分采纳，不持久化流式字节）
（§5.1）、目录预留定稿「隐藏暂存 + no-replace 原子发布」取代可见占位
（§8.1）。

**Round 5 修正（已并入正文）：** 回填守卫改 hash+entry_id 双标识 + 竞态测试
（§3.1/§3.5）、attempt 所有权语义（原子 begin-attempt、取消/提升绑定
attempt、当前 attempt 全量暴露）（§5.1）、T8 非纯前端（设置 API 缺上限
字段）（§8.2）、PR 按 ticket 拆分（§10）。

**Round 4 修正（已并入正文）：** 重发聚合改显式 attempt_id、只汇总当前尝试
（§5.1）、回填写入加条件原子守卫防激活竞态（§3.1）、取消检查贯穿至提升前
（§5.1）。

**Round 3 修正（已并入正文）：** 重发改用尝试级 transfer_id（撤回「同 id
重置」，终态行无法复活）（§5.1）、回填改「解锁后幂等」不绑启动（§3.1/§3.5）、
mobile 读取拆独立小端口不动通用 `LoadActiveClipboardPort`（§3.3）、§1.4
发送端现状描述校正（终态正确、乱的是字节百分比）+ 补终态回归测试、T3 后端
取消入口归入 PR-3（§10）。

**留给实现期的次要确认：**restore 路径每次多一次 file-set 行查询的性能影响
（预期可忽略，探针查询按 entry_id 索引；若有感知在 PR-1 内加缓存）。

## 12. 明确不在阶段 5（Out of scope）

- **发送端 per-member 出站建模**（id 分配 + reporter + wire）：阶段 4 计划 §10
  已记录，等进度 UI 的真实需求驱动。
- **Sync-ineligible 元凶成员指认**（繁档，§7）。
- **v3 能力的持久化记忆 / 版本协商 handshake**（D9 采会话级内存记忆）。
- **阶段 3b**：延迟身份就绪 + `(mtime,size)` 漂移复核（正交，独立排期）。
- **移动端完整参与文件集同步**：ADR 范围外。
- **平铺多文件 mobile GET 只取首文件** 的既有边界（§1.2）：非目录同步引入，
  不在本阶段动。
