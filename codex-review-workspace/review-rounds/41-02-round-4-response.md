# 41-02 Round 4 Review Response (Codex)

**Model:** gpt-5.4
**Date:** 2026-03-18

---

FINDING-1: MAJOR + 计划文本在已接受"`run_rpc_server` 已拆分为 bind + `run_rpc_accept_loop`"之后，仍然保留了过期接口名，导致执行者按字面实现会直接写出错误代码。[41-02-PLAN.md L265] 仍写 `use crate::rpc::server::run_rpc_server;`，[41-02-PLAN.md L327] 的注释也仍写 `// In run_rpc_server:`。SUGGESTION: 把 Task 2 里的所有示例、import、注释统一改成 `check_or_remove_stale_socket` + `run_rpc_accept_loop`，不要再出现 `run_rpc_server`。

FINDING-2: MAJOR + 工作线程启动方案仍然不可直接编译。[41-02-PLAN.md L292] 写的是 `tokio::spawn(worker.start(self.cancel.child_token()))`，这会把对 `worker` 的借用带进 `spawn`，不满足 `'static` 要求；前面虽然改成了 `Vec<Arc<dyn DaemonWorker>>`，但这里没有把 `Arc` 真正 clone 进任务。SUGGESTION: 明确要求使用 `let worker = Arc::clone(worker); let token = self.cancel.child_token(); join_set.spawn(async move { worker.start(token).await });` 这一类 `async move` 形式。

FINDING-3: MAJOR + `DaemonApp::run()` 仍然只等待外部关停信号，没有把 RPC accept loop 的提前失败纳入主生命周期控制。[41-02-PLAN.md L293] 先 `wait_for_shutdown_signal().await?`，然后才在第 8 步等待 accept loop；这意味着如果 accept loop 因 `accept()` 错误提前退出，daemon 进程会继续存活但已失去 RPC 能力。SUGGESTION: 把 `wait_for_shutdown_signal()` 与 accept-loop `JoinHandle` 放进同一个 `tokio::select!`，accept loop 异常退出时立即触发整体 shutdown 并向上返回错误；如果要更完整，worker task 早退也应纳入同一监督面。

FINDING-4: MINOR + socket 清理步骤仍然是静默吞错。[41-02-PLAN.md L298] 写的是 `std::fs::remove_file(&self.socket_path).ok()`，这违反了仓库里"不要 silent failure、错误要可观察"的约束。SUGGESTION: 改成显式处理 `NotFound`，其余错误至少 `warn!(error = %e, path = ?self.socket_path, ...)` 记录出来。

VERDICT: NEEDS_REVISION
