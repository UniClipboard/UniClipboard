# ADR-010 目录同步 · 第二阶段规划

Status: draft (2026-07-10)
Issue: #875 · ADR: `docs/architecture/adr-010-directory-sync-as-file-set-manifest.md`
Phase 1: PR #1290 (merged 2026-07-10)

## 1. 第一阶段落地了什么（现状盘点）

PR #1290 只做了「manifest 基础设施 + 捕获侧写入」：

- `uc-core`：`EntryFileSet` 领域模型（`crates/uc-core/src/clipboard/file_set.rs`）
  + `EntryFileSetRepositoryPort`（整体 replace/load）。
  - `content_digest_contribution()` 是 all-or-nothing：任一 `Excluded` 行 →
    整体回退路径文本身份，与出站 `publish_file_blob_refs` 的全有或全无对齐。
  - `SizeCapExceeded` 排除原因已预留（codec/身份规则已处理），**捕获路径尚未产出**。
  - `blob_id` / `size_bytes` 为 `Option`，约定由「后续物化该行的一方」补齐——目前无人补。
- `uc-infra`：`entry_file_set` 表 + Diesel adapter。列：`entry_id, line_index,
  original_text, kind, content_hash, blob_id, size_bytes, exclude_reason`。
  **尚无 `root_index` / `relative_path` / `kind_tag` 列**（目录成员定位所需）。
- `clipboard-capture`：文件类快照 → 从 LocalFile reps 或 inline uri-list 构建
  manifest，逐文件 `hash_path`（同步、仅身份不物化 blob），entry 落库后 best-effort
  持久化 manifest。`file_uri_line` 单行解析器已抽出，capture 与 outbound 共用。

**关键缺口**：manifest 目前是「只写不读」的——出站 dispatch 仍走
`extract_file_paths_from_snapshot` 现场重新解析 reps（`facade/clipboard_outbound/mod.rs:213`）。
基础设施未被任何生产路径消费，其正确性未经端到端验证。

## 2. 整条 ADR-010 弧线的阶段划分（提议）

| 阶段 | 内容 | 用户可见性 |
| --- | --- | --- |
| 1 ✅ | manifest 模型 + 表 + 捕获写入（平铺文件集） | 无 |
| **2（本文）** | **manifest 成为出站单一真相源 + 捕获限额护栏** | 无（纯加固） |
| 3 | 目录捕获：schema 升级（root_index/relative_path/kind_tag）、目录遍历、成员边界规则、`file-set-v1` 结构入身份、延迟身份就绪 + 漂移复核 | 目录进本地历史，暂不同步 |
| 4 | 传输与接收：wire 格式携带成员相对路径、接收侧全有或全无重建目录树、root 冲突加后缀、uri-list 改写、协议版本门控 | **跨设备目录粘贴可用（#875 核心验收）** |
| 5 | UX 与收尾：进度/取消、Sync-ineligible 原因展示、`file_sync` 设置 UI、移动端 register 兼容确认、文档 | 完整体验 |

排序理由：

- **读取方先于遍历**：dispatch 改读 manifest 是对第一阶段的端到端验证
  （tracer bullet），且接收侧重建（阶段 4）本来就要求发送端以 manifest 为真相源。
  在未被消费的基础设施上继续堆目录遍历，等于扩大未验证面。
- **限额先于遍历**：平铺文件集很少触顶 2000 成员/1 GiB，目录会轻易触顶。
  护栏必须在成员数被目录放大之前就位（ADR 连带决策 3）。
- **遍历与身份升级不可拆**：无结构入身份的目录 entry 会与同名文件集错误碰撞，
  两者必须同一阶段落地（ADR 连带决策 1）。
- **阶段 3 与 4 之间需要 dispatch 门控**：接收端尚不认识目录成员之前，含目录的
  entry 必须判 Sync-ineligible（可观测原因，不静默），避免旧接收端错误消费。

## 3. 第二阶段范围（两个 PR）

### PR-A：manifest 成为出站路径的单一真相源

1. **dispatch 读 manifest**：`clipboard_outbound` 文件类计划构建时，优先
   `EntryFileSetRepositoryPort::load(entry_id)`；`File` 行经 `file_uri_line`
   还原路径参与 `publish_file_blob_refs`。
   - manifest 缺失（存量 entry、或 phase-1 的 best-effort 写入失败）→ 回退现有
     `extract_file_paths_from_snapshot`，并记 WARN（可观测回退率）。
   - manifest 含 `Excluded` 行 → **fail fast**：直接以可观测错误终止该 entry 的
     出站（与 all-or-nothing 语义一致），不再等 publish 阶段撞 I/O 错误。
2. **重发路径同源**：manual resend 与 dispatch 走同一条 manifest 读取路径，
   禁止并行保留两套文件列表推导逻辑。
3. ~~**blob_id / size_bytes 回填**~~ **实现时裁掉**：出站 publish 走的是
   iroh-blobs 传输通道（产出 `BlobDigest` + ticket，落 `blob_reference` 表），
   不是 manifest `blob_id: Option<BlobId>` 所指的本地 `EncryptedBlobStore`
   句柄——把 iroh digest 写进 `blob_id` 会污染语义。该字段仍留给未来真正把
   成员物化进本地 blob 仓库的路径（接收侧重建 / 本地缓存）补齐。
4. 测试：manifest 命中（优先于 rep 重解析、两种 original_text 形态、排序去重）
   / 缺失与读错回退 / Excluded fail-fast（dispatch Skipped + resend PayloadLost）
   / manifest 成员在发送时刻不可读 → all-or-nothing 拦截。
5. 实现中追加的语义决定（超出原计划）：
   - **manifest 命中时成员丢失也全有或全无**：capture 后、send 前成员被删 →
     整条 entry 不出站/不可重发，不再静默发子集（旧回退路径保留宽松语义）。
   - **漂移可观测**：publish 后对比 wire digests 与 manifest 捕获时刻贡献值，
     不一致记 WARN（阶段 3 的 (mtime,size) 漂移复核先有观测基线）。

### PR-B：捕获限额护栏（SizeCapExceeded 落地）✅ 已实现

1. **设置**：`FileSyncSettings` 新增
   `max_file_set_total_bytes`（默认 1 GiB）、`max_file_set_member_count`（默认 2000），
   `#[serde(default)]` 缺省回退（沿用 issue #581 的兼容模式）。与既有 `max_file_size`
   （单文件 5 GiB）语义正交：前者限文件集总量/成员数，后者限单成员。**暂不经
   daemon settings DTO 暴露**（`FileSyncSettingsDto` 不加字段）——设置面板留到阶段 5，
   现阶段只作为 core 内部、可通过 settings 文件手改的护栏。
2. **捕获预检**：`build_entry_file_set` 前置毫秒级元数据预检
   （`file_set_exceeds_caps`：先查成员数，再 `metadata()` 累加 size，不读内容）；
   超限 → 全部文件行标 `EntryFileSetExcludeReason::SizeCapExceeded`，**跳过逐文件哈希**。
   成员数上限 `N` 语义为「> N 才超」（恰好 N 放行）。
3. **只对「全部为可 stat 的普通文件」生效**：任一成员不可测（缺失/目录/stat 失败）
   → size 判定保持 `false`，落到逐行哈希由其自然标 `IngestFailed`（同样触发路径文本
   回退），避免 SizeCapExceeded 与 IngestFailed 语义打架。
4. **caps 读取**：capture 仅在文件类分支 `settings.load()` 读一次；load 失败 →
   `FileSetCaps::unbounded()`（不因瞬时 settings 错误静默停掉文件同步）。文本/图片
   捕获热路径不付这次读取。`SettingsPort` 穿线进 `CaptureClipboardUseCase`，4 个装配点已接。
5. 行为语义：超限集合仍正常进本地历史（身份=路径文本，与 phase 1 的
   IngestFailed 路径一致）；出站被 PR-A 的 Excluded 拦截（dispatch `Skipped`），原因可观测。
   **已知边界**：若 phase-1 的 best-effort manifest 持久化失败，dispatch 走 PR-A 的
   rep 重解析回退，此时 caps 不生效（可用性优先，与 manifest-missing 回退一致）。
6. **行为变更（生效）**：>2000 成员或 >1 GiB 的平铺复制此前会尝试出站，现被判超限
   不出站（仍进本地历史）。按开放问题 #1 的倾向直接生效，`file_sync` 设置文件可调。
7. 测试：成员数超限（跳哈希，用 panic-on-hash 证明）/ 恰好在界 / 总字节超限（真实
   tempfile）/ 界内 / 不可测成员落 IngestFailed 而非 SizeCapExceeded / LocalFile 形态
   同样受成员数上限约束 / settings 缺省与往返。

### 明确不在第二阶段

- 目录检测/遍历、`root_index`/`relative_path`/`kind_tag` schema 列（阶段 3）。
- `file-set-v1` 结构入身份、延迟身份就绪、(mtime,size) 漂移复核（阶段 3）。
- wire 格式、接收侧重建、uri-list 改写（阶段 4）。
- Sync-ineligible 的 UI 展示与设置界面（阶段 5；阶段 2 只保证日志/错误可观测）。

## 4. 待拍板的开放问题

1. **超限平铺文件集的出站行为是行为变更**：今天一个 3000 文件的平铺复制会尝试
   出站（可能成功）；PR-B 后会被判超限不出站。默认值 2000/1 GiB 是否需要放宽，
   或首发只对「含目录」集合生效？（倾向：直接生效，`file_sync` 设置可调，
   否则护栏在阶段 3 前形同虚设。）
2. **阶段 3 的 dispatch 门控形式**：Sync-ineligible 标记（简单、需要 entry 级
   持久化原因字段）vs 协议能力协商（复杂、但阶段 4 可能反正需要）。建议阶段 3
   先用 Sync-ineligible，阶段 4 再评估版本门控。
3. **延迟身份就绪（Deferred snapshot identity）的落点**：ADR 把它列为捕获护栏，
   但它是独立的大改动（捕获流水线异步化、就绪前不广播）。若阶段 3 体量过大，
   可拆为 3a（同步遍历 + 身份，靠限额压住时延）/ 3b（异步化）。
