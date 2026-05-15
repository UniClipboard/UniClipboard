# Task Plan: 097 · Entry Delivery Status 可视化

## 目标

让用户在打开某条 entry 的 detail 视图时，看到这条内容来自哪台设备、已经同步到了哪些配对设备 (每台设备的具体状态)、失败的设备及原因。后端落实"对端 adapter 接收"语义的投递记录，前端在 quick-panel 和主窗口两个 detail 视图中完整展示。

## 当前阶段

Phase 1 已完成 (2026-05-14)。下一阶段 Phase 2(前端 detail UI)。

## 关键非目标 (本期不做)

- 不改 wire protocol、不加 application-level ack。语义停留在"对端 adapter 已接收"(`DispatchAck::Accepted` / `DuplicateIgnored`)。
- 不覆盖 mobile (iOS Shortcut) 设备。delivery 表只记录 desktop trusted peers,UI 上不显示 mobile。
- **列表 UI 不展示同步信息**。列表的 `EntryProjectionResponseDto` 不增加来源 / 同步状态字段;不做汇总徽章;不做批量 summary API。所有展示集中到 entry detail。
- 不提供 UI 上的"重新同步"按钮。失败只展示，不重试。
- 不做出/入站方向的统一追踪。本机视角下，delivery 表只记录"本机作为发送方"对每个对端的投递结果。

## 已对齐的需求

1. **语义**:复用现有 `DispatchAck::Accepted` / `DuplicateIgnored`,语义为"对端节点已接收 bytes"。
2. **入口**:仅在 entry detail 视图中展示 (quick-panel 的 `ClipboardPreviewPane` + 主窗口的 `ClipboardPreview`),列表保持不变。
3. **粒度**:两套 detail UI 都完整铺开"来源 + 每台设备的同步状态"。
4. **失败展示**:可见，但不提供操作按钮。
5. **范围**:仅 desktop peer。Mobile 不在 UI 里出现。

## 关键代码锚点

- Entry 实体：`src-tauri/crates/uc-core/src/clipboard/entry.rs:4-48`
- 变更来源：`src-tauri/crates/uc-core/src/clipboard/change.rs:4-14`
- Dispatch port:`src-tauri/crates/uc-core/src/ports/clipboard/sync_dispatch.rs`
- 出站广播：`src-tauri/crates/uc-application/src/usecases/clipboard_sync/dispatch_entry.rs`
- 入站 ingest:`src-tauri/crates/uc-application/src/usecases/clipboard_sync/ingest_inbound.rs`
- TrustedPeer:`src-tauri/crates/uc-core/src/trusted_peer/peer.rs:14-19`
- Clipboard header(已带 origin_device_name):`src-tauri/crates/uc-core/src/ports/clipboard/sync_dispatch.rs:32-52`
- Clipboard event 表 (已带 source_device):`src-tauri/crates/uc-infra/migrations/2026-01-09-141527_clipboard_core/up.sql`
- Entry DTO:`src-tauri/crates/uc-daemon-contract/src/api/dto/clipboard.rs:15-48`
- 前端 quick-panel detail:`src/quick-panel/ClipboardPreviewPane.tsx`(94 行)
- 前端主窗口 detail:`src/components/clipboard/ClipboardPreview.tsx` + `ClipboardPreviewInfo.tsx`
- 前端 preview 数据 hook:`src/quick-panel/useClipboardPreview.ts`
- 前端列表：`src/quick-panel/components/HistoryPane.tsx`(本期不动)
- 前端单项：`src/quick-panel/components/PanelItem.tsx`(本期不动)

## 架构约束 (必读)

- `uc-core` 边界严格 (`src-tauri/crates/uc-core/AGENTS.md`):port 注释只描述领域契约;不依赖 SQL/HTTP/tokio。
- `uc-application` 对外仅通过 `src/facade/`(`src-tauri/crates/uc-application/AGENTS.md` §11.4):新 use case 必须经 `ClipboardFacade`(或对应域 facade) 透出，业务子模块保持 `pub(crate)`。
- GUI 走 in-process facade，不走 HTTP webserver(用户记忆 `project_gui_uses_inprocess_facade.md`)。
- 文档/注释全中文;新文件 `//!` 头条段先讲"为什么需要这个模块"。
- 不留并行新旧代码，无清理计划的"过渡"不接受。

---

## Phases

### Phase 1: 后端最小可用 — 表 + dispatch 落盘 + facade(单条 view)

**目的**:打通"出站 dispatch → 写 delivery 行 → 通过 facade 拿到完整 detail view"的端到端最短路径。前端暂不联调，通过单元/集成测试验证。

**改动文件清单**:

新增：
- `src-tauri/crates/uc-core/src/clipboard/delivery.rs` — 领域类型 (`EntryDeliveryStatus`、`DeliveryFailureReason`、`EntryDeliveryRecord`、`EntryDeliveryError`)
- `src-tauri/crates/uc-core/src/ports/clipboard/delivery.rs` — `EntryDeliveryRepositoryPort`
- `src-tauri/crates/uc-infra/migrations/<YYYY-MM-DD-HHMMSS>_entry_delivery/up.sql` + `down.sql` — 新表 + 给 `clipboard_entry` 加 `delivery_tracked` 列
- `src-tauri/crates/uc-infra/src/clipboard/entry_delivery_repository.rs` — Diesel 实现
- `src-tauri/crates/uc-application/src/usecases/clipboard_sync/get_entry_delivery_view.rs` — `GetEntryDeliveryViewUseCase`
- `src-tauri/crates/uc-application/src/facade/clipboard/delivery.rs`(或合并入既有 ClipboardFacade 文件)

修改：
- `src-tauri/crates/uc-core/src/clipboard/mod.rs` — 导出 delivery 模块
- `src-tauri/crates/uc-core/src/ports/clipboard/mod.rs` — 导出 port
- `src-tauri/crates/uc-core/src/ports/mod.rs` — re-export
- `src-tauri/crates/uc-application/src/usecases/clipboard_sync/dispatch_entry.rs` — 每个 target 的 dispatch 结果调 `EntryDeliveryRepositoryPort::record_attempt`
- `src-tauri/crates/uc-application/src/facade/clipboard/mod.rs` 或 `app_facade.rs` — `ClipboardFacade::get_entry_delivery_view(...)`
- `src-tauri/crates/uc-application/src/usecases/clipboard_capture/*`(具体文件待 Phase 1 早期确认)— LocalCapture 写 entry 时设 `delivery_tracked = true`
- `src-tauri/crates/uc-infra/src/schema.rs` — Diesel schema 同步
- `src-tauri/crates/uc-infra/src/clipboard/mod.rs` — wire 新 repository
- bootstrap/wiring(具体文件待 Phase 1 早期确认)— 注入新 repository

**领域类型草案**(`uc-core/src/clipboard/delivery.rs`):

```rust
//! Entry delivery —— "本机视角下某条 entry 对每个对端的投递结果"的领域模型。
//!
//! 为什么需要这个模块:
//! 出站同步是"一对多"广播,但 wire 层每次 dispatch 只覆盖单个对端;
//! 没有任何持久化结构能回答"这条 entry 对端 X 收到了没"。本模块把
//! "投递尝试 + 结果"提升为可被查询的领域事实,让 UI 能展示同步覆盖
//! 情况,也为后续重传/重试策略提供决策依据(本期不实现重试)。
//!
//! Pending(未尝试)状态不在本模块持久化,由应用层用 "trusted_peer
//! 全集 LEFT JOIN delivery 表" 的方式在 view 层合成。

pub enum EntryDeliveryStatus {
    Delivered,   // 对端 adapter 接收(DispatchAck::Accepted)
    Duplicate,   // 对端 adapter 报告已存在(DispatchAck::DuplicateIgnored)
    Failed { reason: DeliveryFailureReason },
}

pub enum DeliveryFailureReason {
    Offline,        // ClipboardDispatchError::Offline
    LocalPolicy,    // LocalPolicyExceeded(...)
    PeerRejected,   // PeerRejected(...)
    Io,             // Io(...)
    Internal,       // Internal(...)
}

pub struct EntryDeliveryRecord {
    pub entry_id: EntryId,
    pub target_device_id: DeviceId,
    pub status: EntryDeliveryStatus,
    pub reason_detail: Option<String>,  // 失败时人类可读补充,从 Error 的内部字符串拷贝
    pub updated_at_ms: i64,
}
```

**Port 草案**(`uc-core/src/ports/clipboard/delivery.rs`):

```rust
//! 投递状态仓储端口。负责持久化"某条 entry 对某个对端的最新投递状态"。
//!
//! 契约:
//! - 同一 (entry_id, target_device_id) 二元组只保留一行,record_attempt 为 upsert 语义
//! - list_by_entry 返回顺序无保证,调用方自行排序
//! - 删除 entry 后,本 port 的行随 FK CASCADE 自动清理
//! - trusted_peer 被删除不影响本 port 行(语义见 mod 头注)

#[async_trait]
pub trait EntryDeliveryRepositoryPort: Send + Sync {
    async fn record_attempt(
        &self,
        record: &EntryDeliveryRecord,
    ) -> Result<(), EntryDeliveryError>;

    async fn list_by_entry(
        &self,
        entry_id: &EntryId,
    ) -> Result<Vec<EntryDeliveryRecord>, EntryDeliveryError>;
}
```

**表结构草案**(`up.sql`):

```sql
CREATE TABLE clipboard_entry_delivery (
    entry_id          TEXT    NOT NULL,
    target_device_id  TEXT    NOT NULL,
    status            TEXT    NOT NULL,   -- 'delivered' | 'duplicate' | 'failed_offline' | 'failed_local_policy' | 'failed_peer_rejected' | 'failed_io' | 'failed_internal'
    reason_detail     TEXT,                -- 失败时人类可读补充,可空
    updated_at_ms     BIGINT  NOT NULL,
    PRIMARY KEY (entry_id, target_device_id),
    FOREIGN KEY (entry_id) REFERENCES clipboard_entry(entry_id) ON DELETE CASCADE
);

CREATE INDEX idx_entry_delivery_entry ON clipboard_entry_delivery(entry_id);

-- 历史降级:新 entry 标 true,老 entry 默认 false
ALTER TABLE clipboard_entry ADD COLUMN delivery_tracked INTEGER NOT NULL DEFAULT 0;
-- SQLite 无原生 BOOLEAN,用 INTEGER 0/1
```

**Facade 输出形态**:

```rust
pub struct GetEntryDeliveryViewQuery { pub entry_id: EntryId }

pub struct EntryDeliveryView {
    pub entry_id: EntryId,
    pub source: EntrySource,            // Local | Remote { device_id } | Historical
    pub deliveries: Vec<EntryDeliveryTargetView>,  // Historical 时为空
}

pub enum EntrySource {
    Local,
    Remote { device_id: DeviceId },     // Phase 3 起补 device_name
    Historical,                          // delivery_tracked=false 的老 entry
}

pub struct EntryDeliveryTargetView {
    pub target_device_id: DeviceId,
    // pub target_device_name: Option<String>,  // Phase 3 起填充
    pub status: EntryDeliveryStatusView,
    pub reason_detail: Option<String>,
    pub updated_at_ms: Option<i64>,     // Pending 时为 None
}

pub enum EntryDeliveryStatusView {
    Pending,    // facade 合成:trusted_peer ∈ T 但 (entry_id, target) ∉ D
    Delivered,
    Duplicate,
    Failed { reason: DeliveryFailureReason },
}
```

**`GetEntryDeliveryViewUseCase` 内部逻辑**:

```
1. entry_repo.get(entry_id) → 拿 entry
2. 若 entry.delivery_tracked == false → 返回 { source: Historical, deliveries: [] }
3. event_repo.get(entry.event_id) → 拿 event.source_device
   → 判定 source:本机 device_id == event.source_device ? Local : Remote
4. trusted_peer_repo.list() → T(所有 trusted desktop peer device_id 集合)
5. delivery_repo.list_by_entry(entry_id) → D
6. 对每个 t ∈ T:
   - 若 t in D:用 D 中的 status
   - 若 t not in D:status = Pending,updated_at = None
7. D 中孤儿行(target ∉ T)→ 忽略(D9 选 C)
8. 排序(建议:Failed/Pending 优先,Delivered 在后)
```

**测试点**:
- `dispatch_entry` 完成后，delivery 表中对每个 target 各有一行，状态正确 (`Accepted` → Delivered, `DuplicateIgnored` → Duplicate, 五种 `ClipboardDispatchError` → 对应 Failed 变体)
- `dispatch_entry` 二次调用同一 entry 时，行被 upsert 而非新增
- LocalCapture 新建 entry 时 `delivery_tracked = true`
- migration 后，老 entry 的 `delivery_tracked` 全部为 false
- facade `get_entry_delivery_view`:
  - 老 entry → `source: Historical`,deliveries 空
  - 新本地 entry，无 peer → source: Local,deliveries 空
  - 新本地 entry,3 peer 已 dispatch → source: Local,3 个 Delivered/Failed
  - 新本地 entry,3 peer 但有一个还没 dispatch → 那个 target 标 Pending
  - 远端 entry → source: Remote { device_id: 对端 id },本机不在 trusted_peer.list() 范围内，deliveries 应不包含本机
  - 已删除的 trusted_peer 在 delivery 表里有行 → view 不显示 (孤儿过滤)

**依赖关系**:无前置 phase。

**Status**: complete(2026-05-14)

**实施总结**:
- `clipboard_entry_delivery` 表 + `delivery_tracked` 列 migration 落地
- `EntryDeliveryRepositoryPort` + `DieselEntryDeliveryRepository` 实现，6 个测试通过 (upsert/list/FK cascade/EntryNotFound 全覆盖)
- `dispatch_entry.rs` 在 JoinSet 各 arm 后落盘 delivery，五种 `ClipboardDispatchError` 全部精确映射到 `DeliveryFailureReason`
- `GetEntryDeliveryViewUseCase` 实现 trusted_peer LEFT JOIN delivery 合成 view，处理 Historical / Local 无 peer / Remote / 孤儿过滤全分支
- `ClipboardSyncFacade::get_entry_delivery_view` 透出，白名单导出视图类型
- bootstrap assembly 装配 `entry_delivery_repo` + `clipboard_event_reader_repo` 双 wired，通过 `WiredDependencies` 注入
- `cargo check --workspace --tests` 通过;uc-application 448 测试 pass

---

### Phase 2: 前端 detail UI — 两套预览各加"来源 + 同步状态"区域

**目的**:扩展 quick-panel 和主窗口两套 detail UI，完整展示来源 + 每台设备同步状态。device 名暂时显示为 device_id 截断 (Phase 3 补 name)。

**改动文件清单**:

新增：
- `src/components/clipboard/EntryDeliverySection.tsx` — 共享渲染组件，接受 `EntryDeliveryView` props，渲染"来源 + 同步状态列表"两块
- `src/api/daemon/clipboard_delivery.ts`(或合入既有 clipboard.ts)— Tauri command invoke 封装
- `src-tauri/src/commands/clipboard_delivery.rs`(或合入既有)— Tauri command，内部调 `AppFacade.clipboard().get_entry_delivery_view(...)`

修改：
- `src/quick-panel/useClipboardPreview.ts` — 返回值增加 `delivery: EntryDeliveryView | null`,在 fetch preview 时并行 fetch delivery view
- `src/quick-panel/ClipboardPreviewPane.tsx` — 在标题栏下、内容预览上方挂 `<EntryDeliverySection delivery={...} />`
- `src/components/clipboard/ClipboardPreview.tsx` — 同上，挂在 `ClipboardPreviewInfo` 附近
- `src/quick-panel/types.ts` — 增加 `EntryDeliveryView` / `EntryDeliveryTargetView` / `EntrySource` / `EntryDeliveryStatusView` / `DeliveryFailureReason` 类型
- `src-tauri/src/commands/mod.rs` — 注册新 command

**Tauri command 草案**:

```rust
#[tauri::command]
pub async fn clipboard_entry_delivery_view(
    state: tauri::State<'_, AppState>,
    entry_id: String,
) -> Result<EntryDeliveryViewDto, CommandError>;
```

DTO 命名遵循既有惯例 (daemon-contract 还是 uc-tauri 本地？Phase 2 早期对齐)。

**UI 草案 — EntryDeliverySection**:

```
┌──────────────────────────────────────────┐
│ 来自: 本地                                 │  ← Source 行
├──────────────────────────────────────────┤
│ 同步状态:                                  │  ← 列表标题
│   ✓ did_a1b2c3...     已接收              │
│   ✓ did_d4e5f6...     已接收(去重)         │
│   ✗ did_g7h8i9...     失败 · 对端离线       │
│   · did_j0k1l2...     未尝试               │
└──────────────────────────────────────────┘
```

Historical entry 时：
```
来自: 本地
同步状态: 此条目在同步追踪启用前创建,无投递记录
```

新本地 entry 但无 peer:
```
来自: 本地
同步状态: 暂未配对任何设备
```

**视觉规范**:
- 列表行高紧凑 (quick-panel 空间紧)
- 状态用 icon 标识:✓ Delivered/Duplicate · · Pending · ✗ Failed
- Failed 行旁边显示 reason 文案 (国际化 key:`delivery.failureReason.<variant>`)
- Duplicate 用淡灰色 + "(去重)"后缀
- device_id 截断展示 (如 `did_a1b2c3...`,前 8 字符),Phase 3 补真实 name

**数据获取策略**:
- 当前 `useClipboardPreview(entryId)` 已经是按需 fetch(只在 entryId 切换时触发)
- 加 delivery 数据:Tauri command 并发或串行 fetch?**串行**(facade 输出已经包含 source，合并到一次 facade 调用更好;在 Tauri command 层就把 preview + delivery 合并返回)— 但这会改 preview command 的形态。**Phase 2 早期对齐**:是合并 fetch 还是开第二个 command 并发
- 倾向开第二个 command(分离关注点，preview 不被影响)

**触发刷新**:
- Phase 2 暂定:Pane 重开 (entryId 切换 + 同 entryId 重新打开) 时 fetch
- 不监听 dispatch 完成事件
- 用户体验：打开 detail 一瞬间看到的是"此刻"的状态，继续打开期间不动态刷新
- Phase 2 完成后评估是否值得加事件机制 (R3 风险)

**测试点**:
- `EntryDeliverySection` 各种状态组合的渲染：
  - Historical
  - Local · 无 peer
  - Local · N peer 全 Delivered
  - Local · N peer 混合 Delivered/Duplicate/Failed/Pending
  - Remote · 3 peer 同步 (注意 Remote 时 deliveries 是本机对其他 peer 的转发记录，不包含来源 peer 自己)
- Tauri command 错误情况:entry 不存在 / facade 抛错
- 国际化：所有文案有 i18n key，中英文都覆盖
- 视觉测试:quick-panel 在 5 peer 时不挤压内容预览到不可用

**依赖关系**:依赖 Phase 1 的 facade 输出。

**Status**: pending

---

### Phase 3: 设备名解析 — device_directory 表 + DeviceDirectoryPort

**目的**:把 device_id 截断展示替换为人类可读的设备名 (如"MacBook Pro")。

**改动文件清单**:

新增：
- `src-tauri/crates/uc-core/src/device_directory/mod.rs` — `DeviceDirectoryEntry` 实体 + `DeviceDirectoryError`
- `src-tauri/crates/uc-core/src/ports/device_directory.rs` — `DeviceDirectoryPort`
- `src-tauri/crates/uc-infra/migrations/<YYYY-MM-DD-HHMMSS>_device_directory/up.sql` + `down.sql`
- `src-tauri/crates/uc-infra/src/device_directory_repository.rs` — Diesel 实现

修改：
- `src-tauri/crates/uc-core/src/lib.rs` — 导出 device_directory 模块
- `src-tauri/crates/uc-core/src/ports/mod.rs` — 导出 port
- `src-tauri/crates/uc-application/src/usecases/clipboard_sync/ingest_inbound.rs` — 接收 entry 后，调 `DeviceDirectoryPort::upsert(header.origin_device_id, header.origin_device_name)`
- `src-tauri/crates/uc-application/src/usecases/clipboard_sync/get_entry_delivery_view.rs` — 拼装 view 时，batch_get 所有相关 device_id 的 name，填到 `EntrySource::Remote.device_name` 和 `EntryDeliveryTargetView.target_device_name`
- `src-tauri/crates/uc-infra/src/schema.rs` — Diesel schema 同步
- bootstrap/wiring — 注入新 repository
- `src/quick-panel/types.ts` — `EntrySource.Remote` 增加 `device_name?: string`;`EntryDeliveryTargetView` 增加 `target_device_name?: string`
- `src/components/clipboard/EntryDeliverySection.tsx` — 优先用 device_name,fallback 到 device_id 截断

**表结构草案**:

```sql
CREATE TABLE device_directory (
    device_id        TEXT NOT NULL PRIMARY KEY,
    display_name     TEXT NOT NULL,
    last_updated_ms  BIGINT NOT NULL
);
```

**Port 草案**:

```rust
//! 设备名目录端口。把 device_id 维度的"用户可读显示名"沉淀为可被查询的领域事实。
//!
//! 写入语义:每次接收到带有 origin_device_id + origin_device_name 的入站消息,
//! 都应触发 upsert。无入站记录的 peer 不在本目录中,调用方应 fallback。
//!
//! 安全注意:display_name 是对端自报值,fingerprint 已认证但 name 不可信。
//! 渲染方应做长度截断、控制字符过滤等防显示破坏处理。

#[async_trait]
pub trait DeviceDirectoryPort: Send + Sync {
    async fn upsert(
        &self,
        device_id: &DeviceId,
        display_name: &str,
    ) -> Result<(), DeviceDirectoryError>;

    async fn get_by_id(
        &self,
        device_id: &DeviceId,
    ) -> Result<Option<String>, DeviceDirectoryError>;

    async fn batch_get(
        &self,
        device_ids: &[DeviceId],
    ) -> Result<HashMap<DeviceId, String>, DeviceDirectoryError>;
}
```

**测试点**:
- inbound ingest 后 directory 中能查到对端 name
- 同一 peer 改名再发 → directory 中 name 被更新，`last_updated_ms` 推进
- batch_get 部分命中：返回的 Map 只包含已知 id
- view 拼装：已知 peer 显示 name，未知 peer fallback 到 device_id 截断
- name 含特殊字符 (emoji / 控制字符 / 超长字符串) 的存取保真

**覆盖率风险**(见 findings R4):
- 仅接收过我方推送、从未推送过我方的 trusted peer 不会出现在 directory
- 缓解:Phase 3 早期看 pairing 协议是否已透传 name;若未，加一个轻量"配对完成时双向交换设备名"的步骤 (超出本期则接受 fallback)

**依赖关系**:依赖 Phase 1 与 Phase 2。本 phase 是叠加增强，Phase 2 落地后 UI 已能用 fallback 跑通。

**Status**: pending

---

## Key Questions(Phase 1/2/3 早期需对齐的点)

1. ~~**Phase 1**:`dispatch_entry.rs` 并发 JoinSet 内对每个 target 单独写 delivery 行，N peer × 高频复制场景下的 SQLite 写吞吐？~~ **已量化，不是瓶颈，安全系数 ~1000×**。见 `findings.md` R1。
2. **Phase 1**:LocalCapture 写 entry 在哪个 usecase 文件？需在那里把 `delivery_tracked` 设为 true。
3. **Phase 1**:`EntryDeliveryRepositoryPort` 在 tokio task 内调用的并发模型 — diesel sync 用 `spawn_blocking` 还是项目已有 async wrapper?
4. **Phase 2**:Tauri command 是合并 preview + delivery 一次返回，还是独立 command 并发？倾向独立。
5. **Phase 2**:实时刷新机制 — 暂走"Pane 重开时 fetch",Phase 2 完成后评估事件机制 (R3)。
6. **Phase 3**:pairing 协议是否已透传对端 device name？若已，可在配对完成时就 upsert directory(R4 缓解)。

## Decisions Made

| 编号 | 决策 | 选择 | 一句话理由 |
|------|------|------|-----------|
| 总体 · 同步语义 | 复用 DispatchAck vs 改 wire ack | 复用 | 用户拍板，改 wire 成本数倍于收益 |
| 总体 · Mobile 范围 | 是否覆盖 mobile | 不覆盖 | iOS Shortcut 是 pull 模型，无 dispatch 路径 |
| 总体 · 失败处理 | 是否提供重试按钮 | 不提供 | 本期只做可视化 |
| 总体 · 展示入口 | 列表 vs detail | 仅 detail | 列表不动，所有展示集中到 detail 视图 |
| D1 | source device 字段位置 | A · JOIN event 反查 | detail 低频，JOIN 成本可忽略，无冗余 |
| D2 | EntryDeliveryRepositoryPort 单独定义 | A · 单独定义 | 投递是新领域概念，职责单一 |
| D3 | Pending 状态表达 | C · facade 层合成 | Pending 本质是"事实的缺失";写放大消失 |
| D4 | 列表批量 API | 失效 | 列表不展示，无需 |
| D5 | facade vs webserver | facade | 既定事实 (in-process facade) |
| D6 | 设备名解析层 | A · device_directory 表 + Port | 同时解决 source/target name，真相单点收口 |
| D7 | 历史 entry 降级 | A1 · entry 表加 delivery_tracked 列 | per-entry 自带标志，不依赖全局变量 |
| D8 | 离线 vs 在线失败区分 | 用现有 ClipboardDispatchError 五分类映射 | wire 层信号已足够细 |
| D9 | trusted_peer 删除后 delivery 处理 | C · 保留行 + facade 默认不展示 | 不丢历史，UI 简洁，改动最小 |
| D10 | quick-panel 紧凑 vs 完整 | A · 两边都完整铺开 | UI 形态一致，不隐藏功能 |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
|       |         |            |

## Notes

- 每个 phase 开始前重读本文件，确认依赖项已就绪
- 每个 phase 完成后，把 Status 从 pending → in_progress → complete，并在 `progress.md` 记录会话日志
- 任何与本 plan 不一致的设计变更，必须先更新 `findings.md` 决策表
- 严格遵守 `uc-core` / `uc-application` 的 AGENTS.md:核心边界、facade 唯一出口、port 注释纪律
