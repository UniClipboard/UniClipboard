# `uc-infra/AGENTS.md`

## 1. 文档目的

`uc-infra` 是 UniClipboard 的基础设施实现层，负责将 `uc-core` 中定义的端口抽象落地为具体实现。

本规范用于约束所有开发者和 AI Agent 在修改 `uc-infra` 时的设计与编码行为，确保：

* 不破坏 `uc-core` 的边界
* 不侵入 `uc-app` 的流程职责
* 基础设施实现清晰、可替换、可测试
* 各种外部依赖被隔离在明确边界内
* 实现细节不会向上泄漏

**任何修改 `uc-infra` 的提交，都必须遵循本规范并完成自我审查。**

---

## 2. `uc-infra` 的定位

### 2.1 核心职责

`uc-infra` 只负责以下事情：

1. **实现 `uc-core` 定义的 ports**
2. **对接外部系统与第三方库**
3. **处理序列化、反序列化、存储格式、协议格式**
4. **处理系统 API、数据库、文件系统、网络、密码学库等具体实现**
5. **提供 adapter / repository / gateway / client 等实现对象**
6. **将技术错误映射为上层可理解的错误**

---

### 2.2 非职责

以下内容**不属于** `uc-infra`：

| 类别        | 示例                               |
| --------- | -------------------------------- |
| 业务规则定义    | 是否允许加入某个 space                   |
| 流程编排      | setup 状态机、join flow、orchestrator |
| UI 状态管理   | 前端状态、页面逻辑                        |
| API 用例组合  | command handler、facade           |
| 启动装配      | wiring、bootstrap、进程入口            |
| 领域模型定义主导权 | 不能由 infra 反过来决定 core 模型长什么样      |

---

## 3. 分层关系

```text
UI / CLI / Daemon API
        ↓
     uc-app
        ↓
     uc-core   ← ports 定义在这里
        ↑
     uc-infra  ← ports 的实现放在这里
```

### 强制规则

* `uc-infra` **可以依赖** `uc-core`
* `uc-infra` **不可以定义业务真相**
* `uc-infra` **不可以绕过 port 直接主导应用行为**
* `uc-infra` **不可以成为“半个 app 层”**

---

## 4. 基本原则

## 4.1 实现层，不是决策层

`uc-infra` 的任务是实现，不是决定业务。

错误示例：

* 因为某个数据库表难设计，就修改 core 模型
* 因为某个网络协议不方便，就改变业务状态机
* 因为某个第三方库字段很多，就把这些字段透传到 core

正确做法：

* infra 适配 core
* 不让 core 适配 infra

---

## 4.2 技术细节必须向下收敛

所有外部依赖都应被隔离在 adapter 内部，不能泄漏到上层。

例如不允许把这些类型泄漏到 `uc-core` / `uc-app`：

* `libp2p::PeerId`
* `sqlx::Error`
* `diesel::result::Error`
* `reqwest::Error`
* `tokio::task::JoinError`
* `std::io::Error` 作为跨层公共错误
* 第三方 SDK 的 response model

---

## 4.3 可替换性优先

每个 infra 实现都必须假设未来可能被替换。

例如：

* SQLite → redb
* libp2p → iroh
* 本地文件存储 → 对象存储
* keychain → 自定义 secret store
* 本地全文搜索 → 嵌入式索引引擎

所以实现必须遵守：

* 依赖 port，而不是让调用方依赖具体类
* 不让实现细节污染公共接口
* 不在 adapter 外部暴露内部格式

---

## 4.4 单一 adapter 单一职责

一个 infra 组件应该只做一件事。

例如：

### 好的拆分

* `SqliteClipboardRepository`
* `FileBlobRepository`
* `Libp2pNetworkAdapter`
* `Argon2KdfAdapter`
* `WindowsCredentialStoreAdapter`

### 不好的拆分

* `SystemService`
* `InfraManager`
* `GlobalRuntimeAdapter`
* `ClipboardSyncRepositoryAndNetworkCoordinator`

任何一个 adapter 如果同时负责：

* 存储
* 缓存
* 网络发送
* 事件重试
* telemetry
* migration

说明已经越界了，需要拆开。

---

## 5. `uc-infra` 允许包含的内容

### 5.1 Repository 实现

例如：

* `SqliteClipboardRepository`
* `SqliteDeviceRepository`
* `SqliteSettingsRepository`

### 5.2 外部系统 adapter

例如：

* `Libp2pNetworkAdapter`
* `OsClipboardAdapter`
* `KeychainSecretStoreAdapter`
* `FileSystemBlobStore`

### 5.3 Gateway / Client

例如：

* HTTP client
* IPC client
* 本地 daemon client
* mDNS / discovery adapter

### 5.4 Codec / Mapper

例如：

* DB record ↔ domain object
* protocol message ↔ domain message
* encrypted blob format ↔ domain blob envelope

### 5.5 Migration / Persistence format

例如：

* schema migration
* 本地文件格式版本迁移
* keyslot 持久化格式兼容

---

## 6. `uc-infra` 禁止包含的内容

### 6.1 禁止流程编排

不允许在 infra 中出现这些职责：

* setup 流程推进
* 配对流程状态机
* “先 A 再 B 再 C”的 use case orchestration
* 用户输入驱动的业务状态流转

这些属于 `uc-app`。

---

### 6.2 禁止业务规则决策

不允许 infra 自己定义：

* 哪个设备应被信任
* 何时允许加入 space
* 何时允许解锁
* 哪种内容应同步
* 是否应保留某条剪切板记录

infra 可以执行校验，但不能拥有业务真相。

---

### 6.3 禁止 UI / API 表示层逻辑

不允许在 infra 中放：

* HTTP response DTO
* Tauri command request/response
* 前端 view model
* 用户提示文案
* daemon API 文本常量

---

### 6.4 禁止“工具箱式公共层”

不要把 `uc-infra` 做成一个什么都能放的技术垃圾场。

例如以下命名要高度警惕：

* `utils.rs`
* `helpers.rs`
* `common_impl.rs`
* `shared.rs`
* `misc.rs`

除非极少数纯技术复用代码，否则应按明确子域归类。

---

## 7. 目录组织规范

推荐优先按能力边界组织，而不是按“库类型”组织。

推荐结构：

```text
uc-infra/
  src/
    storage/
      sqlite/
        mod.rs
        clipboard_repository.rs
        device_repository.rs
        settings_repository.rs
        models.rs
        mappers.rs
        schema.rs
        migrations.rs

      file_blob/
        mod.rs
        blob_store.rs
        blob_codec.rs
        path_layout.rs

    network/
      libp2p/
        mod.rs
        adapter.rs
        event_mapper.rs
        protocol/
        peer_codec.rs

      discovery/
        mod.rs
        mdns_adapter.rs

    crypto/
      mod.rs
      aead_encryptor.rs
      argon2_kdf.rs
      random_bytes.rs
      keyslot_codec.rs

    clipboard/
      mod.rs
      windows.rs
      macos.rs
      linux.rs
      normalizer.rs

    secrets/
      mod.rs
      keychain.rs
      windows_credential_manager.rs

    search/
      mod.rs
      index_adapter.rs
      tokenizer.rs

    time/
      mod.rs
      system_clock.rs
```

---

## 8. Ports 实现规则

## 8.1 实现必须面向 `uc-core` port

任何 infra 实现，都应显式对应某个 port。

例如：

* `ClipboardRepositoryPort` → `SqliteClipboardRepository`
* `BlobRepositoryPort` → `FileBlobRepository`
* `NetworkPort` → `Libp2pNetworkAdapter`

### 强制要求

* 一个实现必须能明确回答：它实现的是哪个 port
* 如果回答不出来，说明职责不清

---

## 8.2 不允许私自扩展 port 语义

infra 实现不能因为底层库能力更强，就偷偷把更多语义带给上层。

例如：

port 只定义：

* `save(entry)`
* `get(id)`

infra 不应在上层公共接口中额外引入：

* 特定数据库游标
* 特定网络连接句柄
* 原始 protocol payload

---

## 8.3 mapper 必须存在明确边界

任何涉及“infra model ↔ domain model”的地方，都应有清晰 mapper。

不允许直接在大量业务代码里手写散乱转换。

例如：

* `SqliteClipboardRow -> ClipboardEntry`
* `Libp2pMessage -> DomainNetworkEvent`
* `BlobFileHeader -> BlobEnvelope`

---

## 9. 错误处理规范

## 9.1 infra 错误必须收敛

infra 内部可以接触原始错误，但向上层暴露时必须收敛。

### 不允许

* 直接把第三方错误类型传播到 `uc-app`
* 在上层 match 第三方库错误码
* 到处透传 `anyhow::Error` 作为边界类型

### 推荐方式

```rust
pub enum BlobStoreError {
    NotFound,
    PermissionDenied,
    CorruptedData,
    Io(String),
}
```

或者在 adapter 内部保留源错误链，但边界类型仍然是本项目定义的错误。

---

## 9.2 不允许静默吞错

不允许：

* `let _ = ...`
* `ok()`
* `unwrap_or_default()` 掩盖真正错误
* 后台任务失败但无日志
* catch 后直接返回空结果

必须做到：

* 有日志
* 有上下文
* 错误能被上抛到正确层级
* 非关键错误也要可观测

---

## 9.3 错误语义必须稳定

不要把底层库的错误语义直接当成系统语义。

例如：

* “SQLITE_BUSY” 不是业务语义
* “Peer closed stream” 不是业务语义
* “InvalidNonceLength” 不是业务语义

应转成上层能理解的语义：

* `TemporaryUnavailable`
* `TransportClosed`
* `CorruptedEncryptedPayload`

---

## 10. 日志与 tracing 规范

## 10.1 `uc-infra` 必须可观测

所有关键适配器必须有 tracing。

至少覆盖：

* 初始化
* 外部资源连接/打开/关闭
* 关键读写
* 失败路径
* 重试路径
* 降级路径
* 数据损坏/兼容分支

---

## 10.2 日志不得泄露敏感数据

严禁打印：

* 明文剪切板内容
* 明文 passphrase
* 原始密钥
* 完整 token / secret
* 可直接恢复用户数据的 payload

允许打印：

* ID
* 长度
* 类型
* hash 截断
* 状态
* 错误类别

---

## 10.3 日志应面向排障，而不是面向表演

日志要回答这些问题：

* 调用了哪个 adapter
* 操作了什么资源
* 输入输出的边界条件是什么
* 为什么失败
* 是否可重试
* 当前影响范围是什么

不要写无效日志，例如：

* `"something failed"`
* `"error occurred"`
* `"done"`

---

## 11. 数据与格式规范

## 11.1 持久化格式必须显式版本化

任何落盘格式、协议格式、缓存格式，只要未来可能演进，就必须考虑版本化。

例如：

* blob 文件头
* keyslot JSON
* network protocol payload
* local metadata file

### 推荐

* `version` 字段
* 明确 backward compatibility 策略
* 迁移逻辑单独管理

---

## 11.2 不允许把内部格式泄漏为公共契约

infra 内部格式不是产品公共语义。

例如：

* 数据库表字段名
* 文件目录布局
* network frame 字段顺序

都不应被上层依赖。

---

## 11.3 mapper 不得偷偷补业务默认值

如果一个 domain 字段缺失，不应在 infra mapper 中随意拍脑袋补默认值，除非该默认值是明确的、稳定的、被业务接受的契约。

否则应：

* 返回错误
* 走兼容迁移
* 或显式记录 fallback 行为

---

## 12. 测试规范

## 12.1 `uc-infra` 必须重视集成测试

这里只做单元测试是不够的。

必须覆盖：

* 真实数据库读写
* 真实文件系统行为
* 序列化/反序列化兼容
* 协议收发
* 失败与恢复路径
* 边界值与损坏数据

---

## 12.2 优先测试适配器契约

测试不应只验证“代码跑了”，而应验证“是否满足 port 契约”。

例如测试 `BlobRepositoryPort` 的实现时，至少覆盖：

* save 后可 get
* 不存在返回 NotFound
* 损坏文件返回 CorruptedData
* 并发或重复写入时行为符合预期

---

## 12.3 必须测试损坏与异常路径

凡是 infra，就一定会遇到脏数据、半写入、权限问题、路径不存在、连接中断、协议不兼容。

这些必须测试，不能只测 happy path。

---

## 13. 性能与资源规范

## 13.1 `uc-infra` 必须显式关注资源占用

尤其你这个项目是剪切板工具，后台常驻。

必须注意：

* 避免无界缓存
* 避免重复序列化/反序列化
* 避免全量读大文件
* 避免主线程阻塞
* 避免无上限重试
* 避免长期持有大对象

---

## 13.2 大 payload 处理必须有策略

涉及图片、文件、富文本时：

* 尽量流式处理
* 明确内存上限
* 明确临时文件策略
* 明确失败回滚策略

---

## 13.3 后台任务必须可停止、可感知失败

任何 watcher、subscription、event loop、network listener 都必须：

* 可关闭
* 有退出日志
* 失败可见
* 不允许悄悄死掉

---

## 14. 平台相关实现规范

## 14.1 平台差异留在 infra 内部

Windows / macOS / Linux 的差异必须留在 `uc-infra` 或 `uc-platform`，不能上浮到 core。

例如：

* 剪切板读取格式差异
* 文件路径差异
* keychain / credential manager 差异
* daemon socket / IPC 差异

---

## 14.2 不允许平台条件编译污染公共业务接口

`cfg(target_os = "...")` 可以存在，但应尽量收敛在实现文件内部，而不是散落在上层公共逻辑里。

---

## 15. 依赖管理规范

## 15.1 引入新依赖前必须回答

1. 这个依赖解决的是 infra 问题，还是 app/core 问题？
2. 是否已经有现有依赖可完成？
3. 是否会把该库类型泄漏到上层？
4. 是否容易替换？
5. 是否会显著增加构建复杂度、平台负担或体积？

---

## 15.2 优先选择可控依赖

优先：

* 文档完整
* 维护稳定
* 平台兼容明确
* API 边界清晰
* 失败语义明确

谨慎对待：

* 宏过重
* 全局运行时强绑定
* 平台 hack 很多
* 错误语义混乱
* 类型过度侵入上层设计

---

## 16. 命名规范

| 类型      | 规则                                    | 示例                     |
| ------- | ------------------------------------- | ---------------------- |
| Port 实现 | `*Adapter` / `*Repository` / `*Store` | `Libp2pNetworkAdapter` |
| DB 行模型  | `*Row` / `*Record`                    | `ClipboardEntryRow`    |
| 协议模型    | `*Frame` / `*Message`                 | `PairingFrame`         |
| 映射器     | `*Mapper` / `map_*`                   | `ClipboardRowMapper`   |
| 错误      | `*Error`                              | `SqliteBlobStoreError` |

避免模糊命名：

* `Manager`
* `Service`
* `Helper`
* `Processor`
* `Engine`

除非职责非常明确。

---

## 17. 提交前自我审查清单

每次修改 `uc-infra`，必须逐项自查：

### 17.1 边界检查

* [ ] 这次修改是否只是基础设施实现，而不是业务规则或流程编排？
* [ ] 是否有任何 `uc-core` 不该知道的技术细节被上浮？
* [ ] 是否有任何 `uc-app` 的流程职责被下沉到 infra？

### 17.2 port 检查

* [ ] 这个实现是否明确对应某个 port？
* [ ] 是否出现了绕过 port 的调用路径？
* [ ] 是否把底层库类型暴露给了上层？

### 17.3 错误检查

* [ ] 是否存在静默吞错？
* [ ] 是否把第三方错误原样传播到边界外？
* [ ] 是否为关键失败路径增加了可观测日志？

### 17.4 数据检查

* [ ] 持久化或协议格式是否需要版本化？
* [ ] mapper 是否显式、集中、可测试？
* [ ] 是否出现隐式默认值补齐或脏数据掩盖？

### 17.5 性能检查

* [ ] 是否引入无界缓存或大对象常驻？
* [ ] 是否有潜在阻塞或重复拷贝？
* [ ] 大 payload 是否有合理策略？

### 17.6 测试检查

* [ ] 是否覆盖真实失败路径？
* [ ] 是否验证 port 契约，而不只是验证代码运行？
* [ ] 是否包含损坏数据、边界值、兼容路径测试？

---

## 18. Code Review 重点

评审 `uc-infra` 时，优先检查：

1. 是否越权承担了 app/core 的职责
2. 是否有第三方类型泄漏
3. 是否有隐式协议耦合
4. 是否有静默吞错
5. 是否有不可替换的实现绑定
6. 是否有平台差异上浮
7. 是否有缓存、重试、并发资源泄漏风险

---

## 19. 反模式清单

以下是 `uc-infra` 中必须警惕的典型反模式：

### 19.1 以实现反推领域

“因为 libp2p 就长这样，所以 core 也这样建模。”

这是错误方向。应该是 adapter 适配领域，而不是领域屈从实现。

---

### 19.2 adapter 长成 orchestrator

一个网络 adapter 开始负责：

* 决定是否重试
* 决定是否接受某设备
* 决定状态如何切换
* 决定何时持久化

这已经不是 infra 了。

---

### 19.3 数据库存储格式被当成领域模型

`Row` / `Record` / `Schema` 不能等同于领域实体。

---

### 19.4 把兼容逻辑散落各处

版本兼容、迁移、fallback 必须集中管理，不能到处 `if version == ...`

---

### 19.5 “先能跑”式的错误吞并

infra 是最接近失败源头的一层，这里如果吞错，上层就会完全失明。

---

## 20. 总原则

`uc-infra` 必须遵守这四条：

### 20.1 对上层隐藏实现细节

### 20.2 对下层真实面对外部复杂性

### 20.3 不定义业务，只实现业务所需能力

### 20.4 保持可替换、可测试、可观测

---

## 21. 一句话原则

> `uc-infra` 的职责不是“让系统先跑起来”，而是“用清晰、可替换、可观测的方式，把 `uc-core` 的抽象稳定落地”。
