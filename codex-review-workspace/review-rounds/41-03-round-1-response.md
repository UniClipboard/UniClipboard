# Codex Review: Plan 41-03 — Round 1

**Model:** gpt-5.4
**Session ID:** 019d010f-c585-7f20-9215-a563d93c8e0e
**Tokens used:** 54,182
**Date:** 2026-03-18

---

FINDING-1: CRITICAL
该计划把 `build_non_gui_runtime()`、`uc_daemon::rpc::types` 和 `41-01-SUMMARY.md` 当作已存在前置，但当前阶段文本本身没有把"前置产物已完成且可用"作为执行闸门，且 `status` 子命令的端到端验证实际上还依赖 daemon 侧可运行接口。这样会导致 41-03 在缺少真实依赖面时直接进入实现，执行中途失败。
SUGGESTION: 在计划开头增加显式前置检查，要求 41-01 产物已落地并可编译；把 `status` 的端到端验收依赖明确绑定到 41-02 完成，或把缺失接口创建纳入本计划，避免引用"研究/规划中的 API"。

FINDING-2: CRITICAL
计划中的 `resolve_socket_path()` 直接使用 `get_storage_paths(&config)?.data_dir.join("uniclipboard-daemon.sock")`，但现有 `AppPaths` 并没有 `data_dir` 字段，只有 `app_data_root`。同时，`AppPaths` 文档已明确要求消费者不要再手写 `.join("...")` 派生路径。按该计划执行会直接编译失败，并继续扩散路径定义源。
SUGGESTION: 先在 `uc-app::AppPaths` 或 `uc-bootstrap` 中引入统一的 daemon socket 路径字段/解析函数，例如 `daemon_socket_path`，然后让 daemon 和 CLI 都复用该单一来源，禁止在 CLI 内部再次手写路径拼接。

FINDING-3: MAJOR
计划无条件在 `uc-cli` 中使用 `tokio::net::UnixStream` 和 Unix socket 路径，但没有任何 `#[cfg(unix)]` 保护。上下文虽然声明 Windows 支持延期，但当前写法会把"延期实现"变成"非 Unix 平台无法编译"，这不是可接受的 defer。
SUGGESTION: 为 Unix RPC 实现加 `cfg(unix)`，并在非 Unix 平台提供明确的"当前平台未支持 daemon RPC"分支；同时把 Cargo/源码结构设计成非 Unix 仍可通过编译。

FINDING-4: MAJOR
验证方案与 must-have 明显不匹配。计划只要求 `cargo check/build`，但成功标准包含"`status` 在 daemon 不可达时返回 5""`--json` 输出合法 JSON""`devices`/`space-status` 在无 daemon 时可工作"。这些都是行为契约，当前计划没有任何自动化断言覆盖。
SUGGESTION: 增加 CLI 集成测试或脚本化验证，至少覆盖：`--help`、`status` 不可达返回码 5、`--json` 输出可被 `serde_json` 解析、`devices`/`space-status` 在 direct mode 下成功执行；并同步补全 `41-VALIDATION.md` 的 per-task verification map。

FINDING-5: MAJOR
计划示例代码违反仓库的生产 Rust 约束：`main.rs` 中明确写了 `.build().expect("Failed to create Tokio runtime")`。该仓库要求生产代码禁止 `unwrap()/expect()`；如果按计划原样执行，会直接引入违反规范的实现。
SUGGESTION: 将入口改成 `fn main() -> anyhow::Result<()>` 或 `run() -> anyhow::Result<i32>`，把 Tokio runtime 创建失败走显式错误传播/映射，并统一由顶层决定退出码。

FINDING-6: MINOR
Task 1 把工作区成员修改、crate 清单创建、参数解析、输出层、RPC 客户端都塞进一个任务里，不符合仓库的原子提交规则，也会让回滚和问题定位变差。
SUGGESTION: 至少拆成两个执行单元：1）crate/workspace/CLI 解析骨架；2）status RPC 客户端与输出契约；3）direct-mode 子命令。每个单元对应单一工程意图和单独验收。

VERDICT: NEEDS_REVISION — the issues above must be addressed
