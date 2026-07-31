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
use uc_engine::{
    DeliveryFailureReasonSummary, EntryDeliveryStatusSummary, EntryDeliveryTargetSummary,
    EntryDeliveryViewSummary, EntrySourceSummary, HistoryClearSummary, HistoryEntryDetailSummary,
    HistoryEntryResourceSummary, HistoryEntrySummary, HistoryStatsSummary, ResendReportSummary,
    SendReportSummary, SendTargetOutcome,
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
            thumbnail_url: self.thumbnail_url,
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
            image_width: self.image_width,
            image_height: self.image_height,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use uc_daemon_contract::api::dto::clipboard_delivery::DeliveryFailureReasonDto;
    use uc_engine::DeliveryFailureReasonSummary;

    use super::IntoApiDto;

    #[test]
    fn maps_peer_incompatible_delivery_failure_reason() {
        assert!(matches!(
            DeliveryFailureReasonSummary::PeerIncompatible.into_api_dto(),
            DeliveryFailureReasonDto::PeerIncompatible
        ));
    }
}
