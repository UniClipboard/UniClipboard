//! Shared local-device query operation.

use crate::error_codes::*;

use tracing::error;
use uc_application::facade::{AppFacade, DeviceFacadeError};

use crate::{EngineError, EngineErrorCategory, LocalDeviceSummary, OperationResult};

pub async fn execute_query_local_device(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let info = facade.device.local_device_info().await.map_err(|error| {
        let variant = match error {
            DeviceFacadeError::DeviceIdentity(_) => "device_identity",
        };
        error!(variant, "query local device failed");
        EngineError::new(
            QUERY_LOCAL_DEVICE_FAILED_CODE,
            EngineErrorCategory::Internal,
            false,
        )
    })?;

    Ok(OperationResult::LocalDevice(LocalDeviceSummary {
        device_id: info.peer_id,
        display_name: info.device_name,
    }))
}
