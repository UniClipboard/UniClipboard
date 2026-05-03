# uc-cli 本地规则

## 定位

`uc-cli` 是 UniClipboard 的终端入口 crate，构建出的二进制名是 `uniclip`。

它只负责命令行参数、终端输出、交互输入、进程退出码，以及把用户动作转交给应用层。不要在 CLI 层重新实现业务规则。

## 必守边界

- 业务命令必须通过 `uc-application` 的 facade 表达用户动作；不要直接访问 core、infra、platform 的内部实现来完成业务流程。
- 需要构造运行环境时，优先走 `uc-bootstrap` 提供的 CLI wiring，不要在命令文件里临时拼装依赖。
- `start` / `stop` 可以处理本机 daemon 生命周期；隐藏的 `daemon` 子命令只供 `start` 内部拉起后台进程，不是公开用户接口。
- 独立业务命令和 daemon 使用同一 profile 时可能冲突；保持现有的 daemon 探测和拒绝策略，不要为了方便绕开。
- CLI 不写系统剪贴板；诊断命令可以观察、发送或打印 payload，但系统剪贴板写入属于 daemon / 应用流程职责。
  - 唯一例外：隐藏的 `uniclip probe` 子命令组（替代旧的 `clipboard-probe` 二进制），仅供开发与 E2E 调试使用，`probe restore` 会直接写系统剪贴板。新增公开命令时不要引用这个例外作为理由。
- 新增命令时先确认它是用户命令、诊断命令还是内部命令，并在 `README.md` 中放到对应区域。

## 输出约定

- CLI 在终端中的所有可见输出必须使用英文，包括 help、错误提示、状态提示、交互 prompt、JSON 字段名以外的文字说明以及示例命令。
- JSON 字段名必须保持稳定，避免破坏脚本调用者。
- 人类可读输出和 JSON 输出要同时考虑；支持 `--json` 的命令不要只改一种输出。
- 退出码使用 `src/exit_codes.rs` 中的常量，不要在命令里散落魔法数字。

## 修改入口

| 任务 | 优先查看 |
| --- | --- |
| 新增或调整命令参数 | `src/main.rs` |
| 命令实现 | `src/commands/` |
| 共享 CLI session | `src/commands/app_session.rs` |
| daemon 启停和探测 | `src/local_daemon.rs` |
| 终端样式和交互 | `src/ui.rs` |
| 输出格式 | `src/output.rs` |
| 退出码 | `src/exit_codes.rs` |

## 验证要求

所有 Cargo 命令都从 `src-tauri/` 目录执行。

改动本 crate 后，至少运行：

```bash
cargo test -p uc-cli
cargo run -p uc-cli -- --help
```

如果改了某个子命令，还要运行对应 help，例如：

```bash
cargo run -p uc-cli -- search --help
cargo run -p uc-cli -- blob --help
```
