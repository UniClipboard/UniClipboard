# ADR-010 目录同步 · 第四阶段规划（传输与接收）

Status: completed (2026-07-12)
Issue: #875 · Map: #1315（子 ticket #1316 #1317 #1318 #1319 #1320）
ADR: `docs/architecture/adr-010-directory-sync-as-file-set-manifest.md`
前序规划：`.planning/2026-07-10-adr010-directory-sync-phase2-plan.md` ·
`.planning/2026-07-10-adr010-directory-sync-phase3-plan.md`
基线假设：**阶段 3a 已合并**（PR-A `6f9f12526`、PR-B `88a70c2d4`+`f9139f53c`、
PR-C `3fa5749be`）。本文所有行号锚点相对阶段 3a 基线。

> 本阶段目标：**跨设备目录粘贴可用**（#875 核心验收）。阶段 5（UX / 进度 / 取消 /
> Sync-ineligible 展示 / `file_sync` 设置 UI / 移动端 register 兼容修复 / 文档）不在本文，
> 见 §8 Out of scope。

## 0. 实施结果（2026-07-12）

阶段 4 已完成。实现按以下提交分阶段落地：

- `a756e3a25`：接收侧目录树暂存、全有或全无重建与原子提升。
- `eeba764ae`：目录帧使用 wire version 3，旧接收端可在确认前明确拒绝。
- `bebda9ecf`：传输目录成员清单。
- `cc46e322a`：冲突后缀、权限、身份复校与目录重发。
- `6238461c9`：按目录成员独立记录传输结果与失败原因。
- `9eb100b3e`：混合选择中的顶层文件与目录保持原始形态。
- `c4f9681e2`：完整接收流程与旧版本拒绝流程验证。
- `99a9e8516`：跨卷复制回退与 Windows 权限行为验证。
- `d58ac2abe`：部分内容已到达时，重发只通过网络补齐缺失内容。

最终验证：

- `uc-application`：773 个单元测试与 10 个集成测试通过。
- `uc-core`：165 个单元测试与 19 个文档测试通过。
- `uc-infra`：556 个单元测试、12 个独立网络测试与 16 个文档测试通过；6 个既有手动/性能测试按声明跳过。
- Windows 目标的 `uc-application` 测试代码编译通过。
- `cargo check --workspace` 通过。

---

## 1. 现状盘点（阶段 3a 落地后，阶段 4 的起点）

调研已在 map #1315 charting 阶段完成，下列锚点直接引用，勿重复调研。

### 1.1 领域模型：目录结构已建，仅服务本地身份

- `crates/uc-core/src/clipboard/file_set.rs`
  - `FileSetMemberLocation { root_index, root_name, relative_path, kind }`（`:36-46`）——
    `root_name`/`relative_path` 已 NFC 归一、`/` 分隔。
  - `FileSetMemberKind { File, Executable, EmptyDirectory }`（`:49-73`），`as_tag()` → `f`/`x`/`d`。
  - `EntryFileSet::has_directory_structure()`（`:126-135`）：任一成员 `kind==EmptyDirectory`
    ‖ `relative_path.contains('/')` ‖ `line_index >= row_count`。
  - `file_set_v1_component()`（`:175-209`）：含目录时按成员逐 leaf
    (`uc_content_hash::file_set_member_v1` → `file_set_v1_wrapper`) 算结构化身份；否则 `None`。
  - `content_digest_contribution()`（`:150-172`）：含目录或含 `Excluded` → 空贡献
    （回退路径文本身份）。
- **关键事实**：以上结构 **从不上 wire**。它只喂 `SystemClipboardSnapshot.file_set_v1_component`
  参与本地 `snapshot_hash`。传输层对目录结构一无所知。

### 1.2 wire 与 header：无目录定位，且尾部扩展对旧端「静默」

- app 层 payload（`crates/uc-application/src/usecases/clipboard_sync/payload_codec.rs`）：
  - `V3BlobRef { ticket, entry_id, filename, mime, size_bytes, representation_index }`
    （`:55-63`）——**无 `root_index`/`relative_path`/`kind_tag`**。
  - `UCBS` 尾部扩展（`:41`）：`encode_snapshot_with_blob_refs_to_v3_bytes` 追加，
    `ClipboardBinaryPayload` 本体之后。**旧 envelope decoder 忽略尾部**（前向兼容），
    但 **`UCBS` decoder 自身严格**：未知 magic → `unknown V3 trailing extension`（`:212-214`），
    读完 `count` 条后仍有剩字节 → `V3 blob refs extension has N trailing byte(s)`（`:241-246`）。
  - **推论**：若在 ref 内新增字段，阶段 3a 接收端读完旧布局后撞到多余字节 → 硬报错 → **丢整帧**
    （连平铺文件都拿不到）。「加尾部字段旧端静默降级为平铺」这条前向兼容 trick 在 `UCBS` 层 **不成立**。
- infra 层 header（`crates/uc-infra/src/network/iroh/clipboard_wire.rs`）：
  - `ClipboardHeader::CURRENT_VERSION = 2`（`sync_dispatch.rs:61`）；`decode_header` 按首字节
    版本分派 `{1,2}`，其余 → `UnsupportedVersion`（`:248-252`）。`encode_header` 恒发 v2。
  - 另有 `payload_version`（=3）字段，但 `decode_header` **不校验它**——版本门在 `version` 字节。
- **ack 时机（#1316 命门）**：`crates/uc-infra/src/network/iroh/clipboard_receiver_adapter.rs`
  - `read_frame`（含 `decode_header`）失败 → `emit_ack(Rejected)`（`:160-172`）→ 发送端
    `send_and_ack` 得 `ClipboardDispatchError::PeerRejected`（`clipboard_dispatch_adapter.rs:244`）。
  - 成功后 `emit_ack(Accepted)`（`:195`）**先于** 上层 `ApplyInboundClipboardUseCase` 解 `UCBS` trailer。
  - **推论**：**header 层拒绝 → 发送端可知（PeerRejected）；trailer 层失败 → ack 已 Accepted → 对发送端静默。**
    这决定了「非静默门控」必须落在 **header 版本**，而非 trailer。

### 1.3 出站：两处目录门控

- `crates/uc-application/src/facade/clipboard_outbound/mod.rs`
  - `resolve_outbound_file_set`（`:596-685`）：`file_set.has_directory_structure()` → 返回
    `OutboundFileSetResolution::DirectoryNotYetSyncable`（`:635-637`）。
  - dispatch 消费（`:240-248`）→ `Skipped { reason: "directory_not_yet_syncable" }`。
  - `publish_file_blob_refs`（`:808-849`）：逐 `FileSyncIntent` → `publish_blob_path` → 产
    `V3BlobRef`（无目录字段）+ `file_content_digests`。
- `crates/uc-application/src/usecases/clipboard_sync/resend_entry.rs`
  - `DirectoryNotYetSyncable` → `Err(Dispatch("directory_not_yet_syncable"))`（`:397-403`），
    带 `TODO(ADR-010 phase 4)` 标记。
  - resend 恒 **全量重发**（`reconstruct_snapshot_from_entry` → `plan` → `publish_file_blob_refs`），
    无「只补缺失成员」路径；选择性只在 target/peer 粒度。

### 1.4 接收：平铺落盘，无树，partial 贴占位

- `crates/uc-application/src/usecases/clipboard_sync/apply_inbound/materializer.rs`
  - `FileCacheBlobMaterializer::materialize(from_device, receiver_entry_id, snapshot, blob_refs)`
    （`:170-614`）：**只吃 `Vec<V3BlobRef>`，无 manifest，无树概念**。
  - 落盘两模式：用户保存目录（`ReserveInboundFileTargetPort` 预留占位，adapter 加后缀）或
    受管缓存 `cache_dir/iroh-blobs/<entry_id>/<unique_filename>`。`unique_filename` 冲突 → `N-{base}`。
  - `MaterializeResult { snapshot, missing: Vec<MissingFileRef>, partial: bool }`（`:64-73`）——
    中间态 **从未持久化**、未与 `EntryDeliveryRecord` 打通。
  - `finalize_partial`（`:626-717`）：混合 `file://` 与 `uniclip-missing:///…?reason=cancelled`
    占位 URI 拼 `text/uri-list`——**平铺文件的宽松半成品路径**。
  - 成功路径不设 `snapshot.file_set_v1_component`（保持 `None`）。
- 解码入口 `decode_v3_bytes_to_snapshot_and_blob_refs`（`payload_codec.rs:140-173`）：产
  `file_content_digests: Vec::new()` + `file_set_v1_component: None`。接收侧 **从不重建 EntryFileSet**。

### 1.5 已存在的 entry 级聚合（#1317 复用地基）

- `crates/uc-core/src/ports/file_transfer.rs`
  - `EntryTransferSummary { entry_id, aggregate_status, failure_reason, transfer_ids }`（`:80-87`）。
  - `compute_aggregate_status(&[TrackedFileTransferStatus])`（`:200-220`）：优先级
    failed > transferring > pending > cancelled > completed。
  - `GetEntryTransferSummaryPort`（`:140-147`）、`RecordReceiverTransferPort`
    （`upsert_pending_transfer` / `link_transfer_to_entry`）、`FindEntryIdForTransferPort` 等。
- `file_transfer` 表（`2026-03-15-000002_upgrade_file_transfer_tracking`）：`transfer_id` PK、
  `entry_id NOT NULL`、`status`、`failure_reason`、`content_hash`、`cached_path` …
  **receiver 侧按 `entry_id` 汇总 per-file 状态的投影已就位**——「哪些成员缺失」天然可查。
- `crates/uc-core/src/clipboard/delivery.rs`：`EntryDeliveryStatus { Delivered, Duplicate,
  Unreachable, Failed{reason} }`（`:21-33`）是 **出站 per-target 结果**，不含 entry 内成员级聚合。

### 1.6 移动端（#1318 调研结论）

- `mobile_sync` 全模块不引用 `EntryFileSet`/`FileSetMemberKind`，inbound 硬编码
  `file_set_v1_component: None`（`apply_incoming.rs:775/823/868` 等）——「移动端不产出文件集」成立。
- **缺口**：出站（mobile GET）降级 **未实现**。`announce_local_activation`
  （`facade/active_clipboard/mod.rs:287-306`）无条件把 register 推进到 file-set entry；
  mobile GET（`latest_snapshot_adapter.rs:112-152`）只读 register 当前指针、无「上一个可消费值」
  概念、无 file-set 检测（`MobileSyncSnapshotPorts` 未含 `EntryFileSetRepositoryPort`）。
  目录 entry 经 `classify_for_sync`（`is_file_mime_or_format`）被判为 **File** →
  以伪 File（`data_name` = 目录路径段，`/file/{name}` 返回 uri-list 而非文件）泄漏给手机。
  这与 ADR 声明「保持上一个可消费值」不符。

---

## 2. 五张 ticket 的决策落地

| ticket | 类型 | 决策 | 依据 |
| --- | --- | --- | --- |
| #1316 协议门控 | grilling | **Header 版本门控**（§3） | §1.2 ack 时机：只有 header 版本能让旧端拒绝且发送端可知 |
| #1317 失败聚合 | grilling | **复用现有聚合**（§4） | §1.5 `EntryTransferSummary`/`file_transfer` 已能按 entry 汇总 |
| #1319 树重建 | grilling | **暂存 + 原子提升 + root 加后缀**（§5） | ADR 连带决策 2；依赖 #1316 #1317 |
| #1320 重发 | grilling | **全量重发，靠 iroh 内容寻址去重**（§6） | §1.3 现状 + iroh 字节级去重，应用层无需新代码 |
| #1318 移动端 | research | **记录缺口，修复留阶段 5**（§7） | 阶段 2 计划已把「移动端 register 兼容」列阶段 5 |

---

## 3. #1316 协议门控：Header 版本门控

### 3.1 决策

**含目录结构的帧把 `ClipboardHeader.version` 从 2 bump 到 3**；目录成员定位走 **app 层新增的自描述
尾部段**（不改动 `V3BlobRef`）。**不新建能力协商 handshake**。

理由（见 §1.2）：header decode 失败会让接收端在 ack 前回 `Rejected` → 发送端得 `PeerRejected`
→ 记 `EntryDeliveryStatus::Failed(PeerRejected)`。这满足 ADR 连带决策 2「不静默丢失（准数据丢失比
失败更糟）」，且复用现有版本分派 + ack 通道，零新增协商基础设施。trailer 层的失败发生在 ack 之后，
对发送端静默，故门控 **必须** 在 header 版本。

### 3.2 wire 格式设计

**两层，各司其职**：

1. **门控层（infra header）**：`clipboard_wire.rs`
   - `WireHeaderV3`：字段与 `WireHeaderV2` 相同（`version` 置 3 即可，无需新增字段）。`version=3`
     的语义 = 「本帧 payload 携带目录成员定位尾部段，需阶段 4 接收端」。
   - `encode_header`：dispatch 传入「是否目录帧」信号时发 v3，否则维持 v2。**纯平铺/文本/图片帧
     继续 v2，与旧端零影响**。
   - `decode_header`：接受 `{1,2,3}`；`version=3` 落 `ClipboardHeader`（新增只读标志或由上层据
     trailer 自描述判定，见下）。旧端（仅认 `{1,2}`）：v3 → `UnsupportedVersion` → `Rejected` ack。
   - `ClipboardHeader`（`sync_dispatch.rs`）：`CURRENT_VERSION` **保持 2**（默认帧仍 v2）；新增一个
     构造侧信号（如 `carries_directory: bool` 或 `min_wire_version: u8`）驱动 `encode_header` 选版。

2. **数据层（app 尾部段）**：`payload_codec.rs`，在 `UCBS` blob-refs 段 **之后** 追加 **新自描述段**
   `UCDS`（directory-set manifest v1），布局：
   ```text
   magic  "UCDS" (4B)
   count  u16 LE                       // 成员总数（含空目录）
   repeat count 次：
     kind_tag        u8               // b'f' | b'x' | b'd'
     root_is_file    u8               // 0 = 目录 root，1 = 顶层独立文件 root
     root_index      u32 LE
     root_name       u16-len + UTF-8   // NFC
     relative_path   u16-len + UTF-8   // NFC, '/' 分隔
     blob_ref_index  optional-u32      // f/x: Some(该成员内容在 UCBS 列表中的下标); d: None
   ```
   - `f`/`x` 成员经 `blob_ref_index` 指回同帧 `UCBS` 里的 `V3BlobRef`（内容 blob）；`d`（空目录）
     无 blob。这样一条帧完整描述「哪些 blob 属于哪个 root 的哪个相对路径 + 空目录骨架」。
   - **自描述**：v3 接收端读完 `UCBS` 后探测是否跟 `UCDS`；有则解成 `InboundFileSetManifest`。
     v2 接收端永远收不到 v3 帧（header 已门控），故 `UCDS` 不会撞上 §1.2 的严格拒绝。
   - 复用现有 `write_string_u16` / `read_string_u16` / `write_optional_u32` 等原语，长度上限沿用
     `MAX_BLOB_REF_STRING_LEN`；`count` 上限沿用 `MAX_BLOB_REFS`（成员数已受阶段 2 caps=2000 约束）。

### 3.3 出站开闸

- `resolve_outbound_file_set`：新增结果变体 `DirectorySyncable { paths, members: Vec<FileSetMemberLocation> }`
  取代对目录的 `DirectoryNotYetSyncable`（保留该变体给「密钥未解锁读不到 manifest」等真正不可同步态）。
- dispatch：`DirectorySyncable` → 走 `publish_file_blob_refs` 得 blob refs → 生成 `UCDS` 成员段
  （成员→blob_ref_index 映射）→ `encode_header` 发 v3 → `dispatch_snapshot_with_blob_refs`（+ UCDS）。
- resend 同源接线（§6）。
- 旧端拒绝：dispatch 得 `PeerRejected` → 现有 delivery 记录路径落 `Failed(PeerRejected)`。
  **不静默**。（可后续记忆「某 peer 拒 v3」以抑制重试，属优化，非阶段 4 必需。）

---

## 4. #1317 entry 级全有或全无失败聚合：复用现有聚合

### 4.1 决策

**复用 `EntryTransferSummary` / `compute_aggregate_status` / `file_transfer` 表**，不新建目录专属
持久化聚合状态。**目录 entry 的落盘走严格全有或全无**（区别于平铺文件的宽松半成品路径）。

### 4.2 谁判定 Failed、缺失成员是否持久化

- **判定方**：materializer 在 **落盘阶段** 做全有或全无判定（§5 暂存 + 原子提升）。目录 entry 任一成员
  fetch 失败 → **不重建树、不提升、不提交半成品快照**；已存在的 `file_transfer` 逐成员行经
  `RecordReceiverTransferPort` 标 `Failed`，`GetEntryTransferSummaryPort` 用 `compute_aggregate_status`
  滚成 entry 级 `Failed`（failed 优先级最高）。
- **缺失成员持久化**：**不新建**。`file_transfer` 行已按 `entry_id` 记 per-file 状态（pending/failed/
  completed），「缺哪些成员」直接从该投影查（`FindEntryIdForTransferPort` 反查 + 按 status 过滤）。
  这也正好喂 #1320——但 #1320 决策是全量重发（§6），故此处只需「可查」不需「可选择性驱动」。
- **与平铺文件的关系**：**同一套机制的加强版**，非新路径。差异是一个策略开关：
  - 目录 entry（`InboundFileSetManifest` 存在）→ 严格全有或全无，**禁止** `finalize_partial` 的
    占位 URI 半成品（ADR 连带决策 2）。
  - 平铺文件集（无 manifest）→ 维持现有宽松路径（`finalize_partial` 贴 `uniclip-missing` 占位）不变。

### 4.3 落点

- `materializer.rs`：`materialize` 接受可选 `InboundFileSetManifest`；present 时进入目录分支
  （§5）。目录分支 partial → 返回一个显式「entry 失败」信号（如 `MaterializeResult::failed(...)`
  或 `Err` 语义），`ApplyInboundClipboardUseCase` 据此 **不 commit 快照**、只更新 `file_transfer`
  聚合 → entry 呈 `Failed`。
- **不碰** `EntryDeliveryStatus`（那是出站 per-target 模型，§1.5）。接收侧 entry 级结果走
  `EntryTransferSummary` 投影。

---

## 5. #1319 接收侧目录树重建：暂存 + 原子提升 + root 加后缀

依赖 #1316（拿到 `InboundFileSetManifest`）与 #1317（何时算失败要回滚 / 成功可提交）。

### 5.1 落盘策略：暂存目录 + 原子提升

1. 为本次 entry 在缓存下开 **唯一暂存目录** `cache_dir/iroh-blobs/staging/<receiver_entry_id>/`。
2. 逐成员 fetch → 按 `root_index` 分组、按 `relative_path`（`/` 拆分）在暂存目录内建子目录树；
   `d` 成员建空目录；`f`/`x` 成员写文件内容（blob）。
3. **全部成员就绪后**，把每个 root 从暂存目录 **原子改名（`rename`）** 到最终目标目录下的落点；
   跨卷 rename 失败则回退「复制 + 校验 + 删暂存」，仍保持「全有才可见」。
4. 任一成员失败 → 删整个暂存目录、不提升 → entry 判 `Failed`（§4）。**永不把半成品目录暴露到最终位置。**

最终目标目录：沿用现有 `ReserveInboundFileTargetPort` 语义决定的用户保存目录；无 reserver 时落
受管缓存布局（与现状一致，只是从「平铺」变「带树」）。

### 5.2 root 冲突：加后缀 `folder (2)`

- **决策**：目标目录下已存在同名 root → 生成 `名字 (2)`、`名字 (3)`… 递增，直至无冲突。
- **冲突检测作用域**：**目标目录既有内容** ∪ **本次粘贴内多个 root 之间**（先在暂存内解决 root 间
  同名，再对目标目录既有内容解决）。
- **永不覆盖、永不合并进已有目录**（与项目安全底线一致）。后缀只作用于 **root 名**；root 内部相对
  路径保持原样。
- 实现建议：抽 `resolve_nonconflicting_root_name(target_dir, desired, reserved_in_this_paste)`，
  在提升前一次性算好所有 root 落点。

### 5.3 uri-list 改写

- 重建后，entry 的 `text/uri-list` representation 从「扁平文件 URI 列表」改写为**指向重建后各 root
  的 `file://` URI**（每个被选中的顶层 root 一条；混合选择时顶层文件走自身 URI、目录 root 走目录 URI）。
- 落点：materializer 成功路径构建 `local_file_uri_list` 时，改用 **root 落点** 而非逐叶子路径；
  复用 `is_file_list_representation` 就地改写、无则合成 `text/uri-list`（FormatId `"files"`）。

### 5.4 跨平台 exec bit

- `kind_tag = x` 成员：
  - **Unix**（macOS/Linux）：落盘后 `set_permissions` 加可执行位（`0o755` 或在原 mode 上 `|= 0o111`）。
  - **Windows**：无 unix exec 概念 → **忽略 exec bit**（文件正常写，不做特殊处理）。
- 身份一致性不受影响：`kind_tag` 已入 `file-set-v1` 身份（ADR 连带决策 1），跨设备身份稳定；仅**物理
  粘贴产物**按平台不同（Windows 无可执行位），此为平台本质差异，不可消除。

### 5.5 接收侧 file-set-v1 身份复校（可选加固，见开放问题 §9.1）

拿到全部 blob + `UCDS` manifest 后，接收端 **有能力** 重算 `file_set_v1_component`（逐成员
`file_set_member_v1` leaf → `file_set_v1_wrapper`）并与 wire `snapshot_hash` 的文件内容组件比对，
mismatch → 判损坏 → `Failed`。**默认放 PR-C 作可选加固**，不阻塞主线。

---

## 6. #1320 目录 entry 重发：全量重发，靠 iroh 内容寻址去重

### 6.1 决策

**应用层无需新代码**：现状 resend 全量重发（§1.3），iroh-blobs 内容寻址在 **字节层面** 去重——已在
接收端存在的 blob 传输被跳过，事实上实现了增量补缺；应用层语义仍是「全量重发」，但传输代价 ≈ 只补缺失。

这与 #1317 的全有或全无接收语义相容：resend 恒携带 **全部成员**（重建完整快照），接收端照常跑全有或
全无重建（§5），已存在 blob 秒级去重、缺失 blob 补传，全齐后原子提升。**接收端无需区分「本次重发只带
部分成员」**——因为重发从不只带部分。

### 6.2 落点

- 移除 `resend_entry.rs:397-403` 的 `DirectoryNotYetSyncable` gate（连同 `TODO(ADR-010 phase 4)`），
  改走 §3.3 的 `DirectorySyncable` 分支（与 dispatch 同源，禁止两套文件列表推导逻辑）。
- resend 走同一 `UCDS` 编码 + v3 header 路径。
- **不引入**「只发缺失成员」的选择性应用层逻辑（YAGNI；iroh 去重已覆盖，选择性会引入「接收端需
  区分部分重发」的复杂度，与全有或全无相冲）。

---

## 7. #1318 移动端降级缺口：记录缺口，修复留阶段 5

- **结论**：ADR 声明的「register 指向 file-set entry 时 mobile GET 保持上一个可消费值」**当前未实现**
  （§1.6）。目录 entry 会以伪 File 泄漏给手机。
- **本阶段动作**：仅 **记录**。实际修复按既定阶段划分留 **阶段 5**（阶段 2 计划 §2 表已把「移动端
  register 兼容确认」列在阶段 5）。阶段 4 聚焦桌面传输/接收主线。
- **阶段 5 修复方向备忘**（不在本阶段实现）：
  - 方案 A：register 推进侧排除——`announce_local_activation` 不把 file-set/目录 entry 推进为
    mobile 可消费指针（单一收口，但需确认不影响桌面 pull 路径 `ACTIVE_CLIPBOARD_PULL_ALPN`）。
  - 方案 B：mobile 出站 adapter 检测——`latest_snapshot_adapter` 引入 file-set 识别
    （`MobileSyncSnapshotPorts` 补 `EntryFileSetRepositoryPort` 或 `ClipboardEntry` 加标记）+
    「上一个可消费值」回退源（当前不存在，需新建）。
  - 倾向阶段 5 拍板；阶段 4 不预设。

---

## 8. PR 拆分（三个 PR，尊重 ticket DAG）

ticket DAG：#1316 #1317 为 frontier；#1319 依赖 #1316+#1317；#1320 依赖 #1317。三个 PR 在 **同一发布**
落地，故 PR 间无「已发布旧端」兼容问题（§3 的 header 门控针对的是 **真正已发布的历史版本**）。

### PR-A：接收侧全有或全无地基（#1317 + #1319 树重建核心）

纯 app（receiver 侧），不改 infra、不开出站闸。

1. 定义 `InboundFileSetManifest`（成员定位的接收侧值对象，独立于 wire codec，供测试直接构造）。
2. `materializer.rs`：`materialize` 增可选 manifest 参数；present → 目录分支：
   - 暂存目录 + 逐成员建树（`d` 建空目录、`f`/`x` 写内容）（§5.1）。
   - 全有或全无：任一失败 → 删暂存、不提升、返回「entry 失败」信号；成功 → 原子提升 + `text/uri-list`
     改写为 root 落点（§5.3）。
   - **禁止** 目录分支走 `finalize_partial` 占位 URI（§4.2）。
3. `ApplyInboundClipboardUseCase`：目录分支失败 → 不 commit 快照 + 更新 `file_transfer` 聚合
   → entry `Failed`（复用 `RecordReceiverTransferPort` + `EntryTransferSummary`，§4）。
4. 测试（port mock + tempfile）：单 root 目录重建（树形/空目录/嵌套 relative_path）/ 混合选择
   （文件 + 目录）/ 任一成员失败 → 暂存清理 + 无半成品 + entry Failed / uri-list 指向 root 落点 /
   平铺文件宽松路径回归不变（`finalize_partial` 仍走占位）。

> 说明：本 PR 出站仍门控（目录不发），目录分支靠单测喂合成 manifest 验证，属受控引入；PR-B 紧随开闸，
> 分支不在中途发布，不构成「未验证基础设施长期悬空」。

### PR-B：wire 格式 + header 版本门控 + 出站开闸（#1316）

跨 infra（header）+ app（codec + 出站）。提交按层拆（infra 一提交、app 一提交）。

1. infra `clipboard_wire.rs`：`WireHeaderV3`（复用 v2 字段，`version=3`）；`encode_header` 按
   `ClipboardHeader` 的 `carries_directory` 信号选版；`decode_header` 接受 `{1,2,3}`。
   `sync_dispatch.rs`：`ClipboardHeader` 加构造信号；`CURRENT_VERSION` 保持 2。
2. app `payload_codec.rs`：`UCDS` 目录成员段的 `write_*`/`read_*`（§3.2）+ 自描述探测；
   `encode_snapshot_with_dir_manifest_*` / decode 侧产 `InboundFileSetManifest`（PR-A 消费）。
   roundtrip + 向后兼容测试（v2 帧无 `UCDS`；v3 帧 `UCDS` 往返；成员→blob_ref_index 映射）。
3. app 出站 `clipboard_outbound/mod.rs`：`OutboundFileSetResolution::DirectorySyncable`；dispatch
   编 `UCDS` + 发 v3；旧端 `PeerRejected` → `Failed`（现有 delivery 路径，无新代码）。
4. 端到端测试：新 sender ↔ 新 receiver 目录粘贴可用；新 sender → 模拟 v2 receiver → `Rejected` ack
   → 发送端 `Failed(PeerRejected)`（非静默回归）；纯平铺帧仍 v2（旧端不受影响回归）。

### PR-C：边界 + 重发（#1319 剩余 + #1320）

1. root 冲突加后缀 `folder (2)`（§5.2，目标目录既有 ∪ 本次多 root 作用域）。
2. 跨平台 exec bit（§5.4，Unix 加位 / Windows 忽略）。
3. 移除 resend `DirectoryNotYetSyncable` gate（§6.2），同源走 `DirectorySyncable`；
   iroh 去重「事实增量」测试（重发已部分到达的目录 → 只补缺失 blob → 全齐提升）。
4. （可选）接收侧 `file_set_v1_component` 复校（§5.5 / 开放问题 §9.1）。
5. 测试：root 同名冲突（既有 + 本次多 root）/ Windows 忽略 exec / Unix 加 exec / resend 全量→去重
   增量 / 复校 mismatch → Failed（若做）。

---

## 9. 待拍板的开放问题

1. **接收侧 file-set-v1 身份复校去留**（§5.5）：收齐 blob + `UCDS` 后重算结构化身份、比对广播值，
   是纵深防御（防中间人/损坏 blob 冒充）还是过度设计？倾向 PR-C 作 **可选加固**（默认做，成本低：
   重算一次 `file_set_v1_wrapper` 比对 header hash）。若体量或时延不划算可外移。
2. **v3 拒绝的记忆/抑制**（§3.3）：首次向旧 peer 发目录 → `Failed(PeerRejected)`；是否持久化「该
   peer 不支持 v3」以抑制后续每条目录都试→Failed 的噪声？倾向阶段 5（UX 层，与 Sync-ineligible
   展示一并做），阶段 4 接受「每条 Failed」。
3. **`ClipboardHeader` 门控信号形态**（§3.2）：`carries_directory: bool` vs `min_wire_version: u8`？
   前者语义直白、后者可扩展（未来更多 wire 特性）。倾向 `min_wire_version`（可扩展，decode 侧统一按
   版本分派），PR-B 定稿。
4. **暂存目录与用户保存目录的卷关系**（§5.1）：暂存置于缓存卷、最终落用户保存目录，跨卷 rename 需
   复制回退。是否改为「暂存直接开在目标目录下的隐藏临时子目录」以保证同卷原子 rename？倾向后者（同卷
   原子性更强），但需处理目标目录不可写/权限场景，PR-A 定稿。

---

## 10. 明确不在阶段 4（Out of scope）

- **阶段 5**：进度/取消 UI、Sync-ineligible 原因展示、`file_sync` 设置界面、**移动端 register 降级
  修复**（§7）、面向用户文档。
- **阶段 5**：目录 root 提升的预留竞态（CodeRabbit PR #1323 Finding 2）。接收侧目录提升复用了面向
  文件的 `ReserveInboundFileTargetPort`：`reserve_target()` 只预留一个文件占位，promote 前先
  `remove_file` 再把目录树 `rename` 就位，中间存在一段预留路径不受保护的 TOCTOU 窗口（Minor，窗口
  极小）。正确修法是给该端口增加目录感知的预留（或让占位在提升全程保持），属跨 crate 端口设计决策，
  与阶段 5 的 reserver/UX 工作一并处理。
- **阶段 3b**：延迟身份就绪（捕获流水线异步化）+ 摄取毕 `(mtime,size)` 漂移复核——与阶段 4 正交，
  独立往后排。
- **移动端完整参与文件集同步**：ADR 范围外（桌面三平台专属）。
- **归档 blob 方案**：ADR 已否（`docs/architecture/adr-010-…`「被否掉的方案」）。
