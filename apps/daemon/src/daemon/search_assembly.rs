//! daemon 搜索服务装配。

use std::sync::Arc;

use tokio::sync::broadcast;
use uc_application::deps::AppDeps;
use uc_application::facade::{SearchCoordinator, SearchCoordinatorDeps};
use uc_webserver::api::types::DaemonWsEvent;

use crate::daemon::search::coordinator::SearchCoordinatorService;

/// daemon 搜索服务装配结果。
pub struct DaemonSearchAssembly {
    pub coordinator: Arc<SearchCoordinator>,
    pub service: Arc<SearchCoordinatorService>,
}

/// 构造 daemon 搜索协调器和服务。
pub fn build_daemon_search_assembly(
    deps: &AppDeps,
    event_tx: broadcast::Sender<DaemonWsEvent>,
) -> DaemonSearchAssembly {
    let coordinator = Arc::new(SearchCoordinator::new(SearchCoordinatorDeps::new(
        deps.search.search_index.clone(),
        deps.search.search_key_derivation.clone(),
        deps.search.search_pipeline.clone(),
        deps.clipboard.entry_ports.list.clone(),
        deps.clipboard.representation_ports.list_for_event.clone(),
        deps.clipboard.selection_repo.clone(),
        deps.clipboard.clipboard_event_reader_repo.clone(),
    )));

    let service = Arc::new(SearchCoordinatorService::new(
        Arc::clone(&coordinator),
        event_tx,
    ));

    DaemonSearchAssembly {
        coordinator,
        service,
    }
}
