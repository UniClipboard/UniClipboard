# uc-engine 跨平台核心接口

`uc-engine` 是桌面与移动宿主使用完整 P2P 核心的唯一稳定入口。宿主只负责提供私有目录、安全存储、系统剪贴板和文件句柄；数据库、加密存储、搜索、设备身份、连接、传输和后台任务均由核心拥有。

## 启动与事件

宿主调用 `Engine::start(config, host)`，成功后同时得到核心实例和一条持续有效的事件流。启动会打开当前资料空间的加密存储、恢复同一设备身份、启动完整 P2P 节点并启动核心后台任务。

事件流采用有限容量。消费者落后时不会收到伪造或不完整的数据，而是收到 `RefreshRequired(ConsumerLagged)`，随后应重新查询当前状态。

核心事件包括：

| 事件 | 含义 |
| --- | --- |
| `StateChanged` | 生命周期状态已经改变 |
| `IncomingEntry` | 收到一条具有完整摘要的新内容 |
| `TransferProgress` | 文件传输进度发生变化 |
| `RefreshRequired` | 宿主必须重新查询当前状态 |
| `OperationFinished` | 一次操作进入成功、失败或取消终态 |
| `Fatal` | 核心遇到不可恢复错误 |

当底层变化事件不包含完整条目摘要时，核心只发送 `RefreshRequired(StateInvalidated)`，不得猜测内容类型、时间或预览。

## 生命周期

合法顺序如下：

```text
Running -> Quiescing -> Quiesced -> Suspended -> Running
Running|Quiescing|Quiesced|Suspended -> ShuttingDown -> Stopped
```

- `quiesce(deadline)`：停止接收新操作，并等待正在执行的操作结束。期限到达后取消剩余操作。
- `suspend()`：先停止接收操作，再释放节点、会话级后台任务和连接。事件流与核心实例保持不变。
- `resume()`：使用原持久数据和原设备身份重建节点与后台任务，不会自动重试暂停前被取消的操作。
- `shutdown(deadline)`：停止操作、节点和全部后台任务，然后关闭事件流。
- 进程被系统结束后，宿主重新调用 `start`；不得尝试恢复旧内存实例。

除 `Running` 外的状态均拒绝新操作。生命周期方法和 `execute` 可从不同线程调用，核心内部负责串行化状态转换。

## 公开操作

| 操作 | 当前行为 |
| --- | --- |
| `CreateSpace` | 创建空间、设备身份和加密存储 |
| `UnlockSpace` | 使用口令恢复当前空间会话 |
| `RecoverSession` | 按宿主策略从系统安全存储恢复加密与空间会话 |
| `JoinSpace` | 首次设备加入空间；已设置设备保留历史并切换空间 |
| `IssueInvitation` | 签发一次配对邀请 |
| `CancelInvitation` | 取消当前尚未兑换的配对邀请 |
| `ResetSpace` | 清除当前空间设置，使设备回到未设置状态 |
| `FactoryResetSpace` | 依次清除密钥材料、空间设置和待处理邀请，使设备可重新初始化 |
| `QuerySetupState` | 查询设置是否完成、当前邀请和已保存设备名 |
| `QueryMigrationProgress` | 查询空间切换所处阶段和已备份记录数量 |
| `QueryStorageStats` | 查询数据库、密钥库、缓存和日志占用大小，不返回本机目录 |
| `ClearStorageCache` | 清理核心缓存并返回实际释放的字节数 |
| `QueryLocalDevice` | 返回本机设备编号和按设置解析后的显示名 |
| `QueryEncryptionState` | 查询当前空间是否已初始化、加密会话是否可用 |
| `LockEncryption` | 清除当前加密会话并关闭接收入口 |
| `VerifySecureStorageAccess` | 检查宿主安全存储是否可在当前环境中访问 |
| `ListDevices` | 返回设备编号、显示名和在线状态 |
| `QueryMemberSyncPreferences` | 查询指定成员的发送、接收和内容类型偏好 |
| `UpdateMemberSyncPreferences` | 局部更新指定成员的同步偏好，未提供字段保持不变 |
| `RemoveMember` | 删除本机保存的成员及其信任和地址关联记录 |
| `SearchEntries` | 使用关键词、时间、内容类型、来源设备和标签等条件查询加密搜索索引 |
| `QuerySearchTags` | 查询当前索引中的标签和条目数量 |
| `QuerySearchStatus` | 查询索引是否可用及最近重建时间 |
| `RebuildSearchIndex` | 请求重建当前加密搜索索引 |
| `SendText` | 写入加密历史、更新搜索并发送不超过 64 KiB 的文本 |
| `SendImage` | 写入加密历史、更新搜索并发送不超过 64 KiB 的图片 |
| `QueryHistory` | 查询历史并返回稳定分页标记 |
| `ExportEntry` | 通过宿主文件句柄分块写出主内容 |
| `ResendEntry` | 重新发送一条本机仍持有内容的历史记录 |
| `SendFiles` | 从宿主句柄分块导入文件，并按现有文件协议发送 |

`RecoverSession` 的 `allow_secure_storage_unlock` 由宿主根据当前运行环境决定。值为 `false` 时核心不得尝试从系统安全存储恢复密钥；值为 `true` 时，核心统一完成加密会话、空间会话、搜索和接收能力恢复。

`CancelInvitation` 在没有待取消邀请时返回冲突错误。`ResetSpace` 只清除当前空间设置，不向宿主暴露底层存储步骤。`FactoryResetSpace` 则先清除密钥材料，再清除空间设置和待处理邀请；密钥清除失败时不得提前清除设置，成功后必须关闭接收入口。`QuerySetupState` 不返回内部服务状态；`QueryMigrationProgress` 只返回准备、握手完成、切换完成三个稳定阶段，不公开内部运行编号或目标空间。

`QueryStorageStats` 和 `ClearStorageCache` 由核心执行。宿主只能看到分类后的字节数和实际释放量，不能取得数据库、密钥库、缓存或日志的本机路径。

`QueryLocalDevice` 的显示名由核心统一读取并规范化；设置缺失、读取失败或名称为空时使用稳定默认名称。调试输出不得包含显示名。

`QueryEncryptionState` 只返回初始化和会话可用状态。`LockEncryption` 成功后必须同时关闭接收入口，避免锁定后继续写入加密业务数据。`VerifySecureStorageAccess` 使用跨平台安全存储语义，宿主接口可按平台显示为 Keychain、Keystore 或对应系统名称。

`QueryMemberSyncPreferences` 和 `UpdateMemberSyncPreferences` 只接受稳定设备编号。局部更新中未提供的开关和内容类型必须保持原值。`RemoveMember` 由核心统一删除成员、信任和地址关联记录；宿主不得自行编排这些持久化步骤。

搜索查询、标签、状态和重建都由核心执行。搜索结果可以正常返回预览、文件名、文件路径、链接和自定义标签，但这些用户内容不得出现在调试输出或日志中。加密会话锁定时，宿主不得读取搜索结果、标签或状态，也不得触发重建。

空目标设备列表表示向所有符合条件的可信设备发送；非空列表只会缩小目标范围，不能绕过信任、在线状态和发送设置。

`SendText` 和 `SendImage` 的内容必须为 1 到 64 KiB，大小按实际字节数计算。超出范围返回输入错误，不会写入历史或进入文件传输缓存。更大内容通过文件入口发送。

历史查询每页必须为 1 到 200 条。分页标记是不可解释的稳定字符串，当前版本形如 `uc-history-v1:<offset>`；宿主必须原样回传，不应自行生成或修改。损坏、未知版本或越界输入返回输入错误。

导出只写宿主传入的目标句柄。核心看不到目标路径，每次最多写 64 KiB，并在全部数据写入后调用完成写入。取消操作时不会在恢复后续写。

## 错误

公开错误只包含稳定编号、类别和是否可重试，不包含底层原因或用户内容。详细原因只进入脱敏日志。

| 类别 | 含义 |
| --- | --- |
| `InvalidInput` | 输入、分页标记、文件句柄或内容类型无效 |
| `InvalidState` | 当前生命周期或空间状态不允许该操作 |
| `Unauthorized` | 口令错误、目标未授权或宿主无权限 |
| `NotFound` | 邀请、设备、记录或资源不存在 |
| `Conflict` | 已初始化、没有可发送目标或内容不可重发 |
| `Unavailable` | 网络、索引、宿主能力或临时服务不可用 |
| `DeadlineExceeded` | 操作超过约定期限 |
| `Internal` | 无法向宿主公开细节的内部失败 |

每次被接受的操作都会产生一个 `OperationFinished` 终态事件。被生命周期期限取消的操作必须产生 `Cancelled`，不能只返回错误后静默消失。

## 宿主能力与存储

- 私有数据目录用于数据库、加密内容、文件内容和设备身份。
- 缓存目录中的非文件业务内容同样必须先加密。
- 临时目录不能用于写入明文非文件业务内容。
- 安全存储保存密钥材料，调试输出必须脱敏。
- 剪贴板读取与写入通过宿主能力完成。
- 文件只能通过不透明句柄分块读写，句柄不能伪装成本机路径。

宿主不得自行保存剪贴板正文、预览、标题、标签名、文件名或文件路径。除内容类型分类和文件内容本体外，所有持久化业务负载必须先经 MasterKey AEAD 加密；文件内容由核心拥有的 blob store 或导入目录按原始字节保存。

## 文件发送

`SendFiles` 通过 `HostFileAccess` 从宿主文件句柄分块读取内容，并交给核心拥有的导入目录和 blob store。文件内容允许按原始字节落盘，以保持现有 P2P 文件格式；文件名、宿主路径和关联元数据仍不得明文持久化。
