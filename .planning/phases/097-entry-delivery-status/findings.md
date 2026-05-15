# Findings: 097 · Entry Delivery Status

## 关键事实 (已通过代码调研验证)

### F1. 现有 dispatch 已经是 request-reply，带 wire-level ack

`ClipboardDispatchPort::dispatch()` (`src-tauri/crates/uc-core/src/ports/clipboard/sync_dispatch.rs:113-120`) 返回 `Result<DispatchAck, ClipboardDispatchError>`。`DispatchAck` 有两个变体：`Accepted` 和 `DuplicateIgnored`。注释明确说 "Adapter-layer ack semantics only — `Accepted` means the bytes reached the peer and its adapter accepted them for ingest; it does **not** promise the application-level ingest succeeded"。

**含义**:不改 wire protocol 就能拿到"对端 adapter 接收"的状态，落盘即得 UI 所需信息。

### F2. `dispatch_entry.rs` 已经做了 per-target 隔离 + 内存报告

注释明确说"failure per target is isolated in the per-target report so a single unreachable peer never blocks the rest of the roster"。每个 target 在 `JoinSet` 里独立 dispatch，结果汇总到一个 per-target 结构 (只在内存)。

**含义**:落盘逻辑可以挂在 JoinSet 各任务完成时，粒度天然就是 per target，改动点局部。

### F3. `ClipboardDispatchError` 分五类

```rust
Offline | LocalPolicyExceeded(_) | PeerRejected(_) | Io(_) | Internal(_)
```

`Offline` 即"对端节点不可达 (没地址或拨号失败)",注释说"application layer treats this as peer offline"。

**含义**:`DeliveryFailureReason` 枚举 1:1 对应这五类即可，无需额外抽象。

### F4. ClipboardHeader 已带 origin_device_id + origin_device_name

wire v2(`sync_dispatch.rs:32-52`) 中 header 包含：
```rust
pub origin_device_id: String,
pub origin_device_name: String,  // Plaintext, passively propagated
```

**含义**:入站接收时，可以把对端的 (id, name) 写到一个"设备目录"表，后续 UI 显示设备名不需要额外通讯。Phase 3 设备名解析的关键基础。

### F5. ClipboardEvent 表已记录 source_device

迁移 `2026-01-09-141527_clipboard_core` 的 `clipboard_event` 表已有 `source_device` 列。entry 通过 `event_id` 关联到 event。

**含义**:"这条 entry 来自哪台设备"通过 JOIN event 即可获得，无需在 entry 表加冗余字段。

### F6. TrustedPeer 没有 device name 字段

`src-tauri/crates/uc-core/src/trusted_peer/peer.rs:14-19` 只有 `local_device_id` / `peer_device_id` / `peer_fingerprint` / `trusted_at`。

**含义**:目前没有 device id → device name 反查机制。Phase 3 通过新增 device_directory 表 + DeviceDirectoryPort 解决。

### F7. uc-application 对外只能走 facade(§11.4 铁律)

`src-tauri/crates/uc-application/AGENTS.md` §11.4:外部 crate(daemon / tauri / CLI) 不得 `use uc_application::usecases::...`,必须走 `uc_application::facade::*`。

**含义**:新增 `GetEntryDeliveryViewUseCase` 必须经 `ClipboardFacade`(或域 facade) 透出。命名也要遵守：`*UseCase` / `*Facade` / `*Command` / `*Query` / `*Result`。

### F8. GUI 走 in-process facade

用户记忆 `project_gui_uses_inprocess_facade.md`:uc-tauri 直调 AppFacade,webserver 仅留给 LAN 业务。

**含义**:Tauri command 通过 `AppFacade.clipboard().get_entry_delivery_view(...)` 调用，不走 HTTP。webserver 路由可不开放 (若 mobile 不需要)。

### F9. 前端已有 detail UI 形态 (两套)

研究发现项目已经有两套预览/detail UI:

| UI | 文件 | 形态 | 当前展示 |
|----|------|------|---------|
| quick-panel detail | `src/quick-panel/ClipboardPreviewPane.tsx`(94 行) | 列表右侧分栏，可折叠，基于 selection/hover 触发 | 标题 + 大小 + 内容预览 + 删除快捷键提示 |
| 主窗口 detail | `src/components/clipboard/ClipboardPreview.tsx` + `ClipboardPreviewInfo.tsx` | 完整页 | 同上 + 一行 InfoRow metadata(类型/字符数/尺寸等) |

两套数据层共用 `useClipboardPreview(entryId)` hook，渲染层分别做。**两套都不展示** device / 同步信息。

**含义**:不需要新建 detail 组件 / 窗口，扩展 `useClipboardPreview` hook 返回值，在两个 Pane 里各加渲染区。

### F10. 列表 UI 不显示同步信息 (用户决定)

`GET /clipboard/entries` 列表返回的 `EntryProjectionResponseDto` **不增加** 来源 / 同步状态字段。所有展示集中到 entry detail 视图。

**含义**:不需要批量 summary API;`EntryDeliveryRepositoryPort` 不需要 `summarize_batch` 方法;原 Phase 3(列表汇总徽章) 整个不存在。

---

## 设计决策 (全部已敲定)

### D1 · source device 字段位置 → 选 A:JOIN event 反查

**最终选择**:facade 拼 view 时通过 `entry.event_id` JOIN `clipboard_event.source_device`,不在 entry 表加冗余列。

**理由**:detail 是低频操作 (点开一条才发生),JOIN 成本完全不重要;无冗余，event.source_device 是唯一权威;migration 也省了。F10 后，列表完全不需要 source 字段，A 没有任何缺点。

---

### D2 · EntryDeliveryRepositoryPort 单独定义 → 选 A:单独定义

**最终选择**:新建 `EntryDeliveryRepositoryPort`(`uc-core/src/ports/clipboard/delivery.rs`),不合并入现有 port。

**理由**:delivery 是新的领域概念 (投递记录),与现有 entry/event 仓储 (管"内容") 职责不同;uc-core AGENTS §5.2 强调 port 应"以业务能力命名" + "保持最小接口"。

**最终接口**(配合 D3 选 C 后大幅精简):
```rust
#[async_trait]
pub trait EntryDeliveryRepositoryPort: Send + Sync {
    async fn record_attempt(&self, record: &EntryDeliveryRecord) -> Result<(), EntryDeliveryError>;
    async fn list_by_entry(&self, entry_id: &EntryId) -> Result<Vec<EntryDeliveryRecord>, EntryDeliveryError>;
}
```

只有两个方法，无 `summarize_batch`(F10 → 列表无徽章),无 Pending 清理方法 (D3 → Pending 不持久化)。

---

### D3 · Pending 状态表达 → 选 C:Pending 不持久化，facade 层合成

**最终选择**:`clipboard_entry_delivery` 表只存"已尝试过的投递结果"(Delivered/Duplicate/Failed),**不存 Pending**。facade 拼 view 时做：
```
trusted_peer 全集 (T)  LEFT JOIN  delivery 表 (D)
  ├─ 存在于 D 的 target → 用 D 中状态
  └─ 不在 D 的 target → view 里合成 Pending
```

**理由**:
- 写放大彻底消失：每条 entry 只对实际尝试过的 target 写行
- Pending 的语义本质就是"事实的缺失",硬塞进表里是把空集变 N 行的反模式
- detail 视图完整性靠 facade 保证，UI 层简单
- 不需要担心 trusted_peer 中途变化的孤儿 Pending 行问题 (C 没有这种行)

**实现要点**:`GetEntryDeliveryViewUseCase` 同时注入 `EntryDeliveryRepositoryPort` 与 `TrustedPeerRepositoryPort`(或等价 port),在 use case 层做差集合并。

---

### D4 · 列表批量 API → 失效

**结果**:不需要。

**理由**:F10 后列表不展示同步信息，无需批量 summary。整个原 Phase 3 不存在。

---

### D5 · GUI 走 facade vs webserver → 选 facade(既定)

**最终选择**:走 in-process facade。本期不开放 HTTP 端点，除非未来 mobile/CLI 需要消费同步状态 (届时再补)。

**理由**:F8(既定事实)。

---

### D6 · 设备名解析层 → 选 A:新增 device_directory 表 + DeviceDirectoryPort

**最终选择**:Phase 3 新增持久化设备目录：
- 新表 `device_directory(device_id PRIMARY KEY, display_name, last_updated_ms)`
- 新 port `DeviceDirectoryPort`(`uc-core/src/ports/device_directory.rs`),方法：`upsert(device_id, name)`、`get_by_id(device_id) -> Option<name>`、`batch_get(device_ids) -> Map<id, name>`
- 入站 ingest(`ingest_inbound.rs`) 收到 entry 时把 `header.origin_device_id` + `header.origin_device_name` upsert
- facade 拼装 view 时 batch_get，补 source / target 的 display_name

**理由**:
- 同时解决"来源 name"与"target name"两种需求
- 真相单点收口 (对应 uc-application AGENTS §10.2)
- 领域语义清晰:DeviceDirectoryPort 是 device_id 维度的字典，职责单一，不污染 TrustedPeer
- 跨重启稳定;跨设备视图独立合理 (每台机器看到的 peer 名字 map 各自维护)

**注意点**:`origin_device_name` 是对端自报的，可被设置为任意字符串。但 fingerprint 已认证，name 只是显示。UI 渲染时做 UTF-8 截断 + 控制字符过滤即可。

---

### D7 · 历史 entry 降级 → 选 A1:entry 表加 delivery_tracked 列

**最终选择**:
- migration 给 `clipboard_entry` 加 `delivery_tracked BOOLEAN NOT NULL DEFAULT FALSE`
- 新 entry 在 LocalCapture 时设为 TRUE
- facade 拼装 view:`delivery_tracked = false` → view 里标 `source_meta: Historical`,deliveries 直接空，**不合成 Pending**

**理由**:
- A1 比 A2(全局时间戳) 更可靠：每条 entry 自带"是否被追踪"标志，不依赖全局变量，任何路径创建的新 entry 都能正确标记
- 比 B(统一显示空) 用户体验更明确：能区分"老 entry(系统升级前)"和"新 entry 但暂时未广播"
- 比 C(migration 回填) 更诚实：不假装我们知道老 entry 当年的同步结果
- SQLite ALTER TABLE ADD COLUMN 是 O(1) schema 操作，代价可控

**UI 文案建议**(view 含 `source_meta: Historical` 时):
```
来自: 本地
同步状态: (此条目在同步追踪启用前创建,无投递记录)
```

---

### D8 · 离线 vs 在线失败如何区分 → 用现有 5 分类映射

**最终选择**:`DeliveryFailureReason` 1:1 映射 `ClipboardDispatchError`,共 5 个变体 (Offline / LocalPolicy / PeerRejected / Io / Internal)。UI 文案直接区分。

**理由**:
- wire 层信号已经足够细
- D3 选 C 后，Pending 是 view 合成概念，真正的离线 peer 会落盘为 `Failed{Offline}`,不会卡在 Pending
- 不需要新增任何状态字段

---

### D9 · trusted_peer 删除后 delivery 行处理 → 选 C:保留行 + facade 默认不展示

**最终选择**:
- delivery 表的 `target_device_id` 列 **不** 加 FK 到 trusted_peer，删除 peer 时不联动删 delivery
- facade 拼 view 维持 "trusted_peer 全集 LEFT JOIN delivery" 语义，delivery 表中"孤儿"行 (target 已不在 trusted_peer 全集) 自然被忽略
- 数据库层保留历史，UI 层简洁

**理由**:
- 不丢历史 (未来调试 / 数据导出 / 审计 / 用户回查"曾经同步过哪些设备"都受益)
- UI 默认简洁，用户不会被"已删除设备"的鬼魂干扰
- 实现量最小：删 peer 时不动 delivery
- 容量基本可忽略 (删除 peer 是低频，且 delivery 表对该 peer 的增量为 0)

**未来可选**:若产品需求"显示历史同步过的已删设备",facade 加一个参数 `include_orphans: bool`,改 JOIN 语义即可，不破坏现有形态。

---

### D10 · quick-panel 紧凑 vs 完整 → 选 A:两边都完整铺开

**最终选择**:quick-panel 的 `ClipboardPreviewPane` 和主窗口的 `ClipboardPreview` 都完整展示"来源 + 完整同步状态列表",形态一致。

**理由**:
- 用户在哪打开 detail 都看到相同信息，学习成本低
- quick-panel 是用户最高频入口，如果只显示汇总，等于这个功能被隐藏
- 两套 UI 共享渲染组件 (同一个 `EntryDeliverySection.tsx`),改动一处

**视觉成本**:
- quick-panel 的 `ClipboardPreviewPane` 高度被占用，内容预览空间被挤压
- 缓解方法：同步状态列表可滚动 + 紧凑行高 + 用 icon 而非文字标状态 (✓ ✗ ·)

---

## 名词速查

- **Delivery**:本机视角下"把某条 entry 投递给某个对端"这一行为及其状态
- **Target**:某个 trusted desktop peer,delivery 的接收方
- **Source**:某条 entry 的来源 (本地捕获 / 远端某设备推送)
- **Dispatch**:wire 层的一次单 target 投递动作 (`ClipboardDispatchPort::dispatch`)
- **Ack**:对端 adapter 在 wire 层返回的接收确认 (`DispatchAck::Accepted` / `DuplicateIgnored`)
- **Historical entry**:`delivery_tracked = false` 的 entry，即新表上线前已存在的老数据
- **Device directory**:device_id → display_name 的持久化字典，由 inbound ingest 维护

## 用语对齐

- 不使用 "sync state" / "sync status" 作为表名或字段名 — 项目内 "sync" 已经被 `sync_planner` 等模块占用语义
- 使用 "delivery"(投递) 作为本功能的唯一专用词
- UI 文案上可对用户使用"同步状态"等口语化词，但代码内部一律 delivery

## 风险与待量化项

### R1. 写放大 · 已量化，无瓶颈风险

**结论：不是瓶颈，安全系数 ~1000×**。Phase 1 可按 plan 实施，无需做批量 insert / 延迟 commit / 事务批处理等优化。

**基准方法**:`bench/r1_delivery_write_amp.py`(Python sqlite3，与生产 SQLite 同引擎;WAL + busy_timeout=5000 + foreign_keys=ON 与 `uc-infra/src/db/pool.rs` 对齐;文件落盘触发真实 fsync)。

**结果**(2026-05-14，本机 macOS):

| 场景 | N peer | M entry | 每条 entry p50 | p99 | 吞吐 |
|------|--------|---------|---------------|-----|------|
| 普通用户 | 3 | 100 | 77 μs | 1.5 ms | 23k rows/s |
| 重度用户 | 5 | 500 | 56 μs | 260 μs | 84k rows/s |
| 极端用户 | 10 | 1000 | 71 μs | 345 μs | 120k rows/s |
| 压力测试 | 10 | 5000 | 66 μs | 385 μs | 122k rows/s |

**对照生产负载**:
- 用户复制频率峰值估计 ~10 Hz(已是机器人级)× 10 peer = 100 rows/s
- SQLite raw 容量 = 120k+ rows/s
- 富余 1000×

**p99 outlier 分析**:
- 普通场景 p99 = 1.5ms 因样本量小 (100 entry) 被 WAL checkpoint 等 outlier 拉高
- 大样本 (5000 entry)p99 反而降到 385μs，说明 checkpoint 开销在长期运行下被均匀摊销
- 即便 1.5ms 的 p99 也远低于用户键盘交互感知阈值 (~100ms)

**已知未模拟项 (不影响结论)**:
- diesel + r2d2 连接池薄包装开销 (对 raw 容量做了 lower bound 估计，仍有 1000× 余量)
- tokio spawn_blocking 上下文切换开销 (~1-10μs 量级，与 75μs p50 相比可忽略)
- 真实 dispatch 完成的时间分布 (我们假设 instant 落盘，实际更松散，争用更少)

### R2. dispatch 完成回写时序

`dispatch_entry.rs` 的 JoinSet 在每个任务完成时立即调 `record_attempt`,需要确保 `EntryDeliveryRepositoryPort` 是 Send + Sync + 可在 tokio task 内调用。Diesel 实现要用 `tokio::task::spawn_blocking` 或 `diesel-async`。Phase 1 早期看现有 repository 怎么做的对齐。

### R3. UI 实时刷新

dispatch 完成 → delivery 表已写 → UI 何时拿到新数据？
- 选项 a:广播事件，UI 监听后刷新对应 entry detail
- 选项 b:轮询 (简单但低效)
- 选项 c:Pane 重新打开时才取 (被动)

Phase 2 早期看现有事件总线机制 (`broadcast channel` / Tauri event) 是否支持 entry-level 事件。如果暂时不支持，先走 c，后续补 a。

### R4. Phase 3 设备名解析的覆盖率

device_directory 只有在 inbound ingest 经过时才会被填充。如果某个 trusted peer **只接收过我们的内容，从未向我们 dispatch**(单向场景),它的 name 永远不会进入 directory。

缓解方案 (Phase 3 早期决定):
- 方案 1:在配对完成时就把对端 name 写一份 (需要配对协议透传 name —— 看一下 pairing 协议是否已带)
- 方案 2:UI 显示 fallback 为 device_id 截断 (用户可接受但不友好)
- 方案 3:加一个"主动同步设备名"的轻量协议 (超出本期范围)

实际预期：大多数 peer 都是双向通信，这个边缘场景影响小。
