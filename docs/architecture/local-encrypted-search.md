# 本地加密搜索设计（V1）

## 状态

当前为需求和架构评审草案。

## 目标

为 UniClipboard 提供一套实用的、本地专用的加密历史搜索能力，同时不改变现有加密内容的存储格式。

V1 明确是 **本地加密索引**，不是远程可搜索加密协议。

## 范围

V1 必须支持：

- 仅本地生成索引
- 只有在加密会话解锁后才允许搜索
- 精确词匹配
- 布尔 `AND` 和 `OR`
- 时间范围过滤
- 文件类型多选过滤
- 记录删除时同步删除索引
- 全量索引重建

V1 的搜索对象仅限可提取文本的内容：

- 纯文本
- 文件路径
- 文件名
- URL
- 从 HTML 中提取的文本
- 其他可以稳定转换为文本的负载

## 重要运行时约束

daemon 在锁定状态下 **不会** 监听剪贴板变化。

这意味着：

- 新的剪贴板内容只有在加密会话解锁后才会被采集
- V1 不需要“锁定期间捕获，解锁后补建索引”的流程
- 索引构建可以直接挂在现有的解锁后持久化链路上

这个约束显著简化了索引更新和重建逻辑。

## 非目标

V1 不包含：

- 远程 SSE trapdoor 搜索
- 服务端加密查询执行
- 模糊匹配
- 语义搜索
- 嵌套布尔表达式
- `NOT` 查询
- 锁定状态下的受限搜索

## 产品语义

### 搜索目标

搜索引擎需要索引以下内容中提取出的文本：

- `text/plain`
- `text/html`
- `text/uri-list`
- 文件复制场景下可稳定提取的文件名或路径
- 后续所有可被规范化为纯文本的文本型 MIME

二进制负载本体不建立索引，除非它们带有稳定的文本元数据。

### 查询语义

支持的操作符：

- 空格隐式表示 `AND`
- 显式 `AND`
- 显式 `OR`

示例：

```text
foo bar
foo AND bar
foo OR bar
```

V1 语义：

- `foo bar` 等价于 `foo AND bar`
- 支持纯 `AND` 查询
- 支持纯 `OR` 查询
- 不支持在同一条无括号查询中混用 `AND` 和 `OR`
- 不支持括号
- 不支持一元否定
- 过滤条件通过结构化参数传递，不嵌入自由查询字符串

如果用户在同一条查询中混用了 `AND` 和 `OR`，后端应返回明确的 `invalid_query` 错误，并提示用户改写为统一操作符。

### 时间范围过滤

建议支持的预设值：

- `today`
- `yesterday`
- `last_24h`
- `last_7d`
- `last_30d`
- `this_week`
- `this_month`

API 同时应支持通过 `from_ms` 和 `to_ms` 指定绝对时间范围。

### 文件类型过滤

文件类型过滤是多选，后端应使用稳定的内部分类：

- `text`
- `html`
- `link`
- `file`
- `image`
- `other`

前端可以本地化显示文案，但后端应以固定枚举值工作。

以下内容统一归入 `text`：

- 代码片段
- JSON
- XML

### 文件扩展名过滤

文件扩展名需要作为一类一等过滤条件暴露，不应隐式并入 `file_type`。

建议通过结构化参数传递扩展名过滤，例如：

- `extensions: ["md", "txt"]`
- `extensions: ["png", "jpg"]`

对于单条目包含多个文件的情况，索引模型应支持多值扩展名。

## 为什么选择本地加密索引而不是 SSE

UniClipboard V1 的搜索需求是本地的：

- 索引在本地生成
- 查询在本地执行
- 内容在解锁后已可在本地解密
- 当前架构中不存在服务端搜索执行者

因此，完整 SSE 方案会引入额外复杂度，但并不贴合现有运行边界。

V1 的实际设计目标应当是：

- 保持现有加密剪贴板内容存储不变
- 构建独立的本地倒排索引
- 避免在索引中存储明文词项
- 从已解锁的主密钥派生专用搜索密钥

## 架构摘要

### 高层流程

```text
解锁后的剪贴板采集
        ↓
通过现有链路持久化加密内容
        ↓
从已持久化的表示中提取可搜索文本
        ↓
规范化并分词
        ↓
使用 HMAC(search_key, token) 生成词项标签
        ↓
写入本地倒排索引
        ↓
搜索查询在本地生成词项标签
        ↓
求解匹配的 entry ID
        ↓
应用时间和文件类型过滤
        ↓
通过现有 use case 加载投影或详情
```

### 安全模型

V1 的安全目标：

- 锁定状态下不可搜索
- 磁盘上不存储明文搜索词
- 搜索密钥与内容加密用途分离
- 本地全量重建仅允许在解锁状态执行

V1 不尝试隐藏：

- 本地访问模式
- 本地 posting list 大小
- 本地查询频率

对本地专用方案来说，这些是可接受的权衡。

## 文本提取与规范化

可搜索内容应按逻辑字段提取：

- `body_text`
- `html_text`
- `url_text`
- `file_path_text`
- `file_name_text`

规范化规则应稳定且带版本：

- Unicode NFKC 规范化
- 全量小写
- 合并重复空白
- 去除首尾空白
- 对 HTML 先去标签，再建立文本索引
- 解析 URL 的 host、path、query key 片段
- 将文件路径拆为目录段、文件名 stem 和扩展名

索引 schema 必须包含 `index_version`，以便未来规范化规则变化时安全触发重建。

## 分词策略

V1 建议采用混合分词：

- 对拉丁文本、数字、路径段、URL 片段、文件名做词级切分
- 对中文额外生成 bigram

这样更适合剪贴板内容常见的混杂形态：

- 自然语言
- shell 命令
- 代码片段
- URL
- 文件路径
- 中英混合文本

中文词典分词可以后续再评估，但 bigram 更适合作为 V1 的稳妥方案。

## 数据模型

搜索索引应独立存储，不改动现有加密剪贴板负载表。

### `search_document`

每个可搜索条目一行：

- `entry_id`
- `event_id`
- `active_time_ms`
- `captured_at_ms`
- `file_type`
- `file_extensions`
- `mime_type`
- `indexed_at_ms`
- `index_version`
- `deleted_at_ms` nullable

### `search_posting`

每个 `(term_tag, entry_id)` 一行：

- `term_tag`
- `entry_id`
- `field_mask`
- `term_freq`

`field_mask` 用于标识词项命中来源：

- 正文
- HTML 文本
- URL
- 文件路径
- 文件名

### `search_index_meta`

索引运行时元数据：

- `schema_version`
- `active_index_version`
- `last_rebuild_started_at_ms`
- `last_rebuild_finished_at_ms`
- `rebuild_state`

## 密钥派生

搜索索引不能直接复用内容加密原语。

建议流程：

1. 解锁现有加密会话
2. 从会话主密钥派生专用本地搜索密钥
3. 计算 `term_tag = HMAC(search_key, normalized_token)`

这样能在沿用现有信任模型的前提下，把搜索词派生与内容加密解耦。

## 查询执行

搜索请求建议按以下顺序执行：

1. 校验加密会话已就绪
2. 将查询解析为简单布尔表达式
3. 将时间预设值解析为绝对时间戳
4. 规范化并分词查询项
5. 派生词项标签
6. 查询 postings
7. 计算 `AND` / `OR` 结果集
8. 用文档元数据应用时间范围和文件类型过滤
9. 排序
10. 通过现有剪贴板 use case 加载投影或详情

建议的 V1 排序策略：

- `active_time_ms` 降序
- 然后按命中词数降序
- 最后按 `captured_at_ms` 降序

## 增量更新规则

### 新条目

当新剪贴板条目在解锁状态下完成持久化后：

1. 通过现有链路持久化加密内容
2. 提取可搜索文本
3. 构建 postings
4. 写入 `search_document`
5. 写入 `search_posting`

### 条目更新

如果可搜索内容发生变化，需要重建该条目的索引行。

如果只是元数据变化：

- 只更新相关文档元数据
- 除非可搜索文本或文件类型分类变化，否则不重建 postings

### 条目删除

删除条目时，必须同步删除：

- 对应的 `search_document` 行
- 该 `entry_id` 下所有 `search_posting` 行

删除必须纳入正常删除工作流，而不是“尽力而为”的异步清理。

## 全量重建

V1 必须支持在解锁状态下执行全量重建。

建议的重建方式：

1. 创建临时搜索表
2. 扫描所有可搜索剪贴板条目
3. 将重建结果写入临时表
4. 原子切换当前生效表
5. 更新 `search_index_meta`

这样可以避免对外暴露半重建状态的搜索结果。

## 建议的六边形边界

### `uc-core`

负责：

- 搜索查询模型
- 搜索结果模型
- 搜索过滤模型
- `SearchIndexPort`

### `uc-app`

负责：

- `SearchClipboardEntries`
- `IndexClipboardEntry`
- `RemoveIndexedClipboardEntry`
- `RebuildSearchIndex`

### `uc-infra`

负责：

- 基于 SQLite 的搜索索引实现
- 文本提取辅助模块
- tokenizer 与 normalization 服务
- 搜索密钥派生适配器

### `uc-daemon`

负责：

- 搜索查询、重建、索引状态相关 HTTP 路由

daemon 层不能承载搜索业务逻辑。

## API 形态

建议的搜索请求：

```json
{
  "query": "foo AND bar",
  "timeRange": {
    "preset": "last_7d"
  },
  "fileTypes": ["text", "html", "file"],
  "extensions": ["md", "txt"],
  "limit": 50,
  "offset": 0
}
```

建议的绝对时间范围请求：

```json
{
  "query": "invoice",
  "timeRange": {
    "fromMs": 1711929600000,
    "toMs": 1712534400000
  },
  "fileTypes": ["file"],
  "extensions": ["pdf", "docx"],
  "limit": 50,
  "offset": 0
}
```

## 待继续确认的问题

以下事项当前仍可延后，不阻塞底层索引方案定稿。

更完整的实现前评审项见下方“架构评审清单”。

- 前端查询构造器的最终交互形式
- URL host 命中是否需要高于正文命中排序
- 短语搜索是否作为 V2 引入

## 架构评审清单

以下问题建议在进入实现前明确归类，避免查询语义、索引归属和运行时工作流在各层出现重复实现。

### 必须先拍板

#### 1. 搜索文本提取的单一权威来源

已定：由现有 clipboard 领域模型统一产出“可搜索投影”，search 子系统只消费该投影，不自行从持久化表示重新猜测。

必须避免出现两套规则：

- 一套由现有剪贴板领域模型决定
- 一套由 search 子系统自行从持久化表示重新猜测

唯一入口统一产出：

- 可搜索字段集合
- 文件类型分类
- 文件扩展名集合

#### 2. 索引绑定 `entry_id` 还是 `event_id`

已定：V1 搜索结果代表“当前条目实体”，`entry_id` 是搜索索引主标识。`event_id` 如保留，仅作为追踪字段，不参与主语义。

- 搜索结果返回的是“当前条目实体”
- 删除和重建是按 `entry_id` 作为唯一索引文档标识

文档应明确 `entry_id` 为搜索索引主标识，避免后续出现双主语义。

#### 3. 条目是否可变

已定：`entry` 在 V1 产品语义上视为不可变，不存在“修改既有 entry 内容”这条主路径。

文档当前写了“条目更新”流程，但需要先确认产品语义：

- 已持久化 entry 是否允许被更新
- 还是只存在新增、删除、归档这类 append-only 事件

文档应删除“更新条目内容后重建索引”的表述，改为：

- 新事件产生新 entry
- 删除移除旧 entry 索引

否则实现层会被迫同时维护“文档更新模型”和“事件追加模型”。

#### 4. 删除语义是硬删还是软删

已定：V1 采用硬删。删除条目时，同步删除 `search_document` 和 `search_posting`，索引中不保留 `deleted_at_ms`。

当前文档同时出现了：

- `deleted_at_ms`
- 删除时同步删除 `search_document` 和 `search_posting`

这里必须二选一作为主语义：

- 硬删：删除时直接移除索引记录，不保留 `deleted_at_ms`
- 软删：保留文档行并通过 `deleted_at_ms` 过滤，同时定义 posting 是否保留

V1 采用硬删，避免 schema 和工作流互相冲突。

#### 5. 搜索密钥与数据隔离维度

已定：V1 从一开始支持隔离维度，隔离单位是 `profile`。

需要明确当前系统是否只存在单一加密会话上下文，还是未来可能存在：

- 多 vault
- 多 profile
- 多 space
- 多用户本地数据隔离

既然隔离维度是 `profile`，搜索索引就不能只靠全局 `search_key` 和全局表工作，至少要定义：

- 搜索 key 的派生上下文
- 索引表的数据归属字段
- rebuild 的作用范围

#### 6. 时间过滤的业务时间基准

已定：V1 以 `active_time_ms` 作为搜索时间过滤的主时间轴。时间过滤、默认排序和前端展示的主时间语义都以它为准；`captured_at_ms` 仅作为辅助字段。

文档同时使用了：

- `active_time_ms`
- `captured_at_ms`

必须明确以下问题：

- `today` / `last_7d` 之类的过滤到底基于哪个时间字段
- 默认排序主时间轴是否与过滤基准一致
- 前端展示给用户的“命中时间”使用哪个字段

V1 指定 `active_time_ms` 为唯一主时间字段，`captured_at_ms` 仅作辅助排序或调试信息。

#### 7. 时区语义

已定：时间预设按当前用户本地时区解释。

时间预设必须明确按哪个时区解释：

- 用户本地时区
- 固定 UTC
- 数据写入时的系统时区快照

尤其是：

- `today`
- `yesterday`
- `this_week`
- `this_month`

前后端和测试用例都应以用户本地时区为准，避免出现不一致。

#### 8. 查询语法的容错边界

已定：V1 查询 parser 采用收紧规则。

当前只定义了理想语义，但还缺 parser 规则：

- 小写 `and` / `or` 接受，并在解析时统一归一化
- 连续空白和全角空格统一规范化为空格
- 引号在 V1 中不承载短语语义，规范化时直接剥离
- 混用 `AND` 和 `OR` 返回 `invalid_query`
- 非法 token 明确报错，不做猜测性修复

V1 应给出一份明确的 grammar 或等价约束。

#### 9. 文件类型分类规则的唯一归属

已定：`text` / `html` / `link` / `file` / `image` / `other` 属于 search 视角的派生分类。clipboard 负责产出可搜索投影字段，search 基于该投影执行统一分类。

`text` / `html` / `link` / `file` / `image` / `other` 是共享业务规则。

需要明确：

- 它属于 clipboard 领域模型的稳定分类
- 还是仅属于 search 视角的派生分类

即便它属于 search 视角的派生分类，也必须只有一个权威实现，避免前端、app、infra 各自推断。

#### 10. rebuild 期间的增量写入策略

已定：rebuild 期间采用双写策略。重建窗口内的新条目同时写入 active 表和 temp 表，确保原子切换后不丢失新数据。

“临时表重建 + 原子切换”还不够，需要补上 rebuild 窗口中的并发写入规则：

- 新条目是否双写到 active 表和 temp 表
- 还是暂停索引写入
- 还是记录增量日志并在切换前回放

V1 在 rebuild 窗口内采用双写，避免原子切换后丢失新数据。

### 可以延后但建议记录

#### 1. 前端查询构造方式

可以在 V1 后段再定，但至少要知道后端是否长期接受自由文本查询，还是最终会切到结构化查询构造器。

#### 2. URL 命中权重

是否让：

- host 命中高于正文
- 文件名命中高于路径命中

这属于排序体验问题，可以在有真实样本后再调。

#### 3. 短语搜索

适合明确标记为 V2，不建议在 V1 tokenizer 和 schema 里预留过多兼容分支。

#### 4. 扩展名过滤对非 `file` 条目的语义

需要后续补充：

- 是否允许 image 条目参与扩展名过滤
- 非文件型条目收到 `extensions` 参数时是忽略还是报错

这项不一定阻塞底层索引实现，但会影响 API 合同完整度。

#### 5. URL query 参数索引粒度

当前文档提到：

- 解析 host
- path
- query key 片段

后续还需要确认：

- 是否完全忽略 query value
- 是否要屏蔽常见敏感参数名

这属于安全性和可用性平衡，可在 schema 定稿前再确认。

#### 6. tokenizer 细则

例如：

- 英文最小 token 长度
- 数字是否单独索引
- 中文 bigram 的边界规则
- 停用词是否移除

这些会影响 `index_version`，但可以在进入具体实现前统一补一份 tokenizer 规范。

#### 7. 搜索结果返回字段

当前只定义了请求，没有定义完整响应。

后续需要补充是否返回：

- `total`
- `has_more`
- 命中字段信息
- 高亮片段或高亮所需元数据

这项主要影响前端集成，不一定阻塞底层索引设计。

## 建议的下一步

将本文档继续收敛为可直接实现的产物：

1. port 定义
2. Rust 查询 DTO
3. SQLite schema 草案
4. daemon API 合同
5. 重建与增量更新工作流计划

---

## 附录 A：render 列加密补账（search-v10，2026-07-03）

> 状态：已实现。设计稿见 `.planning/research/2026-07-03-search-render-columns-encryption-design.md`。

### 背景：功能演进绕过了安全模型审查

v1 定稿的「磁盘上不存储明文搜索词」只审查了元数据 + HMAC 化倒排。后续「浏览统一到
search」把五个从内容派生的字段以 **明文** 落进 `search_document`，绕过了安全模型：

- `text_preview`（内容预览）、`file_names`、`link_urls`、`file_paths`、`char_count`（泄露文本长度）。

同时存在一个 **锁定态明文外泄通道**：主历史列表走 filter-only 浏览，该路径当时不派生密钥、
无 session 锁守卫，因此锁定态下 daemon 会对 filter-only 浏览返回 HTTP 200 + 全部明文列。

### 修复（search-v10）

- **列级加密**：五个内容派生字段打包成单个 JSON，经 XChaCha20-Poly1305 封成固定二进制
  信封 `[magic "UCSR"][version][nonce 24B][ciphertext]`，写入新列 `render_payload BLOB`；
  五个明文列 `DROP COLUMN`。稳态下每条被索引的行都写入加密 payload，`NULL` 一律按损坏行降级。
- **独立 render_key**：`HKDF-SHA256(ikm=master_key, salt=profile_id, info="uniclipboard-search-render/v1")`，
  与 search_key（HMAC-PRF）用途分离；AAD `uc:search_render:v1|{entry_id}` 绑定密文到行。
- **锁定态一律 423**：`GET /search/query`（含 filter-only 浏览）与 `GET /search/tags` 在 handler
  入口前置 `session_ready` 校验，未就绪返回 423；引擎层派生 render_key 失败返回 `SessionLocked`
  作为纵深防御。冷启动锁定态解锁后由统一的 session-ready 通知点触发 rebuild/清理补跑。
- **单行降级**：`render_payload` 解码失败（损坏/magic 非法/版本不支持/AEAD 失败/NULL）时该行
  仍返回、render 字段置空，其 entry_id 经引擎内部通道上报 application 层做合并去重的重投影修复
  （不进 API DTO）。
- **回填**：`CURRENT_INDEX_VERSION` bump 到 `search-v10`，启动版本不符触发全量 rebuild，从解密后的
  representations 重新投影并加密写入。迁移不可逆（down migration 故意报错）。
- **明文残留清理**：rebuild finalize 后由 `SearchIndexMaintenancePort::purge_plaintext_residue`
  跑一次 `wal_checkpoint(TRUNCATE)` + `VACUUM` + 清扫游离临时表，完成时间戳记在
  `search_index_meta.plaintext_purge_done_ms`（NULL=待跑，进程被杀下次启动自愈补跑）。
  - **代价（如实标注）**：搜索索引与主剪贴板存储共用同一个 SQLite 文件与连接池，因此
    `VACUUM` 会对 **整个数据库** 加排他锁，重写文件期间会短暂阻塞 **所有** 写入方（剪贴板采集、
    同步等），而不仅是搜索。这是 rebuild 完成后的一次性 best-effort 任务（通常挑在空闲时刻跑），
    靠 `busy_timeout=5000` + 有限次退避重试等过实时写入尖峰；实现会记录 `VACUUM` 耗时日志以便
    发现异常停顿。若未来该阻塞面成为问题，根治方向是把搜索索引拆到独立 DB 文件后再 VACUUM。

### 显式接受的权衡（明文保留）

- `source_device`：被 SQL 下推 `eq_any` 过滤，加密即破坏过滤；内容为稳定 `DeviceId`（非用户
  自定义名），泄露面 =「哪台设备产生了该条目」，属 v1「元数据明文可接受」范畴。
- `payload_state`：可用性标志位（`Present`/`Lost`），非内容。
- `file_extensions` / `mime_type`：仍为明文列（本次修复范围只覆盖上述五个内容派生字段）。

### 姊妹泄漏：`clipboard_entry.title` 明文列（已删除，非加密）

render 列加密堵住了 `search_document`，但同一份内容还从另一处以明文落盘：主存储表
`clipboard_entry.title` 存了每个条目首个文本 representation 的前 200 字符（URL、文件名、
标题原文）。它是 `generate_title` 在 **捕获瞬间** 从 live snapshot 派生的——与 search 的
`render_payload.text_preview`（`build_from_capture` 同一时刻、同一首个文本 rep）**同源冗余**。

因此选择 **删除而非加密**：给一份已被 AEAD 封好的数据的明文副本再加一层密没有意义。全仓 title
仅两个读取点，都是「无内联文本时的预览兜底」——`build_from_persisted` 的 `text_preview` 兜底与
`resolve_preview_text` 的列表预览兜底；title 不进 API DTO / openapi / 前端 / 网络同步 / SQL 下推。
迁移 `2026-07-03-000002_drop_clipboard_entry_title` 直接 `DROP COLUMN title`；down 可逆
（re-add 空 TEXT 列，让旧二进制仍能找到该列）。dropped 明文由上面的 `VACUUM` purge 一并回收。

- **显式接受的代价**：内联字节已卸载到 blob 的大文本条目（`inline_data` 为空）在一次 search
  索引 rebuild 后拿不到预览兜底，列表退化成占位符「Text content (full payload in background
  processing)」。普通内联文本条目不受影响（预览走 `inline_data`）。新捕获仍在捕获瞬间生成
  `render_payload.text_preview`，只是不再有跨 rebuild 的持久兜底副本。

### 诚实边界

文件系统层历史残留（已删除页曾写过盘、TRIM 前的扇区、旧备份）在无全库/全盘加密前提下无法
保证清除，记入威胁模型边界。
