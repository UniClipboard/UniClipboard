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
3. **blob_id / size_bytes 回填**：`publish_file_blob_refs` 物化成功后，
   load → 补齐对应行 → save（整体 replace 语义，port 不加新方法）。
   兑现 phase 1 在模型注释里许下的「由物化方补齐」契约。
4. 测试：manifest 命中 / 缺失回退 / Excluded fail-fast / 回填后再读一致性。

### PR-B：捕获限额护栏（SizeCapExceeded 落地）

1. **设置**：`FileSyncSettings` 新增
   `max_file_set_total_bytes`（默认 1 GiB）、`max_file_set_member_count`（默认 2000），
   serde 缺省回退（沿用 issue #581 的兼容模式）。与既有 `max_file_size`
   （单文件 5 GiB）语义正交：前者限文件集总量/成员数，后者限单成员。
2. **捕获预检**：`build_entry_file_set` 前置毫秒级元数据预检
   （`metadata()` 取 size，不读内容）；超限 → 全部候选行标
   `EntryFileSetExcludeReason::SizeCapExceeded`，**跳过逐文件哈希**
   （身份反正回退路径文本，读 1 GiB 内容纯浪费，也是 ADR「热路径只做元数据预检」
   的要求）。
3. 行为语义：超限集合仍正常进本地历史（身份=路径文本，与 phase 1 的
   IngestFailed 路径一致）；出站被 PR-A 的 Excluded fail-fast 拦截，原因可观测。
4. 测试：预检超量（总大小/成员数各一）、恰好在界内、设置缺省回退。

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
