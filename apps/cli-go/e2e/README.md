# Go CLI 差分端到端测试

`apps/cli-go` 的 Rust/Go 差分端到端检查。每个场景分别用 Rust `uniclip` 和 Go
`uniclip` 各跑一遍，两者都连接同一个真实 `uniclipd` 构建，并各自使用全新的隔离
HOME 与 profile。

## 安全约束

不要在这些工具之外直接运行任何一个 CLI。`isolated.py` 会设置临时 HOME、唯一的
`UC_PROFILE`、`UNICLIPBOARD_ENV=development`（文件密钥存储，不访问系统钥匙串）与
`UC_DISABLE_SYSTEM_CLIPBOARD=1`（daemon 不读写系统剪贴板），并拒绝使用真实 HOME。

## 文件

| 文件 | 作用 |
| --- | --- |
| `compat.py` | 运行器：按实现分别执行场景，归一化易变值，写出原始记录与差异 |
| `scenarios.py` | 共享夹具（`init_space`、`pair`）以及生命周期、参数错误场景 |
| `scenarios_<group>.py` | 各命令组的场景，自动加载 |
| `dump_help.py` | 导出每个命令路径的 `-h` / `--help`，作为 help 对照基准 |
| `iso.py` | 在共享隔离 HOME 中运行单个二进制，用于手工检查 |

## 运行

```sh
# 两个目录各自包含 uniclip 与同级的 uniclipd。
python3 apps/cli-go/e2e/compat.py --rust target/compat/rust --go target/compat/go \
  --out target/compat/run [--only SCENARIO ...]
```

`summary.tsv` 按场景列出 `same` / `DIFF`；`<scenario>.diff` 是归一化后的统一差异，
`<scenario>.<flavor>.txt` 是原始步骤记录。`pair` 夹具需要访问生产 rendezvous 服务。

## 前台与用户服务验收

```sh
python3 apps/cli-go/e2e/service_lifecycle.py --bin target/compat/go --out target/compat/service
```

目录须含从当前源码构建的 `uniclip` 与 `uniclipd`。运行器使用唯一 HOME/profile、文件密钥
存储、禁用系统剪贴板，在真实 macOS GUI launchd 或 Linux 用户 systemd 上检查前台信号、
原子锁拒绝接管、退出码、服务完整生命周期、重复操作、HTTP 超时和 profile 隔离。
`result.json`、服务定义、前台日志和二进制哈希组成可重复验收工件；finally 只清理本次唯一
服务，并记录卸载与进程退出证据。退出码 37 使用子进程夹具验证传递，不冒充真实 daemon 失败。
旧 `start` 的警告及新增命令帮助是有意差异，不能要求与旧 Rust 帮助逐字节相等。

## 设备组交互验收

```bash
python3 apps/cli-go/e2e/trust_workflow.py --bin target/compat/go --out target/compat/trust
```

运行器使用隔离 HOME/profile、文件密钥存储及本地邀请服务，通过真实 daemon 配对三个
节点并提出移除，验证设备名称、编号输入、本机移除取消与确认，以及输入期间的并发选择。

另有 HTTP 契约场景：健康检查、身份验证和 WebSocket 控制租约仍连接真实 daemon，
只对设备组查询和提交注入固定 DTO，验证多问题连续处理、最新 revision、非法编号不提交、
`--once`、各种结果的停止条件、跨问题确认隔离、显式 ID 和 JSON 的单次兼容性。
这些场景验证 CLI 的交互与传输契约，不证明 Engine 会生成同样的问题序列或决定结果。
`--contract-only` 可跳过三节点场景；验收记录保存在 `transcript.txt`。

差分测试中设备组的人类可读输出与 Rust 开发 CLI 存在有意差异，见上级 README 差异表；
JSON 状态和显式 ID 的调用仍按原契约对比。

## 发送等待的虚拟时间检查

发送前的设备条件、500ms 轮询、共享绝对截止时间及单次发送准入使用生产共享代码，
在 `testing/synctest` 内验证，不启动 daemon、不访问网络或系统凭据：

```bash
(cd apps/cli-go && go test -count=1 -run '^TestSendVirtualTime' ./internal/commands)
(cd apps/cli-go && go test -race -count=1 -run '^TestSendVirtualTime' ./internal/commands)
```

`tests/e2e/tests/send_connect_wait.rs` 保留真实进程启动、重连、传输、离线后无补送和取消检查。
其多节点夹具使用现有 `LocalRendezvous`，地址会在 daemon 重启后保留。该本地邀请目录
不模拟 P2P 网络，也不证明公网服务可用；Python 差分的 `pair` 仍依赖生产目录服务，
须与本地目录组分开记录。公网目录冒烟可独立运行 `compat.py --only send_to_paired_peer`。

构建与执行真实等待组：

```bash
cargo build -p uc-daemon --features uc-daemon/e2e-rendezvous
scripts/e2e/build-cli.sh
cargo test --manifest-path tests/e2e/Cargo.toml --test send_connect_wait -- --ignored --test-threads=1
```

手工运行这组 Rust 测试二进制时也应使用临时 HOME 和独立 XDG 目录，禁用系统剪贴板；
不要直接手工运行内部 CLI。Unix 字节进度用例沿用 `UC_E2E_RECORD_ONLY` 门控，默认通过
并不表示已检查终端进度。详细范围和覆盖迁移见
[规格](../../../docs/specs/2026-10-23-cli-virtual-time-spec.md)与
[执行计划](../../../docs/specs/2026-10-23-cli-virtual-time-plan.md)。
