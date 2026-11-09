# 0011 执行记录：本机 UniClipboard v1 历史迁移

- 日期：2026-11-08。
- 工作分支：`feat/v1-history-migration`。
- 独立 worktree：`/Users/LWP1/Projects/uni-v1-migration`。
- 设计：[0011](../decisions/proposed/0011-v1-history-migration.md)。
- 原始需求：三平台 arm64/amd64，迁移可恢复的文本、图片、文件、标签；新目录与已有历史合并都支持。

## 已实现

- [x] 独立 Go `unic-migrate`，参照相邻旧 Rust 仓库 `1.1.0-rc.23` 读取，不链接旧 Engine。
- [x] 原始二进制系统凭据读取；终端无回显旧口令、宿主 stdin 口令；不写回旧凭据。
- [x] KeySlot、Argon2id、MasterKey、profile-secrets、内容密钥目录与 V3 manifest/generation。
- [x] 有界 postcard 解码、XChaCha20-Poly1305、HKDF、规范 AAD、UCP3、UCBL/zstd。
- [x] 按旧 `ContentHash` 的 `blake3v1:<64 hex>` 校验 blob 与已知本地文件摘要，拒绝未知算法和长度。
- [x] 只读 SQLite 事务快照，排除软删除记录，串行有界遍历。
- [x] 文本、HTML、PNG；JPEG/TIFF/WebP 转 PNG；文件自有复制；目录 ZIP；标签名称与关联及无关联定义。
- [x] Engine 离线导入，单条内容与标签/幂等映射事务发布；不联网、不应用剪贴板、不淘汰既有历史。
- [x] 稳定来源标识、同内容不同记录保留、失败续跑、源目录移动后仍去重。
- [x] 保留旧时间，并与已有历史按创建时间合并；复合分页游标固定提交上界。
- [x] 导入容量下限持久化，下一次普通启动继续生效。
- [x] 关闭并重开目标，通过公开 API 校验内容摘要。
- [x] `unic migrate` 启动相邻 helper；结构化阶段与数量报告、部分完成退出码、SIGINT 取消。
- [x] Go 桌面默认历史页：预览、标签筛选、分页和使用内容。
- [x] 六平台 CLI 构建脚本与 SHA256SUMS。

## 验证证据

Rust 测试生成器仅使用固定非个人数据，独立调用 serde/postcard、Argon2、XChaCha20、HKDF、BLAKE3 与 zstd。夹具覆盖文本、HTML、PNG、文件二进制、标签定义与关联、无关联标签、重复正文、软删除和损坏后修复。

自动测试覆盖：

- 正确/错误口令、系统凭据失败、原始二进制 KEK；错误 AAD、篡改正文和 blob。
- 部分导入、修复续跑、重复运行跳过、移动旧目录保持来源身份；删除旧源后仍可读目标文件。
- 已有历史合并、相同正文多记录、长中文标签、无关联标签、同时间与跨页顺序。
- 截断 reader 不发布记录或映射；极小配置容量不导致迁移淘汰；持久容量下限重开可读。
- 本地文件字节变化被摘要检查拒绝；ZIP 保留相对目录与空目录；拒绝源目标包含关系及符号链接重叠。

最终 `go test -race ./...` 通过 317 个测试（34 个包），`go vet ./...`、三个本机入口构建、六平台 CLI 构建和 `git diff --check` 均通过。产物在 `dist/migration/`，开发入口在 `.build/`。全仓 lint 仍有 124 个原有问题；本次新增文件与修改行无新增问题。

## 本机验收状态

旧源默认目录为 `~/Library/Application Support/app.uniclipboard.desktop`，检测到 V3 活动 profile。初次只读检查约 739 条历史，122 个 blob；本机无标签定义，标签由固定夹具验证。旧应用仍运行时条数可能增加。

真实数据发现并修复了三项旧格式差异：

1. `ContentHash` 保存 `blake3v1:` 前缀，需按 Rust 解析规则校验。
2. 旧后台 TIFF→PNG 后更新 MIME 与 blob，但保留 representation 原大小。已认证并通过自身大小、摘要校验的 blob 使用真实长度；不拿旧大小拒绝可恢复图片。Rust 夹具覆盖此情况。
3. 本地文件清单的 `size_bytes=NULL` 表示未知，不能当作 0 字节；未知时读取文件实际大小，有值时继续严格校验。

已在用户授权读取 macOS 钥匙串后，实际写入默认目标 `~/Library/Application Support/unic`。早期试验导入曾保留部分文件内容；在确认仅有本次创建、revision=1 的迁移条目后，先备份，再重导这些试验条目，未删除原有身份、配置或其他历史。

最终本机结果：

| 项目 | 数量 |
|---|---:|
| 旧源未软删除历史 | 739 |
| 已导入、重开后内容/时间/标签校验通过 | 736 |
| 其中目标图片历史 | 70 |
| 其中目标文件历史 | 15 |
| 其中目标文本/富文本历史 | 651 |
| 无法恢复的整条文件历史 | 3 |
| 缺失内容部分（含多表示及文件集合中的部分内容） | 64 |
| 校验失败 | 0 |
| 再次执行：跳过 / 新增 | 736 / 0 |

3 条不可恢复文件历史已逐条检查，原路径均不存在。本机无标签定义，标签定义与关联由 Rust 夹具和 Engine 合并测试验证。64 个缺失部分按旧来源类型分组为 file=59、image=2、rich_text=2、text=1；不等于 64 条整条历史，不宣称全部原始附件均可用。当前终态为 `partial`。

使用 opt-in `TestLocalMigratedHistoryVisible` 通过与 GUI 相同的 UNIX socket HTTP 客户端，分页读取目标并检查各类型预览：736 条可见，files=15、image=70、text=651。此验证未启动网络、观察剪贴板或应用任何历史。重复迁移验证同样关闭并重开 Engine，736 条全跳过且全校验通过。

仅在显式指定个人目录时才执行这些验收：

```sh
UNIC_V1_TEST_SOURCE="旧目录" UNIC_V1_TEST_TARGET="目标目录" go test -v ./internal/v1migration -run '^TestLocalSource$' -count=1
UNIC_V1_TEST_TARGET="目标目录" go test -v ./app/desktop/internal/host -run '^TestLocalMigratedHistoryVisible$' -count=1
```

第一条会实际导入并重复验证目标。默认自动测试不会读取个人目录或钥匙串。临时备份在最终校验通过后清理，固定 Rust 夹具的编译缓存也已清理。

操作命令见[桌面说明](../../app/desktop/README.md#从本机-uniclipboard-v1-迁移)。

## 交付边界

- 主验收格式是本机 V3。早期 JSON inline/UCBL V1/UCFS 分支未逐个历史发布版本验收。
- Windows/Linux 系统凭据路径已实现并跨编译，尚无真实机器授权运行证据。CLI 未交付平台安装包或生产签名。
- 集成入口是 Go 桌面 CLI；尚无图形化目录选择、口令与迁移进度向导。SwiftUI 客户端路线已取消，后续 GUI 由 mygo + 自定义 UI Kit 承载，界面原型待定。
- 丢失文件、损坏密文、超限记录显示部分完成，不生成虚假的可用占位内容。当前报告为数量级，未提供逐条修复界面。
- 不迁移收藏、设备身份与信任、设置和宿主专用非标准 representation。文件夹以 ZIP 表达。
- 标签保存名称与关联，不保留旧不透明 tag ID 或标签创建时间；已有目标标签关联不会被覆盖。
- 源标签变更不更新已导入记录；目标主动删除的映射保留，不会复活。
- 单记录明文预算 500 MiB、图片 3200 万像素、文件清单 10000 行；不代表峰值 RSS，尚未进行大库峰值内存和磁盘不足故障注入。
- SQLite 只读事务不等于整个文件系统快照，建议退出旧应用再作最终迁移。工具不修改源库、vault、blob 或钥匙串。
