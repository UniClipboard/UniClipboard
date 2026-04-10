use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use uc_core::settings::model::SyncSettings as CoreSyncSettings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalDeviceInfoDto {
    pub peer_id: String,
    pub device_name: String,
}

/// Response wrapper for GET /device/me.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetLocalDeviceInfoResponse {
    pub data: LocalDeviceInfoDto,
    pub ts: i64,
}

// ============================
// Device sync settings DTOs
// ============================

/// Effective sync settings for a paired device (resolved from per-device overrides
/// and global defaults).
///
/// This is the same shape as `SyncSettingsDto` in the settings module.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSyncSettingsDto {
    pub auto_sync: bool,
    pub sync_frequency: SyncFrequencyDto,
    pub content_types: ContentTypesDto,
}

/// Content type toggles for sync filtering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct ContentTypesDto {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

/// Sync frequency mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncFrequencyDto {
    Realtime,
    Interval,
}

/// Partial sync settings for PATCH /device/:peer_id/sync-settings.
///
/// All fields are optional — only provided fields are updated.
/// Content type fields are nested-optional: `{ text: null }` clears that field,
/// `{ text: true }` sets it, and the field can be absent entirely.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSyncSettingsPatchDto {
    pub auto_sync: Option<bool>,
    pub sync_frequency: Option<SyncFrequencyDto>,
    pub content_types: Option<ContentTypesPatchDto>,
}

/// Partial content types for PATCH.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ContentTypesPatchDto {
    pub text: Option<bool>,
    pub image: Option<bool>,
    pub link: Option<bool>,
    pub file: Option<bool>,
    pub code_snippet: Option<bool>,
    pub rich_text: Option<bool>,
}

/// Response wrapper for GET /device/:peer_id/sync-settings.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDeviceSyncSettingsResponse {
    pub data: DeviceSyncSettingsDto,
    pub ts: i64,
}

/// Response wrapper for PATCH /device/:peer_id/sync-settings.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeviceSyncSettingsResponse {
    pub success: bool,
    pub data: DeviceSyncSettingsDto,
    pub ts: i64,
}

// =========================
// From impls
// =========================

impl From<CoreSyncSettings> for DeviceSyncSettingsDto {
    fn from(value: CoreSyncSettings) -> Self {
        Self {
            auto_sync: value.auto_sync,
            sync_frequency: value.sync_frequency.into(),
            content_types: value.content_types.into(),
        }
    }
}

impl From<uc_core::settings::model::ContentTypes> for ContentTypesDto {
    fn from(value: uc_core::settings::model::ContentTypes) -> Self {
        Self {
            text: value.text,
            image: value.image,
            link: value.link,
            file: value.file,
            code_snippet: value.code_snippet,
            rich_text: value.rich_text,
        }
    }
}

impl From<SyncFrequencyDto> for uc_core::settings::model::SyncFrequency {
    fn from(value: SyncFrequencyDto) -> Self {
        match value {
            SyncFrequencyDto::Realtime => Self::Realtime,
            SyncFrequencyDto::Interval => Self::Interval,
        }
    }
}

impl From<uc_core::settings::model::SyncFrequency> for SyncFrequencyDto {
    fn from(value: uc_core::settings::model::SyncFrequency) -> Self {
        match value {
            uc_core::settings::model::SyncFrequency::Realtime => Self::Realtime,
            uc_core::settings::model::SyncFrequency::Interval => Self::Interval,
        }
    }
}
