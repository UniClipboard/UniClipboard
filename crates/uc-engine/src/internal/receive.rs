//! Shared inbound receive progress and cancellation operations.

use tracing::error;
use uc_application::facade::{
    AppFacade, BlobTransferError, CancelEntryReceiveError,
    CancelEntryReceiveOutcome as AppEntryReceiveCancellationOutcome,
    InboundCancelOutcome as AppInboundTransferCancellationOutcome,
};

use crate::{
    CancelEntryReceiveInput, CancelInboundTransferInput, EngineError, EngineErrorCategory,
    EntryReceiveCancellationOutcome, EntryReceiveProgressInput, InboundTransferCancellationOutcome,
    OperationResult, ReceiveProgressSummary, TransferCancellationReason,
};

pub const RECEIVE_INVALID_INPUT_CODE: u32 = 1411;
pub const RECEIVE_UNAVAILABLE_CODE: u32 = 1412;
pub const RECEIVE_FAILED_CODE: u32 = 1413;
pub const TRANSFER_CANCEL_UNAVAILABLE_CODE: u32 = 1414;
pub const TRANSFER_CANCEL_FAILED_CODE: u32 = 1415;

pub async fn execute_query_entry_receive_progress(
    facade: &AppFacade,
    input: EntryReceiveProgressInput,
) -> Result<OperationResult, EngineError> {
    validate_id(&input.entry_id)?;
    let progress = facade
        .get_entry_receive_progress(&uc_core::ids::EntryId::from_string(input.entry_id))
        .await
        .map_err(map_receive_error)?
        .map(receive_progress);
    Ok(OperationResult::EntryReceiveProgress(progress))
}

pub async fn execute_list_entry_receive_progress(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let progress = facade
        .list_entry_receive_progress()
        .await
        .map_err(map_receive_error)?
        .into_iter()
        .map(receive_progress)
        .collect();
    Ok(OperationResult::EntryReceiveProgressList(progress))
}

pub async fn execute_cancel_entry_receive(
    facade: &AppFacade,
    input: CancelEntryReceiveInput,
) -> Result<OperationResult, EngineError> {
    validate_id(&input.entry_id)?;
    validate_id(&input.attempt_id)?;
    let outcome = facade
        .cancel_entry_receive(
            &uc_core::ids::EntryId::from_string(input.entry_id),
            &input.attempt_id,
        )
        .await
        .map_err(map_receive_error)?;
    Ok(OperationResult::EntryReceiveCancellation(
        entry_receive_cancellation(outcome),
    ))
}

pub async fn execute_cancel_inbound_transfer(
    facade: &AppFacade,
    input: CancelInboundTransferInput,
) -> Result<OperationResult, EngineError> {
    validate_id(&input.transfer_id)?;
    let transfer = facade.blob_transfer.get().cloned().ok_or_else(|| {
        EngineError::new(
            TRANSFER_CANCEL_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        )
    })?;
    let outcome = transfer
        .cancel_inbound_transfer(&input.transfer_id, transfer_reason(input.reason))
        .await
        .map_err(map_transfer_cancel_error)?;
    Ok(OperationResult::InboundTransferCancellation(
        inbound_transfer_cancellation(outcome),
    ))
}

fn receive_progress(progress: uc_core::ports::EntryReceiveProgress) -> ReceiveProgressSummary {
    ReceiveProgressSummary {
        entry_id: progress.entry_id,
        attempt_id: progress.attempt_id,
        state: progress.state.to_string(),
        total_bytes: progress.total_bytes,
        completed_bytes: progress.completed_bytes,
        items_total: progress.items_total,
        items_completed: progress.items_completed,
    }
}

fn entry_receive_cancellation(
    outcome: AppEntryReceiveCancellationOutcome,
) -> EntryReceiveCancellationOutcome {
    match outcome {
        AppEntryReceiveCancellationOutcome::CancellationRequested => {
            EntryReceiveCancellationOutcome::CancellationRequested
        }
        AppEntryReceiveCancellationOutcome::Cancelled => EntryReceiveCancellationOutcome::Cancelled,
        AppEntryReceiveCancellationOutcome::NotReceiving => {
            EntryReceiveCancellationOutcome::NotReceiving
        }
        AppEntryReceiveCancellationOutcome::TooLate => EntryReceiveCancellationOutcome::TooLate,
        AppEntryReceiveCancellationOutcome::AlreadyTerminal => {
            EntryReceiveCancellationOutcome::AlreadyTerminal
        }
        AppEntryReceiveCancellationOutcome::Superseded => {
            EntryReceiveCancellationOutcome::Superseded
        }
    }
}

fn inbound_transfer_cancellation(
    outcome: AppInboundTransferCancellationOutcome,
) -> InboundTransferCancellationOutcome {
    match outcome {
        AppInboundTransferCancellationOutcome::Cancelled => {
            InboundTransferCancellationOutcome::Cancelled
        }
        AppInboundTransferCancellationOutcome::NotInflight => {
            InboundTransferCancellationOutcome::NotInflight
        }
    }
}

fn transfer_reason(reason: TransferCancellationReason) -> uc_core::FileTransferCancellationReason {
    match reason {
        TransferCancellationReason::LocalUser => uc_core::FileTransferCancellationReason::LocalUser,
        TransferCancellationReason::RemotePeer => {
            uc_core::FileTransferCancellationReason::RemotePeer
        }
        TransferCancellationReason::Replaced => uc_core::FileTransferCancellationReason::Replaced,
        TransferCancellationReason::Timeout => uc_core::FileTransferCancellationReason::Timeout,
        TransferCancellationReason::Unknown => uc_core::FileTransferCancellationReason::Unknown,
    }
}

fn validate_id(value: &str) -> Result<(), EngineError> {
    if value.trim().is_empty() {
        return Err(EngineError::new(
            RECEIVE_INVALID_INPUT_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ));
    }
    Ok(())
}

fn map_receive_error(error: CancelEntryReceiveError) -> EngineError {
    match error {
        CancelEntryReceiveError::Unavailable => EngineError::new(
            RECEIVE_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ),
        CancelEntryReceiveError::Attempt(_)
        | CancelEntryReceiveError::PublishLog(_)
        | CancelEntryReceiveError::Transfer(_)
        | CancelEntryReceiveError::Cleanup(_) => {
            error!("entry receive operation failed");
            EngineError::new(RECEIVE_FAILED_CODE, EngineErrorCategory::Internal, false)
        }
    }
}

fn map_transfer_cancel_error(_error: BlobTransferError) -> EngineError {
    error!("inbound transfer cancellation failed");
    EngineError::new(
        TRANSFER_CANCEL_FAILED_CODE,
        EngineErrorCategory::Internal,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_failures_keep_stable_categories_without_details() {
        let unavailable = map_receive_error(CancelEntryReceiveError::Unavailable);
        let internal = map_receive_error(CancelEntryReceiveError::Attempt(
            "/private/path/receive.db".into(),
        ));
        let transfer =
            map_transfer_cancel_error(BlobTransferError::Fetch("/private/path/file.bin".into()));

        assert_eq!(unavailable.category(), EngineErrorCategory::Unavailable);
        assert_eq!(internal.category(), EngineErrorCategory::Internal);
        assert_eq!(transfer.category(), EngineErrorCategory::Internal);
        assert!(!internal.to_string().contains("receive.db"));
        assert!(!transfer.to_string().contains("file.bin"));
    }
}
