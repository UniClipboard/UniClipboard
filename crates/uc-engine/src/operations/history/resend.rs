//! Shared clipboard-entry resend operation.

use crate::error_codes::*;

use tracing::error;
use uc_application::facade::{
    AppFacade, NotResendableReason, ResendEntryCommand, ResendEntryError, ResendReport,
};

use crate::{
    EngineError, EngineErrorCategory, EntryNotResendableReason, OperationResult, ResendEntryInput,
    ResendEntryOutcome, ResendReportSummary,
};

pub async fn execute_resend_entry(
    facade: &AppFacade,
    input: ResendEntryInput,
) -> Result<OperationResult, EngineError> {
    let target_filter = (!input.target_devices.is_empty()).then(|| {
        input
            .target_devices
            .into_iter()
            .map(uc_core::ids::DeviceId::new)
            .collect()
    });
    let result = facade
        .resend_entry(ResendEntryCommand {
            entry_id: uc_core::ids::EntryId::from(input.entry_id.as_str()),
            target_filter,
        })
        .await;
    map_resend_result(result)
}

fn map_resend_result(
    result: Result<ResendReport, ResendEntryError>,
) -> Result<OperationResult, EngineError> {
    let outcome = match result {
        Ok(report) => ResendEntryOutcome::Completed(ResendReportSummary {
            accepted: report.accepted,
            duplicate: report.duplicate,
            offline: report.offline,
            errored: report.errored,
            pending: report.pending,
        }),
        Err(ResendEntryError::EntryNotFound(entry_id)) => ResendEntryOutcome::EntryNotFound {
            entry_id: entry_id.as_str().to_string(),
        },
        Err(ResendEntryError::EntryNotResendable { entry_id, reason }) => {
            ResendEntryOutcome::EntryNotResendable {
                entry_id: entry_id.as_str().to_string(),
                reason: match reason {
                    NotResendableReason::RemoteOrigin => EntryNotResendableReason::RemoteOrigin,
                    NotResendableReason::PayloadLost => EntryNotResendableReason::PayloadLost,
                },
            }
        }
        Err(ResendEntryError::TargetNotTrusted(device_id)) => {
            ResendEntryOutcome::TargetNotTrusted {
                device_id: device_id.as_str().to_string(),
            }
        }
        Err(ResendEntryError::NoEligibleTargets) => ResendEntryOutcome::NoEligibleTargets,
        Err(ResendEntryError::Storage(_)) => {
            error!("resend storage operation failed");
            return Err(EngineError::new(
                RESEND_STORAGE_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            ));
        }
        Err(ResendEntryError::Dispatch(_)) => {
            error!("resend dispatch failed");
            return Err(EngineError::new(
                RESEND_DISPATCH_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            ));
        }
    };
    Ok(OperationResult::EntryResent(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::{DeviceId, EntryId};

    #[test]
    fn resend_business_failures_preserve_structured_context() {
        let missing = map_resend_result(Err(ResendEntryError::EntryNotFound(EntryId::from(
            "entry-1",
        ))))
        .unwrap();
        let untrusted = map_resend_result(Err(ResendEntryError::TargetNotTrusted(DeviceId::new(
            "device-1",
        ))))
        .unwrap();

        assert_eq!(
            missing,
            OperationResult::EntryResent(ResendEntryOutcome::EntryNotFound {
                entry_id: "entry-1".into(),
            })
        );
        assert_eq!(
            untrusted,
            OperationResult::EntryResent(ResendEntryOutcome::TargetNotTrusted {
                device_id: "device-1".into(),
            })
        );
    }

    #[test]
    fn resend_report_preserves_every_count() {
        let result = map_resend_result(Ok(ResendReport {
            accepted: 1,
            duplicate: 2,
            offline: 3,
            errored: 4,
            pending: 5,
        }))
        .unwrap();

        assert_eq!(
            result,
            OperationResult::EntryResent(ResendEntryOutcome::Completed(ResendReportSummary {
                accepted: 1,
                duplicate: 2,
                offline: 3,
                errored: 4,
                pending: 5,
            }))
        );
    }

    #[test]
    fn resend_internal_failures_keep_distinct_codes_without_details() {
        let storage = map_resend_result(Err(ResendEntryError::Storage(
            "/private/path/uniclipboard.db".into(),
        )))
        .unwrap_err();
        let dispatch = map_resend_result(Err(ResendEntryError::Dispatch(
            "private session detail".into(),
        )))
        .unwrap_err();

        assert_eq!(storage.code(), RESEND_STORAGE_FAILED_CODE);
        assert_eq!(dispatch.code(), RESEND_DISPATCH_FAILED_CODE);
        assert!(!storage.to_string().contains("uniclipboard.db"));
        assert!(!dispatch.to_string().contains("private session detail"));
    }
}
