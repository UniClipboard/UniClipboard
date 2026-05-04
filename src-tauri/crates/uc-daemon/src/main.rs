//! UniClipboard daemon 二进制入口。
//!
//! 这里保留旧命令入口，实际宿主实现委托给 `uc-desktop` 暴露的
//! `uc_desktop::daemon::run`。daemon binary 永远以
//! [`DaemonRunMode::Standalone`] 运行——GUI 内的 daemon 走 in-process
//! 入口（`uc_desktop::daemon::start_in_process(GuiInProcess)`），不会
//! spawn 这个 binary。
//!
//! 历史 `--gui-managed` / `--hybrid` 标志已经随 sidecar 模型一起移除。

use uc_desktop::daemon::run_mode::DaemonRunMode;

fn main() -> anyhow::Result<()> {
    uc_desktop::daemon::run(DaemonRunMode::Standalone)
}
