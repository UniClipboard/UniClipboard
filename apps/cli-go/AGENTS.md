# cli-go 本地规则

## 定位

`apps/cli-go` 是 `uniclip` 命令行的 Go 实现，模块为
`github.com/UniClipboard/UniClipboard/apps/cli-go`，入口 `cmd/uniclip`。迁移期间与
Rust 开发 CLI `tools/uc-dev-cli`（原 `apps/cli`）并存。用户端 `uniclip` 的发布构建使用本 Go 实现，Rust CLI 不进入任何生产构建。

它只负责参数解析、终端输出、交互输入、退出码，以及通过 daemon 已有的 HTTP / WebSocket
接口调用权威应用动作。

## 必守边界

- 业务动作一律经 `uniclipd` 的 HTTP / WebSocket 接口（与 `crates/uc-daemon-client` 相同的路由与 DTO）完成；不得在 Go 中复制 Engine 的业务、持久化、加密、配对或同步规则。
- 不得调用 Rust `uniclip` 进程来"实现"命令。
- daemon 接口缺失时，先在 daemon 侧做最窄的权威 API 扩展（同步更新 `uc-daemon-contract`），不要在 CLI 绕行。
- `start` / `stop` 负责本机 daemon 生命周期，与 Rust CLI 共享同一套 `daemon.conn`、`.daemon-pid`、交接记录与环境变量约定（见 `internal/daemonproc`）。
- CLI 不写系统剪贴板。Rust 版隐藏的 `dev-tools` 命令（`probe`、`blob`、`dev`、`mobile debug`）依赖进程内 Engine 或平台剪贴板，没有 daemon 接口，Go 版未提供，处置见 `README.md`。

## 兼容约定

- help、参数错误、标准输出、标准错误、JSON 字段名与字段顺序、退出码必须与 Rust release 构建逐字节一致；有意的差异必须写进 `README.md` 的差异表。
- 命令树与 help 文本集中在 `internal/commands/tree.go`；clap 风格的 help 与参数错误渲染集中在 `internal/cli`。
- 人类可读的终端行统一经 `internal/ui`，保持 ` {glyph}  {content}` 版式；JSON 经 `internal/output`（两空格缩进、不转义 HTML）。
- 退出码使用 `internal/exitcode` 常量。
- 包版本与 daemon API revision 由 `go generate ./internal/buildinfo` 从 `Cargo.toml` 与 `crates/uc-daemon-contract/src/lib.rs` 生成，禁止手写。

## 验证

不编写单元测试；以真实 daemon 的 Rust/Go 差分端到端测试验证（`e2e/README.md`）。
任何对 CLI 的手工运行都必须经 `e2e/iso.py` 或 `e2e/compat.py`，绝不使用真实 HOME。

改动后至少运行：

```bash
cd apps/cli-go
go generate ./internal/buildinfo && git diff --exit-code internal/buildinfo
go vet ./...
go build -o ../../target/compat/go/uniclip ./cmd/uniclip
python3 e2e/dump_help.py ../../target/compat/go/uniclip ../../target/compat/help-go
python3 e2e/compat.py --rust ../../target/compat/rust --go ../../target/compat/go --out ../../target/compat/run
```
