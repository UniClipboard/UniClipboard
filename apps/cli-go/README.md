# uniclip（Go 实现）

`apps/cli-go` 是 `uniclip` 命令行的 Go 实现。它只做参数解析、交互、终端输出与退出码，
所有业务动作都通过 `uniclipd` 已有的 HTTP / WebSocket 接口完成，与 Rust 实现
`apps/cli` 使用同一套路由、DTO、`daemon.conn` / `.daemon-pid` / 交接记录与环境变量约定。

迁移期间两者并存。Rust CLI 仍是默认构建与发布对象；是否把发布产物切换为 Go 版本由维护者决定，
见文末「切换发布产物」。

## 结构

| 路径 | 职责 |
| --- | --- |
| `cmd/uniclip` | 入口 |
| `internal/cli` | 命令规格、基于 cobra/pflag 的解析，以及 clap 风格的 help、参数错误与"相似命令"提示 |
| `internal/commands` | 命令树（`tree.go`，help 文本的唯一来源）与各命令实现 |
| `internal/apppaths` | 数据目录、profile 后缀、便携模式、日志目录（对应 `uc-app-paths`） |
| `internal/daemonproc` | `daemon.conn`、`.daemon-pid`、进程身份校验、分离启动与交接记录（对应 `uc-daemon-process`） |
| `internal/localdaemon` | `/health` 版本契约判定、复用探测、oneshot 启动与提升（对应 Rust `local_daemon.rs`） |
| `internal/session` | 连接或拉起 daemon、控制租约、setup 状态（对应 Rust `app_session.rs`） |
| `internal/daemonclient` | 会话令牌、信封请求、请求错误、WebSocket 订阅与租约（对应 `uc-daemon-client`） |
| `internal/ui`、`internal/output`、`internal/exitcode` | 终端版式与交互、JSON 输出、退出码 |
| `internal/errctx` | 与 anyhow `.context()` 一致的错误显示 |
| `internal/buildinfo` | 由 `go generate` 从 `Cargo.toml` 与 daemon 契约生成的版本与 API revision |
| `e2e/` | Rust/Go 差分端到端测试与 help 对照，见 `e2e/README.md` |

## 构建

需要 Go 1.26 或更高版本；`go.mod` 的 `toolchain go1.27.1` 会在本机版本较旧时自动下载对应工具链。依赖：
`github.com/spf13/cobra`、`github.com/spf13/pflag`、`github.com/coder/websocket`、`golang.org/x/term`、`golang.org/x/sys`。

```sh
cd apps/cli-go
go generate ./internal/buildinfo   # 版本或 daemon API revision 变化后
go build -o ../../target/release/uniclip-go ./cmd/uniclip
```

发布目标三元组的静态构建（`CGO_ENABLED=0`，与 `build-cli.yml` 的矩阵一致）：

```sh
scripts/ci/build-go-cli.sh aarch64-apple-darwin target/uniclip
```

脚本会在 `internal/buildinfo` 与 `Cargo.toml` 或契约 revision 不一致时失败。运行时 `uniclipd`
必须与 `uniclip` 同目录或在 `PATH` 上，与 Rust 版相同。

## 兼容范围

- 全部发布版命令（包括隐藏的弃用别名 `status`、`init`、`invite`、`join`、`members` / `devices`、`recv`、`mobile-sync`）均已实现。
- 每个命令路径的 `-h` 与 `--help` 与 Rust release 构建逐字节一致；参数错误（未知参数、相似命令提示、冲突、缺失、取值非法、数值范围）的文本与退出码 2 一致。
- 标准输出、标准错误、JSON 字段名与顺序、退出码、daemon 生命周期（复用、oneshot、提升、前台）、版本不兼容判定、WebSocket 事件流与 Ctrl-C 行为，按 `e2e/` 中的差分场景逐一对比。

### 已知差异

| 场景 | Rust | Go | 处理 |
| --- | --- | --- | --- |
| 交互式口令或文本输入时连续按键 | 只有第一个按键在 raw 模式下读取，其余按键由终端回显，口令会以明文出现在屏幕上 | 全程 raw 模式，只显示掩码 | Rust 的行为是口令泄露缺陷，Go 不复制 |
| 非终端环境下的口令提示 | 忙等，直到外部超时 | 立即报错 `password input failed: IO error: not a terminal` | 不复制挂起 |
| `/health` 返回非法 JSON（只有端口被非 uniclipd 进程占用时才会出现） | 显示 serde_json 的解析错误细节 | 显示 Go `encoding/json` 的解析错误细节 | 退出码与前缀一致，细节文本不同 |
| 底层传输错误（连接中断、握手失败） | reqwest / tungstenite 的错误文本 | Go 标准库的错误文本 | 外层信息一致，底层细节不同 |
| `send --connect-timeout 18446744073709551615` | `Instant + Duration` 溢出，panic 退出 101 | 正常按超时处理 | 不复制 panic |
| 向已关闭的管道输出文本 | `print!` panic 退出 101 | 因 SIGPIPE 结束 | 不复制 panic |
| 等待窗口之外按 Ctrl-C | tokio 吞掉信号，继续执行 | 默认处理，进程结束 | 只影响极短的时间窗口 |

### 不在范围内：`dev-tools` 命令

Rust 的 `dev-tools` 特性（release 构建不包含）提供以下隐藏命令。它们直接在 CLI 进程内装配 Engine
或平台剪贴板，daemon 没有对应接口，因此 Go 版没有实现：

| 命令 | 依赖 | 使用方 |
| --- | --- | --- |
| `probe watch / capture / restore / inspect` | 进程内 `uc-platform` 系统剪贴板读写 | 开发与 E2E 调试 |
| `blob publish / fetch` | Engine `DevOperation::PublishBlob / FetchBlob` | 开发调试 |
| `dev pairing addrs / issue` | Engine 邀请地址列表与指定地址邀请 | 开发调试 |
| `dev seed-clipboard`、`dev dump-clipboard` | Engine 写入 / 读取解密后的历史 | `scripts/e2e/switch-space.sh` |
| `dev capture-files` | Engine 文件捕获管线与设置修改 | `tests/e2e`（`UC_E2E_DEV_CLI`） |
| `mobile debug put-text / put-file / get-doc / get-file` | Engine 移动同步操作 | `scripts/e2e/mobile-sync-debug.sh` |

可选的处置方式（尚未决定）：

1. **保留 Rust 开发工具二进制（已采用）**：Rust CLI 重命名为包 `uc-dev-cli`、二进制 `uc-dev-cli`，用 `cargo build -p uc-dev-cli --features dev-tools` 构建，仅供开发、诊断与 E2E 使用；用户端 `uniclip` 是本 Go 实现。改动最小，不扩大 daemon 接口；代价是 `apps/cli` 在开发期继续存在。
2. **增加仅开发构建可用的 daemon 路由**：把上述操作暴露为受特性开关保护的 daemon 接口，再由 Go 实现。可以彻底移除 Rust CLI，但会扩大 daemon 的攻击面，需要单独评审。
3. **Go 侧直接访问平台剪贴板**（仅 `probe`）：需要 cgo 或平台 API 绑定，并与 `uc-platform` 重复实现，不推荐。

已采用方案 1。

## 平台验证边界

- macOS arm64：本机原生构建，Rust/Go 差分端到端测试与仓库自带 `tests/e2e` 套件。
- Linux aarch64：在 ARM Linux 真机上原生运行差分测试。
- Linux x86_64、Windows x86_64 / arm64、macOS x86_64：只验证了交叉编译，没有原生运行，交叉编译不等于原生验收。

## 切换发布产物（待决定）

`scripts/ci/package-cli.sh` 接受任意 `uniclip` 路径，因此切换只需在 `build-cli.yml`（以及为 macOS / Windows
打包 CLI 的 `build.yml` 步骤）中，用 `scripts/ci/build-go-cli.sh <triple> <path>` 的产物替换
`cargo build -p uc-cli` 的产物。切换前还需要：决定 `dev-tools` 命令的处置方式；完成 Windows 原生验收；
确认 macOS 签名与公证流程对 Go 二进制同样适用。
