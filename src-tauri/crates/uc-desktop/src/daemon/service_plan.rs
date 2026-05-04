//! daemon 后台服务启动清单。

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::daemon::service::{DaemonService, ServiceHealth};
use crate::daemon::state::{DaemonServiceSnapshot, RuntimeState};

use super::run_mode::DaemonRunMode;

/// daemon 启动时已经构造好的后台服务。
pub struct DaemonServicePlanInput {
    pub run_mode: DaemonRunMode,
    pub encryption_unlocked: bool,
    pub file_sync_orchestrator: Arc<dyn DaemonService>,
    pub clipboard_watcher: Arc<dyn DaemonService>,
    pub inbound_clipboard_sync: Arc<dyn DaemonService>,
    pub search_coordinator: Arc<dyn DaemonService>,
}

/// daemon 启动阶段的服务分组。
///
/// `services` 会立即启动，`deferred_services` 会等待 ready 信号后启动。
pub struct DaemonServicePlan {
    pub state: Arc<RwLock<RuntimeState>>,
    pub services: Vec<Arc<dyn DaemonService>>,
    pub deferred_services: Vec<Arc<dyn DaemonService>>,
}

impl DaemonServicePlan {
    pub fn build(input: DaemonServicePlanInput) -> Self {
        let should_defer_clipboard =
            input.run_mode.waits_for_gui_ready() || !input.encryption_unlocked;
        let state = Arc::new(RwLock::new(RuntimeState::new(initial_statuses(
            should_defer_clipboard,
            input.encryption_unlocked,
        ))));

        let mut services: Vec<Arc<dyn DaemonService>> = vec![input.file_sync_orchestrator];
        let mut deferred_services: Vec<Arc<dyn DaemonService>> = Vec::new();

        if should_defer_clipboard {
            deferred_services.push(input.clipboard_watcher);
            deferred_services.push(input.inbound_clipboard_sync);
            deferred_services.push(input.search_coordinator);
        } else {
            services.push(input.clipboard_watcher);
            services.push(input.inbound_clipboard_sync);
            services.push(input.search_coordinator);
        }

        Self {
            state,
            services,
            deferred_services,
        }
    }

    pub fn add_peer_keepalive(
        &mut self,
        encryption_unlocked: bool,
        worker: Arc<dyn DaemonService>,
    ) {
        if encryption_unlocked {
            self.services.push(worker);
        } else {
            self.deferred_services.push(worker);
        }
    }

    pub fn deferred_ready_notify(
        &self,
        notify: Arc<tokio::sync::Notify>,
    ) -> Option<Arc<tokio::sync::Notify>> {
        if self.deferred_services.is_empty() {
            None
        } else {
            Some(notify)
        }
    }
}

fn initial_statuses(
    should_defer_clipboard: bool,
    encryption_unlocked: bool,
) -> Vec<DaemonServiceSnapshot> {
    vec![
        DaemonServiceSnapshot {
            name: "clipboard-watcher".to_string(),
            health: deferred_health(should_defer_clipboard),
        },
        DaemonServiceSnapshot {
            name: "inbound-clipboard-sync".to_string(),
            health: deferred_health(should_defer_clipboard),
        },
        DaemonServiceSnapshot {
            name: "file-sync-orchestrator".to_string(),
            health: ServiceHealth::Healthy,
        },
        DaemonServiceSnapshot {
            name: "peer-keepalive".to_string(),
            health: deferred_health(!encryption_unlocked),
        },
        DaemonServiceSnapshot {
            name: "peer-monitor".to_string(),
            health: ServiceHealth::Healthy,
        },
        DaemonServiceSnapshot {
            name: "search-coordinator".to_string(),
            health: deferred_health(should_defer_clipboard),
        },
    ]
}

fn deferred_health(deferred: bool) -> ServiceHealth {
    if deferred {
        ServiceHealth::Stopped
    } else {
        ServiceHealth::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tokio_util::sync::CancellationToken;

    struct NoopService(&'static str);

    #[async_trait]
    impl DaemonService for NoopService {
        fn name(&self) -> &str {
            self.0
        }

        async fn start(&self, _cancel: CancellationToken) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn health_check(&self) -> ServiceHealth {
            ServiceHealth::Healthy
        }
    }

    #[tokio::test]
    async fn unlocked_standalone_mode_starts_clipboard_services_immediately() {
        let plan = DaemonServicePlan::build(input(DaemonRunMode::Standalone, true));

        assert_eq!(plan.services.len(), 4);
        assert!(plan.deferred_services.is_empty());

        let state = plan.state.read().await;
        assert_eq!(
            health_of(&state, "clipboard-watcher"),
            Some(ServiceHealth::Healthy)
        );
        assert_eq!(
            health_of(&state, "search-coordinator"),
            Some(ServiceHealth::Healthy)
        );
        assert_eq!(
            health_of(&state, "peer-keepalive"),
            Some(ServiceHealth::Healthy)
        );
    }

    #[tokio::test]
    async fn gui_in_process_or_locked_mode_defers_clipboard_services() {
        let mut plan = DaemonServicePlan::build(input(DaemonRunMode::GuiInProcess, false));

        assert_eq!(plan.services.len(), 1);
        assert_eq!(plan.deferred_services.len(), 3);

        plan.add_peer_keepalive(false, service("peer-keepalive"));
        assert_eq!(plan.deferred_services.len(), 4);
        assert!(plan
            .deferred_ready_notify(Arc::new(tokio::sync::Notify::new()))
            .is_some());

        let state = plan.state.read().await;
        assert_eq!(
            health_of(&state, "clipboard-watcher"),
            Some(ServiceHealth::Stopped)
        );
        assert_eq!(
            health_of(&state, "search-coordinator"),
            Some(ServiceHealth::Stopped)
        );
        assert_eq!(
            health_of(&state, "peer-keepalive"),
            Some(ServiceHealth::Stopped)
        );
    }

    fn input(run_mode: DaemonRunMode, encryption_unlocked: bool) -> DaemonServicePlanInput {
        DaemonServicePlanInput {
            run_mode,
            encryption_unlocked,
            file_sync_orchestrator: service("file-sync-orchestrator"),
            clipboard_watcher: service("clipboard-watcher"),
            inbound_clipboard_sync: service("inbound-clipboard-sync"),
            search_coordinator: service("search-coordinator"),
        }
    }

    fn service(name: &'static str) -> Arc<dyn DaemonService> {
        Arc::new(NoopService(name))
    }

    fn health_of(state: &RuntimeState, name: &str) -> Option<ServiceHealth> {
        state
            .worker_statuses()
            .iter()
            .find(|service| service.name == name)
            .map(|service| service.health.clone())
    }
}
