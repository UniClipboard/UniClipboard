//! daemon 应用实例装配。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uc_application::facade::{AppFacade, AppPaths, HostEventBus};
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
    pub host_event_bus: Arc<HostEventBus>,
    pub event_tx: broadcast::Sender<DaemonWsEvent>,
    pub encryption_unlocked: bool,
    pub deferred_ready_notify: Arc<Notify>,
    pub external_shutdown: Option<CancellationToken>,
    pub clipboard_capture_gate: Arc<AtomicBool>,
    pub local_device_id: String,
    /// 见 `DaemonApp::listens_to_os_signals`——现存 run-mode 恒为 true
    /// (ADR-008 P3-3 后 daemon 永远是独立进程)。
    pub listens_to_os_signals: bool,
    /// 写进 PID 文件的进程模式标记。现存 run-mode 恒为 `Standalone`;
    /// `InProcess` 仅作 legacy PID 文件读取保留。
    pub process_mode: DaemonProcessMode,
    /// Mobile sync LAN endpoint adapter — daemon listener 启停时通过 inherent
    /// `set` / `clear` 写入,facade 端只读。
    pub mobile_sync_endpoint_info:
        Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
    /// 移动同步 LAN listener 生命周期控制器。daemon `run()` 启动期把 listener
    /// 状态对齐到 settings(`apply(initial_target)`),退出期 `apply(Disabled)`
    /// 兜底回收端口。`update_settings` 路径也用同一个 controller 即时切换 ——
    /// 两条链路单点状态机。
    pub mobile_lan_lifecycle:
        Arc<crate::daemon::mobile_lan_lifecycle::MobileLanLifecycleController>,
    /// Analytics sink — daemon becomes the single authoritative analytics sender
    /// (ADR-008 D20). Wired into `DaemonApiState` for `POST /analytics/capture`.
    pub analytics: Arc<dyn uc_observability::analytics::AnalyticsPort>,
}

/// 构造 daemon 应用实例。
pub fn build_daemon_app_instance(input: DaemonAppAssemblyInput) -> DaemonApp {
    let DaemonAppAssemblyInput {
        mut service_plan,
        app_facade,
        storage_paths,
        host_event_bus,
        event_tx,
        encryption_unlocked,
        deferred_ready_notify,
        external_shutdown,
        clipboard_capture_gate,
        local_device_id,
        listens_to_os_signals,
        process_mode,
        mobile_sync_endpoint_info,
        mobile_lan_lifecycle,
        analytics,
    } = input;

    let peer_keepalive_worker: Arc<dyn DaemonService> =
        Arc::new(PeerKeepAliveWorker::new(Arc::clone(&app_facade)));
    service_plan.add_peer_keepalive(peer_keepalive_worker);
    let deferred_notify = service_plan.deferred_ready_notify(deferred_ready_notify);

    DaemonApp::new_with_deferred(
        service_plan.services,
        Arc::clone(&app_facade),
        storage_paths,
        host_event_bus,
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
    .with_mobile_lan_endpoint_info(mobile_sync_endpoint_info)
    .with_mobile_lan_lifecycle(mobile_lan_lifecycle)
    .with_analytics(analytics)
}
