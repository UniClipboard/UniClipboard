//! daemon 服务清单装配。

use std::sync::Arc;

use crate::daemon::run_mode::DaemonRunMode;
use crate::daemon::runtime_assembly::DaemonRuntimeWorkers;
use crate::daemon::search_assembly::DaemonSearchAssembly;
use crate::daemon::service::DaemonService;
use crate::daemon::service_plan::{DaemonServicePlan, DaemonServicePlanInput};

/// 构造 daemon 服务启动清单。
pub fn build_daemon_service_plan(
    run_mode: DaemonRunMode,
    encryption_unlocked: bool,
    runtime_workers: &DaemonRuntimeWorkers,
    search_assembly: &DaemonSearchAssembly,
) -> DaemonServicePlan {
    DaemonServicePlan::build(DaemonServicePlanInput {
        run_mode,
        encryption_unlocked,
        file_sync_orchestrator: Arc::clone(&runtime_workers.file_sync_orchestrator)
            as Arc<dyn DaemonService>,
        clipboard_watcher: runtime_workers
            .clipboard_watcher
            .as_ref()
            .map(|w| Arc::clone(w) as Arc<dyn DaemonService>),
        inbound_clipboard_sync: Arc::clone(&runtime_workers.inbound_clipboard_sync)
            as Arc<dyn DaemonService>,
        search_coordinator: Arc::clone(&search_assembly.service) as Arc<dyn DaemonService>,
    })
}
