# CLI 端到端用例

> 本文件由 `go test ./e2e/cli -run TestCatalog -update` 根据 `testdata/*.txtar` 顶部的说明生成，不要手改。
> 怎么运行、怎么新增用例见 [README.md](README.md)。

| 用例 | 标题 |
|---|---|
| [cold-start-offline-peer](#cold-start-offline-peer) | 冷启动发送时对端离线，在有限窗口后完成且不伪造成功 |
| [cold-start-pairing-outcomes](#cold-start-pairing-outcomes) | 冷启动 CLI 配对被中断或请求被撤销时给出各自明确的结果 |
| [cold-start-pairing](#cold-start-pairing) | 冷启动 CLI 配对时保持临时服务直到对端批准，批准窗口到期时给出明确结果 |
| [cold-start-send-killed](#cold-start-send-killed) | 冷启动发送在等待对端窗口内被杀，用量被回收且不重放 |
| [cold-start-send-response-lost](#cold-start-send-response-lost) | 提交已成功但响应丢失，不重放也不回滚 |
| [cold-start-send](#cold-start-send) | 冷启动 CLI 发送文本，等待本次发送完成后才停止临时服务 |
| [content-formats](#content-formats) | HTML、PNG 和文件原样同步 |
| [crash-and-suspend-recovery](#crash-and-suspend-recovery) | 对端崩溃或挂起后自动恢复 |
| [data-dir-single-instance](#data-dir-single-instance) | 同一数据目录只允许一个 unicd，日志不泄露秘密 |
| [file-cache-boundary](#file-cache-boundary) | Cache cleanup cannot escape the managed directory |
| [file-cache-retention](#file-cache-retention) | Unified cleanup deletes managed files with history |
| [file-cache-scope](#file-cache-scope) | File cache ownership and quota |
| [file-size-policy](#file-size-policy) | Default 100 MB file boundary |
| [history-auto-cleanup](#history-auto-cleanup) | Unified history cleanup and tag exclusions |
| [history-cache-quota](#history-cache-quota) | Unified cleanup preserves the per-device cache quota |
| [history-restore-sync](#history-restore-sync) | History restore sync policy |
| [host-received-files](#host-received-files) | Host-owned custom receive files |
| [pair-and-sync-text](#pair-and-sync-text) | 两台设备配对批准后，文本双向同步 |
| [plugin-sensitive-blocks-sync](#plugin-sensitive-blocks-sync) | 敏感内容插件：打标签、禁止同步、设置过期 |
| [plugin-tag-and-action](#plugin-tag-and-action) | 邮箱标签插件与 Action 改写 |
| [restart-keeps-state](#restart-keeps-state) | 重启后身份、配对、历史与标签保持不变 |
| [send-skips-offline-target](#send-skips-offline-target) | 多目标发送时离线设备不是目标，在线目标正常完成 |
| [send-to-device](#send-to-device) | send --to 只发给指定的已配对设备 |
| [service-replaced-mid-submit](#service-replaced-mid-submit) | 提交途中服务被杀并重启，命令报告中断，不重放也不回滚 |
| [sync-settings](#sync-settings) | CLI settings and TOML policy |
| [temporary-transfer](#temporary-transfer) | 两个没有配对的 unic 之间做临时传输 |
| [transfer-code](#transfer-code) | 用 6 位码完成临时传输，并拒绝配对码 |
| [warm-send](#warm-send) | 借用已运行服务发送时，完成规则与冷启动相同，且不停止该服务 |

## cold-start-offline-peer

**冷启动发送时对端离线，在有限窗口后完成且不伪造成功**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/cold-start-offline-peer' -v`

**目的**

验证对端离线时 `unic send` 不会无限等待，本地提交保持成功，并且不报告发送成功。


**流程**

1. 启动 alpha 和 beta 并配对批准，然后停止两者。
2. 在 alpha 上执行 `unic send --to all --json`；beta 保持离线。
3. 确认命令结束后 alpha 的临时服务已退出。

**期望**

- 命令成功退出，提交结果为 committed。
- 发送结果不是 success。
- 命令结束后 alpha 没有服务进程或 socket。

**不应**

- 无限等待离线设备。
- 把没有目标的提交报告成网络发送成功。

<details><summary>脚本 testdata/cold-start-offline-peer.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta

node-stop alpha TERM
node-stop beta TERM
service-gone alpha

u alpha send --to all --json nobody home
stdout '"phase":"committed"'
! stdout '"outcome":"success"'
service-gone alpha
```

</details>

## cold-start-pairing-outcomes

**冷启动 CLI 配对被中断或请求被撤销时给出各自明确的结果**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/cold-start-pairing-outcomes' -v`

**目的**

验证 `unic device add` 在临时服务下，用户中断和请求被撤销是两种不同的结果，且都释放临时服务。


**流程**

1. beta 保持运行。alpha 在后台执行 `unic device add <beta 地址>`，等 beta 看到请求。
2. 向 alpha 的配对命令发送 INT；确认结果为中断，alpha 服务退出。
3. gamma 再次配对 beta；在 gamma 自己的服务上撤销该请求；确认结果为请求被撤销，gamma 服务退出。

**期望**

- 中断的命令以退出码 4 结束，并说明批准仍在等待。
- 被撤销的命令以退出码 4 结束，并说明请求被移除或替换。
- 两个临时服务都已退出。

**不应**

- 把中断或撤销报告成成功。
- 把两种结果报成同一条消息。

<details><summary>脚本 testdata/cold-start-pairing-outcomes.txtar</summary>

```
node-start beta
node-id beta BETA
node-define alpha
u beta device addr
cp stdout beta.addr

u-async p alpha device add @beta.addr
until beta '[0-9A-F]{4} [0-9A-F]{4}' device list
u-kill p INT
! u-wait p
stderr 'interrupted; approval is still pending'
stderr 'exit status 4'
service-gone alpha

node-define gamma
u beta device addr
cp stdout beta2.addr
u-async q gamma device add @beta2.addr
until gamma '[0-9A-F]{4} [0-9A-F]{4}' device list
u gamma device reject $BETA
! u-wait q
stderr 'removed or replaced'
stderr 'exit status 4'
service-gone gamma
```

</details>

## cold-start-pairing

**冷启动 CLI 配对时保持临时服务直到对端批准，批准窗口到期时给出明确结果**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/cold-start-pairing' -v`

**目的**

验证 `unic device add` 在临时服务下是有界会话：批准后报告 connected，到期则报告仍在等待，两者都不让服务提前退出。


**流程**

1. beta 保持运行；alpha 没有服务。alpha 在后台执行 `unic device add <beta 地址>`。
2. 双方核对指纹并批准；等待后台命令结束。
3. 第三台全新设备 gamma 用很短的窗口配对 beta，且无人批准。

**期望**

- 批准后 alpha 的配对命令成功并报告 connected。
- gamma 的命令以退出码 4 结束，并报告批准仍在等待。
- 两个临时服务在各自命令结束后都已退出。

**不应**

- 用无关设备的事件当作配对完成。
- 把仍在等待的批准报告成成功。

<details><summary>脚本 testdata/cold-start-pairing.txtar</summary>

```
node-start beta
node-define alpha
u beta device addr
cp stdout beta.addr

u-async p alpha device add @beta.addr
until beta '[0-9A-F]{4} [0-9A-F]{4}' device list
until alpha '[0-9A-F]{4} [0-9A-F]{4}' device list
pair beta alpha decide
u-wait p
stdout 'approved: connected'
service-gone alpha

node-define gamma
u beta device addr
cp stdout beta2.addr
u-async q gamma ENV:UNIC_E2E_PAIRING_WINDOW_MS=3000 device add @beta2.addr
! u-wait q
stderr 'approval still pending'
stderr 'exit status 4'
service-gone gamma
```

</details>

## cold-start-send-killed

**冷启动发送在等待对端窗口内被杀，用量被回收且不重放**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/cold-start-send-killed' -v`

**目的**

验证发送命令在连接窗口内死亡后，临时服务随最后一个用户消失，且之后没有任何自动提交。


**流程**

1. 启动 alpha 和 beta 并配对批准，然后停止两者，beta 保持离线。
2. 在后台执行 `unic send`，等它进入等待对端的窗口后用 KILL 杀掉。
3. 确认 alpha 的临时服务已退出，再重新读取 alpha 的历史。

**期望**

- alpha 没有服务进程或 socket。
- 历史为空：窗口内死亡的命令没有留下提交。

**不应**

- 服务在发送命令死亡后继续运行。
- 服务重启后自动重放该提交。

<details><summary>脚本 testdata/cold-start-send-killed.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta

node-stop alpha TERM
node-stop beta TERM
service-gone alpha

u-async send alpha send --to all --json killed send text
sleep 3s
u-kill send KILL
service-gone alpha

u alpha history list
! stdout 'killed send text'
stderr 'no entries'
service-gone alpha
```

</details>

## cold-start-send-response-lost

**提交已成功但响应丢失，不重放也不回滚**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/cold-start-send-response-lost' -v`

**目的**

验证大文件发送途中命令被杀后，已提交的历史条目仍然只有一份，临时服务随之退出，且重启后没有自动重发。


**流程**

1. 启动 alpha 和 beta 并配对批准，停止 alpha，beta 保持在线。
2. 在后台执行 `unic send -f` 发送一个大文件，等历史里出现该条目后用 KILL 杀掉命令。
3. 确认 alpha 的临时服务已退出，再读取 alpha 的历史。

**期望**

- alpha 没有服务进程或 socket。
- alpha 历史里该文件恰好一条。
- 重启后按原 request-id 查询得到 no such submission，不承诺持久查询。

**不应**

- 提交被回滚。
- 重启服务后出现第二条同一文件的历史。
- 把已丢失的请求记录当作可持久查询的结果。

<details><summary>脚本 testdata/cold-start-send-response-lost.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta

node-stop alpha TERM
service-gone alpha

genfile rand big.bin 94371840
u-async send alpha send --to all --json --request-id lost-1 -f $WORK/big.bin
until alpha 'files' history list
u-kill send KILL
service-gone alpha

u alpha history list
stdout -count=1 'files'
! u alpha dev submission lost-1
stderr 'no such submission'
! stdout '@2'
service-gone alpha
```

</details>

## cold-start-send

**冷启动 CLI 发送文本，等待本次发送完成后才停止临时服务**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/cold-start-send' -v`

**目的**

验证 unicd 未运行时 `unic send` 自动启动临时服务，等待已配对设备连接，观察本次发送的终态，然后才退出。


**流程**

1. 启动 alpha 和 beta 并配对批准，然后完全停止 alpha。
2. 在 alpha 上执行 `unic send --to all --json`，此时没有任何 alpha 服务在运行。
3. 等 beta 的最新历史出现该文本。
4. 确认 alpha 的临时服务已经退出。

**期望**

- JSON 输出里提交结果与发送结果是分开的两个字段，发送结果为 success。
- beta 的最新历史内容等于发送的文本。
- 命令结束后 alpha 没有任何服务进程或 socket。

**不应**

- 在服务就绪或排队时就停止临时服务，导致 beta 收不到内容。

<details><summary>脚本 testdata/cold-start-send.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta

node-stop alpha TERM
service-gone alpha

u alpha send --to all --json cold start text
stdout '"submission":'
stdout '"outcome":"success"'
until beta 'cold start text' h show
service-gone alpha
```

</details>

## content-formats

**HTML、PNG 和文件原样同步**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/content-formats' -v`

**目的**

验证除纯文本外的内容类型跨设备后字节不变，且富文本的多个表示都保留。


**流程**

1. 启动并配对 alpha 和 beta。
2. alpha 复制一份同时带纯文本和 HTML 两个表示的内容，等 beta 收到。
3. alpha 复制一张 PNG 图片，等 beta 收到。
4. alpha 复制一个 300000 字节的随机文件，等 beta 收到。
5. 查看 beta 历史里图片、文件条目的类型和来源。

**期望**

- beta 的纯文本、HTML、PNG 字节与 alpha 发出的完全一致。
- beta 剪贴板里的文件内容与原文件逐字节相同。
- beta 的历史里图片是 image 类型、文件是 files 类型，来源都是 alpha。
- 带两个表示的内容到达 beta 后，纯文本和 HTML 两个表示都在 beta 的剪贴板里。

**不应**

- 富文本被压成纯文本，丢掉 HTML 表示。
- 图片或文件在传输中被截断、转码。

<details><summary>脚本 testdata/content-formats.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta

board-set alpha text rich.txt html rich.html
board-wait beta text rich.txt
board-wait beta html rich.html
until beta 'alpha\s+\d+B' h list

genfile png pic.png
board-set alpha png pic.png
board-wait beta png pic.png
until beta 'image\s+alpha' h list

genfile rand big.bin 300000
board-set alpha files big.bin
board-wait beta files big.bin
until beta 'files\s+alpha' h list
```

</details>

## crash-and-suspend-recovery

**对端崩溃或挂起后自动恢复**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/crash-and-suspend-recovery' -v`

**目的**

验证一端被强杀（SIGKILL）、或被挂起再恢复（SIGSTOP/SIGCONT）后，不需要人工干预就能重新同步。


**流程**

1. 启动并配对 alpha 和 beta，确认能同步。
2. 强杀 beta；alpha 在 beta 离线时复制一段文本。
3. 重新启动 beta，等双方重新 online。
4. alpha 每 5 秒复制一次带编号的文本，直到 beta 收到（对端刚恢复时旧连接上发出的内容会丢，所以允许重试；日志里记录第几次送达）。
5. 挂起 beta 几秒后恢复，同样重试复制直到 beta 收到。

**期望**

- beta 被强杀后重启，身份不变，仍是已配对设备，无需再批准。
- 重连后 90 秒内新内容能到达 beta。
- 挂起恢复后 90 秒内新内容能到达 beta。

**不应**

- beta 离线期间的内容在重连后被补发（本版本不排队）。
- 重启后出现需要重新批准的请求。

<details><summary>脚本 testdata/crash-and-suspend-recovery.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta
board-set alpha text a1.txt
board-wait beta text a1.txt

node-id beta BETA
node-stop beta KILL
board-set alpha text offline.txt
sleep 2s
node-start beta
until alpha 'online' device list
until beta 'online' device list
u beta status
stdout $BETA
! board-wait beta text offline.txt
u beta device list
! stdout 'Waiting for your approval'

retry-copy alpha beta after-crash

node-stop beta STOP
sleep 5s
node-stop beta CONT
retry-copy alpha beta after-suspend
```

</details>

## data-dir-single-instance

**同一数据目录只允许一个 unicd，日志不泄露秘密**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/data-dir-single-instance' -v`

**目的**

验证第二个实例不会破坏已运行实例的数据，并且配对地址、票据与剪贴板正文不会写进日志。


**流程**

1. 启动并配对 alpha 和 beta，同步一段带哨兵字符串的文本。
2. 对 alpha 的同一数据目录再启动一个 unicd。
3. 检查两个节点的日志。

**期望**

- 第二个实例启动失败并报 in_use。
- 原实例继续正常工作。

**不应**

- 日志里出现剪贴板正文哨兵。
- 日志里出现 unic-pair: 票据前缀。
- 第二个实例影响已运行实例的连接。

<details><summary>脚本 testdata/data-dir-single-instance.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta
board-set alpha text sentinel.txt
board-wait beta text sentinel.txt

! node-start alpha
u alpha status
until alpha 'online' device list
board-set alpha text again.txt
board-wait beta text again.txt

! logs alpha SENTINEL-4f9c1e
! logs beta SENTINEL-4f9c1e
! logs alpha unic-pair:
! logs beta unic-pair:
logs alpha in_use
```

</details>

## file-cache-boundary

**Cache cleanup cannot escape the managed directory**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/file-cache-boundary' -v`

**目的**

Exercise cleanup after a cache directory becomes a symlink.


**流程**

1. Receive a default-cache file and stop its daemon.
2. Replace that payload directory with a symlink to unrelated files.
3. Age the cache entry and restart cleanup.

**期望**

- Cleanup never deletes the symlink target or a familiar-prefix directory.
- The real cached payload remains in its moved directory.

**不应**

- A stored cache ID grants deletion authority over an external symlink target.

<details><summary>脚本 testdata/file-cache-boundary.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta
genfile rand payload.bin 100
board-set alpha files payload.bin
board-wait beta files payload.bin
file-path beta CACHED
u beta dev copy release
u beta config set history.retention_days 1
node-stop beta
history-age beta 172800
cache-link beta $CACHED
node-start beta
sleep 1s
cmp outside/keep.txt keep.txt
cmp moved-payload/payload.bin payload.bin
exists beta/inbox/clip-user/keep.txt
```

</details>

## file-cache-retention

**Unified cleanup deletes managed files with history**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/file-cache-retention' -v`

**目的**

Verify history count cleanup releases managed payloads and honors tags.


**流程**

1. Receive files with cleanup disabled and protect a tagged file.
2. Release clipboard holders and enable a count limit.
3. Remove the excluded tag and verify physical deletion.

**期望**

- Both file content and its history survive while excluded.
- Cleanup deletes eligible file history and its unheld payload.

**不应**

- File-only cleanup leaves unavailable history records.

<details><summary>脚本 testdata/file-cache-retention.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta
u beta config set history.auto_cleanup false
genfile rand protected.bin 100
board-set alpha files protected.bin
board-wait beta files protected.bin
file-path beta PROTECTED
u beta h tag @1 --add keep
u beta dev copy release-protected
retry-copy alpha beta ready-for-second
genfile rand old.bin 110
board-set alpha files old.bin
board-wait beta files old.bin
file-path beta OLD
u beta dev copy release-old
until beta '"size":110,[^}]*"held":false' dev cache --json
u beta config set history.excluded_tags '["keep"]'
u beta config set history.max_items 1
u beta config set history.auto_cleanup true
! file-wait $OLD
cmp $PROTECTED protected.bin
u beta h list --json
! stdout '"available":false'
stdout keep
node-stop beta
node-start beta
cmp $PROTECTED protected.bin
u beta config set history.excluded_tags '[]'
! file-wait $PROTECTED
u beta h list --json
! stdout '"kind":"files"'
```

</details>

## file-cache-scope

**File cache ownership and quota**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/file-cache-scope' -v`

**目的**

Bound default cache without deleting custom received files.


**流程**

1. Receive files with a small quota and release the clipboard pin.
2. Change to a custom receive directory and delete its history.
3. Restart with cleanup enabled and compare the custom files.

**期望**

- Default quota rejects files when protected payloads fill the cache.
- Releasing pins permits unified deletion of files and bound history.
- Custom receives ignore the default quota and survive history deletion and restart.

**不应**

- Cleanup follows a symlink or deletes files outside the default directory.
- A custom receive becomes an owned history copy.

<details><summary>脚本 testdata/file-cache-scope.txtar</summary>

```
exec printf reset
cp stdout reset.txt
# Simulate interrupted Engine-owned temporary writes before startup.
mkdir alpha/inbox/unic-content-dead
mkdir alpha/inbox/unic-recv/clip-dead
mkdir alpha/inbox/holiday-photos
cp reset.txt alpha/inbox/holiday-photos/keep.txt
mkdir outside
cp reset.txt outside/keep.txt
symlink alpha/inbox/unic-content-link -> $WORK/outside
symlink alpha/inbox/unic-content-dead/link -> $WORK/outside
node-start alpha
! exists alpha/inbox/unic-content-dead
! exists alpha/inbox/unic-recv/clip-dead
cmp alpha/inbox/holiday-photos/keep.txt reset.txt
cmp outside/keep.txt reset.txt
node-stop alpha KILL
mkdir alpha/inbox/unic-content-interrupted
mkdir alpha/inbox/unic-recv/clip-interrupted
node-start alpha
! exists alpha/inbox/unic-content-interrupted
! exists alpha/inbox/unic-recv/clip-interrupted
cmp alpha/inbox/holiday-photos/keep.txt reset.txt
cmp outside/keep.txt reset.txt
node-start beta
pair alpha beta
u beta config set file_cache.max_bytes 150
genfile rand one.bin 100
board-set alpha files one.bin
board-wait beta files one.bin
file-path beta ONE
genfile rand two.bin 110
board-set alpha files two.bin
! board-wait beta files two.bin
u beta dev copy clear
u alpha dev copy reset
board-wait beta text reset.txt
board-set alpha files two.bin
board-wait beta files two.bin
! exists $ONE
file-path beta TWO
node-stop beta KILL
node-start beta
cmp $TWO two.bin
u beta h list --json
! stdout '"available":false'
mkdir custom
u beta config set inbox '"'$WORK'/custom"'
node-stop beta
node-start beta
until alpha 'online' device list
retry-copy alpha beta custom-ready
genfile rand custom.bin 200
board-set alpha files custom.bin
board-wait beta files custom.bin
file-path beta CUSTOM
cmp $CUSTOM custom.bin
u beta h delete @1 -y
cmp $CUSTOM custom.bin
u beta dev copy clear
node-stop beta
node-start beta
cmp $CUSTOM custom.bin
u beta config set inbox '""'
node-stop beta
node-start beta
cmp $CUSTOM custom.bin
u beta config --json
stdout '"scope":"default_directory_only"'
```

</details>

## file-size-policy

**Default 100 MB file boundary**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/file-size-policy' -v`

**目的**

Exercise the real default file-size limit on both devices.


**流程**

1. Receive a file of exactly 100 MB.
2. Try a file one byte larger and retain its local history.
3. Lower the receive limit and reject a smaller sender-accepted file.

**期望**

- The default is 104857600 bytes and an exact-size file transfers in full.
- An oversized copy remains in local history without reaching the peer.

**不应**

- A sender can override the receiver limit.

<details><summary>脚本 testdata/file-size-policy.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta
u alpha config --json
stdout '"max_file_bytes":104857600'
genfile rand exact.bin 104857600
board-set alpha files exact.bin
board-wait beta files exact.bin
genfile rand over.bin 104857601
board-set alpha files over.bin
until alpha '"total_bytes":104857601' h list --json
! board-wait beta files over.bin
u beta config set limits.max_file_bytes '"1MB"'
genfile rand rejected.bin 1048577
board-set alpha files rejected.bin
! board-wait beta files rejected.bin
```

</details>

## history-auto-cleanup

**Unified history cleanup and tag exclusions**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/history-auto-cleanup' -v`

**目的**

Verify live count cleanup, tag matching, restart, and removed file settings.


**流程**

1. Disable cleanup and create tagged and untagged local history.
2. Enable a count limit and protect one tag.
3. Restart and remove tag protection.

**期望**

- Excluded history survives; oldest eligible records disappear.
- Zero count means unlimited; removed file keys are rejected.

**不应**

- Cleanup resumes while disabled or an excluded record is counted.

<details><summary>脚本 testdata/history-auto-cleanup.txtar</summary>

```
node-start alpha
u alpha config set history.auto_cleanup false
u alpha config set history.max_items 1
u alpha dev copy protected-one
u alpha h tag @1 --add keep
exec curl -sf --unix-socket $WORK/.alpha.sock http://unic/v1/history/tags
stdout keep
u alpha dev copy ordinary-two
u alpha dev copy ordinary-three
u alpha h show @3
stdout protected-one
u alpha h show @2
stdout ordinary-two
u alpha h show @1
stdout ordinary-three
u alpha config set history.excluded_tags '["keep", "other"]'
u alpha config set history.auto_cleanup true
u alpha h show @2
stdout protected-one
u alpha h show @1
stdout ordinary-three
! u alpha h show @3
node-stop alpha
node-start alpha
u alpha h show @2
stdout protected-one
u alpha h show @1
stdout ordinary-three
u alpha config set history.excluded_tags '[]'
u alpha h show @1
stdout ordinary-three
! u alpha h show @2
u alpha config set history.max_items 0
u alpha dev copy unlimited-four
u alpha h show @2
stdout ordinary-three
u alpha h show @1
stdout unlimited-four
! u alpha config set file_cache.retention_hours 1
! u alpha config set file_cache.auto_cleanup true
u alpha config --json
! stdout retention_hours
! stdout default_ttl_days
u alpha config set history.auto_cleanup false
u alpha config set history.retention_days 1
node-stop alpha
history-age alpha 172800
node-start alpha
u alpha h show @1
stdout unlimited-four
u alpha config set history.auto_cleanup true
! u alpha h show @1
```

</details>

## history-cache-quota

**Unified cleanup preserves the per-device cache quota**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/history-cache-quota' -v`

**目的**

Verify disabled cleanup and excluded tags block capacity reclamation.


**流程**

1. Fill a small cache, release its clipboard holder, and disable cleanup.
2. Enable cleanup while excluding the retained record's tag.
3. Remove the exclusion and resend the rejected batch.

**期望**

- Disabled cleanup and tag exclusions each preserve the first file and record.
- Removing the exclusion permits coupled record/file deletion and new admission.

**不应**

- Quota reclamation bypasses the unified switch or tag exclusions.

<details><summary>脚本 testdata/history-cache-quota.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta
u beta config set file_cache.max_bytes 150
u beta config set history.auto_cleanup false
genfile rand first.bin 100
board-set alpha files first.bin
board-wait beta files first.bin
file-path beta FIRST
u beta h tag @1 --add keep
u beta dev copy -f $FIRST
u beta dev cache --json
stdout '"stored_bytes":100'
u beta dev copy release
until beta '"size":100,[^}]*"held":false' dev cache --json
retry-copy alpha beta reset-off
genfile rand next.bin 110
board-set alpha files next.bin
! board-wait beta files next.bin
cmp $FIRST first.bin
u beta h list --tag keep --json
stdout keep
u beta config set history.excluded_tags '["keep"]'
u beta config set history.auto_cleanup true
retry-copy alpha beta reset-protected
board-set alpha files next.bin
! board-wait beta files next.bin
cmp $FIRST first.bin
u beta h list --tag keep --json
stdout keep
u beta config set history.excluded_tags '[]'
retry-copy alpha beta reset-eligible
board-set alpha files next.bin
board-wait beta files next.bin
! file-wait $FIRST
u beta h list --tag keep --json
! stdout keep
```

</details>

## history-restore-sync

**History restore sync policy**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/history-restore-sync' -v`

**目的**

Restore stored content without duplicate local history.


**流程**

1. Create stored text, change the clipboard, and restore it.
2. Enable restore sync and repeat with automatic sending off.

**期望**

- Default restore affects only the local clipboard.
- Enabled restore updates the peer while automatic sending is off.

**不应**

- Restoring history creates a duplicate local record.
- Restore bypasses the global pause.

<details><summary>脚本 testdata/history-restore-sync.txtar</summary>

```
exec printf first
cp stdout first.txt
exec printf second
cp stdout second.txt
node-start alpha
node-start beta
pair alpha beta
u alpha dev copy first
board-wait beta text first.txt
u alpha dev copy second
board-wait beta text second.txt
u alpha h use @2
board-wait alpha text first.txt
! board-wait beta text first.txt
u alpha h list -n 10 --json
! stdout '"id".*"id".*"id"'
u alpha config set sync.on_restore true
u alpha config set sync.automatic false
u alpha h use @2 --request-id repeated --json
stdout '"local_applied":true'
board-wait beta text first.txt
u beta config set sync.automatic false
board-set beta text changed.txt
u alpha h use @2 --request-id repeated --json
! board-wait beta text first.txt
u alpha h list --json
! stdout '"id".*"id".*"id"'
u alpha config set sync.enabled false
u alpha h use @1
board-wait alpha text second.txt
! board-wait beta text second.txt
```

</details>

## host-received-files

**Host-owned custom receive files**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/host-received-files' -v`

**目的**

Verify stream storage and opaque host references through real daemons.


**流程**

1. Start a custom receiver with a cache quota smaller than each incoming file.
2. Remove its destination and verify failure does not fall back to default storage.
3. Restore the directory, receive a file, and read and apply it after restart.
4. Expire its history and compare the user-owned file bytes.

**期望**

- Failed host storage publishes no file history or clipboard content.
- Custom files use opaque persistent references without owned history blobs.
- File bytes survive restart, history expiry, and clipboard lease release.

**不应**

- Engine treats the host reference as a local filesystem path.
- Default cache quota rejects a host-managed file or cleanup deletes it.

<details><summary>脚本 testdata/host-received-files.txtar</summary>

```
mkdir custom
node-start alpha
node-start beta
pair alpha beta
u beta config set file_cache.max_bytes 1
u beta config set inbox '"'$WORK'/custom"'
node-stop beta
node-start beta
retry-copy alpha beta store-ready
mv custom unavailable
retry-copy alpha beta unavailable-ready
genfile rand rejected.bin 100
board-set alpha files rejected.bin
! board-wait beta files rejected.bin
! until beta '"kind":"files"' h list --json
u beta dev cache --json
stdout '"stored_bytes":0'
stdout '"files":\[\]'
mv unavailable custom
genfile rand accepted.bin 110
board-set alpha files accepted.bin
board-wait beta files accepted.bin
file-path beta SAVED
external-ref beta
u beta h show @1 --part 0
cp stdout read-before.bin
cmp read-before.bin accepted.bin
node-stop beta
node-start beta
u beta h show @1 --part 0
cp stdout read-after.bin
cmp read-after.bin accepted.bin
u beta h use @1 --json
stdout '"local_applied":true'
board-wait beta files accepted.bin
u beta h show @1 --json
! stdout 'desktop-received:'
u beta h expire @1 1s
sleep 2s
! until beta '"kind":"files"' h list --json
cmp $SAVED accepted.bin
u beta dev copy release
cmp $SAVED accepted.bin
u beta dev cache --json
stdout '"stored_bytes":0'
stdout '"custom_destination":true'
```

</details>

## pair-and-sync-text

**两台设备配对批准后，文本双向同步**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/pair-and-sync-text' -v`

**目的**

验证最基本的交付路径：全新的两个 unicd 经完整地址配对、双方批准后互通，且批准前不通。


**流程**

1. 启动 alpha 和 beta 两个全新节点，各自使用独立的状态目录和目录剪贴板。
2. beta 用 alpha 的完整地址发起配对；两边都出现待批准请求。
3. 批准前，在 alpha 复制一段文本，等待一会儿，beta 的剪贴板不应出现它。
4. 两边各自批准，互相显示 online。
5. 在 alpha 复制文本，等 beta 剪贴板收到；再在 beta 复制文本，等 alpha 收到。
6. 查看接收方历史里这两条的来源。

**期望**

- 两端的待批准请求里指纹完全一致（pair 命令逐字核对）。
- 批准后 peers 显示对方 online。
- beta 的剪贴板内容等于 alpha 复制的文本，反向同理。
- 接收方历史里该条目的来源是发送方的设备名（alpha 或 beta）。

**不应**

- 批准之前 beta 收到 alpha 的任何内容。

<details><summary>脚本 testdata/pair-and-sync-text.txtar</summary>

```
node-start alpha
node-start beta

pair alpha beta request

# 请求已发出但没有批准：内容不能流动。
board-set alpha text before.txt
sleep 3s
! board-wait beta text before.txt
! until beta 'before' h list

pair alpha beta approve

board-set alpha text from-alpha.txt
board-wait beta text from-alpha.txt
until beta 'alpha\s+\d+B' h list

board-set beta text from-beta.txt
board-wait alpha text from-beta.txt
until alpha 'beta\s+\d+B' h list
```

</details>

## plugin-sensitive-blocks-sync

**敏感内容插件：打标签、禁止同步、设置过期**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/plugin-sensitive-blocks-sync' -v`

**目的**

验证本地 Lua 插件的目录发现、启用、重启生效，以及它对内容的三种干预（标签、禁发、过期）真的生效。


**流程**

1. 在 alpha 的插件目录放入 example.lua-sensitive（用户 git 拉取后复制的方式），未启用前 plugin list 显示 disabled。
2. 用 unic plugin enable 启用，重启 alpha 让它生效，再与 beta 配对。
3. alpha 复制一段普通文本，beta 应收到。
4. alpha 复制含 "password:" 的文本。
5. 查看 alpha 历史条目的标签和过期时间，并等待足够久确认 beta 没有收到。
6. 把该条目手动设为 3 秒后过期，等它被清理。

**期望**

- 启用前 plugin list 为 disabled，启用后为 enabled。
- 普通文本照常同步到 beta。
- 含密码的条目带 sensitive 标签和过期时间，仅存在于 alpha 本机。
- 过期后条目从 alpha 历史中消失。

**不应**

- 含密码的文本出现在 beta 的剪贴板或历史里。
- 含密码的文本出现在任何一个节点的日志里。

<details><summary>脚本 testdata/plugin-sensitive-blocks-sync.txtar</summary>

```
node-start alpha
node-start beta
install-plugin alpha sensitive
u alpha plugin list
stdout 'example.lua-sensitive\s+1.0.0\s+disabled'
u alpha plugin enable example.lua-sensitive
u alpha plugin list
stdout 'enabled'
node-stop alpha
node-start alpha
pair alpha beta

board-set alpha text plain.txt
board-wait beta text plain.txt

board-set alpha text secret.txt
until alpha 'sensitive' h list
until alpha '"expires_at_ms":[1-9]' h show @1 --json
sleep 4s
! board-wait beta text secret.txt
! until beta '\s21B' h list

u alpha config set sync.on_restore true
u alpha h use @1 --json
stdout '"sync_disposition":"not_requested"'
! board-wait beta text secret.txt
u alpha h expire @1 3s
sleep 6s
u alpha h list --tag sensitive
! stdout 'sensitive'
! logs alpha hunter2-e2e
! logs beta hunter2-e2e
```

</details>

## plugin-tag-and-action

**邮箱标签插件与 Action 改写**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/plugin-tag-and-action' -v`

**目的**

验证接收侧的插件标注，以及"用户主动触发的内容变换"（Action）产生新条目并同步，原条目不被改动。


**流程**

1. 在 beta 安装并启用 example.lua-emailtag，在 alpha 安装并启用 example.lua-textcase，重启后配对。
2. alpha 复制含邮箱地址的文本，beta 收到。
3. alpha 复制 "hello e2e world"，对该条目运行 Action example.lua-textcase/title。
4. 检查 alpha 的新条目来源与剪贴板，并等 beta 收到改写后的文本。

**期望**

- beta 收到含邮箱的文本时带 email 标签。
- Action 产生一个来源为 action:example.lua-textcase 的新条目，剪贴板变成 "Hello E2e World"。
- beta 随后收到 "Hello E2e World"。

**不应**

- 自动阶段改写任何正文（插件只能标注，改写必须由用户触发）。
- 不含邮箱的文本被打上 email 标签。

<details><summary>脚本 testdata/plugin-tag-and-action.txtar</summary>

```
node-start alpha
node-start beta
install-plugin beta emailtag
install-plugin alpha textcase
u beta plugin enable example.lua-emailtag
u alpha plugin enable example.lua-textcase
node-stop alpha
node-stop beta
node-start alpha
node-start beta
pair alpha beta

board-set alpha text mail.txt
board-wait beta text mail.txt
until beta 'alpha\s+\d+B\s+email' h list

board-set alpha text hello.txt
board-wait beta text hello.txt
until beta 'alpha\s+\d+B' h list -n 1
u beta h list -n 1
! stdout 'email'
u alpha h action @1 example.lua-textcase/title --apply
stdout '"item"'
board-wait alpha text title.txt
board-wait beta text title.txt
u alpha h list -n 5
stdout 'action:example.lua-textcase'
```

</details>

## restart-keeps-state

**重启后身份、配对、历史与标签保持不变**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/restart-keeps-state' -v`

**目的**

验证 unicd 正常退出（SIGTERM）再启动，不会重新生成身份、不丢配对和历史，并自动重连。


**流程**

1. 启动并配对 alpha 和 beta，beta 同步收到一条文本，并给它加标签 keep。
2. 记录两个节点的设备 ID。
3. 两个节点都用 SIGTERM 退出再启动。
4. 对比设备 ID，检查 peers、历史和标签，再同步一条新文本。

**期望**

- 重启前后设备 ID 完全相同。
- 重启后无需重新配对，peers 自动恢复 online。
- 历史里的旧条目和 keep 标签仍在。
- 重启后新复制的文本仍能同步。

**不应**

- 重启后出现新的待批准请求。
- 退出时进程被强杀或超时。

<details><summary>脚本 testdata/restart-keeps-state.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta
board-set alpha text one.txt
board-wait beta text one.txt
u beta h tag @1 --add keep
node-id alpha ALPHA
node-id beta BETA

node-stop alpha TERM
node-stop beta TERM
node-start alpha
node-start beta

u alpha status
stdout $ALPHA
u beta status
stdout $BETA
until alpha $BETA'\s+\S+\s+online' device list
until beta $ALPHA'\s+\S+\s+online' device list
u beta device list
! stdout 'Waiting for your approval'
u beta h list --tag keep
stdout 'alpha'

board-set alpha text two.txt
board-wait beta text two.txt
```

</details>

## send-skips-offline-target

**多目标发送时离线设备不是目标，在线目标正常完成**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/send-skips-offline-target' -v`

**目的**

验证 C23 的多目标一半：离线设备不拖住命令，也不进入目标列表，无关工作被排除。


**流程**

1. 启动 alpha、beta、gamma，alpha 分别与 beta、gamma 配对批准。
2. 停止 gamma，等待超过连接窗口。
3. 在 alpha 上执行 `unic send --to all --json`。

**期望**

- 本地提交为 committed，发送结果为 success，目标列表只有一个设备。
- beta 收到内容，alpha 的服务仍在运行。

**不应**

- 因 gamma 离线而报告失败或延迟 beta。
- 把离线设备列为发送目标。

<details><summary>脚本 testdata/send-skips-offline-target.txtar</summary>

```
node-start alpha
node-start beta
node-start gamma
pair alpha beta
pair alpha gamma

node-stop gamma TERM
sleep 11s
u alpha send --to all --json offline skipped text
stdout '"phase":"committed"'
stdout '"outcome":"success"'
stdout -count=1 '"target_id":'
until beta 'offline skipped text' h show
! service-gone alpha
```

</details>

## send-to-device

**send --to 只发给指定的已配对设备**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/send-to-device' -v`

**目的**

验证 `unic send --to` 的目标选择，以及 `--to all` 与按名称、按 ID 选择。


**流程**

1. 启动 alpha、beta、gamma。alpha 分别与 beta 和 gamma 配对批准，并确认两条连接都已稳定（配对后连接会重建）。
2. 在 alpha 上执行 `unic send --to beta`。
3. 在 alpha 上执行 `unic send --to all`。
4. 在 alpha 上执行 `unic send --to` 一个不存在的设备。

**期望**

- 第 2 步只有 beta 收到内容，gamma 没有收到。
- 第 3 步 beta 和 gamma 都收到内容。
- 第 4 步失败，并提示没有这个已配对设备。

**不应**

- 指定设备时把内容发给其他设备。
- 目标不存在时仍然提交或发送。

<details><summary>脚本 testdata/send-to-device.txtar</summary>

```
node-start alpha
node-start beta
node-start gamma
pair alpha beta
pair alpha gamma
retry-copy alpha beta warm-up-beta
retry-copy alpha gamma warm-up-gamma

u alpha send --to beta --json only for beta
stdout '"outcome":"success"'
until beta 'only for beta' h show
! until gamma 'only for beta' h show

u alpha send --to all --json for everyone
stdout '"outcome":"success"'
until beta 'for everyone' h show
until gamma 'for everyone' h show

! u alpha send --to nobody not sent
stderr 'no paired device'
```

</details>

## service-replaced-mid-submit

**提交途中服务被杀并重启，命令报告中断，不重放也不回滚**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/service-replaced-mid-submit' -v`

**目的**

验证 C24 的服务被替换一半：已提交的条目保留，重启后没有第二次提交。


**流程**

1. 启动 alpha 和 beta 并配对批准，两者保持运行。
2. 在后台发送大文件，等历史里出现该条目后 KILL alpha 的服务，再重启 alpha。
3. 等发送命令结束，读取 alpha 的历史。

**期望**

- 发送命令非零退出。
- alpha 历史里该文件恰好一条。

**不应**

- 重启后自动重放出第二条同一文件的历史。
- 回滚已提交的条目。

<details><summary>脚本 testdata/service-replaced-mid-submit.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta

genfile rand big.bin 94371840
u-async send alpha send --to all --json --request-id replaced-1 -f $WORK/big.bin
until alpha 'files' history list
node-stop alpha KILL
node-start alpha
! u-wait send

u alpha history list
stdout -count=1 'files'
```

</details>

## sync-settings

**CLI settings and TOML policy**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/sync-settings' -v`

**目的**

Verify persisted settings govern real paired daemons.


**流程**

1. Save settings through CLI and edit TOML on disk.
2. Exercise automatic, manual, receive, pause, and file-limit paths.

**期望**

- Local history persists while automatic sending is off.
- Manual and received copies work independently of automatic sending.
- Invalid configuration preserves the last valid value.

**不应**

- Enabling automatic sync replays an old copy.
- A file above the configured transfer limit reaches the other clipboard.

<details><summary>脚本 testdata/sync-settings.txtar</summary>

```
exec printf manual
cp stdout manual.txt
exec printf incoming
cp stdout incoming.txt
node-start alpha
node-start beta
pair alpha beta
config-conflict alpha
u alpha config set sync.automatic false
u alpha config --json
stdout '"automatic":false'
board-set alpha text local.txt
until alpha 'local_system' h list --json
! board-wait beta text local.txt
u alpha dev copy manual
board-wait beta text manual.txt
u beta dev copy incoming
board-wait alpha text incoming.txt
u alpha config set sync.automatic true
! board-wait beta text local.txt
u alpha config set sync.enabled false
board-set alpha text paused.txt
until alpha '"total_bytes":7' h list --json
! board-wait beta text paused.txt
u alpha config set sync.enabled true
! board-wait beta text paused.txt
u alpha config set limits.max_file_bytes '"100B"'
genfile rand too-big.bin 101
board-set alpha files too-big.bin
until alpha '"total_bytes":101' h list --json
! board-wait beta files too-big.bin
genfile rand exact.bin 100
board-set alpha files exact.bin
board-wait beta files exact.bin
u alpha config set sync.files false
genfile rand disabled.bin 80
board-set alpha files disabled.bin
! board-wait beta files disabled.bin
cp alpha/config.toml saved.toml
! u alpha config set limits.max_file_bytes 0
cmp alpha/config.toml saved.toml
cp invalid.toml alpha/config.toml
until alpha 'unknown key' config
u alpha config apply $WORK/saved.toml
until alpha '"files":false' config --json
node-stop alpha
node-start alpha
u alpha config --json
stdout '"files":false'
stdout '"max_file_bytes":100'
```

</details>

## temporary-transfer

**两个没有配对的 unic 之间做临时传输**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/temporary-transfer' -v`

**目的**

验证 `unic send`（不带 --to）生成链接并等待一个接收方，`unic recv <链接>` 取走内容，内容进入接收方的历史。


**流程**

1. 启动 alpha 和 beta，不配对。
2. alpha 在后台执行 `unic send` 发送一段文字，并打印链接。
3. beta 执行 `unic recv <链接>`。
4. alpha 再发送一个文件，beta 用同样方式接收。
5. 用无效的码、无效的链接和带配对票据前缀的文本接收。

**期望**

- 第 3 步 beta 打印收到的文字，alpha 报告 delivered 并以 0 退出。
- beta 的历史里有这段文字；alpha 的历史里没有来自 beta 的内容。
- 第 4 步 beta 的历史里出现该文件。
- 第 5 步每一种都失败并给出原因，不挂起。
- 两个设备之间没有建立任何配对关系。

**不应**

- 临时传输让 alpha 和 beta 成为已配对设备。
- 在日志之外的位置以明文以外的方式暴露链接。

<details><summary>脚本 testdata/temporary-transfer.txtar</summary>

```
node-start alpha
node-start beta

u-async s alpha send temp hello from alpha
u-grab s 'Link: (\S+)' link.txt
u-async r beta recv @link.txt
u-wait r
stdout 'temp hello from alpha'
u-wait s
stderr 'delivered'
until beta 'temp hello from alpha' h show
u beta h list
stdout 'external:unic.transfer'

u beta device list
! stdout alpha
u alpha device list
! stdout beta

u-async s2 alpha send -f $WORK/note.txt
u-grab s2 'Link: (\S+)' link2.txt
u-async r2 beta recv @link2.txt
u-wait r2
stderr 'received file note.txt'
u-wait s2
u beta h list
stdout 'files'

! u beta recv 12345
stderr 'expected a 6-digit code or a transfer link'
! u beta recv not-a-link
stderr 'expected a 6-digit code or a transfer link'
! u beta recv unic-pair:abc
stderr 'pairing ticket'
! u beta recv 482913
stderr 'code service is unavailable'
```

</details>

## transfer-code

**用 6 位码完成临时传输，并拒绝配对码**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/transfer-code' -v`

**目的**

验证 `unic send` 在码服务可用时同时显示 6 位码，`unic recv <码>` 通过码取走内容；

配对码不能当作传输码使用，传输码也不能当作配对码使用。


**流程**

1. 启动本地码服务，再启动 alpha 和 beta（不配对）。
2. alpha 在后台执行 `unic send`，打印 6 位码。beta 执行 `unic recv <码>`。
3. 同一个码再接收一次。
4. alpha 执行 `unic device add` 生成配对码，beta 用 `unic recv <配对码>` 接收。
5. alpha 再发送一次，beta 用 `unic device add <传输码>` 加入。

**期望**

- 第 2 步 beta 打印收到的文字，alpha 报告 delivered。
- 第 3 步失败，提示码已过期或已用过。
- 第 5 步失败，且 alpha 与 beta 没有成为已配对设备。

**不应**

- 一个码被两次取走。
- 配对码触发临时传输，或传输码触发配对。

<details><summary>脚本 testdata/transfer-code.txtar</summary>

```
rendezvous-start
node-start alpha
node-start beta

u-async s alpha send via six digits
u-grab s 'Code: (\d{3} \d{3})' code.txt
u-async r beta recv @code.txt
u-wait r
stdout 'via six digits'
u-wait s
stderr 'delivered'

u-async r1 beta recv @code.txt
! u-wait r1
stderr 'expired or was already used'

u-async c alpha device add
u-grab c '(\d{3} \d{3})' pair.txt
u-async r2 beta recv @pair.txt
! u-wait r2
stderr 'this code pairs a device'
u-wait c

u-async s3 alpha send second one
u-grab s3 'Code: (\d{3} \d{3})' code3.txt
u-async j beta device add @code3.txt
! u-wait j
stderr 'not a unic pairing code|not valid|invalid'
u beta device list
! stdout alpha
u-kill s3 INT
```

</details>

## warm-send

**借用已运行服务发送时，完成规则与冷启动相同，且不停止该服务**

运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/warm-send' -v`

**目的**

验证同一条发送规则同时适用于借用的服务和新启动的临时服务。


**流程**

1. 启动 alpha 和 beta 并配对批准，两者都保持运行。
2. 在 alpha 上执行 `unic send --to all --json`，对端在线。
3. 停止 beta 后再次提交，对端离线。

**期望**

- 第一次提交报告 outcome 为 success，beta 收到内容。
- 第二次提交的本地提交为 committed，发送结果不是 success。
- 两次命令结束后 alpha 的服务仍在运行。

**不应**

- 命令结束后停止借用的服务。
- 对端离线时报告发送成功。

<details><summary>脚本 testdata/warm-send.txtar</summary>

```
node-start alpha
node-start beta
pair alpha beta

u alpha send --to all --json warm text
stdout '"submission":'
stdout '"outcome":"success"'
until beta 'warm text' h show
! service-gone alpha

node-stop beta TERM
sleep 11s
u alpha send --to all --json warm offline text
stdout '"phase":"committed"'
! stdout '"outcome":"success"'
! service-gone alpha
```

</details>

