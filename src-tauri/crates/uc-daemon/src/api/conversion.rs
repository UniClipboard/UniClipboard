//! Projection mappings from app-layer types to daemon transport DTOs.
//!
//! `uc-daemon-contract` must not depend on `uc-app`, so all conversions
//! that bridge the two crates live here via the local `IntoApiDto` trait.
//! Using a local trait (rather than `From`) satisfies the orphan rule.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use uc_app::usecases::clipboard::clear_history::ClearHistoryResult;
use uc_app::usecases::clipboard::get_entry_detail::EntryDetailResult;
use uc_app::usecases::clipboard::get_entry_resource::EntryResourceResult;
use uc_app::usecases::clipboard::list_entry_projections::EntryProjectionDto;
use uc_app::usecases::clipboard::ClipboardStats;
use uc_app::usecases::pairing::LocalDeviceInfo;
use uc_daemon_contract::api::dto::clipboard::{
    ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto, EntryProjectionResponseDto,
    EntryResourceDto,
};
use uc_daemon_contract::api::dto::device::LocalDeviceInfoDto;

/// Local projection trait for converting app-layer types into transport DTOs.
///
/// Using a crate-local trait avoids the orphan rule: neither `From` nor the
/// source/target types belong to this crate, but a trait defined here does.
pub trait IntoApiDto<T> {
    fn into_api_dto(self) -> T;
}

impl IntoApiDto<EntryProjectionResponseDto> for EntryProjectionDto {
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
        }
    }
}

impl IntoApiDto<EntryDetailDto> for EntryDetailResult {
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

impl IntoApiDto<EntryResourceDto> for EntryResourceResult {
    fn into_api_dto(self) -> EntryResourceDto {
        EntryResourceDto {
            blob_id: self.blob_id.map(|id| id.to_string()),
            mime_type: self.mime_type,
            size_bytes: self.size_bytes,
            url: self.url,
            inline_data: self.inline_data.map(|bytes| STANDARD.encode(bytes)),
        }
    }
}

impl IntoApiDto<ClipboardStatsDto> for ClipboardStats {
    fn into_api_dto(self) -> ClipboardStatsDto {
        ClipboardStatsDto {
            total_items: self.total_items,
            total_size: self.total_size,
        }
    }
}

impl IntoApiDto<ClearHistoryResultDto> for ClearHistoryResult {
    fn into_api_dto(self) -> ClearHistoryResultDto {
        ClearHistoryResultDto {
            deleted_count: self.deleted_count,
            failed_entries: self.failed_entries,
        }
    }
}

impl IntoApiDto<LocalDeviceInfoDto> for LocalDeviceInfo {
    fn into_api_dto(self) -> LocalDeviceInfoDto {
        LocalDeviceInfoDto {
            peer_id: self.peer_id,
            device_name: self.device_name,
        }
    }
}
