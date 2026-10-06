# uniclip（Go 实现）

`apps/cli-go` 是 `uniclip` 命令行的 Go 实现。它只做参数解析、交互、终端输出与退出码，
所有业务动作都通过 `uniclipd` 已有的 HTTP / WebSocket 接口完成，与 Rust 实现
`tools/uc-dev-cli`（原 `apps/cli`）使用同一套路由、DTO、`daemon.conn` / `.daemon-pid` / 交接记录与环境变量约定。

用户端发布构建使用本 Go 实现；Rust `uc-dev-cli` 仅供开发与诊断。

## 结构

| 路径 | 职责 |
| --- | --- |
| `cmd/uniclip` | 入口 |
| `internal/cli` | 命令规格、基于 cobra/pflag 的解析，以及 clap 风格的 help、参数错误与"相似命令"提示 |
| `internal/commands` | 命令树（`tree.go`，help 文本的唯一来源）与各命令实现 |
| `packages/desktop-host-go/apppaths` | 数据目录、profile 后缀、便携模式、日志目录（对应 `uc-app-paths`） |
| `packages/desktop-host-go/daemonproc` | `daemon.conn`、`.daemon-pid`、进程身份校验、分离启动与交接记录（对应 `uc-daemon-process`） |
| `packages/desktop-host-go/daemonlife` | `/health` 版本契约判定、活跃 PID 复用守卫与健康等待 |
| `internal/localdaemon` | 终端启动提示、oneshot 启动与提升（对应 Rust `local_daemon.rs`） |
| `internal/session` | 连接或拉起 daemon、控制租约、setup 状态（对应 Rust `app_session.rs`） |
| `packages/desktop-host-go/daemonclient` | 会话令牌、信封请求、请求错误、WebSocket 订阅与租约（对应 `uc-daemon-client`） |
| `internal/ui`、`internal/output`、`internal/exitcode` | 终端版式与交互、JSON 输出、退出码 |
| `packages/desktop-host-go/errctx` | 与 anyhow `.context()` 一致的错误显示 |
| `packages/desktop-host-go/buildinfo` | 由 `go generate` 从 `Cargo.toml` 与 daemon 契约生成的版本与 API revision |
| `e2e/` | Rust/Go 差分端到端测试与 help 对照，见 `e2e/README.md` |

## 构建

需要 Go 1.26 或更高版本；`go.mod` 的 `toolchain go1.27.1` 会在本机版本较旧时自动下载对应工具链。依赖：
`github.com/spf13/cobra`、`github.com/spf13/pflag`、`github.com/coder/websocket`、`golang.org/x/term`、`golang.org/x/sys`。

```sh
cd apps/cli-go
(cd ../../packages/desktop-host-go && go generate ./buildinfo)   # 版本或 daemon API revision 变化后
go build -o ../../target/release/uniclip-go ./cmd/uniclip
```

发布目标三元组的静态构建（`CGO_ENABLED=0`，与 `build-cli.yml` 的矩阵一致）：

```sh
scripts/ci/build-go-cli.sh aarch64-apple-darwin target/uniclip
```

脚本会在 `packages/desktop-host-go/buildinfo` 与 `Cargo.toml` 或契约 revision 不一致时失败。运行时 `uniclipd`
必须与 `uniclip` 同目录或在 `PATH` 上，与 Rust 版相同。

## 前台运行与用户服务

```sh
uniclip run                         # 前台运行，终端信号和退出码直接交给 daemon
uniclip run --server                # 无系统剪贴板的前台节点，适合容器
uniclip --profile work service start # 安装并启动当前用户服务，登录后自动启动
uniclip --profile work service status
uniclip --profile work service restart
uniclip --profile work service stop  # 停止并关闭登录启动，保留服务定义
```

`run` 在 macOS/Linux 使用进程替换，标准输入、输出、错误和信号均保留；daemon 的退出码
直接成为命令退出码。Windows 等待同控制台的子进程结束并返回其退出码，控制台行为尚需原生验收。
未完成 Space 初始化也可运行 daemon，随后从另一终端执行 `space init` 或 `space join`。
已有 daemon（包括 GUI 所有、oneshot、健康暂时不可达或版本不兼容）时，`run` 拒绝启动，
不会接管或终止它。daemon 的 `UC_DAEMON_NO_TAKEOVER=1` 启动契约还在原子锁处拒绝并发竞争；
`run` 强制启用单例保护。CLI 和 daemon 必须使用同批发布产物。

`service` 仅支持 Linux 的 systemd 用户服务和 macOS 的 launchd LaunchAgent。
Windows 返回明确的未支持错误，其余 CLI 命令仍可使用。不使用 sudo，不安装全局服务。
macOS 要求当前用户有 GUI 登录域；Linux 要求可连接用户 systemd 管理器。
服务按 HOME 和 profile 命名，只管理本命令创建的服务；`start` 幂等安装/启动，
同配置且正在运行时保留 PID，运行中的配置不同则要求先 `service stop` 再以新选项启动。
`restart` 使用已安装的选项并恢复登录启动，不读取新的 `--server` 设置。`stop` 重复调用成功。
服务定义保留 `--profile`、`--dev` 对应环境、HOME、XDG 数据目录与日志上下文，
不保存 daemon 令牌、密码或 shell 中的所有环境变量。用户钥匙串仍可能因登录状态而不可用，
此时 HTTP 健康会失败，应检查日志。不要让 GUI 与用户服务争用同一 profile。

生产服务要求稳定安装的 `uniclip` 和同目录 `uniclipd`，解析符号链接后拒绝临时目录和
Git 工作树；便携安装不支持用户服务。开发模式允许工作树二进制，但其路径随工作树删除失效。
服务启动时再次经过 `run` 的已有 daemon 守卫。服务没有自动崩溃重启策略，避免错误配置循环启动；
发生失败时可查看状态/日志并显式重启。

`service status` 分别显示 installed、loaded、running 与 http_health，HTTP 状态复用现有
版本契约探测并核对服务 PID。安装或 PID 存在不代表健康；无运行服务、HTTP 不可达、
版本不兼容或端点属于其他 daemon 时返回非零。`recovery_required` 保留原样，表示 daemon
可达但还需要恢复。`start/restart` 等待 HTTP 健康，失败返回非零并保留服务供诊断。
macOS 控制台日志在 profile 日志目录的 `service.stdout.log` / `service.stderr.log`，
Linux 控制台日志使用 `journalctl --user -u <status 中的 name>.service`；daemon 自身继续使用原有轮转日志。

登录/重启边界：macOS 在下次 GUI 登录加载 LaunchAgent；Linux 在用户管理器启动时加载启用的
服务。两者都不承诺未登录就启动。Linux 如需开机用户服务，可由管理员独立配置 linger；
CLI 不更改该系统设置。手动删除服务：先 `service stop`，再删除 HOME 下对应 LaunchAgent
或 XDG_CONFIG_HOME 下对应 systemd 用户 unit；Linux 删除后执行 `systemctl --user daemon-reload`。

兼容期中 `uniclip start`（含 `--foreground`、`--server`）保留原有后台默认和 setup 检查，
只向标准错误输出迁移提示；不会让旧后台脚本突然阻塞。旧 `start --foreground` 保留原语义，
新脚本应使用 `run`。兼容别名计划在后续显式发布变更中移除，本次不删除。

## 兼容范围

- 全部发布版命令（包括隐藏的弃用别名 `status`、`init`、`invite`、`join`、`members` / `devices`、`recv`、`mobile-sync`）均已实现。
- 每个命令路径的 `-h` 与 `--help` 与 Rust release 构建逐字节一致；参数错误（未知参数、相似命令提示、冲突、缺失、取值非法、数值范围）的文本与退出码 2 一致。
- 标准输出、标准错误、JSON 字段名与顺序、退出码、daemon 生命周期（复用、oneshot、提升、前台）、版本不兼容判定、WebSocket 事件流与 Ctrl-C 行为，按 `e2e/` 中的差分场景逐一对比。

### 已知差异

| 场景 | Rust | Go | 处理 |
| --- | --- | --- | --- |
| daemon 生命周期命令 | `start` 默认后台 | 新增 `run` 和 `service`；`start` 保留旧行为但向标准错误输出弃用提示 | 有意变更，根 help 和 `start` help 也随之更新 |
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

1. **保留 Rust 开发工具二进制（已采用）**：Rust CLI 重命名为包 `uc-dev-cli`、二进制 `uc-dev-cli`，目录迁至 `tools/uc-dev-cli`，并从 `default-members` 中排除，用 `cargo build -p uc-dev-cli --features dev-tools` 构建，仅供开发、诊断与 E2E 使用；用户端 `uniclip` 是本 Go 实现。改动最小，不扩大 daemon 接口；代价是 `apps/cli` 在开发期继续存在。
2. **增加仅开发构建可用的 daemon 路由**：把上述操作暴露为受特性开关保护的 daemon 接口，再由 Go 实现。可以彻底移除 Rust CLI，但会扩大 daemon 的攻击面，需要单独评审。
3. **Go 侧直接访问平台剪贴板**（仅 `probe`）：需要 cgo 或平台 API 绑定，并与 `uc-platform` 重复实现，不推荐。

已采用方案 1。

## 平台验证边界

- macOS arm64：本机原生构建，Rust/Go 差分端到端测试与仓库自带 `tests/e2e` 套件。
- Linux aarch64：在 ARM Linux 真机上原生运行差分测试。
- Linux x86_64、Windows x86_64 / arm64、macOS x86_64：只验证了交叉编译，没有原生运行，交叉编译不等于原生验收。

## 发布构建

用户端 `uniclip` 的发布构建已改为本 Go 实现；Rust CLI 已重命名为 `uc-dev-cli`，不再进入任何生产构建。

- `build.yml`（桌面端与 macOS / Windows x64 的 CLI 压缩包）：`build-sidecar` 先构建并以单个 tar 上传 `uniclipd`，随后 `build-cli`（Go）只依赖沙车产物。
- `build-cli.yml`（Linux musl 静态 CLI，以及手动全平台）：同样先构建 `uniclipd`，再由 Go 作业打包。
- `deploy/vps/Dockerfile`：Rust 阶段只构建 `uniclipd`，独立的 Go 阶段构建 `uniclip`。
- 产物名与压缩包内容（`uniclip` + `uniclipd` 同目录）保持不变，`release.yml`、npm 打包与签名公证流程无需改动。
- 沙车用单个 tar 传递：保留可执行位，并避免 `uniclipd-*.exe` 被 `release.yml` 的 `*.exe` 资产收集规则误发布。
- 守卫（`scripts/__tests__/cli-packaging.test.ts`、`scripts/architecture/check-engine-repository.mjs`）：生产构建文件不得出现 `uc-dev-cli` 或 `uc-cli`，任何包不得依赖 `uc-dev-cli`，`packages/desktop-host-go/buildinfo` 必须与 `Cargo.toml` 和 daemon 契约一致。

这些 CI 变更只做了静态验证（actionlint、全部脚本测试、变异检查），没有在 GitHub 上真实运行过；macOS 的 Go 二进制签名与公证、Windows 自托管运行器上的 Go 工具链尚未验证。
