//! Shared system-clipboard restore operation.

use tracing::error;
use uc_application::facade::{AppFacade, ClipboardRestoreError};

use crate::{
    ClipboardRestoreMode, ClipboardRestoreOutcome, EngineError, EngineErrorCategory,
    OperationResult, RestoreClipboardInput,
};

pub const RESTORE_CLIPBOARD_NOT_FOUND_CODE: u32 = 1431;
pub const RESTORE_CLIPBOARD_UNAVAILABLE_CODE: u32 = 1432;
pub const RESTORE_CLIPBOARD_FAILED_CODE: u32 = 1433;

pub async fn execute_restore_clipboard(
    facade: &AppFacade,
    input: RestoreClipboardInput,
) -> Result<OperationResult, EngineError> {
    let restore = facade.clipboard_restore.as_ref().ok_or_else(|| {
        EngineError::new(
            RESTORE_CLIPBOARD_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        )
    })?;
    let result = match input.mode {
        ClipboardRestoreMode::Standard => restore.restore_entry(&input.entry_id).await,
        ClipboardRestoreMode::PlainText => {
            restore.restore_entry_as_plain_text(&input.entry_id).await
        }
        ClipboardRestoreMode::FilePaths => {
            restore.restore_entry_as_file_paths(&input.entry_id).await
        }
    };

    map_restore_result(result)
}

fn map_restore_result(
    result: Result<(), ClipboardRestoreError>,
) -> Result<OperationResult, EngineError> {
    match result {
        Ok(()) => Ok(OperationResult::ClipboardRestored(
            ClipboardRestoreOutcome::Restored,
        )),
        Err(ClipboardRestoreError::NotFound) => Err(EngineError::new(
            RESTORE_CLIPBOARD_NOT_FOUND_CODE,
            EngineErrorCategory::NotFound,
            false,
        )),
        Err(ClipboardRestoreError::PayloadUnavailable {
            entry_id,
            rep_id,
            state,
        }) => Ok(OperationResult::ClipboardRestored(
            ClipboardRestoreOutcome::PayloadUnavailable {
                entry_id,
                representation_id: rep_id,
                state,
            },
        )),
        Err(ClipboardRestoreError::NotApplicable(reason)) => Ok(
            OperationResult::ClipboardRestored(ClipboardRestoreOutcome::NotApplicable { reason }),
        ),
        Err(ClipboardRestoreError::Internal(_)) => {
            error!("clipboard restore failed");
            Err(EngineError::new(
                RESTORE_CLIPBOARD_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_failures_keep_stable_categories_without_details() {
        let not_found = map_restore_result(Err(ClipboardRestoreError::NotFound)).unwrap_err();
        let internal = map_restore_result(Err(ClipboardRestoreError::Internal(
            "/private/path/clipboard-cache".into(),
        )))
        .unwrap_err();

        assert_eq!(not_found.category(), EngineErrorCategory::NotFound);
        assert_eq!(internal.category(), EngineErrorCategory::Internal);
        assert!(!internal.to_string().contains("clipboard-cache"));
        assert_ne!(not_found.code(), internal.code());
    }

    #[test]
    fn payload_and_not_applicable_failures_become_business_outcomes() {
        let unavailable = map_restore_result(Err(ClipboardRestoreError::PayloadUnavailable {
            entry_id: "entry-1".into(),
            rep_id: "rep-1".into(),
            state: "Lost".into(),
        }))
        .unwrap();
        let not_applicable = map_restore_result(Err(ClipboardRestoreError::NotApplicable(
            "entry has no restorable file paths".into(),
        )))
        .unwrap();

        assert_eq!(
            unavailable,
            OperationResult::ClipboardRestored(ClipboardRestoreOutcome::PayloadUnavailable {
                entry_id: "entry-1".into(),
                representation_id: "rep-1".into(),
                state: "Lost".into(),
            },)
        );
        assert_eq!(
            not_applicable,
            OperationResult::ClipboardRestored(ClipboardRestoreOutcome::NotApplicable {
                reason: "entry has no restorable file paths".into(),
            })
        );
    }
}
