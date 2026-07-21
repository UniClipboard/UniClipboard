//! Shared current-system-clipboard capture operation.

use tracing::error;
use uc_application::facade::{AppFacade, ClipboardCaptureFacadeError};

use crate::{EngineError, EngineErrorCategory, OperationResult};

pub const CAPTURE_CURRENT_CLIPBOARD_FAILED_CODE: u32 = 1421;

pub async fn execute_capture_current_clipboard(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let entry_id = facade
        .clipboard_capture
        .capture_current()
        .await
        .map_err(map_capture_error)?
        .map(|captured| captured.entry_id);
    Ok(OperationResult::ClipboardCaptured { entry_id })
}

fn map_capture_error(_error: ClipboardCaptureFacadeError) -> EngineError {
    error!("current clipboard capture failed");
    EngineError::new(
        CAPTURE_CURRENT_CLIPBOARD_FAILED_CODE,
        EngineErrorCategory::Internal,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_failure_does_not_expose_internal_details() {
        let error = map_capture_error(ClipboardCaptureFacadeError::Internal(
            "/private/path/clipboard-cache".into(),
        ));

        assert_eq!(error.category(), EngineErrorCategory::Internal);
        assert_eq!(error.code(), CAPTURE_CURRENT_CLIPBOARD_FAILED_CODE);
        assert!(!error.to_string().contains("clipboard-cache"));
    }
}
