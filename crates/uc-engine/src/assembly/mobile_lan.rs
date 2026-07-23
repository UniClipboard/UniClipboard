use std::sync::Arc;

use uc_core::mobile_sync::LanEndpointInfo;
use uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter;

use crate::{EngineError, MobileLanEndpointUpdate, OperationResult};

pub(crate) struct MobileLanEndpointUpdater {
    endpoint: Arc<InMemoryMobileSyncEndpointInfoAdapter>,
}

impl MobileLanEndpointUpdater {
    pub(crate) fn new(endpoint: Arc<InMemoryMobileSyncEndpointInfoAdapter>) -> Self {
        Self { endpoint }
    }

    pub(crate) async fn update(
        &self,
        update: MobileLanEndpointUpdate,
    ) -> Result<OperationResult, EngineError> {
        match update {
            MobileLanEndpointUpdate::Stopped => self.endpoint.clear().await,
            MobileLanEndpointUpdate::Listening { base_url } => {
                self.endpoint.set(LanEndpointInfo { url: base_url }).await;
            }
            MobileLanEndpointUpdate::BindFailed { reason } => {
                self.endpoint.set_bind_failure(reason).await;
            }
        }
        Ok(OperationResult::MobileLanEndpointUpdated)
    }
}
