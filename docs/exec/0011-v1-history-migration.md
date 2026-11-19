# 0011 执行记录：本机 UniClipboard v1 历史迁移

- 日期：2026-11-08。
- 工作分支：`feat/v1-history-migration`。
- 独立 worktree：`/Users/LWP1/Projects/uni-v1-migration`。
- 设计：[0011](../decisions/proposed/0011-v1-history-migration.md)。
- 原始需求：三平台 arm64/amd64，迁移可恢复的文本、图片、文件、标签；新目录与已有历史合并都支持。

## 已实现

- [x] Go `unic migrate` 子命令（进程内，无独立程序），参照相邻旧 Rust 仓库 `1.1.0-rc.23` 读取，不链接旧 Engine。
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
- [x] `unic migrate` 在进程内运行；结构化阶段与数量报告、部分完成退出码、SIGINT 取消。
- [x] Go 桌面默认历史页：预览、标签筛选、分页和使用内容。
- [x] 独立迁移程序与六平台打包脚本已移除；迁移随 `unic` 构建与发布。

## 验证证据

Rust 测试生成器仅使用固定非个人数据，独立调用 serde/postcard、Argon2、XChaCha20、HKDF、BLAKE3 与 zstd。夹具覆盖文本、HTML、PNG、文件二进制、标签定义与关联、无关联标签、重复正文、软删除和损坏后修复。

自动测试覆盖：

- 正确/错误口令、系统凭据失败、原始二进制 KEK；错误 AAD、篡改正文和 blob。
- 部分导入、修复续跑、重复运行跳过、移动旧目录保持来源身份；删除旧源后仍可读目标文件。
- 已有历史合并、相同正文多记录、长中文标签、无关联标签、同时间与跨页顺序。
- 截断 reader 不发布记录或映射；极小配置容量不导致迁移淘汰；持久容量下限重开可读。
- 本地文件字节变化被摘要检查拒绝；ZIP 保留相对目录与空目录；拒绝源目标包含关系及符号链接重叠。

最终 `go test -race ./...` 通过 317 个测试（34 个包），`go vet ./...`、三个本机入口构建、六平台 CLI 构建和 `git diff --check` 均通过（均为移除独立程序之前的结果）。开发入口在 `.build/`。全仓 lint 仍有 124 个原有问题；本次新增文件与修改行无新增问题。

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
- Windows credential lookup is implemented and compile-checked. Real Linux GNOME Secret Service acceptance is recorded below. Native locked-store prompts and Windows native execution remain NOT_RUN. Platform installer/signing acceptance remains separate.
- The CLI importer and mygo/UI Kit setup wizard are implemented. Decision 0026 supplies source selection, passphrase fallback, progress, cancellation and later Settings migration. Existing macOS complete/partial evidence remains valid; the integrated follow-up below extends it.
- 丢失文件、损坏密文、超限记录显示部分完成，不生成虚假的可用占位内容。当前报告为数量级，未提供逐条修复界面。
- 不迁移收藏、设备身份与信任、设置和宿主专用非标准 representation。文件夹以 ZIP 表达。
- 标签保存名称与关联，不保留旧不透明 tag ID 或标签创建时间；已有目标标签关联不会被覆盖。
- 源标签变更不更新已导入记录；目标主动删除的映射保留，不会复活。
- Limits remain 500 MiB plaintext per record, 32 million image pixels and 10,000 file-list rows. The 10,000-record RSS and bounded ENOSPC evidence below measures record-count pressure; it does not establish multi-GiB payload performance.
- SQLite 只读事务不等于整个文件系统快照，建议退出旧应用再作最终迁移。工具不修改源库、vault、blob 或钥匙串。


## E-group implementation and process acceptance (2026-11-19)

Baseline: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452` in the clean,
independent `feat/migration-first-run` worktree. Origin was checked as
`https://github.com/UniClipboard/UniClipboard.git`. The source session worktree
was not modified. Failure inventories preceded implementation in the
[first-run inventory](migration-first-run-failures.md),
[resilience plan](../../e2e/release/MIGRATION_RESILIENCE.md) and 0026 follow-up.
No unit tests were added after implementation.

### Behavior

Each CLI profile requires explicit migration initialization. Ordinary commands
refuse with exit 6 and an actionable JSON/text error before service, sync or
clipboard work. Help, version and migration remain accessible. No-source users
explicitly run `unic migrate --initialize-empty --json`. An absent source or empty leftover directory can use this explicit path. A pending import cannot
use that path. Missing-source typos do not create an unfinishable import.

The CLI and Desktop share pending/source/completion state. A verified explicit
Desktop import can complete the gate; welcome/skip cannot. Dry-run, partial,
failure and cancellation never complete it. Sources bind first to a public
key-slot fingerprint and then to the importer's stable source identity.
No-source and import preflight reject source/target overlap before any target
write. Portable, development and `UNIC_HOME` paths use one resolver. Lock waits
and stdin/terminal passphrase input can be cancelled; private PTY acceptance
verifies restored echo after SIGINT. See the
[exact contract](../contracts/local-service-contract.md#explicit-cli-migration-initialization).

Three platform credential readers now retain fixed actionable errors. Linux
implements the standard Prompt/Completed/Dismiss handshake with bounded,
context-aware waits. Windows keeps the original keyring-rs target layout.
Only task-created GNOME Secret Service items were used for native credential
reads. NUL/non-UTF-8 values, modern/legacy lookup, missing/ambiguous items,
pre-cancelled access and unavailable bus cases passed. This is not locked-store
native-prompt acceptance.

### Evidence and limits

- macOS CLI first-run/partial/repair/idempotence/two-restart and private PTY cases:
  PASS. Receipts: [macOS first-run receipt](evidence/migration-first-run/macos-first-run.json).
- Real Linux arm64 CLI/service first-run and private PTY cases: PASS in a
  read-only, no-network Docker container. Receipt:
  [Linux first-run receipt](evidence/migration-first-run/linux-first-run.json).
- Real Linux 10,000-record production import: PASS for partial repair, SIGINT
  resume, exact mappings/IDs, two service restarts, ownership refusal before
  source import, unchanged service generation and source hashes, and idempotence.
- A separate 32 MiB tmpfs produced actual ENOSPC. Recovery after removing only
  the task filler verified all 10,000 records and retained an unrelated seed
  record. No host filesystem was filled. Receipt:
  [Linux resilience receipt](evidence/migration-first-run/linux-resilience.json).
- The private test disables configured age cleanup because frozen records date
  from 1970. Normal user cleanup policy is preserved. Small byte limits still
  exercise the import capacity floor. Peak RSS has a predeclared 512 MiB bound.
- Canonical Desktop process and macOS native later-migration results are in
  [0026](0026-first-install-setup.md). System-dialog automation paused because
  Accessibility permission was absent. Windows native credentials/execution,
  Linux locked-store native prompts and macOS actual credential prompts remain
  NOT_RUN. No personal history, credentials or configuration was accessed.

Reproduction entries:

```sh
TMPDIR=/private/tmp UNIC_E2E=1 go test ./e2e/cli -run '^(TestMigrationFirstRun|TestMigrationTerminalCancellation)$' -count=1 -v
make desktop-e2e CASE=first-install-setup
make desktop-e2e CASE=setup-later-migration
UNIC_MIGRATION_RESILIENCE_E2E=1 go test ./e2e/headless -run '^TestProductionMigrationResilience$' -count=1 -v
# In an isolated Linux container, add a dedicated bounded /limited tmpfs:
python3 e2e/release/migration_resilience.py --cli /bin/task/unic --daemon /bin/task/unicd --artifacts /results --records 10000 --limited-dir /limited
sh e2e/credentials/run.sh /bin/task/credentials
```

The ordinary race suite passed with canonical `TMPDIR=/private/tmp`. The first
run used macOS temporary-directory aliases and hit existing cache-root checks.
Static checks use the repository-pinned golangci-lint v2.15.0. The installed
legacy checker could not parse the v2 configuration and was not used as evidence.
The separate lifecycle process suite reproduced its already-recorded C03
assertion (`temporary service outlived its last user`); a three-run isolated C03
probe failed once and passed twice. The migration/ownership cases passed. This
receipt does not claim the entire lifecycle process suite passed or identify the
C03 cause. See the existing [lifecycle gap](../architecture/headless.md#gaps).

Each process runner writes checksummed redacted receipts and removes task-owned
profiles. Source snapshots, programs, raw logs, encrypted private test profiles
and native recordings are not committed. Local commits do not publish, merge,
release or deploy the change.


Final production source: `9d6c3fb56e31b0b29ec33301c073f66722ebb753`.
All three OS/two architecture CLI/service targets compiled. Actual process runs
were macOS arm64 and Linux arm64. The exact final Linux run measured 81,948,672
bytes peak child RSS and a 33,554,432-byte ENOSPC filesystem. Final macOS/Linux
production first-run receipts name that source commit and the actual executable
hashes. See the [small evidence package](evidence/migration-first-run/README.md)
and its [SHA256SUMS](evidence/migration-first-run/SHA256SUMS).
Task-only binaries, build trees, patches, caches, private profiles and worker
worktrees were removed after receipt verification. Canonical native report
paths remain local, ignored review artifacts. No binary, source snapshot, raw
log, screenshot or recording is committed.


## PR-base integration (2026-11-19)

Integrated `origin/v2` at `a0c49ae55bd0bddfa261a3a4cf3b6879c408c8f9`
with merge commit `afcce7e17fbc97a07840e4d79b63bf87efefa52f`. This includes
upstream observability acceptance and plugin installation/settings/Actions.
Three architecture verification-marker conflicts and the appended local-service
contract sections were resolved without dropping either migration or plugin
behavior. CLI dispatch, command requirements and the observability fixture
merged automatically and were inspected.

Revalidation on the integrated tree: `TMPDIR=/private/tmp go test -race -p 2
./...` passed 782 tests in 80 packages; fixed golangci-lint v2.15.0 reported zero
issues. The real CLI/service `TestMigrationFirstRun` and private-PTY
`TestMigrationTerminalCancellation` both passed. Original macOS/Linux native,
large-library, ENOSPC and credential evidence retains its frozen source and
program identities; this merge does not extend unperformed native acceptance.
The required read-only context audit found no `CONTEXT.md` on the updated base
or integrated tree. Architecture verification markers were refreshed after the
merge commit. The existing C03 acceptance limit remains unchanged.
