# uc-dev-cli

`uc-dev-cli` 是 UniClipboard 的开发与诊断命令行工具，Cargo 包名和二进制名均为
`uc-dev-cli`，位于 `tools/uc-dev-cli`（原 `apps/cli`，原包名 `uc-cli`、二进制名
`uniclip`）。它是 workspace 成员，不在 `default-members` 中，且 `publish = false`。

面向用户的 `uniclip` 由 [apps/cli-go](../../apps/cli-go/AGENTS.md) 维护并用于发布。
用户命令的新功能和修复应在 Go CLI 中完成；本 crate 保留开发诊断、端到端测试的
历史种子工具，以及现有命令作为兼容性对照，不再新增用户功能。

## 运行方式与构建能力

所有命令都从仓库根目录执行。未启用 feature 时，本工具仍保留通过 daemon 执行的
兼容命令，例如空间状态查询：

```bash
cargo run -p uc-dev-cli -- --help
cargo run -p uc-dev-cli -- --profile dev-cli-check --json space status
```

开发诊断需要显式启用 `dev-tools`：

```bash
CARGO_TARGET_DIR=target/e2e-dev cargo build -p uc-dev-cli --features dev-tools
./target/e2e-dev/debug/uc-dev-cli dev --help
./target/e2e-dev/debug/uc-dev-cli blob --help
./target/e2e-dev/debug/uc-dev-cli probe --help
./target/e2e-dev/debug/uc-dev-cli mobile debug --help
```

Windows 二进制带 `.exe` 后缀。单独的 `target/e2e-dev` 构建目录与 CI 一致，避免把
开发工具的 Engine feature 合并进 daemon 构建。

| 构建方式 | 能力与依赖 |
| --- | --- |
| 不启用 `dev-tools` | 保留 daemon 客户端命令；使用 `uc-daemon-client`、`uc-daemon-contract`、`uc-daemon-process` 和 `uc-app-paths`。 |
| 启用 `dev-tools` | 增加 `blob`、隐藏的 `dev`、`probe` 和 `mobile debug`；引入 `uc-engine`（含 `dev-tools`、`lan-compat`）、`uc-bootstrap`、`uc-platform` 和 `uc-observability`。 |

`--dev` 是运行时开发模式，使用文件安全存储代替系统 keychain；它不能代替
编译时的 `--features dev-tools`。`--profile <NAME>`（或 `UC_PROFILE`）选择隔离的数据、
密钥和网络身份。`--json` 用于脚本输出，`-v` / `--verbose` 用于详细日志。

## 与 daemon 和 Engine 的关系

现有空间、成员、发送、接收、搜索、移动端 LAN 配置等兼容命令通过 HTTP / WebSocket
访问外部 `uniclipd`。连接辅助函数负责复用已有 daemon，或在缺席时启动临时 daemon，
并通过控制租约保留命令期间的连接；初始化和加入有独立的连接入口，允许尚未完成
设置的 profile。普通命令不会回退到进程内 Engine。

`start` / `stop` 负责外部 daemon 的生命周期；本工具没有内嵌 daemon 子命令。
需要启动 daemon 的入口使用 `uc-daemon-process` 的二进制发现与进程管理。

`dev`、`blob`、`mobile debug` 的诊断路径使用
`uc-bootstrap::build_cli_engine_runtime` 构造独立 Engine，会拒绝同一 profile 已有
可探测 daemon 的情况。运行这些命令前停止测试 profile 的 daemon，或换用独立
profile；不要绕过现有探测守卫。Engine 业务能力由 `UniClipboard/Engine` 维护，
不要在 CLI 层复制业务规则。

`probe` 直接调用平台剪贴板适配器；`probe restore` 会写本机系统剪贴板。
兼容命令 `get --copy` 使用 OSC 52 向当前终端请求复制文本或文件路径，效果取决于
终端支持；它与直接调用平台剪贴板 API 的诊断路径不同。

## 开发诊断入口

下列命令均需 `dev-tools`。`dev`、`probe`、`mobile debug` 是隐藏命令组，
不会显示在顶层或所属父命令的常规帮助中，但可显式查看各自帮助。

```bash
./target/e2e-dev/debug/uc-dev-cli --dev --profile dev-cli-check dev seed-clipboard --text "test text"
./target/e2e-dev/debug/uc-dev-cli --dev --profile dev-cli-check --json dev dump-clipboard --limit 10
./target/e2e-dev/debug/uc-dev-cli --dev --profile dev-cli-check --json dev capture-files --path ./sample-dir
./target/e2e-dev/debug/uc-dev-cli --dev --profile dev-cli-check dev pairing addrs
./target/e2e-dev/debug/uc-dev-cli --dev --profile dev-cli-check dev pairing issue --addr <IP>
./target/e2e-dev/debug/uc-dev-cli --dev --profile dev-cli-check blob publish ./sample.bin
./target/e2e-dev/debug/uc-dev-cli --dev --profile dev-cli-check blob fetch <TICKET> --entry-id <ENTRY_ID> --out ./restored.bin
./target/e2e-dev/debug/uc-dev-cli probe capture --out snap.json
./target/e2e-dev/debug/uc-dev-cli probe inspect --in snap.json
```

Engine 诊断需要先在测试 profile 初始化或加入空间。`seed-clipboard` 写入用当前
MasterKey 加密的文本历史，不覆盖生产剪贴板捕获链路；`dump-clipboard` 输出解密预览。
`capture-files` 经真实文件捕获链路处理文件或目录，可重复 `--path`；
`--max-members` / `--max-bytes` 只覆盖本次捕获上限，不改 profile 设置。
`mobile debug` 在进程内模拟 SyncClipboard 文本与文件操作，不证明真实手机或 HTTP
网络链路正常。`blob publish` 输出 ticket 和 entry id，`fetch` 必须同时提供二者。

## 保留的兼容命令

这些入口供现有脚本与 Go CLI 行为对照使用，不代表 Rust 用户客户端仍是产品维护入口。
具体参数以 `src/main.rs` 和各命令的 `--help` 为准。

| 命令组 | 用途 |
| --- | --- |
| `start` / `stop` | 管理本机外部 daemon。 |
| `space` | 状态、初始化、邀请、加入、切换、重建空间与修改口令。 |
| `member` | 成员、信任选择及同步偏好。 |
| `send` / `watch` / `get` | 发送、监听及取回内容。 |
| `search` | 搜索、索引状态及重建。 |
| `mobile` | SyncClipboard 兼容 LAN 通道配置。 |
| `debug` / `upgrade` | daemon 日志诊断与升级管理；与隐藏的 `mobile debug` 不同。 |

旧的顶层 `status`、`init`、`invite`、`join`，以及 `members` / `devices`、`recv`、
`mobile-sync` 仍保留为隐藏兼容入口。新增脚本使用对应的规范命令。
终端可见输出保持英文，JSON 字段与退出码保留现有契约。

## 消费者与验证边界

`.github/workflows/pr-check.yml` 单独以 `dev-tools` 构建本工具，并通过
`UC_E2E_DEV_CLI` 提供给 `tests/e2e`。测试中的 `NodeBinarySet::current()` 默认使用
Go `uniclip`；`current_dev_cli()` 显式选择 Rust 工具，供需要开发能力的测试使用，
也可能覆盖整段兼容命令流程。
空间切换、口令恢复、CLI / Engine 工作流与历史搜索测试仍消费这些能力。
`scripts/e2e` 中的空间切换、剪贴板重发和移动 LAN 调试脚本也有消费者，不能把本工具
当作无调用方的废弃产物。

需要验证行为时，可复跑现有真实进程端到端测试。以下示例分别构建 daemon、Go CLI
和 Rust 开发工具，再运行历史搜索测试；本次文档更新不执行构建或测试：

```bash
cargo build -p uc-daemon
bash scripts/e2e/build-cli.sh
CARGO_TARGET_DIR=target/e2e-dev cargo build -p uc-dev-cli --features dev-tools
UC_E2E_DEV_CLI="$(pwd)/target/e2e-dev/debug/uc-dev-cli" \
  cargo test --manifest-path tests/e2e/Cargo.toml --test history_search_counts -- --ignored --nocapture
```

该历史搜索测试全程选择 Rust 开发工具，不能替代 Go CLI 的行为验收。它使用独立
临时 profile，默认产物目录为 `target/e2e-artifacts/history-search`，
包含输入、请求响应与 daemon 日志，也可用 `UC_E2E_ARTIFACT_DIR` 指定目录。
复跑时记录源码提交、Engine 来源、平台、二进制路径和结果；编译成功、帮助可显示或
种子成功均不能替代端到端验证，也不能证明真实桌面、手机、安装发布或自启动行为。

`tools/uc-dev-cli/tests/directory_capture_e2e.rs` 仍引用旧的
`CARGO_BIN_EXE_uniclip`，与当前二进制名不符；该遗留测试需要另行修复，不能当作当前
可直接通过的验证入口。仅修改文档或注释时，核对路径、参数、feature 和消费者并执行
`git diff --check` 即可，无需新增测试或重型构建。
