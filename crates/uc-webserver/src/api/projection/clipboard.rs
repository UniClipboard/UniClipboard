//! Clipboard boundary projections: history / delivery / command facade views
//! onto the clipboard wire DTOs.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use uc_application::facade::{
    ClipboardClearHistoryResultView, ClipboardStatsView, DispatchEntryOutcome, EntryDetailView,
    EntryProjectionView, EntryResourceView,
};
use uc_application::facade::{
    EntryDeliveryStatusView, EntryDeliveryTargetView, EntryDeliveryView, EntrySource, ResendReport,
};
use uc_core::clipboard::DeliveryFailureReason;
use uc_core::ports::DispatchAck;
use uc_daemon_contract::api::dto::clipboard_command::{
    DispatchOutcomeResponse, PerTargetOutcomeDto, ResendResponse,
};
use uc_daemon_contract::api::dto::clipboard_delivery::{
    DeliveryFailureReasonDto, EntryDeliveryStatusDto, EntryDeliveryTargetDto, EntryDeliveryViewDto,
    EntrySourceDto,
};

use super::IntoApiDto;
use crate::api::dto::clipboard::{
    ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto, EntryProjectionResponseDto,
    EntryResourceDto,
};

impl IntoApiDto<EntryProjectionResponseDto> for EntryProjectionView {
    fn into_api_dto(self) -> EntryProjectionResponseDto {
        EntryProjectionResponseDto {
            id: self.id,
            preview: self.preview,
            has_detail: self.has_detail,
            size_bytes: self.size_bytes,
            captured_at: self.captured_at,
            content_type: self.content_type,
            thumbnail_url: self.thumbnail_url,
            is_encrypted: self.is_encrypted,
            is_favorited: self.is_favorited,
            updated_at: self.updated_at,
            active_time: self.active_time,
            file_transfer_status: self.file_transfer_status,
            file_transfer_reason: self.file_transfer_reason,
            link_urls: self.link_urls,
            link_domains: self.link_domains,
            file_sizes: self.file_sizes,
            image_width: self.image_width,
            image_height: self.image_height,
            payload_state: self.payload_state,
        }
    }
}

impl IntoApiDto<EntryDetailDto> for EntryDetailView {
    fn into_api_dto(self) -> EntryDetailDto {
        EntryDetailDto {
            id: self.id,
            content: self.content,
            size_bytes: self.size_bytes,
            created_at_ms: self.created_at_ms,
            active_time_ms: self.active_time_ms,
            mime_type: self.mime_type,
        }
    }
}

impl IntoApiDto<EntryResourceDto> for EntryResourceView {
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

impl IntoApiDto<ClipboardStatsDto> for ClipboardStatsView {
    fn into_api_dto(self) -> ClipboardStatsDto {
        ClipboardStatsDto {
            total_items: self.total_items,
            total_size: self.total_size,
        }
    }
}

impl IntoApiDto<ClearHistoryResultDto> for ClipboardClearHistoryResultView {
    fn into_api_dto(self) -> ClearHistoryResultDto {
        ClearHistoryResultDto {
            deleted_count: self.deleted_count,
            failed_entries: self.failed_entries,
        }
    }
}

impl IntoApiDto<EntryDeliveryViewDto> for EntryDeliveryView {
    fn into_api_dto(self) -> EntryDeliveryViewDto {
        EntryDeliveryViewDto {
            entry_id: self.entry_id.as_str().to_string(),
            source: self.source.into_api_dto(),
            deliveries: self
                .deliveries
                .into_iter()
                .map(IntoApiDto::into_api_dto)
                .collect(),
        }
    }
}

impl IntoApiDto<EntrySourceDto> for EntrySource {
    fn into_api_dto(self) -> EntrySourceDto {
        match self {
            EntrySource::Local => EntrySourceDto::Local,
            EntrySource::Remote {
                device_id,
                device_name,
            } => EntrySourceDto::Remote {
                device_id: device_id.as_str().to_string(),
                device_name,
            },
            EntrySource::Historical => EntrySourceDto::Historical,
        }
    }
}

impl IntoApiDto<EntryDeliveryTargetDto> for EntryDeliveryTargetView {
    fn into_api_dto(self) -> EntryDeliveryTargetDto {
        EntryDeliveryTargetDto {
            target_device_id: self.target_device_id.as_str().to_string(),
            target_device_name: self.target_device_name,
            status: self.status.into_api_dto(),
            reason_detail: self.reason_detail,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

impl IntoApiDto<EntryDeliveryStatusDto> for EntryDeliveryStatusView {
    fn into_api_dto(self) -> EntryDeliveryStatusDto {
        match self {
            EntryDeliveryStatusView::Pending => EntryDeliveryStatusDto::Pending,
            EntryDeliveryStatusView::Delivered => EntryDeliveryStatusDto::Delivered,
            EntryDeliveryStatusView::Duplicate => EntryDeliveryStatusDto::Duplicate,
            EntryDeliveryStatusView::Unreachable => EntryDeliveryStatusDto::Unreachable,
            EntryDeliveryStatusView::Failed { reason } => EntryDeliveryStatusDto::Failed {
                reason: reason.into_api_dto(),
            },
        }
    }
}

impl IntoApiDto<DeliveryFailureReasonDto> for DeliveryFailureReason {
    fn into_api_dto(self) -> DeliveryFailureReasonDto {
        match self {
            DeliveryFailureReason::LocalPolicy => DeliveryFailureReasonDto::LocalPolicy,
            DeliveryFailureReason::PeerRejected => DeliveryFailureReasonDto::PeerRejected,
            DeliveryFailureReason::Io => DeliveryFailureReasonDto::Io,
            DeliveryFailureReason::Internal => DeliveryFailureReasonDto::Internal,
        }
    }
}

impl IntoApiDto<DispatchOutcomeResponse> for DispatchEntryOutcome {
    fn into_api_dto(self) -> DispatchOutcomeResponse {
        let per_target = self
            .per_target
            .iter()
            .map(|t| {
                let (outcome, error) = match &t.outcome {
                    Ok(DispatchAck::Accepted) => ("accepted", None),
                    Ok(DispatchAck::DuplicateIgnored) => ("duplicate", None),
                    Err(msg) => ("error", Some(msg.clone())),
                };
                PerTargetOutcomeDto {
                    device_id: t.device_id.as_str().to_string(),
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

impl IntoApiDto<ResendResponse> for ResendReport {
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
