# CLI 端到端测试

用真实的 `unic` / `unicd` 二进制做黑盒验收：同一台机器上起多个 `unicd`，各自有独立状态目录和**目录剪贴板**（环境变量 `UNIC_MOCK_CLIPBOARD_DIR`，不碰系统剪贴板），只通过 `unic` 命令和剪贴板目录操作与观察。

## 先看这里

**[CASES.md](CASES.md)**：所有用例的说明——目的、流程、期望什么、不应出现什么，以及完整脚本（折叠）。不需要读 Go 代码。它由用例顶部的说明自动生成，过期会让 `go test` 失败。

## 运行

```sh
# 全部用例（约 2 分钟；无需图形会话，会构建 unic 与 unicd）
UNIC_E2E=1 go test ./e2e/cli -v

# 单个用例（名字见 CASES.md）
UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/pair-and-sync-text' -v

# 用已构建好的二进制（目录里放 unic 和 unicd）
UNIC_E2E=1 UNIC_E2E_BIN=/path/to/bin go test ./e2e/cli -v
```

不设 `UNIC_E2E` 时，`go test ./...` 只检查 CASES.md 是否最新，不起进程。失败时输出里有每一步命令、输出和节点日志位置（`-v`）。

## 新增用例

1. 在 `testdata/` 新建 `<名字>.txtar`。顶部必须有这几段说明，缺任何一段 `TestCatalog` 都会失败：

   ```
   # 用例: 一句话标题
   # 目的: 为什么要有这个用例
   # 流程:
   #   1. ...
   # 期望:
   #   - ...
   # 不应:
   #   - ...
   ```

   说明只写脚本真正检查的事；写了但没检查，就是在骗人。
2. 空行之后写脚本；需要的配置文件、文本夹具用 `-- 文件名 --` 附在后面（`alpha/config.toml` 会成为 alpha 的配置）。
3. `go test ./e2e/cli -run TestCatalog -update` 重新生成 CASES.md，再一起提交。

## 脚本命令

脚本是 [testscript](https://pkg.go.dev/github.com/rogpeppe/go-internal/testscript)：内置的 `exec`、`cmp`、`stdout`、`!`（取反）、`cp`、`env` 等照常可用。下面是本仓库额外提供的命令（实现见 `e2e_test.go`）：

| 命令 | 作用 |
|---|---|
| `node-start <名>` | 启动节点，等就绪；用 `<名>/` 下预置的 `config.toml`。已停止的节点再启动即重启（状态保留）。`! node-start <名>` 对运行中的节点表示"第二个实例应被拒绝" |
| `node-stop <名> [TERM\|KILL\|STOP\|CONT]` | 发信号；TERM/KILL 等进程退出 |
| `u <名> <参数…>` | 以该节点身份运行 `unic`，输出供 `stdout` 断言 |
| `until <名> <正则> <参数…>` | 反复运行 `unic` 直到输出匹配（最多 40 秒）；`! until` 要求 3 秒内一直不匹配 |
| `pair <a> <b> [request\|approve]` | b 用 a 的完整地址配对；request 阶段核对两端指纹一致，approve 阶段双方批准并等到互相 online；不写阶段则两步都做 |
| `board-set <名> <类型> <文件> […]` | 像应用一样设置节点剪贴板；类型 `text`/`html`/`png`/`files`，可同时给多个表示 |
| `board-wait <名> <类型> <文件>` | 等节点剪贴板内容与文件逐字节相同（files 比较所列文件的内容）；`!` 要求 3 秒内一直不同 |
| `retry-copy <from> <to> <前缀>` | 每 5 秒在 from 复制 `<前缀> <n>`，直到 to 的剪贴板收到（最多 90 秒） |
| `node-id <名> <变量>` | 把设备 ID 放进环境变量 |
| `install-plugin <名> <插件>` | 把 `plugins/examples/lua/<插件>` 复制到节点的本地插件目录 |
| `genfile png\|rand <路径> [字节数]` | 生成 PNG 或确定性随机文件 |
| `u-async <id> <名> [ENV:K=V…] <参数…>` / `u-wait <id>` / `u-kill <id> [KILL\|INT]` | 后台运行 `unic`；等待它结束；发信号并等它退出（命令已结束时失败） |
| `logs <名> <子串>` | 节点日志必须包含该子串；`!` 表示不得包含 |
| `sleep <时长>` | 等待 |

## 注意

- 每个用例用全新的目录，互不影响；`unicd` 进程在用例结束时被强杀。
- 节点之间走真实的 tailcat 连接（本机直连），不依赖外部服务；配对码服务不用，只测完整地址配对。
- 不覆盖：真实系统剪贴板、真实休眠/唤醒、切换网络、跨机器。这些仍属真机基线（执行计划 0004 P0）。
- 目录剪贴板由 `app/desktop/internal/board/dir.go` 实现，只在设置了 `UNIC_MOCK_CLIPBOARD_DIR` 时启用，平时的 `unicd` 不受影响。

## Sync settings and receive ownership

See the [CLI acceptance matrix](MATRIX.md) for coverage and remaining checks.

## OTLP logs and metrics

The Go scenarios `TestObservability`, `TestObservabilityPipeline`,
`TestObservabilityProductionCycle`, `TestObservabilityResources`, and
`TestObservabilityFaults` run real processes with a local HTTPS OTLP receiver.
They are separate from the generated txtar catalog.

The production daemon is UI-free. The test-only `unic_e2e` tag adds diagnostics fixtures and a directory-backed IPC adapter.
Release builds exclude this entry and its fixed fixtures. Diagnostic scenarios
use the existing host transport seam over local TCP; they do not establish
public DERP or native clipboard acceptance.

```sh
mkdir -p /private/tmp/unic-otlp-bin
go build -race -tags unic_e2e -o /private/tmp/unic-otlp-bin/unicd ./app/service/cmd/unicd
go build -race -o /private/tmp/unic-otlp-bin/unic ./app/service/cmd/unic
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN=/private/tmp/unic-otlp-bin UNIC_E2E_ARTIFACTS=/private/tmp/unic-otlp-evidence go test -race ./e2e/cli -run '^TestObservability' -count=1 -v
```

Most scenarios explicitly use 500 ms metrics and 100 ms logs intervals in the
nonvisual test host. The production-cycle case uses the normal 60 s / 5 s
intervals and observes a complete disabled interval first.
Artifacts include decoded OTLP, local JSONL, assertions, source/binary hashes,
resource samples, and `replay.sh`. Run from an immutable source snapshot when
other work can change the checkout during acceptance.

## Production headless acceptance

Run `make headless-e2e` to build production binaries without test tags and exercise two real services plus a separate directory-backed adapter process.
The result is `.cache/0015-acceptance/manifest.json`, with source hashes, binary hashes, dependency inventory and passed checks.
This fixture does not open a graphical application or access the system clipboard.


## PostHog integration

Run `make posthog-e2e` on macOS for nonvisual local TLS acceptance.
It uses real `unic`/`unicd` processes, synthetic ingestion credentials and isolated profiles.
The release-assembly scenario builds a production daemon without test tags and verifies default-on logs, delta metrics and product events at the normal export intervals.
It also verifies pre-start false, endpoint-bound credentials, all five event types, idempotency, identity rotation, privacy, queue bounds and transport faults.
Evidence is written under `.cache/0019/acceptance/`, including per-run source/binary hashes, sanitized decoded data, assertions, checksums and `replay.sh`.
Cloud read-back and native GUI/system clipboard acceptance are separate and are not performed by this command.
Use `TMPDIR=/private/tmp` for the broader suite on macOS; the existing file cache rejects symlinked temporary paths.
