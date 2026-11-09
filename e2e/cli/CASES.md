# CLI 端到端用例

> 本文件由 `go test ./e2e/cli -run TestCatalog -update` 根据 `testdata/*.txtar` 顶部的说明生成，不要手改。
> 怎么运行、怎么新增用例见 [README.md](README.md)。

| 用例 | 标题 |
|---|---|
| [content-formats](#content-formats) | HTML、PNG 和文件原样同步 |
| [crash-and-suspend-recovery](#crash-and-suspend-recovery) | 对端崩溃或挂起后自动恢复 |
| [data-dir-single-instance](#data-dir-single-instance) | 同一数据目录只允许一个 unicd，日志不泄露秘密 |
| [pair-and-sync-text](#pair-and-sync-text) | 两台设备配对批准后，文本双向同步 |
| [plugin-sensitive-blocks-sync](#plugin-sensitive-blocks-sync) | 敏感内容插件：打标签、禁止同步、设置过期 |
| [plugin-tag-and-action](#plugin-tag-and-action) | 邮箱标签插件与 Action 改写 |
| [restart-keeps-state](#restart-keeps-state) | 重启后身份、配对、历史与标签保持不变 |

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
until alpha 'online' peers
until beta 'online' peers
u beta status
stdout $BETA
! board-wait beta text offline.txt
u beta requests
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
until alpha 'online' peers
board-set alpha text again.txt
board-wait beta text again.txt

! logs alpha SENTINEL-4f9c1e
! logs beta SENTINEL-4f9c1e
! logs alpha unic-pair:
! logs beta unic-pair:
logs alpha in_use
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
u alpha h action @1 example.lua-textcase/title
stdout 'new entry'
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
until alpha $BETA'\s+\S+\s+online' peers
until beta $ALPHA'\s+\S+\s+online' peers
u beta requests
! stdout $ALPHA
u beta h list --tag keep
stdout 'alpha'

board-set alpha text two.txt
board-wait beta text two.txt
```

</details>

