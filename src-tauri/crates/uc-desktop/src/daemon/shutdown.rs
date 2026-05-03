//! daemon 关闭信号接入。

use tokio_util::sync::CancellationToken;

/// GUI sidecar 旧模型：父进程会保持 daemon 的 stdin 管道打开。
///
/// 一旦 GUI 正常退出、崩溃或被强制结束，stdin 会关闭；这里 spawn 一个
/// 阻塞线程把 EOF 翻译成 `CancellationToken::cancel()`，复用同一个 token
/// 触发 daemon main loop 的 external shutdown 分支。
///
/// 仅在 [`crate::daemon::run_mode::DaemonRunMode::GuiSidecar`] 模式下使用——
/// 该模式属于 sidecar 进程模型，in-process 化迁移完成后会一起删除。
pub fn spawn_stdin_eof_canceller(token: CancellationToken) {
    std::thread::spawn(move || {
        use std::io::Read;

        let mut buf = [0u8; 1];
        let _ = std::io::stdin().read(&mut buf);
        token.cancel();
    });
}
