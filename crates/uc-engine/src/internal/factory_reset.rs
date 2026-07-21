//! Shared factory-reset operation.

use tracing::error;
use uc_application::facade::{AppFacade, FactoryResetError};
use uc_application::receive_reconciliation::EnsureReceiveReadyPort;

use crate::{EngineError, EngineErrorCategory, OperationResult};

pub const FACTORY_RESET_UNAVAILABLE_CODE: u32 = 1371;
pub const FACTORY_RESET_KEY_MATERIAL_FAILED_CODE: u32 = 1372;
pub const FACTORY_RESET_STORAGE_FAILED_CODE: u32 = 1373;
pub const FACTORY_RESET_FAILED_CODE: u32 = 1374;

pub async fn execute_factory_reset_space(
    facade: &AppFacade,
    receive_readiness: &dyn EnsureReceiveReadyPort,
) -> Result<OperationResult, EngineError> {
    let setup = facade.space_setup.get().ok_or_else(|| {
        EngineError::new(
            FACTORY_RESET_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        )
    })?;
    setup
        .factory_reset()
        .await
        .map_err(map_factory_reset_error)?;
    receive_readiness.close_receive_gate();

    Ok(OperationResult::SpaceFactoryReset)
}

fn map_factory_reset_error(error: FactoryResetError) -> EngineError {
    let code = match error {
        FactoryResetError::KeyMaterialWipeFailed(_) => FACTORY_RESET_KEY_MATERIAL_FAILED_CODE,
        FactoryResetError::StorageFailed(_) => FACTORY_RESET_STORAGE_FAILED_CODE,
        FactoryResetError::Internal(_) => FACTORY_RESET_FAILED_CODE,
    };
    error!(code, error = %error, "factory reset space failed");
    EngineError::new(code, EngineErrorCategory::Internal, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_reset_failures_keep_distinct_stable_codes() {
        let key_material = map_factory_reset_error(FactoryResetError::KeyMaterialWipeFailed(
            "private detail".into(),
        ));
        let storage =
            map_factory_reset_error(FactoryResetError::StorageFailed("private detail".into()));
        let internal =
            map_factory_reset_error(FactoryResetError::Internal("private detail".into()));

        assert_ne!(key_material.code(), storage.code());
        assert_ne!(storage.code(), internal.code());
        assert_ne!(key_material.code(), internal.code());
    }
}
