//! daemon 运行控制量。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::{broadcast, Notify};
use uc_webserver::api::types::DaemonWsEvent;

use crate::daemon::run_mode::DaemonRunMode;

const DAEMON_EVENT_CHANNEL_CAPACITY: usize = 64;

/// daemon 启动时创建的共享控制量。
pub struct DaemonRuntimeControls {
    pub event_tx: broadcast::Sender<DaemonWsEvent>,
    pub deferred_ready_notify: Arc<Notify>,
    pub clipboard_capture_gate: Arc<AtomicBool>,
    pub encryption_unlocked: bool,
}

/// 构造 daemon 运行控制量。
pub fn build_daemon_runtime_controls(run_mode: DaemonRunMode) -> DaemonRuntimeControls {
    let (event_tx, _) = broadcast::channel::<DaemonWsEvent>(DAEMON_EVENT_CHANNEL_CAPACITY);

    DaemonRuntimeControls {
        event_tx,
        deferred_ready_notify: Arc::new(Notify::new()),
        clipboard_capture_gate: Arc::new(AtomicBool::new(!run_mode.waits_for_gui_ready())),
        encryption_unlocked: false,
    }
}
