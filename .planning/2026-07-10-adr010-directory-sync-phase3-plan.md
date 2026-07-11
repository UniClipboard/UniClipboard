# ADR-010 目录同步 · 第三阶段规划（目录捕获）

Status: draft (2026-07-10)
Issue: #875 · ADR: `docs/architecture/adr-010-directory-sync-as-file-set-manifest.md`
Phase 1: PR #1290 (merged 2026-07-10) · Phase 2: PR #1313 + #1314 (merged 2026-07-10)
前序规划: `.planning/2026-07-10-adr010-directory-sync-phase2-plan.md`

## 1. 现状盘点（阶段 1+2 落地了什么）

- **领域模型**（`crates/uc-core/src/clipboard/file_set.rs`）：`EntryFileSet` 逐行清单，
  行 kind = `File { content_hash, blob_id, size_bytes }` / `NonFile` /
  `Excluded { reason }`。`content_digest_contribution()` all-or-nothing：任一
  `Excluded` 行 → 空贡献 → 回退路径文本身份。**尚无 `root_index` / `relative_path` /
  `kind_tag`——扁平逐行，无目录树语义。**
- **持久化**（`crates/uc-infra/migrations/2026-07-03-000000_create_entry_file_set/` +
  `db/repositories/entry_file_set_repo.rs`）：`entry_file_set` 表 8 列，
  `(entry_id, line_index)` PK，整体 replace/load。**`original_text` 明文 TEXT 落库**
  （见 §4 合规缺口）。
- **身份计算**（`crates/uc-core/src/clipboard/system.rs:531`
  `SystemClipboardSnapshot::snapshot_hash`）：文件类 rep 的路径文本 hash 被
  `file_content_digests`（排序后的成员 content hash 平集）经
  `uc_content_hash::file_content_wrapper`（前缀 `file-content|`）替换，再进外层
  `snapshot_hash`（前缀 `snapshot-hash-v1|`）。**平集聚合，无结构/路径信息。**
- **捕获遍历**（`crates/uc-application/src/clipboard_capture/usecase.rs:838`
  `build_entry_file_set`）：只处理平铺文件集（LocalFile reps 或 inline uri-list），
  逐行 `classify_file_path`（`hash_path` 仅身份不物化 blob）。caps 预检
  `file_set_exceeds_caps`（阶段 2 落地，毫秒级元数据预检）已就位。**不遍历目录。**
- **出站单一真相源**（阶段 2，`facade/clipboard_outbound/mod.rs:581`
  `resolve_outbound_file_set`）：dispatch 优先读 manifest 还原路径参与
  `publish_file_blob_refs`；manifest 缺失/读错回退 `extract_file_paths_from_snapshot`；
  含 `Excluded` 行 → `Skipped { reason: "file_set_excluded" }`。

**关键缺口**：整条链路只认「平铺文件集」。目录被复制时，捕获侧当前把目录路径当普通文件行
`hash_path`（对目录会 `IngestFailed` → 整体回退路径文本身份），不展开、不进同步。

## 2. 阶段 3 拆分为 3a / 3b（已拍板）

ADR 开放问题 #3 提示阶段 3 可能大到需要拆。**决策：拆 3a / 3b，先做 3a。**

| 子阶段 | 内容 | 用户可见性 |
| --- | --- | --- |
| **3a（本文重点）** | schema 密文化升级 + 目录遍历 + 成员边界规则 + `file-set-v1` 结构入身份；**同步遍历**（靠阶段 2 的限额压住时延，1 GiB ≈ 秒级） | 目录进本地历史，dispatch 判 Sync-ineligible 不出站 |
| 3b（本文只勾勒） | 延迟身份就绪（捕获流水线异步化，就绪前不广播/不 dispatch）+ 摄取毕 `(mtime, size)` 漂移复核 | 无（内部加固，去掉同步遍历对捕获热路径的时延占用） |

排序理由（沿用 ADR）：

- **3a 同步遍历可先行**：限额护栏（阶段 2）已把成员数/总量压在 2000 / 1 GiB，同步遍历+哈希
  时延有界（秒级），不必先做异步化就能让目录进本地历史并为阶段 4 准备结构化身份。
- **异步化是独立大改动**：延迟身份就绪 = 捕获流水线异步化 + 就绪前不广播 + 漂移复核，与
  遍历/身份逻辑正交，拆到 3b 独立验证，避免 3a 面过大。

## 3. 3a 范围（三个 PR）

ADR 连带决策 1 明言「遍历与身份升级不可拆」——无结构入身份的目录 entry 会与同名文件集
**内容碰撞**（`{a/1.txt, a/2.txt}` 目录 vs `{1.txt, 2.txt}` 两个同内容平铺文件，排序后
content-digest 平集相同 → `file-content|` 身份相同 → 接收端建重复 entry）。

本拆分用**安全阀**化解，使 PR 可切分：**PR-B 落地遍历时，含目录结构的 entry 一律回退
路径文本身份**（贡献空集，与 `Excluded` 行同一机制）——路径文本天然区分目录与平铺文件，
不碰撞；PR-C 再把路径文本回退替换为 `file-set-v1` 结构化身份。任一 PR 落地后系统都自洽。

### PR-A：合规基座 —— 路径列加密 + schema 预留列（零行为变更）✅ 已实现

落地记录：迁移 `2026-07-10-000001_encrypt_entry_file_set_paths`（DELETE 存量 →
`original_text` 改 `original_text_ct BLOB` + 预留 `root_index`/`relative_path_ct`/
`kind_tag`）；`EntryFileSetPathCipher`（UCFS 信封,XChaCha20-Poly1305,AAD=
`for_file_set_line(entry_id,line_index)`）；repo 持 `DeriveSpaceSubkeyPort`+
`CurrentProfilePort`,per-op 派生子密钥（`info=uniclipboard-file-set/v1`,salt=profile）;
因密钥依赖 `platform`,repo 构造移到 space-access 之后（`InfraLayer` 暴露 `db_executor`）。
测试 18 绿（10 codec + 8 repo:往返/密文非明文/锁定报错/chunk 边界/FK CASCADE）。

1. **密文化迁移**：`entry_file_set` 表 2026-07-03 建、捕获写入今日才合并，**几乎无存量
   数据**——迁移直接 DROP+重建带密文列，无需 app 侧回填（用 MasterKey 不可在 Diesel
   迁移期取得，回填方案不成立；无存量数据使这一点无关紧要）。
   - `original_text` TEXT → `original_text_ct BLOB`（AEAD 密文）。
   - 预留三列 `root_index BIGINT NULL` / `relative_path_ct BLOB NULL`（密文）/
     `kind_tag TEXT NULL`（分类枚举 f/x/d，同 `kind`/`content_category` 例外，明文）。
     **本 PR 不写这三列**（保持 NULL），只开 schema headroom。
2. **AEAD codec**：新增 `FileSetPathCodec`（`uc-infra/src/db/...` 或 `security/`），复刻
   `search/render_payload.rs::RenderPayloadCodec` 模式——`v1_aead::{encrypt,decrypt}
   _xchacha_raw`，per-session 子密钥经 `DeriveSpaceSubkeyPort::derive_subkey`（IKM=
   MasterKey，HKDF-SHA256，独立 `info` 标签 `b"uniclipboard-file-set/v1"`），
   **AAD 绑定 `entry_id ‖ line_index`**（防跨行/跨 entry 密文搬运）。
3. **repo 穿线**：`DieselEntryFileSetRepository::new` 增加 key-derivation 依赖
   （`wire.rs:571`）；`encode_line` 密封 `original_text`、`decode_row` 解封。
   密钥不可用（未解锁）时 save/load 的错误语义需明确（沿用现有 `Storage` 翻译或
   新增语义，见开放问题 #2）。
4. **测试**：加密往返、AAD 绑定（换 `entry_id`/`line_index` 解密失败）、`save`→`load`
   契约不变（既有测试全绿）、密文列非明文（落库字节不含路径子串）。
5. **合规论证**：路径/文件名是用户内容，本 PR 对齐铁律「持久化即密文」，并顺带补齐
   阶段 1 的明文欠账（见 §4）。`kind_tag` 明文豁免需在 PR 描述论证（分类枚举，
   目录重建/展示按类型分支依赖其明文，与 `content_category` 同性质）。

### PR-B：目录遍历 + 成员边界规则（含目录 entry 判 Sync-ineligible）

1. **领域模型扩展**：`EntryFileSetLine` 增加成员定位（`root_index` / `relative_path` /
   `kind_tag`）。平铺文件行：`root_index` = 其位置、`relative_path` = basename、
   `kind_tag` = f/x。目录成员行：`root_index` = 所属目录 root 的序号、`relative_path` =
   相对该 root 的路径。建议抽 `FileSetMemberLocation` 值对象承载三者。
2. **遍历**（`build_entry_file_set`）：LocalFile/uri-list 成员若为目录 → 遍历展开为逐叶子
   文件行。遍历在 caps 命中时**早停**（避免遍历超大目录树；阶段 2 的
   `file_set_exceeds_caps` 需接上目录展开的成员流，而非只数顶层条目）。
3. **成员边界**（ADR 连带决策 4）：
   - symlink 与特殊文件（FIFO/socket/设备节点）→ **整个目录判 Sync-ineligible**
     （不静默跳过、不解引用；可观测原因）。
   - 隐藏文件包含；硬链接当独立文件；除 exec bit 外权限/xattr/时间戳不保留。
   - `relative_path` 经 NFC 归一化、`/` 分隔；`kind_tag` = `f`/`x`（可执行）/`d`（空目录）。
4. **身份安全阀（关键）**：含目录结构的 entry，本 PR **一律回退路径文本身份**——遍历出的
   叶子 content digests **不**喂进 `file_content_digests`（等价于存在 `Excluded` 行的
   处理）。既避免 §3 的内容碰撞，也不需要 PR-C 先落地。目录本地去重靠路径文本（复制同一
   目录两次 → 同路径 → 同身份），够用。
5. **dispatch 门控**：`resolve_outbound_file_set`（`clipboard_outbound/mod.rs:581`）检测
   manifest 含目录结构（`kind_tag=d` 或 `relative_path` 跨层 / `root_index` 语义）→
   `Skipped { reason: "directory_not_yet_syncable" }`（可观测，不静默）。旧接收端在阶段 4
   之前不认识目录成员，含目录 entry 一律不出站。
6. **测试**：单目录展开（叶子行的 root/relative_path/kind_tag）/ 混合选择（文件+目录）/
   空目录（kind_tag=d）/ 隐藏文件包含 / symlink 或特殊文件 → 整目录 Sync-ineligible /
   可执行位 → kind_tag=x / caps 在遍历中早停（panic-on-hash 证明超限跳哈希）/ 含目录
   entry 身份走路径文本回退（`content_digest_contribution` 空）/ dispatch 门控 Skipped。

### PR-C：`file-set-v1` 结构入身份（替换 PR-B 的路径文本回退）

1. **叶子与组件公式**（ADR 连带决策 1）：
   - leaf = `BLAKE3("file-set-member-v1|" ‖ root_name ‖ 0x00 ‖ relative_path ‖ 0x00 ‖
     kind_tag ‖ 0x00 ‖ file_digest)`
   - 文件内容组件 = `BLAKE3("file-set-v1|" ‖ sort(leaves))`
   - 新增 `uc_content_hash::file_set_v1_wrapper(leaves)`，与既有
     `file_content_wrapper`（`file-content|`）并列。
2. **版本化落在文件内容组件**：外层 `snapshot-hash-v1|` 聚合不动。
   **仅当文件集含目录结构时**用 `file-set-v1|`；**纯平铺文件集继续走 `file-content|`
   平集**——存量与纯裸文件集合哈希逐字节不变，身份零迁移。
3. **捕获侧接线**：含目录时，`build_entry_file_set` 产出结构化 leaves，
   `snapshot.file_content_digests`（或新字段）承载 file-set-v1 组件，替换 PR-B 的
   路径文本回退。exec bit 入身份（影响粘贴产物）。
4. **注意**：阶段 3 目录仍不出站（dispatch 门控保留），结构化身份此阶段主要服务**本地
   去重的正确性**与**阶段 4 接收侧重建的前置**。若 3a 体量过大，PR-C 可评估延到阶段 4
   前置（ADR 允许，但默认留在 3a 以尊重连带决策 1）。
5. **测试**：`{a/1.txt,a/2.txt}` 目录 vs `{1.txt,2.txt}` 平铺同内容 → **身份不同**
   （碰撞回归）/ 同目录跨设备/重试 → 身份稳定 / relative_path NFC 归一 / kind_tag
   f↔x 改变 → 身份改变 / 纯平铺文件集身份与阶段 2 逐字节一致（零迁移回归）。

## 4. 合规缺口：`entry_file_set` 路径明文落库（本阶段顺带修复）

已确认：连接层只设常规 pragma（`db/pool.rs`），**无整库加密**（无 `PRAGMA key`/cipher）。
`entry_file_set.original_text` 存设备本地文件路径（用户内容，暴露文件名与目录结构），
与项目铁律「持久化即密文（不可打破的基石）」冲突。对照 `search_document.render_payload`
（走 Binary 密文）与 `search_posting.term_tag`（走 tag）——规则确实要求用户内容加密。

**决策：阶段 3 顺带修复**——PR-A 把 `original_text` 与新增 `relative_path` 一并 AEAD
加密落库。阶段 1 表几乎无存量数据，迁移成本可忽略。`content_hash`（单向摘要）与
`kind_tag`（分类枚举）按现有豁免保持明文。

## 5. 3b 勾勒（延迟身份就绪 + 漂移复核，本文不细化）

- **延迟身份就绪**：捕获流水线异步化——文件类 entry 先进本地历史（identity pending），
  遍历/哈希移出捕获热路径异步执行；就绪前不广播、不 dispatch。
- **漂移复核**：摄取毕对成员做 `(mtime, size)` 复核；目录被改则放弃该次身份
  （阶段 2 已埋下漂移观测基线：publish 后比对 wire digests 与捕获时刻贡献值记 WARN）。
- 细化推迟到 3a 落地后单独规划（ADR 开放问题 #3：若 3a 中同步遍历时延在真实目录上不可
  接受，再评估 3b 是否需提前，或 3a 内先接 (mtime,size) 观测）。

## 6. 明确不在阶段 3

- wire 格式携带成员相对路径、接收侧全有或全无重建目录树、root 冲突加后缀、uri-list
  改写、协议版本门控（阶段 4）。
- Sync-ineligible 的 UI 展示与 `file_sync` 设置界面（阶段 5；阶段 3 只保证日志/错误可观测）。
- 移动端消费目录文件集（ADR 范围：mobile 不消费，register 指向时保持上一可消费值）。

## 7. 待拍板的开放问题

1. **PR-C 去留**：`file-set-v1` 结构化身份留在 3a（尊重 ADR 连带决策 1）还是延到阶段 4
   前置（目录阶段 3 本就不出站，结构化身份主要服务阶段 4）？倾向留 3a，但若 3a 体量过大
   可外移。
2. **密钥未解锁时的 manifest 读写语义**：PR-A 后 `original_text` 需密钥才能解封。设备
   锁定/未解锁时 dispatch 读 manifest 应回退（`extract_file_paths_from_snapshot`）还是
   报可观测错误？倾向回退（与既有 manifest-missing 回退一致），但需确认捕获侧写入时机
   密钥必然可用。
3. **`root_index` 语义细化**：混合选择「2 个文件 + 1 个目录」时，root 的编号与 root_name
   的取值（顶层文件的 root_name = 自身 basename？目录的 root_name = 目录名）需在 PR-B
   定稿并写进 leaf 公式，与阶段 4 接收侧重建对齐。
4. **遍历实现依赖**：是否引入 `walkdir` 等遍历库，还是手写 `tokio::fs` 递归（异步、可在
   caps 命中时早停）？倾向手写异步递归以贴合 caps 早停与 3b 异步化，避免同步阻塞库。
