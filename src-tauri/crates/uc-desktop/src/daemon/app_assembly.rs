//! daemon 应用实例装配。

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uc_application::facade::{AppFacade, AppPaths, HostEventEmitterPort};
use uc_daemon_local::process_metadata::DaemonProcessMode;
use uc_webserver::api::types::DaemonWsEvent;

use crate::daemon::app::DaemonApp;
use crate::daemon::service::DaemonService;
use crate::daemon::service_plan::DaemonServicePlan;
use crate::daemon::workers::peer_keepalive::PeerKeepAliveWorker;

/// daemon 应用实例装配输入。
pub struct DaemonAppAssemblyInput {
    pub service_plan: DaemonServicePlan,
    pub app_facade: Arc<AppFacade>,
    pub storage_paths: AppPaths,
    pub emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>,
    pub event_tx: broadcast::Sender<DaemonWsEvent>,
    pub encryption_unlocked: bool,
    pub deferred_ready_notify: Arc<Notify>,
    pub external_shutdown: Option<CancellationToken>,
    pub clipboard_capture_gate: Arc<AtomicBool>,
    pub local_device_id: String,
    /// 见 `DaemonApp::listens_to_os_signals`——
    /// `GuiInProcess` 模式置 false，其他置 true。
    pub listens_to_os_signals: bool,
    /// 写进 PID 文件的进程模式标记。
    /// `GuiInProcess` → `InProcess`；其他 → `Standalone`。
    pub process_mode: DaemonProcessMode,
}

/// 构造 daemon 应用实例。
pub fn build_daemon_app_instance(input: DaemonAppAssemblyInput) -> DaemonApp {
    let DaemonAppAssemblyInput {
        mut service_plan,
        app_facade,
        storage_paths,
        emitter_cell,
        event_tx,
        encryption_unlocked,
        deferred_ready_notify,
        external_shutdown,
        clipboard_capture_gate,
        local_device_id,
        listens_to_os_signals,
        process_mode,
    } = input;

    let peer_keepalive_worker: Arc<dyn DaemonService> =
        Arc::new(PeerKeepAliveWorker::new(Arc::clone(&app_facade)));
    service_plan.add_peer_keepalive(encryption_unlocked, peer_keepalive_worker);
    let deferred_notify = service_plan.deferred_ready_notify(deferred_ready_notify);

    DaemonApp::new_with_deferred(
        service_plan.services,
        Arc::clone(&app_facade),
        storage_paths,
        emitter_cell,
        service_plan.state,
        event_tx,
        encryption_unlocked,
        service_plan.deferred_services,
        deferred_notify,
        external_shutdown,
        Some(clipboard_capture_gate),
        Some(local_device_id),
        listens_to_os_signals,
        process_mode,
    )
}
