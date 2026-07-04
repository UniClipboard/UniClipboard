# search_document render 列加密方案

日期：2026-07-03
状态：Draft v5（已应用 codex 第 1-4 轮评审修订）
关联：`docs/architecture/local-encrypted-search.md`（v1 设计定稿）

## 1. 问题定性

`search_document` 表中的五个从内容派生的列以明文落盘，违反 v1 安全模型：

- `text_preview`（截 200 字的内容预览）
- `file_names`
- `link_urls`
- `file_paths`
- `char_count`（全文字符数，泄露文本长度）

**这不是有意接受的权衡，而是功能演进绕过了安全模型审查：**

- v1 定稿（`docs/architecture/local-encrypted-search.md` §安全模型，"磁盘上不存储明文搜索词"）审查的 schema 只含元数据 + HMAC 化倒排（§数据模型：`entry_id`/`event_id`/时间戳/`file_type`/`mime_type`/`index_version` 等）。
- `text_preview` 随索引表初版引入（`crates/uc-infra/migrations/2026-04-11-000001_create_search_index/up.sql:22`）。
- `file_names`/`link_urls`/`source_device`/`payload_state` 由 search-v5 迁移补入（`crates/uc-infra/migrations/2026-06-25-000003_add_search_document_render_columns/up.sql:18-21`），`char_count` 由 search-v8 补入（`add_search_document_char_count` 迁移，见 `crates/uc-infra/src/search/constants.rs:35-38`），`file_paths` 由 search-v9 补入（`crates/uc-infra/migrations/2026-06-28-000002_add_search_document_file_paths/up.sql:16`），目的都是「浏览统一到 search(3B)」后给列表卡片直接供渲染数据。
- 写入路径全程明文：`SearchProjectionBuilder`（`crates/uc-application/src/facade/search/projection.rs:102-133,190,206`）→ `pipeline.rs:65-82` → `NewSearchDocumentRow::from_domain`（`crates/uc-infra/src/search/rows.rs:85-113`）→ 明文 INSERT（`sqlite_index.rs:990-1018`，rebuild temp-table 路径 `:1164-1168`）。

### 附带发现：锁定态明文外泄通道

主历史列表走 `GET /search/query` 的 filter-only 浏览。该路径 **不派生 search_key、无 session 锁守卫**（`crates/uc-webserver/src/api/search.rs:219-230`；`crates/uc-infra/src/search/sqlite_index.rs:1299` 仅在非 filter-only 时派生 key）。因此 **锁定态下 daemon 对 filter-only 浏览返回 HTTP 200 + 全部明文列**。关键词搜索因派生 key 失败返回 423；`/search/status`、`/search/rebuild` 有 `require_encryption_ready` 守卫。即：问题不止「磁盘明文」，还有「锁定的 daemon 主动吐明文」。

### 主存储对照（证明这是索引层的例外）

剪贴板主体内容 `inline_data` 是加密的（`EncryptingClipboardEventWriter`，XChaCha20-Poly1305）；blob 文件走 `EncryptedBlobStore`（UCBL 头 + 加密）。`clipboard_history` 的另一条投影 `list_entry_projections.rs:71-90` 从解密后的 representations 重算 link_urls，不依赖 search_document——即明文列是主存储加密体系之外的旁路副本。

## 2. 现状事实清单（设计依据）

### 2.1 密钥与加密设施

- **search_key 派生**：`SearchKeyDerivationPort`（`crates/uc-core/src/ports/search/search_key.rs:14`）→ `HkdfSearchKeyDerivation`（`crates/uc-infra/src/search/search_key_derivation.rs:51-69`）→ `DeriveSpaceSubkeyPort::derive_subkey`（`crates/uc-infra/src/security/space_access_adapter.rs:508-526`）：`HKDF-SHA256(ikm=master_key, salt=profile_id, info=b"uniclipboard-search-index/v1")` → 32B。无缓存，每次实时派生；锁定态返回 `SessionLocked`。
- **term_tag 是 HMAC-SHA256 PRF tag，非加密**（`search_key_derivation.rs:78-83`）：确定性、无 nonce、不可逆。倒排表本身不泄露明文——明文泄露仅来自 render 列。
- **可复用 AEAD**：`crates/uc-infra/src/security/v1_aead.rs` 提供 `encrypt_blob_xchacha(master_key: &MasterKey, plaintext, aad)` / `decrypt_blob_xchacha(...)`（`:115`/`:150`），XChaCha20-Poly1305 + 24B 随机 nonce。**注意签名绑定 `MasterKey` 类型，不接受任意 32B key**——本方案需先下沉出一个以裸 32B key 为参的底层 helper（见 §3.2）。现有两个消费者（`BlobCipherAdapter`、`EncryptedBlobStore`）都直接用 master_key。
- **AAD 约定**（`crates/uc-core/src/crypto/aad.rs`）：`uc:<type>:v<n>|<ids>`，如 `uc:inline:v1|{event_id}|{rep_id}`、`uc:blob:v2|{blob_id}`。
- **锁定态**：`InMemorySession`（`crates/uc-infra/src/security/session.rs`）单 `Option<MasterKey>`；锁定时一切密钥不可用，无例外。
- **数据库为裸 SQLite**（无 SQLCipher；`crates/uc-infra/src/db/pool.rs:34-66` 仅 busy_timeout/foreign_keys/WAL pragma），加密全为应用层列级。

### 2.2 读取与渲染路径（改动面）

- 唯一 DAO：`crates/uc-infra/src/search/sqlite_index.rs`。filter-only 分页 `filter_only_page`（`:633-635`）、关键词候选加载 `load_documents_by_ids`（`:527,537`）、行→domain 的 `hydrate_results`（`:778-812`，含 `char_count` `:805`）。
- 流向：`SearchResult`（`uc-core/src/search/result.rs:25-43`）→ `SearchResultView`（`crates/uc-application/src/facade/search/mod.rs:272-297`）→ `SearchResultDto`（`crates/uc-daemon-contract/src/api/dto/search.rs:12-37`）→ 前端 `src/lib/clipboard-transform.ts:162-201` → 卡片 `HistoryCardContent.tsx:30-74`。
- `text_preview` 是卡片正文预览的唯一文本来源，无独立 snippet 生成器；倒排只存 HMAC tag 不参与展示。
- `char_count` 只用于渲染（hydrate `:805`），不参与任何 SQL 过滤/排序——可安全并入加密 payload。
- `source_device` 被 SQL 下推做 `eq_any` 过滤（`sqlite_index.rs:586-588`）；语义为稳定 `DeviceId`（`get_source_device` 返回 `Option<DeviceId>`，`crates/uc-application/src/facade/clipboard/facade.rs:646`），非用户自定义设备名。
- **rebuild 机制**：`CURRENT_INDEX_VERSION = "search-v9"`（`crates/uc-infra/src/search/constants.rs:43`）；启动时版本不符自动触发全量 rebuild（`coordinator.rs:232-286`），版本不符期间查询被拦截（`sqlite_index.rs:1349`），rebuild 中断有 search_blocked 自愈（PR #1179）。**bump 版本号即获得零成本全量回填。**
- 写入侧锁定态：live index 派生 key 失败即整条跳过（`clipboard_live_index/mod.rs:124-136`），锁定态不产生新行。
- **明文列的全部 raw-SQL 触点**（迁移必须原子切换的清单）：
  1. `index_entry` 单行 INSERT 列清单（`sqlite_index.rs:997`，字符串 SQL，编译期不校验）
  2. rebuild temp-table 定义（`sqlite_index.rs:894`，字符串 SQL）
  3. rebuild temp-table `INSERT...SELECT` 列清单（`sqlite_index.rs:1164-1168`，字符串 SQL）
  4. Diesel DSL 路径（`schema.rs` + `rows.rs` + `as_select()`）编译期强制同步，不构成运行期风险
  5. 测试辅助构造（`sqlite_index.rs:1901,2339` 等 `char_count: None` 样板）随编译期同步
- 待删除的五列上 **无任何索引/视图/触发器**（索引仅在 `active_time`/`file_type`/posting 表上，见 `2026-04-11-000001_create_search_index/up.sql:48-57`），SQLite `DROP COLUMN`（≥3.35）无障碍。

## 3. 方案

### 3.1 加密粒度：五个内容派生字段打包为单个 AEAD envelope

新增列 `render_payload BLOB`（列定义 nullable，但 **NULL 只是迁移期状态**：v10 稳态下每条被索引的行都必须写入加密 payload——即使所有 render 字段为空也加密一个「空字段」payload。否则 NULL 会成为未经认证的绕过：篡改者删掉 payload 即可静默抹除预览/文件名/链接而不触发任何校验。v10 之后读到 NULL 一律走 §3.4 损坏行路径，与 magic 非法/解密失败同等对待。锁定态 live index 本就整条跳过不写行，不产生 NULL）。明文结构：

```json
{
  "v": 1,
  "text_preview": "...",
  "file_names": [...],
  "link_urls": [...],
  "file_paths": [...],
  "char_count": 12345
}
```

**密文 envelope 为固定二进制格式**（仿 `EncryptedBlobStore` 的 UCBL 头，`encrypted_blob_store.rs:37-55`）：

```text
[magic "UCSR" 4B][format_version 1B = 0x01][nonce 24B][XChaCha20-Poly1305 ciphertext+tag]
```

- 解码行为：magic 不符 / format_version 不支持 / AEAD 验证失败 → 统一走 §3.4 的单行降级路径，不区分对待。
- 未来算法/结构演进 bump `format_version`，旧版本可继续读或触发重投影。

**删除五个明文列**（`text_preview`/`file_names`/`link_urls`/`file_paths`/`char_count`；`DROP COLUMN` 可行性已在 §2.2 验证）。

理由：

- `hydrate_results` 永远整组读取、卡片渲染整组消费，无按列部分解密需求。
- 每行一份 nonce+tag 开销而非五份；AAD 一条。
- domain 层 `SearchDocument`/`SearchResult` 结构不变（仍是五个字段），加解密收敛在 DAO 行映射层，上层无感知。

**不加密的两列及理由（记为显式接受的权衡）：**

- `source_device`：被 SQL 下推 `eq_any` 过滤，加密即破坏过滤。内容为稳定 `DeviceId`（已核实非用户自定义名），泄露面 =「哪台设备产生了该条目」，属 v1「元数据明文可接受」范畴；在文档 amendment 中显式写明。
- `payload_state`：可用性标志位（`Present`/`Lost`），非内容。

### 3.2 密钥：HKDF 派生独立 render_key，每个外层操作派生一次

- 派生：`derive_subkey(salt=profile_id, info=b"uniclipboard-search-render/v1")`，与 search_key 同端口、同风格。
- 不复用 search_key：其用途是 HMAC-PRF（term_tag），兼职 AEAD key 违反用途分离。
- 不直接用 master_key：保持搜索域隔离（inline_data 直用 master_key 是存量现状，新代码不延续）。
- AAD：`uc:search_render:v1|{entry_id}`，绑定密文到行，防跨行搬运替换。在 `aad.rs` 增加 `for_search_render(entry_id)`。
- **派生所有权（单一模型）**：render_key 的派生 **统一收敛在 `SqliteSearchIndex` adapter 内部**——它已持有 key 派生端口（查询路径本就在 adapter 内派生 search_key，`sqlite_index.rs:1299-1300`）。每个外层操作在 adapter 入口派生一次、操作内复用、不做全局缓存：
  - 查询：`search()` 入口派生一次，hydrate 整页复用；
  - live index：`index_entry()` 入口每条派生一次；
  - rebuild：adapter 的 rebuild 入口在 run 开始派生一次，跨批复用。
  coordinator **不** 负责 render_key 派生（它只继续管 search_key 与调度），避免两套所有权模型并存。
- 新增独立 `RenderKey` newtype（仿 `SearchKey`：32B、无 Serialize、Debug 脱敏），端口返回该类型，防止与 search_key 混用。
- 锁定态派生返回 `SessionLocked`，自然阻断。

**AEAD helper 分层**（现有 `encrypt_blob_xchacha` 签名绑定 `MasterKey`，直接复用会编译不过或诱导把 `RenderKey` 伪装成 `MasterKey`）：

- 在 `v1_aead.rs` 下沉一个以裸 32B key 为参的底层原语（如 `encrypt_xchacha_raw(key: &[u8; 32], plaintext, aad)` / `decrypt_xchacha_raw(...)`）；
- 现有 `MasterKey` 包装函数改为调用该底层原语（行为不变，纯重构）；
- `RenderPayloadCodec` 只接收 `RenderKey`，经底层原语加解密，绝不出现 `RenderKey`→`MasterKey` 的类型转换。

**密钥传递接口形状**（避免行映射层缺少密钥上下文）：

- 新增 `RenderPayloadCodec`（uc-infra 内部，持有 `RenderKey` + AAD 构造），提供 `encrypt(entry_id, RenderFields) -> Vec<u8>` / `decrypt(entry_id, &[u8]) -> Result<RenderFields, RenderDecodeError>`。
- adapter 在进入阻塞数据库闭包 **之前** 完成 render_key 派生并构造 codec，把 codec 显式传入 `rows.rs` 的 `from_domain(codec, ...)` / `to_domain(codec, ...)`（签名改变，不依赖环境态）。

### 3.3 锁定态行为：`/search/query` 统一前置锁校验 → 423

不再依赖引擎层解密失败兜底，改为 handler 统一前置校验：

- `GET /search/query` 所有形态（filter-only 浏览、关键词、tag）在 handler 入口检查 `session_ready`，未就绪一律 423（与 `/search/status`、`/search/rebuild` 的 `require_encryption_ready` 一致）。引擎层 `SessionLocked` 保留为纵深防御。
- **`GET /search/tags` 同步收紧为锁定态 423**：当前锁定态仍返回 builtin tag（link/code/image 等）的条目计数（`search.rs:286-297`），这些计数是内容派生信息，与本方案拒绝「锁定态骨架行」的理由（泄露数量/类型分布）直接矛盾。统一 423 后前端 tag 面板与列表同用 `session_ready` gate，无额外 UX 成本。
- **`/search/tags` 同时套用与 `/search/query` 相同的 index meta guard**：tag 列表路径直接读 tag 表，不经 `sqlite_index` 的版本拦截；v9→v10 版本不符或 rebuild 阻塞期间会返回过期计数。补上同一守卫：index 被 block 或存储版本非 current 时，返回与 search query 一致的 unavailable/rebuilding 响应。
- 同步更新：handler 文件头注释（`search.rs:6-11`，现在明确写着允许锁定态浏览）、utoipa OpenAPI 注释（423 响应）、`SearchResultDto` 契约无变化、错误文案。
- **前端统一收口**：锁定态处理放在共享层而非逐页面处理——在 `/search/query` 的 API wrapper（`src/api/daemon/search.ts`）或共享数据 hook 层拦截 423 并暴露 `locked` 态；所有已知调用方随之统一：
  - `src/hooks/useLiveSearch.ts` / `src/hooks/liveSearchModel.ts`
  - `src/hooks/useHistoryData.ts`（主历史列表）
  - `src/quick-panel/hooks/useHistorySearch.ts`（快捷面板）
  - 实现前用 `grep -rn "search/query\|searchQuery" src/` 复核穷尽，防止遗漏入口反复 423 重试。
- 历史页/快捷面板锁定期间显示「等待解锁」占位，gate 在 `encryption.session_ready`（WS 事件已有；`GET /encryption/state` 轮询模式在 restore-on-startup 修复中已建立，可复用）。
- 已知 UX 回退：冷启动静默 keychain 解锁比 webview 晚约 1.6s，该窗口内历史页短暂显示占位。数据本身需解密，此代价不可消除且窗口极短。

### 3.4 单行解密失败的降级策略（唯一策略，非开放争点）

`render_payload` 解码失败（损坏 / magic 不符 / 版本不支持 / AEAD 验证失败）时：

1. **该行仍然返回**：render 字段置空（`text_preview=None` 等），元数据（时间戳/file_type/payload_state）照常——分页游标、total、排序完全不受影响，不会出现整页 500 或 total 漂移。
2. 记 `warn!` 日志（含 entry_id + 失败类别）并计数指标。
3. **修复所有权在 application 层，不在 index 层（唯一上报路径，已定死）**：`SqliteSearchIndex` 只有索引库句柄，无法读主存储、无法重建投影。上报链路：引擎内部返回结构（`SearchResultsPage` 级）新增 `corrupted_entry_ids: Vec<EntryId>` 字段——**不进入 API DTO**；`SearchFacade::query` 收到后把这些 id 交给 `SearchCoordinator::schedule_repair(ids)`。coordinator 持有合并去重的「修复该 entry」任务：对同一 entry 的多次上报合并为一次修复；coordinator 需新增 **单条 entry 按 id 加载** 的依赖（复用 rebuild 的 `project_persisted_entry` 所用的持久层读取端口），经 `build_from_persisted`（`projection.rs:281-317`）重投影并经 `index_entry` 覆写（异步、尽力而为、限速；重投影再失败只记日志，不重试风暴）。验收测试：一条损坏行跨多次查询恰好调度一次修复。
4. 前端对 render 字段为空的行渲染通用占位卡片（与 `payload_state=Lost` 的既有降级样式同族）。

### 3.5 迁移与回填

1. SQL 迁移（单个迁移内完成）：`ALTER TABLE search_document ADD COLUMN render_payload BLOB` + 五条 `DROP COLUMN`。
2. **原子切换清单**（与迁移同一提交、同一 release，缺一不可——§2.2 已穷举 raw-SQL 触点）：
   - `schema.rs` / `rows.rs`（Diesel 编译期强制）
   - `sqlite_index.rs:997` INSERT 列清单（字符串 SQL，人工核对）
   - `sqlite_index.rs:894` temp-table 定义（字符串 SQL，人工核对）
   - `sqlite_index.rs:1164-1168` temp-table `INSERT...SELECT`（字符串 SQL，人工核对）
   - 集成测试对新 schema 的插入/查询全链路跑通后才允许合并。
   - **旧临时表残留处理**（已核实风险成立）：rebuild 临时表是 **非 TEMP 的持久表**，表名按 profile 固定（`tmp_search_document_rebuild_{suffix}`，`sqlite_index.rs:89-90`），且用 `CREATE TABLE IF NOT EXISTS`（`:877`）——v9 中断残留的旧结构临时表会被 v10 原样复用，导致 `render_payload` 写不进且明文列残留。修法：rebuild 建表前先 `DROP TABLE IF EXISTS` 同名临时表（doc + posting 两张）；§3.6 清理任务同时扫掉游离的 `tmp_search_*_rebuild_*` 表。补集成测试：「v9 结构的残留临时表存在时升级 v10」。
3. bump `CURRENT_INDEX_VERSION` → `"search-v10"`：启动时自动全量 rebuild，从主存储（解密后 representations）重新投影并加密写入。版本不符期间查询本来就被拦截，无空窗渲染；中断有既有自愈。
4. **解锁触发器（修复「锁定态启动永远搁浅」）**：`startup_evaluation` 在 daemon 启动时运行，而加密会话通常在前端 webview 加载后才静默解锁（晚约 1.6s，冷启动常态是「锁定态启动」）。coordinator 目前没有任何 unlock 钩子（已核实无 `session_ready` 订阅），rebuild/purge 在锁定态只会内存标记 unavailable，不会在解锁后重试——v10 rebuild 与清理任务将永远搁浅到下次进程重启。补法：
   - **单一收敛入口**：解锁入口不止一个（keychain 静默恢复、`POST /encryption/unlock`、`/encryption/unlock-with-passphrase`、deferred services 启动时序）。不在每个入口各自通知，而是在 daemon 运行层选定 **唯一的 session-ready 通知点**（所有解锁路径最终都经过的那一处，即广播 `encryption.session_ready` 事件的公共出口），由它调用 `SearchCoordinator::on_session_ready()`。
   - **幂等防重**：`on_session_ready()` 触发时 **重新读取 `search_index_meta`、拿到 rebuild 锁后再复核一次**（版本不符→rebuild；v10 已 current 但 purge 标记 NULL→清理），避免「coordinator 延后启动 + 解锁通知」双触发导致重复 rebuild，也避免任何入口漏通知导致漏跑。「rebuild needed / purge needed」的判定本就持久化在 `search_index_meta`（index_version、plaintext_purge_done_ms），无需额外内存状态。
5. **downgrade 策略：显式不支持（不可逆迁移）**：
   - down migration 故意返回错误（拒绝执行），不重建明文列——密文无法在迁移上下文中解密回填，重建空明文列只会制造静默数据丢失。
   - 旧 binary 对新 DB：raw-SQL INSERT 引用已删列即报错，搜索/索引功能不可用（应用其余部分不受影响）；声明为不支持的组合。
   - release notes 标注「不可逆索引 schema 迁移；回退需删除搜索索引由新版本重建」。

### 3.6 明文残留清理（独立维护任务）

`DROP COLUMN` 后旧明文可能残留在页内未覆写空间、freelist 页与 WAL 中。设计为 **可重入的后台维护任务**，不阻塞主流程：

1. 触发时机：search-v10 全量 rebuild 成功 finalize 并写入版本戳 **之后**，由 coordinator 调度一次性清理任务。
2. 执行方式与 **分层落点**：coordinator 属 application 层，不能直接持数据库连接跑 SQLite 维护命令。新增 infra 侧端口 `SearchIndexMaintenancePort::purge_plaintext_residue()`，由 `SqliteSearchIndex`（或同 crate 维护 adapter）实现：从连接池获取专用连接，先 `PRAGMA wal_checkpoint(TRUNCATE)`，后 `VACUUM`，并扫掉游离的 `tmp_search_*_rebuild_*` 残留表；`SQLITE_BUSY` 时指数退避重试（有限次），不无限等待；完成标记的写入也在 adapter 内完成。coordinator 只负责触发与状态流转。
3. 幂等与自愈（**标记的完整落点**）：
   - schema：`search_index_meta` 新增 `plaintext_purge_done_ms INTEGER`（nullable），与 render_payload 同一迁移加入；
   - 访问接口：沿 `index_version`/`search_blocked` 的既有 meta 读写路径扩展读/写该字段（对应 port trait 增补 getter/setter）；
   - 启动接入：`startup_evaluation`（`coordinator.rs:232-286`）新增分支——`index_version` 已是 `search-v10` 但 `plaintext_purge_done_ms` 为 NULL → 调度清理任务补跑（与 search_blocked 自愈同款模式）；
   - 清理成功后写入完成时间戳；任务失败/进程被杀则字段保持 NULL，下次启动自动重试。
4. 用户感知：VACUUM 期间写查询可能短暂阻塞，任务安排在 rebuild 完成后的空闲回调执行并记录耗时日志。
5. 诚实边界：文件系统层历史残留（已删除页曾写过盘、TRIM 前的扇区、旧备份）在无全库/全盘加密前提下无法保证清除。记入威胁模型边界。

### 3.7 文档补账

在 `docs/architecture/local-encrypted-search.md` 增补 amendment 一节：

- 记录 v5/v8/v9 render 列曾绕过 v1 安全模型（明文落盘 + 锁定态 filter-only 200 外泄）。
- 记录修复后状态与显式接受的权衡：`source_device` 明文（稳定 DeviceId，泄露面为条目来源设备）、`payload_state` 明文（可用性标志）、文件系统残留边界。

## 4. 改动清单

| 层 | 文件 | 改动 |
|---|---|---|
| schema | 新迁移 + `crates/uc-infra/src/db/schema.rs` | 加 `render_payload`、删五明文列、`search_index_meta` 加 `plaintext_purge_done_ms`；down 迁移显式报错 |
| 行映射 | `crates/uc-infra/src/search/rows.rs` | `from_domain`/`to_domain` 改签名接收 `RenderPayloadCodec` |
| codec | uc-infra search 模块新文件 | `RenderPayloadCodec`：UCSR envelope 编解码 + AEAD |
| AEAD 原语 | `crates/uc-infra/src/security/v1_aead.rs` | 下沉裸 32B key 底层 helper，`MasterKey` 包装改调它（纯重构） |
| 维护端口 | `crates/uc-core/src/ports/search/` + infra adapter | `SearchIndexMaintenancePort::purge_plaintext_residue()`（checkpoint/VACUUM/残留表清扫/标记写入在 infra） |
| DAO | `crates/uc-infra/src/search/sqlite_index.rs` | 3 处 raw-SQL 列清单切换；hydrate 解密 + 单行降级；操作级 key 派生 |
| 密钥 | `crates/uc-infra/src/search/search_key_derivation.rs`（或旁邻新模块）、`crates/uc-core/src/crypto/aad.rs` | render_key 派生（`uniclipboard-search-render/v1`）+ `for_search_render` AAD |
| 端口 | `crates/uc-core/src/ports/search/` + `uc-core/src/search/` | 扩展 key 派生端口（render_key）+ `RenderKey` newtype；meta port 增补 purge 标记读写；损坏 entry 上报通道 |
| coordinator | `crates/uc-application/src/facade/search/coordinator.rs` | 合并去重的单 entry 修复任务（`schedule_repair` + 单条 entry 加载依赖）；清理任务调度；startup_evaluation 增 purge 补跑分支；`on_session_ready()` 解锁触发器 |
| encryption facade | unlock 成功路径 | 通知 `SearchCoordinator::on_session_ready()` |
| 引擎结构 | `SearchResultsPage`（引擎内部） | 新增 `corrupted_entry_ids`（不进 API DTO）；`SearchFacade::query` 转交 coordinator |
| API | `crates/uc-webserver/src/api/search.rs` | `/search/query` + `/search/tags` 统一前置 423 锁校验；文件头注释 + OpenAPI 423 响应更新 |
| 前端 | `src/api/daemon/search.ts`（wrapper 层收口）+ 4 个已知 hooks + 占位组件 | 423→`locked` 态；session_ready gate；render 空行占位卡片 |
| 版本 | `crates/uc-infra/src/search/constants.rs` | `search-v10` + 版本历史注释 |
| 文档 | `docs/architecture/local-encrypted-search.md` | amendment |

架构注意：`rows.rs`/`sqlite_index.rs` 属 uc-infra，可直接依赖 `v1_aead`（同 crate `security` 模块，`pub(crate)`）；若跨模块可见性受限，优先扩 `pub(crate)` 面而非复制原语。

## 5. 测试计划

- 单元：
  - codec roundtrip（含空字段、全字段、大 payload）；
  - AAD 换 `entry_id` 解密必败（防搬运）；
  - envelope 异常样本（错 magic / 未知 version / 截断 / 篡改密文）全部落入 §3.4 降级路径；
  - 锁定态派生返回 `SessionLocked`。
- 集成：
  - v9→v10 启动自动 rebuild 回填，回填后查询内容正确；
  - **锁定态启动**：daemon 以锁定态启动 → 解锁事件后 rebuild/清理任务自动开跑（不需进程重启）；coordinator 延后启动 + 解锁通知双触发时 rebuild 只跑一次（幂等复核）；
  - **v9 残留临时表**：预置 v9 结构的 `tmp_search_document_rebuild_*` 表后升级 v10，rebuild 正常完成且残留表被清掉；
  - **v10 稳态 NULL payload**：手工置 NULL 的行走损坏路径（返回空 render + 调度修复），且「一条损坏行跨多次查询恰好调度一次修复」；
  - rebuild 阻塞/版本不符期间 `/search/tags` 返回 unavailable/rebuilding（不吐过期计数）；
  - **明文探针**：以已知探针字符串写入若干 entry，完成迁移 + rebuild + 清理任务后，对主 DB 文件、`-wal`、`-shm`、VACUUM 后文件做字节级扫描均不得命中；清理任务被杀后重启能补跑并最终通过探针；
  - filter-only 浏览锁定态 423、解锁后 200 且内容正确；tag/关键词路径 423 行为不回归；
  - 单行密文损坏：该页返回、损坏行 render 为空、total/分页不变、重投影后恢复。
- 前端：锁定态历史页与快捷面板占位、`session_ready` 事件后自动加载；render 空行占位卡片。
- 回归：关键词搜索、source_device 过滤、payload_state=Lost 标记、char_count 显示不受影响。

## 6. 已裁决的争点（决策记录）

1. **单 blob 打包 vs 按列独立加密** → 单 blob。读取方永远整组消费（§2.2），无部分解密需求；未来若出现按列需求，envelope `format_version` 提供演进空间。
2. **render_key 独立派生 vs 复用 search_key** → 独立派生。search_key 用途是 HMAC-PRF，兼职 AEAD 违反用途分离；多一个 HKDF label 成本可忽略。
3. **锁定态 filter-only：423 vs 元数据骨架行** → 423。骨架行泄露锁定态下的条目数量/时间/类型分布，且需要前后端双份「半锁定」渲染逻辑；解锁窗口实际只有 ~1.6s，UX 收益不值得复杂度与泄露面。
4. **单行解密失败策略** → 已定为 §3.4（返回空 render + 单条重投影），不再是开放争点。
5. **VACUUM 时机** → 已定为 §3.6（rebuild finalize 后的幂等后台任务），不再是开放争点。
6. **迁移方式：`DROP COLUMN` vs 全表重建** → `DROP COLUMN`。待删五列无索引/视图/触发器依赖（§2.2 已验证）；raw-SQL 触点已穷举并纳入原子切换清单（§3.5）；全表重建引入复制窗口与外键/约束重建风险，收益为零。
