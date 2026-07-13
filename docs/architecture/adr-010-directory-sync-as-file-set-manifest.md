# ADR-010: 目录同步建模为逐文件 manifest（EntryFileSet），而非归档 blob

Status: accepted (2026-07-03)

## 决策

支持「复制/同步目录」时，目录 **不打包成单一归档 blob 传输**，而是展开为逐文件
manifest：复用 dedup 计划的 `entry_file_set` 逐行表，每行以
`(root_index, relative_path)` 定位成员，成员文件各自走既有的单文件 blob 通道。
「复制 3 个文件」「复制 1 个文件夹」「混合选择」统一为同一个概念——entry 的
文件集（**EntryFileSet**）；目录不是独立聚合。

## 被否掉的方案：tar/zip 归档为单一 blob

归档方案改动面小（传输层仍只见一个 blob），但被否，理由：

- **废掉内容级去重**：目录里改一个文件，归档整体 hash 变、全量重传；manifest
  方案只重传变化的成员。与进行中的 dual-channel dedup 设计（内容寻址、
  `entry_file_set`）方向正冲突，等于开一条平行旧逻辑。
- 打包需要与目录等大的临时磁盘空间。
- `max_file_size` 的每文件语义被改变成对归档总量限制。
- 跨平台归档元数据（权限、符号链接）是持续的坑。

## 关键连带决策

1. **结构入身份**：含目录的文件集，`snapshot_hash` 的文件内容组件由逐成员
   leaf 聚合——`BLAKE3("file-set-member-v1|" ‖ root_name ‖ 0x00 ‖ relative_path
   ‖ 0x00 ‖ kind_tag ‖ 0x00 ‖ file_digest)`，组件 = `BLAKE3("file-set-v1|" ‖
   sort(leaves))`。relative_path 经 NFC 归一化、`/` 分隔；kind_tag `f`/`x`
   （可执行）/`d`（空目录），exec bit 影响粘贴产物故入身份。**版本化落在文件
   内容组件**（`file-content|` → `file-set-v1|`），外层 `snapshot-hash-v1|`
   聚合不动——纯裸文件集合哈希逐字节不变，存量 entry 身份零迁移。
2. **全有或全无的接收语义**：全部成员完成后才重建目录树、改写 uri-list；部分
   失败按 Entry transfer summary 聚合为 `Failed`，手动重发靠 blob 内容去重
   增量补缺。不粘贴静默缺文件的目录（准数据丢失比失败更糟）。
3. **捕获侧护栏**：捕获热路径只做毫秒级元数据预检（文件集总大小上限 ~1 GiB、
   成员数上限 ~2000，`file_sync` 设置可调）；内容哈希/摄取异步（Deferred
   snapshot identity），就绪前不 dispatch、不广播；摄取毕做 `(mtime, size)`
   漂移复核，目录被改则放弃。内容 hash 物理上受磁盘 I/O 限制（1 GiB ≈ 秒级），
   不可能毫秒级；元数据身份（mtime）方案因跨设备不可比、破坏 pull 反查而被否。
4. **成员边界**：symlink 与特殊文件（FIFO/socket/设备节点）→ 整个目录判
   Sync-ineligible（不静默跳过、不解引用）；隐藏文件包含；硬链接当独立文件；
   除 exec bit 外权限/xattr/时间戳不保留。
5. **范围**：桌面三平台；移动端不消费 **含目录结构的** 文件集 entry（register
   指向它时 mobile GET 保持上一个可消费值）。单文件与平铺多文件仍可由移动端
   消费。前置依赖：dedup 计划的 `entry_file_set` 表先落地。

领域术语与完整语义见 `CONTEXT.md`「Language — 文件集与目录」。
