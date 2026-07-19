# T3 / #1329 实现方案 · 接收端 attempt 状态机 + entry 级取消 + 聚合进度（端到端）

Status: draft（待确认 → 编码）
Issue: #1329（T3，P1）· Map: #1325
上游设计（定稿，10 轮对抗审查）：`.planning/2026-07-12-adr010-directory-sync-phase5-plan.md` §5.1
硬前置（已合并）：#1328 / PR #1382「原子 no-replace 目录发布」——本分支 HEAD `16606fe43`
基线：本文所有代码锚点相对本分支当前树。

> 本文是把 §5.1 的决策落到「表结构 / 端口签名 / CAS 状态迁移矩阵 / 事务边界 / 提交切分 / 测试清单」
> 的实现级方案。§5.1 已把「做什么、为什么」拍死；本文只回答「怎么落地」。

---

## 0. 范围核对（勘探结论）

三份独立勘探（发布器/物化、持久化/取消、前端）交叉确认：

**已有、可复用**
- 原子发布器地基（#1382）：`AtomicPublishPort`（`crates/uc-core/src/ports/atomic_publish.rs`）+
  `FsAtomicPublisher`（三平台 no-replace，`crates/uc-infra/src/fs/atomic_publish.rs`）+
  内存态 `DirectoryPublication`（commit/rollback/`RollbackOutcome`，`materializer.rs:296`）+
  `.uniclip-incoming-*` 隐藏暂存 + 启动前缀清扫（`materializer::sweep_inbound_staging`）。
- `file_transfer` 表（PK=`transfer_id`，按 `entry_id` 分组索引），成员 id
  `{entry}:member:{i}`（`materializer.rs:1466`），`file_size` 列已在（可空）。
- `FindEntryIdForTransferPort`（transfer_id→entry_id，`ports/file_transfer.rs:151`）。
- 事件链路：`FileTransferEvent`（`core/file_transfer/event.rs:78`）→
  `FileTransferHostEventPublisher`（解析 entry_id）→ `HostEvent::Transfer` →
  `DaemonApiEventEmitter` → WS `file-transfer.progress` / `file-transfer.status_changed`
  → 前端 `clipboardEventReducer` / `useTransferProgress`。
- 单成员取消：Tauri `activity_hud/actions.rs:78` → `POST /clipboard/cancel-transfer/{id}`
  （`webserver/api/clipboard.rs:585`）→ `blob_transfer::facade::cancel_inbound_transfer`
  （token cancel + QUIC teardown + `Cancelled` 事件，`facade.rs:478`）。
- AEAD 派生范式：`DeriveSpaceSubkeyPort::derive_subkey(salt=profile, info=用途标签)`
  （`core/ports/space/access.rs:285`）；`entry_file_set` 用独立 `FILE_SET_KEY_INFO`
  标签 + `EntryFileSetPathCipher`（`seal_*`/`open_*`，AAD=`(entry_id,line_index)`）。

**Greenfield（本票新建，勘探三方独立确认「今天不存在」）**
- 持久 attempt 权威（`current_attempt_id` + `attempt_state`）、`attempt_id` 概念。
- 加密发布日志（AEAD：attempt_id / 阶段 / root 暂存→最终映射 / landed 标记）。
- 「落库标记与接收记录同事务」——现状 file-set manifest 保存是 **事务外 best-effort**
  且 `publication.commit()` 在 `capture` 返回之后（`clipboard_capture/usecase.rs:635` FIXME、
  `apply_inbound/usecase.rs:746`）。这是本票 **结构性改造最重、风险最高** 的一块。
- 聚合 Σ 字节 / 文件计数——现有 `EntryTransferSummary` 只是 **状态 roll-up**，无字节/计数
  （`ports/file_transfer.rs:79`、`compute_aggregate_status:200`）。
- entry 级取消入口（各层全是单 transfer_id）。
- 启动收敛认识 attempt——现有 `reconcile_on_startup`（`bootstrap/subsystem/file_transfer.rs:213`）
  只 bulk-fail 传输投影行，不识 attempt。
- 前端聚合：`entryTransferMap[entryId]=transferId`（单尝试）、卡片渲染单 transfer、
  取消按钮只在预览面板不在卡片、entry DTO 无目录标志。

**回归红线**：单文件 / 平铺多文件接收行为 **不变**（attempt 权威只覆盖含目录结构的接收 entry）。

---

## 1. attempt 状态机（领域权威）

### 1.1 状态

`AttemptState`（uc-core 领域枚举，稳定字符串编码）：

| 状态 | 含义 | 终态 | 可 begin-retry |
| --- | --- | --- | --- |
| `Active` | 成员下载 / 完整性校验中 | 否 | 是（未认领提升） |
| `Promoting` | 已认领提升（身份已校验，正在 no-replace 发布 root） | 否 | 否（返回 no-op） |
| `Published` | root 全部发布 + 接收记录落库 | 是（成功） | —（已完成） |
| `Failed` | 校验/发布/恢复失败 | 是 | 是（可重发） |
| `Cancelled` | 用户 entry 级取消 | 是 | 是（可重发） |

**整体状态以状态机为唯一权威**（§5.1 round 8）：成员行只喂进度计算，**禁止** 由「全部成员
completed」直接推导整体完成——成员下载完成早于校验/发布。UI 状态序列：
`Active`（传输中）→ `Promoting`（校验/发布中，仍显示进行中）→ `Published`（完成）。

### 1.2 CAS 迁移矩阵

所有迁移 = 单行条件 UPDATE，守卫 `(entry_id, current_attempt_id, attempt_state)` 三元组
（乐观并发：动作先读当前 `(attempt_id, state)`，再以其为守卫 CAS，按 rows-affected 判输赢）。

| 动作 | 守卫（前态） | 目标态 | CAS 失败（rows=0）语义 |
| --- | --- | --- | --- |
| **begin**（首次接收） | 无行 → INSERT | `Active` | 已有行 → 见 begin-retry |
| **begin-retry** | `current_attempt_id=<观测>` 且 state∈{`Failed`,`Cancelled`,`Active`} | 新 `attempt_id` + `Active` | 前态是 `Promoting`/`Published` → no-op（「提升中，无法重试」） |
| **publish-claim** | `current_attempt_id=<本尝试>` 且 `Active` | `Promoting` | ① `current_attempt_id≠本尝试` → 本尝试已被重试替换，弃 staging 不发布；② 同尝试 `Cancelled` → 尊重取消，弃 staging 不发布 |
| **finalize-publish** | `current_attempt_id=<本尝试>` 且 `Promoting` | `Published` | 只有认领者到达此态；**与接收记录同事务**（§4） |
| **cancel**(target_attempt) | `current_attempt_id=<target>` 且 `Active` | `Cancelled` | ① 前态 `Promoting` → 「正在提升/已完成，无法取消」；② `current≠target` → 迟到取消，已被新尝试替换，忽略（**迟到取消不杀新尝试**） |
| **fail**(attempt) | `current_attempt_id=<本尝试>` 且 state∈{`Active`,`Promoting`} | `Failed` | 已被替换/取消 → 忽略 |

关键不变量（对应验收）：
- publish-claim 与 cancel 都对 `Active` 认领，**谁先谁生效**，输家明确 no-op → 「取消成功则目录
  必不出现；提升已认领则取消返回已完成」。
- publish-claim 守卫本尝试 id，天然区分「被重试替换」与「被取消」两种 no-op → 「旧 attempt 存活时
  重试→旧完成不提升」「迟到取消不杀新尝试」。

### 1.3 取消检查贯穿全程（§5.1 round 4）

物化流程在 **三个检查点** 查 attempt 是否已 `Cancelled`（经 `GetEntryAttemptPort`）：
① 成员边界（每个成员 fetch 前/后）② 全部成员完成后、完整性校验前 ③ 原子提升前（即 publish-claim
CAS，本身就是原子裁决点）。任一处发现 `Cancelled` → 弃 staging、剩余成员标 `Cancelled(LocalUser)`、
整体 `Cancelled`，**禁落 Failed**。

---

## 2. 持久化设计（3 个迁移）

迁移目录 `crates/uc-infra/migrations/YYYY-MM-DD-NNNNNN_<name>/{up,down}.sql`，embed 后重生成
`schema.rs`。命名前缀按日期 + 序列，字典序排序。

### 2.1 迁移 A：`entry_receive_attempt`（attempt 权威表）

```sql
CREATE TABLE entry_receive_attempt (
    entry_id           TEXT    NOT NULL PRIMARY KEY,  -- 接收方 entry_id，一行一目录接收
    current_attempt_id TEXT    NOT NULL,              -- 当前尝试；重试时切换
    attempt_state      TEXT    NOT NULL,              -- active|promoting|published|failed|cancelled
    updated_at_ms      BIGINT  NOT NULL
);
```

只对 **含目录结构** 的接收 entry 建行。单文件/平铺不建（回归不变）。所有列均 **非用户内容**
（entry_id/attempt_id 是内部标识，state 是枚举），无需加密。

### 2.2 迁移 B：`file_transfer` 加 `attempt_id` 列（加性）

```sql
ALTER TABLE file_transfer ADD COLUMN attempt_id TEXT;   -- 可空；仅目录成员行填充
CREATE INDEX idx_file_transfer_entry_attempt ON file_transfer(entry_id, attempt_id);
```

- 成员 transfer_id 改为 `{entry}:member:{i}:attempt:{n}`（`directory_member_transfer_id`
  加 attempt 段）——保证重试的成员行与旧尝试成员行 **主键不撞**（旧行保留为历史）。
- `attempt_id` 列用于 **高效过滤当前尝试**（避免按 id 前缀 `LIKE` 解析）。单文件/平铺行 `attempt_id=NULL`，
  沿用既有 `EntryTransferSummary` 路径，回归不变。
- 成员 `file_size` 在 seed 时按清单成员大小填入（聚合 Σ 的数据源）。

### 2.3 迁移 C：`directory_publish_log`（加密发布日志）

```sql
CREATE TABLE directory_publish_log (
    entry_id            TEXT    NOT NULL,
    attempt_id          TEXT    NOT NULL,
    phase               TEXT    NOT NULL,     -- staging|publishing|landed（明文，非敏感阶段标记）
    root_map_ciphertext BLOB,                 -- AEAD(Vec<(staged_path, final_path)>)；root 名/路径是用户内容
    partial_publication INTEGER NOT NULL DEFAULT 0,  -- 明文非敏感标记
    partial_root_count  INTEGER NOT NULL DEFAULT 0,  -- 明文非敏感计数（回滚失败时的已可见 root 数）
    landed              INTEGER NOT NULL DEFAULT 0,  -- 加速索引；权威仍是接收记录是否存在
    updated_at_ms       BIGINT  NOT NULL,
    PRIMARY KEY (entry_id, attempt_id)
);
```

加密口径（§5.1 round 9 CRITICAL：root 名是用户内容，明文侧不得出现路径/名称）：
- `root_map_ciphertext` = AEAD 密封 `Vec<(staged_path, final_path)>`。密钥沿 §3.1 同派生链路：
  `DeriveSpaceSubkeyPort` + 独立 info 标签（如 `directory-publish-log-v1`，与 `FILE_SET_KEY_INFO`
  不同名，密钥永不复用），AAD 绑 `(entry_id, attempt_id)`。复用 `EntryFileSetPathCipher` 同款
  seal/open 封装（新增一个 `PublishLogCipher` 兄弟类型，或泛化现有 cipher）。
- 明文列只留非敏感标记：`phase` / `partial_publication` / `partial_root_count` / `landed`。
- 会话锁定时无法解密映射——恢复裁决 **优先看接收记录是否存在**（权威），映射只在需要回滚时读；
  锁定态下的回滚推迟到解锁（与 `entry_file_set` 锁定语义一致）。

---

## 3. 端口清单（uc-core）

遵循 `docs/architecture/ports.md` §12.2「一个内层 Store，拆多个小意图 port」与 uc-core AGENTS §5.4
「port 文档只描述领域契约，禁止引用上层/HTTP/协议/具体场景」。新增文件
`crates/uc-core/src/ports/entry_receive_attempt.rs` 与 `directory_publish_log.rs`。

### 3.1 attempt 权威 —— 查询 + 命令拆分

```rust
// 领域值对象
pub struct EntryReceiveAttempt { pub entry_id: String, pub current_attempt_id: String,
                                 pub state: AttemptState, pub updated_at_ms: i64 }
pub enum AttemptState { Active, Promoting, Published, Failed, Cancelled }  // Display+FromStr 单一权威
pub enum AttemptError { Backend(String) }

// Query：读当前 attempt（取消检查点、恢复裁决、聚合投影都用它）
pub trait GetEntryAttemptPort {
    async fn get_entry_attempt(&self, entry_id: &str)
        -> Result<Option<EntryReceiveAttempt>, AttemptError>;
}

// Query：列非终态 attempt（启动收敛用）
pub trait ListNonTerminalAttemptsPort {
    async fn list_non_terminal_attempts(&self) -> Result<Vec<EntryReceiveAttempt>, AttemptError>;
}

// Command：begin / begin-retry（返回是否认领成功 + 新 attempt_id）
pub trait BeginEntryAttemptPort {
    async fn begin_attempt(&self, entry_id: &str, attempt_id: &str, now_ms: i64)
        -> Result<(), AttemptError>;                       // 首次；INSERT
    async fn begin_retry_attempt(&self, entry_id: &str, expected_current: &str,
        new_attempt_id: &str, now_ms: i64) -> Result<bool, AttemptError>;  // CAS，见矩阵
}

// Command：三方裁决（CAS，返回是否认领成功）
pub trait AdvanceEntryAttemptPort {
    async fn claim_promotion(&self, entry_id: &str, attempt_id: &str, now_ms: i64)
        -> Result<bool, AttemptError>;                     // Active→Promoting
    async fn cancel_attempt(&self, entry_id: &str, attempt_id: &str, now_ms: i64)
        -> Result<AttemptCancelOutcome, AttemptError>;     // Active→Cancelled；输给 Promoting/替换有区分
    async fn fail_attempt(&self, entry_id: &str, attempt_id: &str, now_ms: i64)
        -> Result<bool, AttemptError>;
}
pub enum AttemptCancelOutcome { Cancelled, AlreadyPromotingOrPublished, SupersededByNewer }
```

`finalize-publish`（Promoting→Published）**不在此端口**——它必须与接收记录同事务，归入 §4 的组合事务端口。

### 3.2 发布日志 —— 查询 + 命令

```rust
pub struct DirectoryPublishRecord { pub entry_id: String, pub attempt_id: String,
    pub phase: PublishPhase, pub root_map: Vec<(PathBuf, PathBuf)>,  // 解密后
    pub partial: Option<u32>, pub landed: bool }
pub enum PublishPhase { Staging, Publishing, Landed }

pub trait RecordDirectoryPublishPort {   // Command：写阶段 + 映射（加密在 infra 内）
    async fn record_phase(&self, entry_id: &str, attempt_id: &str, phase: PublishPhase,
        root_map: &[(PathBuf, PathBuf)], now_ms: i64) -> Result<(), PublishLogError>;
    async fn record_partial(&self, entry_id: &str, attempt_id: &str, visible_roots: u32,
        now_ms: i64) -> Result<(), PublishLogError>;
}
pub trait GetDirectoryPublishRecordPort {  // Query：恢复裁决读映射
    async fn get_publish_record(&self, entry_id: &str, attempt_id: &str)
        -> Result<Option<DirectoryPublishRecord>, PublishLogError>;
}
```

### 3.3 聚合进度投影 —— 查询

```rust
pub struct DirectoryReceiveProgress {
    pub entry_id: String,
    pub attempt_id: String,
    pub state: AttemptState,          // 整体权威态
    pub total_bytes: i64,             // Σ 当前尝试成员 file_size
    pub completed_bytes: i64,         // Σ 已完成成员 file_size（不含飞行中部分字节）
    pub members_total: u32,
    pub members_completed: u32,
}
pub trait GetDirectoryReceiveProgressPort {   // 新端口，不污染现有 EntryTransferSummary
    async fn get_directory_receive_progress(&self, entry_id: &str)
        -> Result<Option<DirectoryReceiveProgress>, FileTransferProjectionError>;
}
```

实现：join `entry_receive_attempt`（取 current_attempt_id + state）与 `file_transfer`
（`WHERE entry_id=? AND attempt_id=<current>`），聚合 Σ/计数。**不持久化流式字节**——飞行中成员的
部分字节由前端从实时 progress 事件叠加，下一条事件即自愈（§5.1 round 6）。

### 3.4 组合事务端口 —— 命令（§4 核心）

```rust
// 一次 Diesel 事务内：插 entry+selection+file_set manifest + attempt→Published + publish_log.landed=1
pub trait CommitDirectoryReceiveRecordPort {
    async fn commit_directory_receive(&self, record: &DirectoryReceiveCommit)
        -> Result<(), DirectoryReceiveCommitError>;
}
```

见 §4。

> 编译器强制：新端口经 `wire_dependencies`（`bootstrap/wiring/wire.rs`）把同一个
> `DieselEntryReceiveAttemptRepository` / 发布日志 repo / 投影 repo 注入各小 port（§8.3 一 adapter 多 port）。

---

## 4. 事务边界（最高风险块）

**要求**（§5.1 round 10）：landed 标记与接收记录 **同事务** 提交；恢复裁决以「接收记录是否存在」为准，
标记只是加速索引。补「写接收记录与标记之间崩溃」定点测试。

**现状**（勘探）：`clipboard_capture/usecase.rs` 里
`save_entry.save_entry_and_selection`（Create，:608）/ `replace_entry.replace_entry_content`（Replace）
各自开事务；file-set manifest 是 **事务外 best-effort**（:642，FIXME:635）；`publication.commit()`
在 `apply_inbound/usecase.rs:746` 更晚。三步分离，间隙崩溃会让恢复误判。

**方案**：目录接收的落地走 **单一组合事务端口** `CommitDirectoryReceiveRecordPort`（§3.4）。
一次 `conn.transaction(|| { … })` 内原子写入：
1. entry + selection（Create/Replace 复用现有 **私有插入 helper**，不复制 SQL——单一真相源）；
2. entry_file_set manifest（同事务，顺带修掉 :635 FIXME 对目录路径的不一致）；
3. `entry_receive_attempt` → `Published`（finalize-publish CAS，守卫 Promoting）；
4. `directory_publish_log.landed = 1` + `phase = landed`。

事务提交成功后，接收编排再 `publication.commit()`（丢弃 staging——staging 丢弃非事务性、幂等，
崩溃残留由启动清扫兜底）。

**为何是组合端口而非拆开**：round 10 明确「两者分开写，间隙崩溃会让恢复误回滚已成功保存的结果」。
entry/selection/file_set/attempt/publish_log 同在 SQLite 主库，一个事务即可跨表原子。
组合端口只是 **事务包一层**，SQL 逻辑仍收敛在既有私有 helper，无并存新旧逻辑。

**恢复语义**：即便 3/4 与 1/2 之间还想再细分，权威永远是「entry 接收记录是否存在」；标记不同步时由
定点测试守护。锁定态无法解密映射时，回滚推迟到解锁。

> 风险标注：这是唯一触碰共享捕获持久化路径的改动。编码时先给组合端口写 infra 契约测试
> （含「插入接收记录后、标记前注入 panic/rollback」的定点用例），再接编排。

---

## 5. 应用层编排（uc-application，经 `ClipboardSyncFacade` 暴露）

### 5.1 物化流程改造（`usecases/clipboard_sync/apply_inbound/materializer.rs` + `usecase.rs`）

- 目录接收入口分配 `attempt_id`，调 `begin_attempt`；成员 transfer_id 带 attempt 段；成员行 seed 时
  写 `attempt_id` + `file_size`。
- 成员 fetch 循环：区分 cancel 与真失败（复用 `is_cancel_error`，:42），并在 **三检查点** 查
  `GetEntryAttemptPort`；命中 `Cancelled` → 走取消收尾（弃 staging、剩余成员 `Cancelled(LocalUser)`、
  整体 Cancelled）。真失败 → `fail_attempt` + 剩余成员 Failed（现状行为收敛到「非取消」分支）。
- 完整性校验通过后 → `RecordDirectoryPublishPort::record_phase(Publishing, root_map)` →
  `claim_promotion`（CAS）：认领成功才 `publish_root` 逐 root 发布；认领失败按矩阵 no-op（弃 staging）。
- 发布成功 → 组合事务端口落地（§4）→ `publication.commit()`。
- 发布/校验/落库任一失败 → `publication.rollback()`（复用 #1382 `RollbackOutcome`）+ `fail_attempt`；
  回滚失败 → `record_partial(visible_roots)` + Failed。

### 5.2 entry 级取消用例（新）

`CancelEntryReceiveUseCase::execute(entry_id) -> CancelEntryReceiveOutcome`
（`{ Cancelled, AlreadyCompleted, InFlightPublication, NotFound }`），经 `ClipboardSyncFacade` 转发：
1. `get_entry_attempt`；无 → NotFound；`Published` → AlreadyCompleted；已 `Cancelled/Failed` → 幂等。
2. `cancel_attempt`（CAS）：`AlreadyPromotingOrPublished` → InFlightPublication/AlreadyCompleted；
   `SupersededByNewer` → 取消旧的、不动新的。
3. 认领成功 → 逐当前尝试 **在飞成员** 调既有单成员 abort（`blob_transfer::facade` 的 token+QUIC
   teardown，reason `LocalUser`）；**未开始成员** 行标 `Cancelled(LocalUser)`，**禁 Failed**。
4. 物化任务在检查点感知 Cancelled 后弃 staging；整体 `Cancelled`。

> 复用现有单成员 abort，但 **不暴露单成员取消**（§5.1 D7）——entry 级取消内部循环成员，对外只有一个入口。

### 5.3 重试用例（新 / 并入 resend 路径）

`begin_retry_attempt`（CAS）成功 → 中止旧尝试在飞 fetch、装入新 attempt_id 重新走物化；失败（Promoting）
→ 返回「提升中，无法重试」。旧尝试成员行保留为历史，聚合按 attempt_id 只看当前尝试。

### 5.4 启动收敛（新 facade 方法，`bootstrap/startup` 调用，与 `sweep_orphaned_inbound_staging` 同级）

`reconcile_directory_attempts_on_startup()`：对 `list_non_terminal_attempts` 的每条（Active/Promoting）：
- 接收记录存在（当前 attempt）→ 收终态 `Published`、补 `publication.commit()` 收尾（若 staging 仍在）。
- 不存在 → 读 `get_publish_record` 的 root 映射，用 `AtomicPublishPort` 回滚已发布 root（final→staging）、
  `fail_attempt`（Failed 可重发）、成员行 Failed、弃 staging。锁定态无法解密 → 推迟到解锁重跑。

现有 `reconcile_on_startup`（`bootstrap/subsystem/file_transfer.rs:213`）保持只管 per-transfer 投影行；
attempt 收敛是 **新的一步**，不混入（避免语义耦合）。

---

## 6. webserver / daemon 契约（`uc-webserver` + `uc-daemon-contract` + `uc-daemon-client` + Tauri）

- 新端点 `POST /clipboard/cancel-entry-receive/{entry_id}`，body `{ reason: "local_user" }`，
  返回 `{ outcome: "cancelled" | "already_completed" | "in_flight_publication" | "not_found" }`。
  handler 调 `ClipboardSyncFacade::cancel_entry_receive`。OpenAPI operation_id `cancelEntryReceive`。
- 列表投影 DTO（`list_entry_projections` → `EntryProjectionResponseDto`）为目录接收 entry 增可选
  `directoryReceive` 聚合对象（attemptId/state/totalBytes/completedBytes/membersTotal/membersCompleted）。
  其 **存在** 即前端「按聚合渲染」信号（现状 entry DTO 无目录标志）。
- 事件负载 `FileTransferProgressPayload` / `FileTransferStatusChangedPayload` 增 `attemptId`
  （`daemon-contract/api/types.rs:127` 一带 + `event_emitter.rs`），前端据此丢弃旧尝试事件。
- Tauri 命令 + `uc-daemon-client` 方法 + 前端 `src/api` 包装，镜像既有 `cancel-transfer` 链路。

---

## 7. 前端（`src/`）

- `fileTransferSlice`：`entryTransferMap[entryId]=transferId`（单）→ 支持目录聚合。保留
  `activeTransfers`（按 transferId 存成员行），新增 `entryCurrentAttempt: Record<entryId, attemptId>`；
  聚合选择器 `selectDirectoryAggregate(entryId)`：按 `entryId + 当前 attemptId` 过滤成员，Σ
  bytesTransferred / totalBytes + 完成计数，整体态取自 attempt state（投影字段/status 事件），
  **非**「成员全 completed」。旧尝试事件（attemptId 不符）丢弃。
- 初始查询 hydrate：从投影 `directoryReceive` 重建基线（completedBytes = 已完成成员大小和、计数照实），
  飞行中成员从 0 显示、随实时事件自愈（§5.1 round 6）。
- `HistoryCardTransferProgress`：目录 entry 渲染聚合百分比（Σdone/Σtotal）+「n/m 个文件」；
  单文件 entry 不变。
- 卡片新增 entry 级取消按钮（接收方 + 进行中）→ 调 `cancelEntryReceive(entryId)` 新 API。
  现状取消按钮只在预览面板（`ClipboardPreview`/`TransferProgressBar`），本票在 **卡片** 补目录取消。
- 单文件路径（无 attemptId / 无 directoryReceive）完全走既有单 transfer 渲染与取消，回归不变。

---

## 8. 提交切分（原子，按层；对应计划 PR-3 后端 + PR-6 前端一部分）

顺序满足 hex commit boundary（core 与 infra 不同提交；port 与 adapter 不同提交）：

1. `arch:` attempt 权威 + 发布日志 + 聚合投影 + 组合事务 + entry 取消端口（uc-core，纯 trait/值对象）
2. `impl:` 迁移 A/B/C + `entry_receive_attempt` repo（CAS）+ 发布日志 repo（AEAD）+ 聚合投影查询
   + 组合事务端口 infra 实现（uc-infra）
3. `feat:` 物化流程接 attempt/CAS/检查点/发布日志/组合落地（uc-application）
4. `feat:` entry 级取消用例 + 重试用例 + facade 暴露（uc-application）
5. `feat:` 启动 attempt 收敛（uc-application facade + bootstrap 接线）
6. `feat:` webserver 端点 + DTO + 事件带 attemptId + daemon-client + Tauri 命令
7. `feat:` 前端聚合选择器 + 卡片聚合渲染 + entry 取消按钮 + hydrate 重建
8. 每个行为提交配套 `test:`（见 §9）

（若单提交过大，2 可再按「表/repo」拆，3 可按「检查点/发布/落地」拆。）

---

## 9. 测试清单（映射验收项）

**重发六组**（§5.1）：取消后重发 / 失败后重发 / 旧尝试事件晚到不污染 / 旧失败存在但新尝试成功显示完成 /
旧 attempt 存活时重试旧完成不提升 / 取消与重试竞态迟到取消不杀新尝试。

**崩溃恢复五组**：发布前中断 / 多 root 发布一半中断 / 全部发布但校验前中断 / 落库前中断 / Active 态中断；
外加「写接收记录与标记之间崩溃」**定点**（§4，infra 契约测试注入 panic/rollback）。

**取消/提升 CAS 竞态定点**：在终检与 rename（publish-claim）之间注入暂停，断言认领确定性。

**聚合与整体态**：多成员进度合成、成员失败/取消对聚合行影响、「成员全完成但发布失败 → 整体 Failed 而非完成」、
重连/重载后聚合从成员终态 + 已知大小恢复正确、成员传输中途重连从 0 显示随事件自愈。

**回归**：单文件 / 平铺多文件接收与取消行为不变；`root 名/路径不入明文 DB 字段与日志`断言（发布日志密文、
明文列只标记/计数）。

**加密**：发布日志映射密文往返、锁定态无法解密时恢复以接收记录为权威。

层次：uc-core 领域/CAS 矩阵单测；uc-infra repo 契约测试（真实 SQLite，含损坏/半写）；uc-application
用例/编排测试（port mock）；前端 vitest 选择器/reducer/卡片测试；可选 `tests/e2e` 端到端。

---

## 10. 风险与开放问题

1. **组合事务端口（§4）是唯一触碰共享捕获持久化路径的改动**——先写 infra 契约测试再接编排；确认
   Create/Replace 两路都能在组合事务内复用现有私有插入 helper（不复制 SQL）。
2. **物化流程 `materializer.rs` 体量大**（~1.7k 行）——attempt/检查点接入按最小侵入，取消/发布决策
   收敛在单一裁决点，避免扩散式 if。
3. **begin-retry 并发**：CAS 守卫观测到的 `current_attempt_id`，两次重试串行化（后者胜、中止前者），
   确定性；文档化。
4. **锁定态恢复**：无法解密 root 映射时回滚推迟到解锁——需确认启动收敛可重入（解锁后重跑），
   与 §3.1 回填的「解锁后幂等」范式一致。
5. **前端目录标志来源**：本票用投影 `directoryReceive` 字段的存在性驱动聚合渲染；T5（sync-ineligible
   徽标）另有目录感知需求，届时若需显式 `isDirectory` 再统一，不在本票提前引入。

## 11. 明确不做（Out of scope，随 §5.1 §12）

- 发送端 per-member 出站建模（T4 只去假百分比，不在本票）。
- 单成员取消 UI（D7）。
- sync-ineligible 徽标（T5）、v3 拒绝记忆（T6）、settings 界面（T8）、用户文档（T9）。
- 移动端目录参与（ADR 范围外）。
