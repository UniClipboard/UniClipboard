# 41-02 Round 2 — Codex Review Response

**Model:** gpt-5.4
**Date:** 2026-03-18

---

FINDING-1: CRITICAL
`DaemonApp::run()` 里虽然写了"先绑定 RPC 再启动 worker"，但当前步骤仍是 `tokio::spawn(run_rpc_server(...))`。这不会等待真实的 `check_or_remove_stale_socket()`/`UnixListener::bind()` 成功完成，绑定失败只会留在后台任务里，worker 仍可能继续启动。也就是说，上一轮已接受的"fail-fast on already-running daemon"修复在这版计划里并未真正落地。
SUGGESTION: 把"绑定 socket"从后台 accept loop 中拆出来，改成先在前台完成 `check_or_remove_stale_socket + UnixListener::bind`，拿到已绑定的 listener 后再启动 worker，并把 listener/serve task 的 `JoinHandle` 保存在 `DaemonApp` 中，关闭时显式等待并传播错误。

FINDING-2: MAJOR
Plan 的 `<interfaces>` 仍然保留了旧版 `RuntimeState` 签名：`new(workers: Vec<Box<dyn DaemonWorker>>)`、`workers()`、`workers_mut()`，这和 41-01 已锁定的"RuntimeState 仅做 snapshot、不拥有 workers、使用 `&[Arc<dyn DaemonWorker>]`"设计直接冲突。执行者如果按接口段实现，会把上一轮已修正的问题重新引回计划。
SUGGESTION: 统一更新 `<interfaces>` 为 41-01 的最终设计：`RuntimeState::new(workers: &[Arc<dyn DaemonWorker>])`，仅保留 `uptime_seconds()` 和 `worker_statuses()`，删除 `workers()`/`workers_mut()` 及任何 `Box<dyn DaemonWorker>` 表述。

FINDING-3: MAJOR
`main.rs` 的执行步骤调用了 `build_daemon_app()`，但示例里除了取 `storage_paths.app_data_root` 外，没有实际使用 `deps/background/watcher_control/platform_*`，`build_non_gui_runtime` 也只是导入未落地。这意味着该计划并没有真正把 daemon 接入 `uc-bootstrap` 产出的非 GUI runtime，只是借了一个 socket 路径；与目标里"initializes via uc-bootstrap"以及关键链路 `main.rs -> build_non_gui_runtime` 不一致。
SUGGESTION: 在计划中明确 `main.rs` 必须实际构建并持有 non-GUI `CoreRuntime`（或明确把所需 bootstrap 依赖注入 `DaemonApp`），并说明这些对象在 daemon 生命周期中的归属；如果本计划暂不接入 runtime，就应同步收缩 objective、key_links 和 success criteria，避免假装已完成 wiring。

VERDICT: NEEDS_REVISION
