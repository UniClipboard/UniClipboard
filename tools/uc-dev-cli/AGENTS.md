# uc-dev-cli 本地规则

## 定位

`uc-dev-cli` 是 UniClipboard 的开发与诊断命令行 crate，构建出的二进制名是 `uc-dev-cli`（原 `uc-cli` / `uniclip`），位于 `tools/uc-dev-cli`（原 `apps/cli`）。它是 workspace 成员但不在 `default-members` 中，`publish = false`。

**面向用户的终端客户端 `uniclip` 由 Go 实现 `apps/cli-go` 承担，发布产物使用它。** 用户命令的新功能和修复应在 Go CLI 中完成，不在本 crate 新增用户功能。本 crate 保留 `dev-tools` 诊断能力、端到端测试的历史种子工具和现有命令作为兼容性对照；CI 与 `scripts/e2e` 仍有消费者，不是无调用方的废弃产物。

## 必守边界

- 普通兼容命令通过 `uc-daemon-client` 的 `DaemonService` 或查询、控制客户端访问外部 daemon，不构造进程内 Engine，也不回退到进程内执行。
- `start` / `stop` 通过 `uc-daemon-process` 管理外部 `uniclipd`；本 crate 没有内嵌 daemon 子命令。
- `dev-tools` 是编译时 feature，启用 `blob`、`dev`、`probe`、`mobile debug` 及其 Engine / bootstrap / platform 依赖；运行时 `--dev` 只选择开发安全存储，不能替代它。
- 进程内诊断使用 `uc-bootstrap::build_cli_engine_runtime` 和 Engine 的 `Operation` / `DevOperation`，不要在命令文件里拼装依赖或复制业务规则。Engine 内部包在 `UniClipboard/Engine` 维护。
- `dev`、`blob`、`mobile debug` 的独立 Engine 与同 profile daemon 可能竞争；保持现有探测和拒绝策略，使用独立测试 profile，不绕开守卫。
- `probe` 直接访问平台剪贴板，`probe restore` 是直接写本机系统剪贴板的诊断入口，仅供开发与 E2E 调试。普通 `get --copy` 使用 OSC 52 请求当前终端复制内容或文件路径，不调用平台剪贴板 API。
- 调整诊断或兼容命令时，在现有 `README.md` 更新对应说明，不新增重复文档。

## 输出约定

- CLI 在终端中的所有可见输出必须使用英文，包括 help、错误提示、状态提示、交互 prompt、JSON 字段名以外的文字说明以及示例命令。
- JSON 字段名必须保持稳定，避免破坏脚本调用者。
- 人类可读输出和 JSON 输出要同时考虑；支持 `--json` 的命令不要只改一种输出。
- 退出码使用 `src/exit_codes.rs` 中的常量，不要在命令里散落魔法数字。

## 视觉缩进与字符规范

所有面向终端的人类可读输出（提示行、状态行、交互 prompt、错误、章节头）必须走 `src/ui.rs` 暴露的辅助函数；**不要** 在命令实现里直接 `eprintln!` / `println!` / `Term::stderr().write_line(...)` 拼前缀。每一行都要遵循统一的视觉模板：

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

所有 Cargo 命令都从仓库根目录（cargo workspace 根）执行。

仅修改文档或说明注释时，核对 Cargo 包名、二进制名、feature、命令入口和消费者，执行 `git diff --check`；不新增测试，不要求重型构建。

行为改动优先复跑现有真实进程端到端测试，并保留可验证产物。`README.md` 提供分开构建 daemon、Go CLI 和 Rust 开发工具的历史搜索复跑入口与证明边界。CI 在 `target/e2e-dev` 单独构建本工具，并用 `UC_E2E_DEV_CLI` 供测试选择，避免开发工具 feature 合并进 daemon 构建。

需要检查参数时可从仓库根目录运行：

```bash
cargo run -p uc-dev-cli -- --help
cargo run -p uc-dev-cli --features dev-tools -- dev --help
cargo run -p uc-dev-cli --features dev-tools -- blob --help
```

目录内 `tests/directory_capture_e2e.rs` 仍引用旧的 `CARGO_BIN_EXE_uniclip`，不能将其当前状态当作可直接通过的端到端验证入口；修复属于另项行为维护。
