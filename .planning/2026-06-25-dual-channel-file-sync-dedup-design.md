# 双通道文件同步去重 — 设计方案（issue: filesyncbug）

状态：设计 v6（已并入 Codex round-1..5 共 31 条 finding；round-4/5 为 re-raise/规格精度）。核心三层设计已稳固收敛；剩余精度（manifest schema、锁 wiring、legacy backfill 细节）在实现期对真实类型/schema 落实。审查循环在 round 5 收口。
日期：2026-06-25。
关联：issue #1117 ActiveClipboardState（`.planning/2026-06-19-issue-1017-active-clipboard-state-design.md`）。本方案修复其一个实现漏洞，**不** 改动其锁定决策。

---

## 1. 现象

Windows 复制一个文件（`deskflow-1.26.0-win-x64.msi`，~15MB）同步到 macOS，dashboard 出现 **两个 entry**：一个完整、一个永久 partial（`uniclip-missing://` 占位的幽灵）。复现条件：先建 relay（慢），叠加 Windows dispatch + restore，再叠加接收端 daemon 反复重启。

## 2. 根因（已从日志 + 代码双向确认）

同一份内容经 **两条通道** 到达接收端，且两条通道携带 **不同的 `snapshot_hash`**：

- 通道 1 — **push/dispatch**（`dispatch_entry::delivery`）：`snapshot_hash = A`（`blake3v1:4985…`）
- 通道 2 — **active-clipboard pull**（`active_state::serve_pull`，ALPN `uniclipboard/active-clipboard-pull/0`）：`snapshot_hash = B`（`blake3v1:6cb3…`）

接收端 entry 去重以 `snapshot_hash` 为键（`find_entry_id_by_snapshot_hash`），A ≠ B → 两次 miss → 两个 entry。慢 relay + 重启把传输拖成并发，并让其中一个 partial 永久残留。

### 2.1 为什么同一内容会有两个 hash（精确机制）

`Snapshot::snapshot_hash()`（`crates/uc-core/src/clipboard/system.rs:520`）对文件 rep 有两条分支：
- 有 `file_content_digests` → 用 **文件内容** 哈希（设备无关，blake3 of bytes）。
- 无 → 回退哈希 **`text/uri-list`** 内联字节（含设备本地路径，设备相关）。

`file_content_digests` 在 capture 时 **只从 `LocalFile`-source rep 填充**（`crates/uc-application/src/clipboard_capture/usecase.rs:189-202`，`content_hash()` 对 LocalFile 是 `stream_blake3(path)` = 真实文件内容，`system.rs:223`）。

- macOS Finder 复制 → `LocalFile` rep → capture 填了 digests → 存储 hash = 内容哈希 = dispatch 的 hash → **不分叉**。
- Windows 文件复制 → **Inline 的 `text/uri-list` rep**（非 LocalFile）→ capture 填不到 digests → **存储的 `clipboard_event.snapshot_hash` = uri-list 哈希 B**。

而 dispatch 走 `publish_file_blob_refs`（`crates/uc-application/src/facade/clipboard_outbound/mod.rs:618`）读真实文件字节算 `plaintext_hash` → digest 哈希 A。

restore / active-state / pull **忠实地复用存储的 hash B**（`crates/uc-application/src/usecases/clipboard_restore/restore_selection.rs:142-180` 注释亲口说明要用持久 `clipboard_event.snapshot_hash`、不可用 reconstruct 重算）。serve_pull 用请求方传入的 hash 查库、丢弃自己重算的值（`crates/uc-application/src/usecases/clipboard_sync/active_state/serve_pull.rs:99-115,199`）。

**结论**：分叉发生在 **源头 capture**——同一次捕获，**存储的 hash（B，uri-list）≠ dispatch 发出的 hash（A，内容）**。restore/pull 不是 bug，它们正确地用了存储值；错的是存储值本身不是 device-independent 的内容哈希。这是 **Windows 文件复制专属** bug。

## 3. 约束（不可违背）

- VISION 锁定：**剪贴板瞬时性，禁止自动重发 / 排队 / 最终一致**。本方案只做 **幂等去重**，不引入 store-and-forward 或定时重试；§4③ 的失败回退被严格限定为 **单次补偿**（见 4.3）。
- issue1017 §1.2（已过 7 维对抗审查的锁定决策）：跨设备身份键 = **`content_hash`（= snapshot_hash）**，明确否决用 `entry_id`。→ 不引入 source_entry_id 当跨设备身份；修复方向是 **让 content_hash 真正稳定**，而非新增第二套身份。
- daemon = per-profile 单例（fs2 文件锁），**单写进程**。这是 §4② 用进程内 keyed-lock 即可保证原子性（不依赖 DB 约束）的前提。

## 4. 设计：三层

### 0. 贯穿的术语与不变量

- **canonical H**：一份内容的跨设备身份 = 设备无关的内容哈希。**唯一生成入口（R3-F1）**：
  - **只有一个**"计算内容身份"的函数 `content_identity(snapshot) -> H`（= 修正后的 `snapshot_hash()`，见下结构保留）。**所有本地产生内容的路径**（capture、resend 重建、restore-broadcast 等）都先填好 `file_content_digests` 再调它；**不** 允许任何路径 fork 出第二套算法。
  - **结构保留，且为逐行有序贡献（R2-F1 + R4-F1）**：现有 `snapshot_hash()` 在有 digest 时 **整条 file-list rep 被跳过**、只拼一个松散 `file_content_digests` 集合（`system.rs:526-537`）——会丢顺序、丢重复文件、丢非文件 URI/comment → 不同内容撞同一 H。修正为 **逐行有序 file-list 身份贡献**：按 uri-list 原行序，每行编码为三类之一并连同 **行序** 一起进 hash——`file://` 选中行 → `(标记:file, 内容 digest)`；非文件 URI/comment 行 → `(标记:literal, 原字节)`；被排除/物化失败的 file 行 → `(标记:excluded, 原 URI 字节)`（明确定义、不丢行）。绝不退化成"digest 集 + literal 集"。改的是这 **一个** `content_identity` 函数，所有本地产生路径一致受益。
  - **本地产生**：capture 基于 **文件内容** 填 digest 后调 `content_identity` 算出 H，存进 `clipboard_event.snapshot_hash`。
  - **远端接收（inbound）**：H 等于发送方 wire 携带的 `snapshot_hash`。**接收端持久化必须原样采用 wire H，禁止用本机 materialized 快照（已重写本地路径）重算**（F-4）。
  - 编码函数（`encode_*_to_v3_bytes`）**不再产出可被业务使用的 hash**，至多产诊断值。
  - **范围说明**：mobile 入站走独立 LAN HTTP 协议（VISION 锁定，非 iroh），有自己的身份处理，不在本次 iroh 双通道修复内；本节"唯一入口"指桌面 iroh 同步链路的本地产生路径。
- **H 不可变 + Resend 一致性（R4-F3）**：一个 entry 的 canonical H 一旦在出生点确定即 **不可变**，它绑定的是 capture 时持久化的那份 `EntryFileSet`（含被 size-cap 排除的事实）。由此：
  - 任何复用该 entry 的再发（dispatch / active pull serve / 显式 Resend）**只能发 `EntryFileSet` 记录的那一组字节**——**绝不** 在旧 H 下多发当初被排除的文件。
  - 用户若想"把当初超限排除的大文件也发出去"，那是一次 **新捕获 → 新 `EntryFileSet` → 新 H → 新 entry**，不是同一身份。
  - 即消除 R2-F2 接受的"Resend 绕过 size-cap"与"H 不可变"之间的矛盾：Resend 可以是用户显式动作，但它发的仍是原 `EntryFileSet`；绕过 cap 只在 **新捕获** 时影响选集，不回灌旧 entry。
- **完整性/可用性 = 实时派生（F-3/F-7；R2-F4 不反规范化；R3-F4 含 FS 实查）**：一个 entry「可用持有」⟺ 其全部 representation 的 `PayloadAvailability` 均就绪（无 `uniclip-missing://` 占位 / 无 Failed/Lost/未物化 blob ref）**且**——对文件 entry——其 file-list 指向的本地 cache 文件 **实际存在、可读、是普通文件**。**不** 存 denormalized `is_complete` 列（rep 状态由异步物化、janitor/reconciler 多路改写，反规范化会陈旧）。改为一个 **可用性查询接口** `is_entry_available(entry_id)`：先查 reps + missing-URI（DB），文件 entry 再 **实查本地文件 FS 状态 + transfer/cache 一致性**（不能只是 SQL）。调用方（dedup skip、active-state converge）按需查，非热点。**关键**：active-state 仅在 `is_entry_available` 为真时 converge，避免把 DB 显示 Inline 但本地文件已失效的坏内容写进 OS 剪贴板。
- **「已持有」⟺ 完整持有**：所有「我有没有 H」的判断（dedup skip、active-state converge、in-flight 决策）一律以 **complete & available** 为准，**hash 命中但 partial ≠ 已持有**。

### 4.1 ① 根因修复 — canonical compute-once hash

**目标不变量**：canonical H 在出生点算一次，作为 entry 不可变身份存储；**所有再发/持久化路径只读权威值，永不重算充当身份**。

- **capture（本地产生）对 Inline `text/uri-list` 文件 rep 也产出内容 digest**：把这些 rep 在 capture **物化进 blob 仓库**，用物化得到的 ContentHash 填 `file_content_digests`，再算并存储 `snapshot_hash`。
  - **分层（F-10）**：capture usecase **已有** 注入的 blob-writer port（现用于 LocalFile rep，`usecase.rs:266-280` 调 `self.blob_writer.write_path_if_absent`）。inline uri-list 物化 **复用同一注入 port**，application 层不新增对 `uc-infra` 具体 blob writer 的直接依赖。物化一次流式即返回 `(ContentHash, size)`（`crates/uc-infra/src/blob/blob_writer.rs:160`），一次读拿哈希 + 预热 blob，dispatch 命中 `reused_existing` 不再读第二遍。
  - **不** 改 Windows 平台适配器（application 层检测 inline uri-list 指向的本地文件并物化）。
  - **持久"行级 manifest" + 被拥有的读/建接口（R2-F2 + R3-F2 + R4-F2 + R5-F2）**：仅"同一时刻规则一致"不够，且模型必须是 **行级清单而非文件集**（逐行有序身份要求保留行序/重复/非文件行/失败行）。裁决：**capture 时把 uri-list 解析成一份持久 `EntryFileSet` 行级 manifest**——每行 `{ line_index, 原行字节, kind ∈ {file, non_file, excluded}, 选中文件: digest + blob/cache ref + size, 排除行: 排除原因 }`，随 blob 物化落库。落成 **一等公民接口 `EntryFileSet`（reader/builder）**——`content_identity`（算 H）、`dispatch`、`active pull serve`、`restore-broadcast` **都从同一份 manifest 读** 来算身份/构 envelope，**不** 再 reconstruct snapshot + 提取路径 + 探测当前 FS + 重 plan（把 `serve_pull` 现"重建快照→extract paths→metadata→plan"那段换成读 `EntryFileSet`）。
  - **size-cap 语义（R2-F2 + R5-F1，消除自相矛盾）**：size-cap 判定在 **capture 时一次性做** 并固化进 manifest（超限文件记为 `excluded`）。**所有复用既有 entry 的再发（dispatch / active pull serve / 显式 Resend）一律只发 manifest 记录的 `file` 行、不发 `excluded` 行——既有 entry 上没有任何 cap 绕过**。"想把当初超限的文件也发出去" = **新捕获 → 新 manifest → 新 H → 新 entry**（见 §4.0「H 不可变」）。`max_file_size` 只在 **新捕获选集** 时起作用，绝不回灌旧 entry。
- **compute-once 落到每个出口（F-5）**：dispatch 改为按 entry_id 读 `clipboard_event.snapshot_hash` 作为 **唯一 wire identity**，贯穿 **envelope header / delivery 记录 / active register 前进 / 0xC3 广播** 全部使用此存储值；`encode_*_to_v3_bytes` 重算出的 hash **只能用于 debug 断言/诊断，不参与身份**。restore/pull 已是复用存储值。
- **inbound（远端接收）持久化采用 wire H（F-4）**：接收端把 entry 落库时，`clipboard_event.snapshot_hash` = 入站 envelope 携带的 `snapshot_hash`，**不** 由接收端对 materialized（本地路径已重写）快照重算。需在 apply_inbound 持久化点明确传入并断言。

#### 4.1.1 capture 物化 inline uri-list 的逐项规则（F-6）

对 file-list rep 解析出的每个 URI：
- 非 `file://` scheme（http/data/自定义）→ **不参与 digest**，保留原 inline 字节（这类不是本机文件，dispatch 也不会发布）。
- 路径不存在 / 不可读 / 权限拒绝 / 是目录 / UNC·network path 物化失败 → **该 URI 跳过 digest**，记 warn；**与 dispatch 的排除规则一致**（dispatch 同样会 `metadata` 失败即排除，见 serve_pull/outbound 既有逻辑）。
- 物化中途文件被移动/改变 → blob writer 以其读到的字节为准产出 ContentHash；dispatch 复用该 blob（reused_existing）→ 两端一致。
- **部分文件成功、部分失败**：digest 集 = 成功物化的文件集；canonical H 基于该集合；**dispatch 必须用同一集合**（共享 helper 保证），故身份仍唯一。
- **全部文件失败/排除**：不产出文件 digest → 回退原 uri-list 分支哈希（与 dispatch 的 `all_files_excluded` 行为对齐：此时该 entry 不走文件同步路径，无双通道问题）。
- **capture 不因物化失败丢整条捕获**：物化失败只影响 digest 与该 rep 的可同步性，entry 仍落库（可能 partial）。

### 4.2 ② 兜底 — 提交期原子去重 + partial 升级

即使 hash 统一，慢传输下两通道仍可能并发（接收端早期 `find(H)` 在 blob 物化前跑，entry 落库在物化后）。

- **原子机制（F-1/F-9）**：`apply_inbound` 按 H 加 **进程内 keyed-lock**（单写进程，进程内串行足够）。**不加 `snapshot_hash` UNIQUE 索引**——它与软删除（`deleted_at_ms` 留存的 event）冲突（删后重复制同文件会撞约束），且历史库可能已有重复 hash / 孤儿 event 导致迁移失败；进程内锁已是正确性充分条件，UNIQUE 反成负担。
- **keyed-lock 精确范围（F-9，避免长占/重复下载）**：
  1. decode envelope、取 wire H。
  2. **进锁**：登记 in-flight(H)（③ 用）；`find(H)` + 读完整性；判定路线（见下）。**出锁**。
  3. **blob 下载/物化在锁外**（不长占；下载是 IO 重活）。③ 的 in-flight 抑制在常规场景已避免第二路并发下载；残留并发（两路都漏过抑制）会各下一遍，由步骤 4 收敛。
  4. **再进锁**：二次 `find(H)` + 完整性复查 → 提交（新建 / partial 升级 / skip）；清 in-flight 登记。**出锁**。
  - 锁不跨下载 → 无长占、无死锁；下载可能偶发重复（罕见，③ 已抑制），但 entry 不会重复。
- **`find(H)` 的软删除查找契约（R3-F3 + R4-F5，返回三态）**：`Option<EntryId>` 无法区分 none / 已删 / 可见，是自相矛盾的契约。改为查找返回 **三态可见性** `enum LookupByHash { None, Deleted(EntryId), Visible(EntryId) }`（或拆成"可见查找"+"在 replace/restore 事务内的 deleted-candidate 查找"）。调用方据此：
  - `None` → 新建。
  - `Deleted(id)` → "用户删了某内容、随后又重新收到同内容"：在同一事务内 **恢复可见**（清 `deleted_at_ms`）+ 按下方 available/partial 路线处理（必须最终 dashboard 可见，不能只 touch 隐藏项）。
  - `Visible(id)` → 按下方 available/partial 路线。
- **路线判定（在锁内，基于 `is_entry_available`）**：
  - `find(H)` 命中（未删/已恢复）且 **available** → skip / resurface（`touch_entry` bump active_time，不新建卡片、不重下）。
  - `find(H)` 命中但 **不 available（partial/本地文件失效）** → 走 **升级**（用本次完整投递替换）。
  - 未命中 → 新建（物化结果可能 available 或 partial）。
- **完整性判定（F-3；R2-F4 派生）**：用 §4.0 的 `is_entry_complete(entry_id)` 派生查询区分 complete/partial，**不存反规范化列**；`find_entry_id_by_snapshot_hash` 命中后再查完整性。
- **partial 升级 = 事务级替换（F-2）**：**不** 用裸 `delete_entry` 拼 `save`（现有 `delete_entry` 不级联 event/representation/thumbnail/delivery/transfer，会留孤儿 event 占着同一 snapshot_hash）。新增一个 **事务级 entry-replace 能力**：在单事务内，按既定顺序删除旧 entry 的 event/selection/representation/thumbnail/delivery/transfer 关联，再以 **同 entry_id** + wire H 重建完整 entry；定义回滚行为；整个替换在 §4.2 的 keyed-lock 内执行。
  - **保留契约（R2-F5）**：replace 必须 **保留** 用户可见/粘性状态——`pinned`、`active_time_ms`、`created_at_ms`、以及 register 指针语义（指向不变的 entry_id）；**替换** 的是 event（新 event_id）、representation、selection、snapshot 内容、delivery/transfer 关联。契约逐字段列清，并加测试覆盖 pinned/active_time/selection/register 指针/transfer 状态在 replace 前后的预期值。
- **删除**：`recent_source_entries` 内存缓存（`apply_inbound/usecase.rs:234-247,386-389`，§1.2 所禁的 entry_id 兜底）、`if !is_partial` 特例（`usecase.rs:380`，由"升级"取代其"取消后可恢复"目的）。

### 4.3 ③ in-flight 抑制（源头掐第二路 + 省冗余下载）

- `apply_inbound` 进锁时登记按 H 的 **内存** in-flight 标记（两通道共用，RAII / 步骤 4 清除）。
- `active_state/handle_one`（`crates/uc-application/src/usecases/clipboard_sync/active_state/apply_inbound.rs:316-334`）改为按"完整持有"分路（F-7）：
  - `find(H)` 命中且 **complete** → converge（OS 写 + register 前进）。
  - `find(H)` 命中但 **partial**、或未命中 —— 内容未完整持有：
    - **in-flight → 延迟**（不 pull，登记待激活）。
    - 否则 → pull（pull 成功后经 ② 升级/新建为 complete，再 converge）。
  - **关键**：partial 命中 **绝不** 直接 converge（否则把 `uniclip-missing://` 占位写进 OS 剪贴板）。
- **失败回退 = 严格单次补偿（F-8，不构成自动重试）**：
  - 延迟时登记 **内存** `pending_activation[H] = (activated_at_ms, activated_by 等 LWW key + 元数据)`；同一 H 仅保留一条（重复延迟覆盖为最新 activation）。
  - **in-flight 成功必须消费待激活、而非简单丢弃（R2-F3，关键）**：被抑制后那条 in-flight（dispatch/0xC1 bulk）**成功完整提交** 时，若存在 `pending_activation[H]`，**必须用该 activation 的 LWW key `(activated_at_ms, activated_by)` 走与正常 0xC3 收敛同一条 converge tail**（前进 register + OS 写 + re-broadcast），**不可** 用 bulk inbound 自带的入站快照时间充当激活键——否则丢掉 issue1017 的权威 LWW 键、破坏跨设备收敛/断环。消费后清除该条。
  - 触发 **补偿 pull** 的 **唯一** 事件：`apply_inbound` 对 H **失败（partial/cancel）** 时发轻量事件 → active-state worker 取出 `pending_activation[H]` 补偿 pull **一次**；成功后同样用其 LWW key 走 converge tail。
  - `pending_activation[H]` 在以下任一情况 **立即清除、不再补偿**：被 in-flight 成功消费 / 补偿 pull 成功 / 该 activation 被更新的 LWW 取代（过期）/ receive 闸门拒绝 / sender 锁定不可服务 / 补偿 pull 本身再次失败。**绝不循环重试**。
  - in-flight 标记 + `pending_activation` **均内存**，重启自动清空（重启已取消所有传输）→ 交给现有 reconcile + peer-online-resync（presence 驱动，非定时）。
  - 反例驱动：本次事故里 in-flight 的 dispatch 恰是失败者、pull 才是送达者；故"抑制"必须配"失败补偿一次"，但补偿有界、不违反 VISION 红线。

### 4.4 共享 EntryIdentityCoordinator（唯一拥有者 + 覆盖所有写者，R3-F5 + R5-F3）

②③ 的内存协调态（per-H keyed-lock、in-flight 标记集、`pending_activation`、inbound-失败事件分发）**必须有单一拥有者**，否则 bulk inbound、pull store、active-state worker 各持不同实例时"两通道共用"形同虚设——仍会并发下载、漏补偿。

**per-H 身份锁必须覆盖所有 `snapshot_hash` 写者，而不止 inbound（R5-F3）**："单进程 ≠ 单异步写者"——本地 capture 的 resurface/dedup（`clipboard_capture/usecase.rs:223-242` 的 find-then-save）若在 per-H 锁外，会与 inbound 对同一 H 并发双建。裁决：**所有创建/替换 entry-by-H 的路径（本地 capture + inbound）都经一个身份感知写接口，在同一 per-H 锁内做 find-or-create/replace**。该 coordinator 因此命名为 `EntryIdentityCoordinator`（不止 inbound），由它持 per-H 锁、对 capture 与 inbound 的"按 H 落库"统一串行。

- 新增一个 **`InboundCoordinator`** 组件，**单实例** 在组装根（`uc-bootstrap`）创建，以 **同一个 `Arc`** 注入：`apply_inbound`（dispatch 与 pull store 两条入站都经它）、active-state worker。
- 它统一拥有并对外暴露：`lock(H)`（per-H keyed async lock）、`register_inflight(H)/clear_inflight(H)`、`is_inflight(H)`、`set_pending(H, activation)/take_pending(H)`、以及 inbound-失败 → 待激活补偿的事件分发。
- 所有对这些态的读写只经该 coordinator；apply_inbound 与 active-state worker 不各自新建。wiring 在 `crates/uc-bootstrap/src/assembly.rs` 一处完成。
- **register 推进的单一路径（R4-F4，消除并行新旧逻辑）**：本流程里对某 H 的 active-register 推进只有 **一条** converge tail（前进 register + OS 写 + re-broadcast），由 coordinator 决定其 LWW key：
  - bulk `apply_inbound`（含 dispatch/0xC1）成功提交后 **不再直接就地推进 register**（旧路径用入站快照时间戳，会与新 converge tail 竞态、压住正确 LWW key——正是 R2-F3 要防的）；改为 **通知 coordinator** inbound 结果（complete/failed）。
  - coordinator 的 converge tail：**若存在 `pending_activation[H]`** → 用其 `(activated_at_ms, activated_by)` 收敛；**否则**（纯 dispatch、无 0xC3 延迟）→ 用入站自身的激活键收敛（保持 issue1017 D1 的"入站正常 apply 也推进 register"语义）。
  - 即不是删掉 register 推进，而是把它 **收口到 coordinator 的一条路径、用单一权威 LWW key**，杜绝 D1 旧路径与新 pending 路径并存竞态。

## 5. 被否决的备选（及理由）

- **B（平台层让 Windows 产 LocalFile rep）**：消除不对称看似更根治，但 Windows 剪贴板适配器是 churn 重灾区，改 uri-list→LocalFile 可能影响粘回保真与 shell 元数据 rep，风险/代价更高。→ 改在 application 层（4.1 的 capture 物化）等效且风险可控。
- **接收端按 (from_device, source_entry_id) 去重**：违反 issue1017 §1.2；等于承认 content_hash 不可靠再加第二套身份，留两套真相源。
- **追求 reconstruct 字节级一致**：脆弱，加一种新 representation 就可能再裂。直接"停止重算 + 出生点算对"更稳。
- **只做 ① 不做 ②③**：① 修顺序场景，但本次并发 relay 场景仍漏（早期 find 在物化前）。
- **③ 用 wait-with-timeout 回退**：占住 handler，超时阈值与慢传输两难，退化成并发反吐回省下的下载。→ 选事件驱动单次补偿。
- **partial 升级用"原地改写 rep 字节"**：需新增"替换 rep inline 字节"port，为低频路径加新 API，边界 bug 风险高。→ 选事务级 entry-replace 复用 id。
- **`snapshot_hash` 加 UNIQUE 索引**：与软删除 event 留存冲突、历史脏数据迁移风险；进程内 keyed-lock 已充分（见 4.2）。

## 6. 版本兼容矩阵（F-11）

修复本质是 **sender 侧 capture 算对 + 全链路 compute-once**，但接收端持久化是否采用 wire H（F-4）也参与。混合版本：

| sender | receiver | 结果 |
|---|---|---|
| 新 | 新 | 两通道同 canonical H，inbound 存 wire H → **去重生效**。目标态。 |
| 新 | 旧 | sender 两通道发同 H，但 **旧 receiver 可能仍用本机 materialized 快照重算并存另一个 hash** → 仍可能重复。故"升级 sender 即可"**不成立**。 |
| 旧 | 新 | 旧 sender 仍可能两通道发不同 hash（capture 存 uri-list、dispatch 发 content）→ 新 receiver 也救不回。 |
| 旧 | 旧 | 现状（bug）。 |

**结论**：彻底修复需 **两端都升**。alpha 阶段策略：接受混合版本下偶发重复（不引入 wire 兼容标记/版本协商，保持简单）；发版说明注明"两端升级后生效"。若后续需在混合期消重，再评估接收端兼容修复（旧→新方向：新 receiver 对"同 source、近时刻、不同 hash"做启发式合并）作为独立任务，不进本次。

### 6.1 存量文件 entry 的升级策略（R5-F4）

升级后，新代码要求文件 entry 带 `EntryFileSet` 行级 manifest，但 **升级前落库的旧文件 entry 没有 manifest**。若让它们 serve 时回退"reconstruct + 重 plan"，等于把老 bug 在不可变 H 下又引回来。裁决（取最简且安全）：
- **无 manifest 的旧文件 entry 标记为"不可 serve / 不可 resend"，直到用户重新捕获该内容**（重新复制即走新 capture → 新 manifest → 新 H → 新 entry）。它们在 dashboard 仍可见、可本地 restore（本地 restore 不依赖 manifest，走现有 reconstruct 写 OS 即可），只是不参与跨设备主动外发。
- 不做有风险的批量 backfill（旧 entry 的 blob/路径状态不确定，backfill 可能改变身份）。backfill 作为可选增强留待后续，若做必须保证不改变已存 H 或显式产生新 entry。
- 纯文本/图片等非文件 entry 不受影响（它们的身份不依赖 manifest）。

## 7. 改动落点

- `crates/uc-core/src/clipboard/system.rs:520-561`：修正 `snapshot_hash()`（= 唯一 `content_identity`）的 file-list 贡献为 **结构保留**（R2-F1/R3-F1）；所有本地产生路径走它，不 fork。
- `crates/uc-application/src/clipboard_capture/usecase.rs:189-211`：inline uri-list 文件 rep 经 **已注入的 blob-writer port** 物化 + 填 digest；**capture 时一次性做 size-gate 选集并把"选中文件集 + digest + size + 排除原因"持久化**（R3-F2）；逐项失败规则（4.1.1）。
- `crates/uc-application/src/facade/clipboard_outbound/mod.rs`：dispatch 按 entry_id 读存储 hash 作唯一 wire identity（envelope/header/delivery/register/广播一致），重算仅断言；**用 capture 持久的文件集，不重 plan**（R3-F2）。
- `crates/uc-application/src/usecases/clipboard_sync/active_state/serve_pull.rs:150`：active pull serve 用持久文件集、**尊重 size gate**（不再借 Resend 绕过，R2-F2）；Resend 绕过仅留显式 `ResendEntryUseCase`。
- `crates/uc-application/src/usecases/clipboard_sync/apply_inbound/usecase.rs:194-247,380-389`：inbound 持久化采用 wire H（F-4）；经 `InboundCoordinator` 的 per-H lock 精确分段（F-9/R3-F5）；`find(H)` 排除/恢复软删除（R3-F3）；基于 `is_entry_available` 的 find-or-create + partial 事务级替换（F-2/R2-F5）+ in-flight 标记；删 `recent_source_entries`、`if !is_partial`。
- `crates/uc-application/src/usecases/clipboard_sync/active_state/apply_inbound.rs:316-334`：按 `is_entry_available` 分路 + 延迟登记 `pending_activation`；in-flight 成功消费待激活走 converge tail（R2-F3）。
- 新增组件：**`EntryIdentityCoordinator`**（R3-F5/R5-F3，单 `Arc` 注入 capture + apply_inbound + active-state worker，wiring 在 `crates/uc-bootstrap/src/assembly.rs`；per-H 锁覆盖 **所有** snapshot_hash 写者；含 register 推进的单一 converge tail，R4-F4）；**`EntryFileSet` 行级 manifest reader/builder**（R3-F2/R4-F2/R5-F2，capture 持久、content_identity/dispatch/serve/restore-broadcast 共读）；`is_entry_available(entry_id)`（DB + FS，R2-F4/R3-F4）；事务级 entry-replace port（保留 pinned/active_time/created_at，R2-F5）；`find` 三态契约 `LookupByHash`（R3-F3/R4-F5）；`content_identity` 逐行有序贡献（R4-F1/R5-F2）；存量文件 entry"不可 serve 直到重捕获"策略（R5-F4）；inbound-失败 → active-state 单次补偿事件线（F-8）。
- **不** 新增 UNIQUE 迁移（F-1）。

## 8. 测试

- **应用层单测**：同一 Inline uri-list 文件快照，capture 存储 hash == dispatch wire hash；inbound 持久化存的是 wire H（注入与本机重算不同的 H 也存 wire H）；`apply_inbound` 并发两路只落一个 entry；partial 被后到完整投递事务级替换且 entry_id 不变；派生完整性查询正确区分 complete/partial。
- **canonical hash 碰撞（R2-F1）**：同一组本地文件 + 不同 http/comment/顺序的混合 uri-list → **不同 H**（结构保留生效）。
- **manifest 一致 + 不可变（R2-F2/R4-F3/R5-F1/R5-F2）**：capture 产出行级 manifest（含超 `max_file_size` 记为 excluded）；dispatch/active-pull-serve/Resend **都只发 manifest 的 `file` 行、不发 excluded 行**，**既有 entry 无任何 cap 绕过**；manifest 保留行序/重复/非文件行（不同行序或多/少一行 → 不同 H）。
- **写者全覆盖竞态（R5-F3）**：本地 capture 与 inbound 对同一 H 并发 → 经 `EntryIdentityCoordinator` 同一 per-H 锁串行 → 只落一个 entry。
- **存量升级（R5-F4）**：无 manifest 的旧文件 entry → serve/resend 被拒（不回退 reconstruct+replan）；本地 restore 仍可用；重新捕获产生带 manifest 的新 entry。
- **active-state 单测**：四路分支（complete→converge / partial→不 converge 去补全 / in-flight→延迟 / none→pull）；**in-flight 成功用 `pending_activation` 的 LWW key 走 converge tail**（R2-F3）；`pending_activation` 单次补偿 + 各清除条件（in-flight 消费/补偿成功/过期/闸门拒/锁定/再失败）不循环。
- **SQLite 级集成测试（F-12 / R2-F5）**：事务级 entry-replace 的级联删除/重建（event/selection/representation/thumbnail/delivery/transfer 不留孤儿）+ **保留 pinned/active_time/created_at/register 指针**；并发同 hash 提交；软删除 event 留存场景。
- **软删除可见性（R3-F3）**：删除某 entry 后重收同 H → 事务内恢复可见 → dashboard 可见（断言不是只 touch 隐藏项）。
- **时间漂移（R3-F2）**：capture 选集后改 `max_file_size` / 删一个被选文件 → dispatch 与 active pull serve 仍按 capture **持久文件集** 发送，集合与 H 一致。
- **可用性（R3-F4）**：file entry 的本地 cache 文件被删/变目录 → `is_entry_available` 为假 → active-state **不 converge**、不写坏内容到 OS。
- **协调器单例（R3-F5）**：apply_inbound 与 active-state worker 注入同一 `InboundCoordinator` 实例；两通道对同一 H 的 in-flight 标记互相可见、补偿事件可达。
- **单一身份入口（R3-F1）**：结构保留的 `content_identity` 对混合 uri-list 不碰撞；本地各产生路径产出同一 H。
- **e2e（`tests/e2e`）**：双节点 dispatch + restore 同步一个文件，dashboard 仅一条；接收端中途取消（partial）后重试 → 同一卡片补全为完整；partial 期间 active-state 不写 OS 剪贴板。
- **capture 文件规则单测（4.1.1）**：非 file scheme / 不存在 / 目录 / 部分失败 → digest 集与 dispatch 集合一致。

## 9. 验收（回放事故）

两通道同 canonical H、inbound 存 wire H → pull 见 dispatch in-flight（或 partial-held）→ 不并发拉/不误 converge；dispatch 重启失败 → 单次补偿 pull 补全；全程经 per-H 锁 + 同 entry_id 事务级替换 → dashboard 自始至终一条卡片，从 partial 自动变完整。重复 + 幽灵 partial 一并消失。

---

## 附：请审查者重点攻击的弱点

1. **inbound 存 wire H 的覆盖面（F-4）**：apply_inbound 是否存在某条持久化分支仍会重算 hash（如经 capture pipeline 的 `snapshot_hash()`）？mobile 入站、resend 入站等旁路是否都接住？
2. **事务级 entry-replace（F-2）**：级联删除/重建的顺序、外键约束、回滚；replace 期间 register/active_time/pinned/selection 的保留；与 ③ 待激活、resurface 的交互；并发下二次 find 与 replace 的原子边界。
3. **完整性持久化（F-3/F-7）**：`is_complete` 何时写/更新（partial→complete 升级时、rep 异步物化完成时）；active-state converge 与 dedup 是否都改用它；rep-bound blob（图片）与 free-file 的完整性判定差异。
4. **keyed-lock 出锁下载再进锁（F-9）**：二次 find 与提交之间是否仍有窗口；in-flight 标记在锁外下载期间对第三方通道的可见性；是否真的不会重复建 entry。
5. **单次补偿的边界（F-8）**：`pending_activation` 被新 activation 覆盖后旧补偿是否正确放弃；补偿 pull 与正常 0xC3/resync 同时触发的去重；compensation 是否可能与 VISION「禁止自动重发」擦边。
6. **共享文件集合 helper（F-6）**：capture 与 dispatch 真能共用同一排除规则吗（dispatch 侧有 `max_file_size`/planner，capture 侧是否需要同样的 size gate 才能集合一致）？size gate 不一致会不会再次让两端 digest 集分叉。
7. **与 issue1017 LWW 不变量**：canonical H 取值变化是否影响 register 的 `(content_hash, activated_at_ms, activated_by)` 跨设备可比性 / 断环判定。
