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
