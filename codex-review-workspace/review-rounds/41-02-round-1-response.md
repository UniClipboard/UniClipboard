# Codex Review: Plan 41-02 — Round 1

**Reviewer model:** gpt-5.4 (via Codex)
**Date:** 2026-03-18
**Session ID:** 019d010f-a363-7fa2-905e-557abac63127
**Tokens used:** 29,768

---

FINDING-1: CRITICAL
计划的启动顺序有根本性问题：`DaemonApp::run()` 先启动 workers，再 `tokio::spawn(run_rpc_server(...))`。而 `run_rpc_server()` 内部才执行 stale socket 检查和 `UnixListener::bind()`。这意味着一旦 socket 已被活跃 daemon 占用，RPC 任务会在后台失败，但 workers 已经启动，进程进入"部分启动成功、对外接口不可用"的错误状态；而且该 `JoinHandle` 未被立即检查，错误可能被延后甚至丢失。
SUGGESTION: 将 socket 检查与 bind 提前为同步启动门槛。先完成 `check_or_remove_stale_socket + bind`，确认 RPC server 已成功监听后，再启动 workers；或者让 `run_rpc_server` 返回"ready/failed"握手结果，`DaemonApp::run()` 必须在继续前显式 await 该结果。

FINDING-2: CRITICAL
worker 生命周期设计在计划内部自相矛盾，当前接口无法按计划安全实现。`RuntimeState` 暴露的是 `Vec<Box<dyn DaemonWorker>>`，`start(&self, ...)` 被说明为需要 `tokio::spawn` 到后台长期运行，但 `spawn` 需要 `'static` future，基于 `&self` 从 `Box`/切片借用出来的 worker 不能直接满足这个约束。计划虽然意识到这个问题，却只写了"Consider making ... Arc"，没有把它升格为明确前置变更。
SUGGESTION: 在执行前明确并固定所有权模型。推荐将 `RuntimeState` 和相关接口统一改为 `Vec<Arc<dyn DaemonWorker>>`，并把该变更显式纳入本计划的修改文件与验收标准；否则改为由 `DaemonApp` 单独持有 workers + task handles，`RuntimeState` 仅保存可序列化状态快照，不直接持有 worker trait objects。

FINDING-3: MAJOR
计划中的 `tokio::spawn(run_rpc_server(&self.socket_path, ...))` 很可能无法编译。`tokio::spawn` 要求 `'static` future，但这里把对 `self.socket_path` 的借用传进了后台任务，生命周期绑定到 `&self`，不满足 `'static`。
SUGGESTION: 让 `run_rpc_server` 接收 `PathBuf` 而不是 `&Path`，在 spawn 前克隆 `self.socket_path.clone()` 并把所有权移动进任务。

FINDING-4: MAJOR
计划与已锁定决策不一致。上下文明确要求 stale socket 判断应通过对现有 socket 发起 `ping` RPC 区分"活跃 daemon"与"残留文件"，但计划仅通过 `UnixStream::connect()` 成功与否判断。这弱化了协议级健康确认，也偏离了本 phase 已记录的技术决策。
SUGGESTION: 将 `check_or_remove_stale_socket` 改为真正发送一次最小 `ping` JSON-RPC 请求，并校验返回 `pong`；只有在 connect/读写/超时/解析失败时，才将其视为 stale socket 并删除。

FINDING-5: MAJOR
计划直接在生产路径中使用 `expect("SIGTERM handler")` 和 `expect("Ctrl-C handler")`，违反仓库明确约束"production code 禁止 `unwrap()/expect()`"。这类信号注册失败虽然少见，但这里属于进程生命周期关键路径，必须可观测并可传播错误。
SUGGESTION: 让 `wait_for_shutdown_signal()` 返回 `anyhow::Result<()>`，对 `signal(...)` 与 `ctrl_c().await` 的失败使用结构化 `tracing::error!` 记录后向上返回。

FINDING-6: MAJOR
计划没有闭合"优雅关闭"的并发面。RPC accept loop 会为每个连接 `tokio::spawn(handle_connection(...))`，但没有保存这些 `JoinHandle`，关闭时也不等待它们完成。结果是进程可能在仍有请求处理中时退出，和"graceful shutdown"目标不一致。
SUGGESTION: 为连接任务建立 `JoinSet`/任务注册表，shutdown 时先停止 accept，再等待在途连接在超时预算内完成；若超时，再记录 `warn!` 后取消退出。

FINDING-7: MAJOR
验证方案不足以证明本计划声称交付的行为。任务级 verify 只有 `cargo check/build`，没有任何自动或手工步骤去验证 `ping/status`、stale socket 清理、SIGTERM/Ctrl-C 退出、socket 删除等关键需求。当前 acceptance criteria 更接近"代码里出现了某些字符串"，不是行为验证。
SUGGESTION: 在本计划内加入至少一项端到端验证：启动 `uniclipboard-daemon`，通过 Unix socket 发送 `ping/status` 请求并断言响应；再增加 SIGTERM 后退出码与 socket 清理验证。即使先用集成测试或脚本测试，也必须把行为验证写入计划。

FINDING-8: MAJOR
计划没有真正解决 non-GUI `CoreRuntime` 装配前置问题，却在验收标准里要求 `build_non_gui_runtime` 或构造 `CoreRuntime`。研究文档已明确指出这依赖一个当前尚不可用的 non-GUI `HostEventEmitterPort` 实现，是本 phase 的关键前置。该计划既未把相关改动纳入文件范围，也未声明依赖其他计划完成，导致执行时很可能卡住。
SUGGESTION: 二选一明确化：要么把 non-GUI emitter/runtime 装配作为本计划的显式前置任务并纳入修改范围；要么从本计划目标中删除 `build_non_gui_runtime/CoreRuntime` 要求，保持 daemon skeleton 只做 RPC/生命周期，不伪装成已完成 bootstrap-runtime 验证。

---

VERDICT: NEEDS_REVISION — the issues above must be addressed
