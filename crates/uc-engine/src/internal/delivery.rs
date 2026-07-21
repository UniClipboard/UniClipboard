//! Shared clipboard-entry delivery view operation.

use tracing::error;
use uc_application::facade::{
    AppFacade, EntryDeliveryStatusView, EntryDeliveryTargetView, EntryDeliveryView, EntrySource,
    GetEntryDeliveryViewError,
};
use uc_core::clipboard::DeliveryFailureReason;

use crate::{
    DeliveryFailureReasonSummary, EngineError, EngineErrorCategory, EntryDeliveryStatusSummary,
    EntryDeliveryTargetSummary, EntryDeliveryViewSummary, EntrySourceSummary, HistoryEntryInput,
    OperationResult,
};

pub const ENTRY_DELIVERY_NOT_FOUND_CODE: u32 = 1441;
pub const ENTRY_DELIVERY_FAILED_CODE: u32 = 1442;

pub async fn execute_query_entry_delivery(
    facade: &AppFacade,
    input: HistoryEntryInput,
) -> Result<OperationResult, EngineError> {
    let entry_id = uc_core::ids::EntryId::from_string(input.entry_id);
    let view = facade
        .get_entry_delivery_view(&entry_id)
        .await
        .map_err(map_delivery_error)?;
    Ok(OperationResult::EntryDelivery(delivery_view(view)))
}

fn delivery_view(view: EntryDeliveryView) -> EntryDeliveryViewSummary {
    EntryDeliveryViewSummary {
        entry_id: view.entry_id.as_str().to_string(),
        source: entry_source(view.source),
        deliveries: view.deliveries.into_iter().map(delivery_target).collect(),
    }
}

fn entry_source(source: EntrySource) -> EntrySourceSummary {
    match source {
        EntrySource::Local => EntrySourceSummary::Local,
        EntrySource::Remote {
            device_id,
            device_name,
        } => EntrySourceSummary::Remote {
            device_id: device_id.as_str().to_string(),
            device_name,
        },
        EntrySource::Historical => EntrySourceSummary::Historical,
    }
}

fn delivery_target(target: EntryDeliveryTargetView) -> EntryDeliveryTargetSummary {
    EntryDeliveryTargetSummary {
        target_device_id: target.target_device_id.as_str().to_string(),
        target_device_name: target.target_device_name,
        status: delivery_status(target.status),
        reason_detail: target.reason_detail,
        updated_at_ms: target.updated_at_ms,
    }
}

fn delivery_status(status: EntryDeliveryStatusView) -> EntryDeliveryStatusSummary {
    match status {
        EntryDeliveryStatusView::Pending => EntryDeliveryStatusSummary::Pending,
        EntryDeliveryStatusView::Delivered => EntryDeliveryStatusSummary::Delivered,
        EntryDeliveryStatusView::Duplicate => EntryDeliveryStatusSummary::Duplicate,
        EntryDeliveryStatusView::Unreachable => EntryDeliveryStatusSummary::Unreachable,
        EntryDeliveryStatusView::Failed { reason } => EntryDeliveryStatusSummary::Failed {
            reason: delivery_failure_reason(reason),
        },
    }
}

fn delivery_failure_reason(reason: DeliveryFailureReason) -> DeliveryFailureReasonSummary {
    match reason {
        DeliveryFailureReason::LocalPolicy => DeliveryFailureReasonSummary::LocalPolicy,
        DeliveryFailureReason::PeerRejected => DeliveryFailureReasonSummary::PeerRejected,
        DeliveryFailureReason::Io => DeliveryFailureReasonSummary::Io,
        DeliveryFailureReason::Internal => DeliveryFailureReasonSummary::Internal,
    }
}

fn map_delivery_error(error: GetEntryDeliveryViewError) -> EngineError {
    match error {
        GetEntryDeliveryViewError::EntryNotFound(_) => EngineError::new(
            ENTRY_DELIVERY_NOT_FOUND_CODE,
            EngineErrorCategory::NotFound,
            false,
        ),
        GetEntryDeliveryViewError::Storage(_) => {
            error!("entry delivery view failed");
            EngineError::new(
                ENTRY_DELIVERY_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_failures_keep_stable_categories_without_details() {
        let missing =
            map_delivery_error(GetEntryDeliveryViewError::EntryNotFound("entry-1".into()));
        let internal = map_delivery_error(GetEntryDeliveryViewError::Storage(
            "/private/path/uniclipboard.db".into(),
        ));

        assert_eq!(missing.category(), EngineErrorCategory::NotFound);
        assert_eq!(internal.category(), EngineErrorCategory::Internal);
        assert!(!internal.to_string().contains("uniclipboard.db"));
        assert_ne!(missing.code(), internal.code());
    }
}
