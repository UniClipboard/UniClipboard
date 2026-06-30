# 统一搜索接口 + Tag 化内容分类（废弃 list 端点）设计文档

- 日期：2026-06-25
- 状态：v8 — **codex 评审通过**（round-8 APPROVED；8 轮共 29 finding 全部处理，0 拒绝）；可据此拆分 phase 与 commit
- 范围：剪贴板历史的「浏览」与「搜索/过滤」两条数据路径统一为单一搜索接口；把 `link` 从 `content_type` 枚举值重构为派生 **tag**、把 `html` 升为独立 `content_type`、删除误导性的 `code` tag；为用户自定义 tag 预留扩展点；最终废弃 list（浏览）端点。

---

## 1. 背景与动机

### 1.1 触发问题

历史页点击「图片 / 文件」过滤后，卡片显示成图片/文件的 URL 文本而非缩略图/文件卡片。已先行做了渲染层降级修复（`src/components/history/HistoryCard.tsx`），但那只是症状。

### 1.2 根因：两条丰富度不同的数据路径

历史列表有两个数据来源，由 `src/hooks/useHistoryData.ts` 的 `isSearchActive` 切换：

- **浏览态**（无任何过滤）：`displayItems` ← Redux `clipboard.items` ← `GET /clipboard/entries`（list 端点）← `EntryProjectionDto`（**完整投影，带结构化 content**）。
- **搜索/过滤态**（任意 content-type / source / time / 关键词过滤）：`searchResults` ← 搜索引擎 ← `SearchResultDto`（**轻量命中，无结构化 content**）。`mapSearchResult` 把 `content` 填 `null`，仅留 `textPreview`。

卡片渲染按「有完整 content」的前提编写，搜索态 `content` 为 `null` 时掉进只显示一行文本的 fallback —— 图片/文件因此显示成 URL。

**之所以一选过滤就走搜索**：list 端点不支持 content-type/source/time 过滤（`useHistoryData.ts` 注释明确），过滤只能走搜索引擎。

### 1.3 目标

从根因出发，让搜索接口成为剪贴板历史的 **唯一数据源**，浏览 = 空查询搜索，最终删除 list 端点，消除「两条并行路径」这一不一致之源（契合根 `AGENTS.md`：单一真相源、不保留无清理计划的新旧并行逻辑）。

### 1.4 领域模型升级（本设计的核心）

用户明确了未来场景，纠正了「类型」这一维度的建模。**划分标准**：一份内容是「独立的数据/MIME 形态」→ `content_type`（物理）；是「text 内容命中某规则」→ `tag`（派生）。

- **content_type（物理，单值）**：`text / image / file / html / other`。决定主体渲染与存储。`html`（富文本）是 `text/html` MIME 这一独立数据形态，故属物理类型，**不是** tag。
- **tag（派生，多值 0..N）**：`link / favorited / <custom>`。
  - `link`：规则派生 tag，判定 **对齐现状 list projection 的 link 检测**——覆盖 `text/uri-list` 中的 web URL **与纯文本中的 web URL**（移植 list 现状判据）。注意：搜索现状 `infer_content_type` 对 `text/plain` 直接判 Text、**不检测 URL**，故现状 search 比 list 漏了纯文本 URL 的 link 识别；统一到搜索须把 list 的 link 检测移植过来，否则丢能力。渲染元数据 `linkUrls` 与 `link` tag **同源同判据**，不存在「渲染成链接卡却过滤不到」的割裂。**精确判据见 §4.3 的 `detect_link_urls` 单一契约**（纯文本仅当 trim 后整段是单个 web URL、或每个非空行都是 web URL 才算；散文中夹带 URL 不算）。
  - `favorited`：用户手动状态（`user_state` 类）；真相源在主存储，不在搜索索引（见 §4.0）。
  - 未来用户可自建规则 → 自建 tag。
- **`code` tag 删除**：旧模型把 `text/html` 硬做成 `code` 规则 tag，是承袭「前端把 html 显示成代码块」的历史 UI 决定而非数据本质。html 升为 content_type 后该伪规则消失，**规则引擎 MVP 只剩一个 `LinkRule`**。

即：**`content_type`（物理，5 值）与 `tag`（派生，多值可扩展）是两个正交维度。**

---

## 2. 现状（精确落点）

### 2.1 扁平的 6 值 ContentType

```rust
// crates/uc-core/src/search/document.rs:14
pub enum ContentType { Text, Html, Link, File, Image, Other }
```

`Link` 应降为 tag；`Html` 保留为独立物理类型（语义不变，只是不再与 tag 混居）。

### 2.2 分类判定落点（后端，非前端）

```rust
// crates/uc-application/src/facade/search/projection.rs:22
fn infer_content_type(mime: &str, uri_list: &[String], has_file_paths: bool) -> ContentType
// image/* => Image；text/html => Html；text/uri-list(file://) => File；
// text/uri-list(http/https) => Link；text/plain => Text；else => Other
```

由 `SearchableContent::into_pipeline_input`（projection.rs:161-184）在构造 `SearchPipelineInput` 时调用。前端 `mapSearchContentType` 只做名称映射。**改造落点集中在后端投影 + 索引 schema，前端跟随。**

### 2.3 索引 schema（派生索引，可整体重建）

`search_document` 表列（active 表与 rebuild temp 表同构，`crates/uc-infra/src/search/sqlite_index.rs:470-483`）：

```
profile_id, entry_id, event_id, active_time_ms, captured_at_ms,
file_type (= content_type 编码), file_extensions, mime_type,
indexed_at_ms, index_version, text_preview
```

`search_posting`（HMAC 加密倒排，:489-498）：`profile_id, term_tag(BLOB), entry_id, field_mask, term_freq`。

**关键迁移机制**：`search_document.index_version` 与常量 `CURRENT_INDEX_VERSION` 不匹配时，搜索自动 `mark_blocked` 并触发全量 rebuild（:846-855）；首次/未完成时触发 `initial_backfill`（`crates/uc-application/src/facade/search/coordinator.rs`）。**搜索索引是 entry 主存储的派生物，schema 变更只需 bump version + 改建表语句，重建自动发生。** 正因为它可被整体重建，**任何真相数据都不能只存在于此**（见 §4.0）。

### 2.4 搜索执行：浏览语义已具备（一手验证）

`sqlite_index.rs:795-999`：空查询 → `is_filter_only`（:801）→ `load_all_documents` 全量加载（:863）；统一排序 `active_time_ms DESC`（:952-959）；分页在排序后（:962-969），`total` 权威；filter-only 不需 search key（:804）。

→ 「空查询 = 全量、时间倒序、稳定分页」引擎层已就绪。**但全量加载进内存（非 SQL 下推）是性能债，见 §5 Phase 2 前置门槛。**

### 2.5 查询模型缺 tag 维度

```rust
// crates/uc-core/src/search/query.rs:39
pub struct SearchQuery { query_string, operator, time_range,
  content_types: Vec<ContentType> /* 当前含 Link/Html */, extensions, source_devices, limit, offset }
// 无 tags 字段
```

### 2.6 索引未就绪的兜底缺口

`search()` 在 block / 版本不匹配 / rebuild 期间返回 `IndexNotReady`（:841-855）。list 直读 SQLite 永远可用。**这是 list 唯一搜索语义上无法自然替代的职责。**

---

## 3. 目标领域模型

```
content_type (物理，单值)   text │ image │ file │ html │ other
        · html = text/html 富文本（独立 MIME 形态，非 tag）
        · other = internal-only 兜底，不对外暴露为过滤项（§4.4）

tag (派生，多值 0..N)       link │ favorited │ <custom...>
        · builtin_rule:  link              （web URL：uri-list + 纯文本单/多行 URL，对齐 list；MVP 唯一规则）
        · user_state:    favorited         （用户手动；真相源在主存储，§4.0）
        · custom_rule:   用户自建           （未来；定义存主存储，§4.0）

过滤 = content_types[] ⊥ tags[]   组内 OR、跨组 AND（§4.4）
```

### 3.1 已确认决策

| # | 决策 | 选择 | 理由 |
|---|---|---|---|
| 1 | `html`/`code` 建模 | 删除 `code` tag；`html` 升为独立 `content_type`；真正的代码检测延后到自定义规则 | html 是独立 MIME 形态而非「text 命中规则」；规则引擎 MVP 因此只剩 `LinkRule` |
| 2 | favorited 归属 | 并入 tag 体系作内置 `user_state` tag 供过滤；**真相源 = 主存储专门的 entry user-state owner**（独立于 representation-selection），`entry_tag` 仅镜像（§4.0/§4.3） | 可变状态真相源不放在可被重建的派生索引里 |
| 3 | tag 定义与加密边界 | 内置 tag id = **代码常量**（不持久化定义）；自定义 tag 定义（名/规则）= **主存储**（Phase 4），不入搜索索引；`entry_tag` 仅 `(entry_id, tag_id)`；自定义 tag 的过滤/列举/计数/结果输出均需 **解锁会话**（§4.6） | tag 定义是真相，不能随索引 drop；隐私不可从锁定态反推 |
| 4 | 结构化字段补齐 | **混合**：轻量元数据入索引；完整文本/图片二进制前端按 entryId 懒加载；每个 kept 字段标注 live+rebuild 来源（§4.5） | 卡片自洽又不让索引承载重 payload |
| 5 | 索引未就绪兜底 | 无过滤浏览走主存储兜底（`state=degraded`）；带过滤/带词查询重建期返回稳定非 200 `index_rebuilding`（§4.7） | 杜绝删 list 后过滤结果静默错误 |
| 6 | 推进方式 | 落盘本设计 → codex 评审 → 分阶段实现 | tag 化引入实质架构决策 |

---

## 4. 详细设计

### 4.0 分层真相源原则（贯穿全设计）

搜索索引是 **派生物**（version bump 时 drop/recreate + backfill）。据此：

| 数据 | 真相源 | 搜索索引中的角色 |
|---|---|---|
| 内置 tag 定义（link/favorited 的 id、kind） | **代码常量**（编译期固定） | 无需持久化定义；`entry_tag` 直接引用常量 id |
| 自定义 tag 定义（名、规则配置） | **主存储专门表/port**（Phase 4） | 不入搜索索引；索引只存成员关系 |
| favorited 用户状态 | **主存储 entry user-state owner** | `entry_tag` 中的 favorited 行 = 镜像，从真相源重建 |
| payloadState（Lost） | entry 主存储 representation state | `search_document.payload_state` 列 = 镜像；payload 变 Lost 时 **write-through** 更新，rebuild 回填 |
| tag 成员关系（entry↔tag） | 由「内容 + 规则」或「user-state」**可重算** | `search_entry_tag`，纯派生，drop/recreate 安全 |
| 渲染元数据（尺寸/大小/名/lost…） | entry 主存储 representation | 索引列镜像，rebuild 时从主存储重算（§4.5 来源矩阵） |

一句话：**搜索索引只放可重算的派生数据；任何用户真相（自建定义、收藏状态）都在主存储。**

### 4.1 core 模型（`uc-core`）

```rust
pub enum ContentType { Text, Image, File, Html, Other }  // html 独立形态；other 仅内部兜底

pub struct TagId(String);                     // 内置用保留常量（"link"/"favorited"）
pub enum TagKind { BuiltinRule, UserState, CustomRule }

pub struct TaggableContent<'a> {              // 规则求值的领域中性输入
    pub content_type: ContentType,
    pub uri_list: &'a [String],
    pub plain_text: Option<&'a str>,          // LinkRule 检测纯文本中的 web URL（对齐 list）
}
pub trait TagRule: Send + Sync {              // tag 生产者（内置在 app/infra 实现）
    fn tag_id(&self) -> &TagId;
    fn evaluate(&self, content: &TaggableContent<'_>) -> bool;
}
```

`SearchDocument`：`content_type` 改 5 值（含 Html）；新增 `tags: Vec<TagId>` + 渲染元数据（§4.2）。tag **定义** 不进 core 持久模型（内置=常量）。

### 4.2 索引 schema（`uc-infra`，bump `CURRENT_INDEX_VERSION` → active 表 drop/recreate）

**迁移方式（round-1 F-12 / round-4 F-1）**：搜索表是派生索引，但 **现状机制不会因 version bump 自动改表结构**——version mismatch 触发的 rebuild 只 DELETE active 表行、再把 temp 行拷回（重灌数据，**保持旧表结构**），既不加新列也不会建 `search_entry_tag`。因此 **必须新增显式的 schema 迁移 / 启动时 schema reconciliation**：在 rebuild cutover 之前 drop+recreate search-owned active 表与索引、建 `search_entry_tag`。下列 `CREATE TABLE` 为最终形态。**凡手写这些表列名的 SQL 都须一并同步**（round-5 F-1：漏一处则重建后该列变空）：① 新增的 active 表迁移/重整 DDL；② `create_rebuild_tables` temp DDL（:470-483）；③ `insert_temp_entry` temp INSERT 列（:560-580）；④ **`finalize_rebuild` 把 temp 拷回 active 的 `INSERT…SELECT` 列清单（手写列名）**；⑤ bump `CURRENT_INDEX_VERSION`。**升级测试必须覆盖：旧库升级 → 重建一批带 sourceDevice / 图片尺寸 / 文件名 / 链接的记录 → 确认重建完成后这些字段仍保留**（不能只测 fresh database）。

```sql
CREATE TABLE search_document (
    profile_id      TEXT NOT NULL,
    entry_id        TEXT NOT NULL,
    event_id        TEXT NOT NULL,
    active_time_ms  INTEGER NOT NULL,
    captured_at_ms  INTEGER NOT NULL,
    file_type       TEXT NOT NULL,              -- content_type: text/image/file/html/other
    file_extensions TEXT NOT NULL DEFAULT '[]',
    mime_type       TEXT NOT NULL DEFAULT '',
    source_device   TEXT,                        -- 来源设备（实时匹配判定 + 过滤，§4.5/§4.8）
    indexed_at_ms   INTEGER NOT NULL,
    index_version   TEXT NOT NULL,
    text_preview    TEXT,
    payload_state   TEXT,                        -- NULL | 'Lost'
    image_width     INTEGER,                     -- nullable
    image_height    INTEGER,
    file_sizes      TEXT,                        -- JSON array, nullable
    file_names      TEXT,                        -- JSON array, nullable
    link_urls       TEXT,                        -- JSON array, nullable
    PRIMARY KEY (profile_id, entry_id)
);

-- entry ↔ tag 成员关系（纯派生镜像；drop/recreate + backfill 安全）
CREATE TABLE search_entry_tag (
    profile_id TEXT NOT NULL,
    entry_id   TEXT NOT NULL,
    tag_id     TEXT NOT NULL,          -- 内置常量 id；（自定义 id 引用主存储定义，Phase 4）
    PRIMARY KEY (profile_id, entry_id, tag_id)
);
CREATE INDEX idx_entry_tag_by_tag ON search_entry_tag (profile_id, tag_id);
```

**注意（round-2 F-1）**：**不在搜索索引建 tag 定义表**。内置 tag 是代码常量；自定义 tag 定义（名/规则）属用户真相，Phase 4 建在主存储（独立表/port），搜索侧只持有 `entry_tag` 成员关系。

**硬删除一致性**：entry 删除时一并删 `search_document` + `search_posting` + `search_entry_tag`（扩展 :248-279 删除路径）。

**rebuild 全流程纳入 `search_entry_tag`（round-3 F-3）**：现状 rebuild 只建 doc + posting 两张 temp 表（`create_rebuild_tables`, :462-516）并原子 cutover。新增 `search_entry_tag` 必须同样纳入，否则重建后 tag 行会丢失或与新 doc 失步：① 建 `tmp_search_entry_tag_rebuild_*` temp 表；② `insert_temp_entry` 同时写该 entry 的 tag 行（与 doc/posting 一致地幂等替换）；③ rebuild 期间的 live `index_entry`/`delete` 镜像也写 temp tag 表；④ 最终 **三表（documents / postings / entry_tags）一起原子 cutover**。

### 4.3 规则引擎 + 真相源（round-1 F-2/F-5、round-2 F-2/F-3）

- `infer_content_type` **拆两半**：① `infer_physical_type(...) -> ContentType`（`text/html`→Html，5 值）；② `evaluate_tags(&TaggableContent, &[Box<dyn TagRule>]) -> Vec<TagId>`。
- 内置规则（MVP 唯一）：`LinkRule`，依赖 **单一契约 `detect_link_urls(uri_list, plain_text) -> Vec<Url>`**（round-7 F-1）；`link` tag 与渲染 `linkUrls` **都调它**（同源，§4.5），parity 测试保证一致。契约精确定义（移植现状 list projection）：
  - 允许 scheme：`http` / `https`。
  - `text/uri-list`：取其中的 web URL 行。
  - `text/plain`：**仅当** trim 后整段是单个 web URL，**或** 每个非空行都是 web URL，才算；**散文中夹带 URL 不算**（负例须有测试）。
  - 命中 → 同一次调用产出 `link` tag + `linkUrls` 列表。

  **无 CodeRule。**
- 求值时机：`SearchProjectionBuilder::into_pipeline_input`（projection.rs:161）算完物理类型后跑规则 → `SearchPipelineInput.tags` → 落 `file_type` + `search_entry_tag`。

**content_type 的权威来源（round-6 F-1）**：一条 entry 可能多 representation（浏览器复制常同时有 `text/plain` + `text/html`）。`infer_physical_type` 的 MIME 输入须取 **primary/paste representation** 的 MIME（内容主体形态），**不是 preview representation**——preview 偏好 plain text，会把富文本误判成 `text`，令 `html` 过滤漏掉真实富文本。preview 文本仍来自 preview rep。须加 `text/plain + text/html` 的 live/rebuild parity 测试（应判 `html`）。

**favorited 真相源（F-2 ×2）**：真相源 = 主存储 **专门的 entry user-state owner**（entry 元数据或独立 user-state 表/port），**不是** `clipboard_selection`（后者管 representation 选择，语义无关）。`search_entry_tag` 的 favorited 行是镜像：toggle 用例双写真相源 + 镜像；rebuild 从真相源回填。**前置依赖**：favorite 持久化能力须就绪（现状 toggle 为 stub，是本 tag 的前置工作项）。

**rebuild 输入完整性（升级为全字段，见 §4.5）**：link tag 与所有渲染元数据列在 rebuild 时都需可靠来源；任何 rebuild 拿不到原始输入的字段不得依赖索引重建，须保持 lazy。这是 Phase 0 gate。

**存量/新规则回填**：用户新增/改规则 → 复用 `initial_backfill`/rebuild 对存量重新求值（Phase 4）。

### 4.4 搜索查询 + 过滤语义（round-1 F-3/F-4、round-2 F-6）

```rust
pub struct SearchQuery { query_string, operator, time_range,
  content_types: Vec<ContentType> /* text/image/file/html；other 非公开过滤项 */,
  tags: Vec<TagId> /* link/favorited/custom */, extensions, source_devices, limit, offset }
```

- **布尔语义**：组内 OR（多 content_type / 多 tag 任一匹配）；跨组 AND（type ∧ tag ∧ source ∧ time）。AND-of-tags 延后。
- **`Other`**：internal-only，不渲染为过滤 chip（公开 content_types = text/image/file/html）；渲染层仍处理 `other`（通用卡片）。
- **tag id 校验（F-6）**：请求中的 `tags` 必须校验——未知 id 拒绝；**任何 custom tag id 的过滤要求解锁会话**（锁定态只接受内置 tag id）。
- 执行：现有 filter 链（:884-950）后加 tag 过滤 `entry_id ∈ (SELECT entry_id FROM search_entry_tag WHERE profile_id=? AND tag_id IN (...))`，分页前完成以保 `total` 权威。
- `GET /search/tags`：列 tag + 计数（隐私见 §4.6）。

### 4.5 DTO 字段 + 来源矩阵（round-1 F-9、round-2 F-4/F-5）

每个 kept 渲染字段标注 live capture 与 rebuild 两路来源；rebuild 无可靠来源者保持 lazy。**删 list 前须做 search vs list-projection 逐字段对照测试。**

| 渲染字段 | list 现状 | search 方案 | live 来源 | rebuild 来源 |
|---|---|---|---|---|
| id/activeTime/contentType | ✅ | keep（5 值） | snapshot | entry repo |
| tags（link/favorited/custom） | — | **新增** | 规则求值 / user-state | 规则重算 / user-state 真相源回填 |
| sourceDevice | ✅ | **新增**（索引列，F-5） | clipboard event store（按 event_id） | clipboard event store（按 event_id，与 live 同路径）+ parity test |
| textPreview | ✅ | keep | preview rep | preview rep |
| isFavorited | ✅ | 由 `tags` 含 favorited | user-state 真相源 | 同左回填 |
| payloadState (Lost) | ✅ | keep（列，**write-through**） | paste_rep state | representation state 回填 |
| imageWidth/Height | ✅ | keep（列） | image rep | **待验证**：rebuild 能否读尺寸；否则 lazy |
| fileSizes/fileNames | ✅ | keep（列） | uri-list rep | **待验证**：同上 |
| fileExtensions | ✅ | keep | 派生自名 | 派生自名 |
| linkUrls | ✅ | keep（列）；domains 前端派生 | list link 检测（uri-list + 纯文本 URL，与 tag 同源） | **待验证**：同路径 |
| mimeType | ✅ | keep | rep | rep |
| fileTransferStatus/Reason | ✅ | **lazy** ← `fileTransferSlice` | 实时 slice | 实时 slice |
| 完整文本 (hasDetail+content) | 按需 | **lazy** `getEntryDetail` | — | — |
| 图片二进制 | resource | **lazy** `getClipboardEntryResource` | — | — |

「**待验证**」项进入 Phase 0 gate：若 rebuild 路径（`representation_repo`）拿不到原始尺寸/大小/uri，则该字段降级为 lazy（按 id 补查），不依赖索引重建。

### 4.6 隐私模型（round-1 F-6、round-2 F-6）

- 内置 tag（`link`/`favorited`）：固定保留 id，语义公开，列举/计数/过滤在锁定态亦可（filter-only 不需 key）。
- 自定义 tag：`tag_id`=uuid，名加密存主存储。**锁定态：完全不可见**——不列举、不计数、**不接受按 custom tag id 过滤**、**不在结果 `tags` 中输出 custom id**（解锁后才输出）。`search_entry_tag` 仅 `(entry_id, tag_id)`，脱离主存储加密定义无法反推语义。
- 即：custom tag 的任何读写路径都 gate 在解锁会话之后；锁定态的搜索世界只有内置 tag。

**锁契约（round-3 F-1）**：引擎层 filter-only 不需 search key（§2.4），但 **上层 `/search/query` 路由/use case 当前可能对锁定会话整体拒绝**。删 list 前必须按 query 类型拆分该 guard，使其与引擎能力一致：

- **空浏览 + 仅内置 tag/类型/时间/来源过滤**：绕过 search-key guard，锁定态可返回。
- **关键词搜索 + 任何 custom tag 访问**：需解锁会话。

落地前需核实并相应调整 `/search` 路由/use case 的锁定 guard；作为 **Phase 0/1 验收测试**（锁定态空浏览/内置过滤能返回，关键词/custom 被拒）。否则删 list 后锁定态浏览会回归。

### 4.7 索引未就绪兜底 + wire 契约（round-1 F-1、round-2 F-7）

`search()` 检测 `IndexNotReady`（block/version-mismatch/rebuild 中）时按 query 类型分流，**契约明确**：

- **无过滤、无关键词（纯浏览）**：内部降级直读主存储，返回 **HTTP 200**，body `state: "degraded"` + 正常 items/total/has_more。
- **带 content_type/tag/source/time 过滤或带关键词**：返回 **稳定非 200 错误** `index_rebuilding`（如 503 + 错误码），前端展示「索引重建中，过滤暂不可用，可清除过滤浏览全部」。
- 正常路径：HTTP 200，`state: "ready"`。

响应 DTO 增 `state: "ready" | "degraded"` 字段（仅成功响应）；`index_rebuilding` 走错误通道（与既有 `IndexNotReady` 错误码对齐）。**四处同步**：OpenAPI spec、generated SDK、CLI、前端处理。降级须先于/随 Phase 0 重建落地。

**degraded 浏览路径的归属（round-7 F-2）**：降级直读用的是 **搜索 facade 内部拥有的主存储浏览投影**（把 list 的核心读逻辑下沉/重命名为搜索 facade 私有的兜底读路径），**不是** 公开 list 端点/DTO/usecase。因此 Phase 4 删 list 删的是 **公开路由 + DTO + SDK 契约**，内部浏览投影 **保留**。验收测试：公开 list 端点已删 + 索引 blocked + 空浏览仍返回 degraded 结果。

### 4.8 实时更新规则矩阵（round-1 F-8、round-2 F-5）

搜索结果列表订阅 `clipboard` WS 事件。**匹配判定所需字段（contentType/tags/activeTime/sourceDevice）现已在 SearchResult 中（§4.5 补了 sourceDevice）。** 但 WS new-content 事件本身可能不携带全部 searchable 字段——**新 entry 的匹配判定与渲染数据，按 entryId 拉一次搜索投影（或 refetch 当前页）补全，不假设事件自带全字段**。

| 事件 | 无过滤浏览 | 有过滤 |
|---|---|---|
| 新 entry | 拉投影 → 插顶 | 拉投影 → **匹配当前过滤才插顶**，否则忽略 |
| 删除 | 移除该行 | 若在结果集则移除 |
| 收藏 toggle | 原地 patch `tags` | 含 `favorited` 过滤：取消→移除，新增且匹配→插入 |
| 传输状态/进度 | 原地 patch | 原地 patch |
| payload→Lost | **write-through 更新索引** + 原地 patch（灰显） | 同（write-through，防 refetch 读到过期 healthy）|
| tag 变更（未来） | 原地 patch | 改变符合性 → 插入/移除 |

分页边界：插入跨页时以 `total`/`has_more` 为准，必要时 refetch 受影响页（不做乐观跨页搬运）。

---

## 5. 废弃 list 的迁移路线

| Phase | 内容 | 门槛/产出 |
|---|---|---|
| **0 模型重构（后端）** | `ContentType` 5 值化（html 升级、link/favorited 降 tag）；`search_entry_tag`（含 rebuild temp 表 + 三表原子 cutover，§4.2）+ `LinkRule`；内置 tag 常量；**显式 schema 迁移**（drop/recreate active 表与索引、建 `search_entry_tag`）+ bump version → backfill；degraded 兜底（§4.7） | **前置 gate**：① rebuild 能重算 link tag **及 §4.5「待验证」渲染字段**（拿不到则降级 lazy）；② favorited 真相源 owner 落位；③ 锁契约 guard 按 query 类型拆分 + 验收测试（§4.6）；④ sourceDevice 来源 = event store 且 live/rebuild parity（§4.5）；⑤ `search_entry_tag` 纳入 rebuild cutover（§4.2）；⑥ **「当前 schema → v4」升级 + 重建测试：旧库升级后重建带新字段（sourceDevice/尺寸/文件名/链接）的记录、确认字段不丢（非仅 fresh-db，§4.2）** |
| **1 DTO 对齐 + 前端双维** | `SearchResult` 补 tags + sourceDevice + 轻量元数据；前端 `contentType`+`tags` 渲染（html→现状代码块样式）；FilterBar 接 tag；`GET /search/tags`；实时矩阵（§4.8） | search vs list-projection 逐字段对照测试通过 |
| **2 性能下推（删 list 硬前置）** | filter-only 从「全量内存分页」改 **SQL 下推分页 + tag JOIN 索引**；验收阈值（如 N=5 万 entry 浏览/过滤 P95 延迟上限） | **达标前不得进 Phase 3** |
| **3 读路径收口 + 弃用 list** | 历史页/quick panel/dashboard 全切搜索；`getClipboardEntry(id)` 改走 detail 端点；stats 改造；标记 list `deprecated`（OpenAPI + 日志告警），迁移 GUI/CLI，保留一个 release 兼容窗口 | GUI/daemon 同版本发布，窗口主要兜底 CLI 与未预期消费者；telemetry 证无残留 |
| **4 删 list + 自定义 tag（未来）** | 无残留后删 **公开 list 路由 + DTO + SDK**；**保留搜索 facade 私有的主存储浏览投影** 供 degraded 兜底（§4.7）；自定义 tag 定义入主存储 + 规则编辑 UI + 存量回填 | 单一公开数据源；degraded 兜底仍在；tag 体系开放 |

---

## 6. 删除 list 的影响面（依赖点）

- **前端**：`clipboardSlice.fetchClipboardItems`、`useClipboardCollection`、`useClipboardEventStream`、quick panel；`getClipboardEntry(id)` 当前 **误用** list 的 `?id` filter，需改走 `GET /clipboard/entries/:id`。
- **CLI**：`apps/cli` 的 `uniclip dev dump-clipboard`、`uniclip get --list`。
- **stats 端点**：`list_uc.execute(10_000, 0)` 全量扫描，需改造。
- **生成物**：OpenAPI spec + generated SDK 的 `listClipboardEntries`。
- **`ready/not_ready`**：前端虚构状态（后端永远返回 ready），随 list 删除弃用，改用 §4.7 的 `state` + HTTP 状态码。

---

## 7. 风险与未决

1. **性能**（Phase 2 硬前置门槛）：filter-only 全量进内存（:863,898,953,965）；删 list 前须 SQL 下推 + tag JOIN 索引并过验收阈值。
2. **rebuild 输入完整性**（Phase 0 gate）：link tag 与 §4.5「待验证」渲染字段未证实前不假设可重建；拿不到者降级 lazy。
3. **favorite 持久化前置**：favorited tag 依赖主存储专门 user-state owner 就绪（现状 toggle 为 stub，是前置工作项；其归属独立于 representation-selection）。
4. **加密会话与带词搜索**：filter-only 不需 key；带关键词搜索需 `derive_search_key`，锁定态无法全文搜（与现状一致，非回归）。
5. **active 表 schema 迁移 + 列清单同步**：现状 version bump 只触发数据重灌、**不改表结构**；须新增显式 migration / 启动 schema reconciliation（drop/recreate active 表 + 建 `search_entry_tag`），且 rebuild 管线 **所有手写列名处（建表 / temp / insert / finalize 拷回）须同步**，漏一处则重建后新列变空；升级 + 重建测试覆盖字段保留（§4.2）。
6. **link 检测对齐 list**：`link` tag 与 `linkUrls` 渲染元数据同源，判定移植现状 list projection 的 link 检测（含纯文本 web URL），避免删 list 后纯文本 URL 从链接卡降级；含 search-vs-list parity 测试。（修正 round-2 的「收紧到 uri-list」——现状 list 本就检测纯文本 URL，收紧反而丢能力。）
7. **`code` 语义债已消除**：html 升 content_type 后不再有误导；真正代码检测作为未来自定义规则。
8. **锁契约 guard**（Phase 0/1）：上层 `/search` 路由若锁定即全拒，须按 query 类型放行 filter-only，否则锁定态浏览回归（§4.6）。
9. **sourceDevice 来源**：在 clipboard event（按 event_id）而非 entry record，live/rebuild 须同路径并做 parity test（§4.5）。
10. **`search_entry_tag` 的 rebuild 一致性**：必须纳入 temp 表 + 三表原子 cutover，否则重建丢/失步 tag 行（§4.2）。
11. **content_type 多 rep 归属**：须从 paste/primary representation 派生（非 preview rep），否则 `text/plain + text/html` 被判 `text`、`html` 过滤漏掉富文本；含 parity 测试（§4.3）。
12. **可变状态 write-through**：`payloadState` 在 capture 后会变（payload→Lost），须 write-through 更新索引列（非仅前端 patch），否则删 list 后 refetch 读到过期 healthy（§4.0/§4.8）。
13. **link 判据单一契约**：`detect_link_urls` 精确定义（scheme / uri-list / 纯文本单多行 / 散文负例），`link` tag 与 `linkUrls` 共用 + parity（§4.3）。
14. **degraded 浏览归属**：删 list 仅删公开路由/DTO/SDK，保留搜索 facade 私有的主存储浏览投影供兜底；含「list 已删 + 索引 blocked + 空浏览仍 degraded」验收测试（§4.7）。

---

## 8. 附录：ContentType 迁移映射

| 旧 ContentType | 新 content_type | 新增 tag |
|---|---|---|
| Text | Text | — |
| Html | **Html** | — |
| Link | **Text** | `link` |
| File | File | — |
| Image | Image | — |
| Other | Other（internal-only） | — |

> 前端旧 `type='code'`（html）→ `contentType='html'`（保留代码块样式，未来可升级富文本渲染）；旧 `type='link'` → `contentType='text'` 且 `tags` 含 `link` → `LinkContent`；收藏由 `tags` 含 `favorited` 表达。

---

*本文档由 AI 辅助起草（Claude），经 codex 8 轮对抗评审收敛至 APPROVED：round-1（12）+ round-2（7）+ round-3（3）+ round-4（1）+ round-5（1）+ round-6（2）+ round-7（3）共 29 条 finding 全部处理（0 拒绝；round-1 F-11 被用户更强方案——html 升 content_type——取代），round-8 零 finding 通过。可据此拆分 phase 与 commit。评审日志见 scratchpad/codex-review/。*
