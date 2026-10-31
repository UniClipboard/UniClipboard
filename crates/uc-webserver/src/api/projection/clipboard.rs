//! Clipboard boundary projections: history / delivery / command facade views
//! onto the clipboard wire DTOs.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use uc_daemon_contract::api::dto::clipboard_command::{
    DispatchOutcomeResponse, PerTargetOutcomeDto, ResendResponse,
};
use uc_daemon_contract::api::dto::clipboard_delivery::{
    DeliveryFailureReasonDto, EntryDeliveryStatusDto, EntryDeliveryTargetDto, EntryDeliveryViewDto,
    EntrySourceDto,
};
use uc_daemon_contract::api::dto::clipboard_outbound::{
    OutboundProgressItemDto, OutboundProgressResponse, ResendTargetProgressDto,
};
use uc_engine::{
    DeliveryFailureReasonSummary, EntryDeliveryStatusSummary, EntryDeliveryTargetSummary,
    EntryDeliveryViewSummary, EntrySourceSummary, HistoryClearSummary, HistoryEntryDetailSummary,
    HistoryEntryResourceSummary, HistoryEntrySummary, HistoryStatsSummary,
    OutboundProgressItemSummary, OutboundProgressSummary, ResendReportSummary,
    ResendTargetProgressSummary, SendReportSummary, SendTargetOutcome,
};

use super::IntoApiDto;
use crate::api::dto::clipboard::{
    ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto, EntryProjectionResponseDto,
    EntryResourceDto,
};

impl IntoApiDto<EntryProjectionResponseDto> for HistoryEntrySummary {
    fn into_api_dto(self) -> EntryProjectionResponseDto {
        EntryProjectionResponseDto {
            id: self.entry_id,
            preview: self.preview,
            has_detail: self.has_detail,
            size_bytes: self.size_bytes,
            captured_at: self.captured_at_ms,
            content_type: self.content_type,
            is_encrypted: self.is_encrypted,
            is_favorited: self.is_favorited,
            updated_at: self.updated_at_ms,
            active_time: self.active_time_ms,
            file_transfer_status: self.file_transfer_status,
            file_transfer_reason: self.file_transfer_reason,
            content_tags: self.content_tags,
            link_urls: self.link_urls,
            link_domains: self.link_domains,
            file_sizes: self.file_sizes,
            is_directory: self.is_directory,
            payload_state: self.payload_state,
        }
    }
}

impl IntoApiDto<EntryDetailDto> for HistoryEntryDetailSummary {
    fn into_api_dto(self) -> EntryDetailDto {
        EntryDetailDto {
            id: self.entry_id,
            content: self.content,
            size_bytes: self.size_bytes,
            created_at_ms: self.created_at_ms,
            active_time_ms: self.active_time_ms,
            mime_type: self.mime_type,
        }
    }
}

impl IntoApiDto<EntryResourceDto> for HistoryEntryResourceSummary {
    fn into_api_dto(self) -> EntryResourceDto {
        EntryResourceDto {
            blob_id: self.blob_id,
            mime_type: self.mime_type,
            size_bytes: self.size_bytes,
            url: self.url,
            inline_data: self.inline_data.map(|bytes| STANDARD.encode(bytes)),
        }
    }
}

impl IntoApiDto<ClipboardStatsDto> for HistoryStatsSummary {
    fn into_api_dto(self) -> ClipboardStatsDto {
        ClipboardStatsDto {
            total_items: self.total_items,
            total_size: self.total_size,
        }
    }
}

impl IntoApiDto<ClearHistoryResultDto> for HistoryClearSummary {
    fn into_api_dto(self) -> ClearHistoryResultDto {
        ClearHistoryResultDto {
            deleted_count: self.deleted_count,
            failed_entries: self
                .failed_entry_ids
                .into_iter()
                .map(|entry_id| (entry_id, "entry deletion failed".to_string()))
                .collect(),
        }
    }
}

impl IntoApiDto<EntryDeliveryViewDto> for EntryDeliveryViewSummary {
    fn into_api_dto(self) -> EntryDeliveryViewDto {
        EntryDeliveryViewDto {
            entry_id: self.entry_id,
            source: self.source.into_api_dto(),
            deliveries: self
                .deliveries
                .into_iter()
                .map(IntoApiDto::into_api_dto)
                .collect(),
        }
    }
}

impl IntoApiDto<EntrySourceDto> for EntrySourceSummary {
    fn into_api_dto(self) -> EntrySourceDto {
        match self {
            EntrySourceSummary::Local => EntrySourceDto::Local,
            EntrySourceSummary::Remote {
                device_id,
                device_name,
            } => EntrySourceDto::Remote {
                device_id,
                device_name,
            },
            EntrySourceSummary::Historical => EntrySourceDto::Historical,
        }
    }
}

impl IntoApiDto<EntryDeliveryTargetDto> for EntryDeliveryTargetSummary {
    fn into_api_dto(self) -> EntryDeliveryTargetDto {
        EntryDeliveryTargetDto {
            target_device_id: self.target_device_id,
            target_device_name: self.target_device_name,
            status: self.status.into_api_dto(),
            reason_detail: self.reason_detail,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

impl IntoApiDto<EntryDeliveryStatusDto> for EntryDeliveryStatusSummary {
    fn into_api_dto(self) -> EntryDeliveryStatusDto {
        match self {
            EntryDeliveryStatusSummary::Pending => EntryDeliveryStatusDto::Pending,
            EntryDeliveryStatusSummary::Delivered => EntryDeliveryStatusDto::Delivered,
            EntryDeliveryStatusSummary::Duplicate => EntryDeliveryStatusDto::Duplicate,
            EntryDeliveryStatusSummary::Unreachable => EntryDeliveryStatusDto::Unreachable,
            EntryDeliveryStatusSummary::Superseded => EntryDeliveryStatusDto::Superseded,
            EntryDeliveryStatusSummary::Failed { reason } => EntryDeliveryStatusDto::Failed {
                reason: reason.into_api_dto(),
            },
        }
    }
}

impl IntoApiDto<DeliveryFailureReasonDto> for DeliveryFailureReasonSummary {
    fn into_api_dto(self) -> DeliveryFailureReasonDto {
        match self {
            DeliveryFailureReasonSummary::LocalPolicy => DeliveryFailureReasonDto::LocalPolicy,
            DeliveryFailureReasonSummary::PeerRejected => DeliveryFailureReasonDto::PeerRejected,
            DeliveryFailureReasonSummary::PeerIncompatible => {
                DeliveryFailureReasonDto::PeerIncompatible
            }
            DeliveryFailureReasonSummary::Io => DeliveryFailureReasonDto::Io,
            DeliveryFailureReasonSummary::Internal => DeliveryFailureReasonDto::Internal,
        }
    }
}

impl IntoApiDto<DispatchOutcomeResponse> for SendReportSummary {
    fn into_api_dto(self) -> DispatchOutcomeResponse {
        let per_target = self
            .per_target
            .into_iter()
            .map(|t| {
                let (outcome, error) = match t.outcome {
                    SendTargetOutcome::Accepted => ("accepted", None),
                    SendTargetOutcome::Duplicate => ("duplicate", None),
                    SendTargetOutcome::Error { message } => ("error", Some(message)),
                };
                PerTargetOutcomeDto {
                    device_id: t.device_id,
                    outcome: outcome.to_string(),
                    error,
                }
            })
            .collect();

        DispatchOutcomeResponse {
            snapshot_hash: self.snapshot_hash,
            at_ms: self.at_ms,
            total_accepted: self.total_accepted,
            total_duplicate: self.total_duplicate,
            total_offline: self.total_offline,
            total_errored: self.total_errored,
            per_target,
        }
    }
}

impl IntoApiDto<ResendResponse> for ResendReportSummary {
    fn into_api_dto(self) -> ResendResponse {
        ResendResponse {
            accepted: self.accepted,
            duplicate: self.duplicate,
            offline: self.offline,
            errored: self.errored,
            pending: self.pending,
            reconciled: self
                .reconciled
                .into_iter()
                .map(IntoApiDto::into_api_dto)
                .collect(),
        }
    }
}

impl IntoApiDto<OutboundProgressItemDto> for OutboundProgressItemSummary {
    fn into_api_dto(self) -> OutboundProgressItemDto {
        OutboundProgressItemDto {
            kind: self.kind.to_string(),
            index: self.index,
            state: self.state.to_string(),
            last_known_phase: self.last_known_phase.map(|phase| phase.to_string()),
            round: self.round,
            completed_bytes: self.completed_bytes,
            total_bytes: self.total_bytes,
            failure_reason: self.failure_reason.map(|reason| reason.to_string()),
            cancellation_reason: self.cancellation_reason.map(|reason| reason.to_string()),
        }
    }
}

impl IntoApiDto<OutboundProgressResponse> for OutboundProgressSummary {
    fn into_api_dto(self) -> OutboundProgressResponse {
        OutboundProgressResponse {
            entry_id: self.entry_id,
            target_device_id: self.target_device_id,
            attempt_id: self.attempt_id,
            revision: self.revision,
            freshness: self.freshness.to_string(),
            freshness_available: self.freshness_available,
            state: self.state.to_string(),
            items_total: self.items_total,
            items_waiting: self.items_waiting,
            items_running: self.items_running,
            items_completed: self.items_completed,
            items_failed: self.items_failed,
            items_cancelled: self.items_cancelled,
            items_unconfirmed: self.items_unconfirmed,
            items: self
                .items
                .into_iter()
                .map(IntoApiDto::into_api_dto)
                .collect(),
        }
    }
}

impl IntoApiDto<ResendTargetProgressDto> for ResendTargetProgressSummary {
    fn into_api_dto(self) -> ResendTargetProgressDto {
        ResendTargetProgressDto {
            target_device_id: self.target_device_id,
            state: self.state.to_string(),
            current: self.current.map(IntoApiDto::into_api_dto),
            new_transfer_requested: self.new_transfer_requested,
            duplicate_files_possible: self.duplicate_files_possible,
        }
    }
}

#[cfg(test)]
mod tests {
    use uc_daemon_contract::api::dto::clipboard_delivery::{
        DeliveryFailureReasonDto, EntryDeliveryStatusDto,
    };
    use uc_engine::{DeliveryFailureReasonSummary, EntryDeliveryStatusSummary};

    use super::IntoApiDto;

    #[test]
    fn maps_peer_incompatible_delivery_failure_reason() {
        assert!(matches!(
            DeliveryFailureReasonSummary::PeerIncompatible.into_api_dto(),
            DeliveryFailureReasonDto::PeerIncompatible
        ));
    }

    #[test]
    fn maps_superseded_delivery_status() {
        assert!(matches!(
            EntryDeliveryStatusSummary::Superseded.into_api_dto(),
            EntryDeliveryStatusDto::Superseded
        ));
    }
}
