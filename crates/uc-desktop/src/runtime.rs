use std::sync::Arc;

use uc_observability::analytics::AnalyticsPort;

use crate::gui_wiring::GuiClientDeps;
use crate::paths::DesktopPaths;
use crate::task_registry::TaskRegistry;

pub struct DesktopRuntime {
    task_registry: Arc<TaskRegistry>,
    analytics: Arc<dyn AnalyticsPort>,
    storage_paths: DesktopPaths,
    device_id: String,
}

impl DesktopRuntime {
    pub fn new(client: GuiClientDeps) -> Self {
        Self {
            task_registry: Arc::new(TaskRegistry::new()),
            analytics: client.analytics,
            storage_paths: client.storage_paths,
            device_id: client.device_id,
        }
    }

    pub fn device_id(&self) -> String {
        self.device_id.clone()
    }

    pub fn analytics(&self) -> Arc<dyn AnalyticsPort> {
        self.analytics.clone()
    }

    pub fn storage_paths(&self) -> &DesktopPaths {
        &self.storage_paths
    }

    pub fn task_registry(&self) -> &Arc<TaskRegistry> {
        &self.task_registry
    }
}
