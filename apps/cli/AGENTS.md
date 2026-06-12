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

## 视觉缩进与字符规范

所有面向终端的人类可读输出（提示行、状态行、交互 prompt、错误、章节头）必须走 `src/ui.rs` 暴露的辅助函数；**不要**在命令实现里直接 `eprintln!` / `println!` / `Term::stderr().write_line(...)` 拼前缀。每一行都要遵循统一的视觉模板：

```text
 {glyph}  {content}
```

**1 个 leading space + 1 字符 glyph + 2 个 spaces + 内容**。glyph 与内容之间永远是双空格，单空格会让内容起始列偏 1 列，与同屏其它行不对齐。

| 用途 | glyph | 颜色 | 函数 |
| --- | --- | --- | --- |
| 章节标题 | `◆` | cyan + bold | `ui::header` |
| 成功 / 完成收尾 | `✓` / `└` | green | `ui::success` / `ui::end` |
| 警告 | `⚠` | yellow | `ui::warn` |
| 错误 | `✗` | red | `ui::error` |
| 信息 / 子项 | `│` | dim | `ui::info` / `ui::bar` / `ui::verification_code` |
| 交互提示（live） | `?` | yellow | `ui::confirm` / `ui::input` / `ui::password` 内部 |
| 交互完成（resolved） | `✓` | green | `UniclipTheme::format_*_selection` 内部 |

dialoguer 的 `Confirm` / `Input` / `Password` 必须用 `ui::confirm` / `ui::input` / `ui::password`，它们已经绑定了 `UniclipTheme` 与 `Term::stderr()`，不要直接构造 dialoguer 组件或换用 `dialoguer::theme::ColorfulTheme`。新增交互 prompt 时按以下要求写：

- prompt 文本不要以 `:` 结尾——`UniclipTheme` 会自动接 `[y/N]` / `[default]` 等后缀。
- 想给"按 Enter 走默认值"的语义，prompt 末尾用 `[Enter for auto]` 之类的人类提示，并把 `allow_empty=true` 传给 `ui::input`。
- 必填字段 `allow_empty=false`，由 dialoguer 自动重读；不要在外层手写"空 → 报错退出"的旧逻辑。

新增 ui 函数时，把渲染集中在 `src/ui.rs`，并在文档注释上画出最终视觉的 `text` 块（可参照现有 `read_masked_password` 的 doc）。这样下一个改动者改格式时只要看一处。

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
