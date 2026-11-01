# 接收项结果与选择性重试

Desktop 适配 Engine PR #230（`UniClipboard/Engine` 提交 `5cbca8fc24a278fa7b01ae611bf4b925914bef13`）。尝试、重试、取消、持久化和 SDK 关闭都由 Engine 负责；Desktop 只把 Engine 的结论传给界面，并把用户的选择传回 Engine。

## 依赖固定

- `Cargo.toml` 把 `uc-engine` 固定到上述完整提交，`Cargo.lock` 同步。
- Engine 要求 `tokio >= 1.53.3`（锁定为 1.54.1）以及 iroh-blobs 分支 `152c42a9217f1377a998b271e0052311e8ada789`。`crates/p2p-bench` 直接引用该分支，所以它的修订随 Engine 一起移动，工作区仍只解析出一个 `iroh-blobs`。
- 本次未改变：`RUSTSEC-2026-0387`（`crypto-bigint` 0.5.5 与 0.7.5）、`RUSTSEC-2026-0385`（`ctutils`）和三项 `zip` 1.1.4 公告在升级前的锁文件里就存在，升级后不变。升级只消除了 `tokio` 1.53.1 的警告（`RUSTSEC-2026-0386`）。没有新增审计豁免。Engine 自身失败的审计任务和 MBX R2 任务是另外的问题，Desktop 不处理。

## Daemon API

所有路由都在已有的 `/clipboard` 下，沿用 `{ data, ts }` 信封。已有字段不变，新字段只增不改。

| 路由 | 变化 |
| --- | --- |
| `GET /clipboard/entries/{id}/receive`、`GET /clipboard/receives` | `EntryReceiveProgressResponse` 包含 `snapshotVersion` 和 `items[]`；每项包含结果、内容定位、当前轮次字节及可选的历史文件索引 |
| `POST /clipboard/entries/{id}/receive/cancel` | 新增结果 `cancellation_incomplete` |
| `POST /clipboard/cancel-transfer/{transfer_id}` | 新增结果 `cancellation_incomplete` |
| `POST /clipboard/entries/{id}/receive/retry` | 新增：请求体 `{ expectedAttemptId, itemIds }`，结果 `{ outcome, attemptId?, reason? }` |

重试路由的 `outcome` 取值为 `accepted`、`already_receiving`、`superseded`、`cancellation_incomplete`、`unavailable`。`reason` 只随 `unavailable` 出现（`source_unavailable` 或 `access_denied`），`attemptId` 只随 `accepted` 出现。空选择或格式错误的编号返回 HTTP 400，其他 Engine 错误沿用已有的脱敏映射。`itemIds` 必须取自最近一次查询：它们不透明、绑定被查询的尝试，不得由文件名推导。早于 `items` 的旧 daemon，客户端按空列表处理。

`file-transfer.receive_progress_changed` 通知界面重新查询接收快照。事件不携带项目数组，只用来使界面持有的状态失效。快照版本和逐项字节使用十进制字符串，避免 JavaScript 丢失 64 位精度。

## 界面保持的语义

| 规则 | 落实位置 |
| --- | --- |
| `accepted` 只表示接纳了新尝试；文件只有在之后的查询显示 `completed` 才算保存成功 | `retryOutcomeMessageKey` 的措辞；每次重试后 hook 重新读取 snapshot |
| 整体 `failed` 或 `cancelled` 的尝试仍可含已保存文件 | `summarizeReceiveItems`；按项渲染，失败时不移除任何项 |
| 已保存项不重试、不删除 | `retryableItemIds` 只选 `failed`/`cancelled` 尝试里的 `failed`/`cancelled` 项 |
| 每个文件独立显示接收字节和保存状态；字节达到总量不代表保存成功 | `HistoryKitFileReceiveProgress` 按 Engine 项状态呈现 |
| 目录是一个 `directory_unit` 项，整体重试 | 由 Engine 归并；界面不展开成员 |
| `cancellation_incomplete` 不等于已取消 | `markReceiveCancellationIncomplete`；只在该尝试结束时清除 |
| 旧尝试的迟到事件或迟到回复不能覆盖新尝试 | `fileTransferSlice` 的 `supersededAttemptsByEntry` 忽略已被取代的尝试；已结束的尝试不会被更旧的 snapshot 重新打开 |
| 事件只是失效通知，snapshot 才是事实 | 事件与已存 snapshot 不一致时 `useEntryReceive` 重新读取，且只采用最新一次请求的回复 |
| 重启后的 `source_unavailable` 如实呈现 | 重试结果 `unavailable` 有独立文案，已有结果保持不变 |
| 发送端重发不等于接收端重试 | 重发保留自己的路由和 hook；重试只存在于已接收的记录 |
| 不增加宿主调度器、重试循环或数据库访问 | 均未新增；Go 经 HTTP/WS 访问 daemon，没有 Go FFI 层 |

## 代码位置

- DTO：`crates/uc-daemon-contract/src/api/dto/clipboard_command.rs`
- 路由与映射：`crates/uc-webserver/src/api/clipboard.rs`；OpenAPI 注册在 `crates/uc-webserver/src/api/openapi.rs`；生成物为 `schema/openapi.json` 与 `apps/gui-go/frontend/src/api/generated/`
- Rust 客户端：`crates/uc-daemon-client/src/http/clipboard.rs` 中的 `retry_entry_receive`
- 前端逻辑（macOS、Windows、Linux 共用）：`src/lib/receive-items.ts`、`src/hooks/useEntryReceive.ts`、`src/hooks/useCancelEntryTransfer.ts`、`src/store/slices/fileTransferSlice.ts`、`src/api/file_transfer.ts`
- 前端视图：`src/pages/history-kit/HistoryKitReceive.tsx`（三个平台使用的 kit）与 `src/components/clipboard/ReceiveItemsPanel.tsx`（只有不带平台的构建才会到达的旧版历史详情）。两者都是 `useEntryReceive` 之上的视图。kit 详情在接收进行中提供取消按钮（`HistoryKitDetail.tsx`），经 `useCancelEntryTransfer` 取消。
- 文案：六种语言的 `clipboard.receive.*`

## 失败模型与检查

先于代码写成；每一行首次运行都失败，现在通过。

| 失败情形 | 检查 |
| --- | --- |
| 重试路由缺失，空选择或格式错误的选择被接受 | 针对真实 `uniclipd` 的 `tests/e2e/tests/entry_receive.rs` |
| 没有接收时重试却报告 `accepted` | 同一文件；Engine 返回 `unavailable` / `source_unavailable` |
| OpenAPI 缺少重试或项目 schema | 同一文件，读取 daemon 的 `/api-docs/openapi.json` |
| 取消未完成被报成已取消、重试结果映射错误、`items` 丢失、线上键名不是 camelCase | `uc-webserver`、`uc-daemon-contract`、`uc-daemon-client` 的单元测试 |
| 兄弟项失败掩盖已保存文件、重试发错编号、旧尝试复活、取消未完成显示为已取消 | Vitest：`receive-items`、`fileTransferSlice.receiveItems`、`file_transfer.receiveItems`、`ReceiveItemsPanel`、`useCancelEntryTransfer` |

## 未执行

部分送达的接收需要两台已配对设备，单个隔离 daemon 无法产生。未执行：跨设备送达与取消、对真实失败发送方的逐文件重试、目录发布与回滚、带原始重试材料的重启、混合版本、物理设备，以及新界面行（含 kit 取消按钮）的全部视觉检查（视觉验收由任务负责人完成）。

## 逐文件进度补充

Engine PR #370 增加内容定位、逐项字节、历史文件索引及快照版本。Desktop 使用这些字段关联文件详情；目录单元仍按整体处理。本次 macOS 原生隔离验证传输了三个文件并校验哈希。浏览器夹具覆盖三种皮肤；Windows/Linux 原生传输与 Android 真机未验证。
