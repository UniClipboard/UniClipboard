use std::sync::Arc;

use uc_application::facade::{SearchCoordinator, SearchCoordinatorDeps};

pub(crate) fn build_search_coordinator(
    deps: &uc_application::deps::AppDeps,
) -> Arc<SearchCoordinator> {
    Arc::new(SearchCoordinator::new(SearchCoordinatorDeps::new(
        deps.search.search_index.clone(),
        deps.search.search_maintenance.clone(),
        deps.search.search_key_derivation.clone(),
        deps.search.search_pipeline.clone(),
        deps.clipboard.entry_ports.list.clone(),
        deps.clipboard.entry_ports.get.clone(),
        deps.clipboard.representation_ports.list_for_event.clone(),
        deps.clipboard.selection_repo.clone(),
        deps.clipboard.clipboard_event_reader_repo.clone(),
        deps.storage.entry_file_set_repo.clone(),
        uc_infra::search::constants::CURRENT_INDEX_VERSION,
    )))
}
