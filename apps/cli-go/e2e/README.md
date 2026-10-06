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
