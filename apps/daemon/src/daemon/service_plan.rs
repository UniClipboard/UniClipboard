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
    /// 系统剪贴板出站监听。`ServerHeadless` 模式下为 `None`——无 OS 剪贴板，
    /// 既不进 `services` 也不进 `deferred_services`。
    pub clipboard_watcher: Option<Arc<dyn DaemonService>>,
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
        let has_clipboard_watcher = input.clipboard_watcher.is_some();
        let state = Arc::new(RwLock::new(RuntimeState::new(initial_statuses(
            should_defer_clipboard,
            has_clipboard_watcher,
        ))));

        let mut services: Vec<Arc<dyn DaemonService>> = vec![input.file_sync_orchestrator];
        let mut deferred_services: Vec<Arc<dyn DaemonService>> = Vec::new();

        // `clipboard_watcher` 在 `ServerHeadless` 下为 `None` —— 无头节点没有
        // OS 剪贴板可监听,跳过它;inbound sync / search 仍按解锁状态编排。
        if should_defer_clipboard {
            if let Some(watcher) = input.clipboard_watcher {
                deferred_services.push(watcher);
            }
            deferred_services.push(input.inbound_clipboard_sync);
            deferred_services.push(input.search_coordinator);
        } else {
            if let Some(watcher) = input.clipboard_watcher {
                services.push(watcher);
            }
            services.push(input.inbound_clipboard_sync);
            services.push(input.search_coordinator);
        }

        Self {
            state,
            services,
            deferred_services,
        }
    }

    pub fn add_peer_keepalive(&mut self, worker: Arc<dyn DaemonService>) {
        // Keepalive 不依赖加密解锁:它读的 peer_address 表是明文的,iroh dial
        // 用的 device identity 也独立于 master key。锁定期就开始保活,把 iroh
        // magicsock 路径预热好,避免解锁后 ~22s 真空期内复制被判 Offline 丢失。
        self.services.push(worker);
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
    has_clipboard_watcher: bool,
) -> Vec<DaemonServiceSnapshot> {
    let mut statuses = Vec::new();
    // `ServerHeadless` 没有 clipboard-watcher —— 不列进 status,免得 status
    // 输出谎报一个永远不会启动的服务。
    if has_clipboard_watcher {
        statuses.push(DaemonServiceSnapshot {
            name: "clipboard-watcher".to_string(),
            health: deferred_health(should_defer_clipboard),
        });
    }
    statuses.push(DaemonServiceSnapshot {
        name: "inbound-clipboard-sync".to_string(),
        health: deferred_health(should_defer_clipboard),
    });
    statuses.push(DaemonServiceSnapshot {
        name: "file-sync-orchestrator".to_string(),
        health: ServiceHealth::Healthy,
    });
    statuses.push(DaemonServiceSnapshot {
        name: "peer-keepalive".to_string(),
        health: ServiceHealth::Healthy,
    });
    statuses.push(DaemonServiceSnapshot {
        name: "peer-monitor".to_string(),
        health: ServiceHealth::Healthy,
    });
    statuses.push(DaemonServiceSnapshot {
        name: "search-coordinator".to_string(),
        health: deferred_health(should_defer_clipboard),
    });
    statuses
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
    async fn locked_mode_defers_clipboard_services() {
        // ADR-008 P3-3 (B2'-3): the GuiInProcess run-mode (which deferred via
        // `waits_for_gui_ready`) is gone. Deferral now hinges solely on the
        // encryption-locked branch — a locked Standalone daemon defers the same way.
        let mut plan = DaemonServicePlan::build(input(DaemonRunMode::Standalone, false));

        assert_eq!(plan.services.len(), 1);
        assert_eq!(plan.deferred_services.len(), 3);

        // peer-keepalive 不再随解锁状态被延后:它读 peer_address (明文) +
        // iroh dial (用 device identity, 与 master key 解耦),锁定期就该开始
        // 把 magicsock 路径预热好,消除解锁后真空期。
        plan.add_peer_keepalive(service("peer-keepalive"));
        assert_eq!(plan.services.len(), 2);
        assert_eq!(plan.deferred_services.len(), 3);
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
            Some(ServiceHealth::Healthy)
        );
    }

    #[tokio::test]
    async fn server_headless_omits_clipboard_watcher() {
        // 无头 server: clipboard_watcher = None。watcher 既不进 services 也不进
        // deferred,status 列表里也不出现 clipboard-watcher。inbound sync +
        // search 仍照常 —— server 要靠它们收 P2P 入站并建索引。
        let plan = DaemonServicePlan::build(DaemonServicePlanInput {
            run_mode: DaemonRunMode::ServerHeadless,
            encryption_unlocked: true,
            file_sync_orchestrator: service("file-sync-orchestrator"),
            clipboard_watcher: None,
            inbound_clipboard_sync: service("inbound-clipboard-sync"),
            search_coordinator: service("search-coordinator"),
        });

        // file-sync + inbound-sync + search-coordinator = 3,无 watcher。
        assert_eq!(plan.services.len(), 3);
        assert!(plan.deferred_services.is_empty());

        let state = plan.state.read().await;
        assert!(
            health_of(&state, "clipboard-watcher").is_none(),
            "headless server must not list a clipboard-watcher service"
        );
        assert_eq!(
            health_of(&state, "inbound-clipboard-sync"),
            Some(ServiceHealth::Healthy)
        );
    }

    fn input(run_mode: DaemonRunMode, encryption_unlocked: bool) -> DaemonServicePlanInput {
        DaemonServicePlanInput {
            run_mode,
            encryption_unlocked,
            file_sync_orchestrator: service("file-sync-orchestrator"),
            clipboard_watcher: Some(service("clipboard-watcher")),
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
