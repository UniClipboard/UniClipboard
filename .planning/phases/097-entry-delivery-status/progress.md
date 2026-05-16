# Progress Log: 097 · Entry Delivery Status

## 会话索引

| 日期 | 会话简述 | 主要产出 |
|------|----------|----------|
| 2026-05-13 | 需求对齐 + 设计稿 | task_plan.md / findings.md / progress.md 初版 |
| 2026-05-14 | 决策逐一 review + plan 重写 | task_plan.md / findings.md 第二版，所有决策已敲定 |
| 2026-05-14 | R1 写放大基准 | `bench/r1_delivery_write_amp.py` + findings R1 更新 (已量化，1000× 余量) |
| 2026-05-14 | Phase 1 实施完成 | uc-core + uc-infra + uc-application + uc-bootstrap 端到端落地，workspace+tests 编译通过，uc-infra 6+ / uc-application 448 测试 pass |
| 2026-05-15 | Phase 3 实施完成 (D6 策略调整，复用 SpaceMember) | uc-application view use case 注入 member_repo;facade/wiring 零改动;前端 deviceName fallback;workspace check 通过，uc-application 477/477,EntryDeliverySection 7/7 |

---

## 2026-05-13 · 需求对齐 + 设计稿

### 上下文

用户提出需求："展示每一个 entry 的同步情况，同步到了哪台设备上"。

### 调研发现

通过 Explore 子代理 + 直接读取关键文件，确认了以下事实 (全部沉淀到 `findings.md`):

- 现有 dispatch 已经是 request-reply，带 `DispatchAck::Accepted` / `DuplicateIgnored`
- 现有 `dispatch_entry.rs` 已有 per-target 隔离 + 内存报告，但不落盘
- 后端无任何 delivery 持久化结构，前端无同步状态展示位
- ClipboardHeader 已带 origin_device_id + origin_device_name(为设备名解析提供基础)
- uc-application 对外只能走 facade(§11.4 铁律)
- GUI 走 in-process facade，不走 webserver

### 通过 AskUserQuestion 对齐的关键决策

1. 同步语义 → "对端确认已接收"(实际复用现有 DispatchAck，避免改 wire protocol)
2. 展示粒度 → 汇总徽章 + 详情弹层
3. 失败处理 → 只展示，不重试
4. Mobile 范围 → 不覆盖，UI 不显示

### 产出

- `task_plan.md`:4-phase 分期 (后端最小可用 → 前端 MVP → 列表汇总 → 设备名打磨)
- `findings.md`:9 个设计分叉 (D1-D9) 的初步决策与理由，4 个风险待量化项
- `progress.md`:本文件

### 下一步

(被 2026-05-14 会话取代，见下方。)

### 遇到的问题

无 (本次仅做设计，未触代码)。

---

## 2026-05-14 · 决策逐一 review + plan 重写

### 上下文

用户要求"一个一个 review"所有设计决策。逐个介绍背景、选项、影响，等用户拍板。

### 重要转向 (在 D1 review 前)

用户给出关键产品决策：**列表 UI 不展示 device / 同步信息，所有展示集中到 entry detail 视图**。

连锁影响：
- D1 倒向 A(JOIN 反查),A 无任何缺点
- D4 失效 — 整个原 Phase 3"列表汇总徽章"被砍掉
- `EntryDeliveryRepositoryPort` 不需要 `summarize_batch` 方法
- 原 4 phase 重整为 3 phase

由此触发对当前前端 detail UI 形态的研究 (见 F9):发现项目已有两套 detail UI 共存 (quick-panel `ClipboardPreviewPane` + 主窗口 `ClipboardPreview`),数据层共用 `useClipboardPreview` hook。Phase 2 改写为"扩展两处既有预览",不新建 detail 组件。

### 决策最终结论

| 编号 | 选择 |
|------|------|
| D1 | A · JOIN event 反查 |
| D2 | A · 单独定义 EntryDeliveryRepositoryPort |
| D3 | C · Pending 不持久化，facade 层合成 |
| D4 | 失效 (列表不展示) |
| D5 | facade(既定) |
| D6 | A · 新增 device_directory 表 + DeviceDirectoryPort |
| D7 | A1 · entry 表加 delivery_tracked 列 |
| D8 | 用现有 ClipboardDispatchError 五分类映射，无需新状态 |
| D9 | C · 保留行 + facade 默认不展示 |
| D10 | A · quick-panel 和主窗口都完整铺开 |

### 产出

- `findings.md` 第二版:F1-F10 事实，D1-D10 全部敲定，R1-R4 风险更新
- `task_plan.md` 第二版:3-phase 拆分 (后端最小可用 → 前端 detail UI → 设备名解析),完整 schema/port/facade/UI 草案，每个 phase 都有测试点与依赖关系

### 下一步

(被 2026-05-14 R1 基准会话取代，见下方。)

### 遇到的问题

无。

---

## 2026-05-14 · R1 写放大基准

### 上下文

用户选 b:Phase 1 实施前先量化 R1(delivery 表的 INSERT OR REPLACE 是否会成为瓶颈)。

### 预判

数学上 R1 不应是瓶颈：
- 用户复制峰值 ~10 Hz × 10 peer ≤ 100 rows/s
- SQLite WAL 在 SSD 上 raw write capacity ≈ 10000+ rows/s
- 安全系数 100×+

但跑 baseline 数据给 plan 一个可引用的证据，且能暴露真实风险点 (连接池竞争、WAL checkpoint、FK 检查 overhead)。

### 方法

Python sqlite3 写 `bench/r1_delivery_write_amp.py`:
- 与生产 `uc-infra/src/db/pool.rs` 对齐:WAL + busy_timeout=5000 + foreign_keys=ON
- 文件落盘 (非 :memory:),触发真实 fsync
- 模拟生产 schema:`clipboard_entry` 表 + `clipboard_entry_delivery` 表 + FK + index
- 模拟生产语义：每条 entry 1 次 entry INSERT + N 次 delivery INSERT OR REPLACE + 单事务 commit
- 不模拟:diesel/r2d2 池子、tokio spawn_blocking 切换、dispatch 时间分布

### 结果

| 场景 | N peer | M entry | p50 | p99 | 吞吐 |
|------|--------|---------|-----|-----|------|
| 普通用户 | 3 | 100 | 77μs | 1.5ms | 23k rows/s |
| 重度用户 | 5 | 500 | 56μs | 260μs | 84k rows/s |
| 极端用户 | 10 | 1000 | 71μs | 345μs | 120k rows/s |
| 压力测试 | 10 | 5000 | 66μs | 385μs | 122k rows/s |

### 结论

- R1 完全不是瓶颈，安全系数 ~1000×(实际峰值 100 rows/s vs SQLite raw 容量 120k rows/s)
- p99 outlier 在 ~1ms 量级，远低于键盘交互感知阈值
- WAL checkpoint 开销在长期运行下被均匀摊销 (大样本 p99 反而比小样本低)
- **Phase 1 可按 plan 实施，无需任何写入侧优化**

### 产出

- `bench/r1_delivery_write_amp.py` — 可重跑的基准脚本
- `findings.md` R1 section 更新：从"待量化"改为"已量化，无瓶颈风险",填入完整数据表
- `task_plan.md` Key Questions 第 1 项 strikethrough，引用 findings R1

### 下一步

- 选项 a:进入 Phase 1 实施
- 选项 b:还有其他 R 风险想先量化的 (R2 dispatch 回写时序 / R3 UI 刷新机制 / R4 设备名解析覆盖率)

### 遇到的问题

无。

---

## 2026-05-14 · Phase 1 实施

### 实施顺序与决策

按"领域 → 基础设施 → 应用 → wiring → 测试"6-step 任务串行推进：

1. **Task 1 · uc-core**:新增 `clipboard/delivery.rs` 领域类型 + `ports/clipboard/delivery.rs` Port + `ClipboardEntry` 加 `delivery_tracked: bool` 字段。`ClipboardEntry::new` / `new_with_active_time` 改为必填参数，强制 5 个调用点显式选择 (2 处真实新建 = true,2 处测试 helper / 1 处 mapper)。
2. **Task 2 · uc-infra**:新建 migration `2026-05-14-000001_entry_delivery`(新表 + ALTER 列);手动同步 schema.rs;新建 `EntryDeliveryRow` model 与 `DieselEntryDeliveryRepository`,照抄 `clipboard_entry_repo` 的 `DbExecutor::run` pattern;status 用 7 个字符串字面量持久化，定义 `status_codec` 子模块双向 codec;FK violation 翻译为 `EntryDeliveryError::EntryNotFound`。
3. **Task 3 · uc-application**:
   - `clipboard_capture/usecase.rs:301` 构造 entry 时 `delivery_tracked=true`
   - `facade/clipboard_history/mod.rs:335` seed_text_entry 也设 true(本机起源语义)
   - 2 个测试 helper 设 false(historical 风格)
   - `dispatch_entry.rs`:`DispatchClipboardEntryInput` 加 `Option<EntryId>`;usecase 加 `entry_delivery_repo` 字段;在 JoinSet 各 arm 内累积 `delivery_records`,fan-out 完成后串行落盘;失败仅 log 不阻塞主流程;5 种 `ClipboardDispatchError` 1:1 映射 `DeliveryFailureReason`。
4. **Task 4 · 视图 use case**:新建 `get_entry_delivery_view.rs`,实现"entry 不存在 → 报错，delivery_tracked=false → Historical，远端 entry → Remote + 空 deliveries，本机 entry → trusted_peer LEFT JOIN delivery 合成"五分支;`ClipboardEventRepositoryPort` 加 `get_source_device` 默认方法;`ClipboardSyncFacade` 加 `get_entry_delivery_view` 方法;白名单导出视图类型。
5. **Task 5 · bootstrap**:
   - assembly.rs:`DieselClipboardEventRepository` 共享 `Arc<Impl>` 双重 unsize 到 `ClipboardEventWriterPort` + `ClipboardEventRepositoryPort`;新建 `DieselEntryDeliveryRepository`;两者加入 `InfraLayer`
   - `WiredDependencies` 加 `entry_delivery_repo` + `clipboard_event_reader_repo` 旁路
   - space_setup.rs `ClipboardSyncDeps` 注入 4 个新字段 (entry_delivery_repo / entry_repo / event_repo / trusted_peer_repo)
   - CLI 路径 (AppFacade::dispatch_clipboard_snapshot / facade::dispatch_entry / e2e tests) 统一传 `None`
6. **Task 6 · 验证**:
   - `cargo check --workspace --tests` 全通过 (uc-core + uc-infra + uc-application + uc-bootstrap + uc-cli + uc-tauri + 测试代码)
   - `cargo test -p uc-infra db::repositories::entry_delivery_repo`:**6/6 pass**(record_attempt_inserts / upserts / list_by_entry / EntryNotFound / list_empty / fk_cascade)
   - `cargo test -p uc-application --lib`:**448/448 pass**,既有 dispatch_entry/facade 测试不破坏

### 关键技术点

- **`DispatchPerTarget.outcome` 类型信息丢失**:wire 层的 `ClipboardDispatchError` 被 to_string 化为 `Result<DispatchAck, String>`。Phase 1 解法是 **在 JoinSet 各 arm 内** 精确分类，提前构造 `DeliveryFailureReason`,不依赖 outcome 字符串反推。
- **测试 mock 设计**:dispatch_entry 测试有 1 个共享 helper `build_uc_with_presence_and_first_sync_state`(底层),其他 helper 透传到它。我只改它一处，加 `Arc::new(NoopEntryDeliveryRepo)` 默认参数，其他 helper 签名不动 → 测试 break 面最小。
- **facade 内三类视图 port 注入**:为避免给现有 ClipboardSyncDeps 加太多字段，我把"视图相关"的 3 个 port(entry_repo / event_repo / trusted_peer_repo) 单独标注用途，与 dispatch 用的 port 并列。
- **`ClipboardEventRepositoryPort` 默认实现**:`get_source_device` 给 default 返回 `Ok(None)`,旧 adapter / 测试 mock 自动兼容，新实现按需 override。
- **`DieselClipboardEventRepository` 双角色**:同一份 impl 同时实现 reader 与 writer port,assembly 内用 `Arc<Impl>` clone 后 unsize 到两个 trait object，避免重复构造与状态分裂。
- **`delivery_tracked` 字段定位**:本机新建 (LocalCapture / seed) 设 true,DB mapper 直接从 row 取;测试 helper 设 false 模拟历史 entry。这样视图层的 Historical 分支天然能被覆盖测试。

### 未做 (留给后续 phase)

- Phase 1 没写 dispatch_entry 的 delivery 落盘集成测试 (覆盖五种 ack/error → 五种 record 的端到端)。`cargo test` 既有 dispatch 测试都用 `entry_id: None`,验证"None 时不落盘"路径;Some 路径的覆盖留给 Phase 2 / Phase 3。

---

## 测试结果

### uc-infra · DieselEntryDeliveryRepository

```
test record_attempt_inserts_new_row ... ok
test record_attempt_upserts_existing_row ... ok
test list_by_entry_returns_all_targets ... ok
test record_attempt_on_missing_entry_returns_entry_not_found ... ok
test list_by_entry_returns_empty_for_unknown ... ok
test fk_cascade_deletes_delivery_rows ... ok

test result: ok. 6 passed; 0 failed
```

### uc-application · 全部 lib 测试

```
test result: ok. 448 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### workspace 编译

```
cargo check --workspace --tests
Finished `dev` profile [unoptimized + debuginfo] target(s) in 28.16s
```

---

## 2026-05-15 · Phase 3 实施 (策略调整)

### 上下文

按原 plan D6 准备新建 `device_directory` 表 + `DeviceDirectoryPort`,在实施前用户挑战："为什么需要新建一个表，space member 中没有吗"。

### 重新评估

调研发现 plan D6/F6 漏看了既有事实：
- `SpaceMember`(`uc-core/src/membership/member.rs:15`) 已经有 `device_name: String`
- `MemberRepositoryPort.get(device_id)` / `list()` 已能按 id 取名
- pairing 双方都会在 `redeem_invitation` / `pairing_inbound/orchestrator` 里把对端 name 写入 SpaceMember
- `revoke_member` 同时清 member 与 trusted_peer → trusted_peer ⊆ space_member 恒成立

结论：新建 device_directory 表是冗余的二号真相，违反"单一真相来源"原则。R4 提到的"仅接收过我方推送、从未推送过我方的 trusted peer 不出现在 directory" 在 SpaceMember 路径不存在。

### 决策 (D6.v2)

复用 `SpaceMember.device_name`,放弃新建 device_directory 表。已删除：
- `uc-core/src/device_directory/`
- `uc-core/src/ports/device_directory.rs`
- `uc-infra/migrations/2026-05-15-000001_device_directory/`
- `uc-infra/src/db/models/device_directory.rs`
- `schema.rs` 中的 device_directory 表条目

### 实施

1. **`GetEntryDeliveryViewUseCase`** 注入 `Arc<dyn MemberRepositoryPort>`:
   - execute 流程里 `member_repo.list()` 一次取全集，过滤空白名后建 `HashMap<DeviceId, String>` 索引
   - `EntrySource::Remote` 加 `device_name: Option<String>`,从 index 取
   - 每个 `EntryDeliveryTargetView` 加 `target_device_name: Option<String>`,从 index 取
   - member_repo 故障降级为空 index,view 仍正常返回，前端 fallback 到 id
2. **`ClipboardSyncFacade::new`** 把已有的 `deps.member_repo` 透传给 `view_uc`,无新增 dep,bootstrap 零改动
3. **uc-tauri DTO** `EntrySourceDto::Remote` / `EntryDeliveryTargetDto` 加 `deviceName` / `targetDeviceName`,转换函数同步;`cargo test -p uc-tauri --test specta_export` 自动同步生成 `src/lib/ipc-bindings.generated.ts`
4. **前端** `src/api/tauri-command/clipboard_delivery.ts` 加 `deviceName` / `targetDeviceName`;`EntryDeliverySection.tsx` + `EntryDeliveryBadge.tsx` 各引入 `deviceLabel(name, id)` 辅助函数 (空白等同缺失，fallback 到 id 截断，fallback 时保留 monospace 字体)
5. 测试：
   - uc-application 新增 3 个分支测试 (name 命中 / 空白 fallback / member_repo 故障降级),view use case 测试 10/10 通过
   - EntryDeliverySection 新增"name 优先 + 空白 fallback"测试，7/7 通过

### 校验

- `cargo check --workspace --tests` → 通过
- `cargo test -p uc-application --lib` → 477 / 477 通过
- `cargo test -p uc-infra --lib db::repositories::entry_delivery_repo` → 6 / 6 通过 (无回归)
- `bunx tsc --noEmit` → 无错误
- `bun run test src/components/clipboard/__tests__/EntryDeliverySection.test.tsx` → 7 / 7 通过

### 关键经验

plan 的事实清单要先和现有代码核对一次再决策。D6/F6 当初只看了 `TrustedPeer`(确实没 device_name),漏掉 `SpaceMember` 已经有的字段，差点新建一张冗余表。用户的"为什么不复用"挑战直接命中根问题。

### 下一步

无，Phase 3 完结。Issue #704 可关闭。

## 错误日志

| 日期 | 错误 | 解决 |
|------|------|------|
|      |      |      |
